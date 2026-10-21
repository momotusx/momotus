//! Versions (stream picker): queries every stream addon for one video,
//! normalizes and ranks the union on each arrival, and publishes the pick
//! with its reasons, intent groups and hidden rows.
//!
//! The recommendation is stable once the user starts moving focus
//! (`freeze`), so late addon results never swap the row under the cursor.

use crate::Core;
use crate::learn::LearnedNote;
use crate::live::{FRAME, coalesce};
use crate::speed::Network;
use crate::views::addons_changed;
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::{AddonEvent, Deadlines, InstalledAddon};
use mq_library::{Policy, Table};
use mq_protocol::{ResourcePath, decode_streams};
use mq_streams::{
    Conditions, DeviceCaps, Group, HideReason, ParsedStream, RankInput, RankKey, Ranked, RankedRow,
    Reason, ReasonLine, SourceClass, Speed, StreamPrefs, Target, normalize, rank, screen_class,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;
use url::Url;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct VersionRow {
    pub key: String,
    /// "4K", "1440p", "1080p", "720p" or "SD"; when the release names no
    /// resolution, its source ("Blu-ray", "WEB-DL") or "Quality not stated".
    pub quality: String,
    /// Words, never emoji: "Dolby Vision", "HDR10", "Atmos", "5.1", "Remux".
    pub features: Vec<String>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size_bytes: Option<u64>,
    pub status: VersionStatus,
    /// Service ("Real-Debrid") or addon name.
    pub via: String,
    pub release_name: String,
    pub languages: Vec<String>,
    pub reasons: Vec<Reason>,
    pub group: Group,
    pub failover_count: u16,
    /// What the row tells the user, with its numbers (3.7).
    pub line: ReasonLine,
    /// How it starts (3.2).
    pub source_class: SourceClass,
    /// Size over runtime after the pack guard; null when either is unknown.
    pub bitrate_mbps: Option<f32>,
    /// The sort key, for a details panel and tests.
    pub score: RankKey,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VersionStatus {
    Ready { service: Option<String> },
    Torrent { seeds: Option<u32> },
    NeedsDownload { service: Option<String> },
    Direct,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct HiddenSummary {
    pub count: u32,
    pub by_reason: Vec<(HideReason, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct VersionsSnapshot {
    /// What the ranking read: the screen, the connection, the disk, the
    /// title and the preferences in effect (4.2).
    pub conditions: VersionsConditions,
    /// Every addon answered, nothing is listed, and the hidden rows say
    /// why (3.11).
    pub blocked: Option<BlockedVersions>,
    /// The one-time note when a pick taught Marquee something (3.9).
    pub note: Option<LearnedNote>,
    pub recommended: Option<VersionRow>,
    pub rows: Vec<VersionRow>,
    pub hidden: HiddenSummary,
    pub pending_addons: Vec<String>,
    pub slow_addons: Vec<String>,
    pub failed_addons: Vec<String>,
    pub done: bool,
    pub frozen: bool,
    /// Every listed version is a swarm the engine would fetch whose
    /// trackers count one seeder or none: almost nobody is sharing this
    /// title right now, so say so before Play. Its attempt keeps the stall
    /// windows and the full deadline, but a swarm where no peer unchoked by
    /// the end of the first window (30 s) is let go then.
    pub nobody_sharing: bool,
}

/// What the ranking read for this view, for the header "This TV: 4K HDR,
/// about 42 Mbit/s on Wi-Fi, 61 GB free".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct VersionsConditions {
    /// 2160, 1080, 720 or 480 (3.3).
    pub screen_class: u32,
    /// The screen shows HDR and the user wants it.
    pub hdr: bool,
    pub network: Network,
    pub http: Speed,
    pub p2p: Speed,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub free_bytes: Option<u64>,
    pub runtime_min: Option<u32>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size_limit_bytes: Option<u64>,
    pub min_seeders: u32,
    /// This build has the torrent engine.
    pub engine: bool,
    /// The profile has a streaming service account.
    pub account: bool,
}

/// Why nothing is listed (3.11), how many versions were found, and those
/// rows ranked as they would be once the reason is fixed ("Show them").
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct BlockedVersions {
    pub reason: BlockedReason,
    pub count: u32,
    pub rows: Vec<VersionRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum BlockedReason {
    /// Torrents this build opens only through a paid streaming service.
    NeedsAccount,
    /// Torrents larger than the free space.
    NoSpace,
    /// Nothing found plays on this device.
    NothingPlaysHere,
}

/// How long `done` waits for the meta and for each cache check (4.2,
/// r4 N4): the apps play at `done`.
const SIDE_WAIT: Duration = Duration::from_secs(3);

struct State {
    conditions: VersionsConditions,
    /// A playback of a pick failed in this view: no later pick teaches
    /// (an offer taken after a failed hand pick included).
    recommendation_failed: bool,
    /// What the rows rank with, for a rank again outside the view's task
    /// (a playback's failure, `PickHandle::failed`). None when denied.
    ranking: Option<Arc<Ranking>>,
    /// The recommendation was picked and has not started or failed yet:
    /// a pick now could still turn out to follow its failure (code review
    /// r1 P3), so it teaches nothing.
    recommendation_starting: bool,
    /// A pick already counted in this view (only the first does).
    learned: bool,
    note: Option<LearnedNote>,
    /// What the ranking reads beyond the device and the user; the meta and
    /// the cache checks fill it in.
    rank: Conditions,
    /// Cache answers by store and lowercase info hash, and those asked:
    /// with two accounts one store's answer never relabels the other's
    /// rows (code review r1 P3).
    cache: HashMap<CacheKey, bool>,
    asked: HashSet<CacheKey>,
    /// Live seeders by lowercase info hash, and the hashes asked: a swarm's
    /// count from its trackers stands over the addon's number.
    live: HashMap<String, u32>,
    scraped: HashSet<String>,
    blocked: Option<BlockedVersions>,
    streams: Vec<ParsedStream>,
    ranked: Ranked,
    recommended: Option<RankedRow>,
    frozen: bool,
    /// Installs by transport URL, with their names: two installs may share a
    /// manifest id, and each keeps its own progress.
    pending: Vec<(Url, String)>,
    slow: Vec<(Url, String)>,
    failed: Vec<(Url, String)>,
    done: bool,
    /// The pick's debrid link was warmed (once per view).
    prefetched: bool,
    /// Live seeder asks still on the way.
    counting: usize,
}

pub struct VersionsView {
    core: Core,
    /// The playlist formats the device plays, from its caps.
    plays: Vec<mq_server::Format>,
    episode: (Option<u32>, Option<u32>),
    snapshot: SharedObservable<VersionsSnapshot>,
    state: Arc<Mutex<State>>,
    /// A kids profile's checked target; playback ids come from it.
    verified: Option<Verified>,
    /// A kids policy refused the target: the view does no work.
    denied: bool,
    /// What counting a pick reads (3.9).
    learning: Box<Learning>,
    _tasks: JoinSet<()>,
}

/// What a pick needs to count: the screen class it teaches, the device
/// (for HDR), the title, and the profile's preferences before learning.
struct Learning {
    on: bool,
    screen_class: u32,
    caps: DeviceCaps,
    item_id: String,
    prefs: StreamPrefs,
}

/// A row the user picked: its stream and failover chain for playback, the
/// handle playback reports a failure through, and the learning note if
/// this pick crossed one.
pub struct Pick {
    pub candidates: Vec<ParsedStream>,
    pub handle: PickHandle,
    pub note: Option<LearnedNote>,
    /// The audio languages this view ranked with: the profile's, else the
    /// device's language, then English (the bridge fills them in). The
    /// playback orders subtitles with them too.
    pub languages: Vec<String>,
    /// The meta runtime the view ranked with: a torrent's window is sized
    /// with the same rate (plans/streaming-window.md 2.2).
    pub runtime_s: Option<u32>,
}

/// Playback reports through this that the picked row failed; when it was
/// the recommendation, no later pick in the view teaches anything (3.9).
#[derive(Clone)]
pub struct PickHandle {
    state: std::sync::Weak<Mutex<State>>,
    core: Core,
    snapshot: SharedObservable<VersionsSnapshot>,
    recommended: bool,
    /// The device the view ranked for.
    pub caps: DeviceCaps,
}

impl PickHandle {
    /// The source `key` of the pick failed. No later pick in this view
    /// teaches (3.9), and the view learns what the device recorded of the
    /// failure: its rows rank again, and a frozen pick that failed is let
    /// go (code review 2026-10-20 P2-1). Called outside the playback's lock.
    pub fn failed(&self, key: &str) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let mut s = lock(&state);
        s.recommendation_failed = true;
        s.recommendation_starting = false;
        relearn(&self.core, &mut s, Some(key));
        self.snapshot.set(render(&s));
    }

    /// The device recorded something since (a swarm proven dead once
    /// another source reached a peer): the rows rank again.
    pub fn relearn(&self) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let mut s = lock(&state);
        relearn(&self.core, &mut s, None);
        self.snapshot.set(render(&s));
    }

    /// The picked row started playing.
    pub fn ready(&self) {
        if let (true, Some(state)) = (self.recommended, self.state.upgrade()) {
            lock(&state).recommendation_starting = false;
        }
    }

    /// The pick was the recommendation.
    pub fn recommended(&self) -> bool {
        self.recommended
    }

    /// Almost nobody is sharing any version of the title right now (the
    /// view's `nobody_sharing`, read now). False once the view is gone.
    pub fn nobody_sharing(&self) -> bool {
        self.state
            .upgrade()
            .is_some_and(|state| nobody_sharing(&lock(&state)))
    }

    /// The version the ranking picks after the sources `tried` (stream
    /// keys): the first listed row that can start and holds none of them,
    /// with its failover chain. A row a folded pick played through is a
    /// tried row (code review 2026-10-20 r3 P3-6). None once the view is
    /// gone.
    pub fn next_version(&self, tried: &[String]) -> Option<NextVersion> {
        let state = self.state.upgrade()?;
        let s = lock(&state);
        let untried = |r: &&RankedRow| {
            !std::iter::once(&r.stream)
                .chain(&r.failover)
                .any(|st| tried.contains(&st.key))
        };
        let row = s
            .ranked
            .rows
            .iter()
            .filter(|r| r.key.viable)
            .find(untried)?;
        Some(NextVersion {
            key: row.stream.key.clone(),
            label: version_label(&row.stream.release),
            candidates: std::iter::once(&row.stream)
                .chain(&row.failover)
                .cloned()
                .collect(),
        })
    }
}

/// A version a playback can move on to.
pub struct NextVersion {
    pub key: String,
    pub label: String,
    pub candidates: Vec<ParsedStream>,
}

/// "1080p WEB", "4K Remux", "Blu-ray": the row's quality, then its source.
fn version_label(rel: &mq_relparse::ParsedRelease) -> String {
    use mq_relparse::Quality as Q;
    let quality = quality_word(rel.resolution.height(), rel.quality);
    let source = match rel.quality {
        Q::WebDl | Q::WebRip => "WEB",
        Q::BluRay => "Blu-ray",
        Q::Remux => "Remux",
        Q::Hdtv => "HDTV",
        Q::Dvd => "DVD",
        _ => return quality,
    };
    if rel.resolution.height() == 0 {
        return quality;
    }
    format!("{quality} {source}")
}

/// A kids profile's target as the list allows it (plans/m11-kids.md 4.3):
/// the title is on the list as this kind and the video is one of its own.
/// Only [`verify_play`] makes one.
#[derive(Debug, Clone, PartialEq)]
pub struct Verified {
    kind: String,
    item_id: String,
    video_id: String,
    /// The allowlist entry's name, then its other names.
    names: Vec<String>,
}

impl Verified {
    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    pub fn video_id(&self) -> &str {
        &self.video_id
    }

    /// The allowlist entry's name.
    pub fn name(&self) -> &str {
        &self.names[0]
    }

    /// Season and episode from the video id itself (`tt1:2:5`), never from
    /// what an app passed.
    fn episode(&self) -> (Option<u32>, Option<u32>) {
        let rest = self
            .video_id
            .strip_prefix(&self.item_id)
            .unwrap_or_default();
        let numbers: Vec<u32> = rest
            .split(':')
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect();
        match numbers[..] {
            [season, episode] => (Some(season), Some(episode)),
            [episode] => (None, Some(episode)),
            _ => (None, None),
        }
    }
}

/// Checks a kids profile's target against its list: `item_id` is given and
/// allowed as `kind`, and `video_id` belongs to it. `None` refuses.
pub fn verify_play(
    core: &Core,
    kind: &str,
    item_id: Option<&str>,
    video_id: &str,
) -> Option<Verified> {
    let item_id = item_id?;
    if !core.policy().allows(kind, item_id) || !Policy::belongs(item_id, video_id) {
        return None;
    }
    let entry = core
        .library()
        .allowed_list(core.profile())
        .ok()?
        .counted
        .into_iter()
        .find(|r| r.item_id == item_id && r.kind == kind)?;
    Some(Verified {
        kind: kind.to_owned(),
        item_id: item_id.to_owned(),
        video_id: video_id.to_owned(),
        names: std::iter::once(entry.name).chain(entry.names).collect(),
    })
}

pub struct VersionsParams {
    pub kind: String,
    pub video_id: String,
    pub target: Target,
    pub caps: DeviceCaps,
    pub prefs: StreamPrefs,
    pub last_binge_group: Option<String>,
    /// What the app knows of the title; the meta fills the rest (4.3).
    pub facts: TitleFacts,
}

/// The title's runtime and years and the season's size, as the app sends
/// them with `versions` (4.3).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TitleFacts {
    pub runtime_s: Option<u32>,
    pub title_year: Option<u16>,
    /// The episode's air date's year.
    pub episode_year: Option<u16>,
    pub episodes_in_season: Option<u32>,
}

impl TitleFacts {
    /// From the strings the apps and Cinemeta use: a runtime of "142 min"
    /// or "2h 22min", a release info of "2002", "2022-" or "2005–2013", and
    /// an air date of "2013-09-16".
    pub fn read(
        runtime: Option<&str>,
        release_info: Option<&str>,
        released: Option<&str>,
        episodes_in_season: Option<u32>,
    ) -> Self {
        Self {
            runtime_s: runtime.and_then(minutes).map(|m| m * 60),
            title_year: release_info.and_then(year),
            episode_year: released.and_then(year),
            episodes_in_season: episodes_in_season.filter(|n| *n > 0),
        }
    }
}

/// "142 min" is 142, "2h 22min" is 142, "1 h" is 60; a bare number is
/// minutes.
fn minutes(text: &str) -> Option<u32> {
    let mut total = 0;
    let mut found = false;
    let mut rest = text.trim();
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        let digits: String = rest[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        rest = &rest[start + digits.len()..];
        let n: u32 = digits.parse().ok()?;
        let hours = rest.trim_start().starts_with(['h', 'H']);
        total += if hours { n * 60 } else { n };
        found = true;
    }
    (found && total > 0).then_some(total)
}

/// The first year in `text`.
fn year(text: &str) -> Option<u16> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits
        .get(..4)?
        .parse()
        .ok()
        .filter(|y| (1870..=2200).contains(y))
}

impl VersionsView {
    /// `open_checked` without an item id: a kids policy refuses it.
    pub fn open(core: &Core, params: VersionsParams) -> Self {
        Self::open_checked(core, params, None)
    }

    /// Opens Versions for a video of `item_id`. A kids profile needs the
    /// item id: the target is verified against the list, and the ranking
    /// takes the allowlist entry's names and the video's own season and
    /// episode, not the app's. A refused target opens denied: no work, an
    /// empty snapshot, and `denied()` says so.
    pub fn open_checked(core: &Core, mut params: VersionsParams, item_id: Option<&str>) -> Self {
        let verified = if core.policy().kids() {
            let Some(verified) = verify_play(core, &params.kind, item_id, &params.video_id) else {
                return Self::denied_view(core, &params);
            };
            params.target.title = verified.name().to_owned();
            (params.target.season, params.target.episode) = verified.episode();
            Some(verified)
        } else {
            None
        };
        let screen = screen_class(params.caps.display_height);
        let learning = Box::new(Learning {
            on: params.prefs.learn && verified.is_none(),
            screen_class: screen,
            caps: params.caps.clone(),
            item_id: item_id
                .unwrap_or_else(|| params.video_id.split(':').next().unwrap_or_default())
                .to_owned(),
            prefs: params.prefs.clone(),
        });
        params.prefs = core.learned_prefs(params.prefs, screen);
        if params.last_binge_group.is_none() {
            let item_id =
                item_id.unwrap_or_else(|| params.video_id.split(':').next().unwrap_or_default());
            params.last_binge_group = core
                .library()
                .last_binge_group(core.profile(), item_id)
                .ok()
                .flatten();
        }
        let episode = (params.target.season, params.target.episode);
        let plays = plays(&params.caps);
        // Subscribe before the first read so no addon change slips in between.
        let changes = core.library().subscribe();
        let path = ResourcePath {
            resource: "stream".into(),
            r#type: params.kind.clone(),
            id: params.video_id.clone(),
            extra: vec![],
        };
        let eligible = eligible(core, &path);
        let facts = &params.facts;
        #[cfg(feature = "torrent")]
        let (window, (reclaimable_bytes, committed_bytes)) =
            (core.windows_ok(), core.room_counts());
        #[cfg(not(feature = "torrent"))]
        let (window, (reclaimable_bytes, committed_bytes)) = (false, (None, 0));
        let conditions = Conditions {
            runtime_s: facts.runtime_s,
            title_year: facts.title_year,
            episode_year: facts.episode_year,
            episodes_in_season: facts.episodes_in_season,
            http: core.speed(crate::speed::Kind::Http),
            p2p: core.speed(crate::speed::Kind::P2p),
            p2p_thin: core.speed(crate::speed::Kind::P2pThin),
            free_bytes: core.free_bytes(),
            engine: core.has_engine(),
            debrid: core.debrid().is_some(),
            dead_swarms: crate::speed::dead_swarms(core.library(), mq_library::hlc::now_ms()),
            limit_chosen: core.size_limit_chosen(),
            no_picture: crate::speed::no_picture(core.library()),
            did_not_play: crate::speed::did_not_play(core.library(), mq_library::hlc::now_ms()),
            wrong_file: crate::speed::wrong_file(core.library(), mq_library::hlc::now_ms()),
            containers: crate::speed::containers(core.library()),
            window,
            reclaimable_bytes,
            committed_bytes,
        };
        let item_id = item_id.map_or_else(
            || {
                params
                    .video_id
                    .split(':')
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            },
            str::to_owned,
        );
        let state = Arc::new(Mutex::new(State {
            conditions: view_conditions(core, &params, &conditions),
            recommendation_failed: false,
            recommendation_starting: false,
            learned: false,
            note: None,
            rank: conditions,
            cache: HashMap::new(),
            asked: HashSet::new(),
            live: HashMap::new(),
            scraped: HashSet::new(),
            blocked: None,
            streams: Vec::new(),
            ranked: Ranked::default(),
            recommended: None,
            frozen: false,
            pending: eligible
                .iter()
                .map(|a| (a.transport_url.clone(), a.manifest.name.clone()))
                .collect(),
            slow: Vec::new(),
            failed: Vec::new(),
            done: eligible.is_empty(),
            prefetched: false,
            counting: 0,
            ranking: None,
        }));
        let snapshot = SharedObservable::new(render(&lock(&state)));
        let mut tasks = JoinSet::new();
        let ranking = Arc::new(Ranking {
            kids: verified.as_ref().map(|v| v.names.clone()),
            params,
            item_id,
        });
        lock(&state).ranking = Some(ranking.clone());
        tasks.spawn(supervise(
            core.clone(),
            changes,
            eligible,
            path,
            ranking,
            state.clone(),
            snapshot.clone(),
        ));
        Self {
            core: core.clone(),
            plays,
            episode,
            snapshot,
            state,
            verified,
            denied: false,
            learning,
            _tasks: tasks,
        }
    }

    fn denied_view(core: &Core, params: &VersionsParams) -> Self {
        let state = State {
            conditions: view_conditions(core, params, &Conditions::default()),
            recommendation_failed: false,
            recommendation_starting: false,
            learned: false,
            note: None,
            rank: Conditions::default(),
            cache: HashMap::new(),
            asked: HashSet::new(),
            live: HashMap::new(),
            scraped: HashSet::new(),
            blocked: None,
            streams: Vec::new(),
            ranked: Ranked::default(),
            recommended: None,
            frozen: false,
            pending: Vec::new(),
            slow: Vec::new(),
            failed: Vec::new(),
            done: false,
            prefetched: true,
            counting: 0,
            ranking: None,
        };
        Self {
            core: core.clone(),
            plays: plays(&params.caps),
            episode: (None, None),
            snapshot: SharedObservable::new(render(&state)),
            state: Arc::new(Mutex::new(state)),
            verified: None,
            denied: true,
            learning: Box::new(Learning {
                on: false,
                screen_class: screen_class(params.caps.display_height),
                caps: params.caps.clone(),
                item_id: String::new(),
                prefs: params.prefs.clone(),
            }),
            _tasks: JoinSet::new(),
        }
    }

    /// A kids policy refused the target at open.
    pub fn denied(&self) -> bool {
        self.denied
    }

    /// A kids profile's verified target, for its playback.
    pub fn verified(&self) -> Option<&Verified> {
        self.verified.as_ref()
    }

    pub fn snapshot(&self) -> VersionsSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        VersionsSnapshot,
        Pin<Box<dyn Stream<Item = Vec<VersionsSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }

    /// Keeps the current recommendation from now on (the user moved focus).
    /// Before there is one, it does nothing: the first answer still picks.
    pub fn freeze(&self) {
        let mut state = lock(&self.state);
        if state.recommended.is_none() {
            return;
        }
        state.frozen = true;
        maybe_prefetch(&self.core, &mut state, self.episode);
        self.snapshot.set(render(&state));
    }

    /// Live seeder counts are still on the way: the rows may rank again
    /// when they land.
    pub fn counting(&self) -> bool {
        lock(&self.state).counting > 0
    }

    /// The playlist formats the device plays, for `PlaybackView::open_for`.
    pub fn plays(&self) -> Vec<mq_server::Format> {
        self.plays.clone()
    }

    /// The stream for row `key` followed by its failover chain, for playback.
    pub fn candidates(&self, key: &str) -> Vec<ParsedStream> {
        let s = lock(&self.state);
        // The frozen pick plays its own source first, wherever a later
        // answer folded it (code review 2026-10-20 P2-3).
        let row = s
            .recommended
            .clone()
            .filter(|r| r.stream.key == key)
            .or_else(|| holding(&s.ranked.rows, |st| st.key == key));
        row.map(|r| {
            std::iter::once(&r.stream)
                .chain(&r.failover)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
    }

    /// The user picked row `key` to play. A pick counts toward learning
    /// (3.9) when the view is frozen, it is not the recommendation nor the
    /// stream last played for the title, no playback of the recommendation
    /// failed here, learning is on, the profile is not a kids one, and no
    /// pick counted in this view yet.
    pub fn pick(&self, key: &str) -> Pick {
        let candidates = self.candidates(key);
        let mut s = lock(&self.state);
        let recommended = s.recommended.as_ref().is_some_and(|r| r.stream.key == key);
        let handle = PickHandle {
            state: Arc::downgrade(&self.state),
            core: self.core.clone(),
            snapshot: self.snapshot.clone(),
            recommended,
            caps: self.learning.caps.clone(),
        };
        if recommended {
            s.recommendation_starting = true;
        }
        let l = &self.learning;
        let counts = l.on
            && s.frozen
            && !recommended
            && !s.recommendation_failed
            && !s.recommendation_starting
            && !s.learned;
        let chose = s.ranked.rows.iter().find(|r| r.stream.key == key);
        let taught = match (counts, chose, &s.recommended) {
            (true, Some(chose), Some(over)) if !self.replays(key) => {
                crate::learn::choice(chose, over, &l.caps)
            }
            _ => None,
        };
        let mut note = None;
        if let Some((choice, delta)) = taught {
            s.learned = true;
            note = self
                .core
                .learn_pick(l.screen_class, choice, delta, l.prefs.clone());
            if note.is_some() {
                s.note = note.clone();
                self.snapshot.set(render(&s));
            }
        }
        Pick {
            candidates,
            handle,
            note,
            languages: self.learning.prefs.languages.clone(),
            runtime_s: s.rank.runtime_s,
        }
    }

    /// `key` is the stream last played for the title: a replay teaches
    /// nothing.
    fn replays(&self, key: &str) -> bool {
        let remembered = self
            .core
            .library()
            .stream_memory(self.core.profile(), &self.learning.item_id)
            .ok()
            .flatten();
        remembered
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .is_some_and(|m| m["key"] == key)
    }

    /// The row the addons listed first (addon order, then their own): the
    /// live race's other arm (plans/m13-stream-choice.md 6.4).
    pub fn first_listed(&self) -> Option<String> {
        let s = lock(&self.state);
        s.ranked
            .rows
            .iter()
            .min_by_key(|r| (r.stream.addon_rank, r.stream.index))
            .map(|r| r.stream.key.clone())
    }

    pub fn recommended(&self) -> Option<RankedRow> {
        lock(&self.state).recommended.clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn eligible(core: &Core, path: &ResourcePath) -> Vec<InstalledAddon> {
    let addons = core.installed_addons().unwrap_or_default();
    addons.into_iter().filter(|a| a.supports(path)).collect()
}

/// Asks the installed stream addons, and asks again from the start whenever
/// they change: a source installed while a title is open shows up without
/// reopening it.
async fn supervise(
    core: Core,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    mut addons: Vec<InstalledAddon>,
    path: ResourcePath,
    ranking: Arc<Ranking>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    loop {
        let current = urls(&addons);
        // Dropping an unfinished run stops its writes at once.
        let outcome = tokio::select! {
            () = collect(core.clone(), addons, path.clone(), ranking.clone(), state.clone(), snapshot.clone()) => None,
            next = changed_set(&mut changes, &core, &path, &current) => Some(next),
        };
        let next = match outcome {
            Some(next) => next,
            None => changed_set(&mut changes, &core, &path, &current).await,
        };
        let Some(next) = next else { return };
        addons = next;
        let mut s = lock(&state);
        restart(&mut s, &addons);
        snapshot.set(render(&s));
    }
}

fn urls(addons: &[InstalledAddon]) -> Vec<Url> {
    addons.iter().map(|a| a.transport_url.clone()).collect()
}

/// Waits until the stream addons themselves change (which ones, or their
/// order). A write that leaves them as they were, such as a sync or another
/// addon's switch, keeps the rows the user is moving through. `None` once
/// the library closes.
async fn changed_set(
    changes: &mut tokio::sync::broadcast::Receiver<Vec<Table>>,
    core: &Core,
    path: &ResourcePath,
    current: &[Url],
) -> Option<Vec<InstalledAddon>> {
    loop {
        if !addons_changed(changes).await {
            return None;
        }
        let next = eligible(core, path);
        if urls(&next) != current {
            return Some(next);
        }
    }
}

/// A fresh run over `addons`. A frozen pick stays while its addon is still
/// installed; otherwise the next answers pick again.
fn restart(s: &mut State, addons: &[InstalledAddon]) {
    let from_installed = s
        .recommended
        .as_ref()
        .and_then(|r| r.stream.addon_url.as_ref())
        .is_some_and(|url| addons.iter().any(|a| a.transport_url == *url));
    if !(s.frozen && from_installed) {
        s.frozen = false;
        s.recommended = None;
    }
    s.streams.clear();
    s.ranked = Ranked::default();
    s.pending = addons
        .iter()
        .map(|a| (a.transport_url.clone(), a.manifest.name.clone()))
        .collect();
    s.slow.clear();
    s.failed.clear();
    s.blocked = None;
    s.done = addons.is_empty();
}

async fn collect(
    core: Core,
    addons: Vec<InstalledAddon>,
    path: ResourcePath,
    ranking: Arc<Ranking>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    let params = &ranking.params;
    // Name and rank by transport URL: two installs may share a manifest id.
    let names: std::collections::HashMap<Url, (String, u16)> = addons
        .iter()
        .enumerate()
        .map(|(i, a)| (a.transport_url.clone(), (a.manifest.name.clone(), i as u16)))
        .collect();
    let name_of = |url: &Url, id: &str| {
        let name = names
            .get(url)
            .map_or_else(|| id.to_owned(), |(n, _)| n.clone());
        (url.clone(), name)
    };
    let mut fanout = core.addons().fan_out(&addons, &path, Deadlines::default());
    let mut next_index = 0u32;
    // The meta and the cache checks: `done` waits for them (or 3 s each).
    let mut side: JoinSet<Side> = JoinSet::new();
    // Live seeders: `done` never waits for them (a scrape takes its whole
    // second); they rank the rows again when they come, until the view is
    // frozen.
    let mut live: JoinSet<Side> = JoinSet::new();
    if needs_meta(params, &lock(&state).rank) {
        side.spawn(meta_facts(
            core.clone(),
            params.kind.clone(),
            ranking.item_id.clone(),
            params.video_id.clone(),
            params.target.season,
        ));
    }
    let mut answered = addons.is_empty();
    loop {
        let event = tokio::select! {
            event = fanout.next(), if !answered => match event {
                Some(event) => event,
                None => {
                    answered = true;
                    AddonEvent::Done
                }
            },
            Some(joined) = side.join_next() => {
                let mut s = lock(&state);
                if let Ok(found) = joined {
                    found.apply(&mut s);
                }
                settle(&core, &mut s, &ranking, answered && side.is_empty());
                snapshot.set(render(&s));
                continue;
            }
            Some(joined) = live.join_next() => {
                let mut s = lock(&state);
                s.counting = s.counting.saturating_sub(1);
                if let Ok(found) = joined {
                    found.apply(&mut s);
                }
                settle(&core, &mut s, &ranking, answered && side.is_empty());
                snapshot.set(render(&s));
                continue;
            }
            else => break,
        };
        // Parsing is CPU work: keep it off the async threads that feed UIs.
        let parsed = match &event {
            AddonEvent::Arrived {
                addon_id,
                transport_url,
                body,
                ..
            } => {
                let (name, rank_index) = names
                    .get(transport_url)
                    .cloned()
                    .unwrap_or((addon_id.clone(), u16::MAX));
                let (addon_id, body, start) = (addon_id.clone(), body.clone(), next_index);
                let addon_url = transport_url.clone();
                let parsed = tokio::task::spawn_blocking(move || {
                    decode_streams(&body)
                        .map(|r| {
                            r.streams
                                .iter()
                                .enumerate()
                                .map(|(i, w)| {
                                    let mut stream = normalize(
                                        w,
                                        &addon_id,
                                        &name,
                                        rank_index,
                                        start + i as u32,
                                    );
                                    stream.addon_url = Some(addon_url.clone());
                                    stream
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                next_index += parsed.len() as u32;
                parsed
            }
            _ => Vec::new(),
        };
        let mut s = lock(&state);
        match event {
            AddonEvent::Arrived { transport_url, .. } => {
                for (store, hashes) in cache_asks(&core, &parsed, &mut s.asked) {
                    side.spawn(cache_check(core.clone(), store, hashes));
                }
                // Asked as each addon answers, alongside the others still
                // on the way.
                let swarms = live_asks(&core, &parsed, &mut s.scraped);
                if !swarms.is_empty() {
                    live.spawn(live_check(core.clone(), swarms));
                    s.counting += 1;
                }
                s.streams.extend(parsed);
                s.pending.retain(|(url, _)| *url != transport_url);
                s.slow.retain(|(url, _)| *url != transport_url);
            }
            AddonEvent::Slow {
                addon_ids,
                transport_urls,
            } => {
                s.slow = transport_urls
                    .iter()
                    .zip(&addon_ids)
                    .map(|(url, id)| name_of(url, id))
                    .collect();
            }
            AddonEvent::Failed {
                addon_id,
                transport_url,
                ..
            } => {
                s.pending.retain(|(url, _)| *url != transport_url);
                s.slow.retain(|(url, _)| *url != transport_url);
                s.failed.push(name_of(&transport_url, &addon_id));
            }
            AddonEvent::Done => answered = true,
        }
        settle(&core, &mut s, &ranking, answered && side.is_empty());
        snapshot.set(render(&s));
    }
}

/// Ranks again with what is known now; once `done`, warms the pick's
/// link and says why nothing is listed when nothing is.
fn settle(core: &Core, s: &mut State, ranking: &Ranking, done: bool) {
    s.done = done;
    let stores = core.debrid().map(|r| r.stores()).unwrap_or_default();
    apply_cache(&mut s.streams, &s.cache, &stores);
    apply_live(&mut s.streams, &s.live);
    rerank(s, ranking);
    let target = &ranking.params.target;
    if s.done {
        maybe_prefetch(core, s, (target.season, target.episode));
    }
    s.blocked = blocked(s, ranking);
}

/// A cache answer's store (`None`: the default) and lowercase info hash.
type CacheKey = (Option<mq_debrid::StoreId>, String);

/// What a side task found.
enum Side {
    Meta(TitleFacts),
    Cache(Vec<(CacheKey, bool)>),
    Live(Vec<(String, u32)>),
}

impl Side {
    fn apply(self, s: &mut State) {
        match self {
            Side::Meta(f) => {
                let c = &mut s.rank;
                c.runtime_s = c.runtime_s.or(f.runtime_s);
                c.title_year = c.title_year.or(f.title_year);
                c.episode_year = c.episode_year.or(f.episode_year);
                c.episodes_in_season = c.episodes_in_season.or(f.episodes_in_season);
                s.conditions.runtime_min = c.runtime_s.map(|r| r / 60);
            }
            Side::Cache(found) => s.cache.extend(found),
            Side::Live(found) => s.live.extend(found),
        }
    }
}

/// The app sent no runtime or year, or for an episode no air date or
/// season size: the meta has them.
fn needs_meta(params: &VersionsParams, c: &Conditions) -> bool {
    let episode = params.target.episode.is_some();
    c.runtime_s.is_none()
        || c.title_year.is_none()
        || (episode && (c.episode_year.is_none() || c.episodes_in_season.is_none()))
}

/// The title's runtime, year, the episode's air date and the season's
/// episode count from the first meta addon that answers, within 3 s.
async fn meta_facts(
    core: Core,
    kind: String,
    item_id: String,
    video_id: String,
    season: Option<u32>,
) -> Side {
    let Ok(Ok((meta, _))) = tokio::time::timeout(SIDE_WAIT, core.fetch_meta(&kind, &item_id)).await
    else {
        return Side::Meta(TitleFacts::default());
    };
    let p = &meta.preview;
    let video = meta.videos.iter().find(|v| v.id == video_id);
    let season = video
        .and_then(|v| v.series_info.as_ref())
        .map(|i| i.season)
        .or(season);
    let episodes = season.map(|n| {
        meta.videos
            .iter()
            .filter(|v| v.series_info.as_ref().is_some_and(|i| i.season == n))
            .count() as u32
    });
    let released = video
        .and_then(|v| v.released)
        .map(|d| d.date_naive().to_string());
    Side::Meta(TitleFacts::read(
        p.runtime.as_deref(),
        p.release_info.as_deref(),
        released.as_deref(),
        episodes,
    ))
}

/// A stream's info hash, lowercase, when a streaming service could have it.
fn info_hash(s: &ParsedStream) -> Option<String> {
    match &s.source {
        mq_protocol::Source::Torrent { info_hash, .. } => Some(info_hash.to_ascii_lowercase()),
        mq_protocol::Source::ClientResolve(r) => {
            r.info_hash.as_ref().map(|h| h.to_ascii_lowercase())
        }
        _ => None,
    }
}

/// The hashes of `streams` no one answered or asked about yet, by the
/// store each resolves on (the one its service names when that account is
/// here, else the default): one check per store and arrival.
fn cache_asks(
    core: &Core,
    streams: &[ParsedStream],
    asked: &mut HashSet<CacheKey>,
) -> Vec<(Option<mq_debrid::StoreId>, Vec<String>)> {
    let Some(resolver) = core.debrid() else {
        return Vec::new();
    };
    let stores = resolver.stores();
    let mut asks: Vec<(Option<mq_debrid::StoreId>, Vec<String>)> = Vec::new();
    for s in streams.iter().filter(|s| s.cached.is_none()) {
        let Some(hash) = info_hash(s) else {
            continue;
        };
        let store = store_of(s, &stores);
        if !asked.insert((store, hash.clone())) {
            continue;
        }
        match asks.iter_mut().find(|(st, _)| *st == store) {
            Some((_, hashes)) => hashes.push(hash),
            None => asks.push((store, vec![hash])),
        }
    }
    asks
}

/// How long one scrape may take, the engine's first start included; the
/// counts never hold `done`.
const LIVE_WAIT: Duration = Duration::from_secs(10);
/// The pause before asking again when no tracker answered at all.
const LIVE_RETRY: Duration = Duration::from_secs(1);

/// Swarms asked at most per answer: one UDP scrape packet.
// ponytail: an addon's first 70 swarms, in its order; rank first and ask
// the top ones if an addon lists more and the live ones sit past 70.
const LIVE_MAX: usize = 70;

/// The swarms of `streams` the engine would fetch (no service has them
/// ready) and no one asked about yet, with their trackers.
fn live_asks(
    core: &Core,
    streams: &[ParsedStream],
    scraped: &mut HashSet<String>,
) -> Vec<(String, Vec<String>)> {
    if !core.counts_swarms() {
        return Vec::new();
    }
    let mut asks = Vec::new();
    for s in streams.iter().filter(|s| s.cached != Some(true)) {
        let Some(hash) = info_hash(s) else {
            continue;
        };
        if asks.len() == LIVE_MAX || !scraped.insert(hash.clone()) {
            continue;
        }
        let trackers = match &s.source {
            mq_protocol::Source::Torrent { trackers, .. } => trackers.clone(),
            _ => Vec::new(),
        };
        asks.push((hash, trackers));
    }
    asks
}

/// One scrape of the trackers: an ask takes at most 10 s, and one that
/// answers nothing at all is asked again a second later, so counts can come
/// up to about 21 s after an answer; they never hold `done`. A swarm no
/// answer told anything about keeps the addon's number: the engine gives no
/// count for a default tracker's 0/0, the answer for a swarm it does not
/// track, and gives a zero that a tracker counting leechers, or the
/// stream's own tracker, reported (`Engine::scrape`).
async fn live_check(core: Core, swarms: Vec<(String, Vec<String>)>) -> Side {
    let ask = || async {
        let counts = tokio::time::timeout(LIVE_WAIT, core.live_seeders(&swarms)).await;
        counts.unwrap_or_else(|_| vec![None; swarms.len()])
    };
    // A cold first scrape (the engine starting, tracker names not resolved
    // yet) can come back with no answer at all: ask once more a moment
    // later (Android walk EW-1, the first open after an install).
    let mut counts = ask().await;
    if counts.iter().all(Option::is_none) {
        tokio::time::sleep(LIVE_RETRY).await;
        counts = ask().await;
    }
    Side::Live(
        swarms
            .into_iter()
            .zip(counts)
            .filter_map(|((hash, _), n)| Some((hash, n?)))
            .collect(),
    )
}

/// Live seeders over the addons' numbers, on every row of the swarm.
fn apply_live(streams: &mut [ParsedStream], live: &HashMap<String, u32>) {
    if live.is_empty() {
        return;
    }
    for stream in streams.iter_mut() {
        if let Some(n) = info_hash(stream).and_then(|h| live.get(&h)) {
            stream.seeders = Some(*n);
        }
    }
}

/// One cache check, within 3 s; a store without a reliable check, an error
/// or no answer in time leaves the rows as they were.
async fn cache_check(core: Core, store: Option<mq_debrid::StoreId>, hashes: Vec<String>) -> Side {
    use mq_debrid::CacheStatus;
    let Some(resolver) = core.debrid() else {
        return Side::Cache(Vec::new());
    };
    let answers = tokio::time::timeout(SIDE_WAIT, resolver.check(store, &hashes)).await;
    let Ok(Ok(answers)) = answers else {
        return Side::Cache(Vec::new());
    };
    Side::Cache(
        hashes
            .into_iter()
            .zip(answers)
            .filter_map(|(h, a)| match a {
                CacheStatus::Cached => Some(((store, h), true)),
                CacheStatus::NotCached => Some(((store, h), false)),
                CacheStatus::Unknown => None,
            })
            .collect(),
    )
}

/// The store a stream resolves on: the one its service names when that
/// account is here, else the default (`None`).
fn store_of(s: &ParsedStream, stores: &[mq_debrid::StoreId]) -> Option<mq_debrid::StoreId> {
    s.service
        .as_deref()
        .and_then(mq_debrid::StoreId::from_service)
        .filter(|id| stores.contains(id))
}

/// Our cache answers on the rows no addon said anything about, each from
/// the store it resolves on.
fn apply_cache(
    streams: &mut [ParsedStream],
    cache: &HashMap<CacheKey, bool>,
    stores: &[mq_debrid::StoreId],
) {
    if cache.is_empty() {
        return;
    }
    for stream in streams.iter_mut().filter(|st| st.cached.is_none()) {
        let key = info_hash(stream).map(|h| (store_of(stream, stores), h));
        if let Some(cached) = key.and_then(|k| cache.get(&k)) {
            stream.cached = Some(*cached);
        }
    }
}

/// Every addon answered, nothing is listed, and the hidden rows say why
/// (3.11): the reason, how many, and those rows ranked as they would be
/// once it is fixed. An account comes first (it opens the most), then free
/// space, then a device that plays nothing found.
fn blocked(s: &State, ranking: &Ranking) -> Option<BlockedVersions> {
    if !s.done || !s.ranked.rows.is_empty() {
        return None;
    }
    let of = |reason| -> Vec<ParsedStream> {
        s.ranked
            .hidden
            .iter()
            .filter(|h| h.reason == reason)
            .map(|h| h.stream.clone())
            .collect()
    };
    let params = &ranking.params;
    let mut caps = params.caps.clone();
    let mut c = s.rank.clone();
    let (reason, streams) = [
        (BlockedReason::NeedsAccount, of(HideReason::NeedsDebrid)),
        (BlockedReason::NoSpace, of(HideReason::NoSpace)),
        (BlockedReason::NothingPlaysHere, of(HideReason::Unplayable)),
    ]
    .into_iter()
    .find(|(_, streams)| !streams.is_empty())?;
    match reason {
        BlockedReason::NeedsAccount => c.debrid = true,
        BlockedReason::NoSpace => c.free_bytes = None,
        BlockedReason::NothingPlaysHere => caps.has_fallback_engine = true,
    }
    let count = streams.len() as u32;
    let input = RankInput {
        caps: &caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
        kids: ranking.kids.as_deref(),
        conditions: &c,
    };
    let rows = rank(streams, &input).rows.iter().map(row_dto).collect();
    Some(BlockedVersions {
        reason,
        count,
        rows,
    })
}

/// Once the pick settles (every addon answered, or the user moved focus),
/// warm its debrid link so Play starts from the cache.
fn maybe_prefetch(core: &Core, s: &mut State, (season, episode): (Option<u32>, Option<u32>)) {
    if s.prefetched {
        return;
    }
    if let Some(best) = &s.recommended {
        crate::views::playback::prefetch(core, &best.stream, season, episode);
        s.prefetched = true;
    }
}

/// The manifest formats caps list as containers: `m3u8` for HLS, `mpd`
/// for DASH.
fn plays(caps: &DeviceCaps) -> Vec<mq_server::Format> {
    let lists = |name: &str| caps.containers.iter().any(|c| c.eq_ignore_ascii_case(name));
    [
        ("m3u8", mq_server::Format::Hls),
        ("mpd", mq_server::Format::Dash),
    ]
    .into_iter()
    .filter(|(name, _)| lists(name))
    .map(|(_, format)| format)
    .collect()
}

/// What the ranking reads: the view's params, for a kids profile the
/// allowed title's names (plans/m11-kids.md 4.4), and the title's id for
/// its meta.
struct Ranking {
    params: VersionsParams,
    kids: Option<Vec<String>>,
    item_id: String,
}

fn rerank(s: &mut State, ranking: &Ranking) {
    let params = &ranking.params;
    let input = RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
        kids: ranking.kids.as_deref(),
        conditions: &s.rank,
    };
    s.ranked = rank(s.streams.clone(), &input);
    if !s.frozen {
        s.recommended = s.ranked.best().cloned();
    } else if let Some(pick) = &s.recommended {
        // Frozen: the pick stays, its facts (live seeders, line) follow,
        // as long as its row still leads with the same source.
        let same = |r: &&RankedRow| same_source(&r.stream, &pick.stream);
        if let Some(row) = s.ranked.rows.iter().find(same) {
            s.recommended = Some(row.clone());
        } else if let Some(row) = holding(&s.ranked.rows, |st| same_source(st, &pick.stream)) {
            // A later answer folded the pick into another row: it follows
            // that row, still led by its own source.
            s.recommended = Some(row);
        }
    }
}

/// The same source: one key from one install and one service.
fn same_source(a: &ParsedStream, b: &ParsedStream) -> bool {
    (&a.key, &a.addon_url, &a.service) == (&b.key, &b.addon_url, &b.service)
}

/// The row holding a source `is` matches, led by that source: a row's own
/// head, or a source folded into its failover chain.
fn holding(rows: &[RankedRow], is: impl Fn(&ParsedStream) -> bool) -> Option<RankedRow> {
    rows.iter().find_map(|r| {
        if is(&r.stream) {
            return Some(r.clone());
        }
        let at = r.failover.iter().position(&is)?;
        let mut led = r.clone();
        let source = led.failover.remove(at);
        let head = std::mem::replace(&mut led.stream, source);
        led.failover.insert(0, head);
        Some(led)
    })
}

/// What the device recorded since the view opened (a swarm that stalled,
/// a file with no picture, a version its player could not decode) is read
/// again and the rows rank again; a frozen pick that just failed is let
/// go, so the next one can lead (code review 2026-10-20 P2-1).
fn relearn(core: &Core, s: &mut State, failed: Option<&str>) {
    let (lib, now) = (core.library(), mq_library::hlc::now_ms());
    s.rank.dead_swarms = crate::speed::dead_swarms(lib, now);
    s.rank.no_picture = crate::speed::no_picture(lib);
    s.rank.did_not_play = crate::speed::did_not_play(lib, now);
    s.rank.wrong_file = crate::speed::wrong_file(lib, now);
    s.rank.containers = crate::speed::containers(lib);
    let pick_failed = s
        .recommended
        .as_ref()
        .zip(failed)
        .is_some_and(|(r, key)| r.stream.key == key);
    if pick_failed {
        s.frozen = false;
    }
    if let Some(ranking) = s.ranking.clone() {
        rerank(s, &ranking);
    }
}

/// What the snapshot says the ranking read (4.2).
fn view_conditions(core: &Core, params: &VersionsParams, c: &Conditions) -> VersionsConditions {
    let prefs = &params.prefs;
    VersionsConditions {
        screen_class: screen_class(params.caps.display_height),
        hdr: prefs.hdr && !params.caps.display_hdr.is_empty(),
        network: core.network(),
        http: c.http,
        p2p: c.p2p,
        free_bytes: c.free_bytes,
        runtime_min: c.runtime_s.map(|s| s / 60),
        size_limit_bytes: prefs.size_limit_bytes,
        min_seeders: prefs.min_seeders,
        engine: core.has_engine(),
        account: core.debrid().is_some(),
    }
}

fn names(installs: &[(Url, String)]) -> Vec<String> {
    installs.iter().map(|(_, name)| name.clone()).collect()
}

fn render(s: &State) -> VersionsSnapshot {
    let recommended_key = s.recommended.as_ref().map(|r| r.stream.key.clone());
    // A row whose chain holds the pick is the pick's own row, folded or
    // not: shown once, as the pick (code review 2026-10-20 r2 P3-2).
    let holds_pick = |r: &&RankedRow| {
        std::iter::once(&r.stream)
            .chain(&r.failover)
            .any(|st| Some(&st.key) == recommended_key.as_ref())
    };
    let rows = s
        .ranked
        .rows
        .iter()
        .filter(|r| !holds_pick(r))
        .map(row_dto)
        .collect();
    let mut by_reason: Vec<(HideReason, u32)> = Vec::new();
    for h in &s.ranked.hidden {
        match by_reason.iter_mut().find(|(r, _)| *r == h.reason) {
            Some((_, n)) => *n += 1,
            None => by_reason.push((h.reason, 1)),
        }
    }
    VersionsSnapshot {
        conditions: s.conditions.clone(),
        blocked: s.blocked.clone(),
        note: s.note.clone(),
        recommended: s.recommended.as_ref().map(row_dto),
        rows,
        hidden: HiddenSummary {
            count: s.ranked.hidden.len() as u32,
            by_reason,
        },
        pending_addons: names(&s.pending),
        slow_addons: names(&s.slow),
        failed_addons: names(&s.failed),
        done: s.done,
        frozen: s.frozen,
        nobody_sharing: nobody_sharing(s),
    }
}

/// Every listed version is a swarm the engine would fetch, and every
/// source of it a swarm whose trackers count one seeder or none. A source
/// without a live count, a link, or one a service may have ready can still
/// start.
fn nobody_sharing(s: &State) -> bool {
    let dead_source = |st: &ParsedStream| {
        let maybe_ready = st.cached == Some(true) || (s.rank.debrid && st.cached.is_none());
        !maybe_ready && info_hash(st).is_some_and(|h| s.live.get(&h).is_some_and(|n| *n <= 1))
    };
    let dead = |r: &RankedRow| {
        r.class == SourceClass::Swarm
            && std::iter::once(&r.stream)
                .chain(&r.failover)
                .all(dead_source)
    };
    !s.ranked.rows.is_empty() && s.ranked.rows.iter().all(dead)
}

fn row_dto(r: &RankedRow) -> VersionRow {
    let s = &r.stream;
    let rel = &s.release;
    VersionRow {
        key: s.key.clone(),
        quality: quality_word(rel.resolution.height(), rel.quality),
        features: features(rel),
        size_bytes: s.size_bytes,
        status: status(s),
        via: s
            .service
            .as_deref()
            .map(service_name)
            .unwrap_or_else(|| s.addon_name.clone()),
        release_name: s
            .filename
            .clone()
            .unwrap_or_else(|| first_line(&s.raw_description, &s.raw_name)),
        languages: rel.languages.clone(),
        reasons: r.reasons.clone(),
        group: r.group,
        failover_count: r.failover.len() as u16,
        line: r.line.clone(),
        source_class: r.class,
        bitrate_mbps: r.bitrate_mbps,
        score: r.key,
    }
}

fn quality_word(height: u32, source: mq_relparse::Quality) -> String {
    use mq_relparse::Quality as Q;
    match height {
        h if h >= 2160 => "4K",
        h if h >= 1440 => "1440p",
        h if h >= 1080 => "1080p",
        h if h >= 720 => "720p",
        // A headline never says "unknown": the source is the next best fact.
        0 => match source {
            Q::Remux => "Remux",
            Q::BluRay => "Blu-ray",
            Q::WebDl => "WEB-DL",
            Q::WebRip => "WEBRip",
            Q::Hdtv => "HDTV",
            Q::Dvd => "DVD",
            Q::Screener => "Screener",
            Q::Telesync => "Telesync",
            Q::Cam => "Cam",
            Q::Unknown => "Quality not stated",
        },
        _ => "SD",
    }
    .into()
}

fn features(rel: &mq_relparse::ParsedRelease) -> Vec<String> {
    use mq_relparse::{Audio, Hdr, Quality};
    let mut out = Vec::new();
    for h in &rel.hdr {
        out.push(
            match h {
                Hdr::DolbyVision => "Dolby Vision",
                Hdr::Hdr10Plus => "HDR10+",
                Hdr::Hdr10 => "HDR10",
                Hdr::Hlg => "HLG",
                Hdr::Sdr => continue,
            }
            .to_owned(),
        );
    }
    if rel.audio.contains(&Audio::Atmos) {
        out.push("Atmos".into());
    } else if rel.audio.iter().any(|a| matches!(a, Audio::TrueHd)) {
        out.push("TrueHD".into());
    } else if rel
        .audio
        .iter()
        .any(|a| matches!(a, Audio::DtsHdMa | Audio::DtsX))
    {
        out.push("DTS-HD".into());
    }
    if let Some(ch) = &rel.channels
        && ch != "stereo"
        && ch != "mono"
    {
        out.push(ch.clone());
    }
    if rel.quality == Quality::Remux || rel.remux {
        out.push("Remux".into());
    }
    out
}

fn status(s: &ParsedStream) -> VersionStatus {
    use mq_protocol::Source;
    match (&s.source, s.cached) {
        (_, Some(true)) => VersionStatus::Ready {
            service: s.service.as_deref().map(service_name),
        },
        (Source::ClientResolve(_), _) => VersionStatus::NeedsDownload {
            service: s.service.as_deref().map(service_name),
        },
        (Source::Torrent { .. }, _) => VersionStatus::Torrent { seeds: s.seeders },
        _ => VersionStatus::Direct,
    }
}

fn service_name(id: &str) -> String {
    match id
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "")
        .as_str()
    {
        "realdebrid" | "rd" => "Real-Debrid",
        "torbox" | "tb" => "TorBox",
        "alldebrid" | "ad" => "AllDebrid",
        "premiumize" | "pm" => "Premiumize",
        "debridlink" => "Debrid-Link",
        "easydebrid" => "EasyDebrid",
        _ => return id.to_owned(),
    }
    .into()
}

fn first_line(description: &str, name: &str) -> String {
    description
        .lines()
        .next()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or(name)
        .trim()
        .to_owned()
}

/// Tests: a view whose ranking lists `rows`, nothing recommended. Their
/// sources are the view's streams, so a relearn ranks them again.
#[cfg(test)]
impl VersionsView {
    pub(crate) fn rows_for_tests(&self, rows: Vec<RankedRow>) {
        let mut s = lock(&self.state);
        s.streams = rows
            .iter()
            .flat_map(|r| std::iter::once(&r.stream).chain(&r.failover))
            .cloned()
            .collect();
        s.ranked = Ranked {
            rows,
            hidden: Vec::new(),
        };
        s.recommended = None;
    }
}

/// Tests: the parameters of a film for a 1080p stick.
#[cfg(test)]
pub(crate) fn film_params() -> VersionsParams {
    VersionsParams {
        kind: "movie".into(),
        video_id: "tt1".into(),
        target: Target {
            title: "Movie".into(),
            year: Some(2020),
            season: None,
            episode: None,
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
        // Facts sent: no meta is asked, so nothing ranks the view again.
        facts: TitleFacts::read(Some("120 min"), Some("2020"), None, None),
    }
}

/// Tests: `streams` ranked for a 1080p stick with the engine.
#[cfg(test)]
pub(crate) fn rank_for_tests(streams: Vec<ParsedStream>) -> Vec<RankedRow> {
    let params = film_params();
    let conditions = Conditions {
        engine: true,
        free_bytes: Some(1 << 40),
        ..Conditions::default()
    };
    let input = RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: None,
        kids: None,
        conditions: &conditions,
    };
    rank(streams, &input).rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(json: &str) -> ParsedStream {
        let body = format!(r#"{{"streams":[{json}]}}"#);
        let wire = mq_protocol::decode_streams(body.as_bytes()).unwrap();
        normalize(&wire.streams[0], "t", "T", 0, 0)
    }

    /// Code review 2026-10-20 r3 P3-8, r4 P3-8: nobodySharing reads every
    /// source of a row: a dead swarm with a link behind it can still play.
    #[tokio::test]
    async fn nobody_sharing_reads_every_source_of_a_row() {
        let core = Core::temporary("p").unwrap();
        let view = VersionsView::open(&core, film_params());
        let hash = "d".repeat(40);
        let swarm = wire(&format!(
            r#"{{"title":"Movie.2020.1080p.WEB-DL.x264-GRP\n👤 1 💾 2.0 GB","infoHash":"{hash}"}}"#
        ));
        let link =
            wire(r#"{"title":"Movie.2020.1080p.WEB-DL.x264-GRP","url":"https://a.example/m.mkv"}"#);
        let mut row = rank_for_tests(vec![swarm]).remove(0);
        assert_eq!(row.class, SourceClass::Swarm);
        let mut s = lock(&view.state);
        s.live.insert(hash, 1);
        s.ranked.rows = vec![row.clone()];
        assert!(nobody_sharing(&s), "a dead swarm alone");
        row.failover.push(link);
        s.ranked.rows = vec![row];
        assert!(!nobody_sharing(&s), "a link behind it can play");
    }

    /// Code review r1 P3: with two accounts, one store's cache answer
    /// marks only the rows that resolve on it.
    #[test]
    fn a_cache_answer_marks_only_its_own_stores_rows() {
        use mq_debrid::StoreId;
        let row = |service: &str| {
            let body = format!(
                r#"{{"streams":[{{"name":"T","title":"x","infoHash":"{}","behaviorHints":{{"service":"{service}"}}}}]}}"#,
                "a".repeat(40)
            );
            let wire = mq_protocol::decode_streams(body.as_bytes()).unwrap();
            normalize(&wire.streams[0], "t", "T", 0, 0)
        };
        let mut streams = vec![row("torbox"), row("premiumize")];
        let stores = [StoreId::TorBox, StoreId::Premiumize];
        let cache = HashMap::from([((Some(StoreId::TorBox), "a".repeat(40)), true)]);
        apply_cache(&mut streams, &cache, &stores);
        assert_eq!((streams[0].cached, streams[1].cached), (Some(true), None));
    }

    #[test]
    fn title_facts_read_the_strings_apps_and_cinemeta_send() {
        let f = TitleFacts::read(Some("142 min"), Some("2002"), None, None);
        assert_eq!((f.runtime_s, f.title_year), (Some(142 * 60), Some(2002)));
        let f = TitleFacts::read(
            Some("2h 22min"),
            Some("2005–2013"),
            Some("2013-09-16"),
            Some(16),
        );
        assert_eq!(f.runtime_s, Some(142 * 60));
        assert_eq!((f.title_year, f.episode_year), (Some(2005), Some(2013)));
        assert_eq!(f.episodes_in_season, Some(16));
        let f = TitleFacts::read(Some("45"), Some("2022-"), Some("soon"), Some(0));
        assert_eq!((f.runtime_s, f.title_year), (Some(45 * 60), Some(2022)));
        assert_eq!((f.episode_year, f.episodes_in_season), (None, None));
        assert_eq!(
            TitleFacts::read(Some("N/A"), None, None, None),
            TitleFacts::default()
        );
    }

    #[test]
    fn a_device_plays_hls_when_its_caps_list_m3u8() {
        let mut caps = DeviceCaps::basic_1080p();
        assert!(plays(&caps).is_empty());
        caps.containers.push("M3U8".into());
        assert_eq!(plays(&caps), [mq_server::Format::Hls]);
    }

    /// Apple's real container list, read from its source: HLS, never DASH.
    #[test]
    fn apples_caps_play_hls_and_refuse_dash() {
        let swift = include_str!("../../../../apps/apple/Marquee/Player/DeviceCaps.swift");
        let line = swift
            .lines()
            .find(|l| l.contains("static let containers"))
            .expect("Apple's containers");
        let list = &line[line.find('[').unwrap() + 1..line.rfind(']').unwrap()];
        let mut caps = DeviceCaps::basic_1080p();
        caps.containers = list
            .split(',')
            .map(|c| c.trim().trim_matches('"').to_owned())
            .collect();
        assert!(caps.containers.contains(&"m3u8".to_owned()), "{caps:?}");
        assert_eq!(plays(&caps), [mq_server::Format::Hls]);
    }

    #[test]
    fn a_device_plays_dash_when_its_caps_list_mpd() {
        let mut caps = DeviceCaps::basic_1080p();
        caps.containers.push("MPD".into());
        assert_eq!(plays(&caps), [mq_server::Format::Dash]);
        caps.containers.push("m3u8".into());
        let both = [mq_server::Format::Hls, mq_server::Format::Dash];
        assert_eq!(plays(&caps), both);
    }
}
