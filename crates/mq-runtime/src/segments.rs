//! Skip segments (intro, recap, credits, preview) for what is playing, from
//! public databases called from this device (plans/m8-skip.md). Nothing is
//! written anywhere: TheIntroDB allows a session cache only, so results live
//! in memory for the life of the core.

use mq_http::{HttpClient, HttpError, RequestOptions};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Per request; the playback view bounds a whole lookup separately.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
/// A source that answered 429 without `Retry-After` rests this long.
const REST: Duration = Duration::from_secs(3600);
/// Lookups kept in memory.
const CACHE_LOOKUPS: usize = 200;
/// When the release-matched source could not be asked (a timeout, a refusal), the
/// playback view asks it again after each of these waits while the version plays.
const RETRIES: [Duration; 3] = [
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(90),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum SegmentKind {
    Intro,
    Recap,
    Credits,
    Preview,
}

/// A stretch of the file a Skip button can jump over: `[startMs, endMs)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub kind: SegmentKind,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start_ms: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub end_ms: u64,
    /// Display name of the database it came from, for attribution.
    pub source: String,
}

/// What can be looked up for a playing item.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    Movie {
        imdb: String,
    },
    Episode {
        imdb: String,
        season: u32,
        episode: u32,
    },
    Kitsu {
        id: u64,
        episode: u32,
    },
}

impl Target {
    /// From a play target's kind and ids; `None` for ids no source knows.
    pub fn from_ids(kind: &str, item_id: &str, video_id: &str) -> Option<Self> {
        if let Some(rest) = video_id.strip_prefix("kitsu:") {
            let mut parts = rest.split(':');
            let id = parts.next()?.parse().ok()?;
            let episode = parts.next().map_or(Some(1), |e| e.parse().ok())?;
            return Some(Target::Kitsu { id, episode });
        }
        let mut parts = video_id.split(':');
        let imdb = parts.next().filter(|id| is_imdb(id))?.to_owned();
        match (parts.next(), parts.next()) {
            (Some(season), Some(episode)) => Some(Target::Episode {
                imdb,
                season: season.parse().ok().filter(|&s| s > 0)?,
                episode: episode.parse().ok()?,
            }),
            (None, None) if kind == "movie" && item_id == imdb => Some(Target::Movie { imdb }),
            _ => None,
        }
    }
}

/// `tt` and 1 to 10 digits: the only ids that go into a source's query.
fn is_imdb(id: &str) -> bool {
    id.strip_prefix("tt")
        .is_some_and(|d| (1..=10).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_digit()))
}

/// What a source answered.
enum Got {
    Body(Value),
    /// A real "not found" (404).
    Miss,
    /// Timed out, failed, refused (429), or not JSON: say nothing yet.
    Failed,
}

/// Where each database answers; tests point these at a local server.
#[derive(Debug, Clone)]
pub struct Hosts {
    pub theintrodb: String,
    pub introdb: String,
    pub aniskip: String,
    pub kitsu: String,
    /// The hosts are on this machine (tests).
    pub local: bool,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            theintrodb: "https://api.theintrodb.org".into(),
            introdb: "https://api.introdb.app".into(),
            aniskip: "https://api.aniskip.com".into(),
            kitsu: "https://kitsu.io".into(),
            local: false,
        }
    }
}

/// What the primary source found, and the Up Next time it supports.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub segments: Vec<Segment>,
    /// Earliest credits start from a source that matched the release.
    pub next_at_ms: Option<u64>,
}

#[derive(Default)]
struct Memory {
    found: HashMap<(Target, u64), Found>,
    mal: HashMap<u64, Option<u64>>,
    resting: HashMap<&'static str, Instant>,
}

/// Looks segments up and remembers them for the life of the core.
pub struct Finder {
    http: HttpClient,
    hosts: Mutex<Hosts>,
    memory: Mutex<Memory>,
    retries: Mutex<Vec<Duration>>,
}

impl Finder {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            hosts: Mutex::new(Hosts::default()),
            memory: Mutex::default(),
            retries: Mutex::new(RETRIES.to_vec()),
        }
    }

    pub fn set_hosts(&self, hosts: Hosts) {
        *lock(&self.hosts) = hosts;
        *lock(&self.memory) = Memory::default();
    }

    /// The waits before asking a source again that could not be asked; tests shorten them.
    pub fn retries(&self) -> Vec<Duration> {
        lock(&self.retries).clone()
    }

    pub fn set_retries(&self, waits: Vec<Duration>) {
        *lock(&self.retries) = waits;
    }

    /// The release-matched source: TheIntroDB for IMDb ids, AniSkip for
    /// Kitsu ids. A miss is empty; `None` when the source could not be asked
    /// (a timeout, a refusal), so the caller may ask again later.
    pub async fn primary(&self, target: &Target, duration_ms: u64) -> Option<Found> {
        // The exact duration: another file of the same length to the second
        // is not proof of the same cut.
        let key = (target.clone(), duration_ms);
        if let Some(found) = lock(&self.memory).found.get(&key) {
            return Some(found.clone());
        }
        let raw = match target {
            Target::Movie { .. } | Target::Episode { .. } => {
                self.theintrodb(target, duration_ms).await
            }
            Target::Kitsu { id, episode } => self.aniskip(*id, *episode, duration_ms).await,
        };
        // A failure is not remembered: it is asked again.
        let raw = raw?;
        let segments = normalize(raw, duration_ms);
        let next_at_ms = segments
            .iter()
            .filter(|s| s.kind == SegmentKind::Credits)
            .map(|s| s.start_ms)
            .min();
        let found = Found {
            segments,
            next_at_ms,
        };
        let mut memory = lock(&self.memory);
        if memory.found.len() >= CACHE_LOOKUPS {
            // ponytail: drop everything when full; an LRU if long sessions miss.
            memory.found.clear();
        }
        memory.found.insert(key, found.clone());
        Some(found)
    }

    /// IntroDB's segments of the kinds `have` lacks (IMDb ids only). They
    /// are not matched to the release, so they never set the Up Next time.
    pub async fn fill(&self, target: &Target, duration_ms: u64, have: &[Segment]) -> Vec<Segment> {
        let query = match target {
            Target::Movie { imdb } => format!("imdb_id={imdb}&is_movie=true"),
            Target::Episode {
                imdb,
                season,
                episode,
            } => format!("imdb_id={imdb}&season={season}&episode={episode}"),
            Target::Kitsu { .. } => return Vec::new(),
        };
        let host = lock(&self.hosts).introdb.clone();
        let Got::Body(body) = self
            .get("IntroDB", &format!("{host}/segments?{query}"))
            .await
        else {
            return Vec::new();
        };
        let raw = introdb_segments(&body)
            .into_iter()
            .filter(|r| !have.iter().any(|h| h.kind == r.kind))
            .collect();
        normalize(raw, duration_ms)
    }

    async fn theintrodb(&self, target: &Target, duration_ms: u64) -> Option<Vec<Raw>> {
        let query = match target {
            Target::Movie { imdb } => format!("imdb_id={imdb}"),
            Target::Episode {
                imdb,
                season,
                episode,
            } => format!("imdb_id={imdb}&season={season}&episode={episode}"),
            Target::Kitsu { .. } => return Some(Vec::new()),
        };
        let host = lock(&self.hosts).theintrodb.clone();
        let url = format!("{host}/v3/media?{query}&duration_ms={duration_ms}");
        match self.get("TheIntroDB", &url).await {
            Got::Body(body) => Some(theintrodb_segments(&body)),
            Got::Miss => Some(Vec::new()),
            Got::Failed => None,
        }
    }

    async fn aniskip(&self, kitsu: u64, episode: u32, duration_ms: u64) -> Option<Vec<Raw>> {
        let Some(mut mal) = self.mal_id(kitsu).await? else {
            return Some(Vec::new());
        };
        let host = lock(&self.hosts).aniskip.clone();
        // Split cours: a later part's episodes can live under another MAL id.
        let mut episode = episode;
        let mut seen = vec![mal];
        for _ in 0..3 {
            let url = format!("{host}/v2/relation-rules/{mal}");
            let rules = match self.get("AniSkip", &url).await {
                Got::Body(rules) => rules,
                Got::Miss => break,
                // Without the rules the episode may be the wrong one.
                Got::Failed => return None,
            };
            match relation_target(&rules, episode) {
                Some((next, next_episode)) if !seen.contains(&next) => {
                    seen.push(next);
                    (mal, episode) = (next, next_episode);
                }
                _ => break,
            }
        }
        let url = format!(
            "{host}/v2/skip-times/{mal}/{episode}?types=op&types=ed&types=recap&types=mixed-op&types=mixed-ed&episodeLength={}",
            duration_ms / 1000
        );
        match self.get("AniSkip", &url).await {
            Got::Body(body) => Some(aniskip_segments(&body, duration_ms)),
            Got::Miss => Some(Vec::new()),
            Got::Failed => None,
        }
    }

    /// The MAL id of a Kitsu entry: `Some(None)` when Kitsu has no single
    /// mapping, `None` when Kitsu could not be asked.
    async fn mal_id(&self, kitsu: u64) -> Option<Option<u64>> {
        if let Some(known) = lock(&self.memory).mal.get(&kitsu) {
            return Some(*known);
        }
        let host = lock(&self.hosts).kitsu.clone();
        let url = format!(
            "{host}/api/edge/anime/{kitsu}/mappings?filter%5BexternalSite%5D=myanimelist/anime&page%5Blimit%5D=20"
        );
        let mal = match self.get("Kitsu", &url).await {
            Got::Body(body) => kitsu_mal(&body),
            Got::Miss => None,
            Got::Failed => return None,
        };
        lock(&self.memory).mal.insert(kitsu, mal);
        Some(mal)
    }

    /// GETs JSON from a source. A 429 rests the source for its
    /// `Retry-After` (at most a day), else an hour.
    async fn get(&self, source: &'static str, url: &str) -> Got {
        if lock(&self.memory)
            .resting
            .get(source)
            .is_some_and(|until| Instant::now() < *until)
        {
            return Got::Failed;
        }
        let opts = RequestOptions {
            timeout: REQUEST_TIMEOUT,
            max_bytes: 512 * 1024,
            allow_private: lock(&self.hosts).local,
            ..Default::default()
        };
        match self.http.get(url, &opts).await {
            Ok(response) => response.json().map_or(Got::Failed, Got::Body),
            Err(HttpError::Status { status: 404, .. }) => Got::Miss,
            Err(HttpError::Status {
                status: 429,
                retry_after,
                ..
            }) => {
                let rest = retry_after.unwrap_or(REST).min(Duration::from_secs(86_400));
                if let Some(until) = Instant::now().checked_add(rest) {
                    lock(&self.memory).resting.insert(source, until);
                }
                Got::Failed
            }
            Err(e) => {
                tracing::debug!("{source} skip lookup failed: {e}");
                Got::Failed
            }
        }
    }
}

/// A segment as a source gave it, before the rules; times in milliseconds,
/// `None` for "start of file" and "end of file".
#[derive(Debug, Clone, PartialEq)]
struct Raw {
    kind: SegmentKind,
    start: Option<f64>,
    end: Option<f64>,
    source: &'static str,
}

fn theintrodb_segments(body: &Value) -> Vec<Raw> {
    let kinds = [
        ("intro", SegmentKind::Intro),
        ("recap", SegmentKind::Recap),
        ("credits", SegmentKind::Credits),
        ("preview", SegmentKind::Preview),
    ];
    kinds
        .iter()
        .flat_map(|(key, kind)| {
            body[key].as_array().into_iter().flatten().map(|item| Raw {
                kind: *kind,
                start: item["start_ms"].as_f64(),
                end: item["end_ms"].as_f64(),
                source: "TheIntroDB",
            })
        })
        .collect()
}

fn introdb_segments(body: &Value) -> Vec<Raw> {
    let kinds = [
        ("intro", SegmentKind::Intro),
        ("recap", SegmentKind::Recap),
        ("outro", SegmentKind::Credits),
    ];
    kinds
        .iter()
        .filter(|(key, _)| body[key].is_object())
        .map(|(key, kind)| Raw {
            kind: *kind,
            // Both ends are always given; a missing one is not "the edge".
            start: body[key]["start_ms"].as_f64().or(Some(f64::NAN)),
            end: body[key]["end_ms"].as_f64().or(Some(f64::NAN)),
            source: "IntroDB",
        })
        .collect()
}

/// Per kind, among the results that pass the interval rules: `op` before
/// `mixed-op` (`ed` before `mixed-ed`), then the episode length closest to
/// the file's, then AniSkip's own order (by votes).
fn aniskip_segments(body: &Value, duration_ms: u64) -> Vec<Raw> {
    let results = body["results"].as_array().cloned().unwrap_or_default();
    let duration = duration_ms as f64 / 1000.0;
    let kinds: [(&[&str], SegmentKind); 3] = [
        (&["op", "mixed-op"], SegmentKind::Intro),
        (&["recap"], SegmentKind::Recap),
        (&["ed", "mixed-ed"], SegmentKind::Credits),
    ];
    kinds
        .iter()
        .filter_map(|(types, kind)| {
            results
                .iter()
                .filter_map(|r| {
                    let rank = types.iter().position(|t| r["skipType"] == *t)?;
                    let seconds = |key: &str| r["interval"][key].as_f64().map(|s| s * 1000.0);
                    let raw = Raw {
                        kind: *kind,
                        start: seconds("startTime").or(Some(f64::NAN)),
                        end: seconds("endTime").or(Some(f64::NAN)),
                        source: "AniSkip",
                    };
                    check(&raw, duration_ms)?;
                    let off = (r["episodeLength"].as_f64().unwrap_or(0.0) - duration).abs();
                    Some((rank, off, raw))
                })
                .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
                .map(|(_, _, raw)| raw)
        })
        .collect()
}

fn kitsu_mal(body: &Value) -> Option<u64> {
    let mal: Vec<u64> = body["data"]
        .as_array()?
        .iter()
        .filter(|m| m["attributes"]["externalSite"] == "myanimelist/anime")
        .filter_map(|m| m["attributes"]["externalId"].as_str()?.parse().ok())
        .collect();
    // None or several: no lookup rather than a guess.
    match mal.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// Where AniSkip's relation rules move `episode` of this MAL entry.
fn relation_target(body: &Value, episode: u32) -> Option<(u64, u32)> {
    body["rules"].as_array()?.iter().find_map(|rule| {
        let number = |v: &Value| v.as_u64().and_then(|n| u32::try_from(n).ok());
        let from_start = number(&rule["from"]["start"])?;
        let from_end = rule["from"]["end"]
            .as_u64()
            .map_or(Some(u32::MAX), |e| u32::try_from(e).ok())?;
        if !(from_start..=from_end).contains(&episode) {
            return None;
        }
        let to_start = number(&rule["to"]["start"])?;
        Some((
            rule["to"]["malId"].as_u64()?,
            to_start.checked_add(episode - from_start)?,
        ))
    })
}

/// The interval rules of plans/m8-skip.md section 3.3 for one segment:
/// finite times, ends clamped to the file, a real length, and no stretch
/// longer than a third of the file (half for credits).
fn check(r: &Raw, duration_ms: u64) -> Option<Segment> {
    let duration = duration_ms as f64;
    let start = r.start.unwrap_or(0.0);
    // `min` would turn a NaN end into the duration; a missing end is no end.
    let end = match r.end {
        Some(end) if end.is_nan() => return None,
        Some(end) => end.min(duration),
        None => duration,
    };
    if !start.is_finite() || !end.is_finite() || start < 0.0 || start >= end {
        return None;
    }
    let limit = match r.kind {
        SegmentKind::Credits => duration / 2.0,
        _ => duration / 3.0,
    };
    (end - start <= limit).then(|| Segment {
        kind: r.kind,
        start_ms: start as u64,
        end_ms: end as u64,
        source: r.source.to_owned(),
    })
}

/// Segments that pass `check`, keeping the first of overlapping same-kind
/// ones, in start order.
fn normalize(raw: Vec<Raw>, duration_ms: u64) -> Vec<Segment> {
    let valid = raw.iter().filter_map(|r| check(r, duration_ms)).collect();
    merge(&[], valid)
}

/// Puts `extra` (a gap filler's segments) next to `have`, in start order.
pub fn merge(have: &[Segment], extra: Vec<Segment>) -> Vec<Segment> {
    let mut all: Vec<Segment> = have.to_vec();
    for s in extra {
        let overlaps = all
            .iter()
            .any(|h| h.kind == s.kind && h.start_ms < s.end_ms && s.start_ms < h.end_ms);
        if !overlaps {
            all.push(s);
        }
    }
    all.sort_by_key(|s| s.start_ms);
    all
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const HOUR: u64 = 3_600_000;

    fn seg(kind: SegmentKind, start: u64, end: u64, source: &str) -> Segment {
        Segment {
            kind,
            start_ms: start,
            end_ms: end,
            source: source.into(),
        }
    }

    #[test]
    fn targets_come_from_play_ids() {
        assert_eq!(
            Target::from_ids("series", "tt0903747", "tt0903747:1:2"),
            Some(Target::Episode {
                imdb: "tt0903747".into(),
                season: 1,
                episode: 2
            })
        );
        assert_eq!(
            Target::from_ids("movie", "tt1375666", "tt1375666"),
            Some(Target::Movie {
                imdb: "tt1375666".into()
            })
        );
        assert_eq!(
            Target::from_ids("series", "kitsu:46474", "kitsu:46474:1"),
            Some(Target::Kitsu {
                id: 46474,
                episode: 1
            })
        );
        // Specials, other id schemes and a series id without an episode.
        assert_eq!(Target::from_ids("series", "tt1", "tt1:0:3"), None);
        assert_eq!(Target::from_ids("series", "yt_id:x", "yt_id:x:1"), None);
        assert_eq!(Target::from_ids("series", "tt1", "tt1"), None);
    }

    #[test]
    fn theintrodb_open_ends_become_the_file_edges() {
        let body = json!({"tmdb_id":1396,"type":"tv",
            "intro":[{"start_ms":null,"end_ms":90060}],
            "credits":[{"start_ms":1460204,"end_ms":1550205},{"start_ms":1550294,"end_ms":null}],
            "preview":[{"start_ms":1550294,"end_ms":null}]});
        let got = normalize(theintrodb_segments(&body), 1_560_000);
        assert_eq!(
            got,
            vec![
                seg(SegmentKind::Intro, 0, 90060, "TheIntroDB"),
                seg(SegmentKind::Credits, 1460204, 1550205, "TheIntroDB"),
                seg(SegmentKind::Credits, 1550294, 1560000, "TheIntroDB"),
                seg(SegmentKind::Preview, 1550294, 1560000, "TheIntroDB"),
            ]
        );
    }

    #[test]
    fn introdb_outro_is_credits_and_post_credits_are_dropped() {
        let body = json!({"intro":null,"recap":null,
            "outro":{"start_ms":3431000,"end_ms":3500000,"confidence":1},
            "post_credits":{"start_ms":3500000,"end_ms":3510000}});
        assert_eq!(
            normalize(introdb_segments(&body), HOUR),
            vec![seg(SegmentKind::Credits, 3431000, 3500000, "IntroDB")]
        );
    }

    #[test]
    fn aniskip_skips_an_invalid_best_match_for_a_valid_one() {
        // The closest `op` ends before it starts; the other `op` is usable.
        let body = json!({"results":[
            {"interval":{"startTime":90.0,"endTime":10.0},"skipType":"op","episodeLength":1500.0},
            {"interval":{"startTime":5.0,"endTime":95.0},"skipType":"op","episodeLength":1400.0},
            {"interval":{"startTime":50.0,"endTime":140.0},"skipType":"mixed-op","episodeLength":1500.0}]});
        assert_eq!(
            normalize(aniskip_segments(&body, 1_500_000), 1_500_000),
            vec![seg(SegmentKind::Intro, 5000, 95000, "AniSkip")]
        );
    }

    #[test]
    fn aniskip_picks_the_closest_length_and_prefers_op() {
        let body = json!({"found":true,"results":[
            {"interval":{"startTime":58.7,"endTime":148.7},"skipType":"mixed-op","episodeLength":1470.0},
            {"interval":{"startTime":3.2,"endTime":93.2},"skipType":"op","episodeLength":1560.0},
            {"interval":{"startTime":1417.1,"endTime":1507.1},"skipType":"ed","episodeLength":1510.1},
            {"interval":{"startTime":1300.0,"endTime":1390.0},"skipType":"ed","episodeLength":1400.0}]});
        let got = normalize(aniskip_segments(&body, 1_512_000), 1_512_000);
        assert_eq!(
            got,
            vec![
                seg(SegmentKind::Intro, 3200, 93200, "AniSkip"),
                seg(SegmentKind::Credits, 1417100, 1507100, "AniSkip"),
            ]
        );
    }

    #[test]
    fn rules_drop_what_cannot_be_a_segment() {
        let raw = |kind, start: Option<f64>, end: Option<f64>| Raw {
            kind,
            start,
            end,
            source: "TheIntroDB",
        };
        let got = normalize(
            vec![
                raw(SegmentKind::Intro, Some(50_000.0), Some(40_000.0)),
                raw(SegmentKind::Intro, Some(f64::NAN), Some(40_000.0)),
                raw(SegmentKind::Recap, Some(-5.0), Some(40_000.0)),
                // A source that sends no end does not mean "until the end".
                raw(SegmentKind::Recap, Some(1_000.0), Some(f64::NAN)),
                raw(SegmentKind::Intro, Some(HOUR as f64 + 1.0), None),
                raw(SegmentKind::Intro, Some(0.0), Some(HOUR as f64 / 2.0)),
                raw(SegmentKind::Credits, Some(0.0), None),
                raw(SegmentKind::Intro, Some(10_000.0), Some(70_000.0)),
                raw(SegmentKind::Intro, Some(60_000.0), Some(90_000.0)),
                raw(SegmentKind::Credits, Some(3_300_000.0), Some(9_000_000.0)),
            ],
            HOUR,
        );
        assert_eq!(
            got,
            vec![
                seg(SegmentKind::Intro, 10_000, 70_000, "TheIntroDB"),
                seg(SegmentKind::Credits, 3_300_000, HOUR, "TheIntroDB"),
            ]
        );
    }

    #[test]
    fn a_filler_never_overrides_a_primary_segment() {
        let have = vec![seg(SegmentKind::Intro, 200_000, 246_000, "TheIntroDB")];
        let extra = vec![
            seg(SegmentKind::Intro, 210_000, 250_000, "IntroDB"),
            seg(SegmentKind::Credits, 3_431_000, 3_500_000, "IntroDB"),
        ];
        assert_eq!(
            merge(&have, extra),
            vec![
                seg(SegmentKind::Intro, 200_000, 246_000, "TheIntroDB"),
                seg(SegmentKind::Credits, 3_431_000, 3_500_000, "IntroDB"),
            ]
        );
    }

    #[test]
    fn kitsu_maps_to_one_mal_entry_or_none() {
        let one = json!({"data":[
            {"attributes":{"externalSite":"myanimelist/anime","externalId":"52991"}},
            {"attributes":{"externalSite":"anilist/anime","externalId":"154587"}}]});
        assert_eq!(kitsu_mal(&one), Some(52991));
        let two = json!({"data":[
            {"attributes":{"externalSite":"myanimelist/anime","externalId":"1"}},
            {"attributes":{"externalSite":"myanimelist/anime","externalId":"2"}}]});
        assert_eq!(kitsu_mal(&two), None);
    }

    #[test]
    fn relation_rules_move_a_later_cour() {
        let body = json!({"rules":[{"from":{"start":14,"end":26},"to":{"malId":53998,"start":1,"end":13}}]});
        assert_eq!(relation_target(&body, 14), Some((53998, 1)));
        assert_eq!(relation_target(&body, 20), Some((53998, 7)));
        assert_eq!(relation_target(&body, 3), None);
    }
}
