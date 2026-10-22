//! Ranking (plans/m13-stream-choice.md 3): hard gates, a lexicographic key
//! of plain questions, a bounded score, a line for the user, dedupe with
//! failover chains, intent groups and reasons.

use crate::model::*;
use mq_protocol::Source;
use mq_relparse::{Audio, Hdr, Quality, VideoCodec};
use std::collections::HashMap;

pub struct RankInput<'a> {
    pub caps: &'a DeviceCaps,
    pub prefs: &'a StreamPrefs,
    pub target: &'a Target,
    /// Binge group of the stream used for the previous episode, if any.
    pub last_binge_group: Option<&'a str>,
    /// A kids profile's rules (plans/m11-kids.md 4.4): the names of the
    /// allowed title, one of which a release's title must equal; hand-offs
    /// and adult terms hide. `None` for an open profile.
    pub kids: Option<&'a [String]>,
    pub conditions: &'a Conditions,
}

const MAX_REASONS: usize = 3;
/// A window that holds less than this much of a file ahead hides it (3.1).
const WINDOW_AHEAD_S: u64 = 120;
/// Torrents under this many seeders may not start at all.
const FEW_SEEDERS: u32 = 5;
/// Seeders count in the score up to this many, then the file decides. The
/// owner's rule is as many seeders as possible, at least 50 (question 5).
const SATURATE_SEEDERS: f32 = 1000.0;
/// Share of the measured speed a file may need (Jellyfin's factor): the
/// speed must be at least 1.43 times the bitrate. A pick cannot step down
/// mid-film, so this is the most conservative share players use
/// (research R4: keep it).
const SAFE_SHARE: f32 = 0.7;

/// What the ranking derives for one stream that passed the gates.
struct Scored {
    stream: ParsedStream,
    key: RankKey,
    class: SourceClass,
    line: ReasonLine,
    bitrate: Option<f32>,
}

pub fn rank(streams: Vec<ParsedStream>, input: &RankInput<'_>) -> Ranked {
    let ranked = rank_within(streams, input);
    // The size limit hides only while something else is left: over-limit
    // rows come back when it would leave nothing (play-every-version 2d).
    if !ranked.rows.is_empty()
        || !ranked
            .hidden
            .iter()
            .any(|h| h.reason == HideReason::TooLarge)
    {
        return ranked;
    }
    let prefs = StreamPrefs {
        max_size_bytes: None,
        ..input.prefs.clone()
    };
    let streams = ranked.hidden.into_iter().map(|h| h.stream).collect();
    rank_within(
        streams,
        &RankInput {
            prefs: &prefs,
            ..*input
        },
    )
}

fn rank_within(streams: Vec<ParsedStream>, input: &RankInput<'_>) -> Ranked {
    let mut scored = Vec::new();
    let mut hidden = Vec::new();
    for stream in streams {
        match gate(&stream, input) {
            Err(reason) => hidden.push(HiddenRow { stream, reason }),
            Ok(class) => scored.push(score(stream, class, input)),
        }
    }
    // Higher key first; ties keep addon order then response order.
    scored.sort_by(|a, b| {
        b.key
            .cmp(&a.key)
            .then(a.stream.addon_rank.cmp(&b.stream.addon_rank))
            .then(a.stream.index.cmp(&b.stream.index))
    });
    let rows = dedupe(scored, input);
    Ranked { rows, hidden }
}

/// A torrent's info hash, lowercase.
fn info_hash(s: &ParsedStream) -> Option<String> {
    match &s.source {
        Source::Torrent { info_hash, .. } => Some(info_hash.to_ascii_lowercase()),
        Source::ClientResolve(r) => r.info_hash.as_ref().map(|h| h.to_ascii_lowercase()),
        _ => None,
    }
}

/// The class a stream would play as, or why it is hidden.
fn gate(s: &ParsedStream, input: &RankInput<'_>) -> Result<SourceClass, HideReason> {
    if let Some(reason) = gate_reason(s, input) {
        return Err(reason);
    }
    let class = class(s, input.conditions).ok_or(HideReason::NeedsDebrid)?;
    // The engine fetches one episode of a pack, so the pack guard's size
    // is what must fit (code review r2 P2-4).
    if class == SourceClass::Swarm
        && let (Some(free), Some(size)) = (input.conditions.free_bytes, file_size(s, input))
        && room(s, input, free, size) == Room::None
    {
        return Err(HideReason::NoSpace);
    }
    Ok(class)
}

/// Where a swarm row plays (plans/streaming-window.md 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Room {
    /// Whole: it fits in `free + R - C - reserve` (the engine's 512 MiB).
    Whole,
    /// In a window of `budget` bytes holding `ahead_s` of it ahead.
    Window { ahead_s: u64, budget: u64 },
    /// Nowhere: no windows here, or under 120 s ahead.
    None,
}

/// A swarm row's window, when it plays in one.
fn window_of(s: &ParsedStream, class: SourceClass, input: &RankInput<'_>) -> Option<Room> {
    if class != SourceClass::Swarm {
        return None;
    }
    let room = room(s, input, input.conditions.free_bytes?, file_size(s, input)?);
    matches!(room, Room::Window { .. }).then_some(room)
}

/// The row lists a whole pack's size for one episode of it: the release is
/// a season pack, several episodes or a complete pack, and the addon named
/// no file (a file name or `videoSize` is the file the row plays, whatever
/// its title says, review r2 B P2-1).
fn pack(s: &ParsedStream, input: &RankInput<'_>) -> bool {
    let r = &s.release;
    let episode = input.target.season.is_some() || input.target.episode.is_some();
    episode
        && !s.size_is_file
        && s.filename.is_none()
        && (r.complete || r.episodes.len() >= 2 || (r.episodes.is_empty() && !r.seasons.is_empty()))
}

/// The bytes of the file a row plays, which its room is judged by (3.1). A
/// pack row lists the pack: the file is one episode's share, by the
/// episodes it names, else by the season's count; unknown otherwise (the
/// app then decides at open). Other rows weigh as the pack guard says.
fn file_size(s: &ParsedStream, input: &RankInput<'_>) -> Option<u64> {
    if !pack(s, input) {
        return weight(s, input).size;
    }
    let named = s.release.episodes.len() as u64;
    let episodes = if named >= 2 {
        Some(named)
    } else {
        input
            .conditions
            .episodes_in_season
            .filter(|n| *n >= 2)
            .map(u64::from)
    };
    Some(s.size_bytes? / episodes?)
}

fn room(s: &ParsedStream, input: &RankInput<'_>, free: u64, size: u64) -> Room {
    let c = input.conditions;
    let episode = input.target.season.is_some() || input.target.episode.is_some();
    let file = match &s.source {
        Source::Torrent { file_idx, .. } => file_idx.map(|i| i as usize),
        _ => None,
    };
    let held = info_hash(s).and_then(|h| c.held.get(&h));
    let mut i = mq_engine_sched::WindowInputs {
        free,
        reclaimable: c.reclaimable_bytes.unwrap_or(0),
        committed: c.committed_bytes,
        bitrate: 0,
        piece_len: estimated_piece(size),
        fill: (f64::from(c.p2p.mbps) * 125_000.0) as u64,
        ..Default::default()
    }
    .with_held(held, file, estimated_piece(size));
    if i.fits_whole(size) {
        return Room::Whole;
    }
    if !c.window {
        return Room::None;
    }
    // The rate the app plays it with: the file over the runtime when known
    // (a pack's episode over the episode's), else the guess (2.2).
    let runtime = if pack(s, input) {
        c.runtime_s
    } else {
        weight(s, input).mbps.and(c.runtime_s)
    };
    let runtime = runtime.filter(|r| *r > 0).map(u64::from);
    i.bitrate = window_rate(s, size, episode, runtime);
    let Some(p) = mq_engine_sched::window_plan(&mq_engine_sched::Config::standard(), i) else {
        return Room::None;
    };
    // The engine plans at the guess when the runtime is unknown, and a
    // remux can stream faster than its guess: the seconds ahead are then
    // counted at no less than the file over the shortest runtime of its
    // kind, so the line never promises what a faster file would not hold
    // (review r1 B P2-2).
    let rate = match runtime {
        Some(_) => i.bitrate,
        None => i.bitrate.max(
            size / if episode {
                SHORTEST_EPISODE_S
            } else {
                SHORTEST_FILM_S
            },
        ),
    };
    match p.ahead_secs(rate) {
        ahead_s if ahead_s >= WINDOW_AHEAD_S => Room::Window {
            ahead_s,
            budget: p.budget,
        },
        _ => Room::None,
    }
}

/// The shortest runtimes a file with no runtime is counted at: a feature
/// film and an episode.
const SHORTEST_FILM_S: u64 = 75 * 60;
const SHORTEST_EPISODE_S: u64 = 20 * 60;

#[cfg(test)]
mod tests {
    /// Phase 3 test k (plans/streaming-window.md 3.1): 1 MiB for 571 MB,
    /// 2 MiB for 2.5 GiB, 8 MiB for 15 and 40 GiB, 16 MiB for 60 GiB.
    #[test]
    fn the_piece_estimate_is_four_times_libtorrents_at_most_16_mib() {
        const MIB: u64 = 1 << 20;
        const GIB: u64 = 1 << 30;
        for (size, piece) in [
            (571_346_576, MIB),
            (5 * GIB / 2, 2 * MIB),
            (15 * GIB, 8 * MIB),
            (40 * GIB, 8 * MIB),
            (60 * GIB, 16 * MIB),
        ] {
            assert_eq!(super::estimated_piece(size), piece, "{size}");
        }
    }
}

/// The piece size of a torrent not opened yet (3.1): 4 times libtorrent's
/// automatic size for the file (256 KiB up to 687 MB, doubling every 4
/// times the size), at most 16 MiB.
fn estimated_piece(size: u64) -> u64 {
    let (mut piece, mut upto) = (256u64 << 10, 687_000_000u64);
    while size > upto && piece < 4 << 20 {
        piece *= 2;
        upto = upto.saturating_mul(4);
    }
    4 * piece
}

/// A torrent file's rate in bytes per second before the player reports its
/// duration (plans/streaming-window.md 2.2): the larger of a rate for its
/// resolution and quality (720p and under 5 Mbit/s, 1080p 10, a 1080p remux
/// 30, 2160p 40, a 2160p remux 60) and its size over 3 h for a film, 1 h for
/// an episode.
pub fn guessed_rate(stream: &ParsedStream, len: u64, episode: bool) -> u64 {
    let r = &stream.release;
    let remux = r.quality == Quality::Remux;
    let mbps: u64 = match (r.resolution.height(), remux) {
        (h, true) if h >= 2160 => 60,
        (h, false) if h >= 2160 => 40,
        (h, true) if h >= 1080 => 30,
        (h, false) if h >= 1080 => 10,
        _ => 5,
    };
    let by_size = len / if episode { 3600 } else { 3 * 3600 };
    (mbps * 1_000_000 / 8).max(by_size)
}

/// A torrent file's rate for its window (2.2), the one the ranking and the
/// app both size with: `len` over the duration when one is known (the
/// player's last, else the meta runtime), else [`guessed_rate`]; never
/// under `len` over 3 h (1 h for an episode).
pub fn window_rate(stream: &ParsedStream, len: u64, episode: bool, duration_s: Option<u64>) -> u64 {
    let floor = len / if episode { 3600 } else { 3 * 3600 };
    match duration_s.filter(|&d| d > 0) {
        Some(d) => (len / d).max(floor),
        None => guessed_rate(stream, len, episode),
    }
}

/// How a stream starts (3.2), or `None` when nothing here can open it.
fn class(s: &ParsedStream, c: &Conditions) -> Option<SourceClass> {
    let hash = match &s.source {
        Source::Torrent { .. } => true,
        Source::ClientResolve(r) => r.info_hash.is_some(),
        // A link a service has not prepared yet starts a download first.
        Source::Http { .. } if s.cached == Some(false) => return Some(SourceClass::Fetch),
        _ => return Some(SourceClass::Ready),
    };
    let resolve = matches!(s.source, Source::ClientResolve(_));
    if c.debrid {
        return Some(match s.cached {
            Some(true) => SourceClass::Ready,
            None => SourceClass::Unknown,
            Some(false) if c.engine && hash => SourceClass::Swarm,
            Some(false) => SourceClass::Fetch,
        });
    }
    // Without an account only the engine opens a torrent; a row only a
    // service resolves cannot open.
    (c.engine && hash && !resolve).then_some(SourceClass::Swarm)
}

fn gate_reason(s: &ParsedStream, input: &RankInput<'_>) -> Option<HideReason> {
    if matches!(
        s.source,
        Source::Usenet { .. } | Source::Archive { .. } | Source::Invalid { .. }
    ) {
        return Some(HideReason::UnsupportedSource);
    }
    if let Some(names) = input.kids
        && let Some(reason) = kids_gate(s, names)
    {
        return Some(reason);
    }
    // Playback opens none of these (`open_source`): a trailer or a link
    // out never tops the list (code review r2 P1-1).
    if matches!(
        s.source,
        Source::YouTube { .. } | Source::PlayerFrame { .. } | Source::External { .. }
    ) {
        return Some(HideReason::UnsupportedSource);
    }
    let text = format!("{} {}", s.raw_name, s.raw_description).to_lowercase();
    if input
        .prefs
        .avoid
        .iter()
        .any(|t| !t.is_empty() && text.contains(t.as_str()))
    {
        return Some(HideReason::Excluded);
    }
    if let (Some(max), Some(size)) = (input.prefs.max_size_bytes, s.size_bytes)
        && size > max
    {
        return Some(HideReason::TooLarge);
    }
    if wrong_episode(s, input.target) {
        return Some(HideReason::WrongEpisode);
    }
    if input.kids.is_none() && wrong_title(s, input.target, input.conditions) {
        return Some(HideReason::WrongTitle);
    }
    if playability(s, input).is_none() {
        return Some(HideReason::Unplayable);
    }
    None
}

fn wrong_episode(s: &ParsedStream, target: &Target) -> bool {
    let r = &s.release;
    if let Some(season) = target.season
        && !r.seasons.is_empty()
        && !r.seasons.contains(&season)
    {
        return true;
    }
    if let Some(episode) = target.episode
        && !r.episodes.is_empty()
        && !r.episodes.contains(&episode)
        && !r.complete
    {
        return true;
    }
    false
}

/// Only rejects when the parsed title is clearly another work; short or
/// missing titles never hide a stream.
/// Only rejects when the parsed title is clearly another work; short or
/// missing titles never hide a stream. A release may name the work in
/// several languages, split by " / " in its title line ("Игра престолов /
/// Game of Thrones / Сезон: 1") while its file name carries one: any name
/// matching is enough.
fn wrong_title(s: &ParsedStream, target: &Target, c: &Conditions) -> bool {
    // A release named only in the original language ("Kis Uykusu (2014)"
    // for Winter Sleep) that names the title's own year is kept (code
    // review r2 P3).
    // ponytail: the year alone vouches for the name; another work of the
    // same year an addon lists for this id stays visible. Alternative
    // titles from the meta would close that, if a recording shows one.
    if s.release
        .year
        .is_some_and(|y| [c.title_year, c.episode_year].contains(&Some(y)))
    {
        return false;
    }
    let parsed = normalize_title(&s.release.title);
    let wanted = normalize_title(&target.title);
    if parsed.len() < 3 || wanted.len() < 3 {
        return false;
    }
    let line = s.raw_description.lines().next().unwrap_or_default();
    let others = line.contains(" / ").then(|| line.split(" / "));
    // A pack's file is often named for its episode alone ("S01E01- Winter
    // Is Coming.mkv") or a hash, while the torrent's own name, the title
    // line, names the show (pick audit: 8 right rows over 4 of 6 shows).
    let torrent = std::iter::once_with(|| normalize_title(&mq_relparse::parse(line).title));
    std::iter::once(parsed)
        .chain(others.into_iter().flatten().map(normalize_title))
        .chain(torrent)
        .filter(|n| n.len() >= 3)
        .all(|n| title_similarity(&n, &wanted) < 0.5)
}

fn normalize_title(t: &str) -> String {
    t.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| !matches!(*w, "the" | "a" | "an" | "and" | "of"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A kids profile's gate (plans/m11-kids.md 4.4): a source that leaves the
/// app, a release naming an adult term, and a release whose title is not
/// exactly one of the allowed title's names are hidden.
fn kids_gate(s: &ParsedStream, names: &[String]) -> Option<HideReason> {
    if matches!(
        s.source,
        Source::YouTube { .. } | Source::PlayerFrame { .. } | Source::External { .. }
    ) {
        return Some(HideReason::LeavesApp);
    }
    let filename = s.filename.as_deref().unwrap_or_default();
    if [s.raw_name.as_str(), &s.raw_description, filename]
        .iter()
        .any(|text| tokens(text).any(|t| ADULT_TERMS.contains(&t.as_str())))
    {
        return Some(HideReason::NotForKids);
    }
    let parsed = kids_title(&s.release.title);
    if parsed.is_empty() || !names.iter().any(|n| kids_title(n) == parsed) {
        return Some(HideReason::WrongTitle);
    }
    None
}

/// Lowercase words, split on anything that is not a letter or a digit:
/// terms match whole words only ("Essex" is no adult term).
fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// Terms that hide a release from a kid, as whole words, English only.
/// Best effort: a mislabelled file still passes (the trust boundary of 4.4).
const ADULT_TERMS: &[&str] = &[
    "anal",
    "bangbros",
    "bdsm",
    "blowjob",
    "boobs",
    "brazzers",
    "camgirl",
    "cumshot",
    "ecchi",
    "erotic",
    "erotica",
    "fetish",
    "gangbang",
    "handjob",
    "hardcore",
    "hentai",
    "incest",
    "jav",
    "milf",
    "nsfw",
    "nude",
    "nudes",
    "nudity",
    "onlyfans",
    "orgy",
    "playboy",
    "porn",
    "porno",
    "pornhub",
    "pussy",
    "sex",
    "sextape",
    "slut",
    "softcore",
    "stripper",
    "threesome",
    "tits",
    "xxx",
];

/// A title as the kids rule compares it: `normalize_title` with
/// apostrophes dropped first ("Philosopher's" is "Philosophers") and the
/// roman numerals I to XX read as numbers ("Frozen II" is "Frozen 2").
fn kids_title(t: &str) -> String {
    let joined: String = t
        .chars()
        .filter(|c| !matches!(c, '\'' | '\u{2019}'))
        .collect();
    normalize_title(&joined)
        .split(' ')
        .map(|w| roman(w).map_or_else(|| w.to_owned(), |n| n.to_string()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn roman(word: &str) -> Option<u8> {
    const NUMERALS: [&str; 20] = [
        "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv",
        "xv", "xvi", "xvii", "xviii", "xix", "xx",
    ];
    NUMERALS
        .iter()
        .position(|n| *n == word)
        .map(|i| i as u8 + 1)
}

/// Token overlap (Sorensen-Dice on words), robust to translated subtitles
/// and extra tags in torrent names: the shorter title must be mostly covered.
pub fn title_similarity(a: &str, b: &str) -> f32 {
    let ta: Vec<&str> = a.split_whitespace().collect();
    let tb: Vec<&str> = b.split_whitespace().collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let common = ta.iter().filter(|w| tb.contains(w)).count();
    common as f32 / ta.len().min(tb.len()) as f32
}

fn playability(s: &ParsedStream, input: &RankInput<'_>) -> Option<Playability> {
    let (r, caps) = (&s.release, input.caps);
    let codec_ok = match r.codec {
        VideoCodec::Unknown => true,
        VideoCodec::H265 if r.bit_depth == Some(10) => {
            caps.ten_bit_hevc && caps.decoders.contains(&VideoCodec::H265)
        }
        codec => caps.decoders.contains(&codec),
    };
    let hdr_ok = r.hdr.iter().all(|h| shows(*h, caps));
    // Atmos rides on TrueHD or E-AC-3, so it says nothing about decoding.
    let mut audio = r.audio.iter().filter(|a| **a != Audio::Atmos).peekable();
    let audio_ok = audio.peek().is_none()
        || audio.any(|a| caps.audio_decoders.contains(a) || caps.passthrough.contains(a));
    let container = opened_as(s, input).or(r.container.as_deref());
    let container_ok = container.is_none_or(|c| caps.opens(c));
    // A device that opens mkv opens what releases come in; one that does
    // not may not open an unstated container.
    let unstated = container.is_none() && !caps.opens("mkv");
    match (codec_ok && audio_ok && container_ok, hdr_ok) {
        (true, true) if unstated => Some(Playability::Unstated),
        (true, true) => Some(Playability::Native),
        _ if caps.has_fallback_engine => Some(Playability::NeedsFallbackEngine),
        (false, _) => None,
        (true, false) => Some(Playability::NeedsTranscode),
    }
}

/// The screen class a display height counts as (3.3): a 1179-line iPhone
/// and a 1440-line phone are 1080.
pub fn screen_class(display_height: u32) -> u32 {
    match display_height {
        h if h >= 2000 => 2160,
        h if h >= 1000 => 1080,
        h if h >= 700 => 720,
        _ => 480,
    }
}

/// The resolution steps a release moves down: 4320, 2160, 1080, 720, 480,
/// 360, 240.
const LADDER: [u32; 7] = [4320, 2160, 1080, 720, 480, 360, 240];

fn step_down(height: u32) -> u32 {
    LADDER.iter().copied().find(|&h| h < height).unwrap_or(0)
}

/// Lowest bitrate a real release of that height has (3.3): under it the
/// file is upscaled or starved. TRaSH's WEB-DL 2160p minimum; VortX's
/// 1080p junk floor.
fn floor_mbps(height: u32) -> Option<f32> {
    match height {
        h if h >= 2160 => Some(4.6),
        1080 => Some(0.7),
        _ => None,
    }
}

/// A disc's maximum for that height (3.3): above it a size is a pack's.
fn ceiling_mbps(height: u32) -> f32 {
    if height > 1080 { 144.0 } else { 48.0 }
}

/// What one episode or film of a row weighs: its size, and size over
/// runtime in Mbit/s.
#[derive(Clone, Copy)]
struct Weight {
    size: Option<u64>,
    mbps: Option<f32>,
}

/// The pack guard (3.3): for an episode above the disc ceiling the size is
/// a pack's, divided by the episodes the release names, else by the
/// season's episode count, else the bitrate is unknown. A movie's bitrate
/// is its own, however high (a 3D remux is 51 Mbit/s).
fn weight(s: &ParsedStream, input: &RankInput<'_>) -> Weight {
    let c = input.conditions;
    let size = s.size_bytes;
    let (Some(bytes), Some(runtime)) = (size, c.runtime_s.filter(|r| *r > 0)) else {
        return Weight { size, mbps: None };
    };
    let mbps = bytes as f32 * 8.0 / runtime as f32 / 1e6;
    let episode = input.target.season.is_some() || input.target.episode.is_some();
    if !episode || mbps <= ceiling_mbps(s.release.resolution.height()) {
        return Weight {
            size,
            mbps: Some(mbps),
        };
    }
    let named = s.release.episodes.len() as u32;
    let episodes = if named >= 2 {
        Some(named)
    } else {
        c.episodes_in_season.filter(|n| *n >= 2)
    };
    match episodes {
        Some(n) => Weight {
            size: Some(bytes / n as u64),
            mbps: Some(mbps / n as f32),
        },
        None => Weight { size, mbps: None },
    }
}

/// Seeders of the race swarms the small-swarm prior came from (17 to 22,
/// `bench/m13/race.jsonl`): the prior is their rate.
const THIN_PRIOR_SEEDERS: f32 = 18.0;
/// The least the small-swarm prior gives a swarm.
const THIN_FLOOR_MBPS: f32 = 0.3;

/// The measured speed a row streams at (3.2), and whether it is the
/// small-swarm figure; a fetch has none. A swarm listed (or counted live)
/// under the wanted seeders is judged against the slower of the line and
/// what small swarms give (research R2).
fn speed(s: &ParsedStream, class: SourceClass, input: &RankInput<'_>) -> Option<(Speed, bool)> {
    let c = input.conditions;
    match class {
        SourceClass::Ready => Some((c.http, false)),
        SourceClass::Unknown if c.engine => Some((c.p2p, false)),
        SourceClass::Unknown => Some((c.http, false)),
        SourceClass::Swarm => match s.seeders.filter(|n| *n < input.prefs.min_seeders) {
            Some(seeders) => {
                let thin = thin_speed(c, seeders);
                if thin.mbps < c.p2p.mbps {
                    Some((thin, true))
                } else {
                    Some((c.p2p, false))
                }
            }
            None => Some((c.p2p, false)),
        },
        SourceClass::Fetch => None,
    }
}

/// What a swarm of `seeders` (under the wanted number) streams at: the
/// learned small-swarm estimate once it has samples; before that its prior
/// scaled per seeder, 1.4 Mbit/s x seeders / 18, within 0.3 Mbit/s and the
/// line's speed. So a 31-seeder swarm gets about 2.4 and an 11-seeder one
/// about 0.86, and more seeders still help among thin swarms.
// ponytail: learned samples are not scaled by their seeders; store the
// seeders with each sample if thin picks flip once samples exist.
fn thin_speed(c: &Conditions, seeders: u32) -> Speed {
    if c.p2p_thin.samples > 0 {
        return c.p2p_thin;
    }
    let scaled = c.p2p_thin.mbps * seeders as f32 / THIN_PRIOR_SEEDERS;
    Speed {
        mbps: scaled.clamp(THIN_FLOOR_MBPS, c.p2p.mbps.max(THIN_FLOOR_MBPS)),
        samples: 0,
    }
}

/// Research R1: how far `bitrate` is over `speed`, in bands (3 keeps up
/// with the margin, 2 with none, 1 up to 1.5 times, 0 beyond).
fn pace(bitrate: Option<f32>, speed: Option<f32>) -> u8 {
    let (Some(b), Some(s)) = (bitrate, speed) else {
        return 3;
    };
    let x = b / s;
    if x <= SAFE_SHARE {
        3
    } else if x <= 1.0 {
        2
    } else if x <= 1.5 {
        1
    } else {
        0
    }
}

/// Inside pace band 0, 1000 / x: higher is less far over the row's speed
/// (race 3: a 10x 720p over a 25x 4K file). 0 in the other bands.
fn covered(bitrate: Option<f32>, speed: Option<f32>) -> u16 {
    match (bitrate, speed) {
        (Some(b), Some(s)) if pace(bitrate, speed) == 0 && b > 0.0 => {
            (1000.0 * s / b).round().min(u16::MAX as f32) as u16
        }
        _ => 0,
    }
}

/// Lines the user gets for this row (3.3): one step lower when upscaled or
/// under the floor, then capped by the screen class and the user's maximum.
/// An unstated resolution ranks below every stated one.
fn effective_height(s: &ParsedStream, bitrate: Option<f32>, input: &RankInput<'_>) -> u32 {
    let mut height = s.release.resolution.height();
    let starved = matches!((floor_mbps(height), bitrate), (Some(f), Some(b)) if b < f);
    if s.release.upscaled || starved {
        height = step_down(height);
    }
    let mut cap = screen_class(input.caps.display_height);
    if let Some(max) = input.prefs.max_height {
        cap = cap.min(max);
    }
    height.min(cap)
}

fn legit(s: &ParsedStream) -> bool {
    !matches!(
        s.release.quality,
        Quality::Cam | Quality::Telesync | Quality::Screener
    )
}

/// No year in the name, or one within 3 years of the title's or the
/// episode's (3.5): another film or show is demoted, never hidden.
fn year_ok(s: &ParsedStream, c: &Conditions, target: &Target) -> bool {
    let Some(year) = s.release.year else {
        return true;
    };
    // A number in the title's own name is no year: Blade Runner 2049,
    // Wonder Woman 1984 (code review r2 P3).
    let year_word = year.to_string();
    if target
        .title
        .split(|ch: char| !ch.is_ascii_digit())
        .any(|w| w == year_word)
    {
        return true;
    }
    let near = |t: Option<u16>| t.is_some_and(|t| (year as i32 - t as i32).abs() <= 3);
    (c.title_year.is_none() && c.episode_year.is_none())
        || near(c.title_year)
        || near(c.episode_year)
}

fn start(class: SourceClass, seeders: Option<u32>, prefs: &StreamPrefs) -> u8 {
    let min = prefs.min_seeders;
    match (class, seeders) {
        (SourceClass::Ready, _) => {
            if prefs.prefer_ready {
                4
            } else {
                3
            }
        }
        (SourceClass::Fetch, _) => 0,
        (_, None) => 1,
        // Under 5 a torrent may not start, whatever the user asked for
        // (code review r1 P2-2).
        (_, Some(n)) if n < FEW_SEEDERS => 0,
        (_, Some(n)) if n >= min.saturating_mul(4) => 3,
        (_, Some(n)) if n >= min => 2,
        _ => 1,
    }
}

/// `start` as far as it outranks resolution: a ready file the user prefers,
/// then a ready file or a swarm at the wanted seeders alike, then a thin
/// swarm, then one that may not start.
fn tier(class: SourceClass, start: u8) -> u8 {
    match (class, start) {
        (SourceClass::Ready, 4) => 3,
        (_, s) => s.min(2),
    }
}

/// The size limit guards a torrent's start and the disk: a ready file
/// streams, so it never fails it (r4 N3).
fn size_ok(size: Option<u64>, class: SourceClass, prefs: &StreamPrefs) -> bool {
    if !matches!(class, SourceClass::Swarm | SourceClass::Unknown) {
        return true;
    }
    match (prefs.size_limit_bytes, size) {
        (Some(limit), Some(size)) => size as f64 <= limit as f64 * 1.1,
        _ => true,
    }
}

/// This version's file showed no picture here: it does not play here.
fn no_picture(s: &ParsedStream, input: &RankInput<'_>) -> bool {
    input.caps.hvc1_only
        && input
            .conditions
            .no_picture
            .contains(&crate::key_digest(&s.key))
}

/// The torrent file this version played for this target was not the one
/// listed. Read as "Did not play on this device".
fn wrong_file(s: &ParsedStream, input: &RankInput<'_>) -> bool {
    let (season, episode) = (input.target.season, input.target.episode);
    input
        .conditions
        .wrong_file
        .contains(&crate::target_digest(&s.key, season, episode))
}

/// The container of the file this version opened here for this target
/// (Pioneer One on an iPhone: a torrent listed with no file name was an
/// MKV one).
fn opened_as<'a>(s: &ParsedStream, input: &'a RankInput<'_>) -> Option<&'a str> {
    let (season, episode) = (input.target.season, input.target.episode);
    let digest = crate::target_digest(&s.key, season, episode);
    let opened = &input.conditions.containers;
    opened
        .iter()
        .find(|(d, _)| *d == digest)
        .map(|(_, c)| c.as_str())
}

/// This device's player could not decode this version last time.
fn did_not_play(s: &ParsedStream, input: &RankInput<'_>) -> bool {
    input
        .conditions
        .did_not_play
        .contains(&crate::key_digest(&s.key))
}

/// Whether the screen shows this dynamic range. Dolby Vision and HDR10+
/// carry an HDR10 base, so an HDR10 screen shows them as HDR10.
fn shows(h: Hdr, caps: &DeviceCaps) -> bool {
    h == Hdr::Sdr
        || caps.display_hdr.contains(&h)
        || (matches!(h, Hdr::DolbyVision | Hdr::Hdr10Plus)
            && caps.display_hdr.contains(&Hdr::Hdr10))
}

/// An HDR file on a screen that cannot show it: colors look washed out.
fn washed_out(s: &ParsedStream, caps: &DeviceCaps) -> bool {
    s.release.hdr.iter().any(|h| !shows(*h, caps))
}

fn score(stream: ParsedStream, class: SourceClass, input: &RankInput<'_>) -> Scored {
    let s = &stream;
    let langs = &s.release.languages;
    let weight = weight(s, input);
    let bitrate = weight.mbps;
    let judged = speed(s, class, input);
    let speed = judged.map(|(sp, _)| sp);
    let safe = speed.map(|sp| sp.mbps * SAFE_SHARE);
    let sustains = match (bitrate, safe) {
        (Some(b), Some(safe)) => b <= safe,
        _ => true,
    };
    // Seeders an addon lists are a hint; a swarm that stalled here is not
    // started first again.
    let dead = matches!(class, SourceClass::Swarm | SourceClass::Unknown)
        && info_hash(s).is_some_and(|h| input.conditions.dead_swarms.contains(&h));
    let start = if dead {
        0
    } else {
        start(class, s.seeders, input.prefs)
    };
    let fails_here = no_picture(s, input) || did_not_play(s, input) || wrong_file(s, input);
    let window = window_of(s, class, input);
    let key = RankKey {
        language: input.prefs.languages.is_empty()
            || langs.is_empty()
            || input.prefs.languages.iter().any(|l| langs.contains(l)),
        legit: legit(s),
        year_ok: year_ok(s, input.conditions, input.target),
        sustains,
        pace: pace(bitrate, speed.map(|sp| sp.mbps)),
        covered: covered(bitrate, speed.map(|sp| sp.mbps)),
        playability: if fails_here {
            Playability::NeedsTranscode
        } else {
            playability(s, input).unwrap_or(Playability::NeedsTranscode)
        },
        viable: start > 0 && !fails_here,
        // A windowed row frees the disk as it plays: under the default
        // limit it passes; a limit the viewer chose compares the file
        // (plans/streaming-window.md 3.1).
        size_ok: (window.is_some() && !input.conditions.limit_chosen)
            || size_ok(weight.size, class, input.prefs),
        tier: tier(class, start),
        binge: matches!((input.last_binge_group, &s.binge_group), (Some(a), Some(b)) if a == b),
        effective_height: effective_height(s, bitrate, input),
        start,
        within: within(s, class, bitrate, safe, input),
    };
    let line = line(s, class, &key, weight, judged, window, input);
    Scored {
        stream,
        key,
        class,
        line,
        bitrate,
    }
}

/// The first line that applies, bad before warn before ok (3.7).
fn line(
    s: &ParsedStream,
    class: SourceClass,
    key: &RankKey,
    weight: Weight,
    speed: Option<(Speed, bool)>,
    window: Option<Room>,
    input: &RankInput<'_>,
) -> ReasonLine {
    let (size, bitrate) = (weight.size, weight.mbps);
    let torrent = matches!(class, SourceClass::Swarm | SourceClass::Unknown);
    if no_picture(s, input) {
        return ReasonLine::NoPicture;
    }
    if did_not_play(s, input) || wrong_file(s, input) {
        return ReasonLine::DidNotPlay;
    }
    if !key.legit {
        let quality = match s.release.quality {
            Quality::Cam => "cam",
            Quality::Telesync => "telesync",
            _ => "screener",
        };
        return ReasonLine::Copy {
            quality: quality.into(),
        };
    }
    if class == SourceClass::Fetch {
        return ReasonLine::Fetch {
            service: s.service.clone(),
        };
    }
    if torrent && key.start == 0 && s.seeders.is_none_or(|n| n >= FEW_SEEDERS) {
        return ReasonLine::Stalled;
    }
    if let (true, Some(n)) = (torrent, s.seeders)
        && n < FEW_SEEDERS
    {
        return ReasonLine::VeryFew { seeders: n };
    }
    if !key.year_ok
        && let Some(year) = s.release.year
    {
        return ReasonLine::OtherYear { year };
    }
    if !key.sustains
        && let (Some(need), Some((have, small_swarm))) = (bitrate, speed)
    {
        return ReasonLine::MayPause {
            need_mbps: need,
            have_mbps: have.mbps,
            samples: have.samples,
            small_swarm,
        };
    }
    if washed_out(s, input.caps) {
        return ReasonLine::WashedOut;
    }
    if !key.size_ok
        && let (Some(size), Some(limit)) = (size, input.prefs.size_limit_bytes)
    {
        return ReasonLine::Large {
            size_bytes: size,
            limit_bytes: limit,
            chosen: input.conditions.limit_chosen,
        };
    }
    if let (true, Some(n), 1) = (torrent, s.seeders, key.start) {
        return ReasonLine::Few { seeders: n };
    }
    if class == SourceClass::Ready {
        return ReasonLine::Ready {
            service: s.service.clone(),
            size_bytes: size,
        };
    }
    if let Some(Room::Window { ahead_s, budget }) = window {
        return ReasonLine::Buffer {
            seeders: s.seeders,
            // The file it plays: a pack's episode, not the pack.
            size_bytes: file_size(s, input),
            ahead_min: (!input.conditions.metered).then(|| (ahead_s / 60).min(10) as u32),
            budget_bytes: budget,
        };
    }
    // A pack's episode, as its buffer line says.
    let size = file_size(s, input).or(size);
    if key.start == 3 {
        return ReasonLine::Quick {
            seeders: s.seeders,
            size_bytes: size,
        };
    }
    ReasonLine::Should {
        seeders: s.seeders,
        size_bytes: size,
        service: (class == SourceClass::Unknown)
            .then(|| s.service.clone())
            .flatten(),
    }
}

/// Bounded 0..=1000 score among rows whose key fields tie (3.6).
fn within(
    s: &ParsedStream,
    class: SourceClass,
    bitrate: Option<f32>,
    safe: Option<f32>,
    input: &RankInput<'_>,
) -> u16 {
    let r = &s.release;
    // Start: more seeders, up to `SATURATE_SEEDERS`.
    let mut score = match (class, s.seeders) {
        (SourceClass::Ready, _) => 250.0,
        (_, None) => 125.0,
        (_, Some(n)) => {
            250.0 * ((1.0 + n as f32).log2() / (1.0 + SATURATE_SEEDERS).log2()).min(1.0)
        }
    };
    // Headroom: how far the safe speed is above the file's need.
    score += match (bitrate, safe) {
        (Some(b), Some(safe)) if b > 0.0 => 150.0 * ((safe / b - 1.0) / 2.0).clamp(0.0, 1.0),
        _ => 75.0,
    };
    score += match r.quality {
        Quality::Remux => 200.0,
        Quality::BluRay => 170.0,
        Quality::WebDl => 150.0,
        Quality::WebRip => 120.0,
        Quality::Unknown => 100.0,
        Quality::Hdtv => 80.0,
        Quality::Dvd => 50.0,
        Quality::Screener | Quality::Telesync | Quality::Cam => 0.0,
    };
    let caps = input.caps;
    if input.prefs.hdr {
        if r.hdr.contains(&Hdr::DolbyVision) && caps.display_hdr.contains(&Hdr::DolbyVision) {
            score += 100.0;
        } else if r
            .hdr
            .iter()
            .any(|h| matches!(h, Hdr::Hdr10 | Hdr::Hdr10Plus | Hdr::DolbyVision))
            && caps.display_hdr.contains(&Hdr::Hdr10)
        {
            score += 80.0;
        }
    }
    let atmos = r.audio.contains(&Audio::Atmos);
    let passthrough = r.audio.iter().any(|a| caps.passthrough.contains(a));
    score += match (
        atmos && passthrough,
        passthrough,
        surround(r.channels.as_deref()),
    ) {
        (true, _, _) => 80.0,
        (false, true, _) => 60.0,
        (false, false, true) => 40.0,
        _ => 20.0,
    };
    score += language_score(s, input.prefs) as f32;
    score -= over_resolution_penalty(s, input) as f32;
    score.clamp(0.0, 1000.0) as u16
}

/// Resolution the screen cannot show costs bandwidth and start time for
/// nothing: a 4K remux on a 1080p stick must not beat a 1080p release.
fn over_resolution_penalty(s: &ParsedStream, input: &RankInput<'_>) -> i32 {
    let class = screen_class(input.caps.display_height);
    let height = s.release.resolution.height();
    if height == 0 || height <= class {
        return 0;
    }
    let steps = (height / class).max(1) as i32;
    350 * steps
}

fn surround(channels: Option<&str>) -> bool {
    matches!(channels, Some("5.1" | "7.1"))
}

fn language_score(s: &ParsedStream, prefs: &StreamPrefs) -> i32 {
    let langs = &s.release.languages;
    if prefs.languages.is_empty() || langs.is_empty() {
        return 50;
    }
    if prefs.languages.iter().any(|l| langs.contains(l)) {
        100
    } else {
        0
    }
}

/// Collapses duplicates into one row each; the rest become its failover.
fn dedupe(scored: Vec<Scored>, input: &RankInput<'_>) -> Vec<RankedRow> {
    let mut rows: Vec<RankedRow> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for Scored {
        stream,
        key,
        class,
        line,
        bitrate,
    } in scored
    {
        let identity = release_identity(&stream);
        if let Some(&at) = by_key.get(&stream.key).or_else(|| by_key.get(&identity)) {
            rows[at].failover.push(stream);
            continue;
        }
        by_key.insert(stream.key.clone(), rows.len());
        by_key.insert(identity, rows.len());
        rows.push(RankedRow {
            group: Group::Best,
            reasons: Vec::new(),
            failover: Vec::new(),
            key,
            class,
            line,
            bitrate_mbps: bitrate,
            stream,
        });
    }
    assign_groups_and_reasons(&mut rows, input);
    rows
}

/// Same release across addons: lowercase title, resolution, group and size
/// rounded to 100 MB.
fn release_identity(s: &ParsedStream) -> String {
    let r = &s.release;
    format!(
        "rel:{}:{}:{}:{}",
        r.title.to_lowercase(),
        r.resolution.height(),
        r.group.as_deref().unwrap_or("").to_lowercase(),
        s.size_bytes.map_or(0, |b| b / 100_000_000)
    )
}

fn assign_groups_and_reasons(rows: &mut [RankedRow], input: &RankInput<'_>) {
    let best_size = rows.first().and_then(|r| r.stream.size_bytes);
    // "Needs download" only means something next to a ready alternative.
    let any_ready = rows.iter().any(|r| r.class == SourceClass::Ready);
    for row in rows.iter_mut() {
        let s = &row.stream;
        let wants_lang = !input.prefs.languages.is_empty() && !s.release.languages.is_empty();
        let lang_match = input
            .prefs
            .languages
            .iter()
            .any(|l| s.release.languages.contains(l));
        row.group = if any_ready && row.class != SourceClass::Ready {
            Group::NeedsDownload
        } else if wants_lang && !lang_match {
            Group::OtherLanguages
        } else if matches!((s.size_bytes, best_size), (Some(a), Some(b)) if a * 2 < b) {
            Group::SmallerFaster
        } else {
            Group::Best
        };
        let mut reasons = Vec::new();
        if row.class == SourceClass::Ready {
            reasons.push(Reason::ReadyNow);
        }
        if row.key.binge {
            reasons.push(Reason::SameReleaseAsLast);
        }
        if row.key.playability == Playability::Native {
            reasons.push(Reason::PlaysDirectly);
        }
        if s.release.resolution.height() == screen_class(input.caps.display_height) {
            reasons.push(Reason::MatchesScreen);
        }
        if wants_lang && lang_match {
            reasons.push(Reason::YourLanguage);
        }
        if row.group == Group::SmallerFaster {
            reasons.push(Reason::SmallerFaster);
        }
        reasons.truncate(MAX_REASONS);
        row.reasons = reasons;
    }
}
