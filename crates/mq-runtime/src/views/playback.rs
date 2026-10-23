//! Playback: turns one picked version into a loopback URL the player opens,
//! falls over to the same release from another source when one fails, and
//! saves watch progress.
//!
//! The candidates are the picked Versions row followed by its failover chain
//! (the same release from other addons or services).

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::segments::{self, Segment, Target};
use crate::turkcealtyazi::{self, NAME as BUILT_IN};
use crate::views::versions::{EmptyKind, FailedOn, version_label};
use eyeball::SharedObservable;
use futures_util::{FutureExt, Stream, StreamExt};
use mq_addons::{AddonEvent, Deadlines, InstalledAddon};
use mq_debrid::{ResolveRequest, Resolver, StoreId};
use mq_library::{Authorization, FINISHED_RATIO, LibraryItemRow, Policy, ProgressRow};
use mq_protocol::{ResourcePath, Source, decode_subtitles};
use mq_relparse::fit;
use mq_server::{Format, Lease, SubtitleSlot, SubtitleTrouble, Upstream};
use mq_streams::{DeviceCaps, ParsedStream};
use serde::Serialize;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};
use stremio_core::types::addon::ExtraValue;
use tokio::task::JoinHandle;

/// Progress is written at most this often while playing.
const SAVE_EVERY: Duration = Duration::from_secs(5);
/// A source must answer its first byte within this to count as alive.
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// The subtitle hash reads two 64 KiB ranges; past this, ask without it.
const HASH_TIMEOUT: Duration = Duration::from_secs(4);
/// One step of a skip segment lookup (the release-matched source, or the gap
/// filler), queueing and mapping included.
const SEGMENTS_DEADLINE: Duration = Duration::from_secs(6);
/// A torrent's swarm must send its metadata within this...
#[cfg(feature = "torrent")]
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
/// ...and the head plus the container tail within this, before the player
/// gets the URL (players give up on a silent body after a few seconds).
#[cfg(feature = "torrent")]
const TORRENT_READY_TIMEOUT: Duration = Duration::from_secs(60);
/// ...but a swarm that sends almost nothing is cut early when another
/// candidate can play instead (plans/m13-stream-choice.md 7, dead swarms).
/// Measured: a healthy bare-hash swarm found and unchoked its peers for
/// most of its first 19 s (Android, bench/c27/android-bare-lto.jsonl), and
/// a Torrentio swarm with 22 listed seeders sent nothing for 150 s after
/// its metadata (the Apple walk).
#[cfg(any(test, feature = "torrent"))]
const STALL: Stall = Stall {
    first: Duration::from_secs(30),
    later: Duration::from_secs(15),
    floor: 256 << 10,
};
/// A capacity sample is read this long after its source started
/// (plans/m13-stream-choice.md 5).
const SAMPLE_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshot {
    pub phase: PlaybackPhase,
    /// Loopback URL for the player, once `ready`.
    pub url: Option<String>,
    /// The version that is playing: the picked key or one of its failovers.
    pub key: Option<String>,
    /// The source the view adopted last: one more each time it moves to a
    /// new source, never reused in the view's life (0 before the first).
    /// Two sources of one `key` (two installs listing the same link) differ
    /// here; `failOver` names it.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub attempt: u64,
    /// `url` is a manifest the core rewrote, not a file: HLS (its name ends
    /// in `.m3u8`) or DASH (`.mpd`).
    pub manifest: bool,
    pub filename: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size_bytes: Option<u64>,
    /// Where to start, from saved progress.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub resume_ms: Option<u64>,
    /// Sources that failed, in order, each with a plain reason.
    pub attempts: Vec<Attempt>,
    /// External subtitles: the stream's own, then each subtitles addon's, as
    /// they arrive.
    pub subtitles: Vec<SubtitleTrack>,
    /// Subtitles addons are still being asked.
    pub subtitles_pending: bool,
    /// Subtitle sources that gave nothing this time, each once, shown under
    /// the subtitle list (plans/m13-turkcealtyazi.md 4.2).
    pub subtitle_notes: Vec<SubtitleNote>,
    /// The track (its `url`) every app turns on at start, once, when the
    /// list has settled: the profile set subtitle languages and the audio
    /// is in none of them, or the user chose one for this title before.
    /// Null otherwise. The user's own pick, or Off, always wins: send
    /// `subtitleChosen` (plans/m13-subtitle-languages.md 6).
    pub auto_subtitle: Option<String>,
    /// The next released episode, for an Up Next card.
    pub next: Option<NextEpisode>,
    /// The playing stream's binge group; pass it as `bingeGroup` when opening
    /// Versions for the next episode so the same release line is picked.
    pub binge_group: Option<String>,
    /// Skip segments of the playing version, once its duration is reported.
    pub segments: Vec<Segment>,
    /// When to offer the Up Next card: the credits start from a source that
    /// matched the release; null keeps the app's own timing.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub next_at_ms: Option<u64>,
    /// Trakt's side: `on` scrobbles, `unmapped` (Trakt cannot name it),
    /// `off`, `thisDeviceOff` (the profile uses Trakt, this device is not
    /// signed in).
    pub trakt: crate::trakt::TraktPlayback,
    /// The recommended version failed for a reason of its own (no picture
    /// on this device, a dead swarm, the wrong file) and the view moved on
    /// by itself to this one, the ranking's next pick, at most twice: "Trying
    /// the next version: {label}" while `phase` is `resolving`. Cleared once
    /// a source of it starts.
    pub trying: Option<OtherVersion>,
    /// A version picked by hand failed for a reason of its own: the failure
    /// card offers this one, the ranking's next pick (open `playback` with
    /// its key from the same Versions subscription).
    pub offer: Option<OtherVersion>,
    /// "Try again" taken in this view: one more each time it is accepted,
    /// so a retry that fails again at once still differs from the failure
    /// before it (code review 2026-10-20 r2 P3-6). Show "Trying again".
    pub retries: u32,
    /// How far the start is while `phase` is `resolving`: the loading
    /// screen's fill and its words (plans/player-polish.md 1.3). Null before
    /// the first source starts and from `ready` on.
    pub start: Option<StartProgress>,
    /// The version playing is not the one the start announced: the
    /// recommendation moved on (`trying`) and this one plays. The core's
    /// words, for the apps to show once from `ready`: "Playing 720p WEB:
    /// the 1080p Blu-ray did not start here", "... did not play on this
    /// device". Null otherwise.
    pub moved: Option<String>,
    /// The start has taken `CHOOSE_AFTER` (20 s): the loading screen shows
    /// the quiet "Choose another version" beside Back
    /// (plans/play-every-version.md 2e).
    pub choose_another: bool,
    /// Why nothing played, once `phase` is `failed`: the zero-playable words
    /// for what happened (`empty.<kind>` in stream-choice-words.json):
    /// notStarted when the start's budget ran out; else noRoom when every
    /// failure was a refusal for room, nothingPlaysHere when every failure
    /// was the versions' own on this device, else unreachable.
    pub empty: Option<EmptyKind>,
    /// When a first play's start budget (90 s from its first attempt's add)
    /// ends, in epoch ms, while it runs: from the first attempt until a
    /// source played past its start (or the playback failed). Time the app
    /// spends after `ready` (another player engine opening the file) counts
    /// against it too (plans/apple-second-engine.md 2.3). Null otherwise.
    /// An app reads it only while it is still opening the source: a video
    /// that plays, or is paused, before its first 30 s is never failed by
    /// it.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub start_budget_ends_at: Option<u64>,
    /// The container of the file playing ("mkv", "mp4"), once `ready`: the
    /// opened file's name as the core parses it, else what an earlier play
    /// of this version opened, else the release's; null when unknown. Apple
    /// picks its player engine by it (plans/apple-second-engine.md 2.1).
    pub container: Option<String>,
    /// The playing release's audio formats in the core's names
    /// ("dts_hd_ma", "true_hd", "atmos"), once `ready`; empty when unknown.
    pub audio: Vec<String>,
    /// The second engine this version played on here before (`playedOn`),
    /// after the first refused it ("aether"): the app opens it there first.
    pub engine: Option<String>,
}

/// One start of the playback, from the first source it tries to `ready`
/// (plans/player-polish.md 1.3).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct StartProgress {
    pub stage: StartStage,
    /// Peers connected to a torrent.
    pub peers: u32,
    /// What a torrent's swarm sent of the start the player needs, and all
    /// of it.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub have_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub need_bytes: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes_per_s: u64,
    /// The source of the version it is on, from 1, of `sources`.
    pub source: u32,
    pub sources: u32,
    /// 0 to 1: never back within one start (a move to the next source goes
    /// on from where the last one got), never 1 before `ready`.
    pub progress: f32,
    /// The core's words once a stage takes long: "Getting the file's
    /// details", "Finding peers, 3 so far", "Buffering, 4.1 of 8.4 MB";
    /// "Trying source 2 of 3" as a next source starts. Null otherwise.
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum StartStage {
    /// A torrent's details (its files) are asked of the swarm.
    Details,
    /// A torrent's peers are being found; a link is being opened.
    Connecting,
    /// The swarm sends the start of the video.
    Buffering,
    /// What is in is read through the core's server for the first time
    /// (the player gets the URL next).
    Opening,
}

/// A version the playback moved on to, or offers instead.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct OtherVersion {
    pub key: String,
    /// "1080p WEB", "4K Remux": the row's quality and source.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct NextEpisode {
    pub video_id: String,
    pub title: String,
    pub season: u32,
    pub episode: u32,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub url: String,
    /// As the addon gives it, usually ISO 639-2 ("eng"), sometimes an
    /// OpenSubtitles special ("pob").
    pub lang: String,
    /// The language as one tag for every spelling of it: ISO 639-1 where
    /// there is one, with its region or script ("en", "pt-BR", "zh-Hant",
    /// "es-419"); tracks of one tag sit together in `subtitles`.
    pub language: String,
    /// The language's English name ("Portuguese (Brazil)"): the heading
    /// of its group.
    pub language_name: String,
    /// The language's own name ("Português (Brasil)", "Türkçe"); the
    /// English name when the core knows no other.
    pub native_name: String,
    /// What tells this track from the others of its language: what its
    /// release text says, the group, then source and resolution ("CAKES,
    /// WEB 1080p", "PSA, WEBRip 1080p 10 bit"); the text itself when it
    /// names none of them; else its source. Numbered when two would read
    /// the same.
    pub name: String,
    pub label: Option<String>,
    /// Addon name.
    pub source: String,
    pub forced: bool,
    /// The addon says so, or the release text marks it ("-hi", ".HI.",
    /// "SDH").
    pub hearing_impaired: bool,
    /// The track names the playing release's group: show "Same release
    /// group" for it.
    pub fits: bool,
    /// OpenSubtitles says it was made for this very file (its hash): show
    /// "Matches this file".
    pub hash_match: bool,
    /// The frame rate it was made for, set only when one of it and the
    /// playing file's is PAL (25) and the other is not: show "Made for 25
    /// fps".
    pub made_for_fps: Option<f32>,
    /// The delay the viewer last set for this track on this version and
    /// video, in ms (positive shows the text later): the stepper starts
    /// there. 0 when none.
    pub delay_ms: i32,
    /// The release text the addon names for the track (its label, file
    /// name or release name), for a detail line; null when it names none.
    pub release: Option<String>,
    /// What names the file across plays (its URL here is per play): the
    /// addon id and the file's id, else the addon's URL for it.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    origin: String,
    /// `name` before numbering.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    base: String,
    /// What the track says of its release, for the release match.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    made: Made,
}

impl SubtitleTrack {
    /// A track in `lang` from `source`, its language read, its name what its
    /// release text says (`short_name`), else the source, until `reorder`
    /// tells duplicates apart.
    fn new(
        url: String,
        lang: &str,
        label: Option<String>,
        source: &str,
        release: Option<String>,
    ) -> Self {
        let language = crate::languages::language(lang);
        // A label that only names the language says nothing more.
        let release = release.map(|r| r.trim().to_owned()).filter(|r| {
            !r.is_empty()
                && !r.eq_ignore_ascii_case(&language.name)
                && !r.eq_ignore_ascii_case(lang)
        });
        let (marked, plain) = release
            .as_deref()
            .map_or((false, String::new()), |r| hearing_marks(r, &language.tag));
        // Parsed once, for the name and the release match.
        let parsed = release
            .as_ref()
            .map(|_| mq_relparse::parse(&plain))
            .unwrap_or_default();
        let base = match &release {
            Some(text) => short_name(&parsed).unwrap_or_else(|| text.clone()),
            None => source.to_owned(),
        };
        Self {
            url,
            lang: lang.to_owned(),
            language: language.tag,
            name: base.clone(),
            base,
            language_name: language.name,
            native_name: language.native,
            label,
            source: source.to_owned(),
            forced: false,
            hearing_impaired: marked,
            fits: false,
            hash_match: false,
            made_for_fps: None,
            delay_ms: 0,
            origin: String::new(),
            made: Made {
                family: fit::family(parsed.quality),
                cut: parsed.cut,
                text: plain,
                ..Made::default()
            },
            release,
        }
    }

    /// What the track says of its release, for `fit::misses`.
    fn made(&self) -> fit::Made<'_> {
        self.made.get(self.forced, self.hearing_impaired)
    }
}

/// What tells one release from another in a subtitle menu: its group, then
/// source and resolution and a bit depth over 8 ("CAKES, WEB 1080p", "PSA,
/// WEBRip 1080p 10 bit"); `None` when the text names none of them.
fn short_name(r: &mq_relparse::ParsedRelease) -> Option<String> {
    use mq_relparse::{Quality as Q, Resolution as R};
    let source = match r.quality {
        Q::WebDl => "WEB",
        Q::WebRip => "WEBRip",
        Q::BluRay => "BluRay",
        Q::Remux => "Remux",
        Q::Hdtv => "HDTV",
        Q::Dvd => "DVD",
        Q::Screener => "SCR",
        Q::Telesync => "TS",
        Q::Cam => "CAM",
        Q::Unknown => "",
    };
    let height = (r.resolution != R::Unknown).then(|| format!("{}p", r.resolution.height()));
    let bits = r.bit_depth.filter(|b| *b > 8).map(|b| format!("{b} bit"));
    let what: Vec<String> = std::iter::once(source.to_owned())
        .filter(|s| !s.is_empty())
        .chain(height)
        .chain(bits)
        .collect();
    match (release_group(r), what.join(" ")) {
        (None, what) if what.is_empty() => None,
        (None, what) => Some(what),
        (Some(group), what) if what.is_empty() => Some(group.to_owned()),
        (Some(group), what) => Some(format!("{group}, {what}")),
    }
}

/// The release's group, when its name reads as a release (a source or a
/// resolution beside the group) and the "group" is no language: neither
/// "Sintel (2010) - English" nor "The.Amazing.Spider-Man" names one (review
/// P3-3).
fn release_group(r: &mq_relparse::ParsedRelease) -> Option<&str> {
    use mq_relparse::{Quality, Resolution};
    let named = r.quality != Quality::Unknown || r.resolution != Resolution::Unknown;
    r.group
        .as_deref()
        .filter(|g| named && !crate::languages::is_known(g))
}

/// Whether a release text marks its file for the hard of hearing ("SDH", or
/// "hi" or "HI" after a dash or a dot, or in brackets, with no language
/// beside it: "EN-HI" and ".HI.TA" are Hindi), and the text without those
/// marks, so the parser never takes "hi" for the group. A title word "Hi"
/// is no mark. The track's own language before a lowercase ".hi" is the
/// sidecar naming for the hard of hearing ("Movie.en.hi"), no Hindi: `own`
/// is that language's tag.
fn hearing_marks(text: &str, own: &str) -> (bool, String) {
    let (mut found, mut kept, mut last) = (false, String::new(), 0);
    let mut previous: Option<&str> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if !c.is_alphanumeric() {
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(at, c)) = chars.peek().filter(|(_, c)| c.is_alphanumeric()) {
            end = at + c.len_utf8();
            chars.next();
        }
        let (word, before) = (&text[start..end], text[..start].chars().next_back());
        let after = text[end..].chars().next();
        // A language beside it makes "HI" Hindi ("EN-HI", ".HI.TA"), but
        // not the track's own one before a sidecar's ".hi".
        let sidecar = word == "hi" && before == Some('.');
        let language = |w: Option<&str>| {
            w.is_some_and(|w| {
                let own_one = crate::languages::primary(&crate::languages::language(w).tag)
                    == crate::languages::primary(own);
                w.len() <= 3 && crate::languages::is_known(w) && !(sidecar && own_one)
            })
        };
        let next = text[end..]
            .split(|c: char| !c.is_alphanumeric())
            .find(|w| !w.is_empty());
        let bracketed = matches!(before, Some('[' | '(')) && matches!(after, Some(']' | ')'));
        let hi = (word == "hi" || word == "HI")
            && (bracketed
                || (matches!(before, Some('-' | '.')) && !language(previous) && !language(next)));
        let mark = word.eq_ignore_ascii_case("sdh") || hi;
        previous = Some(word);
        if mark {
            found = true;
            // The mark goes with the separator before it, and in brackets
            // with the closing one.
            let cut = before.map_or(start, |b| start - b.len_utf8());
            kept.push_str(&text[last..cut.max(last)]);
            last = end + after.filter(|_| bracketed).map_or(0, char::len_utf8);
        }
    }
    kept.push_str(&text[last.min(text.len())..]);
    (found, kept.trim().to_owned())
}

/// What a subtitle says of the release it was made for (`fit::Made`),
/// owned by its track.
#[derive(Debug, Clone, Default, PartialEq)]
struct Made {
    text: String,
    family: Option<fit::Family>,
    cut: Option<mq_relparse::Cut>,
    fps: Option<f32>,
    hash: bool,
}

impl Made {
    fn of(made: fit::Made) -> Self {
        Self {
            text: made.text.to_owned(),
            family: made.family,
            cut: made.cut,
            fps: made.fps,
            hash: made.hash,
        }
    }

    /// With the track's own forced and hearing marks: the one place each
    /// source sets them.
    fn get(&self, forced: bool, hearing: bool) -> fit::Made<'_> {
        fit::Made {
            text: &self.text,
            family: self.family,
            cut: self.cut,
            fps: self.fps,
            hash: self.hash,
            forced,
            hearing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleNote {
    /// As `SubtitleTrack::source`: the addon name.
    pub source: String,
    pub problem: SubtitleProblem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleProblem {
    /// The source did not answer; its tracks stay and may load later.
    Unreachable,
    /// The source turns requests away for a few minutes.
    Blocked,
    /// The source's pages changed and cannot be read until an update.
    Changed,
    /// A file of the source could not be read; its track left the list.
    TrackFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackPhase {
    Resolving,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub key: String,
    pub via: String,
    pub error: String,
}

/// What is being watched, for progress and Continue Watching.
#[derive(Debug, Clone, Default)]
pub struct PlayTarget {
    pub kind: String,
    pub item_id: String,
    pub video_id: String,
    pub name: Option<String>,
    pub poster: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

struct State {
    snapshot: PlaybackSnapshot,
    /// The last session's duration of this video, and the meta runtime of
    /// the picked row's view: a torrent's rate until the player reports one
    /// (plans/streaming-window.md 2.2).
    known_duration_ms: Option<u64>,
    runtime_s: Option<u32>,
    candidates: Vec<ParsedStream>,
    next: usize,
    /// A source of the version failed for a reason of the version's own,
    /// in the words of `moved`: `NOT_HERE` when it did not play here (its
    /// decode or audio, no picture, a container never opened, the wrong
    /// file), else `NOT_STARTED` (a swarm that did not start).
    version_failed: Option<&'static str>,
    /// Every source key of every chain this view started (the pick's and
    /// each version it moved on to), and how many times it moved on by
    /// itself.
    tried: Vec<String>,
    hops: u8,
    /// What the failing version did not play on here (a decode or an
    /// audio failure), which the next version should not share.
    failed_on: Option<FailedOn>,
    /// Why the announced version was left, once the recommendation moved
    /// on: "did not start here", "did not play on this device".
    left_because: Option<&'static str>,
    /// The pick's own chain, for "Try again" to start afresh.
    first: Vec<ParsedStream>,
    /// A first play's start: its first attempt's add (the budget's origin,
    /// `START_BUDGET`) and the attempts begun in it. Cleared once a source
    /// played past its start, and for a "Try again" run (its own budget).
    start_at: Option<tokio::time::Instant>,
    tries: u32,
    /// When the loading screen began: `choose_another` after `CHOOSE_AFTER`.
    loading_since: tokio::time::Instant,
    /// What the failures of this start say, for `empty`.
    failed_as: Option<EmptyKind>,
    /// The playing source played `PLAYED_PAST_START`, counted from the
    /// first position reported after it started: a decode error after that
    /// is not the version's.
    progressed: bool,
    /// The last position reported for the playing source, and how long it
    /// has played since its first report (forward steps only: a seek or
    /// a resume point is not playback, code review 2026-10-20 r2 P3-1).
    reported_ms: Option<u64>,
    played_ms: u64,
    /// When a "Try again" run gives up, however many sources are left
    /// (code review 2026-10-20 r3 P3-11); none once a source plays.
    retry_until: Option<tokio::time::Instant>,
    /// The playing source's token: its URL and its subtitles' URLs live as
    /// long as it is kept here.
    lease: Option<Lease>,
    /// Bumped under this lock by every new resolve and every failover; a
    /// resolve task adopts a lease, and publishes tracks, only while its own
    /// number is the newest.
    run: u64,
    /// The view is gone: no lease is ever adopted again.
    closed: bool,
    /// The Versions pick this playback came from: a failure is reported
    /// to it (plans/m13-stream-choice.md 3.9).
    pick: Option<crate::views::versions::PickHandle>,
    /// "Try again" was asked: each torrent gets the full start deadline,
    /// never cut early as a dead swarm.
    patient: bool,
    /// Dead swarms waiting for proof the connection works (a peer or a
    /// source reached in this chain), so an outage marks none of them.
    dead: Dead,
    /// The HTTP capacity sample was taken (once per view: at 30 s, or when
    /// the view closes first, as a short trailer's does).
    sampled: bool,
    last: Option<(u64, u64)>,
    saved_at: Option<Instant>,
    dirty: bool,
    /// Bumped whenever the playing source changes, so a skip lookup for an
    /// earlier source never lands on this one.
    source: u64,
    /// The source and duration the current skip lookup is for.
    lookup: Option<(u64, u64)>,
    /// The running skip lookup; started and replaced under the state lock,
    /// so an older report can never abort a newer lookup.
    segments_task: Option<JoinHandle<()>>,
    /// The audio languages the pick was ranked with (`Pick::languages`):
    /// the profile's, else the device's, then English. Empty without a pick.
    languages: Vec<String>,
    /// Each subtitle track's place, by URL: its source's position in the
    /// installed list (the stream's own first), then its rank there. Off
    /// the wire; `subtitles` is sorted by it within a language, after the
    /// release match.
    order: HashMap<String, (usize, usize)>,
    /// What the playing release says of itself, for the subtitles' release
    /// match: its name's, and the frame rate once the probe read it.
    playing: OwnedPlaying,
    /// The viewer picked a subtitle, or Off, in this view: `autoSubtitle`
    /// stays null from then on.
    subtitle_picked: bool,
    /// This source's start subtitle is decided: a slower source's tracks
    /// still list, but never change it.
    start_settled: bool,
    /// The subtitle delay the title remembers for the playing version and
    /// video: the track's origin (`SubtitleTrack::origin`) and its ms.
    delay: Option<(String, i32)>,
    /// The start's progress when the source starting now began, and when
    /// it began (`start`).
    start_base: f32,
    source_since: tokio::time::Instant,
    /// Torrents of this view that did not start (no details in time, a
    /// stall, the deadline), by lowercase info hash: another source of one
    /// is skipped at once. "Try again" clears it.
    #[cfg(feature = "torrent")]
    failed_hashes: Vec<String>,
    /// The next torrent candidates' details, fetched while the current one
    /// waits (plans/research/start-latency.md 4.1).
    #[cfg(feature = "torrent")]
    ahead: Vec<Ahead>,
    /// Cancelled when the view closes: each fetch ahead then removes the
    /// torrent it added unless a start took it.
    #[cfg(feature = "torrent")]
    closing: tokio_util::sync::CancellationToken,
    /// Every torrent this view started, from the moment `add_magnet`
    /// answered: a policy closure pauses them all, a lease adopted or not.
    #[cfg(feature = "torrent")]
    torrents: Vec<mq_engine::TorrentHandle>,
    /// The torrent file playing, and its length: the player's duration
    /// gives the engine its exact rate (`report_duration`).
    #[cfg(feature = "torrent")]
    rate_of: Option<(mq_engine::TorrentHandle, usize, u64)>,
    /// The player's last reported pause, sent to the engine on a change.
    paused: bool,
    /// The last report moved on by a step of playback (not a seek, not a
    /// pause): a stall from there may move to the next version.
    advancing: bool,
    /// The current stall (bumped at each start and end of one), and
    /// whether this play already moved on for one (at most once).
    stall: u64,
    stall_moved: bool,
    /// The generation the stall now on began with: a repeated
    /// `stalled(true)` while it lasts keeps its clock.
    stall_on: Option<u64>,
    /// The release a Resume plays (the title's remembered one, with a
    /// resume point): a stall never moves it.
    resume_key: Option<String>,
}

pub struct PlaybackView {
    core: Core,
    target: Arc<PlayTarget>,
    /// The playlist formats the player plays (its caps).
    plays: Arc<[Format]>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    state: Arc<Mutex<State>>,
    /// The resolve task and its run.
    task: Arc<TaskSlot>,
    /// The Trakt session; dropped after the last save, which sends the stop.
    trakt: Option<crate::trakt::Scrobble>,
    /// A kids policy refused the target: the view does no work.
    denied: bool,
}

impl State {
    fn new(candidates: Vec<ParsedStream>, resume_ms: Option<u64>) -> Self {
        Self {
            known_duration_ms: None,
            runtime_s: None,
            snapshot: PlaybackSnapshot {
                phase: PlaybackPhase::Resolving,
                url: None,
                key: None,
                attempt: 0,
                manifest: false,
                filename: None,
                size_bytes: None,
                resume_ms,
                attempts: Vec::new(),
                subtitles: Vec::new(),
                subtitles_pending: false,
                subtitle_notes: Vec::new(),
                auto_subtitle: None,
                next: None,
                binge_group: None,
                segments: Vec::new(),
                next_at_ms: None,
                trakt: crate::trakt::TraktPlayback::Off,
                trying: None,
                offer: None,
                retries: 0,
                start: None,
                moved: None,
                choose_another: false,
                empty: None,
                start_budget_ends_at: None,
                container: None,
                audio: Vec::new(),
                engine: None,
            },
            tried: candidates.iter().map(|c| c.key.clone()).collect(),
            first: candidates.clone(),
            start_at: None,
            tries: 0,
            loading_since: tokio::time::Instant::now(),
            failed_as: None,
            candidates,
            next: 0,
            version_failed: None,
            hops: 0,
            failed_on: None,
            left_because: None,
            progressed: false,
            reported_ms: None,
            played_ms: 0,
            retry_until: None,
            lease: None,
            run: 0,
            closed: false,
            pick: None,
            patient: false,
            dead: Dead::default(),
            sampled: false,
            last: None,
            saved_at: None,
            dirty: false,
            source: 0,
            lookup: None,
            segments_task: None,
            order: HashMap::new(),
            playing: OwnedPlaying::default(),
            subtitle_picked: false,
            start_settled: false,
            delay: None,
            start_base: 0.0,
            source_since: tokio::time::Instant::now(),
            languages: Vec::new(),
            #[cfg(feature = "torrent")]
            failed_hashes: Vec::new(),
            #[cfg(feature = "torrent")]
            ahead: Vec::new(),
            #[cfg(feature = "torrent")]
            closing: tokio_util::sync::CancellationToken::new(),
            #[cfg(feature = "torrent")]
            torrents: Vec::new(),
            #[cfg(feature = "torrent")]
            rate_of: None,
            paused: false,
            advancing: false,
            stall: 0,
            stall_moved: false,
            stall_on: None,
            resume_key: None,
        }
    }
}

impl PlaybackView {
    /// `open_for` a player that plays HLS.
    pub fn open(core: &Core, candidates: Vec<ParsedStream>, target: PlayTarget) -> Self {
        Self::open_for(core, candidates, target, &[Format::Hls])
    }

    /// Opens the candidates in turn for a player that plays the playlist
    /// formats `plays` names (`VersionsView::plays`): a playlist it cannot
    /// play fails over like any source that does not open. Under a kids
    /// policy a target off the list, or a video not of its title, opens
    /// denied: no work, an empty snapshot, and `denied()` says so.
    pub fn open_for(
        core: &Core,
        candidates: Vec<ParsedStream>,
        target: PlayTarget,
        plays: &[Format],
    ) -> Self {
        Self::open_with(core, candidates, None, target, plays)
    }

    fn open_with(
        core: &Core,
        candidates: Vec<ParsedStream>,
        pick: Option<crate::views::versions::PickHandle>,
        target: PlayTarget,
        plays: &[Format],
    ) -> Self {
        let denied = core.policy().kids()
            && !matches!(
                core.library().authorize(
                    core.profile(),
                    &target.kind,
                    &target.item_id,
                    Some(&target.video_id),
                ),
                Ok(Authorization::Authorized { .. })
            );
        if denied {
            let state = State::new(Vec::new(), None);
            return Self {
                core: core.clone(),
                snapshot: SharedObservable::new(state.snapshot.clone()),
                state: Arc::new(Mutex::new(state)),
                target: Arc::new(target),
                plays: plays.into(),
                task: Arc::default(),
                trakt: None,
                denied: true,
            };
        }
        let progress = core
            .library()
            .progress(core.profile(), &target.video_id)
            .ok()
            .flatten();
        let resume_ms = resume_point(progress);
        let mut state = State::new(candidates, resume_ms);
        // The last session's length: a torrent's rate before the player
        // reports this one's (plans/streaming-window.md 2.2).
        state.known_duration_ms = progress.map(|(_, dur)| dur as u64).filter(|&d| d > 0);
        state.pick = pick;
        state.resume_key = resume_ms.and_then(|_| {
            stream_memory(core, &target)?["key"]
                .as_str()
                .map(str::to_owned)
        });
        let snapshot = SharedObservable::new(state.snapshot.clone());
        let shared = Arc::new(Mutex::new(state));
        let report = {
            let (shared, snapshot) = (Arc::downgrade(&shared), snapshot.clone());
            move |trakt| {
                if let Some(state) = shared.upgrade() {
                    let mut s = lock(&state);
                    s.snapshot.trakt = trakt;
                    snapshot.set(s.snapshot.clone());
                }
            }
        };
        let session = crate::trakt::ScrobbleTarget {
            kind: target.kind.clone(),
            item_id: target.item_id.clone(),
            video_id: target.video_id.clone(),
            name: target.name.clone().unwrap_or_default(),
        };
        let trakt = core.trakt_scrobble(session, Box::new(report));
        let view = Self {
            core: core.clone(),
            target: Arc::new(target),
            plays: plays.into(),
            snapshot,
            state: shared,
            task: Arc::default(),
            trakt,
            denied: false,
        };
        view.spawn_resolve();
        view
    }

    /// `open_for` the row the user picked in Versions: a source of it that
    /// fails is reported back to the pick.
    pub fn open_picked(
        core: &Core,
        pick: crate::views::versions::Pick,
        target: PlayTarget,
        plays: &[Format],
    ) -> Self {
        let view = Self::open_with(core, pick.candidates, Some(pick.handle), target, plays);
        let mut s = lock(&view.state);
        s.languages = pick.languages;
        s.runtime_s = pick.runtime_s;
        drop(s);
        view
    }

    /// "Try again" once every source failed: the pick's own sources again
    /// from the first, patiently (no early cut), so a swarm that is only
    /// slow to start gets its chance, within one 3 minute budget for the
    /// whole run: each wait is cut to what is left, and when it is spent the
    /// run ends with the attempt "Tried again for 3 minutes" (via
    /// "Marquee"). Ignored unless the playback failed.
    /// The app's second player engine (`engine`, "aether") played the
    /// version playing after the first refused it: its next play here goes
    /// there first (`engine` in the snapshot). Kept per version and target
    /// on this device, never synced. Ignored before `ready`.
    pub fn played_on(&self, engine: &str) -> Result<(), String> {
        if engine != "aether" {
            return Err(format!("unknown engine: {engine}"));
        }
        let mut s = lock(&self.state);
        let Some(key) = s
            .snapshot
            .key
            .clone()
            .filter(|_| s.snapshot.phase == PlaybackPhase::Ready)
        else {
            return Ok(());
        };
        let played_for = (self.target.season, self.target.episode);
        let now = mq_library::hlc::now_ms();
        crate::speed::record_engine(self.core.library(), &key, played_for, engine, now);
        s.snapshot.engine = Some(engine.to_owned());
        self.snapshot.set(s.snapshot.clone());
        Ok(())
    }

    pub fn retry(&self) {
        {
            let mut s = lock(&self.state);
            if s.snapshot.phase != PlaybackPhase::Failed || s.closed {
                return;
            }
            // A fresh start of the pick itself, after any moves on (code
            // review 2026-10-20 P1-1), each torrent with the full deadline.
            s.candidates = s.first.clone();
            s.tried = s.first.iter().map(|c| c.key.clone()).collect();
            s.retry_until = Some(tokio::time::Instant::now() + RETRY_BUDGET);
            s.start_at = None;
            s.tries = 0;
            s.failed_as = None;
            s.snapshot.empty = None;
            s.hops = 0;
            s.next = 0;
            s.version_failed = None;
            s.failed_on = None;
            s.left_because = None;
            s.snapshot.moved = None;
            s.patient = true;
            #[cfg(feature = "torrent")]
            s.failed_hashes.clear();
            s.snapshot.phase = PlaybackPhase::Resolving;
            s.snapshot.attempts.clear();
            s.snapshot.trying = None;
            s.snapshot.offer = None;
            s.snapshot.retries += 1;
            self.snapshot.set(s.snapshot.clone());
        }
        self.spawn_resolve();
    }

    /// Tests: whether a swarm that falls short would be cut at its first
    /// window now.
    #[doc(hidden)]
    pub fn may_cut_early(&self) -> bool {
        may_cut_early(&lock(&self.state))
    }

    /// Tests: how a "Try again" run would judge a torrent's start now: an
    /// early cut, the deadline and the cap.
    #[doc(hidden)]
    #[cfg(feature = "torrent")]
    pub fn start_rule(&self) -> (bool, bool, Duration, Duration) {
        let r = start_rule(&lock(&self.state));
        (r.windows, r.elsewhere, r.deadline, r.cap)
    }

    /// A kids policy refused the target at open.
    pub fn denied(&self) -> bool {
        self.denied
    }

    /// The title left the kids profile's list (plans/m11-kids.md 4.4, 8):
    /// the resolve stops (the subtitle hash reader with it), the position is
    /// saved, the lease drops (its URLs end and bodies being served stop),
    /// no lease is adopted again, and every torrent the view started is
    /// held for this profile with the `generation` the denial was read at
    /// (paused at once unless another profile is playing it). A failover
    /// after this does nothing.
    pub async fn stop_forbidden(&self, generation: u64) {
        if let Some((_, task)) = lock(&self.task).take() {
            task.abort();
        }
        self.save();
        let started = {
            let mut s = lock(&self.state);
            if let Some(task) = s.segments_task.take() {
                task.abort();
            }
            // A resolve still running reads this under the same lock: it
            // adopts no lease and starts no torrent after this.
            s.closed = true;
            #[cfg(feature = "torrent")]
            s.closing.cancel();
            release_all(&mut s);
            self.snapshot.set(s.snapshot.clone());
            #[cfg(feature = "torrent")]
            let started = std::mem::take(&mut s.torrents);
            #[cfg(not(feature = "torrent"))]
            let started: Vec<()> = Vec::new();
            started
        };
        #[cfg(feature = "torrent")]
        if !started.is_empty()
            && let Ok(engine) = self.core.torrent_engine().await
        {
            for torrent in started {
                if let Err(e) = engine
                    .pause_now(torrent, self.core.profile(), generation)
                    .await
                {
                    tracing::warn!("pausing a forbidden torrent: {e}");
                }
            }
        }
        #[cfg(not(feature = "torrent"))]
        let _ = (started, generation);
    }

    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        PlaybackSnapshot,
        Pin<Box<dyn Stream<Item = Vec<PlaybackSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }

    /// The player could not open or keep playing the source `failed` (the
    /// snapshot's `attempt`; `None` means the current one): drop it and move
    /// to the next candidate. A late report about a source already given up
    /// on is ignored, so it never drops the one playing now, even when both
    /// have one `key`.
    ///
    /// `kind`, `status` and `error` are what the player reports; the attempt
    /// list gets the core's words for them (`mq_server::failure_words`).
    pub fn fail_over(
        &self,
        failed: Option<u64>,
        kind: Option<&str>,
        status: Option<u16>,
        error: &str,
    ) {
        let error = mq_server::failure_words(kind, status, error);
        let error = error.as_str();
        let (failed, pick, kept) = {
            let mut s = lock(&self.state);
            let other = failed.is_some_and(|failed| failed != s.snapshot.attempt);
            // A stopped view opens no other source (a debrid one would
            // reach the account for a title off the list).
            if s.snapshot.phase != PlaybackPhase::Ready || other || s.closed {
                return;
            }
            // The running task's work for this source ends here.
            s.run += 1;
            release_all(&mut s);
            let failed = s
                .next
                .checked_sub(1)
                .and_then(|i| s.candidates.get(i))
                .cloned();
            if let Some(stream) = &failed {
                s.snapshot.attempts.push(attempt(stream, error));
            }
            // The player could not decode it before it ever played (Apple
            // reports a frame that never came this way): the version's own
            // failure, and this device remembers it. A decode error after
            // good playback is not the version's (code review 2026-10-20
            // P2-2).
            let decode = kind == Some("decode") && !s.progressed;
            // An audio output that failed on the version's own bitstream (a
            // format this device passes through, E-AC-3 to a receiver that
            // will not take it) before the source played at all is the
            // version's for this playback: the next version takes another
            // audio codec, and nothing is kept. Decoded audio failing, or an
            // output failing once playing (a route change), is the moment's.
            let caps = s.pick.as_ref().map(|p| &p.caps);
            let passed = failed.as_ref().zip(caps).is_some_and(|(st, caps)| {
                st.release
                    .audio
                    .iter()
                    .any(|a| caps.passthrough.contains(a))
            });
            let audio = kind == Some("audio") && s.reported_ms.is_none() && passed;
            let mut follows = false;
            if decode || audio {
                s.version_failed = Some(NOT_HERE);
                s.failed_on = failed.as_ref().map(|st| {
                    let release = st.release.clone();
                    if decode {
                        FailedOn::Decode(release)
                    } else {
                        FailedOn::Audio(release)
                    }
                });
                // The version's other sources carry the same file, codecs
                // and all: when another version follows, it starts at once.
                follows = s.pick.as_ref().is_some_and(|p| {
                    p.recommended() && p.next_version(&s.tried, s.failed_on.as_ref()).is_some()
                });
                if follows {
                    s.next = s.candidates.len();
                }
            }
            note_failure(&mut s);
            s.snapshot.phase = PlaybackPhase::Resolving;
            s.snapshot.key = None;
            s.snapshot.moved = None;
            s.snapshot.subtitles_pending = false;
            clear_segments(&mut s);
            self.snapshot.set(s.snapshot.clone());
            // A decode failure is kept against the failed source, and against
            // every source of the version when they are skipped for the next
            // version (the same file). Another source that is tried and plays
            // must not read as one that did not.
            let kept: Vec<String> = match &failed {
                Some(_) if decode && follows => {
                    s.candidates.iter().map(|c| c.key.clone()).collect()
                }
                Some(stream) if decode => vec![stream.key.clone()],
                _ => Vec::new(),
            };
            (failed, s.pick.clone(), kept)
        };
        // Written and reported outside the playback's lock.
        if let Some(stream) = &failed {
            let now = mq_library::hlc::now_ms();
            for key in &kept {
                crate::speed::record_did_not_play(self.core.library(), key, now);
            }
            if let Some(pick) = &pick {
                pick.failed(&stream.key);
            }
        }
        self.spawn_resolve();
    }

    /// The user picked a subtitle track (`url`), or turned subtitles off
    /// (`None`): nothing is turned on for them again in this playback
    /// (`autoSubtitle` clears), and the title remembers the choice, the
    /// track's language or Off, for its next plays. A track's language also
    /// goes first in the profile's subtitle languages, so the next titles
    /// start with it; Off stays the title's own. A track inside the video
    /// file comes as its `language` code with no `url`, and counts as a
    /// core track of that language; a code the core does not know (`und`)
    /// only clears `autoSubtitle`. A kids profile's preferences stay as the
    /// owner left them (plans/m11-kids.md): its picks are its titles' own.
    pub fn subtitle_chosen(&self, url: Option<&str>, language: Option<&str>) {
        use crate::languages::{is_known, primary};
        let mut s = lock(&self.state);
        let choice = match (url, language) {
            (Some(url), _) => s
                .snapshot
                .subtitles
                .iter()
                .find(|t| t.url == url)
                .map(|t| t.language.clone()),
            (None, Some(code)) => {
                Some(crate::languages::language(code).tag).filter(|tag| is_known(primary(tag)))
            }
            (None, None) => Some(OFF.to_owned()),
        };
        s.subtitle_picked = true;
        if s.snapshot.auto_subtitle.take().is_some() {
            self.snapshot.set(s.snapshot.clone());
        }
        let binge_group = s.snapshot.binge_group.clone();
        drop(s);
        let Some(choice) = choice else {
            return;
        };
        if choice != OFF
            && !self.core.policy().kids()
            && let Some(next) = picked_first(&self.core.prefs().subtitle_languages, &choice)
        {
            let patch = serde_json::json!({ "subtitleLanguages": next });
            if let Err(e) = self.core.set_prefs(&patch) {
                tracing::warn!("subtitle language not kept: {e}");
            }
        }
        self.remember(binge_group, |memory| memory["subtitle"] = choice.into());
    }

    /// The viewer settled the delay of the core's track `url` at `ms` (the
    /// stepper's value, within a minute either way): the title remembers it
    /// for the playing version and video and that track, the one track it
    /// remembers a delay for, and the track's `delayMs` starts there on the
    /// next play. Zero forgets it.
    pub fn subtitle_delay(&self, url: &str, ms: i64) {
        let ms = ms.clamp(-MAX_DELAY_MS, MAX_DELAY_MS) as i32;
        let mut s = lock(&self.state);
        let Some(origin) = s
            .snapshot
            .subtitles
            .iter()
            .find(|t| t.url == url)
            .map(|t| t.origin.clone())
        else {
            return;
        };
        let Some(key) = s.snapshot.key.clone() else {
            return;
        };
        let ours = |d: &Option<(String, i32)>| d.as_ref().is_some_and(|(o, _)| *o == origin);
        // 0 for another track (the app sends the active one's on close)
        // forgets nothing.
        let delay = match ms {
            0 if ours(&s.delay) => None,
            0 => s.delay.clone(),
            ms => Some((origin.clone(), ms)),
        };
        // The same value again (the close resends it): no write, no sync.
        if delay == s.delay {
            return;
        }
        s.delay = delay.clone();
        for t in &mut s.snapshot.subtitles {
            t.delay_ms = delay_of(&delay, &t.origin);
        }
        self.snapshot.set(s.snapshot.clone());
        let binge_group = s.snapshot.binge_group.clone();
        drop(s);
        let video_id = self.target.video_id.clone();
        self.remember(binge_group, |memory| {
            let Some(memory) = memory.as_object_mut() else {
                return;
            };
            if ms == 0 {
                if memory.get(DELAY).and_then(|d| d["origin"].as_str()) == Some(origin.as_str()) {
                    memory.remove(DELAY);
                }
            } else {
                // Nothing is baked into a track yet: its ratio is 1.
                let entry = serde_json::json!({
                    "key": key, "videoId": video_id, "origin": origin,
                    "ratio": 1.0, "offsetMs": ms,
                });
                memory.insert(DELAY.into(), entry);
            }
        });
    }

    /// Rewrites the title's stream memory with `change`.
    fn remember(&self, binge_group: Option<String>, change: impl FnOnce(&mut serde_json::Value)) {
        let library = self.core.library();
        let mut memory: serde_json::Value = library
            .stream_memory(self.core.profile(), &self.target.item_id)
            .ok()
            .flatten()
            .and_then(|m| serde_json::from_str(&m).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        change(&mut memory);
        if let Err(e) = library.set_stream_memory(
            self.core.profile(),
            &self.target.item_id,
            binge_group.as_deref(),
            &memory.to_string(),
            self.core.clock(),
        ) {
            tracing::warn!("subtitle memory not written: {e}");
        }
    }

    /// The player knows the duration of the version `key`: look its skip
    /// segments up. Reports for another version, durations under a minute or
    /// over six hours, and repeats within 2 percent are ignored; a larger
    /// correction looks up again.
    pub fn report_duration(&self, key: &str, duration_ms: u64) {
        if !(60_000..=6 * 3_600_000).contains(&duration_ms) {
            return;
        }
        let t = &self.target;
        let Some(target) = Target::from_ids(&t.kind, &t.item_id, &t.video_id) else {
            return;
        };
        let mut s = lock(&self.state);
        {
            if s.snapshot.phase != PlaybackPhase::Ready || s.snapshot.key.as_deref() != Some(key) {
                return;
            }
            if let Some((source, known)) = s.lookup
                && source == s.source
                && known.abs_diff(duration_ms) * 50 <= known
            {
                return;
            }
        }
        #[cfg(feature = "torrent")]
        if let Some((torrent, file, len)) = s.rate_of {
            let core = self.core.clone();
            let metered = core.network() == crate::speed::Network::Cellular;
            tokio::spawn(async move {
                if let Ok(engine) = core.torrent_engine().await {
                    let rate = len * 1000 / duration_ms;
                    let _ = engine.set_rate(torrent, file, rate, metered).await;
                }
            });
        }
        let lookup = (s.source, duration_ms);
        s.lookup = Some(lookup);
        if !s.snapshot.segments.is_empty() || s.snapshot.next_at_ms.is_some() {
            s.snapshot.segments.clear();
            s.snapshot.next_at_ms = None;
            self.snapshot.set(s.snapshot.clone());
        }
        // Spawning does not block, so it happens under the lock.
        let task = tokio::spawn(find_segments(
            self.core.clone(),
            self.state.clone(),
            self.snapshot.clone(),
            target,
            lookup,
        ));
        if let Some(old) = s.segments_task.replace(task) {
            old.abort();
        }
    }

    /// Player position report, `paused` when the player is paused. Saved at
    /// most every few seconds; the last report is always saved when the
    /// view closes. Trakt gets start and pause from it.
    pub fn report_progress(&self, position_ms: u64, duration_ms: u64, paused: bool) {
        // No progress row for a title off the kids list.
        if self.denied {
            return;
        }
        if let Some(trakt) = &self.trakt {
            trakt.progress(position_ms, duration_ms, paused);
        }
        let due = {
            let mut s = lock(&self.state);
            let step = s
                .reported_ms
                .filter(|&before| position_ms > before)
                .map(|before| position_ms - before)
                .filter(|&step| step <= PLAYED_STEP.as_millis() as u64);
            if let Some(step) = step {
                s.played_ms += step;
            }
            s.advancing = step.is_some() && !paused;
            s.reported_ms = Some(position_ms);
            if s.played_ms >= PLAYED_PAST_START.as_millis() as u64 {
                s.progressed = true;
                // A later move begins a start of its own.
                s.start_at = None;
                s.tries = 0;
                s.snapshot.start_budget_ends_at = None;
                s.failed_as = None;
            }
            s.last = Some((position_ms, duration_ms));
            s.dirty = true;
            let changed = s.paused != paused;
            s.paused = paused;
            #[cfg(feature = "torrent")]
            if changed {
                // A paused window keeps filling its readahead
                // (plans/streaming-window.md 2.3).
                if let Some((torrent, ..)) = s.rate_of {
                    let core = self.core.clone();
                    tokio::spawn(async move {
                        if let Ok(engine) = core.torrent_engine().await {
                            let _ = engine.set_paused(torrent, paused).await;
                        }
                    });
                }
            }
            #[cfg(not(feature = "torrent"))]
            let _ = changed;
            s.saved_at.is_none_or(|t| t.elapsed() >= SAVE_EVERY)
        };
        if due {
            self.save();
        }
    }

    /// The player waits for data (`on`), or plays again, at `position_ms`
    /// (plans/play-every-version.md 2e). A stall that lasts `STALL_MOVE`
    /// moves to the ranking's next version, the position carried over, when
    /// it did not follow a seek or a pause, the version is the
    /// recommendation and not the Resume release, this play has not moved
    /// for a stall yet, and the network is reachable (a lost one waits).
    /// Otherwise "Choose another version" shows over the stalled frame.
    /// Only transitions count: a repeated `stalled(true)` while the same
    /// stall lasts keeps its clock.
    pub fn stalled(&self, on: bool, position_ms: u64) {
        let mut s = lock(&self.state);
        if on && s.stall_on == Some(s.stall) {
            return;
        }
        s.stall += 1;
        if !on || s.snapshot.phase != PlaybackPhase::Ready || s.closed {
            if s.snapshot.choose_another && s.snapshot.phase == PlaybackPhase::Ready {
                s.snapshot.choose_another = false;
                self.snapshot.set(s.snapshot.clone());
            }
            return;
        }
        // A stall right after a seek or a pause waits for data it had no
        // reason to have (bench EW-3: 17.2 s after a seek).
        let steady = s.advancing
            && s.reported_ms.is_some_and(|was| {
                position_ms >= was && position_ms - was <= PLAYED_STEP.as_millis() as u64
            });
        s.stall_on = Some(s.stall);
        let stall = Stalled {
            generation: s.stall,
            at: position_ms,
            steady,
        };
        drop(s);
        let mover = Mover {
            core: self.core.clone(),
            target: self.target.clone(),
            plays: self.plays.clone(),
            state: Arc::downgrade(&self.state),
            snapshot: self.snapshot.clone(),
            task: self.task.clone(),
        };
        tokio::spawn(watch_stall(mover, stall));
    }

    fn save(&self) {
        let (position_ms, duration_ms, first) = {
            let mut s = lock(&self.state);
            let Some((pos, dur)) = s.last.filter(|_| s.dirty) else {
                return;
            };
            let first = s.saved_at.is_none();
            s.dirty = false;
            s.saved_at = Some(Instant::now());
            (pos, dur, first)
        };
        let saved = save_progress(
            &self.core,
            &self.target,
            position_ms,
            duration_ms,
            first,
            self.trakt.as_ref(),
        );
        if let Err(e) = saved {
            tracing::warn!("progress save failed: {e}");
        }
    }

    fn spawn_resolve(&self) {
        spawn_resolve(
            &self.core,
            &self.target,
            &self.plays,
            &self.state,
            &self.snapshot,
            &self.task,
        );
    }
}

/// The view's resolve task and its run.
type TaskSlot = Mutex<Option<(u64, JoinHandle<()>)>>;

/// A new resolve run of the view, installed as its task.
fn spawn_resolve(
    core: &Core,
    target: &Arc<PlayTarget>,
    plays: &Arc<[Format]>,
    state: &Arc<Mutex<State>>,
    snapshot: &SharedObservable<PlaybackSnapshot>,
    slot: &TaskSlot,
) {
    let run = {
        let mut s = lock(state);
        s.run += 1;
        s.run
    };
    let task = tokio::spawn(resolve_next(
        core.clone(),
        target.clone(),
        plays.clone(),
        state.clone(),
        snapshot.clone(),
        run,
    ));
    install(slot, run, task);
}

/// A playing source's stall must last this long before it moves on, or
/// before "Choose another version" shows over the frame (2e).
const STALL_MOVE: Duration = Duration::from_secs(30);
/// The network is reachable when a peer of the playing torrent is
/// connected, or a source answers within this.
const REACH: Duration = Duration::from_secs(5);
/// Why a version was left mid-play, in the words of `moved`.
const STALLED: &str = "stopped loading here";

/// One stall: its generation (`State::stall`), the position it began at,
/// and whether it came during steady playback.
#[derive(Clone, Copy)]
struct Stalled {
    generation: u64,
    at: u64,
    steady: bool,
}

/// What a stall's watcher needs of the view to move it on.
struct Mover {
    core: Core,
    target: Arc<PlayTarget>,
    plays: Arc<[Format]>,
    state: std::sync::Weak<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    task: Arc<TaskSlot>,
}

/// After `STALL_MOVE`, a stall still on moves to the next version when it
/// may (`PlaybackView::stalled`) and the network is reachable; while the
/// network is lost it asks again every `REACH`. A stall that may not move
/// shows "Choose another version" instead.
async fn watch_stall(m: Mover, stall: Stalled) {
    tokio::time::sleep(STALL_MOVE).await;
    loop {
        let Some(state) = m.state.upgrade() else {
            return;
        };
        let (stream, torrent) = {
            let mut s = lock(&state);
            if s.stall != stall.generation || s.closed || s.snapshot.phase != PlaybackPhase::Ready {
                return;
            }
            let playing = s
                .next
                .checked_sub(1)
                .and_then(|i| s.candidates.get(i))
                .cloned();
            let moves = stall.steady
                && !s.stall_moved
                && playing
                    .as_ref()
                    .is_some_and(|p| s.resume_key.as_ref() != Some(&p.key))
                && s.pick
                    .as_ref()
                    .is_some_and(|p| p.recommended() && p.next_version(&s.tried, None).is_some());
            let Some(stream) = playing.filter(|_| moves) else {
                if !s.snapshot.choose_another {
                    s.snapshot.choose_another = true;
                    m.snapshot.set(s.snapshot.clone());
                }
                return;
            };
            #[cfg(feature = "torrent")]
            let torrent = s.rate_of.map(|(t, ..)| t);
            #[cfg(not(feature = "torrent"))]
            let torrent = ();
            (stream, torrent)
        };
        if reachable(&m.core, &stream, torrent).await {
            move_stalled(&m, &state, stall);
            return;
        }
        // A lost network waits; the viewer may choose meanwhile.
        {
            let mut s = lock(&state);
            if s.stall == stall.generation && !s.snapshot.choose_another {
                s.snapshot.choose_another = true;
                m.snapshot.set(s.snapshot.clone());
            }
        }
        drop(state);
        tokio::time::sleep(REACH).await;
    }
}

/// The network works: a peer of the playing torrent is connected, or the
/// source, one of its HTTP trackers or its addon answers within `REACH`
/// (any status is an answer).
async fn reachable(
    core: &Core,
    stream: &ParsedStream,
    #[cfg(feature = "torrent")] torrent: Option<mq_engine::TorrentHandle>,
    #[cfg(not(feature = "torrent"))] _torrent: (),
) -> bool {
    #[cfg(feature = "torrent")]
    if let Some(torrent) = torrent
        && let Ok(engine) = core.torrent_engine().await
        && engine.stats(torrent).await.is_ok_and(|s| s.peers > 0)
    {
        return true;
    }
    let mut urls: Vec<String> = Vec::new();
    match &stream.source {
        Source::Http { url } => urls.push(url.to_string()),
        Source::Torrent { trackers, .. } => urls.extend(
            trackers
                .iter()
                .map(|t| t.trim_start_matches("tracker:"))
                .filter(|t| t.starts_with("http"))
                .map(str::to_owned),
        ),
        _ => {}
    }
    urls.extend(stream.addon_url.as_ref().map(ToString::to_string));
    let opts = mq_http::RequestOptions {
        timeout: REACH,
        allow_private: true,
        ..Default::default()
    };
    for url in urls.iter().take(3) {
        let asked = tokio::time::timeout(REACH, core.http().get_range(url, 0, 1, &opts)).await;
        if let Ok(
            Ok(_) | Err(mq_http::HttpError::Status { .. } | mq_http::HttpError::TooLarge { .. }),
        ) = asked
        {
            return true;
        }
    }
    false
}

/// The stalled version is left for the ranking's next one, at the stall's
/// position, once in this play.
fn move_stalled(m: &Mover, state: &Arc<Mutex<State>>, stall: Stalled) {
    let (stalled, pick) = {
        let mut s = lock(state);
        if s.stall != stall.generation || s.closed || s.snapshot.phase != PlaybackPhase::Ready {
            return;
        }
        s.stall_moved = true;
        s.run += 1;
        release_all(&mut s);
        let stalled = s
            .next
            .checked_sub(1)
            .and_then(|i| s.candidates.get(i))
            .cloned();
        if let Some(stream) = &stalled {
            s.snapshot
                .attempts
                .push(attempt(stream, "The video stopped loading"));
        }
        s.version_failed = Some(STALLED);
        // The version's other sources carry the same swarm or link: the
        // next version follows at once.
        s.next = s.candidates.len();
        s.snapshot.resume_ms = Some(stall.at);
        s.snapshot.phase = PlaybackPhase::Resolving;
        s.snapshot.key = None;
        s.snapshot.moved = None;
        s.snapshot.subtitles_pending = false;
        s.snapshot.choose_another = false;
        clear_segments(&mut s);
        m.snapshot.set(s.snapshot.clone());
        (stalled, s.pick.clone())
    };
    // Not offered first again: a swarm that stopped sending with the
    // network up is remembered as stalled here for a day (its line says
    // so), and the open Versions view lets its frozen pick go.
    if let Some(stream) = &stalled {
        if let Source::Torrent { info_hash, .. } = &stream.source {
            remember_dead(&m.core, vec![info_hash.clone()]);
        }
        if let Some(pick) = &pick {
            pick.failed(&stream.key);
        }
    }
    spawn_resolve(&m.core, &m.target, &m.plays, state, &m.snapshot, &m.task);
}

/// Keeps `task` as the view's resolve task unless a newer run's task is
/// already there: two callers racing between spawning and installing never
/// leave the older task running in place of the newer.
fn install(slot: &TaskSlot, run: u64, task: JoinHandle<()>) {
    let mut slot = lock(slot);
    if slot.as_ref().is_some_and(|(newer, _)| *newer > run) {
        task.abort();
    } else if let Some((_, old)) = slot.replace((run, task)) {
        old.abort();
    }
}

impl Drop for PlaybackView {
    fn drop(&mut self) {
        if let Some((_, task)) = lock(&self.task).take() {
            task.abort();
        }
        if let Some(task) = lock(&self.state).segments_task.take() {
            task.abort();
        }
        self.save();
        // A task still running after the abort can never adopt a lease now.
        let sample = {
            let mut s = lock(&self.state);
            let sample = take_http_sample(&mut s);
            s.closed = true;
            #[cfg(feature = "torrent")]
            s.closing.cancel();
            release_all(&mut s);
            sample
        };
        if let Some(sample) = sample {
            record_http_sample(&self.core, sample);
        }
    }
}

/// A new source: the old one's skip segments do not apply to it.
fn clear_segments(s: &mut State) {
    s.source += 1;
    s.lookup = None;
    if let Some(task) = s.segments_task.take() {
        task.abort();
    }
    s.snapshot.segments.clear();
    s.snapshot.next_at_ms = None;
}

/// Finds skip segments for one source and duration: the release-matched
/// source first, published at once, then the gap filler. When the
/// release-matched source cannot be asked (a timeout, a refusal), the filler
/// goes ahead and that source is asked again after each of the finder's
/// waits, so one slow answer does not cost the whole playback its Skip; what
/// it finds later takes precedence over the filler's. Results for a source or
/// duration that is no longer current are dropped, and the task ends with the
/// source.
async fn find_segments(
    core: Core,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    target: Target,
    lookup: (u64, u64),
) {
    let publish = |change: &dyn Fn(&mut PlaybackSnapshot)| {
        let mut s = lock(&state);
        if s.lookup == Some(lookup) {
            change(&mut s.snapshot);
            snapshot.set(s.snapshot.clone());
        }
    };
    let duration_ms = lookup.1;
    let finder = core.segments();
    let fill = |have: Vec<Segment>| {
        let finder = &finder;
        let target = &target;
        async move {
            let extra =
                tokio::time::timeout(SEGMENTS_DEADLINE, finder.fill(target, duration_ms, &have))
                    .await
                    .unwrap_or_default();
            if !extra.is_empty() {
                publish(&|s| s.segments = segments::merge(&s.segments, extra.clone()));
            }
        }
    };
    let mut filled = false;
    let waits = std::iter::once(Duration::ZERO).chain(finder.retries());
    for wait in waits {
        tokio::time::sleep(wait).await;
        let asked =
            tokio::time::timeout(SEGMENTS_DEADLINE, finder.primary(&target, duration_ms)).await;
        match asked {
            Ok(Some(found)) => {
                if !found.segments.is_empty() {
                    publish(&|s| {
                        s.segments = segments::merge(&found.segments, s.segments.clone());
                        s.next_at_ms = found.next_at_ms;
                    });
                }
                if !filled {
                    fill(found.segments).await;
                }
                return;
            }
            _ if !filled => {
                filled = true;
                fill(Vec::new()).await;
            }
            _ => {}
        }
    }
}

/// Ends the stream's and the subtitles' loopback URLs; they stop working.
/// Under the view's lock, which the lease's drop follows with the server's:
/// never the reverse.
fn release_all(s: &mut State) {
    // Any stall was the released source's: its watcher stands down.
    s.stall += 1;
    s.snapshot.url = None;
    s.snapshot.manifest = false;
    s.snapshot.container = None;
    s.snapshot.audio.clear();
    s.snapshot.engine = None;
    s.snapshot.subtitles.clear();
    s.snapshot.subtitle_notes.clear();
    s.snapshot.auto_subtitle = None;
    // A pick was of this source's tracks: the next one's settle may turn on
    // the title's remembered choice again.
    s.subtitle_picked = false;
    s.start_settled = false;
    s.order.clear();
    s.lease = None;
}

/// The container of the file a source opened (plans/apple-second-engine.md
/// 2.1): the opened file's name as the core parses it, else what an
/// earlier play of this version opened (`remembered`), else the release's.
/// A torrent pack's file says what its release cannot ("Movie.mp4" in a
/// release listed as mp4 can be an mkv).
fn playing_container(
    file: Option<&str>,
    remembered: impl FnOnce() -> Option<String>,
    release: Option<&str>,
) -> Option<String> {
    file.and_then(|f| mq_relparse::parse(f.rsplit('/').next().unwrap_or(f)).container)
        .or_else(remembered)
        .or_else(|| release.map(str::to_owned))
}

/// A release's audio formats in the core's serde names ("dts_hd_ma").
fn audio_names(audio: &[mq_relparse::Audio]) -> Vec<String> {
    audio
        .iter()
        .filter_map(|a| serde_json::to_value(a).ok()?.as_str().map(str::to_owned))
        .collect()
}

/// Whether `run` still owns the view's source and tracks.
fn current(s: &State, run: u64) -> bool {
    !s.closed && s.run == run
}

/// A source failed (its attempt just listed): what it says of the start,
/// for `empty`. A refusal for room reads noRoom, a failure of the version's
/// own on this device nothingPlaysHere, a swarm that did not start
/// notStarted, while every failure says the same; anything else (a source
/// that could not be reached) reads unreachable.
fn note_failure(s: &mut State) {
    let no_room = (s.snapshot.attempts.last()).is_some_and(|a| a.error == no_room_line());
    let kind = if no_room {
        EmptyKind::NoRoom
    } else if s.version_failed == Some(NOT_HERE) {
        EmptyKind::NothingPlaysHere
    } else if s.version_failed == Some(NOT_STARTED) {
        EmptyKind::NotStarted
    } else {
        EmptyKind::Unreachable
    };
    use EmptyKind::{NoRoom, NotStarted, Unreachable};
    s.failed_as = Some(match (s.failed_as, kind) {
        // Room refused some, the others did not start: they did not start.
        (Some(NoRoom), NotStarted) | (Some(NotStarted), NoRoom) => NotStarted,
        (Some(was), kind) if was != kind => Unreachable,
        (_, kind) => kind,
    });
}

/// The budget ran out with nothing playing.
fn out_of_budget(s: &mut State) {
    // A hand pick keeps its offer (code review 2026-10-20 r4 P3-3):
    // move_on only sets it for one.
    if s.pick.as_ref().is_some_and(|p| !p.recommended()) {
        move_on(s);
    }
    // A first play's attempts say what failed; a "Try again" run says it
    // gave up.
    if s.retry_until.is_some() {
        s.snapshot.attempts.push(Attempt {
            key: String::new(),
            via: "Marquee".into(),
            error: RETRY_GAVE_UP.into(),
        });
    }
    // "This didn't start".
    s.failed_as = Some(EmptyKind::NotStarted);
    failed(s);
}

/// Waits until `at`; forever with none.
async fn until(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Nothing played: the failure card, with why.
fn failed(s: &mut State) {
    s.snapshot.phase = PlaybackPhase::Failed;
    s.snapshot.start = None;
    s.snapshot.choose_another = false;
    s.snapshot.start_budget_ends_at = None;
    s.snapshot.empty = Some(s.failed_as.unwrap_or(EmptyKind::Unreachable));
}

/// The version did not play here, for a reason of its own.
fn not_here(state: &Mutex<State>) {
    lock(state).version_failed = Some(NOT_HERE);
}

/// The version did not start here (a swarm); a reason it did not play
/// here, found first, stays.
#[cfg(feature = "torrent")]
fn failed_to_start(s: &mut State) {
    s.version_failed.get_or_insert(NOT_STARTED);
}

/// Keeps `lease` as the playing source's, only while the view is open and
/// `run` is still the newest; else it is dropped here, which ends its token.
fn adopt(s: &mut State, run: u64, lease: Lease) -> bool {
    if !current(s, run) {
        return false;
    }
    s.snapshot.url = Some(lease.url.clone());
    s.snapshot.manifest = lease.format.is_some();
    s.snapshot.attempt += 1;
    // A new source: no stall of an earlier one carries over.
    s.stall += 1;
    s.lease = Some(lease);
    // A source plays: what failed before it says nothing about a later
    // failure, and the move on that led here is over (code review
    // 2026-10-20 P3: a later source failover is not a next version).
    s.version_failed = None;
    s.progressed = false;
    s.reported_ms = None;
    s.played_ms = 0;
    // A source plays: the "Try again" run is over, its patience and its
    // budget with it (code review 2026-10-20 r4 P3-2).
    s.retry_until = None;
    s.patient = false;
    s.snapshot.trying = None;
    s.snapshot.choose_another = false;
    s.failed_on = None;
    s.snapshot.moved = s.left_because.and_then(|why| {
        let playing = s.candidates.get(s.next.checked_sub(1)?)?;
        let announced = s.first.first()?;
        let (now, then) = (
            version_label(&playing.release),
            version_label(&announced.release),
        );
        Some(moved_words(&now, &then, why))
    });
    true
}

/// `moved`: the version playing (`now`), the one announced (`then`) and
/// why it was left.
fn moved_words(now: &str, then: &str, why: &str) -> String {
    if now == then {
        format!("Playing another {now}: the first one {why}")
    } else {
        format!("Playing {now}: the {then} {why}")
    }
}

/// Swarms that failed to start in one playback's chain, remembered as dead
/// only once something in the chain proved the connection works (code
/// review r1 P2-5, P3): a swarm that unchoked us and sent nothing proves
/// it itself; a metadata timeout waits for another candidate's peer or
/// source.
#[derive(Debug, Default)]
struct Dead {
    reached: bool,
    pending: Vec<String>,
}

impl Dead {
    /// `hash` did not start; `proven`: a peer of its own unchoked us.
    /// The hashes to remember now.
    #[cfg_attr(not(feature = "torrent"), allow(dead_code))]
    fn failed(&mut self, hash: &str, proven: bool) -> Vec<String> {
        self.pending.push(hash.to_ascii_lowercase());
        if proven {
            self.reached = true;
        }
        self.flush()
    }

    /// Something in the chain reached a peer or a source.
    fn reached(&mut self) -> Vec<String> {
        self.reached = true;
        self.flush()
    }

    fn flush(&mut self) -> Vec<String> {
        if self.reached {
            std::mem::take(&mut self.pending)
        } else {
            Vec::new()
        }
    }
}

/// Remembers `hashes` as stalled on this device for a day.
fn remember_dead(core: &Core, hashes: Vec<String>) {
    let now = mq_library::hlc::now_ms();
    for hash in hashes {
        crate::speed::record_dead(core.library(), &hash, now);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn attempt(stream: &ParsedStream, error: &str) -> Attempt {
    Attempt {
        key: stream.key.clone(),
        via: stream
            .service
            .clone()
            .unwrap_or_else(|| stream.addon_name.clone()),
        error: error.to_owned(),
    }
}

/// An engine error as the viewer reads it: its refusals for room (a window
/// that does not fit, one that ended its playback) are the one storage line
/// (plans/play-every-version.md 2c), the reason kept in the log. Read from
/// the error's type, so a service's own words about its storage stay its.
#[cfg(feature = "torrent")]
fn engine_words(e: &mq_engine::EngineError) -> String {
    use mq_engine::EngineError;
    match e {
        EngineError::DeviceFull | EngineError::Storage(_) => {
            tracing::info!(error = %e, "no room to play");
            no_room_line().to_owned()
        }
        e => e.to_string(),
    }
}

/// `empty.noRoom.text` from stream-choice-words.json.
fn no_room_line() -> &'static str {
    static LINE: OnceLock<String> = OnceLock::new();
    LINE.get_or_init(|| {
        let words: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../apps/shared/stream-choice-words.json"
        ))
        .expect("the shared words");
        words["empty"]["noRoom"]["text"]
            .as_str()
            .expect("empty.noRoom.text")
            .to_owned()
    })
}

/// Saves a position. At the end it marks the video watched, with the write
/// a Trakt session holds for it in the same transaction.
fn save_progress(
    core: &Core,
    target: &PlayTarget,
    position_ms: u64,
    duration_ms: u64,
    first: bool,
    trakt: Option<&crate::trakt::Scrobble>,
) -> Result<(), crate::CoreError> {
    let (library, profile) = (core.library(), core.profile());
    // Continue Watching needs a name and poster for titles outside the library.
    if first
        && let Some(name) = &target.name
        && !library.is_in_library(profile, &target.item_id)?
    {
        let row = LibraryItemRow {
            item_id: target.item_id.clone(),
            kind: target.kind.clone(),
            name: name.clone(),
            poster: target.poster.clone(),
            poster_shape: None,
            added_ms: mq_library::hlc::now_ms() as i64,
        };
        // Only a cache of what was played: any real library edit wins.
        library.upsert_item(profile, &row, false, &core.zero_hlc())?;
    }
    library.set_progress(
        profile,
        ProgressRow {
            video_id: &target.video_id,
            item_id: &target.item_id,
            position_ms: position_ms as i64,
            duration_ms: duration_ms as i64,
            updated_ms: mq_library::hlc::now_ms() as i64,
        },
        core.clock(),
    )?;
    if duration_ms > 0 && position_ms as f64 >= duration_ms as f64 * FINISHED_RATIO {
        let account = trakt.and_then(|t| t.to_hold(core));
        let hold = trakt
            .zip(account.as_deref())
            .map(|(t, a)| t.intent(profile, a));
        let holds = hold.is_some();
        library.set_watched_holding(
            profile,
            &target.item_id,
            &target.video_id,
            core.clock(),
            hold,
        )?;
        if let Some(trakt) = trakt.filter(|_| holds) {
            trakt.held();
        }
    }
    Ok(())
}

/// Tries candidates from `state.next` until one answers, then publishes its
/// loopback URL; publishes `Failed` when none is left.
async fn resolve_next(
    core: Core,
    target: Arc<PlayTarget>,
    plays: Arc<[Format]>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    run: u64,
) {
    let server = match core.stream_server().await {
        Ok(server) => server,
        Err(e) => {
            let mut s = lock(&state);
            s.snapshot.attempts.push(Attempt {
                key: String::new(),
                via: "Marquee".into(),
                error: format!("local stream server did not start: {e}"),
            });
            failed(&mut s);
            snapshot.set(s.snapshot.clone());
            return;
        }
    };
    let hvc1_only = lock(&state).pick.as_ref().is_some_and(|p| p.caps.hvc1_only);
    loop {
        let stream = {
            let mut s = lock(&state);
            if s.closed {
                return;
            }
            if budget_left(&s).is_some_and(|left| left.is_zero()) {
                out_of_budget(&mut s);
                snapshot.set(s.snapshot.clone());
                return;
            }
            if s.next >= s.candidates.len() && move_on(&mut s) {
                snapshot.set(s.snapshot.clone());
            }
            let Some(stream) = s.candidates.get(s.next).cloned() else {
                failed(&mut s);
                snapshot.set(s.snapshot.clone());
                return;
            };
            s.next += 1;
            // A first play's attempt: the first one's add starts the budget.
            if !s.patient {
                if s.start_at.is_none() {
                    s.start_at = Some(tokio::time::Instant::now());
                    let ends = mq_library::hlc::now_ms() + START_BUDGET.as_millis() as u64;
                    s.snapshot.start_budget_ends_at = Some(ends);
                }
                s.tries += 1;
            }
            let swarm = matches!(stream.source, Source::Torrent { .. })
                && by_engine(&stream, core.debrid().is_some(), core.has_engine());
            begin_start(
                &mut s,
                if swarm {
                    StartStage::Details
                } else {
                    StartStage::Connecting
                },
            );
            snapshot.set(s.snapshot.clone());
            (stream, swarm)
        };
        let (stream, swarm) = stream;
        let work = async {
            match open_source(&core, &stream, &target, (&state, &snapshot, run)).await {
                Ok((upstream, filename, size)) => {
                    // The first byte comes through the server now (review
                    // P3-8): the fill and its words do not sit at the last
                    // count meanwhile.
                    show_opening(&state, &snapshot, run);
                    let name = filename
                        .clone()
                        .unwrap_or_else(|| format!("{}.mkv", target.video_id));
                    let probe = within_budget(&state, PROBE_TIMEOUT);
                    let open = server.open_for(upstream.clone(), &name, probe, &plays);
                    tokio::pin!(open);
                    // A link has no samples of its own: its words come when it
                    // has taken long, as a torrent's do (LONG_STAGE).
                    let long = lock(&state).source_since + LONG_STAGE;
                    let open = tokio::select! {
                        open = &mut open => open,
                        () = tokio::time::sleep_until(long) => {
                            show_opening(&state, &snapshot, run);
                            open.await
                        }
                    };
                    match open {
                        Ok(lease) => shows_picture(&core, &stream, &lease, hvc1_only, &state)
                            .await
                            .map(|()| (upstream, filename, size, lease)),
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            }
        };
        tokio::pin!(work);
        // One clock for every kind of source (review 2026-10-23 P2-2):
        // "Choose another version" at `CHOOSE_AFTER`, and the budget's end.
        // A torrent's own watch ends at the budget's end and closes its
        // swarm; every other source ends here.
        let opened = loop {
            let (end, choose) = {
                let s = lock(&state);
                let end = budget_left(&s)
                    .filter(|_| !swarm)
                    .map(|left| tokio::time::Instant::now() + left);
                let choose = (!s.snapshot.choose_another).then_some(s.loading_since + CHOOSE_AFTER);
                (end, choose)
            };
            tokio::select! {
                opened = &mut work => break opened,
                () = until(end) => {
                    let mut s = lock(&state);
                    if current(&s, run) {
                        out_of_budget(&mut s);
                        snapshot.set(s.snapshot.clone());
                    }
                    return;
                }
                () = until(choose) => {
                    let mut s = lock(&state);
                    if current(&s, run) && s.snapshot.phase == PlaybackPhase::Resolving {
                        s.snapshot.choose_another = true;
                        snapshot.set(s.snapshot.clone());
                    }
                }
            }
        };
        match opened {
            Ok((upstream, filename, size, lease)) => {
                // Read once: a pick while the list fills reorders nothing
                // under it (the built-in source, the addons and the frame
                // rate rank alike).
                let wanted = preferred_languages(&core, &state);
                // The stream's own tracks come from its addon.
                let addon_url = stream.addon_url.as_ref();
                let slot = lease.subtitles();
                listen_for_trouble(&slot, Arc::downgrade(&state), snapshot.clone(), run);
                let reader = lease.reader();
                // Read before the playback's lock.
                let own_tracks: Vec<SubtitleTrack> = stream
                    .subtitles
                    .iter()
                    .filter_map(|w| {
                        let url = slot.add(w.url.as_str(), &w.lang, addon_url)?;
                        let title_forced = forced_mark(stream_name(&stream));
                        Some(track(
                            w,
                            &stream.addon_name,
                            &stream.addon_id,
                            url,
                            title_forced,
                        ))
                    })
                    .collect();
                let delay = remembered_delay(&core, &target, &stream.key);
                let proven = {
                    let mut s = lock(&state);
                    if !adopt(&mut s, run, lease) {
                        return;
                    }
                    let proven = s.dead.reached();
                    if let Some(pick) = &s.pick {
                        pick.ready();
                    }
                    clear_segments(&mut s);
                    s.snapshot.phase = PlaybackPhase::Ready;
                    s.snapshot.start = None;
                    s.playing = OwnedPlaying::of_stream(&stream);
                    s.delay = delay;
                    s.snapshot.key = Some(stream.key.clone());
                    s.snapshot.filename = filename.clone();
                    s.snapshot.size_bytes = size.or(stream.size_bytes);
                    let played_for = (target.season, target.episode);
                    s.snapshot.container = playing_container(
                        filename.as_deref(),
                        || crate::speed::container_of(core.library(), &stream.key, played_for),
                        stream.release.container.as_deref(),
                    );
                    s.snapshot.audio = audio_names(&stream.release.audio);
                    s.snapshot.engine =
                        crate::speed::engine_of(core.library(), &stream.key, played_for);
                    s.snapshot.subtitles = own_tracks;
                    let own: Vec<String> =
                        s.snapshot.subtitles.iter().map(|t| t.url.clone()).collect();
                    for (rank, url) in own.into_iter().enumerate() {
                        s.order.insert(url, (0, rank));
                    }
                    reorder(&mut s, &wanted);
                    s.snapshot.subtitles_pending = true;
                    s.snapshot.binge_group = stream.binge_group.clone();
                    snapshot.set(s.snapshot.clone());
                    proven
                };
                if !proven.is_empty() {
                    remember_dead(&core, proven);
                    let pick = lock(&state).pick.clone();
                    if let Some(pick) = pick {
                        pick.relearn();
                    }
                }
                if matches!(upstream, Upstream::Http { .. }) {
                    let attempt = lock(&state).snapshot.attempt;
                    spawn_http_sample(&core, &state, attempt);
                }
                // The torrent file it plays, for "On this device".
                let played = match &upstream {
                    #[cfg(feature = "torrent")]
                    Upstream::Torrent { file, .. } => size.map(|len| (*file, len)),
                    _ => None,
                };
                remember_stream(&core, &target, &stream, played);
                // The addons ask with the hash; the built-in source lists at
                // once and ranks with the frame rate. Pending ends with both.
                let subtitles = async {
                    let installed = core.installed_addons().unwrap_or_default();
                    let reader = video_reader(&upstream, reader).await;
                    let reader = reader.as_deref();
                    // Read once, for every track's release match (the
                    // frame rate) and the audio language.
                    let probed = probe_video(reader).shared();
                    // The measured rate counts in the order as soon as it is
                    // read, whichever source answers first.
                    let rated_done = tokio::sync::Notify::new();
                    let rated = async {
                        let fps = probed.clone().await.fps.map(|fps| fps as f32);
                        {
                            let mut s = lock(&state);
                            // The one writer of the rate.
                            if fps.is_some() && current(&s, run) {
                                s.playing.fps = fps;
                                reorder(&mut s, &wanted);
                                snapshot.set(s.snapshot.clone());
                            }
                        }
                        // The start pick ranks with the rate in.
                        rated_done.notify_one();
                    };
                    let addons = async {
                        let extra = video_extra(reader, filename.as_deref()).await;
                        let found = (&slot, run, &*state, &snapshot);
                        find_subtitles(&core, &target, &installed, extra, &wanted, found).await
                    };
                    let built_in = built_in_tracks(
                        &core,
                        &target,
                        &stream,
                        &installed,
                        probed.clone(),
                        &wanted,
                        &slot,
                        run,
                        &state,
                        &snapshot,
                    );
                    // The start subtitle waits for every source until
                    // START_SUBTITLE_WAIT and the rate, then settles as soon
                    // as a track in the first language is listed: one slow
                    // addon (SubSense took 12 to 20 s, the iphone-a walk) never
                    // leaves subtitles off that long.
                    let early = async {
                        tokio::time::sleep(START_SUBTITLE_WAIT).await;
                        rated_done.notified().await;
                        let wants = StartWants::read(
                            &core,
                            &target,
                            audio_of(probed.clone().await, &stream),
                        );
                        // Nothing to settle early with no first language.
                        // ponytail: looks at the list every 100 ms; a signal
                        // from each source if those 100 ms ever matter.
                        if wants.first().is_some() {
                            while !settle_start(&state, &snapshot, run, &wants, false) {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                        std::future::pending::<()>().await;
                    };
                    tokio::select! {
                        _ = async { tokio::join!(addons, built_in, rated) } => {}
                        () = early => {}
                    }
                    let wants = StartWants::read(&core, &target, audio_of(probed.await, &stream));
                    settle_start(&state, &snapshot, run, &wants, true);
                };
                let next = async {
                    if let Some(next) = next_episode(&core, &target).await {
                        let mut s = lock(&state);
                        // Checked as it is set: the title may have left a
                        // kids list while its meta was on the way.
                        let policy = core.policy();
                        let allowed = policy.allows(&target.kind, &target.item_id)
                            && (!policy.kids() || Policy::belongs(&target.item_id, &next.video_id));
                        if allowed && !s.closed {
                            s.snapshot.next = Some(next);
                            snapshot.set(s.snapshot.clone());
                        }
                    }
                };
                tokio::join!(subtitles, next);
                return;
            }
            Err(error) => {
                let pick = {
                    let mut s = lock(&state);
                    s.snapshot.attempts.push(attempt(&stream, &error));
                    note_failure(&mut s);
                    snapshot.set(s.snapshot.clone());
                    s.pick.clone()
                };
                if let Some(pick) = pick {
                    pick.failed(&stream.key);
                }
            }
        }
    }
}

/// On an engine that shows HEVC only from `hvc1` entries (Apple), a file
/// with an `hev1` entry that has no parameter sets plays as sound alone,
/// even relabelled: the version fails over, and the ranking remembers it.
async fn shows_picture(
    core: &Core,
    stream: &ParsedStream,
    lease: &Lease,
    hvc1_only: bool,
    state: &Mutex<State>,
) -> Result<(), String> {
    if !hvc1_only || !lease.picture_missing_on_apple().await {
        return Ok(());
    }
    not_here(state);
    let now = mq_library::hlc::now_ms();
    crate::speed::record_no_picture(core.library(), &stream.key, now);
    Err("This file's video shows no picture on this device".into())
}

/// A source that played this long since its first progress report played
/// well: a decode error after it is not the version's (code review
/// 2026-10-20 P2-2). Apple reports a picture that never came within 4 s.
const PLAYED_PAST_START: Duration = Duration::from_secs(30);
/// The apps report progress every few seconds; a larger step forward is a
/// seek or a source that started further on, not playback.
const PLAYED_STEP: Duration = Duration::from_secs(15);

/// One "Try again" run gives up after this, however many sources are left
/// (code review 2026-10-20 r3 P3-11).
const RETRY_BUDGET: Duration = Duration::from_secs(180);
const RETRY_GAVE_UP: &str = "Tried again for 3 minutes";

/// What is left of the budget: a "Try again" run's, else a first play's
/// start (`START_BUDGET`); None before a start's first attempt.
fn budget_left(s: &State) -> Option<Duration> {
    let until = s.retry_until.or(s.start_at.map(|at| at + START_BUDGET))?;
    Some(until.saturating_duration_since(tokio::time::Instant::now()))
}

/// `wait`, cut to what is left of the budget.
fn within_budget(state: &Mutex<State>, wait: Duration) -> Duration {
    budget_left(&lock(state)).map_or(wait, |left| wait.min(left))
}

/// A first play's start budget (plans/play-every-version.md 2e): from the
/// first attempt's add, every attempt and every move to another version
/// ends within it, and the player shows why nothing played at its end,
/// never later. From bench/torrent/start-latency.jsonl (333 healthy runs):
/// Ready p50 8.2 s, p90 20.3 s, max 64.3 s; first unchoke p50 2.4 s, p90
/// 7.3 s, p99 27.9 s. Every swarm dead: attempts at 0, 20, 32, 44, 56 and
/// 68 s, the last running to 90 s.
const START_BUDGET: Duration = Duration::from_secs(90);
/// An attempt with no unchoke this long after its add is a dead swarm: the
/// first attempt (cuts 3.5 % of healthy starts, 12 s would cut 6.0 %), then
/// every later one.
#[cfg(any(test, feature = "torrent"))]
const DEAD_FIRST: Duration = Duration::from_secs(20);
#[cfg(any(test, feature = "torrent"))]
const DEAD_LATER: Duration = Duration::from_secs(12);
/// An attempt with peers unchoked runs this long after its add, then ends
/// unless its window grows (what it received rose in the last `GROWING`).
/// One that begins with less than this left of the budget, or with nothing
/// else to play after it, is the last: it runs to the budget's end.
#[cfg(any(test, feature = "torrent"))]
const LIVE: Duration = Duration::from_secs(30);
#[cfg(any(test, feature = "torrent"))]
const GROWING: Duration = Duration::from_secs(5);
/// The loading screen offers "Choose another version" this long into a
/// start.
const CHOOSE_AFTER: Duration = Duration::from_secs(20);
/// Why a version was left, in the words of `moved`.
const NOT_HERE: &str = "did not play on this device";
const NOT_STARTED: &str = "did not start here";

/// How a "Try again" run judges a torrent's start (a first play's attempts
/// follow `Attempting`): whether a swarm that falls short may be cut at its
/// first window, and the deadline, each limit cut to what is left of its
/// budget.
#[cfg(feature = "torrent")]
fn start_rule(s: &State) -> Rule {
    let elsewhere = may_cut_early(s);
    let mut rule = Rule {
        windows: elsewhere,
        elsewhere,
        deadline: TORRENT_READY_TIMEOUT,
        cap: 3 * TORRENT_READY_TIMEOUT,
    };
    if let Some(left) = budget_left(s) {
        rule.deadline = rule.deadline.min(left);
        rule.cap = rule.cap.min(left);
    }
    rule
}

/// How a torrent's start is judged (`watch_start`).
#[cfg(any(test, feature = "torrent"))]
#[derive(Clone, Copy, Debug)]
struct Rule {
    /// A swarm that falls short of a window may be cut then.
    windows: bool,
    /// Another source or version can play instead: past the deadline even
    /// a swarm still in its window is cut.
    elsewhere: bool,
    deadline: Duration,
    /// Never past this.
    cap: Duration,
}

/// A swarm that falls short may be cut at its first window when something
/// else can play instead: another source of this version, or, for the
/// recommendation with moves left, the version the ranking picks next.
/// Never after "Try again".
fn may_cut_early(s: &State) -> bool {
    let next_version = || {
        s.pick
            .as_ref()
            .is_some_and(|p| p.recommended() && p.next_version(&s.tried, None).is_some())
    };
    !s.patient && (s.next < s.candidates.len() || next_version())
}

/// Every source of the version failed. When it failed for a reason of its
/// own, the ranking's next version follows: by itself when the
/// recommendation started (within the start's budget, `trying` names it), as the
/// failure card's offer when the user picked the version by hand. Whether
/// a version is next comes from the Versions view, read now.
fn move_on(s: &mut State) -> bool {
    let Some(why) = s.version_failed.take() else {
        return false;
    };
    let Some(pick) = &s.pick else {
        return false;
    };
    let Some(next) = pick.next_version(&s.tried, s.failed_on.as_ref()) else {
        return false;
    };
    let other = OtherVersion {
        key: next.key.clone(),
        label: next.label,
    };
    if !pick.recommended() {
        s.snapshot.offer = Some(other);
        return false;
    }
    if s.hops == 0 {
        s.left_because = Some(why);
    }
    s.hops += 1;
    s.tried
        .extend(next.candidates.iter().map(|c| c.key.clone()));
    s.candidates = next.candidates;
    s.next = 0;
    s.snapshot.trying = Some(other);
    true
}

type Opened = (Upstream, Option<String>, Option<u64>);

/// The resolve run a source opens in: the view's state, the snapshot it
/// publishes, and its number (`current`).
type InRun<'a> = (
    &'a Mutex<State>,
    &'a SharedObservable<PlaybackSnapshot>,
    u64,
);

/// Resolves one stream to an upstream. Its scope is the install that listed
/// it (the candidate's `addon_url`); neither the installed row's bit, which
/// sync may set, nor the manifest id, which any addon may declare, is read.
async fn open_source(
    core: &Core,
    stream: &ParsedStream,
    target: &PlayTarget,
    run: InRun<'_>,
) -> Result<Opened, String> {
    let state = run.0;
    let (url, headers, filename, size, addon_url) = match &stream.source {
        Source::Http { url } => (
            url.to_string(),
            stream.headers.clone(),
            stream.filename.clone(),
            stream.size_bytes,
            stream.addon_url.clone(),
        ),
        Source::Torrent {
            info_hash,
            file_idx,
            trackers,
            ..
        } if by_engine(stream, core.debrid().is_some(), core.has_engine()) => {
            // Full builds play torrents with the built-in engine; store
            // builds have none and need a debrid account.
            #[cfg(feature = "torrent")]
            return open_torrent(core, stream, target, run, info_hash, *file_idx, trackers).await;
            #[cfg(not(feature = "torrent"))]
            {
                let _ = (info_hash, file_idx, trackers);
                return Err("Torrents need a debrid account in this version".into());
            }
        }
        Source::ClientResolve(_) | Source::Torrent { .. } => {
            let (service, request) = debrid_request(stream, target.season, target.episode)
                .ok_or("The addon did not say which torrent to play")?;
            let resolver = core
                .debrid()
                .ok_or("This version needs a debrid account. Add one in Settings")?;
            let resolved = resolver
                .resolve(with_store(&resolver, service.as_deref(), request))
                .await
                .map_err(|e| e.to_string());
            // Hybrid: a torrent the debrid store cannot give plays peer to
            // peer in full builds.
            #[cfg(feature = "torrent")]
            if let (
                Err(e),
                Source::Torrent {
                    info_hash,
                    file_idx,
                    trackers,
                    ..
                },
            ) = (&resolved, &stream.source)
            {
                return open_torrent(core, stream, target, run, info_hash, *file_idx, trackers)
                    .await
                    .map_err(|p2p| format!("{e}; peer to peer: {p2p}"));
            }
            let link = resolved?;
            if let Some(why) = never_opens(core, stream, target, state, &link.filename) {
                not_here(state);
                return Err(why);
            }
            // The provider's link, not the addon's: no install scope.
            (link.url, Vec::new(), Some(link.filename), link.size, None)
        }
        other => return Err(format!("{} streams cannot play here", source_word(other))),
    };
    // Whether it answers is checked by the stream server's first read.
    Ok((
        Upstream::Http {
            url,
            headers,
            addon_url,
        },
        filename,
        size,
    ))
}

/// Plays a torrent with the built-in engine: metadata, file choice (the
/// addon's `fileIdx`, else its filename, else the episode, else the largest
/// video), then the head and container tail verified before the player gets
/// the URL. A torrent that already did not start in this view is skipped at
/// once; while this one waits, the next torrent candidate's metadata comes
/// in (start-latency 4.1). Its start shows in `start` once a second.
#[cfg(feature = "torrent")]
async fn open_torrent(
    core: &Core,
    stream: &ParsedStream,
    target: &PlayTarget,
    run: InRun<'_>,
    info_hash: &str,
    file_idx: Option<u32>,
    trackers: &[String],
) -> Result<Opened, String> {
    use mq_engine::FileHint;
    let state = run.0;
    let hash = torrent_id(info_hash);
    {
        let mut s = lock(state);
        if s.failed_hashes.contains(&hash) {
            failed_to_start(&mut s);
            return Err(SAME_TORRENT.into());
        }
    }
    let engine = core.torrent_engine().await.map_err(|e| e.to_string())?;
    // The add, once the engine is up: a cold engine start never eats into
    // the attempt's window.
    let begun = tokio::time::Instant::now();
    let policy = attempt_policy(&lock(state), begun);
    // This playback's own check, read before the torrent is touched: a
    // hold placed after it (the title left a kids list) is never lifted by
    // it (plans/m11-kids.md 4.4).
    let generation = match core.library().authorize(
        core.profile(),
        &target.kind,
        &target.item_id,
        Some(&target.video_id),
    ) {
        Ok(Authorization::Authorized { generation }) => generation,
        Ok(Authorization::Denied { .. }) => return Err("This title is not on the list".into()),
        Err(e) => return Err(e.to_string()),
    };
    // Addresses the policy refused while this torrent tried to start, as
    // counts only (plans/c27-torrent-policy.md 3.8). The window is this
    // open's; another torrent opening at the same time adds its own.
    let refused_before = engine.refused();
    let did_not_start = || {
        let d = engine.refused().since(refused_before);
        tracing::info!(
            trackers = d.trackers,
            peers = d.peers,
            "torrent did not start"
        );
    };
    // Fetched ahead while an earlier candidate waited: its answer first.
    let ahead = lock(state)
        .ahead
        .iter()
        .find(|a| a.hash == hash)
        .map(|a| a.done.clone());
    // Once this candidate has waited LONG_STAGE with no details or Ready,
    // the next torrent's details start (review P2-2: a start that goes at
    // once asks nothing of another swarm).
    let ahead_now = || {
        if begun.elapsed() >= LONG_STAGE {
            look_ahead(core, &engine, &mut lock(state), &hash);
        }
    };
    let add = async {
        // Fetched ahead: its answer first, so the swarm is asked once.
        if let Some(mut done) = ahead {
            let _ = done.wait_for(|done| *done).await;
        }
        engine.add_magnet(info_hash, trackers, &[]).await
    };
    let details = Sample {
        stage: StartStage::Details,
        ..Sample::default()
    };
    // A first play's attempt has its details within its own window; a "Try
    // again" run's gets the full wait, cut to its budget.
    let (wait, full) = match policy {
        Some(p) => {
            let window = p.window();
            (window, window >= METADATA_TIMEOUT)
        }
        None => {
            let wait = within_budget(state, METADATA_TIMEOUT);
            (wait, wait == METADATA_TIMEOUT)
        }
    };
    let tick = || {
        publish_start(run, details);
        ahead_now();
    };
    let torrent = tokio::time::timeout(wait, ticking(add, tick))
        .await
        .map_err(|_| {
            did_not_start();
            // Stale seeder counts fail this way: dead, once the chain shows
            // the connection works, and only when it had the full wait (a
            // "Try again" budget can cut it short, code review 2026-10-20 r4
            // P3-1).
            let now = {
                let mut s = lock(state);
                failed_to_start(&mut s);
                s.failed_hashes.push(hash.clone());
                if full {
                    s.dead.failed(info_hash, false)
                } else {
                    Vec::new()
                }
            };
            remember_dead(core, now);
            "No peer sent the torrent's details in time".to_owned()
        })?
        .map_err(|e| e.to_string())?;
    // This profile's use of it, for as long as the playback lives: a hold
    // for another profile leaves the torrent running for it.
    let profile = core.profile();
    let claim = engine.claim(torrent, profile).map_err(|e| e.to_string())?;
    {
        // Kept from now on, so a closure pauses it even before its head is
        // ready; a view already stopped starts nothing.
        let mut s = lock(state);
        if s.closed {
            return Err("The playback was stopped".into());
        }
        s.torrents.push(torrent);
        // This start's now: its own failure removes it, never the fetch,
        // and a later turn (Try again, another version) may fetch it ahead
        // again.
        s.ahead.retain(|a| {
            let mine = a.hash == hash;
            if mine {
                a.taken.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            !mine
        });
    }
    publish_start(
        run,
        Sample {
            stage: StartStage::Connecting,
            ..Sample::default()
        },
    );
    // Only a newer authorization lifts a hold; an older one leaves it, and
    // the file choice below then answers that the torrent is held.
    if let Err(e) = engine.release_hold(torrent, profile, generation).await {
        tracing::debug!("torrent hold: {e}");
    }
    let hint = match (file_idx, &stream.filename, target.season, target.episode) {
        (Some(i), ..) => FileHint::Index(i as usize),
        (None, Some(name), ..) => FileHint::Name(name.clone()),
        (None, None, Some(season), Some(episode)) => FileHint::Episode { season, episode },
        _ => FileHint::Largest,
    };
    let by_episode = matches!(hint, FileHint::Episode { .. });
    // The rate the ranking sized the row with (2.2): the file over the last
    // session's duration, else the meta runtime, else the guess.
    let rate_of = |len: u64| {
        let s = lock(state);
        let duration_s = s
            .known_duration_ms
            .map(|ms| ms / 1000)
            .or(s.runtime_s.map(u64::from));
        window_rate(
            stream,
            len,
            by_episode || target.episode.is_some(),
            duration_s,
        )
    };
    // A torrent this device kept before this start (its details beside its
    // folder, written by an earlier file choice) keeps its folder when the
    // start fails: an offline or stalled start never takes a kept title off
    // the device.
    let kept = {
        let (dir, hash) = (core.torrent_dir().to_owned(), info_hash.to_owned());
        tokio::task::spawn_blocking(move || mq_engine::cached_on_disk(&dir, &hash, 0, 0).is_some())
            .await
            .unwrap_or(false)
    };
    let started = async {
        let file = engine.select_file(torrent, profile, &hint).await?;
        let len = engine.files(torrent)?.get(file).map_or(0, |f| f.len);
        // Where a resume reads first: the saved share of the file.
        let resume = {
            let s = lock(state);
            s.snapshot.resume_ms.zip(s.known_duration_ms)
        }
        .and_then(|(pos, dur)| resume_offset(pos, dur, len));
        open_window(core, &engine, torrent, file, len, rate_of(len), resume).await?;
        let reader = claim.open_reader(file).await?;
        let info = reader.file().clone();
        // The file the addon pointed at must be the one listed (3.4).
        if let Some(why) = judge_file(core, stream, target, &info.path, info.len, by_episode) {
            return Ok((file, info, Started::Wrong(why.into())));
        }
        // Before its start is asked for: nothing of it is fetched.
        if let Some(why) = never_opens(core, stream, target, state, &info.path) {
            return Ok((file, info, Started::Wrong(why)));
        }
        // A resume opens its own point first: the focus, whose blocking
        // window and the index make Ready. The head's blocking window is
        // kept beside it as a probe (2.3, 2.8), for the player's first read.
        let blocking = engine.blocking_bytes();
        // Within the head's blocking window the head read fetches it anyway.
        let at = resume.filter(|&at| at > blocking);
        let mut from = match at {
            Some(at) => Some(reader.range(at, None).await?),
            None => None,
        };
        let mut head = reader.range(0, at.map(|_| blocking)).await?;
        let ready = async {
            if let Some(from) = from.as_mut() {
                from.ready().await?;
            }
            head.ready().await
        };
        // Received payload and the first unchoke (code review r1 P1): a
        // piece verifies only whole, long after its first blocks.
        let progress = || async {
            let s = engine.stats(torrent).await.ok()?;
            publish_start(run, sample(&s));
            ahead_now();
            Some(Progress {
                received: s.received_bytes,
                unchoked: s.unchoked_ms.is_some(),
            })
        };
        let rule = start_rule(&lock(state));
        let ready = match policy {
            Some(p) => watch_policy(ready, progress, p).await,
            None => watch_start(ready, progress, STALL, rule).await,
        };
        Ok::<_, mq_engine::EngineError>((file, info, ready))
    };
    let started = started.await;
    // Refused before any write ("this device is full"): what the folder
    // holds stays (a pack's other episodes, a warm window), review r3 B P3-1.
    let refused = matches!(started, Err(mq_engine::EngineError::DeviceFull));
    // Another source of a swarm that did not start is skipped in this view.
    if started
        .as_ref()
        .is_ok_and(|(_, _, started)| not_started(started))
    {
        lock(state).failed_hashes.push(hash);
    }
    let failed = match started {
        Ok((file, info, Started::Ready(Ok(())))) => {
            let name = info
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&info.path)
                .to_owned();
            spawn_sample(core, torrent, info_hash, stream.seeders);
            // Seconds of this file for the scheduler, until the player says
            // how long it is (plans/streaming-window.md 2.2).
            let metered = core.network() == crate::speed::Network::Cellular;
            let _ = engine
                .set_rate(torrent, file, rate_of(info.len), metered)
                .await;
            lock(state).rate_of = Some((torrent, file, info.len));
            let upstream = Upstream::Torrent { claim, file };
            return Ok((upstream, Some(name), Some(info.len)));
        }
        // Held for this profile (its check is older than the hold): the
        // torrent, its hold and its files stay, since another profile may
        // be playing it, and the next candidate plays instead.
        Err(mq_engine::EngineError::Held) => return Err("This title is not on the list".into()),
        Ok((_, _, Started::Late { unchoked })) => {
            did_not_start();
            failed_to_start(&mut lock(state));
            if unchoked {
                let now = lock(state).dead.reached();
                remember_dead(core, now);
            }
            "the swarm did not send the start of the video in time".to_owned()
        }
        Ok((_, _, Started::Stalled { nothing })) => {
            did_not_start();
            // Ranked last here for a day only when it sent nothing at all
            // in the full window it got (the listed seeders were wrong);
            // its unchoke proves the connection either way.
            let now = {
                let mut s = lock(state);
                failed_to_start(&mut s);
                if nothing {
                    s.dead.failed(info_hash, true)
                } else {
                    s.dead.reached()
                }
            };
            remember_dead(core, now);
            "the swarm sent almost nothing".to_owned()
        }
        Ok((_, _, Started::Wrong(why))) => {
            not_here(state);
            why
        }
        Ok((_, _, Started::Ready(Err(e)))) | Err(e) => engine_words(&e),
    };
    // The next candidate plays instead: stop downloading this one, unless
    // another playback still claims it (the engine then keeps it). What it
    // held before this start stays.
    drop(claim);
    let _ = engine.remove(torrent, refused || kept).await;
    Err(failed)
}

/// Plays `file` in a window when it does not fit whole
/// (plans/streaming-window.md 2.1, `WindowInputs::fits_whole`), or in a
/// walk build's override window; else it plays whole. On a volume that
/// cannot punch holes a file that does not fit whole is refused.
#[cfg(feature = "torrent")]
async fn open_window(
    core: &Core,
    engine: &mq_engine::Engine,
    torrent: mq_engine::TorrentHandle,
    file: usize,
    len: u64,
    rate: u64,
    resume_at: Option<u64>,
) -> Result<(), mq_engine::EngineError> {
    let walk = core.walk_window();
    let Some(free) = core.free_bytes() else {
        return Ok(());
    };
    let p2p = core.speed(crate::speed::Kind::P2p).mbps;
    // The torrent's own folder counts in R for a window (review P1-2: a
    // resume finds its window there), never for a whole file (review r2
    // P1-1: a pack's earlier episodes are not freed for the next one).
    let inputs = engine
        .window_inputs(
            torrent,
            file,
            free,
            rate,
            (f64::from(p2p) * 125_000.0) as u64,
        )
        .await?;
    if !core.windows_ok() {
        if walk.is_some() {
            tracing::warn!("walk window: this volume cannot punch holes");
        }
        // Whole or not at all: the ranking hides such a row (3.1).
        return if inputs.fits_whole(len) {
            Ok(())
        } else {
            Err(mq_engine::EngineError::DeviceFull)
        };
    }
    if walk.is_none() && inputs.fits_whole(len) {
        // A window another file of the torrent left (Up Next in a pack)
        // ends with it.
        return engine.play_whole(torrent, file).await;
    }
    let mut plan = mq_engine::window_plan(engine.device_class(), inputs)?;
    if let Some(w) = walk {
        plan = plan.within(w).ok_or(mq_engine::EngineError::DeviceFull)?;
    }
    tracing::info!(
        budget = plan.budget,
        free = inputs.free,
        room = inputs.room(),
        walk = ?walk,
        "torrent window"
    );
    let window = mq_engine::Window {
        plan,
        bitrate: rate,
        metered: core.network() == crate::speed::Network::Cellular,
        resume_at,
    };
    engine.window(torrent, file, window).await
}

/// A torrent another source of which did not start a moment ago.
#[cfg(feature = "torrent")]
const SAME_TORRENT: &str = "The same torrent did not start from another source";

/// One torrent candidate's metadata, fetched ahead.
#[cfg(feature = "torrent")]
struct Ahead {
    /// Lowercase.
    hash: String,
    /// Turns true when the add answered, either way.
    done: tokio::sync::watch::Receiver<bool>,
    /// A start took its torrent: that start's own failure removes it.
    taken: Arc<std::sync::atomic::AtomicBool>,
}

/// A fetch ahead gives up after the longest one candidate holds the start:
/// its details, then three deadlines for a swarm that keeps sending with
/// nothing else to play (`start_rule`'s cap). Its own turn then asks again.
#[cfg(feature = "torrent")]
const AHEAD_FOR: Duration =
    Duration::from_secs(METADATA_TIMEOUT.as_secs() + 3 * TORRENT_READY_TIMEOUT.as_secs());

/// A torrent's info hash as the engine spells it (40 lowercase hex), from
/// hex or base32 (review P3-10); the text lowercased when it is neither.
#[cfg(feature = "torrent")]
fn torrent_id(hash: &str) -> String {
    use std::str::FromStr;
    let hash = hash.trim();
    mq_engine::librqbit::dht::Id20::from_str(&hash.to_ascii_uppercase())
        .map_or_else(|_| hash.to_ascii_lowercase(), |id| id.as_string())
}

/// Starts the metadata of the next torrent candidate after the one opening
/// now (`current`, as `torrent_id` spells it), when it is another torrent that has
/// not failed in this view and nothing fetches it yet (start-latency 4.1).
/// Metadata only: nothing downloads before its file is chosen, and the
/// candidate's own `add_magnet` then finds it known. When the view closes,
/// a torrent no start took, and that no reader ever woke, is removed, its
/// folder kept (a resume's data stays).
#[cfg(feature = "torrent")]
fn look_ahead(core: &Core, engine: &mq_engine::Engine, s: &mut State, current: &str) {
    let account = core.debrid().is_some();
    let next = s.candidates[s.next.min(s.candidates.len())..]
        .iter()
        .find_map(|c| match &c.source {
            Source::Torrent {
                info_hash,
                trackers,
                ..
            } if by_engine(c, account, true) => {
                let hash = torrent_id(info_hash);
                (hash != current && !s.failed_hashes.contains(&hash)).then_some((hash, trackers))
            }
            _ => None,
        });
    let Some((hash, trackers)) = next else {
        return;
    };
    if s.closed || s.ahead.iter().any(|a| a.hash == hash) {
        return;
    }
    let (said, done) = tokio::sync::watch::channel(false);
    let taken = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (engine, closing, kept) = (engine.clone(), s.closing.clone(), taken.clone());
    let (fetch, trackers) = (hash.clone(), trackers.clone());
    tokio::spawn(async move {
        let add = tokio::time::timeout(AHEAD_FOR, engine.add_magnet(&fetch, &trackers, &[]));
        let added = tokio::select! {
            added = add => added.ok().and_then(Result::ok),
            () = closing.cancelled() => return,
        };
        let _ = said.send(true);
        closing.cancelled().await;
        // `pieces_total` is 0 until a file choice starts the torrent: one
        // that another playback woke since keeps going.
        if let Some(torrent) = added
            && !kept.load(std::sync::atomic::Ordering::Relaxed)
            && engine
                .stats(torrent)
                .await
                .is_ok_and(|s| s.pieces_total == 0)
        {
            let _ = engine.remove(torrent, true).await;
        }
    });
    s.ahead.push(Ahead { hash, done, taken });
}

/// The start never reads 1 before `ready`.
const START_MAX: f32 = 0.99;
/// A source starting this long gets the core's words for its stage.
const LONG_STAGE: Duration = Duration::from_secs(3);

/// What a source's start is doing now.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    stage: StartStage,
    peers: u32,
    have: u64,
    need: u64,
    rate: u64,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            stage: StartStage::Details,
            peers: 0,
            have: 0,
            need: 0,
            rate: 0,
        }
    }
}

/// The next source of the version starts: its start goes on from where the
/// last source's got (one start never goes back), at `stage`.
fn begin_start(s: &mut State, stage: StartStage) {
    // A loading screen begins with its first source.
    if s.snapshot.start.is_none() {
        s.loading_since = tokio::time::Instant::now();
    }
    s.snapshot.choose_another = s.loading_since.elapsed() >= CHOOSE_AFTER;
    let progress = s.snapshot.start.as_ref().map_or(0.0, |p| p.progress);
    s.start_base = progress;
    s.source_since = tokio::time::Instant::now();
    let (source, sources) = (s.next as u32, s.candidates.len() as u32);
    s.snapshot.start = Some(StartProgress {
        stage,
        peers: 0,
        have_bytes: 0,
        need_bytes: 0,
        bytes_per_s: 0,
        source,
        sources,
        progress,
        caption: (source > 1).then(|| format!("Trying source {source} of {sources}")),
    });
}

/// Shows `x` as the starting source's start: its share of what was left
/// when the source began, never back, and the stage's words once the source
/// has taken long.
fn show_start(s: &mut State, x: Sample) {
    s.snapshot.choose_another = s.loading_since.elapsed() >= CHOOSE_AFTER;
    let long = s.source_since.elapsed() >= LONG_STAGE;
    let base = s.start_base;
    let Some(start) = s.snapshot.start.as_mut() else {
        return;
    };
    start.caption = match x.stage {
        _ if !long => (start.source > 1)
            .then(|| format!("Trying source {} of {}", start.source, start.sources)),
        StartStage::Details => Some("Getting the file's details".into()),
        StartStage::Connecting if x.peers == 0 => Some("Finding peers".into()),
        StartStage::Connecting => Some(format!("Finding peers, {} so far", x.peers)),
        // The engine counts nothing once a range of the file was Ready (a
        // later start of the same file): no numbers then.
        StartStage::Buffering if x.need == 0 => Some("Buffering".into()),
        StartStage::Buffering => Some(format!("Buffering, {} of {} MB", mb(x.have), mb(x.need))),
        StartStage::Opening => Some("Opening the video".into()),
    };
    start.progress = (base + (START_MAX - base) * start_share(&x)).max(start.progress);
    start.stage = x.stage;
    start.peers = x.peers;
    start.have_bytes = x.have;
    start.need_bytes = x.need;
    start.bytes_per_s = x.rate;
}

/// [`show_start`], published, while `run` is the view's and it resolves.
#[cfg(feature = "torrent")]
fn publish_start((state, snapshot, run): InRun, x: Sample) {
    let mut s = lock(state);
    if current(&s, run) && s.snapshot.phase == PlaybackPhase::Resolving {
        show_start(&mut s, x);
        snapshot.set(s.snapshot.clone());
    }
}

/// The source is open; its first byte is read through the server: the
/// start's last numbers stay, at the `opening` stage.
fn show_opening(state: &Mutex<State>, snapshot: &SharedObservable<PlaybackSnapshot>, run: u64) {
    let mut s = lock(state);
    if !current(&s, run) || s.snapshot.phase != PlaybackPhase::Resolving {
        return;
    }
    let Some(start) = s.snapshot.start.as_ref() else {
        return;
    };
    let opening = Sample {
        stage: StartStage::Opening,
        peers: start.peers,
        have: start.have_bytes,
        need: start.need_bytes,
        rate: start.bytes_per_s,
    };
    show_start(&mut s, opening);
    snapshot.set(s.snapshot.clone());
}

/// A source's share of its start done: peers 15 % (8 or more, as Stremio
/// counts them), the start's bytes 75 % and the speed 5 % (1 MiB/s or
/// more) once the details are in, which count 5 %.
fn start_share(x: &Sample) -> f32 {
    let part = |have: u64, full: u64| (have as f32 / full.max(1) as f32).min(1.0);
    match x.stage {
        StartStage::Details => 0.0,
        _ => {
            0.05 + 0.15 * part(x.peers.into(), 8)
                + 0.75 * part(x.have, x.need)
                + 0.05 * part(x.rate, 1 << 20)
        }
    }
}

/// Megabytes with one decimal: "4.1", "8".
fn mb(bytes: u64) -> String {
    let mb = format!("{:.1}", bytes as f64 / 1e6);
    mb.strip_suffix(".0").map_or(mb.clone(), str::to_owned)
}

/// A torrent's start as its stats say: the details are in; connecting
/// until a peer unchoked or sent; of the start, what the engine counts of
/// the focused range's Ready set (its blocking window and the file's
/// index, in whole pieces).
#[cfg(feature = "torrent")]
fn sample(stats: &mq_engine::TorrentStats) -> Sample {
    let sending = stats.unchoked_ms.is_some() || stats.received_bytes > 0;
    Sample {
        stage: if sending {
            StartStage::Buffering
        } else {
            StartStage::Connecting
        },
        peers: stats.peers as u32,
        have: stats.start_have,
        need: stats.start_need,
        rate: stats.down_bytes_per_s,
    }
}

/// `work`, with `tick` once a second while it runs (the first at once).
#[cfg(feature = "torrent")]
async fn ticking<T>(work: impl std::future::Future<Output = T>, mut tick: impl FnMut()) -> T {
    tokio::pin!(work);
    let mut every = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            out = &mut work => return out,
            _ = every.tick() => tick(),
        }
    }
}

/// Where a play saved at `pos_ms` of `dur_ms` resumes in a file of `len`
/// bytes: the same share of the file. The playback, its window and "On this
/// device" all place it so (a guessed or floored rate would land an 80 min
/// episode a third too far). None with the duration unknown, or at or past
/// the file's end.
#[cfg(any(test, feature = "torrent"))]
fn resume_offset(pos_ms: u64, dur_ms: u64, len: u64) -> Option<u64> {
    if dur_ms == 0 {
        return None;
    }
    let at = u128::from(pos_ms) * u128::from(len) / u128::from(dur_ms);
    u64::try_from(at).ok().filter(|&at| at < len)
}

#[cfg(test)]
use mq_streams::guessed_rate;
#[cfg(any(test, feature = "torrent"))]
use mq_streams::window_rate;

/// How a torrent's start went.
#[cfg(any(test, feature = "torrent"))]
#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
enum Started<T> {
    Ready(T),
    /// Verified bytes grew by less than the floor in a window, and another
    /// candidate can play; `nothing`: not one byte came in it.
    Stalled {
        nothing: bool,
    },
    /// The deadline passed; `unchoked`: a peer had unchoked us.
    Late {
        unchoked: bool,
    },
    /// The file is not the one the version listed, or one the device
    /// never opens.
    #[cfg_attr(test, allow(dead_code))]
    Wrong(String),
}

#[cfg(test)]
impl Sample {
    fn with_need(self, need: u64) -> Self {
        Self { need, ..self }
    }
}

/// A start that ended so says its swarm did not start: no Ready by the
/// deadline, or a stall.
#[cfg(any(test, feature = "torrent"))]
fn not_started<T>(started: &Started<T>) -> bool {
    matches!(started, Started::Late { .. } | Started::Stalled { .. })
}

/// How long a swarm may take to send each `floor` of verified bytes: the
/// first window, from its first connected peer, is longer, since a swarm
/// spends its first seconds finding and unchoking peers.
#[cfg(any(test, feature = "torrent"))]
#[derive(Clone, Copy)]
struct Stall {
    first: Duration,
    later: Duration,
    floor: u64,
}

/// What the engine says of a starting torrent, each second.
#[cfg(any(test, feature = "torrent"))]
#[derive(Clone, Copy)]
struct Progress {
    /// Payload received (code review r1 P1: a 16 MiB piece verifies long
    /// after its first blocks arrive).
    received: u64,
    /// A peer unchoked us.
    unchoked: bool,
}

/// How a first play's attempt is judged (plans/play-every-version.md 2e),
/// from its add: with no unchoke it ends at `dead`; with one, at `LIVE`
/// unless its window grows; never past `until`, the budget's end. The last
/// attempt that fits runs to `until`.
#[cfg(any(test, feature = "torrent"))]
#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
struct Attempting {
    added: tokio::time::Instant,
    dead: Duration,
    until: tokio::time::Instant,
    last: bool,
}

#[cfg(any(test, feature = "torrent"))]
#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
impl Attempting {
    /// How long its details may take: its dead window, or to the budget's
    /// end for the last attempt.
    fn window(&self) -> Duration {
        let left = self.until.saturating_duration_since(self.added);
        if self.last { left } else { self.dead.min(left) }
    }
}

/// The policy of a first play's attempt added at `added`; None in a "Try
/// again" run, which waits patiently (`start_rule`). The first attempt of
/// the start gets `DEAD_FIRST`, every later one `DEAD_LATER`; it is the
/// last when less than `LIVE` is left or nothing else can play after it.
#[cfg(feature = "torrent")]
fn attempt_policy(s: &State, added: tokio::time::Instant) -> Option<Attempting> {
    let start = s.start_at.filter(|_| !s.patient)?;
    Some(attempting(start, s.tries, added, may_cut_early(s)))
}

/// Attempt `tries` (from 1) of a start begun at `start`, added at `added`;
/// `more`: something else can play after it.
#[cfg(any(test, feature = "torrent"))]
#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
fn attempting(
    start: tokio::time::Instant,
    tries: u32,
    added: tokio::time::Instant,
    more: bool,
) -> Attempting {
    let until = start + START_BUDGET;
    Attempting {
        added,
        dead: if tries <= 1 { DEAD_FIRST } else { DEAD_LATER },
        until,
        last: until.saturating_duration_since(added) < LIVE || !more,
    }
}

/// Waits for `ready` under a first play's `Attempting`: each second it reads
/// whether a peer unchoked and what the swarm received, which is what
/// buffers ahead of the playhead before Ready. A dead swarm ends as `Late`,
/// a live one whose window stopped growing as `Stalled`.
#[cfg(any(test, feature = "torrent"))]
async fn watch_policy<T, P, F>(
    ready: impl std::future::Future<Output = T>,
    mut progress: P,
    p: Attempting,
) -> Started<T>
where
    P: FnMut() -> F,
    F: std::future::Future<Output = Option<Progress>>,
{
    tokio::pin!(ready);
    let mut unchoked = false;
    let mut first = None;
    // What it had received each second, over the last `GROWING`.
    let mut seen = std::collections::VecDeque::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            out = &mut ready => return Started::Ready(out),
            _ = tick.tick() => {
                let now = tokio::time::Instant::now();
                if now >= p.until {
                    return Started::Late { unchoked };
                }
                if let Some(x) = progress().await {
                    unchoked |= x.unchoked;
                    first.get_or_insert(x.received);
                    seen.push_back((now, x.received));
                }
                while seen.front().is_some_and(|&(at, _)| now - at > GROWING) {
                    seen.pop_front();
                }
                let age = now - p.added;
                if p.last {
                    continue;
                }
                if !unchoked && age >= p.dead {
                    return Started::Late { unchoked: false };
                }
                let growing = seen.front().zip(seen.back()).is_some_and(|(a, b)| b.1 > a.1);
                if unchoked && age >= LIVE && !growing {
                    let received = seen.back().map(|&(_, r)| r);
                    return Started::Stalled { nothing: received == first };
                }
            }
        }
    }
}

/// Waits for `ready` while the swarm keeps sending: a window opens at its
/// first unchoke (the metadata is already in), and each `floor` of
/// received payload must come within it (`first`, then `later`). A swarm
/// that falls short is cut when `others` can play instead; otherwise, and
/// with no unchoke at all, it gets until `deadline`. Past the deadline a swarm still in its window is
/// kept only when nothing else can play, and never past three deadlines
/// (code review 2026-10-20 r2 P2-1: a trickle must not hold the start).
#[cfg(any(test, feature = "torrent"))]
async fn watch_start<T, P, F>(
    ready: impl std::future::Future<Output = T>,
    mut progress: P,
    stall: Stall,
    rule: Rule,
) -> Started<T>
where
    P: FnMut() -> F,
    F: std::future::Future<Output = Option<Progress>>,
{
    let start = tokio::time::Instant::now();
    tokio::pin!(ready);
    let first = progress().await.map_or(0, |p| p.received);
    // The bytes and time the current window began with, and its length.
    let mut window: Option<(u64, tokio::time::Instant, Duration)> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            out = &mut ready => return Started::Ready(out),
            _ = tick.tick() => {
                let now = tokio::time::Instant::now();
                // Within its current window, a swarm is progressing.
                let progressing = window.is_some_and(|(_, since, length)| now - since < length);
                let past = now - start >= rule.deadline && (rule.elsewhere || !progressing);
                if past || now - start >= rule.cap {
                    return Started::Late { unchoked: window.is_some() };
                }
                let Some(p) = progress().await else { continue };
                match window {
                    None if p.unchoked => window = Some((p.received, now, stall.first)),
                    None => {}
                    Some((mark, _, _)) if p.received >= mark.saturating_add(stall.floor) => {
                        window = Some((p.received, now, stall.later));
                    }
                    Some((_, since, length)) if rule.windows && now - since >= length => {
                        return Started::Stalled { nothing: p.received == first };
                    }
                    Some(_) => {}
                }
            }
        }
    }
}

/// The verdict on the file a torrent opened (3.4), kept on this device
/// when it is not the version listed: under the stream key and the target
/// it played for, for 30 days (code review 2026-10-20 r2 P3-3, r3 P3-3,
/// P3-4: the keeping is part of the verdict `open_torrent` asks for).
#[cfg(any(test, feature = "torrent"))]
fn judge_file(
    core: &Core,
    stream: &ParsedStream,
    target: &PlayTarget,
    path: &str,
    len: u64,
    by_episode: bool,
) -> Option<&'static str> {
    let why = wrong_file(path, len, stream.size_bytes, target, by_episode)?;
    // A film's key without a file index (`bt:{hash}:auto`) can stand for
    // every film of a collection: only an episode, or a file index, says
    // which file was wrong (code review 2026-10-20 r4 P3-7).
    if target.episode.is_some() || !stream.key.ends_with(":auto") {
        let now = mq_library::hlc::now_ms();
        let played_for = (target.season, target.episode);
        crate::speed::record_wrong_file(core.library(), &stream.key, played_for, now);
    }
    Some(why)
}

/// Why a debrid or torrent file, named `path` once the service or the
/// swarm says, never plays here: its container is one the device's player
/// does not open and no second engine does (an MKV one on Apple). The
/// version fails as its own before the player gets it; its container is
/// kept, so the ranking hides it from then on. Not "did not play": the
/// listing had not named the file.
fn never_opens(
    core: &Core,
    stream: &ParsedStream,
    target: &PlayTarget,
    state: &Mutex<State>,
    path: &str,
) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let container = mq_relparse::parse(name).container?;
    let opens = |c: &DeviceCaps| c.has_fallback_engine || c.opens(&container);
    if lock(state).pick.as_ref().is_none_or(|p| opens(&p.caps)) {
        return None;
    }
    let played_for = (target.season, target.episode);
    let now = mq_library::hlc::now_ms();
    crate::speed::record_container(core.library(), &stream.key, played_for, &container, now);
    Some(format!(
        "This device cannot open {} files",
        container.to_ascii_uppercase()
    ))
}

/// Why the file a torrent opened is not the version listed (3.4): larger
/// than the listed size by more than 10% (smaller is a pack's file), or
/// named for another episode. `by_episode`: the file was found by the
/// episode already.
#[cfg(any(test, feature = "torrent"))]
fn wrong_file(
    path: &str,
    len: u64,
    listed: Option<u64>,
    target: &PlayTarget,
    by_episode: bool,
) -> Option<&'static str> {
    if listed.is_some_and(|l| len as f64 > l as f64 * 1.1) {
        return Some("the torrent's file is larger than the version listed");
    }
    if by_episode {
        return None;
    }
    let r = mq_relparse::parse(path.rsplit('/').next().unwrap_or(path));
    let other = |named: &[u32], want: Option<u32>| {
        want.is_some_and(|w| !named.is_empty() && !named.contains(&w))
    };
    // A name with no season numbers its episodes across the show (anime:
    // "One Piece - 1071"), which a per-season episode cannot judge (code
    // review r1 P2-3).
    let seasoned = !r.seasons.is_empty();
    (other(&r.seasons, target.season) || (seasoned && other(&r.episodes, target.episode)))
        .then_some("the torrent's file is another episode")
}

/// Reads the torrent's capacity sample once its first 30 s are over and
/// counts it into the estimate; a torrent removed by then (it failed over)
/// gives none. Once per torrent and network: a pack's next episode reads
/// the same first 30 s again (code review r1 P2-1).
#[cfg(feature = "torrent")]
fn spawn_sample(
    core: &Core,
    torrent: mq_engine::TorrentHandle,
    info_hash: &str,
    seeders: Option<u32>,
) {
    if !first_sample(info_hash, core.network()) {
        return;
    }
    let core = core.clone();
    tokio::spawn(async move {
        tokio::time::sleep(SAMPLE_AFTER).await;
        let Ok(engine) = core.torrent_engine().await else {
            return;
        };
        let Ok(stats) = engine.stats(torrent).await else {
            return;
        };
        let sample = crate::speed::Sample {
            kind: crate::speed::Kind::P2p,
            bytes: stats.sample_bytes,
            busy: Duration::from_millis(stats.sample_busy_ms),
            seeders,
        };
        let min = core.prefs().streams.min_seeders;
        crate::speed::record(
            core.library(),
            core.network(),
            &sample,
            min,
            mq_library::hlc::now_ms(),
        );
    });
}

/// Reads an HTTP playback's capacity sample once its first 30 s are over,
/// while it is still the playing source and the view lives, and counts it
/// into the estimate.
fn spawn_http_sample(core: &Core, state: &Arc<Mutex<State>>, attempt: u64) {
    let (core, state) = (core.clone(), Arc::downgrade(state));
    tokio::spawn(async move {
        tokio::time::sleep(SAMPLE_AFTER).await;
        let Some(state) = state.upgrade() else {
            return;
        };
        let sample = {
            let mut s = lock(&state);
            if s.closed || s.snapshot.attempt != attempt {
                return;
            }
            take_http_sample(&mut s)
        };
        if let Some(sample) = sample {
            record_http_sample(&core, sample);
        }
    });
}

/// The playing HTTP source's capacity sample, once per view.
fn take_http_sample(s: &mut State) -> Option<(u64, Duration)> {
    if s.sampled {
        return None;
    }
    let sample = s.lease.as_ref().and_then(Lease::sample)?;
    s.sampled = true;
    Some(sample)
}

fn record_http_sample(core: &Core, (bytes, busy): (u64, Duration)) {
    let sample = crate::speed::Sample {
        kind: crate::speed::Kind::Http,
        bytes,
        busy,
        seeders: None,
    };
    let min = core.prefs().streams.min_seeders;
    crate::speed::record(
        core.library(),
        core.network(),
        &sample,
        min,
        mq_library::hlc::now_ms(),
    );
}

/// True the first time this process samples `info_hash` on `network`.
// ponytail: per process; a restart may sample a torrent once more.
#[cfg(any(test, feature = "torrent"))]
fn first_sample(info_hash: &str, network: crate::speed::Network) -> bool {
    static SAMPLED: Mutex<Vec<(String, crate::speed::Network)>> = Mutex::new(Vec::new());
    let key = (info_hash.to_ascii_lowercase(), network);
    let mut sampled = lock(&SAMPLED);
    if sampled.contains(&key) {
        return false;
    }
    sampled.push(key);
    true
}

/// A torrent row plays through the engine when there is no account, or
/// when our cache check found the service does not have it: the ranking
/// scored it by torrent speed (3.2), and asking the service would add it
/// to the account (code review r1 P2-2).
pub(crate) fn by_engine(stream: &ParsedStream, account: bool, engine: bool) -> bool {
    matches!(stream.source, Source::Torrent { .. })
        && (!account || (engine && stream.cached == Some(false)))
}

/// Verified torrent bytes for the subtitle hash.
#[cfg(feature = "torrent")]
struct TorrentBytes(mq_engine::StreamReader);

#[cfg(feature = "torrent")]
impl mq_media::RangeReader for TorrentBytes {
    fn len(&self) -> futures_util::future::BoxFuture<'_, Result<u64, mq_media::ProbeError>> {
        let len = self.0.len();
        Box::pin(async move { Ok(len) })
    }

    fn read_at(
        &self,
        offset: u64,
        len: u64,
    ) -> futures_util::future::BoxFuture<'_, Result<bytes::Bytes, mq_media::ProbeError>> {
        Box::pin(async move {
            use tokio::io::AsyncReadExt;
            let err = |e: String| mq_media::ProbeError::Http(e);
            let mut range = self
                .0
                .range(offset, Some(offset.saturating_add(len)))
                .await
                .map_err(|e| err(e.to_string()))?;
            let mut out = Vec::new();
            range
                .read_to_end(&mut out)
                .await
                .map_err(|e| err(e.to_string()))?;
            Ok(out.into())
        })
    }
}

/// What to ask a debrid store for this stream, with the service the addon
/// named; `None` for sources that need no debrid.
pub(crate) fn debrid_request(
    stream: &ParsedStream,
    season: Option<u32>,
    episode: Option<u32>,
) -> Option<(Option<String>, ResolveRequest)> {
    match &stream.source {
        Source::ClientResolve(c) => Some((
            c.service.clone().or_else(|| stream.service.clone()),
            ResolveRequest {
                store: None,
                info_hash: c.info_hash.clone()?,
                file_idx: c.file_idx,
                filename_hint: c.filename.clone(),
                season: c.season.or(season),
                episode: c.episode.or(episode),
            },
        )),
        Source::Torrent {
            info_hash,
            file_idx,
            ..
        } => Some((
            stream.service.clone(),
            ResolveRequest {
                store: None,
                info_hash: info_hash.clone(),
                file_idx: *file_idx,
                filename_hint: stream.filename.clone(),
                season,
                episode,
            },
        )),
        _ => None,
    }
}

/// Uses the store the addon named when that account is configured, else the
/// default store.
fn with_store(
    resolver: &Resolver,
    service: Option<&str>,
    mut request: ResolveRequest,
) -> ResolveRequest {
    request.store = service
        .and_then(StoreId::from_service)
        .filter(|id| resolver.stores().contains(id));
    request
}

/// Warms the debrid link of a version the user is likely to play, so Play
/// starts from the resolver cache. Only versions the addon reports cached:
/// asking a store for an uncached torrent adds it to the user's account.
pub(crate) fn prefetch(
    core: &Core,
    stream: &ParsedStream,
    season: Option<u32>,
    episode: Option<u32>,
) {
    if stream.cached != Some(true) {
        return;
    }
    let (Some(resolver), Some((service, request))) =
        (core.debrid(), debrid_request(stream, season, episode))
    else {
        return;
    };
    let request = with_store(&resolver, service.as_deref(), request);
    tokio::spawn(async move {
        if let Err(e) = resolver.prefetch(request).await {
            tracing::debug!("debrid prefetch failed: {e}");
        }
    });
}

fn source_word(source: &Source) -> &'static str {
    match source {
        Source::YouTube { .. } => "YouTube",
        Source::PlayerFrame { .. } => "Embedded player",
        Source::External { .. } => "External",
        Source::Usenet { .. } => "Usenet",
        Source::Archive { .. } => "Archive",
        _ => "These",
    }
}

/// Hears the subtitle route's failures for this run's tracks. Weak: the
/// state owns the lease, which owns this reporter.
fn listen_for_trouble(
    slot: &SubtitleSlot,
    state: Weak<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    run: u64,
) {
    slot.on_trouble(move |url, trouble| {
        let Some(state) = state.upgrade() else {
            return;
        };
        let mut s = lock(&state);
        if current(&s, run) && note_trouble(&mut s.snapshot, url, trouble) {
            snapshot.set(s.snapshot.clone());
        }
    });
}

/// The note for a track the route could not serve; a file that cannot be
/// read also leaves the list. False when nothing changed.
fn note_trouble(snapshot: &mut PlaybackSnapshot, url: &str, trouble: SubtitleTrouble) -> bool {
    let Some(at) = snapshot.subtitles.iter().position(|t| t.url == url) else {
        return false;
    };
    let (source, problem) = match trouble {
        SubtitleTrouble::Unreachable => (
            snapshot.subtitles[at].source.clone(),
            SubtitleProblem::Unreachable,
        ),
        SubtitleTrouble::Unreadable => (
            snapshot.subtitles.remove(at).source,
            SubtitleProblem::TrackFailed,
        ),
    };
    let removed = trouble == SubtitleTrouble::Unreadable;
    add_note(snapshot, source, problem) || removed
}

/// Adds the note once; false when it was there.
fn add_note(snapshot: &mut PlaybackSnapshot, source: String, problem: SubtitleProblem) -> bool {
    let note = SubtitleNote { source, problem };
    let new = !snapshot.subtitle_notes.contains(&note);
    if new {
        snapshot.subtitle_notes.push(note);
    }
    new
}

fn is_built_in(addon: &InstalledAddon) -> bool {
    turkcealtyazi::is_builtin(addon.transport_url.as_str(), Some(&addon.manifest.id))
}

/// The built-in TurkceAltyazi.org tracks (plans/m13-turkcealtyazi.md 3.8):
/// listed at once, ranked when the frame rate is read (the probe waits at
/// most 1.5 s), placed where its row sits among the addons. Once, however
/// many rows stand for it.
#[allow(clippy::too_many_arguments)]
async fn built_in_tracks(
    core: &Core,
    target: &PlayTarget,
    stream: &ParsedStream,
    installed: &[InstalledAddon],
    probed: impl std::future::Future<Output = Probed>,
    wanted: &[String],
    slot: &SubtitleSlot,
    run: u64,
    state: &Arc<Mutex<State>>,
    snapshot: &SharedObservable<PlaybackSnapshot>,
) {
    let Some(position) = installed.iter().position(is_built_in) else {
        return;
    };
    let (Some(digits), Some(want)) = (
        turkcealtyazi::digits(&target.item_id),
        turkcealtyazi::target(target),
    ) else {
        return;
    };
    let site = core.turkcealtyazi();
    let release = &stream.release;
    let mut playing = mq_turkcealtyazi::Playing {
        group: release_group(release),
        source: release.quality,
        cut: release.cut,
        fps: None,
        part: fit::part(stream_name(stream)),
    };
    let probe = async { probed.await.fps };
    let listed = tokio::time::timeout(Deadlines::default().hard, async {
        // The request goes now; the ranking, from the cache, waits for the
        // frame rate.
        let (fps, _) = tokio::join!(probe, site.list(digits, want, &playing));
        playing.fps = fps.map(|fps| fps as f32);
        site.list(digits, want, &playing).await
    })
    .await;
    let listing = listed.unwrap_or(mq_turkcealtyazi::Listing::Problem(
        mq_turkcealtyazi::Problem::Unreachable,
    ));
    let owned = OwnedPlaying {
        group: playing.group.map(str::to_owned),
        source: playing.source,
        cut: playing.cut,
        fps: playing.fps,
        part: playing.part,
        ..OwnedPlaying::default()
    };
    let mut s = lock(state);
    if !current(&s, run) {
        return;
    }
    // The measured rate is the view's to set (`rated`): a listing cut by
    // its deadline has none and must not erase it.
    match listing {
        mq_turkcealtyazi::Listing::Tracks(tracks) => {
            for (rank, found) in tracks.into_iter().enumerate() {
                let url_cell = Arc::new(OnceLock::new());
                let fetch = BuiltInFetch {
                    site: site.clone(),
                    row: found.row.clone(),
                    target: want,
                    playing: owned.clone(),
                    url: url_cell.clone(),
                    state: Arc::downgrade(state),
                    snapshot: snapshot.clone(),
                    run,
                };
                let key = format!("turkcealtyazi:{}", found.row.altid);
                let Some(url) = slot.add_fetched(&key, "tur", fetch.into_fetch()) else {
                    return;
                };
                let _ = url_cell.set(url.clone());
                s.order.insert(url.clone(), (position + 1, rank));
                if !s.snapshot.subtitles.iter().any(|known| known.url == url) {
                    s.snapshot.subtitles.push(built_in_track(url, key, found));
                }
            }
        }
        mq_turkcealtyazi::Listing::Nothing => return,
        mq_turkcealtyazi::Listing::Problem(p) => {
            add_note(&mut s.snapshot, BUILT_IN.into(), problem(p));
        }
    }
    reorder(&mut s, wanted);
    snapshot.set(s.snapshot.clone());
}

/// A built-in TurkceAltyazi.org track: its own label already says what
/// tells its files apart.
fn built_in_track(url: String, origin: String, found: mq_turkcealtyazi::Track) -> SubtitleTrack {
    let label = Some(found.label.clone());
    SubtitleTrack {
        hearing_impaired: found.row.hearing_impaired,
        made: Made::of(found.row.made()),
        name: found.label.clone(),
        base: found.label.clone(),
        origin,
        ..SubtitleTrack::new(url, "tur", label, BUILT_IN, Some(found.label))
    }
}

/// What the playing release says (`fit::Playing`), owned by the view and
/// by a track's fetch.
#[derive(Clone, Default)]
struct OwnedPlaying {
    group: Option<String>,
    source: mq_relparse::Quality,
    cut: Option<mq_relparse::Cut>,
    fps: Option<f32>,
    part: Option<u8>,
    /// Its file name has the word "forced": a title's word, so a
    /// subtitle's name saying it marks nothing.
    title_forced: bool,
}

impl OwnedPlaying {
    /// What its release name says; the frame rate comes from the probe,
    /// the disc from the file name (`fit::part`).
    fn of(release: &mq_relparse::ParsedRelease) -> Self {
        Self {
            group: release_group(release).map(str::to_owned),
            source: release.quality,
            cut: release.cut,
            fps: None,
            part: None,
            title_forced: false,
        }
    }

    /// The playing stream's: its release, and the disc its name says.
    fn of_stream(stream: &ParsedStream) -> Self {
        Self {
            part: fit::part(stream_name(stream)),
            title_forced: forced_mark(stream_name(stream)),
            ..Self::of(&stream.release)
        }
    }

    fn get(&self) -> fit::Playing<'_> {
        fit::Playing {
            group: self.group.as_deref(),
            source: self.source,
            cut: self.cut,
            fps: self.fps,
            part: self.part,
        }
    }
}

/// One built-in track's fetch, run by the subtitle route each time the
/// track loads. `state` is weak: the state owns the lease, which owns this.
#[derive(Clone)]
struct BuiltInFetch {
    site: Arc<mq_turkcealtyazi::Site>,
    row: mq_turkcealtyazi::Row,
    target: mq_turkcealtyazi::Target,
    playing: OwnedPlaying,
    /// The track's URL, known once the slot gave it.
    url: Arc<OnceLock<String>>,
    state: Weak<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    run: u64,
}

impl BuiltInFetch {
    fn into_fetch(self) -> mq_server::Fetch {
        let this = Arc::new(self);
        Arc::new(move || {
            let this = this.clone();
            Box::pin(async move { this.fetch().await })
        })
    }

    /// The file's bytes; else the note (and a faulty file leaves the list,
    /// for 6 h) and nothing.
    async fn fetch(&self) -> Option<Vec<u8>> {
        let fetched = self
            .site
            .fetch(&self.row, self.target, &self.playing.get())
            .await;
        if let mq_turkcealtyazi::Fetched::File(bytes) = fetched {
            return Some(bytes);
        }
        let state = self.state.upgrade()?;
        let mut s = lock(&state);
        if current(&s, self.run) && note_fetched(&mut s.snapshot, self.url.get(), &fetched) {
            self.snapshot.set(s.snapshot.clone());
        }
        None
    }
}

/// The note for a built-in fetch that gave no file: the site's problem
/// keeps the track; a fault of the file takes it off the list.
fn note_fetched(
    snapshot: &mut PlaybackSnapshot,
    url: Option<&String>,
    fetched: &mq_turkcealtyazi::Fetched,
) -> bool {
    use mq_turkcealtyazi::Fetched;
    let note = match fetched {
        Fetched::File(_) => return false,
        Fetched::Problem(p) => problem(*p),
        Fetched::Fault {
            downloads_changed: true,
            ..
        } => SubtitleProblem::Changed,
        Fetched::Fault { .. } => SubtitleProblem::TrackFailed,
    };
    let before = snapshot.subtitles.len();
    if let (Fetched::Fault { .. }, Some(url)) = (fetched, url) {
        snapshot.subtitles.retain(|t| &t.url != url);
    }
    let removed = snapshot.subtitles.len() != before;
    add_note(snapshot, BUILT_IN.into(), note) || removed
}

fn problem(p: mq_turkcealtyazi::Problem) -> SubtitleProblem {
    match p {
        mq_turkcealtyazi::Problem::Unreachable => SubtitleProblem::Unreachable,
        mq_turkcealtyazi::Problem::Blocked => SubtitleProblem::Blocked,
        mq_turkcealtyazi::Problem::Changed => SubtitleProblem::Changed,
    }
}

/// A track served through the loopback server as UTF-8 WebVTT, whatever
/// format and charset the addon's file has; `None` once its playback is
/// released. `title_forced`: the playing file's name has the word
/// "forced" (`OwnedPlaying::title_forced`).
fn track(
    w: &mq_protocol::WireSubtitle,
    source: &str,
    addon_id: &str,
    url: String,
    title_forced: bool,
) -> SubtitleTrack {
    let t = SubtitleTrack::new(url, &w.lang, w.label.clone(), source, release(w));
    let hearing_impaired = w.hearing_impaired || t.hearing_impaired;
    // The word in a film's own title ("Forced Vengeance") is in every
    // file's name: then only the flag counts.
    // ponytail: a real forced file of such a film needs the addon's flag.
    let named = !title_forced && t.release.as_deref().is_some_and(forced_mark);
    let forced = w.forced || named;
    // OpenSubtitles marks a match on the hash we sent with `"m": "h"`; any
    // other addon could mark every row, so only its mark counts. Its
    // `fpsMilli` is the rate the file was timed for.
    let hash = addon_id == OPENSUBTITLES && w.extra.get("m").and_then(|m| m.as_str()) == Some("h");
    let fps = w
        .extra
        .get("fpsMilli")
        .and_then(|f| f.as_f64())
        .filter(|f| *f > 0.0)
        .map(|f| (f / 1000.0) as f32);
    let origin = if addon_id.is_empty() || w.id.is_empty() {
        w.url.to_string()
    } else {
        format!("{addon_id}:{}", w.id)
    };
    SubtitleTrack {
        forced,
        hearing_impaired,
        origin,
        made: Made {
            hash,
            fps,
            ..t.made.clone()
        },
        ..t
    }
}

/// A release text that marks a forced track as a word of its own
/// ("Film.2020.forced.srt", "English (Forced)"): OpenSubtitles' rows carry
/// it in the file name, not in the flag.
fn forced_mark(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.eq_ignore_ascii_case("forced"))
}

/// The OpenSubtitles v3 addon's manifest id: the one whose hash match counts.
const OPENSUBTITLES: &str = "org.stremio.opensubtitlesv3";

/// The release a file was made for, as the addon names it: its label, or
/// OpenSubtitles' file name (without its extension) or release name.
fn release(w: &mq_protocol::WireSubtitle) -> Option<String> {
    let extra = |key: &str| {
        w.extra
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let file = extra("subtitleFileName").map(|f| {
        let stem = f
            .rsplit_once('.')
            .map_or(f, |(stem, ext)| if ext.len() <= 4 { stem } else { f });
        stem.to_owned()
    });
    // A label that only names the language says nothing of the release.
    let language = crate::languages::language(&w.lang);
    let text = w
        .label
        .clone()
        .filter(|l| {
            let l = l.trim();
            !l.is_empty()
                && !l.eq_ignore_ascii_case(&language.name)
                && !l.eq_ignore_ascii_case(&w.lang)
        })
        .or(file)
        .or_else(|| extra("movieReleaseName").map(str::to_owned))?;
    // At most 80 characters: a line in a menu.
    Some(text.chars().take(80).collect())
}

/// What the probe read of the playing file: its frame rate and its audio
/// language (the default track's, else the first's), as tags.
#[derive(Debug, Clone, Default)]
struct Probed {
    fps: Option<f64>,
    audio: Vec<String>,
}

/// `mq_media::probe` on the playback's reader, within 2 MB and 1.5 s;
/// nothing without a reader.
async fn probe_video(reader: Option<&dyn mq_media::RangeReader>) -> Probed {
    let Some(reader) = reader else {
        return Probed::default();
    };
    let budget = mq_media::ProbeBudget::default();
    let found = mq_media::probe(reader, budget, &mq_media::CancellationToken::new()).await;
    let fps = found.video.as_ref().and_then(|v| v.fps.value);
    let track = found
        .audio
        .iter()
        .find(|a| a.default.value == Some(true))
        .or(found.audio.first());
    let audio = track
        .and_then(|a| a.language.value.clone())
        .filter(|l| !l.eq_ignore_ascii_case("und"))
        .map(|l| crate::languages::language(&l).tag)
        .into_iter()
        .collect();
    Probed { fps, audio }
}

/// The audio's languages: the file's, else the release's when the file says none.
fn audio_of(probed: Probed, stream: &ParsedStream) -> Vec<String> {
    if !probed.audio.is_empty() {
        return probed.audio;
    }
    stream
        .release
        .languages
        .iter()
        .map(|l| crate::languages::language(l).tag)
        .collect()
}

/// What the start subtitle is chosen with, read once per settle: the
/// title's remembered choice, the profile's subtitle languages and the
/// audio's.
struct StartWants {
    remembered: Option<String>,
    explicit: Vec<String>,
    audio: Vec<String>,
}

impl StartWants {
    fn read(core: &Core, target: &PlayTarget, audio: Vec<String>) -> Self {
        let explicit = core
            .prefs()
            .subtitle_languages
            .iter()
            .map(|l| crate::languages::language(l).tag)
            .collect();
        Self {
            remembered: remembered_subtitle(core, target),
            explicit,
            audio,
        }
    }

    fn first(&self) -> Option<&str> {
        first_language(self.remembered.as_deref(), &self.explicit, &self.audio)
    }
}

/// Decides the source's start subtitle once: early (`last` false) only when
/// a track in the first language (`first_language`) is listed, as a slower
/// source may bring one; any other pick when every source has answered,
/// which also ends `subtitles_pending`. A pick before it stays the pick.
/// True once there is nothing left to decide early.
fn settle_start(
    state: &Mutex<State>,
    snapshot: &SharedObservable<PlaybackSnapshot>,
    run: u64,
    wants: &StartWants,
    last: bool,
) -> bool {
    let mut s = lock(state);
    if !current(&s, run) {
        return true;
    }
    if s.start_settled || (s.subtitle_picked && !last) {
        if last {
            s.snapshot.subtitles_pending = false;
            snapshot.set(s.snapshot.clone());
        }
        return true;
    }
    let pick = auto_subtitle(
        wants.remembered.as_deref(),
        &wants.explicit,
        &wants.audio,
        &s.snapshot.subtitles,
    )
    .filter(|_| !s.subtitle_picked);
    let first = wants.first();
    let in_first = pick.as_ref().is_some_and(|url| {
        s.snapshot.subtitles.iter().any(|t| {
            &t.url == url
                && first.is_some_and(|f| {
                    crate::languages::primary(&t.language) == crate::languages::primary(f)
                })
        })
    });
    if !(in_first || last) {
        return false;
    }
    s.start_settled = true;
    s.snapshot.auto_subtitle = pick;
    if last {
        s.snapshot.subtitles_pending = false;
    }
    snapshot.set(s.snapshot.clone());
    true
}

/// The language the start subtitle wants most: the title's remembered one,
/// else the profile's first subtitle language the audio is not in; None for
/// Off or none set.
fn first_language<'a>(
    remembered: Option<&'a str>,
    explicit: &'a [String],
    audio: &[String],
) -> Option<&'a str> {
    use crate::languages::primary;
    match remembered {
        Some(OFF) => None,
        Some(tag) => Some(tag),
        None => explicit
            .iter()
            .find(|e| !audio.iter().any(|a| primary(a) == primary(e)))
            .map(String::as_str),
    }
}

/// The track the apps turn on at start, if any (plans/m13-subtitle-languages.md
/// 6), from the snapshot's order:
/// - the user's own choice for this title wins: Off turns nothing on, a
///   language turns on its best track;
/// - else the profile's subtitle languages in order, each skipped when the
///   audio is in it: the best track of the first left that has one.
fn auto_subtitle(
    remembered: Option<&str>,
    explicit: &[String],
    audio: &[String],
    tracks: &[SubtitleTrack],
) -> Option<String> {
    use crate::languages::primary;
    // A forced track only covers foreign lines: never turned on as a
    // language's subtitles.
    let plain = || tracks.iter().filter(|t| !t.forced);
    let best = |tag: &str| {
        let exact = plain().find(|t| t.language == tag);
        exact
            .or_else(|| plain().find(|t| primary(&t.language) == primary(tag)))
            .map(|t| t.url.clone())
    };
    match remembered {
        Some(OFF) => None,
        Some(tag) => best(tag),
        None => explicit
            .iter()
            // Subtitles in the audio's own language are not wanted.
            .filter(|e| !audio.iter().any(|a| primary(a) == primary(e)))
            .find_map(|e| best(e)),
    }
}

/// How a title's memory says the user turned subtitles off.
const OFF: &str = "off";

/// How long the start subtitle waits for every subtitle source before it
/// goes with the tracks listed: OpenSubtitles answers in 0.1 to 0.2 s and
/// the first frame shows at about 0.4 s, so subtitles come with the first
/// seconds of picture (the owner's lead, 2026-10-23).
const START_SUBTITLE_WAIT: Duration = Duration::from_secs(1);

/// The profile's subtitle languages with `tag`, the language of a track the
/// user picked, first: each language once, by its tag (an older "tur" gives
/// way to "tr"), a regional tag kept as picked. None when it is first
/// already, or when the track names no language the core knows.
fn picked_first(languages: &[String], tag: &str) -> Option<Vec<String>> {
    use crate::languages::{is_known, language, primary};
    if !is_known(primary(tag)) || languages.first().is_some_and(|l| l == tag) {
        return None;
    }
    let rest = languages.iter().filter(|l| language(l).tag != tag).cloned();
    Some(std::iter::once(tag.to_owned()).chain(rest).collect())
}

/// The user's subtitle choice for this title: `OFF` or a language tag.
fn remembered_subtitle(core: &Core, target: &PlayTarget) -> Option<String> {
    stream_memory(core, target)?
        .get("subtitle")?
        .as_str()
        .map(str::to_owned)
}

/// Where a title's memory keeps the one subtitle delay it remembers:
/// `{key, videoId, origin, ratio, offsetMs}` (plans/player-polish.md 2g).
const DELAY: &str = "subtitleDelay";

/// The stepper's reach either way.
const MAX_DELAY_MS: i64 = 60_000;

/// The title's stream memory (synced: it belongs to the file pair, not the
/// device).
fn stream_memory(core: &Core, target: &PlayTarget) -> Option<serde_json::Value> {
    let memory = core
        .library()
        .stream_memory(core.profile(), &target.item_id)
        .ok()??;
    serde_json::from_str(&memory).ok()
}

/// The subtitle delay the title remembers for the version `key` of this
/// video: the track's origin and its ms.
fn remembered_delay(core: &Core, target: &PlayTarget, key: &str) -> Option<(String, i32)> {
    let memory = stream_memory(core, target)?;
    let delay = &memory[DELAY];
    if delay["key"].as_str() != Some(key) || delay["videoId"].as_str() != Some(&target.video_id) {
        return None;
    }
    let ms = delay["offsetMs"]
        .as_i64()?
        .clamp(-MAX_DELAY_MS, MAX_DELAY_MS) as i32;
    Some((delay["origin"].as_str()?.to_owned(), ms))
}

/// A track's remembered delay: `delay`'s when it names its origin.
fn delay_of(delay: &Option<(String, i32)>, origin: &str) -> i32 {
    delay
        .as_ref()
        .filter(|(o, _)| o == origin)
        .map_or(0, |(_, ms)| *ms)
}

/// The video's bytes in process, for the subtitle hash and the frame rate:
/// over the playback's own cache and cursors; none for passthrough.
async fn video_reader(
    upstream: &Upstream,
    reader: Option<Box<dyn mq_media::RangeReader>>,
) -> Option<Box<dyn mq_media::RangeReader>> {
    match upstream {
        Upstream::Http { .. } => reader,
        #[cfg(feature = "torrent")]
        Upstream::Torrent { claim, file } => match claim.open_reader(*file).await {
            Ok(reader) => Some(Box::new(TorrentBytes(reader)) as Box<dyn mq_media::RangeReader>),
            Err(e) => {
                tracing::warn!("torrent reader for the subtitle hash: {e}");
                None
            }
        },
    }
}

/// The OpenSubtitles hash and size when they can be had (first and last
/// 64 KiB, read in process for files), and the filename.
async fn video_extra(
    reader: Option<&dyn mq_media::RangeReader>,
    filename: Option<&str>,
) -> Vec<ExtraValue> {
    let mut extra = Vec::new();
    // Origins that ignore Range give no hash; filename and id still match.
    if let Some(reader) = reader
        && let Ok(Ok((hash, size))) =
            tokio::time::timeout(HASH_TIMEOUT, mq_media::opensubtitles_hash(reader)).await
    {
        extra.push(extra_value("videoHash", hash));
        extra.push(extra_value("videoSize", size.to_string()));
    }
    if let Some(name) = filename {
        extra.push(extra_value("filename", name.to_owned()));
    }
    extra
}

/// Asks every subtitles addon for this video with `extra`, and adds tracks
/// to the snapshot as each addon answers, while `run` is still the view's.
/// The built-in source's rows are never asked (`built_in_tracks`).
async fn find_subtitles(
    core: &Core,
    target: &PlayTarget,
    installed: &[InstalledAddon],
    extra: Vec<ExtraValue>,
    wanted: &[String],
    (slot, run, state, snapshot): (
        &SubtitleSlot,
        u64,
        &Mutex<State>,
        &SharedObservable<PlaybackSnapshot>,
    ),
) {
    let path = ResourcePath {
        resource: "subtitles".into(),
        r#type: target.kind.clone(),
        id: target.video_id.clone(),
        extra,
    };
    let addons: Vec<_> = installed
        .iter()
        .filter(|a| a.supports(&path) && !is_built_in(a))
        .cloned()
        .collect();
    let mut fanout = core.addons().fan_out(&addons, &path, Deadlines::default());
    while let Some(event) = fanout.next().await {
        let AddonEvent::Arrived {
            addon_id,
            transport_url,
            body,
            ..
        } = event
        else {
            continue;
        };
        let addon = addons.iter().find(|a| a.transport_url == transport_url);
        let source = addon.map_or(addon_id.clone(), |a| a.manifest.name.clone());
        let position = installed
            .iter()
            .position(|a| a.transport_url == transport_url)
            .map_or(usize::MAX, |at| at + 1);
        let found = decode_subtitles(&body)
            .map(|r| r.subtitles)
            .unwrap_or_default();
        // One file, one loopback address: compare by it. The fetch has the
        // scope of the install that answered.
        let mut listed = Vec::with_capacity(found.len());
        for (rank, w) in found.iter().enumerate() {
            let Some(url) = slot.add(w.url.as_str(), &w.lang, Some(&transport_url)) else {
                return;
            };
            listed.push((rank, url, w));
        }
        // Read outside the playback's lock, once per file not listed yet.
        let (known, title_forced): (std::collections::HashSet<String>, bool) = {
            let s = lock(state);
            if !current(&s, run) {
                return;
            }
            let known = s.snapshot.subtitles.iter().map(|t| t.url.clone()).collect();
            (known, s.playing.title_forced)
        };
        let fresh: Vec<SubtitleTrack> = listed
            .iter()
            .filter(|(_, url, _)| !known.contains(url))
            .map(|(_, url, w)| track(w, &source, &addon_id, url.clone(), title_forced))
            .collect();
        let mut s = lock(state);
        if !current(&s, run) {
            return;
        }
        for (rank, url, _) in &listed {
            s.order.entry(url.clone()).or_insert((position, *rank));
        }
        for t in fresh {
            if !s.snapshot.subtitles.iter().any(|known| known.url == t.url) {
                s.snapshot.subtitles.push(t);
            }
        }
        reorder(&mut s, wanted);
        snapshot.set(s.snapshot.clone());
    }
}

fn extra_value(name: &str, value: String) -> ExtraValue {
    ExtraValue {
        name: name.into(),
        value,
    }
}

/// The languages wanted most, first, as language tags: the profile's
/// subtitle languages, then the audio languages the pick was ranked with
/// (the profile's, else the device's, then English, as for audio), or the
/// profile's audio languages without a pick.
fn preferred_languages(core: &Core, state: &Mutex<State>) -> Vec<String> {
    let prefs = core.prefs();
    let picked = lock(state).languages.clone();
    let audio = if picked.is_empty() {
        prefs.streams.languages
    } else {
        picked
    };
    let mut tags: Vec<String> = Vec::new();
    for code in prefs.subtitle_languages.iter().chain(&audio) {
        let tag = crate::languages::language(code).tag;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// Tracks grouped by language: the preferred languages first in their
/// order (a regional one, "pt-BR", counts with its language), then the
/// others by English name. Within a language, by the release match against
/// the playing release (`fit::misses`, plans/player-polish.md 2g: forced
/// and split files last, then the file's own hash, the group, a capture, a
/// PAL rate, the cut, the source family, a track for the hard of hearing
/// after a plain one, the same rate), then by the source's place in the
/// installed list (the stream's own first), then the source's own rank,
/// whenever each answered. Then each track's `name`: two that would read
/// the same in one language are numbered in that order. A track `fits`
/// when it names the playing group.
fn reorder(s: &mut State, preferred: &[String]) {
    let State {
        snapshot,
        order,
        playing,
        delay,
        ..
    } = s;
    let playing = playing.get();
    let rank = |tag: &str| {
        let primary = crate::languages::primary(tag);
        preferred.iter().position(|p| p == tag).or_else(|| {
            preferred
                .iter()
                .position(|p| crate::languages::primary(p) == primary)
        })
    };
    snapshot.subtitles.sort_by_cached_key(|t| {
        (
            rank(&t.language).unwrap_or(usize::MAX),
            t.language_name.to_lowercase(),
            t.language.clone(),
            fit::misses(&t.made(), &playing),
            order.get(&t.url).copied().unwrap_or((usize::MAX, 0)),
        )
    });
    let base = |t: &SubtitleTrack| t.base.clone();
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut total: HashMap<(String, String), usize> = HashMap::new();
    for t in &snapshot.subtitles {
        *total.entry((t.language.clone(), base(t))).or_default() += 1;
    }
    for t in &mut snapshot.subtitles {
        let key = (t.language.clone(), base(t));
        let n = seen.entry(key.clone()).or_default();
        *n += 1;
        t.name = if total[&key] > 1 {
            format!("{} {}", key.1, n)
        } else {
            key.1
        };
        t.fits = fit::same_group(&t.made.text, playing.group);
        t.hash_match = t.made.hash;
        t.delay_ms = delay_of(delay, &t.origin);
        t.made_for_fps = fit::misses(&t.made(), &playing)
            .rate
            .then_some(t.made.fps)
            .flatten();
    }
}

/// The episode after this one in season and episode order, skipping
/// specials (season 0) unless a special is playing and episodes not out yet.
async fn next_episode(core: &Core, target: &PlayTarget) -> Option<NextEpisode> {
    target.season?;
    let (meta, _) = core.fetch_meta(&target.kind, &target.item_id).await.ok()?;
    next_after(&meta, &target.video_id)
}

/// The released episode after `video_id` in `meta`, shared with Home's
/// Next Up. An episode not out yet ends the search: the ones after it are
/// not out either, whatever their dates say.
pub(crate) fn next_after(meta: &mq_protocol::MetaItem, video_id: &str) -> Option<NextEpisode> {
    let now = mq_library::hlc::now_ms() as i64;
    following(meta, video_id)
        .filter(|(v, _)| aired(v, now))
        .map(|(v, s)| NextEpisode {
            video_id: v.id.clone(),
            title: v.title.clone(),
            season: s.season,
            episode: s.episode,
            thumbnail: v.thumbnail.clone(),
        })
}

/// The episode after `video_id` in season and episode order, out or not,
/// skipping specials (season 0) unless a special is the current one. Shared
/// by Next Up, the player's Up Next and the title page's Play.
pub(crate) fn following<'a>(
    meta: &'a mq_protocol::MetaItem,
    video_id: &str,
) -> Option<(
    &'a stremio_core::types::resource::Video,
    &'a stremio_core::types::resource::SeriesInfo,
)> {
    let current = meta
        .videos
        .iter()
        .find(|v| v.id == video_id)?
        .series_info
        .clone()?;
    meta.videos
        .iter()
        .filter_map(|v| Some((v, v.series_info.as_ref()?)))
        .filter(|(_, s)| s.season > 0 || current.season == 0)
        .filter(|(_, s)| (s.season, s.episode) > (current.season, current.episode))
        .min_by_key(|(_, s)| (s.season, s.episode))
}

/// Out by `now_ms`: no date counts as out.
pub(crate) fn aired(video: &stremio_core::types::resource::Video, now_ms: i64) -> bool {
    video
        .released
        .is_none_or(|r| r.timestamp_millis() <= now_ms)
}

/// Where a play of a video resumes, from its saved `(position, duration)`:
/// none at its start or once it is finished.
pub(crate) fn resume_point(progress: Option<(i64, i64)>) -> Option<u64> {
    progress
        .filter(|&(pos, dur)| pos > 0 && (dur == 0 || (pos as f64) < dur as f64 * FINISHED_RATIO))
        .map(|(pos, _)| pos as u64)
}

/// The name a stream's file goes by: its file name, else the addon's.
fn stream_name(stream: &ParsedStream) -> &str {
    stream.filename.as_deref().unwrap_or(&stream.raw_name)
}

/// Keeps the stream that started playing for this title, so Versions for its
/// next episode prefers the same release line; for a torrent, the file it
/// played (`played`: its index and length) and the video, for "On this
/// device" (`kept_start`).
fn remember_stream(
    core: &Core,
    target: &PlayTarget,
    stream: &ParsedStream,
    played: Option<(usize, u64)>,
) {
    let mut memory = serde_json::json!({
        "key": stream.key,
        "addonId": stream.addon_id,
        "release": stream_name(stream),
    });
    if let (Some((file, len)), Source::Torrent { info_hash, .. }) = (played, &stream.source) {
        memory["torrent"] = serde_json::json!({
            "hash": info_hash.to_ascii_lowercase(),
            "file": file,
            "len": len,
            "videoId": target.video_id,
        });
    }
    let old = stream_memory(core, target).unwrap_or_default();
    // The user's subtitle choice for the title stays, and the subtitle
    // delay while the version and the video are the same.
    if let Some(choice) = old.get("subtitle") {
        memory["subtitle"] = choice.clone();
    }
    let delay = &old[DELAY];
    if delay["key"].as_str() == Some(&stream.key)
        && delay["videoId"].as_str() == Some(&target.video_id)
    {
        memory[DELAY] = delay.clone();
    }
    if let Err(e) = core.library().set_stream_memory(
        core.profile(),
        &target.item_id,
        stream.binge_group.as_deref(),
        &memory.to_string(),
        core.clock(),
    ) {
        tracing::warn!("stream memory write failed: {e}");
    }
}

/// The core's words for a version, or a title, that starts from what this
/// device keeps (plans/player-polish.md 2b).
pub(crate) const ON_DEVICE: &str = "On this device";

/// The key of the stream `item_id` last played for `video_id`, when it is a
/// torrent whose start at the video's resume point this device holds: its
/// details and what Ready waits on there are on disk (`Engine::cached`), so
/// it starts with no peer. Starts no engine: with none running, the data
/// folder answers alone (`mq_engine::cached_on_disk`).
#[cfg(feature = "torrent")]
pub(crate) async fn kept_start(core: &Core, item_id: &str, video_id: &str) -> Option<String> {
    let memory = core
        .library()
        .stream_memory(core.profile(), item_id)
        .ok()??;
    let memory: serde_json::Value = serde_json::from_str(&memory).ok()?;
    let kept = &memory["torrent"];
    if kept["videoId"].as_str() != Some(video_id) {
        return None;
    }
    let (hash, file, len) = (
        kept["hash"].as_str()?,
        kept["file"].as_u64()?,
        kept["len"].as_u64()?,
    );
    let progress = core.library().progress(core.profile(), video_id).ok()?;
    // As the playback places it; its head with the duration unknown.
    let at = resume_point(progress)
        .zip(progress.map(|(_, dur)| dur.max(0) as u64))
        .and_then(|(pos, dur)| resume_offset(pos, dur, len))
        .unwrap_or(0);
    // A running engine answers for what it has loaded; else the data folder
    // answers alone, and no engine starts (no DHT or trackers at a launch).
    let file = file as usize;
    let cached = match core.running_engine() {
        Some(engine) => engine.cached(hash, file, at).await?,
        None => {
            let (dir, hash) = (core.torrent_dir().to_owned(), hash.to_owned());
            tokio::task::spawn_blocking(move || mq_engine::cached_on_disk(&dir, &hash, file, at))
                .await
                .ok()??
        }
    };
    cached
        .ready
        .then(|| memory["key"].as_str().map(str::to_owned))
        .flatten()
}

/// The stream `key` names in `item_id`'s stream memory, built from what
/// the memory keeps (the torrent, its file and release), so it starts with
/// no addon: "Play from this device" when every source failed.
#[cfg(feature = "torrent")]
pub(crate) fn kept_stream(core: &Core, item_id: &str, key: &str) -> Option<ParsedStream> {
    let memory = core
        .library()
        .stream_memory(core.profile(), item_id)
        .ok()??;
    let memory: serde_json::Value = serde_json::from_str(&memory).ok()?;
    if memory["key"].as_str() != Some(key) {
        return None;
    }
    let kept = &memory["torrent"];
    let release = memory["release"].as_str().unwrap_or_default();
    let body = serde_json::json!({ "streams": [{
        "infoHash": kept["hash"].as_str()?,
        "fileIdx": kept["file"].as_u64()?,
        "name": release,
        "behaviorHints": { "filename": release },
    }]});
    let wire = mq_protocol::decode_streams(body.to_string().as_bytes()).ok()?;
    let addon = memory["addonId"].as_str().unwrap_or_default();
    let mut stream = mq_streams::normalize(wire.streams.first()?, addon, ON_DEVICE, 0, 0);
    stream.key = key.to_owned();
    // From this device's engine, an account connected or not.
    stream.cached = Some(false);
    Some(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words for a move: the version left and why, or "another" when
    /// both versions read the same.
    /// A torrent pack's file names its container: an mkv in a release
    /// listed as mp4 is an mkv; without a file name, what an earlier play
    /// opened; then the release's; else nothing.
    #[test]
    fn the_playing_container_follows_the_opened_file() {
        let none = || None;
        assert_eq!(
            playing_container(Some("Pack/Movie.2010.1080p.mkv"), none, Some("mp4")).as_deref(),
            Some("mkv")
        );
        assert_eq!(
            playing_container(None, || Some("avi".into()), Some("mp4")).as_deref(),
            Some("avi")
        );
        assert_eq!(
            playing_container(Some("stream"), none, Some("mp4")).as_deref(),
            Some("mp4")
        );
        assert_eq!(playing_container(None, none, None), None);
    }

    #[test]
    fn moved_words_name_both_versions_and_why() {
        assert_eq!(
            moved_words("720p WEB", "1080p Blu-ray", "did not start here"),
            "Playing 720p WEB: the 1080p Blu-ray did not start here"
        );
        assert_eq!(
            moved_words("720p WEB", "1080p WEB", "did not play on this device"),
            "Playing 720p WEB: the 1080p WEB did not play on this device"
        );
        assert_eq!(
            moved_words("1080p WEB", "1080p WEB", "did not start here"),
            "Playing another 1080p WEB: the first one did not start here"
        );
    }

    fn stream(json: &str) -> ParsedStream {
        let body = format!(r#"{{"streams":[{json}]}}"#);
        let wire = mq_protocol::decode_streams(body.as_bytes()).unwrap();
        mq_streams::normalize(&wire.streams[0], "a", "A", 0, 0)
    }

    #[test]
    fn debrid_requests_follow_the_stream() {
        let resolve = stream(
            r#"{"clientResolve":{"type":"debrid","service":"torbox","infoHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","fileIdx":3,"filename":"Show.S01E02.mkv","episode":2}}"#,
        );
        let (service, req) = debrid_request(&resolve, Some(1), Some(9)).unwrap();
        assert_eq!(service.as_deref(), Some("torbox"));
        assert_eq!(req.file_idx, Some(3));
        assert_eq!(req.filename_hint.as_deref(), Some("Show.S01E02.mkv"));
        // The addon's own episode wins; the season falls back to the target.
        assert_eq!((req.season, req.episode), (Some(1), Some(2)));

        let torrent = stream(
            r#"{"infoHash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","fileIdx":1,"behaviorHints":{"service":"realdebrid"}}"#,
        );
        let (service, req) = debrid_request(&torrent, Some(4), Some(5)).unwrap();
        assert_eq!(service.as_deref(), Some("realdebrid"));
        assert_eq!(req.info_hash, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!((req.season, req.episode), (Some(4), Some(5)));

        assert!(
            debrid_request(&stream(r#"{"url":"https://x.example/a.mkv"}"#), None, None).is_none()
        );
        let no_hash = stream(r#"{"clientResolve":{"type":"debrid","service":"torbox"}}"#);
        assert!(debrid_request(&no_hash, None, None).is_none());
    }

    #[tokio::test]
    async fn a_stale_resolve_never_replaces_a_newer_one() {
        let slot = Mutex::new(None);
        let task = || tokio::spawn(std::future::pending::<()>());
        let (older, newer, newest) = (task(), task(), task());
        let ends = [&older, &newer, &newest].map(|t| t.abort_handle());
        // Caller B installs run 2 before caller A, paused after spawning,
        // installs run 1.
        install(&slot, 2, newer);
        install(&slot, 1, older);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            ends.each_ref().map(|t| t.is_finished()),
            [true, false, false]
        );
        // A newer run still replaces it.
        install(&slot, 3, newest);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            ends.each_ref().map(|t| t.is_finished()),
            [true, true, false]
        );
    }

    #[tokio::test]
    async fn a_late_lease_is_never_adopted() {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let server = mq_server::StreamServer::start(http).await.unwrap();
        let lease = || {
            let upstream = Upstream::Http {
                url: "http://127.0.0.1:9/v.mkv".into(),
                headers: Vec::new(),
                addon_url: None,
            };
            server.register(upstream, "v.mkv")
        };
        let mut s = State::new(Vec::new(), None);
        // A failover moved the view on while run 1 was opening.
        s.run = 2;
        assert!(!adopt(&mut s, 1, lease()), "a run that moved on");
        assert_eq!(server.playbacks(), 0);
        assert!(adopt(&mut s, 2, lease()));
        assert_eq!(server.playbacks(), 1);
        // The view closed while run 2's task still ran.
        s.closed = true;
        release_all(&mut s);
        assert!(!adopt(&mut s, 2, lease()), "a closed view");
        assert_eq!(server.playbacks(), 0);
        assert_eq!(s.snapshot.url, None);
    }

    /// A resolve that runs on after its view closed (one the abort did not
    /// reach in time) takes no other candidate.
    #[tokio::test]
    async fn a_closed_view_opens_no_source() {
        let core = Core::temporary("default").unwrap();
        let mut s = State::new(vec![stream(r#"{"url":"http://127.0.0.1:9/v.mkv"}"#)], None);
        s.closed = true;
        let snapshot = SharedObservable::new(s.snapshot.clone());
        let state = Arc::new(Mutex::new(s));
        let target = Arc::new(PlayTarget::default());
        resolve_next(core, target, Arc::from([]), state.clone(), snapshot, 0).await;
        let s = lock(&state);
        assert_eq!(s.next, 0, "no candidate taken");
        assert_eq!(s.snapshot.phase, PlaybackPhase::Resolving);
    }

    /// Code review r1 P1: the stall rule reads received payload, so a
    /// swarm whose 16 MiB pieces verify late (the first at 20 s) while
    /// blocks arrive every second is never cut.
    #[tokio::test(start_paused = true)]
    async fn received_payload_not_verified_pieces_keeps_a_swarm_alive() {
        // `at` gives received bytes: 128 KiB a second from the unchoke.
        let (started, _) = watch(Some(Duration::from_secs(25)), |t| (t * 128 * KIB, 3), true).await;
        assert!(matches!(started, Started::Ready(())));
    }

    /// Code review r1 P2-5, P3: a metadata timeout is remembered once the
    /// chain proves the connection works, and an outage marks nothing.
    #[test]
    fn dead_swarms_wait_for_proof_the_connection_works() {
        let mut dead = Dead::default();
        assert!(dead.failed("AAAA", false).is_empty(), "no proof yet");
        assert!(dead.failed("bbbb", false).is_empty());
        assert_eq!(dead.reached(), ["aaaa", "bbbb"], "a source opened");
        assert_eq!(dead.failed("cccc", false), ["cccc"], "proof already in");
        let mut outage = Dead::default();
        assert!(outage.failed("dddd", false).is_empty());
        assert!(
            outage.pending.len() == 1 && !outage.reached,
            "dropped with the view"
        );
        let mut own = Dead::default();
        assert_eq!(
            own.failed("eeee", true),
            ["eeee"],
            "its own unchoke proves it"
        );
    }

    /// A torrent file's rate before the player's duration: its resolution
    /// and quality, or its size over 3 h (a film) or 1 h (an episode),
    /// whichever is larger (plans/streaming-window.md 2.2).
    #[test]
    fn a_torrent_rate_is_guessed_from_its_release_and_size() {
        let named = |title: &str| {
            stream(&format!(
                r#"{{"name":"x","title":"{title}","infoHash":"{}"}}"#,
                "a".repeat(40)
            ))
        };
        let gib = 1u64 << 30;
        let web = named("Film.2020.1080p.WEB-DL.x264-G");
        assert_eq!(guessed_rate(&web, gib, false), 1_250_000);
        let remux = named("Film.2020.2160p.UHD.BluRay.REMUX.HEVC-G");
        assert_eq!(guessed_rate(&remux, gib, false), 7_500_000);
        // 40 GiB over 3 h is 32 Mbit/s, over a 1080p's 10.
        assert_eq!(guessed_rate(&web, 40 * gib, false), 40 * gib / 10_800);
        // An episode: its size over 1 h.
        assert_eq!(guessed_rate(&web, 8 * gib, true), 8 * gib / 3600);
        assert_eq!(guessed_rate(&named("Film.2020.720p"), gib, false), 625_000);
    }

    /// The window's rate (review P2-2): the file over a known duration (the
    /// last session's, else the meta runtime), as the ranking sizes the row;
    /// the guess only when none is known; never under the size over 3 h.
    #[test]
    fn a_window_rate_comes_from_the_duration_before_the_guess() {
        let web = stream(&format!(
            r#"{{"name":"x","title":"Film.2020.1080p.WEB-DL.x264-G","infoHash":"{}"}}"#,
            "a".repeat(40)
        ));
        let len = 2_500_000_000;
        assert_eq!(window_rate(&web, len, false, Some(7200)), len / 7200);
        assert_eq!(window_rate(&web, len, false, None), 1_250_000);
        assert_eq!(window_rate(&web, len, false, Some(0)), 1_250_000);
        assert_eq!(window_rate(&web, len, false, Some(100_000)), len / 10_800);
    }

    /// Code review r1 P2-1: one capacity sample per torrent and network.
    #[test]
    fn a_torrent_is_sampled_once_per_network() {
        use crate::speed::Network;
        let hash = "f".repeat(40);
        assert!(first_sample(&hash, Network::Wifi));
        assert!(!first_sample(&hash.to_uppercase(), Network::Wifi));
        assert!(first_sample(&hash, Network::Cellular));
    }

    /// Code review r1 P2-2: a torrent our check found uncached plays
    /// through the engine, never the service first.
    #[test]
    fn an_uncached_swarm_row_goes_to_the_engine() {
        let mut torrent = stream(&format!(r#"{{"infoHash":"{}"}}"#, "a".repeat(40)));
        assert!(by_engine(&torrent, false, true), "no account");
        assert!(
            !by_engine(&torrent, true, true),
            "unknown: the service first"
        );
        torrent.cached = Some(false);
        assert!(by_engine(&torrent, true, true));
        assert!(!by_engine(&torrent, true, false), "no engine: the service");
        torrent.cached = Some(true);
        assert!(!by_engine(&torrent, true, true));
    }

    /// Code review 2026-10-20 r3 P3-11: a "Try again" run has one budget
    /// for all its sources (3 minutes): every wait is cut to what is left,
    /// and once it is spent the card comes back, however many sources are
    /// left.
    #[tokio::test]
    async fn a_retry_gives_up_after_its_budget() {
        let core = Core::temporary("default").unwrap();
        // Loopback links from no install are refused at once.
        let dead = |i: usize| stream(&format!(r#"{{"url":"http://127.0.0.1:9/v{i}.mkv"}}"#));
        let view = PlaybackView::open(&core, (0..5).map(dead).collect(), PlayTarget::default());
        let failed = || async {
            for _ in 0..3000 {
                if view.snapshot().phase == PlaybackPhase::Failed {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("never failed: {:?}", view.snapshot());
        };
        failed().await;
        assert_eq!(view.snapshot().attempts.len(), 5);
        view.retry();
        {
            let s = lock(&view.state);
            let left = budget_left(&s).unwrap();
            assert!(left > RETRY_BUDGET - Duration::from_secs(5), "{left:?}");
        }
        let soon = tokio::time::Instant::now() + Duration::from_secs(2);
        lock(&view.state).retry_until = Some(soon);
        assert!(within_budget(&view.state, PROBE_TIMEOUT) <= Duration::from_secs(2));
        // The budget is spent before the resolve reaches a source.
        lock(&view.state).retry_until = Some(tokio::time::Instant::now());
        failed().await;
        let s = view.snapshot();
        let errors: Vec<&str> = s.attempts.iter().map(|a| a.error.as_str()).collect();
        assert_eq!(errors, [RETRY_GAVE_UP]);
    }

    /// "Try again" restarts the same versions from the first, patiently
    /// (no early cut), and only once the playback failed.
    #[tokio::test]
    async fn try_again_restarts_the_same_versions_patiently() {
        let core = Core::temporary("default").unwrap();
        // A loopback link from no install is refused at once.
        let dead = stream(r#"{"url":"http://127.0.0.1:9/v.mkv"}"#);
        let view = PlaybackView::open(&core, vec![dead], PlayTarget::default());
        view.retry();
        assert!(!lock(&view.state).patient, "not failed yet: ignored");
        let failed = || async {
            for _ in 0..3000 {
                if view.snapshot().phase == PlaybackPhase::Failed {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("never failed: {:?}", view.snapshot());
        };
        failed().await;
        assert_eq!(view.snapshot().attempts.len(), 1);
        view.retry();
        {
            let s = lock(&view.state);
            assert!(s.patient);
            assert_eq!(s.snapshot.phase, PlaybackPhase::Resolving);
            assert!(s.snapshot.attempts.is_empty());
        }
        failed().await;
        assert_eq!(
            view.snapshot().attempts.len(),
            1,
            "the same version tried again"
        );
    }

    /// Plans/play-every-version.md 2c: every refusal for room the engine
    /// gives reads as the one storage line, from the error's type; other
    /// errors, and anyone else's words about storage, stay as they are.
    #[cfg(feature = "torrent")]
    #[test]
    fn a_refusal_for_room_reads_as_the_one_storage_line() {
        use mq_engine::EngineError;
        const LINE: &str = "This device needs a little free space to play this";
        let refusals = [
            EngineError::DeviceFull,
            EngineError::Storage("storage full".into()),
            EngineError::Storage("this storage cannot free space while playing".into()),
        ];
        for e in refusals {
            assert_eq!(engine_words(&e), LINE, "{e}");
        }
        let other = EngineError::Io(std::io::Error::other("no peer sent"));
        assert_eq!(engine_words(&other), other.to_string());
        // A service's words about its own storage stay its (review P3-1).
        let link = stream(r#"{"url":"https://cdn.example/v.mkv"}"#);
        let said = attempt(&link, "Your storage is full, the torrent was not added");
        assert_eq!(
            said.error,
            "Your storage is full, the torrent was not added"
        );
    }

    /// Review 2026-10-23 P3-2: refusals for room mixed with swarms that did
    /// not start read notStarted; with a source that could not be reached,
    /// unreachable.
    #[test]
    fn room_and_not_started_read_not_started() {
        let link = stream(r#"{"url":"https://cdn.example/v.mkv"}"#);
        let fail = |s: &mut State, error: &str, why: Option<&'static str>| {
            s.snapshot.attempts.push(attempt(&link, error));
            s.version_failed = why;
            note_failure(s);
        };
        let mut s = State::new(Vec::new(), None);
        fail(&mut s, no_room_line(), None);
        fail(&mut s, "the swarm sent almost nothing", Some(NOT_STARTED));
        assert_eq!(s.failed_as, Some(EmptyKind::NotStarted));
        fail(&mut s, no_room_line(), None);
        assert_eq!(s.failed_as, Some(EmptyKind::NotStarted));
        fail(&mut s, "The source could not be reached", None);
        assert_eq!(s.failed_as, Some(EmptyKind::Unreachable));
    }

    /// The engine's verified bytes and connected peers, one per second
    /// (`at(t)` gives second `t`'s).
    fn fake(
        at: impl Fn(u64) -> (u64, usize),
    ) -> impl FnMut() -> std::future::Ready<Option<Progress>> {
        let mut t = 0;
        move || {
            let (bytes, peers) = at(t);
            t += 1;
            std::future::ready(Some(Progress {
                received: bytes,
                unchoked: peers > 0,
            }))
        }
    }

    const KIB: u64 = 1 << 10;
    const DEADLINE: Duration = Duration::from_secs(60);

    /// Attempt `tries` of a start begun `start_s` seconds ago, added now,
    /// watched with the swarm `at` and Ready after `ready_after`: how it
    /// ended, and when.
    async fn attempt_at(
        start_s: u64,
        tries: u32,
        more: bool,
        ready_after: Option<Duration>,
        at: impl Fn(u64) -> (u64, usize),
    ) -> (Started<()>, Duration) {
        let added = tokio::time::Instant::now();
        let start = added - Duration::from_secs(start_s);
        let policy = attempting(start, tries, added, more);
        let ready = async move {
            match ready_after {
                Some(d) => tokio::time::sleep(d).await,
                None => std::future::pending::<()>().await,
            }
        };
        let started = watch_policy(ready, fake(at), policy).await;
        (started, added.elapsed())
    }

    const fn secs(s: u64) -> Option<Duration> {
        Some(Duration::from_secs(s))
    }

    /// Plans/play-every-version.md 2e, paused clock: a first attempt with
    /// no unchoke still waits at 19 s and ends at 20 s; a later one still
    /// waits at 11 s and ends at 12 s.
    #[tokio::test(start_paused = true)]
    async fn a_dead_swarm_ends_at_20s_first_then_12s() {
        let dead = |_| (0, 0);
        let (started, _) = attempt_at(0, 1, true, secs(19), dead).await;
        assert!(matches!(started, Started::Ready(())), "19 s: still waiting");
        let (started, took) = attempt_at(0, 1, true, secs(21), dead).await;
        assert!(matches!(started, Started::Late { unchoked: false }));
        assert_eq!(took.as_secs(), 20);
        let (started, _) = attempt_at(20, 2, true, secs(11), dead).await;
        assert!(matches!(started, Started::Ready(())), "11 s: still waiting");
        let (started, took) = attempt_at(20, 2, true, secs(13), dead).await;
        assert!(matches!(started, Started::Late { unchoked: false }));
        assert_eq!(took.as_secs(), 12);
    }

    /// 2e: a swarm that unchoked runs to 30 s, then on only while what it
    /// received rose in the last 5 s. A healthy but slow first swarm, Ready
    /// at 25 s, plays.
    #[tokio::test(start_paused = true)]
    async fn a_live_swarm_runs_to_30s_then_while_its_window_grows() {
        let slow = |t| (t * 100 * KIB, 3);
        let (started, _) = attempt_at(0, 1, true, secs(25), slow).await;
        assert!(matches!(started, Started::Ready(())));
        // Growing all along: on past 30 s, Ready at 45 s.
        let (started, _) = attempt_at(0, 2, true, secs(45), slow).await;
        assert!(matches!(started, Started::Ready(())));
        // Flat from 20 s: cut at 30 s.
        let (started, took) = attempt_at(0, 2, true, None, |t| (t.min(20) * KIB, 3)).await;
        assert!(matches!(started, Started::Stalled { nothing: false }));
        assert_eq!(took.as_secs(), 30);
        // Flat from 28 s: it grew within 5 s at 30 s, not at 33 s.
        let (started, took) = attempt_at(0, 2, true, None, |t| (t.min(28) * KIB, 3)).await;
        assert!(matches!(started, Started::Stalled { nothing: false }));
        assert_eq!(took.as_secs(), 33);
        // Unchoked, nothing ever sent.
        let (started, took) = attempt_at(0, 2, true, None, |_| (0, 3)).await;
        assert!(matches!(started, Started::Stalled { nothing: true }));
        assert_eq!(took.as_secs(), 30);
    }

    /// 2e: the last attempt that fits (under 30 s of the budget left, or
    /// nothing after it) runs to the budget's end, and nothing runs past it.
    #[tokio::test(start_paused = true)]
    async fn the_last_attempt_runs_to_the_budgets_end() {
        let (started, took) = attempt_at(68, 6, true, None, |_| (0, 0)).await;
        assert!(matches!(started, Started::Late { unchoked: false }));
        assert_eq!(took.as_secs(), 22);
        let (started, took) = attempt_at(0, 1, false, None, |_| (0, 0)).await;
        assert!(matches!(started, Started::Late { unchoked: false }));
        assert_eq!(took.as_secs(), 90);
        let (started, took) = attempt_at(50, 3, true, None, |t| (t * KIB, 3)).await;
        assert!(matches!(started, Started::Late { unchoked: true }));
        assert_eq!(took.as_secs(), 40, "growing, cut at the budget's end");
    }

    /// 2e: the loading screen offers "Choose another version" 20 s into a
    /// start, and not before.
    #[tokio::test(start_paused = true)]
    async fn choose_another_shows_20s_into_a_loading_screen() {
        let mut s = State::new(Vec::new(), None);
        begin_start(&mut s, StartStage::Details);
        assert!(!s.snapshot.choose_another);
        tokio::time::advance(Duration::from_secs(19)).await;
        show_start(&mut s, Sample::default());
        assert!(!s.snapshot.choose_another, "19 s");
        tokio::time::advance(Duration::from_secs(1)).await;
        show_start(&mut s, Sample::default());
        assert!(s.snapshot.choose_another, "20 s");
        // The next source of the same start keeps it.
        begin_start(&mut s, StartStage::Connecting);
        assert!(s.snapshot.choose_another);
    }

    /// 2e: a source that played past its start ends the start: a later
    /// move (a decode failure, a stall) begins a budget of its own.
    #[tokio::test]
    async fn playing_past_the_start_ends_its_budget() {
        let core = Core::temporary("default").unwrap();
        let view = PlaybackView::open(&core, Vec::new(), PlayTarget::default());
        {
            let mut s = lock(&view.state);
            s.start_at = Some(tokio::time::Instant::now());
            s.tries = 3;
            s.snapshot.start_budget_ends_at = Some(1);
        }
        for at in (0..=30_000).step_by(10_000) {
            view.report_progress(at, 600_000, false);
        }
        let s = lock(&view.state);
        assert!(s.start_at.is_none() && s.snapshot.start_budget_ends_at.is_none());
        // The next move's first attempt is a first one: 20 s, not 12 s
        // (review 2026-10-23 P2-1).
        assert_eq!(s.tries, 0);
        let now = tokio::time::Instant::now();
        assert_eq!(attempting(now, s.tries + 1, now, true).dead, DEAD_FIRST);
    }

    /// 2e: every swarm dead, the attempts begin at 0, 20, 32, 44, 56 and 68
    /// s, six tries, and the last ends at 90 s.
    #[tokio::test(start_paused = true)]
    async fn every_swarm_dead_gives_six_tries_in_90s() {
        let start = tokio::time::Instant::now();
        let mut begun = Vec::new();
        for tries in 1.. {
            let added = tokio::time::Instant::now();
            if added >= start + START_BUDGET {
                break;
            }
            begun.push((added - start).as_secs());
            let policy = attempting(start, tries, added, true);
            let never = std::future::pending::<()>();
            watch_policy(never, fake(|_| (0, 0)), policy).await;
        }
        assert_eq!(begun, [0, 20, 32, 44, 56, 68]);
        assert_eq!((tokio::time::Instant::now() - start).as_secs(), 90);
    }

    async fn watch(
        ready_after: Option<Duration>,
        at: impl Fn(u64) -> (u64, usize),
        others: bool,
    ) -> (Started<()>, Duration) {
        let rule = Rule {
            windows: others,
            elsewhere: others,
            deadline: DEADLINE,
            cap: 3 * DEADLINE,
        };
        watch_rule(ready_after, at, rule).await
    }

    async fn watch_rule(
        ready_after: Option<Duration>,
        at: impl Fn(u64) -> (u64, usize),
        rule: Rule,
    ) -> (Started<()>, Duration) {
        let begun = tokio::time::Instant::now();
        let ready = async move {
            match ready_after {
                Some(d) => tokio::time::sleep(d).await,
                None => std::future::pending::<()>().await,
            }
        };
        let started = watch_start(ready, fake(at), STALL, rule).await;
        (started, begun.elapsed())
    }

    /// Code review 2026-10-20 r3 P3-3, P3-4: a file larger than the version
    /// listed is kept for the episode it played for, and only that one.
    #[test]
    fn a_wrong_file_is_kept_for_its_episode() {
        let core = Core::temporary("default").unwrap();
        let mut pack = stream(&format!(r#"{{"infoHash":"{}"}}"#, "c".repeat(40)));
        pack.size_bytes = Some(1 << 30);
        let fifth = PlayTarget {
            season: Some(1),
            episode: Some(5),
            ..PlayTarget::default()
        };
        let path = "Show/Show.S01E05.1080p.WEB.mkv";
        assert!(judge_file(&core, &pack, &fifth, path, 3 << 30, false).is_some());
        let kept = crate::speed::wrong_file(core.library(), mq_library::hlc::now_ms());
        let digest = |e| mq_streams::target_digest(&pack.key, Some(1), Some(e));
        assert_eq!(kept, [digest(5)]);
        assert!(!kept.contains(&digest(6)), "another episode of the pack");
        assert!(judge_file(&core, &pack, &fifth, path, 1 << 30, false).is_none());
    }

    /// Code review 2026-10-20 r4 P3-7: a film's key without a file index
    /// can stand for every film of a collection, so its wrong file is not
    /// kept; with a file index, or for an episode, it is.
    #[test]
    fn a_wrong_file_is_kept_only_where_it_names_one_file() {
        let core = Core::temporary("default").unwrap();
        let film = PlayTarget::default();
        let mut collection = stream(&format!(r#"{{"infoHash":"{}"}}"#, "c".repeat(40)));
        collection.size_bytes = Some(1 << 30);
        let path = "Trilogy/Film.3.1080p.mkv";
        assert!(judge_file(&core, &collection, &film, path, 3 << 30, false).is_some());
        let kept = || crate::speed::wrong_file(core.library(), mq_library::hlc::now_ms());
        assert!(kept().is_empty(), "{}: no file index", collection.key);
        let mut one = stream(&format!(
            r#"{{"infoHash":"{}","fileIdx":2}}"#,
            "c".repeat(40)
        ));
        one.size_bytes = Some(1 << 30);
        assert!(judge_file(&core, &one, &film, path, 3 << 30, false).is_some());
        assert_eq!(kept().len(), 1, "{}: a file index", one.key);
    }

    /// Code review 2026-10-20 r4 P3-3: when a "Try again" run of a hand pick
    /// runs out of budget, the card keeps offering the next version.
    #[tokio::test]
    async fn a_hand_pick_keeps_its_offer_when_the_budget_runs_out() {
        let core = Core::temporary("default").unwrap();
        let versions = crate::views::versions::VersionsView::open(
            &core,
            crate::views::versions::film_params(),
        );
        // Loopback links from no install are refused at once.
        let link = |name: &str, n: u32| {
            stream(&format!(
                r#"{{"title":"{name}","url":"http://127.0.0.1:9/{n}.mkv"}}"#
            ))
        };
        let rows = crate::views::versions::rank_for_tests(vec![
            link("Movie.2020.1080p.WEB-DL.x264-HAND", 1),
            link("Movie.2020.1080p.BluRay.x264-NEXT", 2),
        ]);
        assert!(rows.len() == 2 && rows.iter().all(|r| r.key.viable));
        let (hand, next) = (rows[0].stream.key.clone(), rows[1].stream.key.clone());
        while !versions.snapshot().done {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        versions.rows_for_tests(rows);
        let view =
            PlaybackView::open_picked(&core, versions.pick(&hand), PlayTarget::default(), &[]);
        let failed = || async {
            for _ in 0..3000 {
                if view.snapshot().phase == PlaybackPhase::Failed {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("never failed: {:?}", view.snapshot());
        };
        failed().await;
        view.retry();
        {
            // As when the hand pick's source failed for a reason of its own
            // (no picture) just as the 3 minutes ran out.
            let mut s = lock(&view.state);
            s.version_failed = Some(NOT_HERE);
            s.retry_until = Some(tokio::time::Instant::now());
        }
        failed().await;
        let s = view.snapshot();
        assert_eq!(s.attempts.last().unwrap().error, RETRY_GAVE_UP);
        assert_eq!(s.offer.map(|o| o.key), Some(next));
    }

    /// A swarm that did not start reads "did not start here", unless the
    /// version already did not play here: that reason stays, and the first
    /// move keeps it as why the announced version was left.
    #[cfg(feature = "torrent")]
    #[tokio::test]
    async fn a_reason_it_did_not_play_here_outlasts_a_swarm_failure() {
        let core = Core::temporary("default").unwrap();
        let versions = crate::views::versions::VersionsView::open(
            &core,
            crate::views::versions::film_params(),
        );
        let link = |name: &str, n: u32| {
            stream(&format!(
                r#"{{"title":"{name}","url":"http://127.0.0.1:9/{n}.mkv"}}"#
            ))
        };
        let rows = crate::views::versions::rank_for_tests(vec![
            link("Movie.2020.1080p.WEB-DL.x264-FIRST", 1),
            link("Movie.2020.720p.WEB-DL.x264-NEXT", 2),
        ]);
        let first = rows[0].stream.key.clone();
        while !versions.snapshot().done {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        versions.rows_for_tests(rows);
        versions.recommend_first_for_tests();

        let mut s = State::new(Vec::new(), None);
        failed_to_start(&mut s);
        assert_eq!(s.version_failed, Some(NOT_STARTED));
        s.version_failed = Some(NOT_HERE);
        failed_to_start(&mut s);
        assert_eq!(s.version_failed, Some(NOT_HERE), "kept over the swarm");
        s.pick = Some(versions.pick(&first).handle);
        s.tried = vec![first];
        assert!(move_on(&mut s));
        assert_eq!(s.left_because, Some(NOT_HERE));
        assert_eq!((s.version_failed, s.hops), (None, 1));
    }

    /// A run that ended on the budget never reached `move_on`, so the
    /// version's failure and its words are still held: Try again drops them
    /// with the rest, so a later failure that is not the version's never
    /// moves on in their name.
    #[tokio::test]
    async fn try_again_drops_a_version_failure_left_held() {
        let core = Core::temporary("default").unwrap();
        let view = PlaybackView::open(&core, Vec::new(), PlayTarget::default());
        while view.snapshot().phase != PlaybackPhase::Failed {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        lock(&view.state).version_failed = Some(NOT_HERE);
        view.retry();
        assert_eq!(lock(&view.state).version_failed, None);
    }

    /// Code review 2026-10-20 r4 P3-8: in a "Try again" run with 20 s left,
    /// the metadata wait, the deadline and the cap are cut to 20 s.
    #[cfg(feature = "torrent")]
    #[tokio::test(start_paused = true)]
    async fn a_retry_cuts_each_limit_to_what_is_left() {
        let core = Core::temporary("default").unwrap();
        let view = PlaybackView::open(&core, Vec::new(), PlayTarget::default());
        let left = Duration::from_secs(20);
        lock(&view.state).retry_until = Some(tokio::time::Instant::now() + left);
        let rule = start_rule(&lock(&view.state));
        assert_eq!((rule.deadline, rule.cap), (left, left));
        assert_eq!(within_budget(&view.state, METADATA_TIMEOUT), left);
    }

    /// Plans/play-every-version.md 2e: a first play whose only swarm never
    /// answers runs to the budget's end and fails then, never later, with
    /// "This didn't start".
    #[cfg(feature = "torrent")]
    #[tokio::test(start_paused = true)]
    async fn a_first_play_fails_at_90s_as_not_started() {
        let core = Core::temporary("default").unwrap();
        // An engine of its own: two tests starting the shared one at once
        // made this one flaky (review 2026-10-23 P2-3).
        let (engine, _dir) = own_engine().await;
        core.set_torrent_engine(engine);
        let silent = silent_tracker();
        let swarm = dead_torrent(&silent);
        let begun = tokio::time::Instant::now();
        let view = PlaybackView::open(&core, vec![swarm], PlayTarget::default());
        while view.snapshot().phase != PlaybackPhase::Failed && begun.elapsed().as_secs() < 200 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let s = view.snapshot();
        assert_eq!(s.phase, PlaybackPhase::Failed, "{s:?}");
        assert!(
            (90..92).contains(&begun.elapsed().as_secs()),
            "{:?} {:?}",
            begun.elapsed(),
            s.attempts
        );
        assert_eq!(s.empty, Some(EmptyKind::NotStarted), "{:?}", s.attempts);
        assert!(!s.choose_another && s.start_budget_ends_at.is_none());
    }

    /// Review 2026-10-23 P3-1, P3-2: two sources of one dead swarm run out
    /// before the budget (the second is skipped at once): the card reads
    /// notStarted, not unreachable, and a swarm let go before the full 30 s
    /// details wait is not remembered as dead. The budget's end shows while
    /// it runs.
    #[cfg(feature = "torrent")]
    #[tokio::test(start_paused = true)]
    async fn dead_swarms_that_run_out_before_90s_read_not_started() {
        let core = Core::temporary("default").unwrap();
        let (engine, _dir) = own_engine().await;
        core.set_torrent_engine(engine);
        let silent = silent_tracker();
        let (dead, mut again) = (dead_torrent(&silent), dead_torrent(&silent));
        again.addon_id = "another".into();
        let begun = tokio::time::Instant::now();
        let view = PlaybackView::open(&core, vec![dead, again], PlayTarget::default());
        // A peer was reached before in the chain.
        lock(&view.state).dead.reached = true;
        let mut ends = None;
        while view.snapshot().phase != PlaybackPhase::Failed && begun.elapsed().as_secs() < 200 {
            ends = ends.or(view.snapshot().start_budget_ends_at);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let s = view.snapshot();
        assert!(
            (20..22).contains(&begun.elapsed().as_secs()),
            "{:?} {:?}",
            begun.elapsed(),
            s.attempts
        );
        assert_eq!(s.empty, Some(EmptyKind::NotStarted), "{:?}", s.attempts);
        let ends = ends.expect("the budget's end while it ran");
        let now = mq_library::hlc::now_ms();
        assert!(ends > now && ends <= now + 90_000, "{ends} {now}");
        let dead = crate::speed::dead_swarms(core.library(), now);
        assert!(dead.is_empty(), "{dead:?}");
    }

    /// Review 2026-10-23 P2-1: "Try again" begins with a first attempt.
    #[tokio::test]
    async fn try_again_begins_with_a_first_attempt() {
        let core = Core::temporary("default").unwrap();
        let view = PlaybackView::open(&core, Vec::new(), PlayTarget::default());
        for _ in 0..100 {
            if view.snapshot().phase == PlaybackPhase::Failed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        lock(&view.state).tries = 3;
        view.retry();
        assert_eq!(lock(&view.state).tries, 0);
    }

    /// The budget's end on a streaming service's resolve, in any build: a
    /// service that takes the request and never answers ends the playback
    /// as notStarted when the budget runs out, not when its own request
    /// gives up. The budget starts 87 s in, so the test takes seconds.
    #[tokio::test]
    async fn a_silent_service_ends_with_the_budget() {
        let core = Core::temporary("default").unwrap();
        // Takes every request and never answers.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        core.set_debrid_origin(&base);
        core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
        let torrent = stream(&format!(
            r#"{{"infoHash":"{}","fileIdx":0}}"#,
            "e".repeat(40)
        ));
        let view = PlaybackView::open(&core, vec![torrent], PlayTarget::default());
        // Before the resolve runs (this test's one thread): 3 s left.
        let moved = tokio::time::Instant::now();
        lock(&view.state).start_at = Some(moved - START_BUDGET + Duration::from_secs(3));
        while view.snapshot().phase != PlaybackPhase::Failed && moved.elapsed().as_secs() < 60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let s = view.snapshot();
        let ended = moved.elapsed();
        assert_eq!(s.phase, PlaybackPhase::Failed, "{s:?}");
        assert!(
            (Duration::from_millis(2500)..Duration::from_millis(8000)).contains(&ended),
            "{ended:?} {:?}",
            s.attempts
        );
        assert_eq!(s.empty, Some(EmptyKind::NotStarted), "{:?}", s.attempts);
    }

    /// Review 2026-10-23 P2-2, in any build: a link opening slowly gets
    /// "Choose another version" 20 s into the loading screen, and fails as
    /// notStarted when the budget ends, by the one clock every kind of
    /// source shares (the clocks start 19 s and 87 s in, so the test takes
    /// seconds). A link's probe is cut to the budget too, so here the
    /// button is what only the clock gives.
    #[tokio::test]
    async fn a_slow_link_gets_the_button_and_ends_with_the_budget() {
        let core = Core::temporary("default").unwrap();
        // Accepts and never answers: the probe waits its 8 s.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let mut link = stream(&format!(r#"{{"url":"{base}/v.mp4"}}"#));
        link.addon_url = Some(url::Url::parse(&format!("{base}/manifest.json")).unwrap());
        let view = PlaybackView::open(&core, vec![link], PlayTarget::default());
        // Before the resolve runs (this test's one thread): the loading
        // screen up 19 s, 3 s of the budget left.
        let moved = tokio::time::Instant::now();
        {
            let mut s = lock(&view.state);
            s.snapshot.start = Some(StartProgress {
                stage: StartStage::Connecting,
                peers: 0,
                have_bytes: 0,
                need_bytes: 0,
                bytes_per_s: 0,
                source: 0,
                sources: 1,
                progress: 0.0,
                caption: None,
            });
            s.loading_since = moved - Duration::from_secs(19);
            s.start_at = Some(moved - START_BUDGET + Duration::from_secs(3));
        }
        let mut button = None;
        while view.snapshot().phase != PlaybackPhase::Failed && moved.elapsed().as_secs() < 15 {
            if view.snapshot().choose_another && button.is_none() {
                button = Some(moved.elapsed());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let s = view.snapshot();
        let button = button.unwrap_or_else(|| panic!("no button: {s:?}"));
        assert!(button < Duration::from_millis(1500), "{button:?}");
        let ended = moved.elapsed();
        assert!(
            ended < Duration::from_millis(4500),
            "{ended:?} {:?}",
            s.attempts
        );
        assert_eq!(s.phase, PlaybackPhase::Failed);
        assert_eq!(s.empty, Some(EmptyKind::NotStarted), "{:?}", s.attempts);
    }

    /// Code review 2026-10-20 r4 P3-1: a metadata wait a "Try again" budget
    /// cut short is not taken for a dead swarm, though a peer was reached
    /// before in the chain.
    #[cfg(feature = "torrent")]
    #[tokio::test(start_paused = true)]
    async fn a_metadata_wait_the_budget_cut_short_is_not_a_dead_swarm() {
        let core = Core::temporary("default").unwrap();
        let hash = "e".repeat(40);
        let swarm = stream(&format!(r#"{{"infoHash":"{hash}","fileIdx":0}}"#));
        let view = PlaybackView::open(&core, vec![swarm], PlayTarget::default());
        {
            let mut s = lock(&view.state);
            // A "Try again" run, as `retry` starts it.
            s.retry_until = Some(tokio::time::Instant::now() + Duration::from_secs(5));
            s.patient = true;
            s.dead.reached = true;
        }
        for _ in 0..600 {
            if view.snapshot().phase == PlaybackPhase::Failed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let s = view.snapshot();
        assert_eq!(s.phase, PlaybackPhase::Failed, "{s:?}");
        let dead = crate::speed::dead_swarms(core.library(), mq_library::hlc::now_ms());
        assert!(dead.is_empty(), "{dead:?}");
    }

    /// A swarm with peers that sends nothing is cut once its first window
    /// (30 s from the first peer) ends, when another candidate can play,
    /// and is remembered: nothing came.
    #[tokio::test(start_paused = true)]
    async fn a_dead_swarm_is_cut_after_its_first_window() {
        let (started, took) = watch(None, |_| (0, 3), true).await;
        assert!(matches!(started, Started::Stalled { nothing: true }));
        assert!((30..33).contains(&took.as_secs()), "{took:?}");
    }

    /// A healthy swarm that is slow to start (the Android bare-hash runs
    /// spent most of their first 19 s finding peers): no peer for 15 s, the
    /// first bytes 18 s after it, Ready at 42 s. Never cut: its first window
    /// starts at its first peer and lasts 30 s.
    #[tokio::test(start_paused = true)]
    async fn a_slow_starting_healthy_swarm_is_never_cut() {
        let at = |t: u64| {
            let peers = if t < 15 { 0 } else { 4 };
            (t.saturating_sub(33) * 300 * KIB, peers)
        };
        let (started, took) = watch(Some(Duration::from_secs(42)), at, true).await;
        assert!(matches!(started, Started::Ready(())), "{took:?}");
    }

    /// With no other candidate a dead swarm gets the whole deadline; so does
    /// one where no peer ever connects.
    #[tokio::test(start_paused = true)]
    async fn the_deadline_holds_with_no_other_candidate_or_no_peer() {
        let (started, took) = watch(None, |_| (0, 3), false).await;
        assert!(matches!(started, Started::Late { .. }));
        assert!(took >= DEADLINE, "{took:?}");
        let (started, took) = watch(None, |_| (0, 0), true).await;
        assert!(matches!(started, Started::Late { .. }));
        assert!(took >= DEADLINE, "{took:?}");
    }

    /// After its first 256 KiB a swarm gets 15 s for each next 256 KiB: one
    /// that stops is cut, but not remembered, since it sent something.
    #[tokio::test(start_paused = true)]
    async fn a_swarm_that_stops_is_cut_but_not_remembered() {
        let (started, took) = watch(None, |t| ((t.min(6)) * 60 * KIB, 2), true).await;
        assert!(matches!(started, Started::Stalled { nothing: false }));
        assert!((19..23).contains(&took.as_secs()), "{took:?}");
    }

    /// A swarm that keeps sending and is the only thing that can play is
    /// kept past the deadline: a slow head ready at 90 s plays. A ready
    /// head wins at once.
    #[tokio::test(start_paused = true)]
    async fn a_swarm_that_keeps_sending_alone_is_kept_past_the_deadline() {
        let sending = |t| (t * 30 * KIB, 5);
        let (started, took) = watch(Some(Duration::from_secs(90)), sending, false).await;
        assert!(matches!(started, Started::Ready(())), "{took:?}");
        let (started, _) = watch(Some(Duration::from_secs(20)), |t| (t * 300 * KIB, 5), true).await;
        assert!(matches!(started, Started::Ready(())));
    }

    /// Code review 2026-10-20 r2 P2-1: a trickle never holds the start. With
    /// another candidate it is cut at the deadline; alone, at three.
    #[tokio::test(start_paused = true)]
    async fn a_trickle_is_cut_at_the_deadline_or_three() {
        let trickle = |t| (t * 30 * KIB, 5);
        let (started, took) = watch(None, trickle, true).await;
        assert!(matches!(started, Started::Late { unchoked: true }));
        assert!((60..62).contains(&took.as_secs()), "{took:?}");
        let (started, took) = watch(None, trickle, false).await;
        assert!(matches!(started, Started::Late { unchoked: true }));
        assert!((180..182).contains(&took.as_secs()), "{took:?}");
    }

    /// 3.4: the engine's file must be the version listed; only larger
    /// fails (a pack's file is smaller), and another episode fails.
    #[test]
    fn the_engines_file_must_be_the_version_listed() {
        let episode = PlayTarget {
            season: Some(1),
            episode: Some(5),
            ..PlayTarget::default()
        };
        let gb = 1u64 << 30;
        let file = "Show/Show.S01E05.1080p.WEB.mkv";
        assert_eq!(
            wrong_file(file, 2 * gb, Some(2 * gb), &episode, false),
            None
        );
        assert_eq!(
            wrong_file(file, 2 * gb, Some(40 * gb), &episode, false),
            None,
            "a pack's size listed"
        );
        assert!(
            wrong_file(file, 3 * gb, Some(2 * gb), &episode, false).is_some(),
            "larger"
        );
        // Code review r1 P2-3: an absolute episode number with no season
        // is not judged against the per-season episode.
        let anime = PlayTarget {
            season: Some(21),
            episode: Some(1),
            ..PlayTarget::default()
        };
        assert_eq!(
            wrong_file(
                "[SubsPlease] One Piece - 1071 (1080p).mkv",
                gb,
                Some(gb),
                &anime,
                false
            ),
            None
        );
        let other = "Show/Show.S01E06.1080p.WEB.mkv";
        assert!(wrong_file(other, gb, Some(gb), &episode, false).is_some());
        assert_eq!(
            wrong_file(other, gb, Some(gb), &episode, true),
            None,
            "found by the episode"
        );
        assert_eq!(
            wrong_file(
                "Movie.2020.1080p.mkv",
                gb,
                None,
                &PlayTarget::default(),
                false
            ),
            None
        );
    }

    /// Plans/player-polish.md 1.3: one start's progress never goes back,
    /// not when its peers and speed drop, nor when the next source starts;
    /// it stays under 1 with all of the start in; each stage gets its words
    /// once the source has taken 3 s.
    #[tokio::test(start_paused = true)]
    async fn a_start_never_goes_back_and_names_its_stage() {
        let link = |n: u32| stream(&format!(r#"{{"url":"http://127.0.0.1:9/{n}.mkv"}}"#));
        let mut s = State::new(vec![link(1), link(2)], None);
        s.next = 1;
        begin_start(&mut s, StartStage::Details);
        let at = |stage, peers, have, rate| Sample {
            stage,
            peers,
            have,
            need: 8_400_000,
            rate,
        };
        use StartStage::{Buffering, Connecting, Details};
        let steps = [
            (0, at(Details, 0, 0, 0), None),
            (4, at(Details, 0, 0, 0), Some("Getting the file's details")),
            (1, at(Connecting, 3, 0, 0), Some("Finding peers, 3 so far")),
            (
                1,
                at(Buffering, 6, 4_100_000, 2 << 20),
                Some("Buffering, 4.1 of 8.4 MB"),
            ),
            (
                1,
                at(Buffering, 1, 4_100_000, 0),
                Some("Buffering, 4.1 of 8.4 MB"),
            ),
            (
                1,
                at(Buffering, 20, 8_400_000, 9 << 20),
                Some("Buffering, 8.4 of 8.4 MB"),
            ),
        ];
        let mut seen: Vec<f32> = Vec::new();
        for (wait, x, words) in steps {
            tokio::time::advance(Duration::from_secs(wait)).await;
            show_start(&mut s, x);
            let start = s.snapshot.start.clone().unwrap();
            assert_eq!(start.caption.as_deref(), words, "{x:?}");
            assert_eq!(start.stage, x.stage);
            seen.push(start.progress);
        }
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
        assert_eq!(seen[4], seen[3], "fewer peers and no speed: held");
        assert!(seen[3] > seen[2] && seen[2] > seen[1], "{seen:?}");
        let last = seen[5];
        assert!(last > 0.9 && last < 1.0, "{last}");
        // The next source goes on from there and says which it is.
        s.next = 2;
        begin_start(&mut s, Details);
        let start = s.snapshot.start.clone().unwrap();
        assert_eq!((start.source, start.sources, start.progress), (2, 2, last));
        assert_eq!(start.caption.as_deref(), Some("Trying source 2 of 2"));
        show_start(&mut s, Sample::default());
        let start = s.snapshot.start.clone().unwrap();
        assert_eq!(start.progress, last, "never back");
        assert_eq!(start.caption.as_deref(), Some("Trying source 2 of 2"));
        // A later start of a file that was Ready once: the engine counts
        // nothing, and the words say no numbers.
        tokio::time::advance(Duration::from_secs(4)).await;
        show_start(&mut s, at(Buffering, 4, 0, 0).with_need(0));
        let start = s.snapshot.start.clone().unwrap();
        assert_eq!(start.caption.as_deref(), Some("Buffering"));
        assert_eq!(start.progress, last, "never back");
    }

    /// Plans/player-polish.md 2.2: a track is named by what tells its
    /// release apart, the group then source and resolution; by the text
    /// itself when it names none of them. A "-hi", ".HI." or "SDH" mark is
    /// for the hard of hearing, and never read as the group.
    #[test]
    fn a_track_is_named_by_what_its_release_says() {
        for (text, name) in [
            ("Show.S01E01.1080p.WEB.h264-CAKES", Some("CAKES, WEB 1080p")),
            (
                "Show.S01E01.1080p.WEBRip.x265.10bit-PSA",
                Some("PSA, WEBRip 1080p 10 bit"),
            ),
            ("Show.S01E01.DVDRip.XviD-ORPHEUS", Some("ORPHEUS, DVD")),
            (
                "Movie.2020.2160p.UHD.BluRay.REMUX.HEVC-FGT",
                Some("FGT, Remux 2160p"),
            ),
            ("TOPAZ", None),
        ] {
            assert_eq!(
                short_name(&mq_relparse::parse(text)).as_deref(),
                name,
                "{text}"
            );
        }
        let track = |release: &str| {
            SubtitleTrack::new("u".into(), "eng", None, "Subs", Some(release.into()))
        };
        let hi = track("Sintel.2010.1080p.WEB-DL.DDP5.1.H.264-FLUX-hi");
        assert_eq!(
            (hi.name.as_str(), hi.hearing_impaired),
            ("FLUX, WEB 1080p", true)
        );
        assert_eq!(
            hi.release.as_deref(),
            Some("Sintel.2010.1080p.WEB-DL.DDP5.1.H.264-FLUX-hi"),
            "the text kept for the detail line"
        );
        let wire = serde_json::to_value(&hi).unwrap();
        assert_eq!(
            wire["release"],
            hi.release.as_deref().unwrap(),
            "on the wire"
        );
        for marked in [
            "Film.2020.1080p.WEB-FLUX.HI",
            "Film 2020 SDH",
            "Film.2020-HI",
            "Film.2020.1080p.WEB-GRP [HI]",
            "Film.2020.hi.1080p.WEB-GRP",
            // Round 2 N1: the sidecar naming, the track's own language first.
            "Film.2020.1080p.WEB-GRP.en.hi",
        ] {
            assert!(track(marked).hearing_impaired, "{marked}");
        }
        // Round 2 N2: a bracketed mark goes with both its brackets, so the
        // group survives in the name.
        for marked in [
            "Film.2020.1080p.WEB-GRP [HI]",
            "Film.2020.1080p.WEB-GRP (SDH)",
            "Film.2020.1080p.WEB-GRP.en.hi",
        ] {
            assert_eq!(track(marked).name, "GRP, WEB 1080p", "{marked}");
        }
        // Review P3-2: beside a language "HI" is Hindi; a title's "Hi" is
        // no mark.
        for plain in [
            "Hi.Score.Girl.S01E01.1080p.WEB-GRP",
            "Say.Hi.2020.1080p.WEB-GRP",
            "Film.2020.1080p.WEB-HIGH",
            "Movie.2020.1080p.WEB-DL.EN-HI",
            "Movie.2020.1080p.WEB.DD5.1.HI.TA-GRP",
        ] {
            assert!(!track(plain).hearing_impaired, "{plain}");
        }
        assert_eq!(track("TOPAZ").name, "TOPAZ", "nothing parses: the text");
        // Review P3-3: a language or a title word is no group, and with no
        // source or resolution beside it nothing is.
        for text in ["Sintel (2010) - English", "The.Amazing.Spider-Man"] {
            assert_eq!(track(text).name, text, "{text}");
        }
        let bare = SubtitleTrack::new("u".into(), "eng", None, "Subs", None);
        assert_eq!((bare.name.as_str(), bare.release), ("Subs", None));
    }

    /// Review P3-3: the playing release's "group" is no language or title
    /// word, so no subtitle fits by naming it.
    #[test]
    fn the_playing_group_is_a_release_group() {
        let group = |name: &str| OwnedPlaying::of(&mq_relparse::parse(name)).group;
        assert_eq!(group("Sintel (2010) - English"), None);
        assert_eq!(group("Movie.2020.1080p.WEB-DL.Hindi-English"), None);
        assert_eq!(group("The.Amazing.Spider-Man"), None);
        assert_eq!(
            group("Sintel.2010.1080p.WEB-DL-FLUX").as_deref(),
            Some("FLUX")
        );
    }

    /// Review P3-4: a label that only names the language gives way to the
    /// file name.
    #[test]
    fn a_language_label_gives_way_to_the_file_name() {
        let body = br#"{"subtitles":[{"id":"1","lang":"eng","label":"English",
            "url":"https://subs.test/1.srt","subtitleFileName":"Film.2020.1080p.WEB.h264-CAKES.srt"}]}"#;
        let w = &decode_subtitles(body).unwrap().subtitles[0];
        assert_eq!(
            release(w).as_deref(),
            Some("Film.2020.1080p.WEB.h264-CAKES")
        );
        assert_eq!(
            track(w, "Subs", "", "u".into(), false).name,
            "CAKES, WEB 1080p"
        );
    }

    /// Review P3-8: while the first byte is read through the server the
    /// start says so, its numbers kept, instead of sitting at the last
    /// count's words.
    #[tokio::test(start_paused = true)]
    async fn the_first_read_shows_as_opening() {
        let mut st = State::new(vec![stream(r#"{"url":"http://127.0.0.1:9/a.mkv"}"#)], None);
        st.next = 1;
        begin_start(&mut st, StartStage::Details);
        tokio::time::advance(Duration::from_secs(4)).await;
        let full = Sample {
            stage: StartStage::Buffering,
            peers: 6,
            have: 8_400_000,
            need: 8_400_000,
            rate: 2 << 20,
        };
        show_start(&mut st, full);
        let before = st.snapshot.start.clone().unwrap();
        let snapshot = SharedObservable::new(st.snapshot.clone());
        let state = Mutex::new(st);
        show_opening(&state, &snapshot, 0);
        let start = lock(&state).snapshot.start.clone().unwrap();
        assert_eq!(
            (start.stage, start.caption.as_deref()),
            (StartStage::Opening, Some("Opening the video"))
        );
        assert_eq!(
            (start.have_bytes, start.need_bytes, start.peers),
            (before.have_bytes, before.need_bytes, before.peers)
        );
        assert!(start.progress >= before.progress && start.progress < 1.0);
    }

    /// Review P1-1: a resume lands at its saved share of the file, whatever
    /// rate would size the window; none with the duration unknown, at the
    /// end, or past it.
    #[test]
    fn a_resume_lands_at_its_share_of_the_file() {
        let len: u64 = 2_500_000_000;
        // 40 of 80 minutes is half way; an hour's rate floor says 2/3.
        assert_eq!(resume_offset(2_400_000, 4_800_000, len), Some(len / 2));
        assert_eq!(resume_offset(0, 4_800_000, len), Some(0));
        assert_eq!(resume_offset(1_000, 0, len), None, "duration unknown");
        assert_eq!(resume_offset(4_800_000, 4_800_000, len), None, "the end");
        assert_eq!(resume_offset(u64::MAX, 1, len), None, "an absurd position");
    }

    /// A torrent's fill is the engine's count of what Ready waits on, not
    /// the payload received; it buffers once a peer unchoked or sent.
    #[cfg(feature = "torrent")]
    #[test]
    fn a_torrents_start_is_the_engines_ready_set() {
        let mut stats = mq_engine::TorrentStats {
            peers: 3,
            start_need: 3 << 20,
            start_have: 1 << 20,
            received_bytes: 9 << 20,
            down_bytes_per_s: 5,
            ..Default::default()
        };
        let x = sample(&stats);
        assert_eq!(
            (x.stage, x.peers, x.have, x.need, x.rate),
            (StartStage::Buffering, 3, 1 << 20, 3 << 20, 5)
        );
        stats.received_bytes = 0;
        assert_eq!(sample(&stats).stage, StartStage::Connecting);
        stats.unchoked_ms = Some(1);
        assert_eq!(sample(&stats).stage, StartStage::Buffering);
    }

    /// Start-latency 4.1: a torrent that did not start is not waited on
    /// again from another source in the same view: the second is skipped
    /// at once, with no second 30 s for its details.
    #[cfg(feature = "torrent")]
    #[tokio::test(start_paused = true)]
    async fn a_torrent_that_did_not_start_is_skipped_from_another_source() {
        let ((engine, _dir), silent) = (own_engine().await, silent_tracker());
        let core = Core::temporary("default").unwrap();
        core.set_torrent_engine(engine);
        let dead = dead_torrent(&silent);
        let view = PlaybackView::open(&core, vec![dead.clone(), dead], PlayTarget::default());
        let begun = tokio::time::Instant::now();
        for _ in 0..1200 {
            if view.snapshot().phase == PlaybackPhase::Failed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let s = view.snapshot();
        let errors: Vec<&str> = s.attempts.iter().map(|a| a.error.as_str()).collect();
        assert_eq!(
            errors,
            ["No peer sent the torrent's details in time", SAME_TORRENT]
        );
        let took = begun.elapsed();
        assert!(took < METADATA_TIMEOUT + Duration::from_secs(5), "{took:?}");
        assert_eq!(s.start, None, "cleared at the failure");
    }

    /// An HTTP tracker's answer naming `peer` (on IPv6), or no one.
    #[cfg(feature = "torrent")]
    fn announce(peer: Option<std::net::SocketAddr>) -> Vec<u8> {
        let mut body = b"d8:intervali1800e5:peers0:".to_vec();
        if let Some(std::net::SocketAddr::V6(peer)) = peer {
            body.extend(b"6:peers618:");
            body.extend(peer.ip().octets());
            body.extend(peer.port().to_be_bytes());
        }
        body.push(b'e');
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        [head.into_bytes(), body].concat()
    }

    /// A tracker on `[::1]` that names no one.
    #[cfg(feature = "torrent")]
    fn silent_tracker() -> super::engine_support::HttpTracker {
        super::engine_support::HttpTracker::start(announce(None), false)
    }

    /// An engine of its own that reaches `[::1]` only, with no DHT.
    #[cfg(feature = "torrent")]
    async fn own_engine() -> (mq_engine::Engine, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let scope = http.torrent_scope(&[std::net::Ipv6Addr::LOCALHOST.into()]);
        let config = mq_engine::EngineConfig {
            discovery: false,
            ..mq_engine::EngineConfig::new(dir.path(), scope)
        };
        (mq_engine::Engine::start(config).await.unwrap(), dir)
    }

    /// A torrent whose one tracker names no one: its details never come.
    #[cfg(feature = "torrent")]
    fn dead_torrent(silent: &super::engine_support::HttpTracker) -> ParsedStream {
        stream(&format!(
            r#"{{"infoHash":"{}","fileIdx":0,"sources":["tracker:{}"]}}"#,
            "d".repeat(40),
            silent.url
        ))
    }

    /// Start-latency 4.1: while the first torrent waits for its details,
    /// the next torrent candidate's come in from its own tracker, so they
    /// are known the moment the first fails; when the view closes, the
    /// torrent no start took is removed.
    #[cfg(feature = "torrent")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_next_torrents_details_come_while_the_first_waits() {
        let s = super::engine_support::swarm_on(std::net::Ipv6Addr::LOCALHOST.into(), true).await;
        let silent = silent_tracker();
        let tracker =
            super::engine_support::HttpTracker::start(announce(Some(s.seeder_addr)), false);
        let core = Core::temporary("default").unwrap();
        core.set_torrent_engine(s.leecher.clone());
        let dead = dead_torrent(&silent);
        let next = stream(&format!(
            r#"{{"infoHash":"{}","sources":["tracker:{}"]}}"#,
            s.info_hash, tracker.url
        ));
        let mut st = State::new(vec![dead, next], None);
        // The first gets 6 s for its details, then the run's budget is spent.
        st.retry_until = Some(tokio::time::Instant::now() + Duration::from_secs(6));
        let snapshot = SharedObservable::new(st.snapshot.clone());
        let state = Arc::new(Mutex::new(st));
        let target = Arc::new(PlayTarget::default());
        let run = tokio::spawn(resolve_next(
            core,
            target,
            Arc::from([]),
            state.clone(),
            snapshot,
            0,
        ));
        // Review P2-2: nothing is asked of the next swarm in the first 3 s.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(tracker.requests(), 0, "the next torrent was asked at once");
        run.await.unwrap();
        let errors: Vec<String> = lock(&state)
            .snapshot
            .attempts
            .iter()
            .map(|a| a.error.clone())
            .collect();
        assert_eq!(
            errors,
            ["No peer sent the torrent's details in time", RETRY_GAVE_UP]
        );
        assert!(
            tracker.requests() > 0,
            "the next torrent's tracker was asked"
        );
        assert_eq!(
            lock(&state).snapshot.start,
            None,
            "cleared as the budget ran out"
        );
        let known = tokio::time::timeout(
            Duration::from_millis(200),
            s.leecher.add_magnet(&s.info_hash, &[], &[]),
        )
        .await;
        let torrent = known
            .expect("the next torrent's details were not fetched ahead")
            .unwrap();
        lock(&state).closing.cancel();
        for _ in 0..100 {
            if s.leecher.files(torrent).is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the torrent fetched ahead outlived the view");
    }

    /// A tracker on `[::1]` that answers each announce with `peer` after
    /// 1.5 s, counting the announces as they come.
    #[cfg(feature = "torrent")]
    fn slow_tracker(peer: std::net::SocketAddr) -> (String, Arc<Mutex<usize>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, 0)).unwrap();
        let url = format!("http://{}/announce", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(0));
        let count = asked.clone();
        std::thread::spawn(move || {
            for mut socket in listener.incoming().flatten() {
                let count = count.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 2048];
                    let _ = socket.read(&mut buf);
                    *lock(&count) += 1;
                    std::thread::sleep(Duration::from_millis(1500));
                    let _ = socket.write_all(&announce(Some(peer)));
                });
            }
        });
        (url, asked)
    }

    /// Review P2-3, P3-1: a candidate whose details are being fetched ahead
    /// waits for that fetch instead of asking its swarm again, takes the
    /// torrent (the view's close then leaves it), and drops the entry.
    #[cfg(feature = "torrent")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_candidates_own_turn_waits_for_its_fetch_ahead() {
        let s = super::engine_support::swarm_on(std::net::Ipv6Addr::LOCALHOST.into(), true).await;
        let (url, asked) = slow_tracker(s.seeder_addr);
        let core = Core::temporary("default").unwrap();
        core.set_torrent_engine(s.leecher.clone());
        let next = stream(&format!(
            r#"{{"infoHash":"{}","sources":["tracker:{url}"]}}"#,
            s.info_hash
        ));
        let mut st = State::new(vec![next], None);
        // As when an earlier candidate waited: its fetch is in flight.
        look_ahead(&core, &s.leecher, &mut st, "an earlier one");
        assert_eq!(st.ahead.len(), 1);
        let snapshot = SharedObservable::new(st.snapshot.clone());
        let state = Arc::new(Mutex::new(st));
        let target = Arc::new(PlayTarget::default());
        let run = tokio::spawn(resolve_next(
            core,
            target,
            Arc::from([]),
            state.clone(),
            snapshot,
            0,
        ));
        tokio::time::sleep(Duration::from_millis(1000)).await;
        assert_eq!(*lock(&asked), 1, "its own turn asked the swarm again");
        run.await.unwrap();
        {
            let s2 = lock(&state);
            assert_eq!(s2.snapshot.phase, PlaybackPhase::Ready, "{:?}", s2.snapshot);
            assert!(s2.ahead.is_empty(), "the taken entry stays");
            s2.closing.cancel();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let torrent = tokio::time::timeout(
            Duration::from_millis(200),
            s.leecher.add_magnet(&s.info_hash, &[], &[]),
        )
        .await
        .expect("the taken torrent was removed at close")
        .unwrap();
        assert!(s.leecher.files(torrent).is_ok());
    }

    /// Review P2-3: "Try again" gives a torrent that did not start its full
    /// wait again: it is not skipped as one that failed in this run.
    #[cfg(feature = "torrent")]
    #[tokio::test(start_paused = true)]
    async fn try_again_waits_on_a_torrent_that_did_not_start_again() {
        let ((engine, _dir), silent) = (own_engine().await, silent_tracker());
        let core = Core::temporary("default").unwrap();
        core.set_torrent_engine(engine);
        let view = PlaybackView::open(&core, vec![dead_torrent(&silent)], PlayTarget::default());
        let failed = || async {
            for _ in 0..1200 {
                if view.snapshot().phase == PlaybackPhase::Failed {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("never failed: {:?}", view.snapshot());
        };
        failed().await;
        view.retry();
        failed().await;
        let errors: Vec<String> = view
            .snapshot()
            .attempts
            .iter()
            .map(|a| a.error.clone())
            .collect();
        assert_eq!(errors, ["No peer sent the torrent's details in time"]);
    }

    /// Review P2-3: a torrent whose details are known but that sends nothing
    /// by the deadline (`Late`) is skipped from another source too.
    #[cfg(feature = "torrent")]
    #[tokio::test]
    async fn a_torrent_late_at_its_deadline_is_skipped_from_another_source() {
        let (engine, dir) = own_engine().await;
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("movie.mkv"), vec![7u8; 1 << 20]).unwrap();
        let spawner = mq_engine::librqbit::spawn_utils::BlockingSpawner::new(2);
        let options = mq_engine::librqbit::CreateTorrentOptions {
            name: Some("Movie"),
            trackers: vec![],
            piece_length: Some(256 << 10),
        };
        let created = mq_engine::librqbit::create_torrent(&src, options, &spawner)
            .await
            .unwrap();
        let hash = created.info_hash().as_string();
        // Its details known, no peer to send it.
        engine
            .add_torrent_bytes(created.as_bytes().unwrap().to_vec(), &[])
            .await
            .unwrap();
        let core = Core::temporary("default").unwrap();
        core.set_torrent_engine(engine);
        let known = stream(&format!(r#"{{"infoHash":"{hash}","fileIdx":0}}"#));
        tokio::time::pause();
        let view = PlaybackView::open(&core, vec![known.clone(), known], PlayTarget::default());
        for _ in 0..3000 {
            if view.snapshot().phase == PlaybackPhase::Failed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let errors: Vec<String> = view
            .snapshot()
            .attempts
            .iter()
            .map(|a| a.error.clone())
            .collect();
        assert_eq!(
            errors,
            [
                "the swarm did not send the start of the video in time",
                SAME_TORRENT
            ]
        );
    }

    /// Review P2-3: a start that ends late or stalled marks its swarm as not
    /// started in this view; a Ready, a wrong file or an error does not.
    #[test]
    fn late_and_stalled_starts_are_swarms_that_did_not_start() {
        assert!(not_started(&Started::<()>::Late { unchoked: true }));
        assert!(not_started(&Started::<()>::Stalled { nothing: false }));
        assert!(!not_started(&Started::Ready(())));
        assert!(!not_started(&Started::<()>::Wrong(
            "another episode".into()
        )));
    }

    /// Review P3-10: one torrent is one hash however the addon spells it.
    #[cfg(feature = "torrent")]
    #[test]
    fn a_hash_reads_the_same_in_base32_and_hex() {
        let hex = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";
        let base32 = "YEX6DQDLXISUVHOJ6UM3GNNKPQJWPKEK";
        assert_eq!(torrent_id(base32), hex);
        assert_eq!(torrent_id(&hex.to_uppercase()), hex);
        assert_eq!(torrent_id(" not a hash "), "not a hash");
    }

    /// Review P2-3: a start's tick from a run the view moved on from, or one
    /// that lands after Ready, never touches the start shown.
    #[cfg(feature = "torrent")]
    #[test]
    fn a_late_or_stale_tick_never_shows_a_start() {
        let mut st = State::new(vec![stream(r#"{"url":"http://127.0.0.1:9/a.mkv"}"#)], None);
        st.next = 1;
        begin_start(&mut st, StartStage::Details);
        st.run = 2;
        let shown = st.snapshot.start.clone();
        let snapshot = SharedObservable::new(st.snapshot.clone());
        let state = Mutex::new(st);
        let buffering = Sample {
            stage: StartStage::Buffering,
            peers: 5,
            have: 1 << 20,
            need: 2 << 20,
            rate: 1 << 20,
        };
        publish_start((&state, &snapshot, 1), buffering);
        assert_eq!(lock(&state).snapshot.start, shown, "a stale run");
        lock(&state).snapshot.phase = PlaybackPhase::Ready;
        publish_start((&state, &snapshot, 2), buffering);
        assert_eq!(lock(&state).snapshot.start, shown, "after Ready");
    }
}

#[cfg(all(test, feature = "torrent"))]
#[path = "../../../mq-engine/tests/support/mod.rs"]
#[allow(dead_code)]
mod engine_support;

#[cfg(test)]
mod auto_subtitle_tests {
    use super::*;

    fn track(url: &str, lang: &str) -> SubtitleTrack {
        SubtitleTrack::new(url.into(), lang, None, "Subs", None)
    }

    #[test]
    fn the_track_turned_on_at_start() {
        let tracks = [
            track("tr1", "tur"),
            track("tr2", "tur"),
            track("en1", "eng"),
            track("br1", "pob"),
        ];
        let tags = |l: &[&str]| -> Vec<String> { l.iter().map(|s| s.to_string()).collect() };
        let pick = |remembered: Option<&str>, explicit: &[&str], audio: &[&str]| {
            auto_subtitle(remembered, &tags(explicit), &tags(audio), &tracks)
        };
        // (remembered, profile languages, audio) -> the track turned on.
        type Case<'a> = (
            Option<&'a str>,
            &'a [&'a str],
            &'a [&'a str],
            Option<&'a str>,
        );
        let table: &[Case] = &[
            (None, &[], &[], None),
            (None, &[], &["en"], None),
            (None, &["tr"], &[], Some("tr1")),
            (None, &["tr"], &["en"], Some("tr1")),
            (None, &["tr"], &["tr"], None),
            (None, &["en"], &["en"], None),
            (None, &["tr", "en"], &["en"], Some("tr1")),
            (None, &["en", "tr"], &["en"], Some("tr1")),
            (None, &["tr", "en"], &["tr"], Some("en1")),
            (None, &["de", "en"], &["ja"], Some("en1")),
            (None, &["pt"], &["en"], Some("br1")),
            (None, &["de"], &["en"], None),
            (Some(OFF), &["tr"], &["en"], None),
            (Some("en"), &["tr"], &["tr"], Some("en1")),
            (Some("en"), &[], &[], Some("en1")),
        ];
        for (remembered, explicit, audio, expected) in table {
            assert_eq!(
                pick(*remembered, explicit, audio).as_deref(),
                *expected,
                "{remembered:?} {explicit:?} {audio:?}"
            );
        }
    }

    /// Review of feat/subtitle-best P2-3: a forced track is never turned
    /// on, even as its language's only one, and a plain regional track
    /// wins over a forced one of the exact tag.
    #[test]
    fn a_forced_track_is_never_turned_on() {
        let forced = |url: &str, lang: &str| SubtitleTrack {
            forced: true,
            ..track(url, lang)
        };
        let tags = |l: &[&str]| -> Vec<String> { l.iter().map(|s| s.to_string()).collect() };
        let only_forced = [forced("trf", "tur"), track("en1", "eng")];
        assert_eq!(auto_subtitle(None, &tags(&["tr"]), &[], &only_forced), None);
        assert_eq!(auto_subtitle(Some("tr"), &[], &[], &only_forced), None);
        let regional = [forced("ptf", "por"), track("br1", "pob")];
        assert_eq!(
            auto_subtitle(None, &tags(&["pt"]), &[], &regional).as_deref(),
            Some("br1")
        );
    }
}

#[cfg(test)]
mod subtitle_order_tests {
    use super::*;

    /// One OpenSubtitles v3 or SubSense row as it arrives.
    fn row(addon: &str, n: usize, file: &str, fps: Option<u32>, m: &str) -> SubtitleTrack {
        let mut json = serde_json::json!({
            "id": n.to_string(),
            "url": format!("https://subs.example/{n}"),
            "lang": "eng",
            "m": m,
            "subtitleFileName": file,
        });
        if let Some(fps) = fps {
            json["fpsMilli"] = fps.into();
        }
        let w: mq_protocol::WireSubtitle = serde_json::from_value(json).unwrap();
        track(&w, addon, addon, format!("u{n}"), false)
    }

    fn ordered(playing: &str, fps: f32, rows: Vec<(usize, SubtitleTrack)>) -> Vec<String> {
        let mut s = State::new(Vec::new(), None);
        s.playing = OwnedPlaying {
            fps: Some(fps),
            ..OwnedPlaying::of(&mq_relparse::parse(playing))
        };
        for (position, t) in rows {
            s.order.insert(t.url.clone(), (position, s.order.len()));
            s.snapshot.subtitles.push(t);
        }
        reorder(&mut s, &["en".to_owned()]);
        s.snapshot.subtitles.iter().map(|t| t.url.clone()).collect()
    }

    const V3: &str = OPENSUBTITLES;

    /// OpenSubtitles marks a forced file in its name, not in the flag: the
    /// word counts on its own, not inside another.
    #[test]
    fn a_forced_file_name_marks_the_track_forced() {
        let named = row(V3, 1, "Film.2020.1080p.WEB-DL-FLUX.forced.srt", None, "h");
        assert!(named.forced);
        assert!(!row(V3, 2, "Forcedly.2020.1080p.WEB-DL-FLUX.srt", None, "h").forced);
        // A film called "Forced Vengeance": the word is its title's, in
        // every file's name; the addon's flag still counts.
        let w: mq_protocol::WireSubtitle = serde_json::from_value(serde_json::json!({
            "id": "3", "url": "https://subs.example/3", "lang": "eng",
            "subtitleFileName": "Forced.Vengeance.1982.1080p.BluRay.x264-GRP.srt",
        }))
        .unwrap();
        assert!(!track(&w, V3, V3, "u3".into(), true).forced);
        assert!(track(&w, V3, V3, "u3".into(), false).forced);
        let flagged = mq_protocol::WireSubtitle { forced: true, ..w };
        assert!(track(&flagged, V3, V3, "u3".into(), true).forced);
    }

    /// Review of feat/subtitle-best P2-1: the built-in source marks the
    /// hard of hearing on its row, not in the release text; that row still
    /// follows a plain one that matches as well.
    #[test]
    fn a_built_in_row_for_the_hard_of_hearing_follows_a_plain_one() {
        let built_in = |altid: u32, hearing_impaired: bool| {
            let row = mq_turkcealtyazi::Row {
                altid,
                path: format!("/sub/{altid}/x.html"),
                lang: mq_turkcealtyazi::Lang::Turkish,
                kind: mq_turkcealtyazi::Kind::Movie { discs: 1 },
                release: "Film.2020.1080p.WEB-DL-OTHER".into(),
                source: None,
                fps: None,
                downloads: 0,
                translator: None,
                hearing_impaired,
            };
            let label = row.release.clone();
            let url = format!("u{altid}");
            built_in_track(
                url,
                format!("turkcealtyazi:{altid}"),
                mq_turkcealtyazi::Track { row, label },
            )
        };
        let mut s = State::new(Vec::new(), None);
        s.playing = OwnedPlaying::of(&mq_relparse::parse("Film.2020.1080p.WEB-DL-FLUX"));
        for t in [built_in(1, true), built_in(2, false)] {
            s.order.insert(t.url.clone(), (1, s.order.len()));
            s.snapshot.subtitles.push(t);
        }
        reorder(&mut s, &["tr".to_owned()]);
        let urls: Vec<&str> = s
            .snapshot
            .subtitles
            .iter()
            .map(|t| t.url.as_str())
            .collect();
        assert_eq!(urls, ["u2", "u1"]);
    }

    /// Plans/research/subtitle-pick.md 5, case A: Inception, v3 only, a
    /// 23.976 BluRay playing. Today a 25 fps CAM led; now the 30 fps rip
    /// (NTSC keeps film time) leads the 23.976 TS, and both lead the 25 fps
    /// captures, which keep the addon's order.
    #[test]
    fn inception_from_v3_puts_film_rates_before_pal_captures() {
        let rows = vec![
            (
                1,
                row(V3, 0, "Inception.2010.CAM.Xvid-LKRG.srt", Some(25000), "i"),
            ),
            (
                1,
                row(
                    V3,
                    1,
                    "Inception.2010.TS.V2.XVID.FEEL-FREE.srt",
                    Some(25000),
                    "i",
                ),
            ),
            (1, row(V3, 2, "Inception.TS.XviD-KiK.srt", Some(23976), "i")),
            (
                1,
                row(
                    V3,
                    3,
                    "Inception.2010.480p.TS.XviD-ViSiON.srt",
                    Some(25000),
                    "i",
                ),
            ),
            (
                1,
                row(
                    V3,
                    4,
                    "Inception.2010.XviD-Ripped By IndrashekharC.srt",
                    Some(30000),
                    "i",
                ),
            ),
        ];
        let order = ordered("Inception.2010.1080p.BluRay.x264-SPARKS.mkv", 23.976, rows);
        assert_eq!(order, ["u4", "u2", "u0", "u1", "u3"]);
    }

    /// Case B: SubSense's BluRay row (no rate) leads every v3 row.
    #[test]
    fn inception_with_subsense_puts_the_bluray_row_first() {
        let rows = vec![
            (1, row(V3, 2, "Inception.TS.XviD-KiK.srt", Some(23976), "i")),
            (
                1,
                row(
                    V3,
                    4,
                    "Inception.2010.XviD-Ripped By IndrashekharC.srt",
                    Some(30000),
                    "i",
                ),
            ),
            (
                2,
                row(
                    "com.subsense",
                    9,
                    "Inception.2010.1080p.BluRay.DTS.x264-CtrlHD.srt",
                    None,
                    "",
                ),
            ),
        ];
        let order = ordered("Inception.2010.1080p.BluRay.x264-SPARKS.mkv", 23.976, rows);
        assert_eq!(order[0], "u9");
    }

    /// Case C: Sintel's VODO 2K release. Today the row naming VODO, 10.5 s
    /// late, led; now the hash rows lead, the one at the playing rate first.
    /// A hash mark from another addon counts for nothing.
    #[test]
    fn sintel_vodo_puts_the_hash_rows_first() {
        let rows = vec![
            (1, row(V3, 0, "", Some(24000), "h")),
            (1, row(V3, 1, "sintel.srt", None, "h")),
            (1, row(V3, 2, "Sintel (2010).srt", None, "i")),
            (1, row(V3, 3, "Sintel.2010.Xvid-VODO.srt", Some(24000), "i")),
            (2, row("other.addon", 4, "Sintel.2010.srt", None, "h")),
        ];
        let order = ordered("Sintel.2010.2K.SURROUND.x264-VODO.mp4", 24.0, rows);
        assert_eq!(order, ["u0", "u1", "u3", "u2", "u4"]);
    }

    /// What the picker shows: "Matches this file" for a trusted hash, "Made
    /// for 25 fps" only on a PAL conflict; a forced row and a CD2 row never
    /// lead their language.
    #[test]
    fn the_flags_the_picker_shows_and_the_rows_that_never_lead() {
        // The addon's own forced flag, read through `track`.
        let forced: mq_protocol::WireSubtitle = serde_json::from_value(serde_json::json!({
            "id": "5", "url": "https://subs.example/5", "lang": "eng", "m": "h",
            "forced": true, "fpsMilli": 23976,
            "subtitleFileName": "Inception.2010.1080p.BluRay.x264-SPARKS.forced.srt",
        }))
        .unwrap();
        let forced = track(&forced, V3, V3, "u5".into(), false);
        let rows = vec![
            (1, forced),
            (
                1,
                row(
                    V3,
                    6,
                    "Inception.2010.1080p.BluRay.x264-SPARKS.CD2.srt",
                    Some(23976),
                    "i",
                ),
            ),
            (
                1,
                row(V3, 7, "Inception.2010.CAM.Xvid-LKRG.srt", Some(25000), "i"),
            ),
            (
                1,
                row(
                    V3,
                    8,
                    "Inception.2010.1080p.BluRay.x264-OTHER.srt",
                    Some(23976),
                    "h",
                ),
            ),
        ];
        let mut s = State::new(Vec::new(), None);
        s.playing = OwnedPlaying {
            fps: Some(23.976),
            ..OwnedPlaying::of(&mq_relparse::parse(
                "Inception.2010.1080p.BluRay.x264-SPARKS.mkv",
            ))
        };
        for (position, t) in rows {
            s.order.insert(t.url.clone(), (position, s.order.len()));
            s.snapshot.subtitles.push(t);
        }
        reorder(&mut s, &["en".to_owned()]);
        let urls: Vec<&str> = s
            .snapshot
            .subtitles
            .iter()
            .map(|t| t.url.as_str())
            .collect();
        assert_eq!(urls, ["u8", "u7", "u6", "u5"]);
        let of = |url: &str| s.snapshot.subtitles.iter().find(|t| t.url == url).unwrap();
        assert!(of("u8").hash_match && of("u8").made_for_fps.is_none());
        assert_eq!(of("u7").made_for_fps, Some(25.0));
        let wire = serde_json::to_value(of("u7")).unwrap();
        assert_eq!(wire["hashMatch"], false);
        assert_eq!(wire["madeForFps"], 25.0);
    }

    #[test]
    fn a_picked_language_goes_first_once() {
        let first = |list: &[&str], tag: &str| {
            let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
            picked_first(&list, tag)
        };
        let list = |l: &[&str]| Some(l.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(first(&[], "tr"), list(&["tr"]));
        assert_eq!(first(&["en"], "tr"), list(&["tr", "en"]));
        assert_eq!(first(&["en", "tr"], "tr"), list(&["tr", "en"]));
        // Every spelling of the language gives way to its tag.
        assert_eq!(first(&["eng", "tur", "tr"], "tr"), list(&["tr", "eng"]));
        // A region is its own entry, kept as picked.
        assert_eq!(first(&["pt", "en"], "pt-BR"), list(&["pt-BR", "pt", "en"]));
        assert_eq!(first(&["pt-BR", "en"], "pt"), list(&["pt", "pt-BR", "en"]));
        // First already, or no language: nothing to write.
        assert_eq!(first(&["tr", "en"], "tr"), None);
        assert_eq!(first(&["tr"], "und"), None);
        assert_eq!(first(&["tr"], "xx"), None);
    }
}
