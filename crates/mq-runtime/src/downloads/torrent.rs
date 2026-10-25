//! A torrent download (plans/offline-downloads.md 4.6): the engine fetches
//! the file whole into its own folder and keeps it from every sweep; this
//! job follows its progress and ends it.

use super::http::Ended;
use super::{DownloadReason, Downloads, TorrentRef, Version, save_run};
use crate::Core;
use mq_engine::FileHint;
use mq_library::DownloadRecord;
use mq_protocol::Source;
use mq_streams::ParsedStream;
use std::time::{Duration, Instant};

/// How often the job reads its progress.
const LOOK_EVERY: Duration = Duration::from_secs(1);
/// How often its progress is written to the library.
const SAVE_EVERY: Duration = Duration::from_secs(5);
/// A swarm that gives nothing for this long, the job allowed to fetch the
/// whole time, is given up for the pick's next source (4.3).
const STALL: Duration = Duration::from_secs(600);

/// Fetches `stream` (a torrent) for the job `row` with the engine.
pub(super) async fn fetch(
    d: &Downloads,
    core: &Core,
    row: &mut DownloadRecord,
    stream: &ParsedStream,
) -> Ended {
    let Source::Torrent {
        info_hash,
        file_idx,
        trackers,
        ..
    } = &stream.source
    else {
        return Ended::Move(DownloadReason::Source);
    };
    let engine = match core.torrent_engine().await {
        Ok(engine) => engine,
        Err(e) => {
            tracing::warn!("the torrent engine did not start: {e}");
            return Ended::Move(DownloadReason::Source);
        }
    };
    let Ok(mut version) = serde_json::from_str::<Version>(row.version.as_deref().unwrap_or(""))
    else {
        return Ended::Move(DownloadReason::NoVersion);
    };
    let t = match engine.add_magnet(info_hash, trackers, &[]).await {
        Ok(t) => t,
        Err(e) => {
            tracing::info!("download torrent details: {e}");
            return Ended::Move(DownloadReason::Source);
        }
    };
    let file = match version.torrent.as_ref() {
        Some(kept) => kept.file,
        None => {
            let season = row.season.and_then(|s| u32::try_from(s).ok());
            let episode = row.episode.and_then(|e| u32::try_from(e).ok());
            let hint = match (file_idx, &stream.filename, season, episode) {
                (Some(i), ..) => FileHint::Index(*i as usize),
                (None, Some(name), ..) => FileHint::Name(name.clone()),
                (None, None, Some(season), Some(episode)) => FileHint::Episode { season, episode },
                _ => FileHint::Largest,
            };
            match engine.select_file(t, "", &hint).await {
                Ok(file) => file,
                Err(e) => {
                    tracing::info!("download file choice: {e}");
                    return Ended::Move(DownloadReason::Source);
                }
            }
        }
    };
    let Some(len) = engine
        .files(t)
        .ok()
        .and_then(|f| f.get(file).map(|f| f.len))
    else {
        return Ended::Move(DownloadReason::Source);
    };
    if version.torrent.is_none() {
        version.torrent = Some(TorrentRef {
            hash: info_hash.to_ascii_lowercase(),
            file,
            name: engine
                .files(t)
                .ok()
                .and_then(|f| f.get(file).map(|f| f.path.clone()))
                .unwrap_or_default(),
        });
        version.ext = version
            .torrent
            .as_ref()
            .and_then(|k| k.name.rsplit_once('.'))
            .map(|(_, e)| e.to_ascii_lowercase());
        row.version = serde_json::to_string(&version).ok();
    }
    row.bytes_total = Some(len as i64);
    save_run(core, row);
    let held = row.bytes_done.max(0) as u64;
    if !d.fits_making_room(core, &row.id, len, held).await {
        return Ended::Wait(DownloadReason::Space);
    }
    let handle = match engine.download(t, file).await {
        Ok(handle) => handle,
        Err(e) => {
            tracing::warn!("download start: {e}");
            return Ended::Move(DownloadReason::Source);
        }
    };
    let (mut last, mut last_change, mut saved) = (held, Instant::now(), Instant::now());
    loop {
        let p = handle.progress().await;
        let received = p.have.saturating_sub(last);
        d.progress(&row.id, p.have, Some(len), received);
        d.swarm(&row.id, p.peers);
        if p.have != last {
            (last, last_change) = (p.have, Instant::now());
        }
        if saved.elapsed() >= SAVE_EVERY || p.have >= len {
            row.bytes_done = p.have as i64;
            save_run(core, row);
            saved = Instant::now();
        }
        if p.have >= len {
            // Its torrent pauses when nothing else uses it, which writes
            // the bitfield out.
            handle.stop().await;
            return Ended::Done;
        }
        if p.out_of_space {
            handle.stop().await;
            d.ran_out(&row.id);
            return Ended::Wait(DownloadReason::Space);
        }
        // A full disk waits for space; any other stop is the source's, or
        // it would wait as "Not enough space" and restart in a loop (code
        // review r1, runtime P2-4).
        if let Some(why) = p.failed {
            tracing::info!("download torrent stopped: {why}");
            handle.stop().await;
            let ended = stopped(&why);
            if matches!(ended, Ended::Wait(_)) {
                d.ran_out(&row.id);
            }
            return ended;
        }
        if last_change.elapsed() >= STALL {
            handle.stop().await;
            return Ended::Move(DownloadReason::Source);
        }
        tokio::time::sleep(LOOK_EVERY).await;
    }
}

/// What a torrent the engine stopped means for its job.
fn stopped(why: &str) -> Ended {
    if mq_engine::is_space_failure(why) {
        Ended::Wait(DownloadReason::Space)
    } else {
        Ended::Move(DownloadReason::Source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a full disk waits for space; librqbit's other errors move to
    /// the next source, so nothing waits as "Not enough space" in a loop.
    #[test]
    fn only_a_full_disk_waits_for_space() {
        for why in [
            "storage full",
            "this storage cannot free space while playing",
            "error writing piece: No space left on device (os error 28)",
        ] {
            assert!(
                matches!(stopped(why), Ended::Wait(DownloadReason::Space)),
                "{why}"
            );
        }
        for why in [
            "Permission denied (os error 13)",
            "error checking file: bad length",
        ] {
            assert!(
                matches!(stopped(why), Ended::Move(DownloadReason::Source)),
                "{why}"
            );
        }
    }
}
