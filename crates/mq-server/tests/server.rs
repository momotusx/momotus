use mq_http::{ClientConfig, HttpClient, RequestOptions, Url};
use mq_server::{Lease, StreamServer, Upstream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What a test origin saw and sent.
#[derive(Clone, Default)]
struct Seen {
    ranges: Arc<Mutex<Vec<String>>>,
    sent: Arc<AtomicU64>,
    /// Requests that carried an `Authorization` header.
    credentials: Arc<AtomicU64>,
    /// Every request head, lowercased.
    heads: Arc<Mutex<Vec<String>>>,
    /// Connections the client closed while a slow start held the answer.
    closed: Arc<AtomicU64>,
    /// Answers still being written.
    open: Arc<AtomicU64>,
}

/// Counts an answer as open while it lives.
struct Open(Arc<AtomicU64>);

impl Drop for Open {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Seen {
    fn ranges(&self) -> Vec<String> {
        self.ranges.lock().unwrap().clone()
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

#[derive(Clone, Copy, Default)]
struct Behaviour {
    /// Answer every request with the whole file and 200.
    ignore_ranges: bool,
    /// Close the connection after this many body bytes.
    cut_after: Option<usize>,
    /// Answer at most this many bytes per range, as some hosts do.
    cap: Option<usize>,
    /// Refuse requests without `x-token: abc`.
    token: bool,
    /// From the second request on, serve another version of the file.
    changed: bool,
    /// From the second request on, keep the length but send no `ETag`.
    validator_lost: bool,
    /// Answer requests for the start this much later.
    slow_start: Option<Duration>,
    /// On the first request only, stop sending after this many body bytes
    /// and keep the connection open, as a stuck origin node does.
    hang_after: Option<usize>,
    /// From the second request on, answer 403, as an expired link does.
    expires: bool,
    /// Answer 401 to requests without Basic credentials, as a NAS does.
    basic: bool,
    /// Redirect every other path to `/video.mkv` on this host, by an
    /// absolute `Location`.
    moved: bool,
}

/// Bytes that differ with their offset, so a misplaced range shows.
fn file(len: usize) -> Arc<Vec<u8>> {
    Arc::new((0..len).map(|i| (i ^ (i >> 8) ^ (i >> 16)) as u8).collect())
}

/// HTTP/1.1 origin over `data` with single byte ranges, one request per
/// connection.
async fn range_origin(data: Arc<Vec<u8>>, how: Behaviour) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Seen::default();
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(answer(socket, data.clone(), how, log.clone()));
        }
    });
    (format!("http://{addr}/video.mkv"), seen)
}

async fn answer(mut socket: TcpStream, data: Arc<Vec<u8>>, how: Behaviour, seen: Seen) {
    seen.open.fetch_add(1, Ordering::SeqCst);
    let _open = Open(seen.open.clone());
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
    seen.heads.lock().unwrap().push(head.clone());
    if head.contains("\r\nauthorization:") {
        seen.credentials.fetch_add(1, Ordering::Relaxed);
    }
    if how.basic && !head.contains("\r\nauthorization: basic ") {
        let _ = socket
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await;
        return;
    }
    if how.moved && !head.starts_with("get /video.mkv ") {
        let host = head
            .lines()
            .find_map(|l| l.strip_prefix("host: "))
            .unwrap_or_default();
        let reply = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://{host}/video.mkv\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = socket.write_all(reply.as_bytes()).await;
        return;
    }
    if how.token && !head.contains("x-token: abc") {
        let _ = socket
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    }
    let range = head
        .lines()
        .find_map(|l| l.strip_prefix("range: bytes="))
        .map(|r| r.trim().to_owned());
    seen.ranges
        .lock()
        .unwrap()
        .push(range.clone().unwrap_or_default());
    if how.expires && seen.ranges().len() > 1 {
        let _ = socket
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    }
    if let Some(delay) = how.slow_start.filter(|_| range.as_deref() == Some("0-")) {
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = socket.read(&mut buf) => {
                seen.closed.fetch_add(1, Ordering::SeqCst);
                return;
            }
        }
    }
    let later = seen.ranges().len() > 1;
    let etag = if how.validator_lost && later {
        String::new()
    } else {
        "ETag: \"v1\"\r\n".to_owned()
    };
    let data = if how.changed && later {
        let mut other = file(data.len() + 1000).to_vec();
        other.reverse();
        Arc::new(other)
    } else if how.validator_lost && later {
        let mut other = data.to_vec();
        other.reverse();
        Arc::new(other)
    } else {
        data
    };
    let len = data.len();
    let span = range.as_deref().filter(|_| !how.ignore_ranges).map(|spec| {
        let (a, b) = spec.split_once('-').unwrap();
        match (a, b) {
            ("", n) => (len - n.parse::<usize>().unwrap(), len),
            (a, "") => (a.parse().unwrap(), len),
            (a, b) => (
                a.parse().unwrap(),
                (b.parse::<usize>().unwrap() + 1).min(len),
            ),
        }
    });
    let (start, mut end) = span.unwrap_or((0, len));
    if start >= len {
        let reply = format!(
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = socket.write_all(reply.as_bytes()).await;
        return;
    }
    if let (Some(cap), Some(_)) = (how.cap, span) {
        end = end.min(start + cap);
    }
    let status = match span {
        Some(_) => format!(
            "206 Partial Content\r\nContent-Range: bytes {start}-{}/{len}",
            end - 1
        ),
        None => "200 OK".into(),
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: video/x-matroska\r\n{etag}Content-Length: {}\r\nConnection: close\r\n\r\n",
        end - start
    );
    if socket.write_all(header.as_bytes()).await.is_err() {
        return;
    }
    let first = seen.ranges().len() == 1;
    let hang = how.hang_after.filter(|_| first);
    let stop = how.cut_after.or(hang).map_or(end, |n| (start + n).min(end));
    for chunk in data[start..stop].chunks(64 * 1024) {
        if socket.write_all(chunk).await.is_err() {
            return;
        }
        seen.sent.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    if hang.is_some() {
        tokio::time::sleep(Duration::from_secs(300)).await;
    }
}

/// An origin that only redirects, as archive.org sends every request to a
/// data node: its `n`th request goes to `to[n]`, the last one from then on.
/// Counts its requests.
async fn redirector(to: Vec<String>) -> (String, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicU64::new(0));
    let count = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let n = count.fetch_add(1, Ordering::Relaxed) as usize;
            let location = to[n.min(to.len() - 1)].clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let reply = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}/download/video.mkv"), hits)
}

/// A playback of `origin` sending an origin-only credential.
async fn serve_url(origin: String) -> (StreamServer, Lease, HttpClient) {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = server.register(
        Upstream::Http {
            url: origin,
            headers: vec![("Authorization".into(), "Bearer origin-only".into())],
            addon_url: Some(local_addon()),
        },
        "video.mkv",
    );
    (server, lease, http)
}

fn local() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        max_bytes: 64 << 20,
        ..Default::default()
    }
}

/// An install on this machine: its playbacks may reach 127.0.0.1.
fn local_addon() -> Url {
    Url::parse("http://127.0.0.1:9/manifest.json").unwrap()
}

async fn serve(data: &Arc<Vec<u8>>, how: Behaviour) -> (StreamServer, Lease, Seen, HttpClient) {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let (origin, seen) = range_origin(data.clone(), how).await;
    let lease = server.register(
        Upstream::Http {
            url: origin,
            headers: vec![("x-token".into(), "abc".into())],
            addon_url: Some(local_addon()),
        },
        "video.mkv",
    );
    (server, lease, seen, http)
}

async fn range(http: &HttpClient, url: &str, offset: u64, len: u64) -> Vec<u8> {
    let resp = http.get_range(url, offset, len, &local()).await.unwrap();
    assert_eq!(resp.status, 206);
    resp.body.to_vec()
}

/// Reads about `bytes` of an open-ended range, then drops it, as players do.
async fn read_some(http: &HttpClient, url: &str, offset: u64, bytes: usize) -> Vec<u8> {
    let mut resp = http
        .stream(url, Some(format!("bytes={offset}-")), &local())
        .await
        .unwrap();
    let mut out = Vec::new();
    while out.len() < bytes {
        let Some(Ok(chunk)) = resp.next_chunk().await else {
            break;
        };
        out.extend_from_slice(&chunk);
    }
    out
}

/// A raw request; returns the whole reply as text up to the body.
async fn raw(server: &StreamServer, url: &str, method: &str, range: Option<&str>) -> String {
    let base = format!("http://{}", server.addr());
    let path = url.trim_start_matches(&base);
    let range = range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default();
    let mut socket = TcpStream::connect(server.addr()).await.unwrap();
    socket
        .write_all(
            format!(
                "{method} {path} HTTP/1.1\r\nHost: {}\r\n{range}Connection: close\r\n\r\n",
                server.addr()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut reply = Vec::new();
    socket.read_to_end(&mut reply).await.unwrap();
    let text = String::from_utf8_lossy(&reply);
    text.split("\r\n\r\n").next().unwrap_or_default().to_owned()
}

#[tokio::test]
async fn proxies_ranges_with_injected_headers_and_releases_tokens() {
    let mut bytes = file(100).to_vec();
    bytes[10..20].copy_from_slice(b"0123456789");
    let data = Arc::new(bytes);
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let (origin, _) = range_origin(
        data,
        Behaviour {
            token: true,
            ..Default::default()
        },
    )
    .await;
    let lease = server.register(
        Upstream::Http {
            url: origin,
            headers: vec![("x-token".into(), "abc".into())],
            addon_url: Some(local_addon()),
        },
        "Show S01E01 2160p.mkv",
    );
    let url = lease.url.clone();
    assert!(url.starts_with("http://127.0.0.1:"));
    assert!(url.ends_with("/Show_S01E01_2160p.mkv"));

    let resp = http.get_range(&url, 10, 10, &local()).await.unwrap();
    assert_eq!(resp.status, 206);
    assert_eq!(&resp.body[..], b"0123456789");
    assert_eq!(resp.header("content-range"), Some("bytes 10-19/100"));
    assert_eq!(resp.header("access-control-allow-origin"), Some("*"));
    assert_eq!(resp.header("content-type"), Some("video/x-matroska"));

    drop(lease);
    let err = http.get(&url, &local()).await.unwrap_err();
    assert!(
        matches!(err, mq_http::HttpError::Status { status: 404, .. }),
        "{err:?}"
    );
}

/// AVPlayer's opening for an MP4 with its index first: two bytes, the start
/// (dropped after a while), then the start again a little further on.
#[tokio::test]
async fn player_ranges_reuse_one_upstream() {
    let data = file(8 << 20);
    let (_server, lease, seen, http) = serve(&data, Behaviour::default()).await;
    let url = lease.url.clone();
    assert_eq!(range(&http, &url, 0, 2).await, data[..2]);
    let start = read_some(&http, &url, 0, 1 << 20).await;
    assert_eq!(start[..], data[..start.len()]);
    assert_eq!(
        range(&http, &url, 8192, 512 << 10).await,
        data[8192..8192 + (512 << 10)]
    );
    assert_eq!(seen.ranges(), ["0-"], "one upstream request");
}

/// The index at the end, then the ranges inside it a player reads next.
#[tokio::test]
async fn tail_sub_ranges_come_from_the_cache() {
    let data = file((8 << 20) + 12_345);
    let len = data.len() as u64;
    let (_server, lease, seen, http) = serve(&data, Behaviour::default()).await;
    let url = lease.url.clone();
    let tail = len - (200 << 10);
    assert_eq!(
        range(&http, &url, tail, 200 << 10).await,
        data[tail as usize..]
    );
    let a = len - 16_000;
    assert_eq!(range(&http, &url, a, 16_000).await, data[a as usize..]);
    let b = len - 90_000;
    assert_eq!(
        range(&http, &url, b, 70_000).await,
        data[b as usize..b as usize + 70_000]
    );
    let aligned = tail - tail % (64 << 10);
    assert_eq!(seen.ranges(), ["0-".to_owned(), format!("{aligned}-")]);
}

#[tokio::test]
async fn every_range_is_byte_exact() {
    let data = file((3 << 20) + 777);
    let len = data.len() as u64;
    let (server, lease, _seen, http) = serve(&data, Behaviour::default()).await;
    let url = lease.url.clone();
    for (offset, n) in [
        (0, 1),
        (65_535, 2),
        (65_536, 65_536),
        (1_000_003, 1_234_567),
        (len - 1, 1),
        (len - 100_000, 100_000),
        (12_345, 3_000_000),
    ] {
        let got = range(&http, &url, offset, n).await;
        assert_eq!(
            got,
            data[offset as usize..(offset + n) as usize],
            "{offset}+{n}"
        );
    }
    let whole = http.get(&url, &local()).await.unwrap();
    assert_eq!(whole.status, 200);
    assert_eq!(whole.body[..], data[..]);

    let head = raw(&server, &url, "HEAD", None).await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains(&format!("content-length: {len}")), "{head}");
    let suffix = raw(&server, &url, "HEAD", Some("bytes=-500")).await;
    assert!(
        suffix.contains(&format!(
            "content-range: bytes {}-{}/{len}",
            len - 500,
            len - 1
        )),
        "{suffix}"
    );
    let past = raw(&server, &url, "GET", Some(&format!("bytes={len}-"))).await;
    assert!(past.starts_with("HTTP/1.1 416"), "{past}");
    assert!(
        past.contains(&format!("content-range: bytes */{len}")),
        "{past}"
    );
}

#[tokio::test]
async fn a_dropped_upstream_resumes_where_it_stopped() {
    let data = file(2 << 20);
    let how = Behaviour {
        cut_after: Some(300 << 10),
        ..Default::default()
    };
    let (_server, lease, seen, http) = serve(&data, how).await;
    let url = lease.url.clone();
    assert_eq!(
        range(&http, &url, 1000, (2 << 20) - 1000).await,
        data[1000..]
    );
    assert!(seen.ranges().len() > 1, "{:?}", seen.ranges());
}

#[tokio::test]
async fn a_cursor_reads_ahead_only_so_far() {
    let data = file(64 << 20);
    let (_server, lease, seen, http) = serve(&data, Behaviour::default()).await;
    let url = lease.url.clone();
    let mut resp = http
        .stream(&url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    let first = resp.next_chunk().await.unwrap().unwrap();
    assert_eq!(first[..], data[..first.len()]);
    tokio::time::sleep(Duration::from_millis(700)).await;
    let sent = seen.sent.load(Ordering::Relaxed);
    println!("origin sent {sent} bytes");
    assert!(
        sent < 16 << 20,
        "the origin sent {sent} bytes for one chunk"
    );
}

#[tokio::test]
async fn an_origin_without_ranges_still_plays() {
    let data = file(16 << 20);
    let how = Behaviour {
        ignore_ranges: true,
        ..Default::default()
    };
    let (_server, lease, _seen, http) = serve(&data, how).await;
    let url = lease.url.clone();
    // The origin can only send the whole file; the proxy hands that over as
    // is, near the start and far from it.
    for offset in [100, 12 << 20] {
        let resp = http.get_range(&url, offset, 1000, &local()).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body[..], data[..]);
    }
}

#[tokio::test]
async fn rejects_unknown_tokens_and_foreign_hosts() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let base = format!("http://{}", server.addr());
    let err = http
        .get(&format!("{base}/s/deadbeef/x.mkv"), &local())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        mq_http::HttpError::Status { status: 404, .. }
    ));

    let (origin, _) = range_origin(file(100), Behaviour::default()).await;
    let lease = server.register(
        Upstream::Http {
            url: origin,
            headers: vec![],
            addon_url: Some(local_addon()),
        },
        "x.mkv",
    );
    let url = lease.url.clone();
    let path = url.trim_start_matches(&base).to_owned();
    // A rebinding page would reach us with its own Host header.
    let mut socket = tokio::net::TcpStream::connect(server.addr()).await.unwrap();
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut reply = String::new();
    socket.read_to_string(&mut reply).await.unwrap();
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
}

/// Origin serving one Russian SRT in windows-1251, as many subtitle sites do.
/// Counts its requests.
async fn subtitle_origin() -> (String, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(AtomicU64::new(0));
    let count = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let (srt, _, _) = encoding_rs::WINDOWS_1251
                    .encode("1\r\n00:00:01,000 --> 00:00:02,000\r\nПривет\r\n");
                let mut resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
                    srt.len()
                )
                .into_bytes();
                resp.extend_from_slice(&srt);
                let _ = socket.write_all(&resp).await;
            });
        }
    });
    (format!("http://{addr}/subs/12345"), asked)
}

#[tokio::test]
async fn subtitles_come_out_as_utf8_webvtt_for_any_page() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let (origin, _) = subtitle_origin().await;
    let lease = server.register(local_upstream(PLAYBACK.into()), "v.mkv");
    let slot = lease.subtitles();
    let url = slot.add(&origin, "rus", Some(&local_addon())).unwrap();
    assert!(url.ends_with("/rus.vtt"), "{url}");
    assert_eq!(
        slot.add(&origin, "rus", Some(&local_addon())),
        Some(url.clone()),
        "one address per file"
    );

    let resp = http.get(&url, &local()).await.unwrap();
    assert_eq!(resp.header("content-type"), Some("text/vtt; charset=utf-8"));
    assert_eq!(resp.header("access-control-allow-origin"), Some("*"));
    let text = String::from_utf8(resp.body.to_vec()).unwrap();
    assert!(text.starts_with("WEBVTT"), "{text}");
    assert!(
        text.contains("00:00:01.000 --> 00:00:02.000\nПривет"),
        "{text}"
    );

    drop(lease);
    assert!(http.get(&url, &local()).await.is_err(), "released");
}

/// Live: time range requests through the loopback server against an origin
/// that redirects (archive.org). Run with `--ignored --nocapture`.
#[tokio::test]
#[ignore = "network"]
async fn live_redirecting_origin_range_times() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = server.register(
        Upstream::Http {
            url: "https://archive.org/download/BigBuckBunny_124/Content/big_buck_bunny_720p_surround.mp4".into(),
            headers: vec![],
            addon_url: None,
        },
        "bbb.mp4",
    );
    let url = lease.url.clone();
    let mut times = Vec::new();
    for i in 0..6u64 {
        let started = std::time::Instant::now();
        let resp = http
            .get_range(&url, i * 4_000_000, 65_536, &local())
            .await
            .unwrap();
        assert_eq!(resp.status, 206);
        times.push(started.elapsed().as_millis());
    }
    println!("range request ms: {times:?}");
}

/// A seek on a redirecting origin goes straight to where the first read
/// landed, and the origin's credential never follows it to another host.
#[tokio::test]
async fn a_seek_skips_the_origins_redirect() {
    let data = file(16 << 20);
    let (target, seen) = range_origin(data.clone(), Behaviour::default()).await;
    let (origin, redirects) = redirector(vec![target]).await;
    let (_server, lease, http) = serve_url(origin).await;
    let url = lease.url.clone();
    assert_eq!(range(&http, &url, 0, 2).await, data[..2]);
    let far = 12usize << 20;
    assert_eq!(
        range(&http, &url, far as u64, 1000).await,
        data[far..far + 1000]
    );
    assert_eq!(seen.ranges(), ["0-".to_owned(), format!("{far}-")]);
    assert_eq!(
        redirects.load(Ordering::Relaxed),
        1,
        "one redirect for both reads"
    );
    assert_eq!(seen.credentials.load(Ordering::Relaxed), 0);
}

/// A remembered target that refuses (an expired link) sends the read back
/// to the original URL, whose fresh redirect serves it.
#[tokio::test]
async fn an_expired_redirect_target_falls_back_to_the_original_url() {
    let data = file(16 << 20);
    let expiring = Behaviour {
        expires: true,
        ..Default::default()
    };
    let (old, old_seen) = range_origin(data.clone(), expiring).await;
    let (new, new_seen) = range_origin(data.clone(), Behaviour::default()).await;
    let (origin, redirects) = redirector(vec![old, new]).await;
    let (_server, lease, http) = serve_url(origin).await;
    let url = lease.url.clone();
    assert_eq!(range(&http, &url, 0, 2).await, data[..2]);
    let far = 12usize << 20;
    assert_eq!(
        range(&http, &url, far as u64, 1000).await,
        data[far..far + 1000]
    );
    assert_eq!(
        old_seen.ranges().len(),
        2,
        "the remembered target is asked first"
    );
    assert_eq!(redirects.load(Ordering::Relaxed), 2);
    assert_eq!(new_seen.ranges(), [format!("{far}-")]);
}

#[tokio::test]
async fn a_host_that_caps_its_answers_is_proxied_plainly() {
    let data = file(2 << 20);
    let how = Behaviour {
        cap: Some(100_000),
        ..Default::default()
    };
    let (_server, lease, _seen, http) = serve(&data, how).await;
    let url = lease.url.clone();
    let resp = http.get_range(&url, 5000, 300_000, &local()).await.unwrap();
    assert_eq!(resp.status, 206);
    assert_eq!(
        resp.header("content-range"),
        Some("bytes 5000-104999/2097152")
    );
    assert_eq!(resp.body[..], data[5000..105_000]);
}

#[tokio::test]
async fn release_stops_an_open_response() {
    let data = file(64 << 20);
    let (_server, lease, _seen, http) = serve(&data, Behaviour::default()).await;
    let url = lease.url.clone();
    let mut resp = http
        .stream(&url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    resp.next_chunk().await.unwrap().unwrap();
    drop(lease);
    let mut got = 0;
    let drained = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(chunk)) = resp.next_chunk().await {
            got += chunk.len();
        }
    })
    .await;
    assert!(drained.is_ok(), "the response kept going");
    assert!(got < 32 << 20, "{got} bytes after release");
}

#[tokio::test]
async fn release_stops_a_plain_proxied_response() {
    let data = file(64 << 20);
    let how = Behaviour {
        ignore_ranges: true,
        ..Default::default()
    };
    let (_server, lease, _seen, http) = serve(&data, how).await;
    let url = lease.url.clone();
    let mut resp = http
        .stream(&url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    resp.next_chunk().await.unwrap().unwrap();
    drop(lease);
    let mut got = 0;
    let drained = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(chunk)) = resp.next_chunk().await {
            got += chunk.len();
            // A slow reader, so the whole file cannot simply arrive first.
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await;
    assert!(drained.is_ok(), "the response kept going");
    assert!(got < 32 << 20, "{got} bytes after release");
}

#[tokio::test]
async fn another_version_of_the_file_never_joins_the_cache() {
    let data = file(8 << 20);
    let how = Behaviour {
        changed: true,
        ..Default::default()
    };
    let (_server, lease, seen, http) = serve(&data, how).await;
    let url = lease.url.clone();
    let tail = (8 << 20) - 1000;
    if let Ok(resp) = http.get_range(&url, tail, 1000, &local()).await {
        // Only bytes of the version the headers describe, or no answer.
        assert_eq!(resp.body[..], data[tail as usize..], "mixed versions");
    }
    assert!(seen.ranges().len() > 1);
}

#[tokio::test]
async fn a_cold_request_past_the_end_names_the_length() {
    let data = file(1 << 20);
    let how = Behaviour {
        slow_start: Some(Duration::from_millis(500)),
        ..Default::default()
    };
    let (server, lease, _seen, _http) = serve(&data, how).await;
    let url = lease.url.clone();
    let reply = raw(&server, &url, "GET", Some("bytes=5000000-")).await;
    assert!(reply.starts_with("HTTP/1.1 416"), "{reply}");
    assert!(reply.contains("content-range: bytes */1048576"), "{reply}");
}

#[tokio::test]
async fn a_validator_that_disappears_means_another_version() {
    let data = file(8 << 20);
    let how = Behaviour {
        validator_lost: true,
        ..Default::default()
    };
    let (_server, lease, seen, http) = serve(&data, how).await;
    let url = lease.url.clone();
    let tail = (8 << 20) - 1000;
    if let Ok(resp) = http.get_range(&url, tail, 1000, &local()).await {
        assert_eq!(resp.body[..], data[tail as usize..], "mixed versions");
    }
    assert!(seen.ranges().len() > 1);
}

/// Opening checks the origin with the read that goes on to serve the
/// player: one origin request, and the player's first bytes need no wait.
#[tokio::test]
async fn open_checks_the_origin_with_the_read_that_serves_the_player() {
    let data = file(1 << 20);
    let delay = Duration::from_millis(400);
    let how = Behaviour {
        slow_start: Some(delay),
        ..Default::default()
    };
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let (origin, seen) = range_origin(data.clone(), how).await;
    let upstream = |url: &str| Upstream::Http {
        url: url.to_owned(),
        headers: vec![],
        addon_url: Some(local_addon()),
    };
    let started = std::time::Instant::now();
    let lease = server
        .open(upstream(&origin), "v.mkv", Duration::from_secs(5))
        .await
        .unwrap();
    let url = lease.url.clone();
    assert!(started.elapsed() >= delay);
    let first = std::time::Instant::now();
    assert_eq!(range(&http, &url, 0, 1000).await, data[..1000]);
    let wait = first.elapsed();
    assert!(wait < delay / 2, "first bytes took {wait:?}");
    assert_eq!(seen.ranges(), ["0-"], "no separate liveness request");

    // A refusing origin and a dead port fail at open and are released.
    let (refusing, _) = range_origin(
        data,
        Behaviour {
            token: true,
            ..Default::default()
        },
    )
    .await;
    let err = server
        .open(upstream(&refusing), "v.mkv", Duration::from_secs(5))
        .await
        .unwrap_err();
    assert_eq!(err, "The source refused access (403)");
    let err = server
        .open(
            upstream("http://127.0.0.1:9/v.mkv"),
            "v.mkv",
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert_eq!(err, "The source could not be reached");
}

/// An origin node that stops feeding an open connection must not hold the
/// player for the long network timeouts: the stuck read is replaced.
#[tokio::test]
async fn a_stuck_upstream_is_replaced_quickly() {
    let data = file(4 << 20);
    let how = Behaviour {
        hang_after: Some(1 << 20),
        ..Default::default()
    };
    let (_server, lease, seen, http) = serve(&data, how).await;
    let url = lease.url.clone();
    let started = std::time::Instant::now();
    assert_eq!(range(&http, &url, 0, 3 << 20).await, data[..3 << 20]);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(12), "took {took:?}");
    assert!(seen.ranges().len() >= 2, "{:?}", seen.ranges());
}

// C23 in the server (plans/c21-c23-loopback.md 3.1, 3.4).

/// A listener on a not-public address no scope here allows, counting the
/// connections it accepts: `127.0.0.3` on Linux, `[::1]` on macOS (lo0 has
/// only 127.0.0.1 and ::1). `port` 0 picks one.
struct Canary {
    addr: std::net::SocketAddr,
    accepted: Arc<AtomicU64>,
}

impl Canary {
    async fn start(port: u16) -> Canary {
        let ip: std::net::IpAddr = if cfg!(target_os = "linux") {
            std::net::Ipv4Addr::new(127, 0, 0, 3).into()
        } else {
            std::net::Ipv6Addr::LOCALHOST.into()
        };
        let listener = TcpListener::bind((ip, port))
            .await
            .expect("the canary must bind");
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicU64::new(0));
        let count = accepted.clone();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                drop(socket);
            }
        });
        Canary { addr, accepted }
    }

    fn accepted(&self) -> u64 {
        self.accepted.load(Ordering::SeqCst)
    }
}

/// An origin that answers its `n`th request with `replies[n]`, the last one
/// from then on, one request per connection.
async fn scripted(replies: Vec<String>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = Arc::new(AtomicU64::new(0));
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let n = served.fetch_add(1, Ordering::SeqCst) as usize;
            let reply = replies[n.min(replies.len() - 1)].clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}/v.mkv")
}

/// A raw request; returns the reply's head and body.
async fn raw_full(server: &StreamServer, url: &str, range: &str) -> (String, Vec<u8>) {
    let base = format!("http://{}", server.addr());
    let path = url.trim_start_matches(&base);
    let mut socket = TcpStream::connect(server.addr()).await.unwrap();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nRange: {range}\r\nConnection: close\r\n\r\n",
        server.addr()
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    socket.read_to_end(&mut reply).await.unwrap();
    let end = reply.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&reply[..end]).to_ascii_lowercase();
    (head, reply[end + 4..].to_vec())
}

/// A playback whose bytes are never read: a home for subtitle tracks.
const PLAYBACK: &str = "http://127.0.0.1:9/v.mkv";

fn local_upstream(url: String) -> Upstream {
    Upstream::Http {
        url,
        headers: vec![],
        addon_url: Some(local_addon()),
    }
}

const REDIRECT_WITHOUT_LOCATION: &str =
    "HTTP/1.1 302 Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
/// The whole file for a range request: the origin is proxied plainly.
const WHOLE_FILE: &str = "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd";

#[tokio::test]
async fn the_player_never_sees_a_redirect() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    // The first answer of a registered playback (the cache path).
    let origin = scripted(vec![REDIRECT_WITHOUT_LOCATION.into()]).await;
    let lease = server.register(local_upstream(origin), "v.mkv");
    let url = lease.url.clone();
    let (head, _) = raw_full(&server, &url, "bytes=0-").await;
    assert!(head.starts_with("http/1.1 502"), "{head}");
    assert!(!head.contains("location"), "{head}");
    // A later answer of an origin proxied plainly.
    let origin = scripted(vec![WHOLE_FILE.into(), REDIRECT_WITHOUT_LOCATION.into()]).await;
    let lease = server.register(local_upstream(origin), "v.mkv");
    let url = lease.url.clone();
    let (head, _) = raw_full(&server, &url, "bytes=0-").await;
    assert!(head.starts_with("http/1.1 502"), "{head}");
    assert!(!head.contains("location"), "{head}");
    // A child of an HLS playback.
    let (base, _) = hls_origin(|asked| match asked.path.as_str() {
        "/m.m3u8" => Reply::playlist("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\ns.ts\n"),
        _ => Reply::status(302),
    })
    .await;
    let lease = open_hls(&server, format!("{base}/m.m3u8")).await;
    let playlist = text(&http.get(&lease.url, &local()).await.unwrap());
    let (head, _) = raw_full(&server, &named_urls(&playlist)[0], "bytes=0-").await;
    assert!(head.starts_with("http/1.1 502"), "{head}");
    assert!(!head.contains("location"), "{head}");
}

#[tokio::test]
async fn an_error_page_is_not_passed_on() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let page = "HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: 20\r\nConnection: close\r\n\r\n<html>denied</html>\n";
    let origin = scripted(vec![WHOLE_FILE.into(), page.into()]).await;
    let lease = server.register(local_upstream(origin), "v.mkv");
    let url = lease.url.clone();
    let (head, body) = raw_full(&server, &url, "bytes=0-").await;
    assert!(head.starts_with("http/1.1 403"), "{head}");
    assert!(body.is_empty(), "{}", String::from_utf8_lossy(&body));
    // A read past the end keeps the length players end the file by.
    let past = "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */4\r\nContent-Type: text/html\r\nContent-Length: 20\r\nConnection: close\r\n\r\n<html>beyond</html>\n";
    let origin = scripted(vec![WHOLE_FILE.into(), past.into()]).await;
    let lease = server.register(local_upstream(origin), "v.mkv");
    let (head, body) = raw_full(&server, &lease.url, "bytes=4-").await;
    assert!(head.starts_with("http/1.1 416"), "{head}");
    assert!(head.contains("\r\ncontent-range: bytes */4"), "{head}");
    assert!(body.is_empty(), "{}", String::from_utf8_lossy(&body));
}

#[tokio::test]
async fn the_plain_proxy_keeps_the_playbacks_scope() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let canary = Canary::start(0).await;
    mq_http::test_dns::set("proxy-canary.c23.test", &[canary.addr.ip()]);
    let port = canary.addr.port();
    for target in [
        format!("http://{}/x", canary.addr),
        format!("http://proxy-canary.c23.test:{port}/x"),
    ] {
        // The first answer makes the playback a plain proxy; the player's
        // request then meets the redirect.
        let redirect = format!(
            "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let origin = scripted(vec![WHOLE_FILE.into(), redirect]).await;
        let lease = server.register(local_upstream(origin), "v.mkv");
        let (head, _) = raw_full(&server, &lease.url, "bytes=0-").await;
        assert!(head.starts_with("http/1.1 502"), "{target}: {head}");
    }
    assert_eq!(canary.accepted(), 0);
}

#[tokio::test]
async fn subtitles_obey_their_addons_scope() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let canary = Canary::start(0).await;
    mq_http::test_dns::set("sub-canary.c23.test", &[canary.addr.ip()]);
    let port = canary.addr.port();
    let (redirect, _) = redirector(vec![format!("http://{}/b.srt", canary.addr)]).await;
    let lease = server.register(local_upstream(PLAYBACK.into()), "v.mkv");
    let slot = lease.subtitles();
    for track in [
        // A local install's track on another private address, by literal
        // and by name.
        format!("http://{}/a.srt", canary.addr),
        format!("http://sub-canary.c23.test:{port}/a.srt"),
        // A track on the install's own host that redirects off it.
        redirect,
    ] {
        let url = slot.add(&track, "eng", Some(&local_addon())).unwrap();
        let err = http.get(&url, &local()).await.unwrap_err();
        assert!(
            matches!(err, mq_http::HttpError::Status { status: 502, .. }),
            "{track}: {err:?}"
        );
    }
    assert_eq!(canary.accepted(), 0);
}

#[tokio::test]
async fn a_redirect_target_that_turns_private_is_refused() {
    let data = file(16 << 20);
    let (target, _seen) = range_origin(data.clone(), Behaviour::default()).await;
    let port = Url::parse(&target).unwrap().port().unwrap();
    // The canary listens on the target's port, where the name will point.
    let canary = Canary::start(port).await;
    mq_http::test_dns::set("memo.c23.test", &["127.0.0.1".parse().unwrap()]);
    let (origin, redirects) =
        redirector(vec![format!("http://memo.c23.test:{port}/video.mkv")]).await;
    let (_server, lease, http) = serve_url(origin).await;
    let url = lease.url.clone();
    assert_eq!(range(&http, &url, 0, 2).await, data[..2]);
    assert_eq!(redirects.load(Ordering::Relaxed), 1, "one read so far");
    // The remembered target's name now answers the canary.
    mq_http::test_dns::set("memo.c23.test", &[canary.addr.ip()]);
    let far = 12u64 << 20;
    let read = http.get_range(&url, far, 1000, &local()).await;
    assert!(read.is_err(), "{read:?}");
    assert!(
        redirects.load(Ordering::Relaxed) >= 2,
        "the fallback asked the original URL"
    );
    assert_eq!(canary.accepted(), 0);
}

/// Userinfo in a stream URL goes with every read, a seek's too, and to a
/// redirect target on the same origin, so no read pays a 401 first.
#[tokio::test]
async fn url_credentials_go_with_every_read() {
    let data = file(16 << 20);
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    // The file itself, then a download path that redirects to it.
    for (moved, requests) in [(false, 2), (true, 3)] {
        let how = Behaviour {
            basic: true,
            moved,
            ..Default::default()
        };
        let (origin, seen) = range_origin(data.clone(), how).await;
        let mut origin = origin.replace("http://", "http://u:p@");
        if moved {
            origin = origin.replace("/video.mkv", "/download");
        }
        let lease = server.register(local_upstream(origin), "video.mkv");
        assert_eq!(range(&http, &lease.url, 0, 2).await, data[..2]);
        let far = 12usize << 20;
        assert_eq!(
            range(&http, &lease.url, far as u64, 1000).await,
            data[far..far + 1000]
        );
        let heads = seen.heads();
        assert_eq!(heads.len(), requests, "{heads:?}");
        assert!(heads.iter().all(|h| h.contains("authorization: basic ")));
    }
}

#[tokio::test]
async fn secrets_stay_with_their_origin() {
    let data = file(16 << 20);
    let (target, seen) = range_origin(data.clone(), Behaviour::default()).await;
    let (origin, redirects) = redirector(vec![target]).await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let headers = [
        ("X-Api-Key", "k1"),
        ("Authorization", "Bearer t1"),
        ("Referer", "https://site.example/"),
    ]
    .map(|(n, v)| (n.to_owned(), v.to_owned()))
    .to_vec();
    let lease = server.register(
        Upstream::Http {
            url: origin,
            headers,
            addon_url: Some(local_addon()),
        },
        "video.mkv",
    );
    let url = lease.url.clone();
    assert_eq!(range(&http, &url, 0, 2).await, data[..2]);
    // A seek: the second read goes to the remembered target directly.
    let far = 12usize << 20;
    assert_eq!(
        range(&http, &url, far as u64, 1000).await,
        data[far..far + 1000]
    );
    assert_eq!(redirects.load(Ordering::Relaxed), 1);
    let heads = seen.heads();
    assert_eq!(heads.len(), 2, "{heads:?}");
    for head in heads {
        assert!(head.contains("referer: https://site.example/"), "{head}");
        assert!(
            !head.contains("x-api-key") && !head.contains("authorization"),
            "{head}"
        );
    }
}

// C21: one token per playback source (plans/c21-c23-loopback.md 3.2).

/// The token in a playback URL.
fn token(url: &str) -> &str {
    url.split('/').nth(4).unwrap()
}

/// An origin that sends its headers, then one byte every 100 ms. Counts its
/// requests.
async fn trickle() -> (String, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(AtomicU64::new(0));
    let count = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let count = count.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                count.fetch_add(1, Ordering::SeqCst);
                let head = "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(head.as_bytes()).await;
                for _ in 0..1000 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if socket.write_all(b"x").await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (format!("http://{addr}/slow.srt"), asked)
}

/// Waits, at most 5 s, until `count` is at least 1.
async fn asked_once(count: &AtomicU64) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while count.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the origin was never asked");
}

#[tokio::test]
async fn a_released_playback_answers_404_everywhere() {
    let data = file(1 << 20);
    let (server, lease, _seen, http) = serve(&data, Behaviour::default()).await;
    let (subs, _) = subtitle_origin().await;
    let sub = lease
        .subtitles()
        .add(&subs, "rus", Some(&local_addon()))
        .unwrap();
    let url = lease.url.clone();
    assert_eq!(range(&http, &url, 0, 10).await, data[..10]);
    let head = raw(&server, &url, "HEAD", None).await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(http.get(&sub, &local()).await.unwrap().status, 200);

    // An HLS playback and one of its children.
    let (base, _) = hls_origin(pages(vec![
        (
            "/m.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\ns.ts\n".to_vec(),
        ),
        ("/s.ts", "video/mp2t", b"segment".to_vec()),
    ]))
    .await;
    let hls = open_hls(&server, format!("{base}/m.m3u8")).await;
    let main = hls.url.clone();
    let child = named_urls(&text(&http.get(&main, &local()).await.unwrap())).remove(0);
    for (url, method) in [(&main, "HEAD"), (&child, "GET"), (&child, "HEAD")] {
        let reply = raw(&server, url, method, None).await;
        assert!(reply.starts_with("HTTP/1.1 200"), "{method} {reply}");
    }

    drop((lease, hls));
    for (url, method) in [
        (&url, "GET"),
        (&url, "HEAD"),
        (&sub, "GET"),
        (&main, "GET"),
        (&child, "GET"),
        (&child, "HEAD"),
    ] {
        let reply = raw(&server, url, method, None).await;
        assert!(reply.starts_with("HTTP/1.1 404"), "{method} {url}: {reply}");
    }
}

#[tokio::test]
async fn ids_reach_only_their_own_playback() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let (subs, asked) = subtitle_origin().await;
    let a = server.register(local_upstream(PLAYBACK.into()), "a.mkv");
    let b = server.register(local_upstream(PLAYBACK.into()), "b.mkv");
    let track = a
        .subtitles()
        .add(&subs, "rus", Some(&local_addon()))
        .unwrap();
    let base = |lease: &Lease| format!("http://{}/sub/{}", server.addr(), token(&lease.url));
    // A's id under B's token, and forms of it under A's own.
    let foreign = [(&b, "0"), (&a, "+0"), (&a, "00"), (&a, "-1"), (&a, "1")];
    for (lease, id) in foreign {
        let reply = raw(
            &server,
            &format!("{}/{id}/rus.vtt", base(lease)),
            "GET",
            None,
        )
        .await;
        assert!(reply.starts_with("HTTP/1.1 404"), "{id}: {reply}");
    }
    assert_eq!(asked.load(Ordering::SeqCst), 0, "A's origin was asked");
    assert_eq!(http.get(&track, &local()).await.unwrap().status, 200);
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    // A's playlist children under B's token: a file's, and another
    // playlist's with fewer children.
    let one = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\ns.ts\n";
    let three =
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\na.ts\n#EXTINF:4,\nb.ts\n#EXTINF:4,\nc.ts\n";
    let (a_base, a_log) = hls_origin(move |_| Reply::playlist(three)).await;
    let (b_base, _) = hls_origin(move |_| Reply::playlist(one)).await;
    let a = open_hls(&server, format!("{a_base}/a.m3u8")).await;
    let b = open_hls(&server, format!("{b_base}/b.m3u8")).await;
    let file = server.register(local_upstream(PLAYBACK.into()), "f.mkv");
    let before = a_log.lock().unwrap().len();
    for other in [&b, &file] {
        let prefix = format!("http://{}/c/{}", server.addr(), token(&other.url));
        for id in ["1", "2"] {
            let reply = raw(&server, &format!("{prefix}/{id}/b.ts"), "GET", None).await;
            assert!(reply.starts_with("HTTP/1.1 404"), "{id}: {reply}");
        }
    }
    assert_eq!(a_log.lock().unwrap().len(), before, "A's origin was asked");
    drop(a);
}

#[tokio::test]
async fn an_open_dropped_before_its_answer_leaves_no_token() {
    let how = Behaviour {
        slow_start: Some(Duration::from_secs(5)),
        ..Default::default()
    };
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let (origin, seen) = range_origin(file(1 << 20), how).await;
    let open = server.open(local_upstream(origin), "v.mkv", Duration::from_secs(10));
    let gave_up = tokio::time::timeout(Duration::from_millis(500), open).await;
    assert!(gave_up.is_err(), "the origin answered early");
    assert_eq!(server.playbacks(), 0);
    // The read that was waiting for the answer is gone too.
    asked_once(&seen.closed).await;
}

#[tokio::test]
async fn subtitle_work_keeps_its_playback() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let (slow, asked) = trickle().await;
    let first = server.register(local_upstream(PLAYBACK.into()), "v.mkv");
    let slot = first.subtitles();
    let sub = slot.add(&slow, "eng", Some(&local_addon())).unwrap();
    // A subtitle fetch in flight holds the first playback.
    let fetch = tokio::spawn({
        let http = http.clone();
        async move { http.get(&sub, &local()).await }
    });
    asked_once(&asked).await;
    drop(first);
    assert_eq!(slot.add(&slow, "tur", Some(&local_addon())), None);
    let _ = fetch.await;
    let second = server.register(local_upstream(PLAYBACK.into()), "v.mkv");
    let base = format!("http://{}/sub/{}", server.addr(), token(&second.url));
    for id in ["0", "1"] {
        let reply = raw(&server, &format!("{base}/{id}/tur.vtt"), "GET", None).await;
        assert!(reply.starts_with("HTTP/1.1 404"), "{id}: {reply}");
    }
}

#[tokio::test]
async fn release_ends_work_in_flight() {
    let data = file(64 << 20);
    let (server, lease, _seen, http) = serve(&data, Behaviour::default()).await;
    // A media playlist whose origin trickles it: its collection is in flight.
    let collecting = Arc::new(AtomicU64::new(0));
    let count = collecting.clone();
    let (base, _) = hls_origin(move |asked| {
        if asked.path == "/master.m3u8" {
            return Reply::playlist("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nslow.m3u8\n");
        }
        count.fetch_add(1, Ordering::SeqCst);
        Reply {
            status: 200,
            headers: vec![("Content-Type", "application/vnd.apple.mpegurl".into())],
            parts: b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n"
                .iter()
                .map(|b| vec![*b])
                .collect(),
            gap: Duration::from_millis(200),
            ..Reply::default()
        }
    })
    .await;
    let hls = open_hls(&server, format!("{base}/master.m3u8")).await;
    let variant = named_urls(&text(&http.get(&hls.url, &local()).await.unwrap())).remove(0);
    let collect = tokio::spawn({
        let http = http.clone();
        async move { http.get(&variant, &local()).await }
    });
    asked_once(&collecting).await;
    let (slow, asked) = trickle().await;
    let sub = lease
        .subtitles()
        .add(&slow, "eng", Some(&local_addon()))
        .unwrap();
    let mut body = http
        .stream(&lease.url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    body.next_chunk().await.unwrap().unwrap();
    let fetch = tokio::spawn({
        let http = http.clone();
        async move { http.get(&sub, &local()).await }
    });
    asked_once(&asked).await;
    drop((lease, hls));
    let ended = tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(Ok(_)) = body.next_chunk().await {}
        (fetch.await.unwrap(), collect.await.unwrap())
    })
    .await
    .expect("work went on after release");
    for ended in [ended.0, ended.1] {
        assert!(
            matches!(ended, Err(mq_http::HttpError::Status { status: 404, .. })),
            "{ended:?}"
        );
    }
}

#[tokio::test]
async fn dropping_the_server_ends_keep_alive_connections() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let addr = server.addr();
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "GET /s/{}/v.mkv HTTP/1.1\r\nHost: {addr}\r\n\r\n",
        "0".repeat(64)
    );
    let mut buf = [0u8; 1024];
    socket.write_all(request.as_bytes()).await.unwrap();
    let n = socket.read(&mut buf).await.unwrap();
    let first = String::from_utf8_lossy(&buf[..n]).into_owned();
    assert!(first.starts_with("HTTP/1.1 404"), "{first}");

    drop(server);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = socket.write_all(request.as_bytes()).await;
    let second = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut buf))
        .await
        .expect("the connection stayed open");
    if let Ok(n @ 1..) = second {
        panic!(
            "answered after the drop: {}",
            String::from_utf8_lossy(&buf[..n])
        );
    }
}

/// An origin that serves `data` from its start and redirects every other
/// range to `to`, one request per connection.
async fn steering_origin(data: Arc<Vec<u8>>, to: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (data, to) = (data.clone(), to.clone());
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                if !head.contains("range: bytes=0-") {
                    let reply = format!(
                        "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    return;
                }
                let len = data.len();
                let reply = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-{}/{len}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
                    len - 1
                );
                if socket.write_all(reply.as_bytes()).await.is_ok() {
                    let _ = socket.write_all(&data).await;
                }
            });
        }
    });
    format!("http://{addr}/v.mkv")
}

#[tokio::test]
async fn the_in_process_reader_keeps_the_playbacks_scope() {
    let canary = Canary::start(0).await;
    let len = 16u64 << 20;
    let origin = steering_origin(file(len as usize), format!("http://{}/tail", canary.addr)).await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let lease = server
        .open(local_upstream(origin), "v.mkv", Duration::from_secs(5))
        .await
        .unwrap();
    let reader = lease.reader().expect("a ranged file reads in process");
    // The hash's tail read opens its own upstream read, which the origin
    // sends off its host.
    let tail = tokio::time::timeout(
        Duration::from_secs(10),
        reader.read_at(len - 65_536, 65_536),
    )
    .await
    .expect("the read ended");
    assert!(tail.is_err(), "the tail came from somewhere");
    assert_eq!(canary.accepted(), 0);
}

/// Waits, at most `limit`, until `done` holds.
async fn until(limit: Duration, what: &str, done: impl Fn() -> bool) {
    let wait = async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    if tokio::time::timeout(limit, wait).await.is_err() {
        panic!("{what}");
    }
}

/// A player with a small receive buffer that reads the response head of
/// `url` and then nothing more.
async fn stalled(server: &StreamServer, url: &str) -> TcpStream {
    let base = format!("http://{}", server.addr());
    let path = url.trim_start_matches(&base);
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(16 * 1024).unwrap();
    let mut player = socket.connect(server.addr()).await.unwrap();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nRange: bytes=0-\r\n\r\n",
        server.addr()
    );
    player.write_all(request.as_bytes()).await.unwrap();
    let mut head = [0u8; 1024];
    let n = player.read(&mut head).await.unwrap();
    let head = String::from_utf8_lossy(&head[..n]);
    assert!(head.starts_with("HTTP/1.1 2"), "{head}");
    player
}

#[tokio::test]
async fn a_stalled_player_holds_nothing_after_release() {
    let data = file(64 << 20);
    let how = Behaviour {
        ignore_ranges: true,
        ..Default::default()
    };
    let (server, lease, seen, _http) = serve(&data, how).await;
    let mut player = stalled(&server, &lease.url).await;
    // Only the plainly proxied answer is left, held back by the player.
    let open = || seen.open.load(Ordering::SeqCst);
    until(Duration::from_secs(5), "the prefetch answer stayed", || {
        open() == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(open(), 1);
    drop(lease);
    until(
        Duration::from_secs(1),
        "the upstream outlived its lease",
        || open() == 0,
    )
    .await;
    // Dropping the server closes the stalled connection: the player's
    // writes then meet a reset.
    drop(server);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let _ = player.write_all(b"\r\n").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let late = player.write_all(b"\r\n").await;
    assert!(late.is_err(), "the connection outlived the server");
}

#[tokio::test]
async fn dropping_the_server_ends_bodies_in_flight() {
    let data = file(64 << 20);
    let (server, lease, _seen, http) = serve(&data, Behaviour::default()).await;
    let mut body = http
        .stream(&lease.url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    body.next_chunk().await.unwrap().unwrap();
    drop(server);
    let mut got = 0;
    let drained = tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(Ok(chunk)) = body.next_chunk().await {
            got += chunk.len();
        }
    })
    .await;
    assert!(drained.is_ok(), "the body outlived the server");
    assert!(got < 32 << 20, "{got} bytes after the drop");
    // The lease outlived the server; its playback did not.
    assert_eq!(lease.subtitles().add(PLAYBACK, "eng", None), None);
    assert!(http.get(&lease.url, &local()).await.is_err());
}

// C22: HLS through the loopback server (plans/c21-c23-loopback.md 3.3).

/// A request an `hls_origin` saw.
#[derive(Clone, Debug)]
struct Asked {
    path: String,
    query: Option<String>,
    /// The request head, lowercased.
    head: String,
}

impl Asked {
    /// The `Range` asked for, as `(first, last)`.
    fn range(&self) -> Option<(usize, Option<usize>)> {
        let spec = self
            .head
            .lines()
            .find_map(|l| l.strip_prefix("range: bytes="))?;
        let (a, b) = spec.trim().split_once('-')?;
        Some((a.parse().ok()?, b.parse().ok()))
    }
}

/// What an `hls_origin` answers: after `wait`, `status` with `headers`, then
/// the body in `parts`, each after `gap`.
#[derive(Clone, Default)]
struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    parts: Vec<Vec<u8>>,
    wait: Duration,
    gap: Duration,
}

impl Reply {
    fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> Reply {
        Reply {
            status: 200,
            headers: vec![("Content-Type", content_type.into())],
            parts: vec![body.into()],
            ..Reply::default()
        }
    }

    fn playlist(body: impl Into<Vec<u8>>) -> Reply {
        Reply::ok("application/vnd.apple.mpegurl", body)
    }

    fn status(status: u16) -> Reply {
        Reply {
            status,
            ..Reply::default()
        }
    }

    /// `data` with single byte ranges, as a file server answers.
    fn ranged(asked: &Asked, content_type: &str, data: &[u8]) -> Reply {
        let Some((first, last)) = asked.range() else {
            return Reply::ok(content_type, data);
        };
        let end = last.map_or(data.len(), |l| (l + 1).min(data.len()));
        let mut reply = Reply::ok(content_type, &data[first..end]);
        reply.status = 206;
        let span = format!("bytes {first}-{}/{}", end - 1, data.len());
        reply.headers.push(("Content-Range", span));
        reply
    }
}

type Log = Arc<Mutex<Vec<Asked>>>;

/// An origin answering every request with `answer`, one request per
/// connection; returns its base URL and what it was asked.
async fn hls_origin(answer: impl Fn(&Asked) -> Reply + Send + Sync + 'static) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let seen = log.clone();
    let answer = Arc::new(answer);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (seen, answer) = (seen.clone(), answer.clone());
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 4096];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let target = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let (path, query) = match target.split_once('?') {
                    Some((p, q)) => (p.to_owned(), Some(q.to_owned())),
                    None => (target, None),
                };
                let asked = Asked {
                    path,
                    query,
                    head: head.to_ascii_lowercase(),
                };
                seen.lock().unwrap().push(asked.clone());
                let reply = answer(&asked);
                tokio::time::sleep(reply.wait).await;
                let len: usize = reply.parts.iter().map(Vec::len).sum();
                let mut out = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {len}\r\nConnection: close\r\n",
                    reply.status
                );
                for (name, value) in &reply.headers {
                    out.push_str(&format!("{name}: {value}\r\n"));
                }
                out.push_str("\r\n");
                if socket.write_all(out.as_bytes()).await.is_err() {
                    return;
                }
                for part in &reply.parts {
                    tokio::time::sleep(reply.gap).await;
                    if socket.write_all(part).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (base, log)
}

/// Pages by path, files answering ranges; 404 for anything else.
fn pages(
    map: Vec<(&'static str, &'static str, Vec<u8>)>,
) -> impl Fn(&Asked) -> Reply + Send + Sync + 'static {
    move |asked| match map.iter().find(|(path, _, _)| *path == asked.path) {
        Some((_, kind, body)) if kind.contains("mpegurl") => Reply::playlist(body.clone()),
        Some((_, kind, body)) => Reply::ranged(asked, kind, body),
        None => Reply::status(404),
    }
}

async fn open_hls(server: &StreamServer, url: String) -> Lease {
    let lease = server
        .open(local_upstream(url), "v.mkv", Duration::from_secs(5))
        .await
        .expect("an HLS playback opens");
    assert_eq!(lease.format, Some(mq_server::Format::Hls));
    assert!(lease.url.ends_with("/v.m3u8"), "{}", lease.url);
    lease
}

/// Every URL a playlist names: its URI lines and its `URI="..."` values.
fn named_urls(playlist: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in playlist.lines() {
        if !line.is_empty() && !line.starts_with('#') {
            out.push(line.to_owned());
        }
        for (at, _) in line.match_indices("URI=\"") {
            let rest = &line[at + 5..];
            out.push(rest[..rest.find('"').unwrap()].to_owned());
        }
    }
    out
}

fn text(response: &mq_http::Response) -> String {
    String::from_utf8(response.body.to_vec()).unwrap()
}

#[tokio::test]
async fn an_hls_playback_is_served_entirely_through_loopback() {
    let media = file(2100);
    let key = b"0123456789abcdef".to_vec();
    let (far, far_log) = hls_origin(pages(vec![
        (
            "/far/index.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nseg.ts\n#EXT-X-ENDLIST\n".to_vec(),
        ),
        ("/far/seg.ts", "video/mp2t", b"far segment".to_vec()),
    ]))
    .await;
    let master = format!(
        "#EXTM3U\n#EXT-X-INDEPENDENT-SEGMENTS\n\
         #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",DEFAULT=YES,URI=\"audio/en.m3u8\"\n\
         #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"sub\",NAME=\"English\",LANGUAGE=\"en\",URI=\"/subs/en.m3u8\"\n\
         #EXT-X-STREAM-INF:BANDWIDTH=800000,AUDIO=\"aud\",SUBTITLES=\"sub\"\nlow/index.m3u8\n\
         #EXT-X-STREAM-INF:BANDWIDTH=2000000,AUDIO=\"aud\",SUBTITLES=\"sub\"\n/v/high/index.m3u8\n\
         #EXT-X-STREAM-INF:BANDWIDTH=4000000,AUDIO=\"aud\",SUBTITLES=\"sub\"\n{far}/far/index.m3u8\n"
    );
    let low = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:4\n\
               #EXT-X-KEY:METHOD=AES-128,URI=\"../key.bin\",IV=0x00000000000000000000000000000001\n\
               #EXT-X-MAP:URI=\"media.mp4\",BYTERANGE=\"100@0\"\n\
               #EXTINF:4.0,\n#EXT-X-BYTERANGE:1000@100\nmedia.mp4\n\
               #EXTINF:4.0,\n#EXT-X-BYTERANGE:1000@1100\nmedia.mp4\n#EXT-X-ENDLIST\n";
    let plain = |seg: &str| {
        format!("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\n{seg}\n#EXT-X-ENDLIST\n")
            .into_bytes()
    };
    let (base, log) = hls_origin(pages(vec![
        (
            "/v/master.m3u8",
            "application/vnd.apple.mpegurl",
            master.into_bytes(),
        ),
        (
            "/v/low/index.m3u8",
            "application/vnd.apple.mpegurl",
            low.as_bytes().to_vec(),
        ),
        (
            "/v/high/index.m3u8",
            "application/vnd.apple.mpegurl",
            plain("h.ts"),
        ),
        (
            "/v/audio/en.m3u8",
            "application/vnd.apple.mpegurl",
            plain("a.aac"),
        ),
        (
            "/subs/en.m3u8",
            "application/vnd.apple.mpegurl",
            plain("en.vtt"),
        ),
        ("/v/key.bin", "application/octet-stream", key.clone()),
        ("/v/low/media.mp4", "video/mp4", media.to_vec()),
        ("/v/high/h.ts", "video/mp2t", b"high segment".to_vec()),
        ("/v/audio/a.aac", "audio/aac", b"audio segment".to_vec()),
        ("/subs/en.vtt", "text/vtt", b"WEBVTT\n".to_vec()),
    ]))
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/v/master.m3u8")).await;
    let core = format!("http://{}/", server.addr());
    // Walk every playlist; every URL in them is the core's.
    let mut playlists = vec![lease.url.clone()];
    let mut files = Vec::new();
    while let Some(url) = playlists.pop() {
        let response = http.get(&url, &local()).await.unwrap();
        assert_eq!(
            response.header("access-control-allow-origin"),
            Some("*"),
            "{url}"
        );
        assert_eq!(
            response.header("content-type"),
            Some("application/vnd.apple.mpegurl")
        );
        for named in named_urls(&text(&response)) {
            assert!(named.starts_with(&core), "{named}");
            if named.ends_with(".m3u8") {
                playlists.push(named);
            } else {
                files.push(named);
            }
        }
    }
    files.sort();
    files.dedup();
    let by_end = |end: &str| files.iter().find(|f| f.ends_with(end)).unwrap().clone();
    // Ranges as BYTERANGE gives them, the key whole, every other file whole.
    let mp4 = by_end("/media.mp4");
    for (first, len) in [(0, 100), (100, 1000), (1100, 1000)] {
        let response = http.get_range(&mp4, first, len, &local()).await.unwrap();
        assert_eq!(response.status, 206);
        assert_eq!(response.header("access-control-allow-origin"), Some("*"));
        assert_eq!(
            response.body[..],
            media[first as usize..(first + len) as usize]
        );
    }
    let fetched = http.get(&by_end("/key.bin"), &local()).await.unwrap();
    assert_eq!(fetched.body[..], key[..]);
    assert_eq!(fetched.header("access-control-allow-origin"), Some("*"));
    for (end, body) in [
        ("/h.ts", &b"high segment"[..]),
        ("/seg.ts", b"far segment"),
        ("/a.aac", b"audio segment"),
        ("/en.vtt", b"WEBVTT\n"),
    ] {
        assert_eq!(
            http.get(&by_end(end), &local()).await.unwrap().body[..],
            *body,
            "{end}"
        );
    }
    // The origins saw only the core.
    for asked in log
        .lock()
        .unwrap()
        .iter()
        .chain(far_log.lock().unwrap().iter())
    {
        assert!(
            asked.head.contains("user-agent: marquee/"),
            "{}",
            asked.head
        );
    }
}

#[tokio::test]
async fn a_relative_child_is_not_answered_with_the_master() {
    let (base, _) = hls_origin(pages(vec![
        (
            "/live/index.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:2,\nseg0.ts\n".to_vec(),
        ),
        ("/live/seg0.ts", "video/mp2t", b"the first segment".to_vec()),
    ]))
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/live/index.m3u8")).await;
    let playlist = text(&http.get(&lease.url, &local()).await.unwrap());
    let first = named_urls(&playlist).remove(0);
    assert_eq!(
        http.get(&first, &local()).await.unwrap().body[..],
        b"the first segment"[..]
    );
}

/// The canary by literal, by mapped literal (an IPv4 canary) and by name.
fn canary_hosts(canary: &Canary, name: &str) -> Vec<String> {
    mq_http::test_dns::set(name, &[canary.addr.ip()]);
    let port = canary.addr.port();
    let mut hosts = vec![canary.addr.to_string(), format!("{name}:{port}")];
    if let std::net::IpAddr::V4(v4) = canary.addr.ip() {
        hosts.push(format!("[::ffff:{v4}]:{port}"));
    }
    hosts
}

#[tokio::test]
async fn a_manifest_naming_the_canary_never_reaches_it() {
    let canary = Canary::start(0).await;
    let hosts = canary_hosts(&canary, "hls-canary.c22.test");
    let mut master = String::from("#EXTM3U\n");
    let mut media = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n");
    for host in &hosts {
        master.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH=1\nhttp://{host}/v.m3u8\n"
        ));
        media.push_str(&format!("#EXTINF:4,\nhttp://{host}/s.ts\n"));
    }
    media.push_str(&format!(
        "#EXT-X-KEY:METHOD=AES-128,URI=\"http://{}/k\"\n#EXTINF:4,\nlocal.ts\n",
        hosts[0]
    ));
    let (base, _) = hls_origin(pages(vec![
        (
            "/master.m3u8",
            "application/vnd.apple.mpegurl",
            master.into_bytes(),
        ),
        (
            "/media.m3u8",
            "application/vnd.apple.mpegurl",
            media.into_bytes(),
        ),
    ]))
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    for main in ["master", "media"] {
        let lease = open_hls(&server, format!("{base}/{main}.m3u8")).await;
        let playlist = text(&http.get(&lease.url, &local()).await.unwrap());
        for child in named_urls(&playlist)
            .into_iter()
            .filter(|c| !c.ends_with("/local.ts"))
        {
            let err = http.get(&child, &local()).await.unwrap_err();
            assert!(
                matches!(err, mq_http::HttpError::Status { status: 502, .. }),
                "{child}: {err:?}"
            );
        }
    }
    assert_eq!(canary.accepted(), 0);
}

#[tokio::test]
async fn media_children_are_never_sniffed() {
    let canary = Canary::start(0).await;
    let playlist_like = format!(
        "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nhttp://{}/x.m3u8\n",
        canary.addr
    )
    .into_bytes();
    let mut mpd_like = vec![b' '; 2048];
    mpd_like.extend_from_slice(format!("<mpd profiles=\"urn:mpeg:dash:profile:isoff-live:2011\"><BaseURL>http://{}/</BaseURL></mpd>", canary.addr).as_bytes());
    let (base, _) = hls_origin(pages(vec![
        (
            "/m.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\none.ts\n#EXTINF:4,\ntwo.ts\n".to_vec(),
        ),
        ("/one.ts", "video/mp2t", playlist_like.clone()),
        ("/two.ts", "application/dash+xml", mpd_like.clone()),
    ]))
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/m.m3u8")).await;
    let children = named_urls(&text(&http.get(&lease.url, &local()).await.unwrap()));
    assert_eq!(
        http.get(&children[0], &local()).await.unwrap().body[..],
        playlist_like[..]
    );
    assert_eq!(
        http.get(&children[1], &local()).await.unwrap().body[..],
        mpd_like[..]
    );
    assert_eq!(canary.accepted(), 0);
}

#[tokio::test]
async fn keys_are_read_up_to_64_kib() {
    let (base, _) = hls_origin(pages(vec![
        (
            "/m.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-KEY:METHOD=AES-128,URI=\"fits\"\n#EXTINF:4,\none.ts\n#EXT-X-KEY:METHOD=AES-128,URI=\"big\"\n#EXTINF:4,\ntwo.ts\n".to_vec(),
        ),
        ("/fits", "application/octet-stream", vec![7; 64 << 10]),
        ("/big", "application/octet-stream", vec![7; (64 << 10) + 1]),
    ]))
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/m.m3u8")).await;
    let named = named_urls(&text(&http.get(&lease.url, &local()).await.unwrap()));
    let key = |name: &str| named.iter().find(|u| u.ends_with(name)).unwrap().clone();
    let fits = http.get(&key("/fits"), &local()).await.unwrap();
    assert_eq!(fits.body.len(), 64 << 10);
    let big = http.get(&key("/big"), &local()).await.unwrap_err();
    assert!(
        matches!(big, mq_http::HttpError::Status { status: 502, .. }),
        "{big:?}"
    );
}

#[tokio::test]
async fn a_child_redirect_to_the_canary_is_refused() {
    let canary = Canary::start(0).await;
    let hosts = canary_hosts(&canary, "redirect-canary.c22.test");
    let mut media = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n");
    for i in 0..hosts.len() {
        media.push_str(&format!("#EXTINF:4,\nhop{i}.ts\n"));
    }
    let (base, _) = hls_origin(move |asked| {
        if asked.path == "/m.m3u8" {
            return Reply::playlist(media.clone());
        }
        let i: usize = asked
            .path
            .trim_start_matches("/hop")
            .trim_end_matches(".ts")
            .parse()
            .unwrap();
        let mut reply = Reply::status(302);
        reply
            .headers
            .push(("Location", format!("http://{}/s.ts", hosts[i])));
        reply
    })
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/m.m3u8")).await;
    for child in named_urls(&text(&http.get(&lease.url, &local()).await.unwrap())) {
        let (head, _) = raw_full(&server, &child, "bytes=0-").await;
        assert!(head.starts_with("http/1.1 502"), "{child}: {head}");
    }
    assert_eq!(canary.accepted(), 0);
}

#[tokio::test]
async fn a_child_path_cannot_change_the_origin() {
    let (base, log) = hls_origin(pages(vec![
        (
            "/m.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\ns.ts\n".to_vec(),
        ),
        ("/s.ts", "video/mp2t", b"segment".to_vec()),
    ]))
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/m.m3u8")).await;
    let before = log.lock().unwrap().len();
    let prefix = format!("http://{}/c/{}", server.addr(), token(&lease.url));
    for id in ["+0", "00", "-1", "4294967296", "0x0"] {
        let reply = raw(&server, &format!("{prefix}/{id}/s.ts"), "GET", None).await;
        assert!(reply.starts_with("HTTP/1.1 404"), "{id}: {reply}");
    }
    assert_eq!(log.lock().unwrap().len(), before, "the origin was asked");
}

#[tokio::test]
async fn ll_hls_directives_reach_only_playlists() {
    let (base, log) = hls_origin(pages(vec![
        (
            "/master.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nlive.m3u8?sig=1\n".to_vec(),
        ),
        (
            "/live.m3u8",
            "application/vnd.apple.mpegurl",
            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\ns.ts\n".to_vec(),
        ),
        ("/s.ts", "video/mp2t", b"segment".to_vec()),
    ]))
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/master.m3u8")).await;
    let asked = "?_HLS_msn=5&_HLS_part=1&x=1";
    let variant = named_urls(&text(
        &http
            .get(&format!("{}{asked}", lease.url), &local())
            .await
            .unwrap(),
    ))
    .remove(0);
    let segment = named_urls(&text(
        &http
            .get(&format!("{variant}{asked}"), &local())
            .await
            .unwrap(),
    ))
    .remove(0);
    http.get(&format!("{segment}{asked}"), &local())
        .await
        .unwrap();
    let log = log.lock().unwrap();
    let last = |path: &str| {
        log.iter()
            .rev()
            .find(|a| a.path == path)
            .unwrap()
            .query
            .clone()
    };
    assert_eq!(
        last("/master.m3u8").as_deref(),
        Some("_HLS_msn=5&_HLS_part=1")
    );
    assert_eq!(
        last("/live.m3u8").as_deref(),
        Some("sig=1&_HLS_msn=5&_HLS_part=1")
    );
    assert_eq!(last("/s.ts"), None);
}

#[tokio::test]
async fn a_blocking_reload_may_wait_three_target_durations() {
    let opened = Arc::new(AtomicU64::new(0));
    let count = opened.clone();
    let (base, _) = hls_origin(move |_| {
        let mut reply = Reply::playlist("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\ns.ts\n");
        // Open's own reads answer at once; a player's reload waits 11 s.
        if count.load(Ordering::SeqCst) == 1 {
            reply.wait = Duration::from_secs(11);
        }
        reply
    })
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/live.m3u8")).await;
    opened.store(1, Ordering::SeqCst);
    let slow = RequestOptions {
        timeout: Duration::from_secs(30),
        ..local()
    };
    let blocking = http.get(&format!("{}?_HLS_msn=9", lease.url), &slow).await;
    assert_eq!(blocking.map(|r| r.status).ok(), Some(200));
    let plain = http.get(&lease.url, &slow).await.unwrap_err();
    assert!(
        matches!(plain, mq_http::HttpError::Status { status: 502, .. }),
        "{plain:?}"
    );
}

#[tokio::test]
async fn a_live_playlist_is_fetched_fresh() {
    let reloads = Arc::new(AtomicU64::new(0));
    let count = reloads.clone();
    let (base, _) = hls_origin(move |_| {
        let n = count.fetch_add(1, Ordering::SeqCst);
        Reply::playlist(format!(
            "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:{n}\n#EXTINF:4,\ns{n}.ts\n#EXTINF:4,\ns{}.ts\n",
            n + 1
        ))
    })
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = open_hls(&server, format!("{base}/live.m3u8")).await;
    let a = named_urls(&text(&http.get(&lease.url, &local()).await.unwrap()));
    let b = named_urls(&text(&http.get(&lease.url, &local()).await.unwrap()));
    assert_ne!(a, b, "a reload was answered from a cache");
    // A segment both reloads name keeps its id; a new one gets the next.
    assert_eq!(a[1], b[0]);
    let id = |url: &str| -> u64 { url.rsplit('/').nth(1).unwrap().parse().unwrap() };
    assert_eq!(id(&b[1]), id(&a[1]) + 1);
}

#[tokio::test]
async fn declared_playlists_never_pass_raw() {
    let mkv = b"\x1aE\xdf\xa3 a matroska file".to_vec();
    let (base, _) = hls_origin(move |asked| match asked.path.as_str() {
        "/stream" => Reply::ranged(asked, "Application/X-MpegURL; charset=UTF-8", &mkv),
        "/manifest.mpd" => Reply::ok("application/octet-stream", "garbage, not an MPD"),
        _ => Reply::status(404),
    })
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    for path in ["/stream", "/manifest.mpd"] {
        let opened = server
            .open(
                local_upstream(format!("{base}{path}")),
                "v.mkv",
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(
            opened.unwrap_err(),
            "The stream's playlist could not be read",
            "{path}"
        );
    }
    assert_eq!(server.playbacks(), 0);
}

#[tokio::test]
async fn chunk_split_signatures_are_found() {
    let (base, _) = hls_origin(|asked| {
        let mut reply = Reply {
            status: 200,
            headers: vec![("Content-Type", "application/octet-stream".into())],
            gap: Duration::from_millis(50),
            ..Reply::default()
        };
        reply.parts = match asked.path.as_str() {
            "/a" => vec![
                b"#EXT".to_vec(),
                b"M3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\ns.ts\n".to_vec(),
            ],
            _ => vec![
                b"<?xml version=\"1.0\"?>".to_vec(),
                format!("<!--{}-->", "c".repeat(2048)).into_bytes(),
                b"<d:MPD xmlns:d=\"urn:mpeg:dash:schema:mpd:2011\"></d:MPD>".to_vec(),
            ],
        };
        reply
    })
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let hls = server
        .open(
            local_upstream(format!("{base}/a")),
            "v.mkv",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(hls.format, Some(mq_server::Format::Hls));
    let dash = server
        .open(
            local_upstream(format!("{base}/b")),
            "v.mkv",
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(dash.unwrap_err(), "This player cannot play DASH streams");
}

#[tokio::test]
async fn doctype_and_patch_are_refused() {
    let (base, _) = hls_origin(|asked| match asked.path.as_str() {
        "/doctype" => Reply::ok("application/xml", "<?xml version=\"1.0\"?><!DOCTYPE MPD [<!ENTITY x \"y\">]><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\"/>"),
        _ => Reply::ok("application/xml", "<?xml version=\"1.0\"?><Patch xmlns=\"urn:mpeg:dash:schema:mpd-patch:2020\"/>"),
    })
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    for path in ["/doctype", "/patch"] {
        let opened = server
            .open(
                local_upstream(format!("{base}{path}")),
                "v.mkv",
                Duration::from_secs(5),
            )
            .await;
        assert_eq!(
            opened.unwrap_err(),
            "The stream's playlist could not be read",
            "{path}"
        );
    }
}

#[tokio::test]
async fn manifest_limits_and_deadlines() {
    let mut huge = b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n".to_vec();
    huge.resize((8 << 20) + 1, b'\n');
    let mut wide = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n");
    for i in 0..180_000 {
        wide.push_str(&format!("#EXTINF:1,\n{i}\n"));
    }
    assert!(wide.len() < 8 << 20);
    // Accepted at 8 MiB, but every line becomes a 100 byte core URL.
    let mut repeated = b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n".to_vec();
    repeated.resize(8 << 20, b'\n');
    for pair in repeated[32..].chunks_exact_mut(2) {
        pair[0] = b'a';
    }
    let (base, _) = hls_origin(move |asked| match asked.path.as_str() {
        "/huge.m3u8" => Reply::playlist(huge.clone()),
        "/repeated.m3u8" => Reply::playlist(repeated.clone()),
        "/wide.m3u8" => Reply::playlist(wide.clone()),
        _ => Reply {
            status: 200,
            headers: vec![("Content-Type", "application/vnd.apple.mpegurl".into())],
            parts: b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n"
                .iter()
                .map(|b| vec![*b])
                .collect(),
            gap: Duration::from_millis(500),
            ..Reply::default()
        },
    })
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    // The wide one is rewritten in full before it is refused: in a debug
    // build that alone takes several seconds.
    let s = Duration::from_secs;
    for (path, deadline, words) in [
        (
            "/huge.m3u8",
            s(5),
            "The stream's playlist could not be read",
        ),
        (
            "/repeated.m3u8",
            s(5),
            "The stream's playlist could not be read",
        ),
        (
            "/wide.m3u8",
            s(30),
            "The stream's playlist could not be read",
        ),
        ("/trickle.m3u8", s(3), "The source did not answer in time"),
    ] {
        let started = std::time::Instant::now();
        let opened = server
            .open(local_upstream(format!("{base}{path}")), "v.mkv", deadline)
            .await;
        assert_eq!(opened.unwrap_err(), words, "{path}");
        let took = started.elapsed();
        assert!(took < deadline + s(1), "{path} took {took:?}");
    }
}

#[tokio::test]
async fn a_video_file_is_never_sniffed() {
    let mkv = file(4 << 20);
    let (base, log) = hls_origin(move |asked| {
        let mut reply = Reply::ranged(asked, "video/x-matroska", &mkv);
        reply.gap = Duration::from_secs(2);
        reply
    })
    .await;
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let started = std::time::Instant::now();
    let lease = server
        .open(
            local_upstream(format!("{base}/v.mkv")),
            "v.mkv",
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(lease.format, None);
    assert_eq!(log.lock().unwrap().len(), 1, "only the prefetch request");
}
