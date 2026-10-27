//! `mq versions`: the Versions view as an app opens it, with no playback
//! (plans/research/pick-audit.md). Tracker scrapes only: no piece of any
//! torrent is fetched.

use crate::{emit, ms, until};
use mq_addons::{AddonEvent, Deadlines};
use mq_protocol::{ResourcePath, Source, decode_streams};
use mq_relparse::{Hdr, VideoCodec};
use mq_runtime::speed::{self, Kind, Sample};
use mq_runtime::{Core, TitleView, VersionRow, VersionStatus, VersionsParams, VersionsView};
use mq_streams::{DeviceCaps, ParsedStream, Target, normalize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The device the view ranks for.
pub(crate) struct Device {
    pub height: u32,
    pub hdr: bool,
    pub dv: bool,
    pub h264_only: bool,
    pub mbps: Option<f32>,
    pub free_gb: Option<f64>,
    pub language: String,
}

impl Device {
    /// As the Android app reports a device (`DeviceCaps.kt`): hardware
    /// video decoders, Media3's containers, no fallback engine.
    fn caps(&self) -> DeviceCaps {
        let mut caps = DeviceCaps::basic_1080p();
        caps.display_height = self.height;
        caps.decoders = vec![
            VideoCodec::H264,
            VideoCodec::H265,
            VideoCodec::Vp9,
            VideoCodec::Av1,
        ];
        caps.containers = [
            "mkv", "mp4", "m4v", "webm", "ts", "m2ts", "mov", "m3u8", "mpd",
        ]
        .map(str::to_owned)
        .to_vec();
        caps.has_fallback_engine = false;
        if self.hdr {
            caps.display_hdr = vec![Hdr::Hdr10, Hdr::Hlg];
        }
        if self.dv {
            caps.display_hdr.extend([Hdr::DolbyVision, Hdr::Hdr10Plus]);
        }
        if self.h264_only {
            caps.decoders = vec![VideoCodec::H264];
            caps.ten_bit_hevc = false;
        }
        caps
    }
}

/// The line's speed as one measured playback of each kind: a well seeded
/// swarm's sample, so it counts for the line, not for thin swarms.
fn measured(core: &Core, mbps: f32) {
    let (lib, network, now) = (core.library(), core.network(), mq_library::hlc::now_ms());
    speed::reset(lib, network);
    let busy = Duration::from_secs(8);
    let bytes = (f64::from(mbps) * 1e6 / 8.0 * busy.as_secs_f64()) as u64;
    for kind in [Kind::Http, Kind::P2p] {
        let sample = Sample {
            kind,
            bytes,
            busy,
            seeders: Some(u32::MAX),
        };
        speed::record(lib, network, &sample, 0, now);
    }
}

/// What each addon listed, by stream key, before any live count.
async fn listed(core: &Core, path: &ResourcePath) -> HashMap<String, ParsedStream> {
    let addons = core.installed_addons().unwrap_or_default();
    let mut fanout = core.addons().fan_out(&addons, path, Deadlines::default());
    let mut seeders = HashMap::new();
    while let Some(event) = fanout.next().await {
        match event {
            AddonEvent::Arrived { addon_id, body, .. } => {
                for wire in decode_streams(&body).map(|r| r.streams).unwrap_or_default() {
                    let s = normalize(&wire, &addon_id, &addon_id, 0, 0);
                    seeders.insert(s.key.clone(), s);
                }
            }
            AddonEvent::Done => break,
            _ => {}
        }
    }
    seeders
}

pub(crate) async fn versions(
    core: &Core,
    kind: String,
    id: String,
    device: Device,
) -> Result<(), String> {
    if let Some(gb) = device.free_gb {
        core.set_free_bytes((gb * 1e9) as u64);
    }
    if let Some(mbps) = device.mbps {
        measured(core, mbps);
    }
    let mut parts = id.split(':');
    let item_id = parts.next().unwrap_or_default().to_owned();
    let season = parts.next().and_then(|s| s.parse().ok());
    let episode = parts.next().and_then(|s| s.parse().ok());
    // The title page first, as in the apps: it warms the trackers.
    let title = TitleView::open(core, &kind, &item_id);
    let name = until(Duration::from_secs(10), || {
        title.snapshot().header.details.map(|d| d.name)
    })
    .await
    .unwrap_or_default();
    let path = ResourcePath {
        resource: "stream".into(),
        r#type: kind.clone(),
        id: id.clone(),
        extra: vec![],
    };
    let claimed = listed(core, &path).await;
    // The device's language, then English, as the bridge sends them.
    let mut prefs = core.prefs().streams;
    if prefs.languages.is_empty() {
        prefs.languages = vec![device.language.clone(), "en".into()];
        prefs.languages.dedup();
    }
    let started = Instant::now();
    let view = VersionsView::open(
        core,
        VersionsParams {
            device_language: None,
            kind: kind.clone(),
            video_id: id.clone(),
            target: Target {
                title: name.clone(),
                year: None,
                season,
                episode,
            },
            caps: device.caps(),
            prefs,
            last_binge_group: None,
            facts: Default::default(),
        },
    );
    // What the apps play: the recommendation once every addon answered.
    let at_done = until(Duration::from_secs(15), || {
        let s = view.snapshot();
        s.done.then_some(s.recommended)
    })
    .await
    .flatten();
    let done_ms = ms(started.elapsed());
    until(Duration::from_secs(25), || (!view.counting()).then_some(())).await;
    let counted_ms = ms(started.elapsed());
    let snapshot = view.snapshot();
    let rows: Vec<Value> = snapshot
        .recommended
        .iter()
        .chain(&snapshot.rows)
        .map(|r| row(&view, r, &claimed))
        .collect();
    // Listed but in no row's chain: hidden by a gate.
    let shown: std::collections::HashSet<String> = snapshot
        .recommended
        .iter()
        .chain(&snapshot.rows)
        .flat_map(|r| view.candidates(&r.key))
        .map(|s| s.key)
        .collect();
    let hidden_rows: Vec<Value> = claimed
        .values()
        .filter(|s| !shown.contains(&s.key))
        .map(|s| {
            let r = &s.release;
            json!({ "name": s.filename.clone().unwrap_or_else(|| s.raw_description.lines().next().unwrap_or_default().to_owned()),
                    "codec": r.codec, "bitDepth": r.bit_depth, "audio": r.audio, "container": r.container,
                    "hdr": r.hdr, "title": r.title, "year": r.year, "seasons": r.seasons, "episodes": r.episodes })
        })
        .collect();
    emit(json!({
        "event": "versions", "kind": kind, "id": id, "title": name,
        "conditions": snapshot.conditions, "doneMs": done_ms, "countedMs": counted_ms,
        "pickAtDone": at_done.map(|r| json!({ "key": r.key, "seeders": seeds(&r) })),
        "pick": snapshot.recommended.as_ref().map(|r| &r.key),
        "hidden": snapshot.hidden, "hiddenRows": hidden_rows, "failedAddons": snapshot.failed_addons,
        "nobodySharing": snapshot.nobody_sharing, "rows": rows,
    }));
    Ok(())
}

fn seeds(r: &VersionRow) -> Option<u32> {
    match r.status {
        VersionStatus::Torrent { seeds } => seeds,
        _ => None,
    }
}

/// One row: what the view shows, the parsed release, and both seeder
/// counts.
fn row(view: &VersionsView, r: &VersionRow, claimed: &HashMap<String, ParsedStream>) -> Value {
    let stream = view.candidates(&r.key).into_iter().next();
    let hash = stream.as_ref().and_then(|s| match &s.source {
        Source::Torrent { info_hash, .. } => Some(info_hash.clone()),
        _ => None,
    });
    json!({
        "key": r.key, "hash": hash, "release": r.release_name, "quality": r.quality,
        "parsed": stream.as_ref().map(|s| &s.release), "sizeBytes": r.size_bytes,
        "bitrateMbps": r.bitrate_mbps, "seeders": seeds(r),
        "listedSeeders": claimed.get(&r.key).and_then(|s| s.seeders),
        "cached": stream.as_ref().and_then(|s| s.cached), "class": r.source_class,
        "line": r.line, "reasons": r.reasons, "group": r.group, "score": r.score,
        "failover": r.failover_count,
    })
}
