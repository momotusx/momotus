//! A download's subtitles (plans/offline-downloads.md 4.10): once its video
//! is whole, every subtitles source is asked as a playback asks (the file's
//! hash and name), and for the languages of every profile on the device
//! and the device's own, up to five tracks each and the forced ones are
//! kept as UTF-8 WebVTT beside the download, with what names each one
//! across plays, so a delay set online holds offline and the other way.

use super::{Downloads, Version};
use crate::Core;
use mq_addons::{AddonEvent, Deadlines};
use mq_library::DownloadRecord;
use mq_protocol::{ResourcePath, WireSubtitle, decode_subtitles};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Tracks kept per language.
const PER_LANGUAGE: usize = 5;
/// Tracks kept in all.
const MAX_TRACKS: usize = 40;
/// A subtitle file larger than this is no subtitle.
const MAX_BYTES: usize = 2 << 20;

/// One kept track: the source's own listing, which source listed it and
/// where in its answer, and its WebVTT file in the job's folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredSubtitle {
    pub(crate) wire: WireSubtitle,
    /// The source's name and manifest id, as a playback names them.
    pub(crate) source: String,
    pub(crate) addon_id: String,
    /// Its place in the installed list (from 1) and in its answer.
    pub(crate) position: usize,
    pub(crate) rank: usize,
    /// The file, in the job's folder.
    pub(crate) file: String,
}

/// What a source listed for the video.
struct Listed {
    wire: WireSubtitle,
    source: String,
    addon_id: String,
    transport_url: url::Url,
    position: usize,
    rank: usize,
}

/// How the subtitles step ended.
pub(super) enum Kept {
    /// These tracks are on disk (maybe none: no source, nothing listed).
    Done(Vec<StoredSubtitle>),
    /// No source answered (no network): ask again later.
    Later,
}

/// Asks the sources about the downloaded video of `row` and keeps what the
/// device's languages want.
pub(super) async fn keep(d: &Downloads, core: &Core, row: &DownloadRecord) -> Kept {
    let core = core.for_profile(&row.added_by);
    // The built-in Turkish source is left out on purpose: it reads a site,
    // release by release, at play time (plans/offline-downloads.md 8a).
    let installed: Vec<_> = core
        .installed_addons()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| {
            !crate::turkcealtyazi::is_builtin(a.transport_url.as_str(), Some(&a.manifest.id))
        })
        .collect();
    let Some(video) = d.file_of(row) else {
        return Kept::Later;
    };
    // A video kept as piece files (a volume that cannot punch holes) has
    // no hash to give: its sources are asked by its name alone (code review
    // r1 m11).
    let plain = video.is_file();
    let version: Option<Version> = row
        .version
        .as_deref()
        .and_then(|v| serde_json::from_str(v).ok());
    let name = version
        .as_ref()
        .and_then(|v| v.chain.first())
        .and_then(|s| s.stream().filename)
        .or_else(|| video.file_name().map(|n| n.to_string_lossy().into_owned()));
    let reader = mq_media::FileRangeReader::new(&video);
    let extra =
        crate::views::playback::video_extra(plain.then_some(&reader), name.as_deref()).await;
    let path = ResourcePath {
        resource: "subtitles".into(),
        r#type: row.kind.clone(),
        id: row.video_id.clone(),
        extra,
    };
    let asked: Vec<_> = installed
        .iter()
        .filter(|a| a.supports(&path))
        .cloned()
        .collect();
    // No subtitles addon yet: one installed later still fills this download
    // at a later look (code review r1, runtime P3-7).
    if asked.is_empty() {
        return Kept::Later;
    }
    let mut listed = Vec::new();
    let mut answered = 0;
    let mut fanout = core.addons().fan_out(&asked, &path, Deadlines::default());
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
        answered += 1;
        let source = asked
            .iter()
            .find(|a| a.transport_url == transport_url)
            .map_or(addon_id.clone(), |a| a.manifest.name.clone());
        let position = installed
            .iter()
            .position(|a| a.transport_url == transport_url)
            .map_or(usize::MAX, |at| at + 1);
        let found = decode_subtitles(&body)
            .map(|r| r.subtitles)
            .unwrap_or_default();
        for (rank, wire) in found.into_iter().enumerate() {
            listed.push(Listed {
                wire,
                source: source.clone(),
                addon_id: addon_id.clone(),
                transport_url: transport_url.clone(),
                position,
                rank,
            });
        }
    }
    if answered == 0 {
        return Kept::Later;
    }
    let wanted = wanted_languages(&core, row);
    let chosen = choose(listed, &wanted);
    let dir = d.job_dir(&row.id).join("subs");
    if tokio::fs::create_dir_all(&dir).await.is_err() {
        return Kept::Later;
    }
    let mut kept = Vec::new();
    for (n, l) in chosen.into_iter().enumerate() {
        let file = format!("{n}.vtt");
        if fetch(&core, &l, &dir.join(&file)).await {
            kept.push(StoredSubtitle {
                wire: l.wire,
                source: l.source,
                addon_id: l.addon_id,
                position: l.position,
                rank: l.rank,
                file: format!("subs/{file}"),
            });
        }
    }
    Kept::Done(kept)
}

/// The languages kept, as tags: every profile's subtitle languages on this
/// device, then the device's language the add carried.
fn wanted_languages(core: &Core, row: &DownloadRecord) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let mut push = |code: &str| {
        let tag = crate::languages::language(code).tag;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    };
    let profiles = core.device_library().profiles().unwrap_or_default();
    for p in profiles {
        for code in core.for_profile(&p.id).prefs().subtitle_languages {
            push(&code);
        }
    }
    let request: serde_json::Value = serde_json::from_str(&row.request).unwrap_or_default();
    if let Some(device) = request.get("language").and_then(|l| l.as_str()) {
        push(device);
    }
    tags
}

/// Of `listed`, those of the wanted languages: per language, the tracks
/// OpenSubtitles made for this very file first, then by source order and
/// the source's own, five at most and every forced one; forty in all.
fn choose(mut listed: Vec<Listed>, wanted: &[String]) -> Vec<Listed> {
    let primary = |tag: &str| crate::languages::primary(tag).to_owned();
    let language = |l: &Listed| crate::views::playback::listed_track(&l.wire).language;
    listed.sort_by_key(|l| {
        let hash = crate::views::playback::hash_match(&l.wire, &l.addon_id);
        (!hash, l.position, l.rank)
    });
    let mut out: Vec<Listed> = Vec::new();
    for tag in wanted {
        let mut taken = 0;
        for l in listed
            .iter()
            .filter(|l| primary(&language(l)) == primary(tag))
        {
            let forced = crate::views::playback::listed_track(&l.wire).forced;
            if (taken < PER_LANGUAGE || forced) && !out.iter().any(|o| o.wire.url == l.wire.url) {
                if !forced {
                    taken += 1;
                }
                out.push(Listed {
                    wire: l.wire.clone(),
                    source: l.source.clone(),
                    addon_id: l.addon_id.clone(),
                    transport_url: l.transport_url.clone(),
                    position: l.position,
                    rank: l.rank,
                });
            }
        }
    }
    out.truncate(MAX_TRACKS);
    out
}

/// Fetches one track through the scope of the source that listed it, as
/// WebVTT into `to`. False when it does not come or is no subtitle.
async fn fetch(core: &Core, l: &Listed, to: &Path) -> bool {
    let scope = l
        .transport_url
        .host_str()
        .map(mq_http::local_addrs)
        .unwrap_or_default();
    let opts = mq_http::RequestOptions {
        timeout: Duration::from_secs(20),
        max_bytes: MAX_BYTES,
        ..Default::default()
    };
    let Ok(answer) = core
        .http()
        .scoped(&scope)
        .get(l.wire.url.as_str(), &opts)
        .await
    else {
        return false;
    };
    let Some(vtt) = mq_server::to_vtt(&answer.body, &l.wire.lang) else {
        return false;
    };
    tokio::fs::write(to, vtt).await.is_ok()
}

/// A kept track's file, for a play.
pub(crate) fn file_path(d: &Downloads, id: &str, s: &StoredSubtitle) -> PathBuf {
    d.job_dir(id).join(&s.file)
}
