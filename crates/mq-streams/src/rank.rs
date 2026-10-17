//! Ranking: hard gates, a lexicographic key, a bounded within-tier score,
//! dedupe with failover chains, intent groups and plain-language reasons.

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
}

const MAX_REASONS: usize = 3;

pub fn rank(streams: Vec<ParsedStream>, input: &RankInput<'_>) -> Ranked {
    let mut kept = Vec::new();
    let mut hidden = Vec::new();
    for stream in streams {
        match gate(&stream, input) {
            Some(reason) => hidden.push(HiddenRow { stream, reason }),
            None => kept.push(stream),
        }
    }
    let mut scored: Vec<(RankKey, ParsedStream)> =
        kept.into_iter().map(|s| (key(&s, input), s)).collect();
    // Higher key first; ties keep addon order then response order.
    scored.sort_by(|(ka, a), (kb, b)| {
        kb.cmp(ka)
            .then(a.addon_rank.cmp(&b.addon_rank))
            .then(a.index.cmp(&b.index))
    });
    let rows = dedupe(scored, input);
    Ranked { rows, hidden }
}

fn gate(s: &ParsedStream, input: &RankInput<'_>) -> Option<HideReason> {
    if matches!(
        s.source,
        Source::Usenet { .. } | Source::Archive { .. } | Source::Invalid { .. }
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
    if wrong_title(s, input.target) {
        return Some(HideReason::WrongTitle);
    }
    if playability(s, input.caps).is_none() {
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
fn wrong_title(s: &ParsedStream, target: &Target) -> bool {
    let parsed = normalize_title(&s.release.title);
    let wanted = normalize_title(&target.title);
    if parsed.len() < 3 || wanted.len() < 3 {
        return false;
    }
    title_similarity(&parsed, &wanted) < 0.5
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

fn playability(s: &ParsedStream, caps: &DeviceCaps) -> Option<Playability> {
    let r = &s.release;
    let codec_ok = match r.codec {
        VideoCodec::Unknown => true,
        VideoCodec::H265 if r.bit_depth == Some(10) => {
            caps.ten_bit_hevc && caps.decoders.contains(&VideoCodec::H265)
        }
        codec => caps.decoders.contains(&codec),
    };
    let hdr_ok = r.hdr.iter().all(|h| match h {
        Hdr::Sdr => true,
        h => {
            caps.display_hdr.contains(h)
                || (*h == Hdr::DolbyVision && caps.display_hdr.contains(&Hdr::Hdr10))
        }
    });
    match (codec_ok, hdr_ok) {
        (true, true) => Some(Playability::Native),
        _ if caps.has_fallback_engine => Some(Playability::NeedsFallbackEngine),
        (false, _) => None,
        (true, false) => Some(Playability::NeedsTranscode),
    }
}

fn tier(s: &ParsedStream) -> u8 {
    match (&s.source, s.cached) {
        (Source::ClientResolve(_), Some(true)) => 4,
        (Source::Http { .. }, Some(true)) => 4,
        (Source::Http { .. }, _) => 3,
        (Source::Torrent { .. }, _) if s.seeders.unwrap_or(1) > 0 => 2,
        (Source::ClientResolve(_), _) => 1,
        _ => 0,
    }
}

fn effective_height(s: &ParsedStream, input: &RankInput<'_>) -> u32 {
    let mut cap = input.caps.display_height;
    if let Some(max) = input.prefs.max_height {
        cap = cap.min(max);
    }
    let height = s.release.resolution.height();
    if height == 0 {
        720.min(cap)
    } else {
        height.min(cap)
    }
}

fn key(s: &ParsedStream, input: &RankInput<'_>) -> RankKey {
    RankKey {
        tier: tier(s),
        binge: matches!((input.last_binge_group, &s.binge_group), (Some(a), Some(b)) if a == b),
        playability: playability(s, input.caps).unwrap_or(Playability::NeedsTranscode),
        effective_height: effective_height(s, input),
        within: within(s, input),
    }
}

/// Bounded 0..=1000 score inside a tier.
fn within(s: &ParsedStream, input: &RankInput<'_>) -> u16 {
    let r = &s.release;
    let mut score: i32 = match r.quality {
        Quality::Remux => 300,
        Quality::BluRay => 260,
        Quality::WebDl => 220,
        Quality::WebRip => 180,
        Quality::Unknown => 150,
        Quality::Hdtv => 120,
        Quality::Dvd => 80,
        Quality::Screener => 20,
        Quality::Telesync | Quality::Cam => 0,
    };
    if r.remux && r.quality != Quality::Remux {
        score += 40;
    }
    let caps = input.caps;
    if r.hdr.contains(&Hdr::DolbyVision) && caps.display_hdr.contains(&Hdr::DolbyVision) {
        score += 120;
    } else if r
        .hdr
        .iter()
        .any(|h| matches!(h, Hdr::Hdr10 | Hdr::Hdr10Plus | Hdr::DolbyVision))
        && caps.display_hdr.contains(&Hdr::Hdr10)
    {
        score += 100;
    }
    let atmos = r.audio.contains(&Audio::Atmos);
    let passthrough = r.audio.iter().any(|a| caps.passthrough.contains(a));
    score += match (
        atmos && passthrough,
        passthrough,
        surround(r.channels.as_deref()),
    ) {
        (true, _, _) => 80,
        (false, true, _) => 60,
        (false, false, true) => 40,
        _ => 20,
    };
    score += match s.seeders {
        Some(n) if matches!(s.source, Source::Torrent { .. }) => {
            ((n as f32 + 1.0).log10() * 40.0).min(120.0) as i32
        }
        _ if s.cached == Some(true) => 120,
        _ => 30,
    };
    score += language_score(s, input.prefs);
    score += size_score(s, input.prefs);
    score -= over_resolution_penalty(s, input.caps);
    score.clamp(0, 1000) as u16
}

/// Resolution the screen cannot show costs bandwidth and start time for
/// nothing: a 4K remux on a 1080p stick must not beat a 1080p release.
fn over_resolution_penalty(s: &ParsedStream, caps: &DeviceCaps) -> i32 {
    let height = s.release.resolution.height();
    if height == 0 || height <= caps.display_height {
        return 0;
    }
    let steps = (height / caps.display_height.max(1)).max(1) as i32;
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

fn size_score(s: &ParsedStream, prefs: &StreamPrefs) -> i32 {
    let Some(size) = s.size_bytes else { return 40 };
    let gb = size as f64 / 1e9;
    match prefs.quality {
        QualityPref::Best => 40,
        QualityPref::Balanced => (80.0 - (gb - 8.0).abs() * 4.0).clamp(0.0, 80.0) as i32,
        QualityPref::DataSaver => (100.0 - gb * 8.0).clamp(0.0, 100.0) as i32,
    }
}

/// Collapses duplicates into one row each; the rest become its failover.
fn dedupe(scored: Vec<(RankKey, ParsedStream)>, input: &RankInput<'_>) -> Vec<RankedRow> {
    let mut rows: Vec<RankedRow> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (key, stream) in scored {
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
    let any_ready = rows
        .iter()
        .any(|r| r.stream.cached == Some(true) || matches!(r.stream.source, Source::Http { .. }));
    for row in rows.iter_mut() {
        let s = &row.stream;
        let wants_lang = !input.prefs.languages.is_empty() && !s.release.languages.is_empty();
        let lang_match = input
            .prefs
            .languages
            .iter()
            .any(|l| s.release.languages.contains(l));
        row.group = if any_ready
            && row.key.tier <= 2
            && s.cached != Some(true)
            && !matches!(s.source, Source::Http { .. })
        {
            Group::NeedsDownload
        } else if wants_lang && !lang_match {
            Group::OtherLanguages
        } else if matches!((s.size_bytes, best_size), (Some(a), Some(b)) if a * 2 < b) {
            Group::SmallerFaster
        } else {
            Group::Best
        };
        let mut reasons = Vec::new();
        if row.key.tier == 4 {
            reasons.push(Reason::ReadyNow);
        }
        if row.key.binge {
            reasons.push(Reason::SameReleaseAsLast);
        }
        if row.key.playability == Playability::Native {
            reasons.push(Reason::PlaysDirectly);
        }
        if s.release.resolution.height() == input.caps.display_height {
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
