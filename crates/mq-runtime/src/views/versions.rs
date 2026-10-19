//! Versions (stream picker): queries every stream addon for one video,
//! normalizes and ranks the union on each arrival, and publishes the pick
//! with its reasons, intent groups and hidden rows.
//!
//! The recommendation is stable once the user starts moving focus
//! (`freeze`), so late addon results never swap the row under the cursor.

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::views::addons_changed;
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::{AddonEvent, Deadlines, InstalledAddon};
use mq_library::{Policy, Table};
use mq_protocol::{ResourcePath, decode_streams};
use mq_streams::{
    DeviceCaps, Group, HideReason, ParsedStream, RankInput, Ranked, RankedRow, Reason, StreamPrefs,
    Target, normalize, rank,
};
use serde::Serialize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
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
    pub recommended: Option<VersionRow>,
    pub rows: Vec<VersionRow>,
    pub hidden: HiddenSummary,
    pub pending_addons: Vec<String>,
    pub slow_addons: Vec<String>,
    pub failed_addons: Vec<String>,
    pub done: bool,
    pub frozen: bool,
}

struct State {
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
    _tasks: JoinSet<()>,
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
        let state = Arc::new(Mutex::new(State {
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
        }));
        let snapshot = SharedObservable::new(render(&lock(&state)));
        let mut tasks = JoinSet::new();
        let ranking = Arc::new(Ranking {
            kids: verified.as_ref().map(|v| v.names.clone()),
            params,
        });
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
            _tasks: tasks,
        }
    }

    fn denied_view(core: &Core, params: &VersionsParams) -> Self {
        let state = State {
            streams: Vec::new(),
            ranked: Ranked::default(),
            recommended: None,
            frozen: false,
            pending: Vec::new(),
            slow: Vec::new(),
            failed: Vec::new(),
            done: false,
            prefetched: true,
        };
        Self {
            core: core.clone(),
            plays: plays(&params.caps),
            episode: (None, None),
            snapshot: SharedObservable::new(render(&state)),
            state: Arc::new(Mutex::new(state)),
            verified: None,
            denied: true,
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

    /// The playlist formats the device plays, for `PlaybackView::open_for`.
    pub fn plays(&self) -> Vec<mq_server::Format> {
        self.plays.clone()
    }

    /// The stream for row `key` followed by its failover chain, for playback.
    pub fn candidates(&self, key: &str) -> Vec<ParsedStream> {
        let state = lock(&self.state);
        state
            .ranked
            .rows
            .iter()
            .find(|r| r.stream.key == key)
            .map(|r| {
                std::iter::once(&r.stream)
                    .chain(&r.failover)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
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
    while let Some(event) = fanout.next().await {
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
            AddonEvent::Done => s.done = true,
        }
        rerank(&mut s, &ranking);
        if s.done {
            maybe_prefetch(&core, &mut s, (params.target.season, params.target.episode));
        }
        snapshot.set(render(&s));
    }
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

/// What the ranking reads: the view's params, and for a kids profile the
/// allowed title's names (plans/m11-kids.md 4.4).
struct Ranking {
    params: VersionsParams,
    kids: Option<Vec<String>>,
}

fn rerank(s: &mut State, ranking: &Ranking) {
    let params = &ranking.params;
    let input = RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
        kids: ranking.kids.as_deref(),
    };
    s.ranked = rank(s.streams.clone(), &input);
    if !s.frozen {
        s.recommended = s.ranked.best().cloned();
    }
}

fn names(installs: &[(Url, String)]) -> Vec<String> {
    installs.iter().map(|(_, name)| name.clone()).collect()
}

fn render(s: &State) -> VersionsSnapshot {
    let recommended_key = s.recommended.as_ref().map(|r| r.stream.key.clone());
    let rows = s
        .ranked
        .rows
        .iter()
        .filter(|r| Some(&r.stream.key) != recommended_key.as_ref())
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
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
