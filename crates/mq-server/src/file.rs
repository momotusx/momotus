//! A file on this device: an offline download (plans/offline-downloads.md
//! 4.12), served with ranges as a torrent's file is. A download still being
//! written serves what it holds and waits for the rest while its download
//! runs.

use super::*;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::watch;

/// How long a read past what a running download holds waits for it.
pub(crate) const PARTIAL_WAIT: Duration = Duration::from_secs(20);

pub(crate) struct LocalFile {
    pub(crate) path: std::path::PathBuf,
    /// The lease's name, which gives the type: a partial is `video.mp4.part`
    /// and a poster `poster` on disk (code review r1 P2-8).
    pub(crate) name: String,
    /// The whole video's length, as the player is told.
    pub(crate) len: u64,
    /// Bytes on disk from the start, while its download writes; `None` for
    /// a whole file. Closed when the download stops.
    pub(crate) held: Option<watch::Receiver<u64>>,
}

pub(crate) async fn serve(
    local: Arc<LocalFile>,
    range: Option<&str>,
    head_only: bool,
    released: CancellationToken,
    relabel: Relabel,
) -> Response {
    let mut out = HeaderMap::new();
    out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    out.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&local.name)),
    );
    let Some((status, start, end)) = range_headers(range, local.len, &mut out) else {
        return (StatusCode::RANGE_NOT_SATISFIABLE, out).into_response();
    };
    if head_only {
        return (status, out).into_response();
    }
    let mut file = match open(&local.path).await {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("download file unavailable: {e}");
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
        tracing::warn!("download file seek failed: {e}");
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let chunks = async move {
        let relabel = relabel.await;
        chunks(file, local.held.clone(), start, end, relabel)
    }
    .flatten_stream();
    (status, out, pumped(chunks, released)).into_response()
}

/// Opens the file a lease names. A partial download (`video.mp4.part`) that
/// finished since the lease was made has its final name now: the next range
/// of the same play opens that (code review r1 P1-1).
async fn open(path: &std::path::Path) -> std::io::Result<tokio::fs::File> {
    match tokio::fs::File::open(path).await {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match path.to_str().and_then(|p| p.strip_suffix(".part")) {
                Some(done) => tokio::fs::File::open(done).await,
                None => Err(e),
            }
        }
        other => other,
    }
}

/// `[start, end)` of `file` in 256 KiB reads, each waiting until `held`
/// covers it. A download that stops, or holds no more for `PARTIAL_WAIT`,
/// ends the body there: the player's error names the source, and the
/// playback words it "Not downloaded yet".
fn chunks(
    file: tokio::fs::File,
    held: Option<watch::Receiver<u64>>,
    start: u64,
    end: u64,
    relabel: Arc<[Relabelled]>,
) -> impl Stream<Item = std::io::Result<Bytes>> + Send + 'static {
    stream::unfold(Some((file, held, start)), move |state| {
        let relabel = relabel.clone();
        async move {
            let (mut file, mut held, pos) = state?;
            if pos >= end {
                return None;
            }
            let mut want = (end - pos).min(256 * 1024);
            if let Some(rx) = held.as_mut() {
                let covered = tokio::time::timeout(PARTIAL_WAIT, rx.wait_for(|h| *h > pos))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|h| *h);
                // Stopped, or no more in time: what it holds now.
                let have = covered.unwrap_or_else(|| *rx.borrow());
                if have <= pos {
                    return Some((Err(not_downloaded()), None));
                }
                want = want.min(have - pos);
            }
            let mut buf = vec![0u8; want as usize];
            let reading = std::time::Instant::now();
            loop {
                match file.read(&mut buf).await {
                    // The download holds these bytes, the file shows them not
                    // yet (its last write is on its way): read again while a
                    // wait for the download lasts, never an early end (code
                    // review r2 m5).
                    Ok(0) if held.is_some() && reading.elapsed() < PARTIAL_WAIT => {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    Ok(0) => return Some((Err(not_downloaded()), None)),
                    Ok(n) => {
                        buf.truncate(n);
                        relabel_chunk(&mut buf, pos, &relabel);
                        return Some((Ok(Bytes::from(buf)), Some((file, held, pos + n as u64))));
                    }
                    Err(e) => return Some((Err(e), None)),
                }
            }
        }
    })
}

fn not_downloaded() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "not downloaded yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("video.mp4");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    fn local() -> RequestOptions {
        RequestOptions {
            allow_private: true,
            max_bytes: 64 << 20,
            ..Default::default()
        }
    }

    fn http() -> HttpClient {
        HttpClient::new(mq_http::ClientConfig::default()).unwrap()
    }

    /// A whole download plays with ranges, its type from its name.
    #[tokio::test]
    async fn a_whole_file_serves_ranges() {
        let body: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
        let (_dir, path) = temp_file(&body);
        let server = StreamServer::start(http()).await.unwrap();
        let upstream = Upstream::File {
            path,
            len: body.len() as u64,
            held: None,
        };
        let lease = server
            .open(upstream, "video.mp4", Duration::from_secs(5))
            .await
            .unwrap();
        let resp = http()
            .get_range(&lease.url, 1000, 1000, &local())
            .await
            .unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body[..], body[1000..2000]);
        assert_eq!(resp.header("content-type"), Some("video/mp4"));
        let resp = http().get(&lease.url, &local()).await.unwrap();
        assert_eq!((resp.status, resp.body.len()), (200, body.len()));
        assert!(lease.reader().is_some());
    }

    /// A download still being written: a read past what it holds waits for
    /// it, and ends when the download stops.
    #[tokio::test]
    async fn a_partial_file_waits_for_its_download_and_ends_when_it_stops() {
        let body: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
        let (_dir, path) = temp_file(&body[..50_000]);
        let (tx, rx) = watch::channel(50_000u64);
        let server = StreamServer::start(http()).await.unwrap();
        let upstream = Upstream::File {
            path: path.clone(),
            len: body.len() as u64,
            held: Some(rx),
        };
        let lease = server
            .open(upstream, "video.mkv", Duration::from_secs(5))
            .await
            .unwrap();
        // The rest arrives while the player waits.
        let url = lease.url.clone();
        let read =
            tokio::spawn(async move { http().get_range(&url, 40_000, 20_000, &local()).await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        std::fs::write(&path, &body).unwrap();
        tx.send(body.len() as u64).unwrap();
        let resp = read.await.unwrap().unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body[..], body[40_000..60_000]);
        // Shortened again and stopped: a read past it ends at once.
        std::fs::write(&path, &body[..50_000]).unwrap();
        let (tx2, rx2) = watch::channel(50_000u64);
        drop(tx2);
        let lease2 = server
            .open(
                Upstream::File {
                    path,
                    len: body.len() as u64,
                    held: Some(rx2),
                },
                "video.mkv",
                Duration::from_secs(5),
            )
            .await
            .unwrap();
        let started = Instant::now();
        let answer = http()
            .get_range(&lease2.url, 60_000, 10_000, &local())
            .await;
        assert!(answer.is_err() || answer.unwrap().body.is_empty());
        assert!(started.elapsed() < PARTIAL_WAIT);
        drop(tx);
    }

    /// A partial download leased as `video.mp4.part` finishes and takes its
    /// final name mid-play: the next range of the same lease still answers,
    /// and the type comes from the lease's name, not the `.part` on disk.
    #[tokio::test]
    async fn a_partial_that_finishes_mid_play_keeps_answering() {
        let body: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("video.mp4.part");
        std::fs::write(&part, &body).unwrap();
        let (tx, rx) = watch::channel(body.len() as u64);
        let server = StreamServer::start(http()).await.unwrap();
        let upstream = Upstream::File {
            path: part.clone(),
            len: body.len() as u64,
            held: Some(rx),
        };
        let lease = server
            .open(upstream, "video.mp4", Duration::from_secs(5))
            .await
            .unwrap();
        let resp = http()
            .get_range(&lease.url, 0, 1000, &local())
            .await
            .unwrap();
        assert_eq!(resp.header("content-type"), Some("video/mp4"));
        std::fs::rename(&part, dir.path().join("video.mp4")).unwrap();
        drop(tx);
        let resp = http()
            .get_range(&lease.url, 50_000, 1000, &local())
            .await
            .unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body[..], body[50_000..51_000]);
    }

    /// A player reading at the download's tip, where the download already
    /// says it holds bytes the file shows not yet (its last write still on
    /// its way): the read waits for them instead of ending the body early
    /// (code review r2 m5).
    #[tokio::test]
    async fn a_read_at_the_tip_waits_for_bytes_still_on_their_way() {
        let body: Vec<u8> = (0..=255u8).cycle().take(200_000).collect();
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("video.mp4.part");
        std::fs::write(&part, &body[..100_000]).unwrap();
        // The download says it holds all of it; the file has half.
        let (tx, rx) = watch::channel(body.len() as u64);
        let server = StreamServer::start(http()).await.unwrap();
        let upstream = Upstream::File {
            path: part.clone(),
            len: body.len() as u64,
            held: Some(rx),
        };
        let lease = server
            .open(upstream, "video.mp4", Duration::from_secs(5))
            .await
            .unwrap();
        let rest = body[100_000..].to_vec();
        let writer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&part)
                .unwrap();
            std::io::Write::write_all(&mut file, &rest).unwrap();
        });
        let resp = http()
            .get_range(&lease.url, 90_000, 20_000, &local())
            .await
            .unwrap();
        assert_eq!(resp.status, 206);
        assert_eq!(resp.body[..], body[90_000..110_000]);
        writer.await.unwrap();
        drop(tx);
    }
}
