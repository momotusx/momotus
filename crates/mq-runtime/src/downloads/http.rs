//! An HTTP download (plans/offline-downloads.md 4.7): a debrid link or a
//! direct one, fetched with one ranged request into `video.<ext>.part`,
//! checkpointed every 8 MiB so a kill or a reboot loses at most that much.

use super::{DownloadReason, Downloads, FLOOR, Version, save_run};
use crate::Core;
use crate::views::playback::debrid_request;
use mq_http::{Priority, RequestOptions};
use mq_library::DownloadRecord;
use mq_protocol::Source;
use mq_streams::ParsedStream;
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Bytes between checkpoints.
const CHECKPOINT: u64 = 8 << 20;
/// What the head check reads first.
const HEAD: usize = 64 * 1024;
/// Waits between tries of a source that failed (4.7.4).
const BACKOFF: [Duration; 3] = [
    Duration::from_secs(2),
    Duration::from_secs(8),
    Duration::from_secs(30),
];
/// Tests: the waits between tries in milliseconds, not seconds.
#[cfg(feature = "test-hooks")]
pub(super) static SHORT_BACKOFF: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// A body that sends nothing for this long has failed.
const IDLE: Duration = Duration::from_secs(60);

/// How one source's fetch ended.
pub(super) enum Ended {
    Done,
    /// It stops here and goes on later from its checkpoint.
    Wait(DownloadReason),
    /// This source cannot give the file: the next one of the pick.
    Move(DownloadReason),
}

/// This build downloads the source over HTTP: a direct link, or a torrent
/// a connected debrid account resolves. Swarm downloads come with the
/// engine's kept files (plans/offline-downloads.md 4.6).
pub(super) fn downloadable(core: &Core, s: &ParsedStream) -> bool {
    match &s.source {
        Source::Http { url } => !looks_like_playlist(url.path()),
        Source::Torrent { .. } => core.debrid().is_some() || core.has_engine(),
        Source::ClientResolve(_) => core.debrid().is_some(),
        _ => false,
    }
}

/// HLS and DASH are not downloaded (plans/offline-downloads.md 10).
fn looks_like_playlist(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.ends_with(".m3u8") || path.ends_with(".mpd")
}

struct Link {
    url: String,
    headers: Vec<(String, String)>,
    addon_url: Option<url::Url>,
    filename: Option<String>,
    debrid: bool,
}

async fn link(core: &Core, stream: &ParsedStream, row: &DownloadRecord) -> Result<Link, String> {
    match &stream.source {
        Source::Http { url } => Ok(Link {
            url: url.to_string(),
            headers: stream.headers.clone(),
            addon_url: stream.addon_url.clone(),
            filename: stream.filename.clone(),
            debrid: false,
        }),
        _ => {
            let season = row.season.and_then(|s| u32::try_from(s).ok());
            let episode = row.episode.and_then(|e| u32::try_from(e).ok());
            let (service, request) =
                debrid_request(stream, season, episode).ok_or("no torrent named")?;
            let resolver = core.debrid().ok_or("no debrid account")?;
            let request =
                crate::views::playback::with_store(&resolver, service.as_deref(), request);
            let resolved = resolver.resolve(request).await.map_err(|e| e.to_string())?;
            Ok(Link {
                url: resolved.url,
                headers: Vec::new(),
                addon_url: None,
                filename: Some(resolved.filename),
                debrid: true,
            })
        }
    }
}

/// The file's extension, from its name, else the answer's type.
fn extension(filename: Option<&str>, content_type: Option<&str>) -> String {
    let named = filename
        .and_then(|f| f.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .filter(|e| {
            matches!(
                e.as_str(),
                "mkv" | "mp4" | "m4v" | "webm" | "avi" | "mov" | "ts" | "m2ts"
            )
        });
    named.unwrap_or_else(|| {
        match content_type.unwrap_or_default() {
            t if t.contains("mp4") => "mp4",
            t if t.contains("webm") => "webm",
            t if t.contains("quicktime") => "mov",
            t if t.contains("mp2t") => "ts",
            _ => "mkv",
        }
        .into()
    })
}

/// `Content-Range`'s first byte (`bytes 100-199/1000`).
fn range_start(headers: &mq_http::HeaderMap) -> Option<u64> {
    headers
        .get("content-range")?
        .to_str()
        .ok()?
        .trim()
        .strip_prefix("bytes ")?
        .split_once('-')?
        .0
        .trim()
        .parse()
        .ok()
}

/// `Content-Range`'s total, else the length of a whole answer.
fn total_of(status: u16, headers: &mq_http::HeaderMap) -> Option<u64> {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if status == 206 {
        return header("content-range")
            .and_then(|v| v.rsplit_once('/'))
            .and_then(|(_, total)| total.trim().parse().ok());
    }
    header("content-length").and_then(|v| v.trim().parse().ok())
}

/// Fetches `stream` for the job `row`, from `row.bytes_done`.
pub(super) async fn fetch(
    d: &Downloads,
    core: &Core,
    row: &mut DownloadRecord,
    stream: &ParsedStream,
) -> Ended {
    let dir = d.job_dir(&row.id);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        tracing::warn!("download folder not made: {e}");
        // Waited for as space, a minute at least (code review r1 M6).
        d.ran_out(&row.id);
        return Ended::Wait(DownloadReason::Space);
    }
    let mut tries = 0usize;
    let mut asked_again = false;
    let mut restarted = false;
    loop {
        if d.link().offline {
            return Ended::Wait(DownloadReason::Network);
        }
        let link = match link(core, stream, row).await {
            Ok(link) => link,
            Err(e) => {
                tracing::info!("download source not resolved: {e}");
                match backoff(&mut tries).await {
                    Some(()) => continue,
                    None => return Ended::Move(DownloadReason::Source),
                }
            }
        };
        match one_request(d, core, row, &link, &dir).await {
            Request::Done => return Ended::Done,
            Request::Ended(ended) => return ended,
            Request::Refused if link.debrid && !asked_again => {
                // An expired link: the store gives a new one once.
                asked_again = true;
                if let Some(resolver) = core.debrid() {
                    resolver.forget(&link.url);
                }
            }
            Request::Refused => return Ended::Move(DownloadReason::Source),
            // Once from 0; a source that changes again is not this file.
            Request::Restart if !restarted => restarted = true,
            Request::Restart => return Ended::Move(DownloadReason::Source),
            Request::Failed { progressed } => {
                if progressed {
                    tries = 0;
                }
                if d.link().offline {
                    return Ended::Wait(DownloadReason::Network);
                }
                if backoff(&mut tries).await.is_none() {
                    return Ended::Move(DownloadReason::Source);
                }
            }
        }
    }
}

/// The next wait, or `None` once the tries are spent.
async fn backoff(tries: &mut usize) -> Option<()> {
    let wait = *BACKOFF.get(*tries)?;
    #[cfg(feature = "test-hooks")]
    let wait = if SHORT_BACKOFF.load(std::sync::atomic::Ordering::Relaxed) {
        wait / 1000
    } else {
        wait
    };
    *tries += 1;
    tokio::time::sleep(wait).await;
    Some(())
}

enum Request {
    Done,
    Ended(Ended),
    /// 403, 404 or 410: the link is dead.
    Refused,
    /// The answer is another file or another range than the one resumed:
    /// the file starts again from 0, at once.
    Restart,
    /// The network or the origin failed; `progressed` when bytes came first.
    Failed {
        progressed: bool,
    },
}

async fn one_request(
    d: &Downloads,
    core: &Core,
    row: &mut DownloadRecord,
    link: &Link,
    dir: &Path,
) -> Request {
    let mut version: Version = match row.version.as_deref().map(serde_json::from_str) {
        Some(Ok(v)) => v,
        _ => return Request::Ended(Ended::Move(DownloadReason::NoVersion)),
    };
    let from = row.bytes_done.max(0) as u64;
    let known = row.bytes_total.and_then(|t| u64::try_from(t).ok());
    // Killed between the last checkpoint and the rename: the file is whole
    // already, and a request for nothing would answer 416 (code review r1,
    // engine P2-7).
    if let (Some(ext), Some(total)) = (version.ext.clone(), known)
        && total > 0
    {
        let part = dir.join(format!("video.{ext}.part"));
        let whole = dir.join(format!("video.{ext}"));
        let len = |path: &Path| {
            let path = path.to_owned();
            async move { tokio::fs::metadata(path).await.map(|m| m.len()).ok() }
        };
        // Renamed already, the row never told (a kill right after the
        // rename): done, never fetched again (code review r1 m2).
        if len(&whole).await == Some(total) {
            return Request::Done;
        }
        if from == total && len(&part).await == Some(total) {
            return finish(&part, &whole).await;
        }
        // A `.part` shorter than its checkpoint (the folder was restored):
        // what follows would land after a hole, so it starts again.
        if from > 0 && len(&part).await.is_none_or(|l| l < from) {
            row.bytes_done = 0;
            save_run(core, row);
            return Request::Restart;
        }
    }
    let scope = link
        .addon_url
        .as_ref()
        .and_then(url::Url::host_str)
        .map(mq_http::local_addrs)
        .unwrap_or_default();
    let http = core.http().scoped(&scope);
    let mut headers = link.headers.clone();
    if from > 0
        && let Some(v) = &version.validator
    {
        headers.push(("If-Range".into(), v.clone()));
    }
    let opts = RequestOptions {
        priority: Priority::Playback,
        timeout: IDLE,
        headers,
        ..Default::default()
    };
    let mut answer = match http
        .stream(&link.url, Some(format!("bytes={from}-")), &opts)
        .await
    {
        Ok(answer) => answer,
        Err(e) => {
            tracing::info!("download source unreachable: {e}");
            return Request::Failed { progressed: false };
        }
    };
    match answer.status {
        403 | 404 | 410 => return Request::Refused,
        // A range past the end: the file it resumes is not this one, or is
        // whole already (taken care of above); never a dead source.
        416 => {
            row.bytes_done = 0;
            save_run(core, row);
            return Request::Restart;
        }
        200 | 206 => {}
        status if status == 429 || status >= 500 => return Request::Failed { progressed: false },
        _ => return Request::Refused,
    }
    let Some(total) = total_of(answer.status, &answer.headers) else {
        // No length: completeness cannot be checked (4.8).
        return Request::Ended(Ended::Move(DownloadReason::Source));
    };
    // A resume must get the same file from where it stopped: a link
    // resolved again can name another file of a pack, or answer another
    // range (code review r1, runtime P2-7, engine P2-6). From 0 the body is
    // the file's start anyway.
    let start = range_start(&answer.headers);
    if answer.status == 206
        && start != Some(0)
        && (start != Some(from) || known.is_some_and(|k| k != total))
    {
        row.bytes_done = 0;
        save_run(core, row);
        return Request::Restart;
    }
    // An origin that ignores ranges, or a changed file under `If-Range`,
    // sends it all again.
    let from = if answer.status == 200 || start == Some(0) {
        0
    } else {
        from
    };
    if from == 0 {
        let header = |name: &str| answer.headers.get(name).and_then(|v| v.to_str().ok());
        version.validator = header("etag")
            .filter(|e| !e.starts_with("W/"))
            .or_else(|| header("last-modified"))
            .map(str::to_owned);
        row.version = serde_json::to_string(&version).ok();
    }
    if version.ext.is_none() {
        let content_type = answer
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok());
        version.ext = Some(extension(link.filename.as_deref(), content_type));
        row.version = serde_json::to_string(&version).ok();
    }
    let ext = version.ext.clone().unwrap_or_else(|| "mkv".into());
    // Known before the space rule: a job waiting for space looks again
    // with what it needs (`Downloads::tick`).
    row.bytes_total = Some(total as i64);
    row.bytes_done = from as i64;
    save_run(core, row);
    if !d.fits_making_room(core, &row.id, total, from).await {
        return Request::Ended(Ended::Wait(DownloadReason::Space));
    }
    let part = dir.join(format!("video.{ext}.part"));
    let mut file = match open_at(&part, from).await {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("download file not opened: {e}");
            d.ran_out(&row.id);
            return Request::Ended(Ended::Wait(DownloadReason::Space));
        }
    };
    let mut done = from;
    let mut since_checkpoint = 0u64;
    let mut head = Vec::new();
    let mut progressed = false;
    while done < total {
        let chunk = match answer.next_chunk().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(e)) => {
                tracing::info!("download body failed: {e}");
                checkpoint(core, row, &mut file, done).await;
                return Request::Failed { progressed };
            }
            None => break,
        };
        if done == 0 && head.len() < HEAD {
            head.extend_from_slice(&chunk);
            if head.len() >= HEAD.min(total as usize) && !mq_media::looks_like_video(&head) {
                return Request::Ended(Ended::Move(DownloadReason::NotVideo));
            }
            if head.len() < HEAD.min(total as usize) {
                continue;
            }
            let bytes = std::mem::take(&mut head);
            if write(&mut file, &bytes).await.is_err() {
                d.ran_out(&row.id);
                return Request::Ended(Ended::Wait(DownloadReason::Space));
            }
            done += bytes.len() as u64;
            since_checkpoint += bytes.len() as u64;
            d.progress(&row.id, done, Some(total), bytes.len() as u64);
            progressed = true;
            continue;
        }
        if write(&mut file, &chunk).await.is_err() {
            checkpoint(core, row, &mut file, done).await;
            d.ran_out(&row.id);
            return Request::Ended(Ended::Wait(DownloadReason::Space));
        }
        done += chunk.len() as u64;
        since_checkpoint += chunk.len() as u64;
        progressed = true;
        if since_checkpoint >= CHECKPOINT {
            since_checkpoint = 0;
            checkpoint(core, row, &mut file, done).await;
            if core.free_bytes().is_some_and(|free| free < FLOOR) {
                d.ran_out(&row.id);
                return Request::Ended(Ended::Wait(DownloadReason::Space));
            }
        }
        d.progress(&row.id, done, Some(total), chunk.len() as u64);
    }
    if !head.is_empty() {
        // A file shorter than the head check's span.
        if !mq_media::looks_like_video(&head) {
            return Request::Ended(Ended::Move(DownloadReason::NotVideo));
        }
        if write(&mut file, &head).await.is_err() {
            return Request::Ended(Ended::Wait(DownloadReason::Space));
        }
        done += head.len() as u64;
    }
    checkpoint(core, row, &mut file, done).await;
    if done < total {
        return Request::Failed { progressed };
    }
    drop(file);
    finish(&part, &dir.join(format!("video.{ext}"))).await
}

/// A whole `.part`: checked as a video, then given its final name.
async fn finish(part: &Path, whole: &Path) -> Request {
    if !whole_video(part).await {
        return Request::Ended(Ended::Move(DownloadReason::NotVideo));
    }
    if let Err(e) = tokio::fs::rename(part, whole).await {
        tracing::warn!("download not renamed: {e}");
        return Request::Failed { progressed: false };
    }
    Request::Done
}

/// The `.part` file, cut back to the last checkpoint (bytes after it may
/// not have reached the disk) and opened for writing at its end.
async fn open_at(path: &Path, from: u64) -> std::io::Result<tokio::fs::File> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .await?;
    file.set_len(from).await?;
    use tokio::io::AsyncSeekExt;
    file.seek(std::io::SeekFrom::Start(from)).await?;
    Ok(file)
}

/// Writes `bytes` and waits until the file has them: `held` never names
/// bytes a reader of the file could not read yet (code review r1 m5).
async fn write(file: &mut tokio::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    file.write_all(bytes).await?;
    file.flush().await
}

/// Flushes the file to the disk, then records `done`: a checkpoint never
/// names bytes the disk may not hold.
async fn checkpoint(core: &Core, row: &mut DownloadRecord, file: &mut tokio::fs::File, done: u64) {
    if file.flush().await.is_err() || file.sync_data().await.is_err() {
        return;
    }
    row.bytes_done = done as i64;
    save_run(core, row);
}

/// The whole file reads as a video: a Matroska or MP4 container whose
/// headers parse to a video track; another container by its first bytes.
async fn whole_video(path: &Path) -> bool {
    let reader = mq_media::FileRangeReader::new(path);
    let head = match mq_media::RangeReader::read_at(&reader, 0, HEAD as u64).await {
        Ok(head) => head,
        Err(_) => return false,
    };
    if !mq_media::looks_like_video(&head) {
        return false;
    }
    let budget = mq_media::ProbeBudget {
        max_bytes: 64 << 20,
        max_time: Duration::from_secs(30),
    };
    let probed = mq_media::probe(&reader, budget, &mq_media::CancellationToken::new()).await;
    match probed.container.value {
        // Its container parsed: a track list must be there.
        Some(_) => probed.video.is_some() || !probed.audio.is_empty(),
        // MPEG-TS and AVI are not parsed further.
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_comes_from_the_name_then_the_type() {
        assert_eq!(extension(Some("Show.S01E01.1080p.MP4"), None), "mp4");
        assert_eq!(extension(Some("index"), Some("video/webm")), "webm");
        assert_eq!(extension(None, Some("application/octet-stream")), "mkv");
        // A name with an extension that is not a video's does not count.
        assert_eq!(extension(Some("file.php"), Some("video/mp4")), "mp4");
    }

    #[test]
    fn playlists_are_not_downloaded() {
        assert!(looks_like_playlist("/a/master.M3U8"));
        assert!(looks_like_playlist("/a/manifest.mpd"));
        assert!(!looks_like_playlist("/a/video.mkv"));
    }
}
