use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_server::{StreamServer, Upstream};
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
}

impl Seen {
    fn ranges(&self) -> Vec<String> {
        self.ranges.lock().unwrap().clone()
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
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
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
    if let Some(delay) = how.slow_start.filter(|_| range.as_deref() == Some("0-")) {
        tokio::time::sleep(delay).await;
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
    let stop = how.cut_after.map_or(end, |n| (start + n).min(end));
    for chunk in data[start..stop].chunks(64 * 1024) {
        if socket.write_all(chunk).await.is_err() {
            return;
        }
        seen.sent.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
}

fn local() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        max_bytes: 64 << 20,
        ..Default::default()
    }
}

async fn serve(data: &Arc<Vec<u8>>, how: Behaviour) -> (StreamServer, String, Seen, HttpClient) {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let (origin, seen) = range_origin(data.clone(), how).await;
    let url = server.register(
        Upstream::Http {
            url: origin,
            headers: vec![("x-token".into(), "abc".into())],
            allow_private: true,
        },
        "video.mkv",
    );
    (server, url, seen, http)
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
    let url = server.register(
        Upstream::Http {
            url: origin,
            headers: vec![("x-token".into(), "abc".into())],
            allow_private: true,
        },
        "Show S01E01 2160p.mkv",
    );
    assert!(url.starts_with("http://127.0.0.1:"));
    assert!(url.ends_with("/Show_S01E01_2160p.mkv"));

    let resp = http.get_range(&url, 10, 10, &local()).await.unwrap();
    assert_eq!(resp.status, 206);
    assert_eq!(&resp.body[..], b"0123456789");
    assert_eq!(resp.header("content-range"), Some("bytes 10-19/100"));
    assert_eq!(resp.header("access-control-allow-origin"), Some("*"));
    assert_eq!(resp.header("content-type"), Some("video/x-matroska"));

    server.release(&url);
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
    let (_server, url, seen, http) = serve(&data, Behaviour::default()).await;
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
    let (_server, url, seen, http) = serve(&data, Behaviour::default()).await;
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
    let (server, url, _seen, http) = serve(&data, Behaviour::default()).await;
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
    let (_server, url, seen, http) = serve(&data, how).await;
    assert_eq!(
        range(&http, &url, 1000, (2 << 20) - 1000).await,
        data[1000..]
    );
    assert!(seen.ranges().len() > 1, "{:?}", seen.ranges());
}

#[tokio::test]
async fn a_cursor_reads_ahead_only_so_far() {
    let data = file(64 << 20);
    let (_server, url, seen, http) = serve(&data, Behaviour::default()).await;
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
    let (_server, url, _seen, http) = serve(&data, how).await;
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
    let url = server.register(
        Upstream::Http {
            url: origin,
            headers: vec![],
            allow_private: true,
        },
        "x.mkv",
    );
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
async fn subtitle_origin() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
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
    format!("http://{addr}/subs/12345")
}

#[tokio::test]
async fn subtitles_come_out_as_utf8_webvtt_for_any_page() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let origin = subtitle_origin().await;
    let url = server.register_subtitle(&origin, "rus", true);
    assert!(url.ends_with("/rus.vtt"), "{url}");
    assert_eq!(
        server.register_subtitle(&origin, "rus", true),
        url,
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

    server.release(&url);
    assert!(http.get(&url, &local()).await.is_err(), "released");
}

/// Live: time range requests through the loopback server against an origin
/// that redirects (archive.org). Run with `--ignored --nocapture`.
#[tokio::test]
#[ignore = "network"]
async fn live_redirecting_origin_range_times() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let url = server.register(
        Upstream::Http {
            url: "https://archive.org/download/BigBuckBunny_124/Content/big_buck_bunny_720p_surround.mp4".into(),
            headers: vec![],
            allow_private: false,
        },
        "bbb.mp4",
    );
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

#[tokio::test]
async fn a_host_that_caps_its_answers_is_proxied_plainly() {
    let data = file(2 << 20);
    let how = Behaviour {
        cap: Some(100_000),
        ..Default::default()
    };
    let (_server, url, _seen, http) = serve(&data, how).await;
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
    let (server, url, _seen, http) = serve(&data, Behaviour::default()).await;
    let mut resp = http
        .stream(&url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    resp.next_chunk().await.unwrap().unwrap();
    server.release(&url);
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
    let (server, url, _seen, http) = serve(&data, how).await;
    let mut resp = http
        .stream(&url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    resp.next_chunk().await.unwrap().unwrap();
    server.release(&url);
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
    let (_server, url, seen, http) = serve(&data, how).await;
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
    let (server, url, _seen, _http) = serve(&data, how).await;
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
    let (_server, url, seen, http) = serve(&data, how).await;
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
        allow_private: true,
    };
    let started = std::time::Instant::now();
    let url = server
        .open(upstream(&origin), "v.mkv", Duration::from_secs(5))
        .await
        .unwrap();
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
    assert!(err.contains("http status 403"), "{err}");
    let err = server
        .open(
            upstream("http://127.0.0.1:9/v.mkv"),
            "v.mkv",
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert!(err.starts_with("source did not answer"), "{err}");
}
