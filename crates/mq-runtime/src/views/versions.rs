//! Versions (stream picker): queries every stream addon for one video,
//! normalizes and ranks the union on each arrival, and publishes the pick
//! with its reasons, intent groups and hidden rows.
//!
//! The recommendation is stable once the user starts moving focus
//! (`freeze`), so late addon results never swap the row under the cursor.

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::{AddonEvent, Deadlines};
use mq_protocol::{ResourcePath, decode_streams};
use mq_streams::{
    DeviceCaps, Group, HideReason, ParsedStream, RankInput, Ranked, RankedRow, Reason, StreamPrefs,
    Target, normalize, rank,
};
use serde::Serialize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::task::JoinSet;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct VersionRow {
    pub key: String,
    /// "4K", "1080p", "720p", "SD" or "Unknown".
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
    pending: Vec<String>,
    slow: Vec<String>,
    failed: Vec<String>,
    done: bool,
}

pub struct VersionsView {
    snapshot: SharedObservable<VersionsSnapshot>,
    state: Arc<Mutex<State>>,
    _tasks: JoinSet<()>,
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
    pub fn open(core: &Core, mut params: VersionsParams) -> Self {
        if params.last_binge_group.is_none() {
            let item_id = params.video_id.split(':').next().unwrap_or_default();
            params.last_binge_group = core
                .library()
                .last_binge_group(core.profile(), item_id)
                .ok()
                .flatten();
        }
        let addons = core.installed_addons().unwrap_or_default();
        let path = ResourcePath {
            resource: "stream".into(),
            r#type: params.kind.clone(),
            id: params.video_id.clone(),
            extra: vec![],
        };
        let eligible: Vec<_> = addons.into_iter().filter(|a| a.supports(&path)).collect();
        let state = Arc::new(Mutex::new(State {
            streams: Vec::new(),
            ranked: Ranked::default(),
            recommended: None,
            frozen: false,
            pending: eligible.iter().map(|a| a.manifest.name.clone()).collect(),
            slow: Vec::new(),
            failed: Vec::new(),
            done: eligible.is_empty(),
        }));
        let snapshot = SharedObservable::new(render(&lock(&state)));
        let mut tasks = JoinSet::new();
        tasks.spawn(collect(
            core.clone(),
            eligible,
            path,
            params,
            state.clone(),
            snapshot.clone(),
        ));
        Self {
            snapshot,
            state,
            _tasks: tasks,
        }
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
    pub fn freeze(&self) {
        let mut state = lock(&self.state);
        state.frozen = true;
        self.snapshot.set(render(&state));
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

async fn collect(
    core: Core,
    addons: Vec<mq_addons::InstalledAddon>,
    path: ResourcePath,
    params: VersionsParams,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<VersionsSnapshot>,
) {
    let names: std::collections::HashMap<String, (String, u16)> = addons
        .iter()
        .enumerate()
        .map(|(i, a)| (a.id().to_owned(), (a.manifest.name.clone(), i as u16)))
        .collect();
    let mut fanout = core.addons().fan_out(&addons, &path, Deadlines::default());
    let params = Arc::new(params);
    let mut next_index = 0u32;
    while let Some(event) = fanout.next().await {
        // Parsing is CPU work: keep it off the async threads that feed UIs.
        let parsed = match &event {
            AddonEvent::Arrived { addon_id, body, .. } => {
                let (name, rank_index) = names
                    .get(addon_id)
                    .cloned()
                    .unwrap_or((addon_id.clone(), u16::MAX));
                let (addon_id, body, start) = (addon_id.clone(), body.clone(), next_index);
                let parsed = tokio::task::spawn_blocking(move || {
                    decode_streams(&body)
                        .map(|r| {
                            r.streams
                                .iter()
                                .enumerate()
                                .map(|(i, w)| {
                                    normalize(w, &addon_id, &name, rank_index, start + i as u32)
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
            AddonEvent::Arrived { addon_id, .. } => {
                let name = names
                    .get(&addon_id)
                    .map(|(n, _)| n.clone())
                    .unwrap_or(addon_id);
                s.streams.extend(parsed);
                s.pending.retain(|p| p != &name);
                s.slow.retain(|p| p != &name);
            }
            AddonEvent::Slow { addon_ids } => {
                s.slow = addon_ids
                    .iter()
                    .map(|id| {
                        names
                            .get(id)
                            .map(|(n, _)| n.clone())
                            .unwrap_or_else(|| id.clone())
                    })
                    .collect();
            }
            AddonEvent::Failed { addon_id, .. } => {
                let name = names
                    .get(&addon_id)
                    .map(|(n, _)| n.clone())
                    .unwrap_or(addon_id);
                s.pending.retain(|p| p != &name);
                s.slow.retain(|p| p != &name);
                s.failed.push(name);
            }
            AddonEvent::Done => s.done = true,
        }
        rerank(&mut s, &params);
        snapshot.set(render(&s));
    }
}

fn rerank(s: &mut State, params: &VersionsParams) {
    let input = RankInput {
        caps: &params.caps,
        prefs: &params.prefs,
        target: &params.target,
        last_binge_group: params.last_binge_group.as_deref(),
    };
    s.ranked = rank(s.streams.clone(), &input);
    if !s.frozen {
        s.recommended = s.ranked.best().cloned();
    }
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
        pending_addons: s.pending.clone(),
        slow_addons: s.slow.clone(),
        failed_addons: s.failed.clone(),
        done: s.done,
        frozen: s.frozen,
    }
}

fn row_dto(r: &RankedRow) -> VersionRow {
    let s = &r.stream;
    let rel = &s.release;
    VersionRow {
        key: s.key.clone(),
        quality: quality_word(rel.resolution.height()),
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

fn quality_word(height: u32) -> String {
    match height {
        h if h >= 2160 => "4K",
        h if h >= 1440 => "1440p",
        h if h >= 1080 => "1080p",
        h if h >= 720 => "720p",
        0 => "Unknown",
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
