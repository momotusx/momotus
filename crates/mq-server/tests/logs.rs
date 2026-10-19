//! Its own test binary: tracing caches whether a log line is wanted per call
//! site, process-wide, so other tests running alongside could hide these
//! lines from this test's subscriber.

use mq_http::{ClientConfig, HttpClient, RequestOptions, Url, test_dns};
use mq_server::{StreamServer, Upstream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An origin whose `n`th connection gets `replies[n]` after `delay`, the
/// last one from then on; an empty reply closes the connection unanswered.
async fn origin(replies: Vec<String>, delay: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = Arc::new(AtomicUsize::new(0));
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let n = served.fetch_add(1, Ordering::SeqCst);
            let reply = replies[n.min(replies.len() - 1)].clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    port
}

/// An origin serving `len` zero bytes from any `bytes=N-` range, which
/// stops listening after `accepts` connections.
async fn ranged(len: usize, accepts: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        for _ in 0..accepts {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let start: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim().trim_end_matches('-').parse().ok())
                    .unwrap_or(0);
                let reply = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    len - 1,
                    len - start
                );
                if socket.write_all(reply.as_bytes()).await.is_ok() {
                    let _ = socket.write_all(&vec![0u8; len - start]).await;
                }
            });
        }
    });
    port
}

#[tokio::test]
async fn upstream_logs_never_name_the_origin() {
    // The server's tasks run on this test's current-thread runtime, so their
    // lines land here.
    let logs = Arc::new(Mutex::new(Vec::new()));
    let sink = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(move || Sink(sink.clone()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    test_dns::set("log-origin.test", &["127.0.0.1".parse().unwrap()]);
    let local = Url::parse("http://127.0.0.1:9/manifest.json").unwrap();
    let forbidden = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let refusing = origin(vec![forbidden.into()], Duration::ZERO).await;
    let silent = origin(vec![String::new()], Duration::from_secs(30)).await;
    // A port nothing listens on: an in-scope host refusing the connection.
    let dead = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let upstream = |port: u16, addon_url: Option<Url>| Upstream::Http {
        url: format!("http://log-origin.test:{port}/v.mkv"),
        headers: vec![],
        addon_url,
    };
    let open = |port: u16, addon_url: Option<Url>, deadline: Duration| {
        server.open(upstream(port, addon_url), "v.mkv", deadline)
    };
    let second = Duration::from_secs(5);
    // Blocked: no install, so 127.0.0.1 is out of scope.
    let blocked = open(refusing, None, second).await.unwrap_err();
    assert_eq!(blocked, "The source points to a private network address");
    let refused = open(refusing, Some(local.clone()), second)
        .await
        .unwrap_err();
    assert_eq!(refused, "The source refused access (403)");
    let late = open(silent, Some(local.clone()), Duration::from_millis(300)).await;
    assert_eq!(late.unwrap_err(), "The source did not answer in time");
    let down = open(dead, Some(local.clone()), second).await.unwrap_err();
    assert_eq!(down, "The source could not be reached");

    let player = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    // A subtitle on a dead port.
    let lease = server.register(upstream(dead, Some(local.clone())), "v.mkv");
    let track = format!("http://log-origin.test:{dead}/s.srt");
    let sub = lease.subtitles().add(&track, "eng", Some(&local)).unwrap();
    assert!(http.get(&sub, &player).await.is_err());
    // A plain proxied origin that stops answering after its first reply.
    let whole = "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nabcd";
    let flaky = origin(vec![whole.into(), String::new()], Duration::ZERO).await;
    let lease = server.register(upstream(flaky, Some(local.clone())), "v.mkv");
    assert!(http.get_range(&lease.url, 0, 4, &player).await.is_err());
    // A redirect target remembered by a read, whose name then points out of
    // the install's scope; then, remembered again, that stops listening.
    let target = ranged(16 << 20, 2).await;
    let home = ["127.0.0.1".parse().unwrap()];
    test_dns::set("memo-log.test", &home);
    let hop = format!(
        "HTTP/1.1 302 Found\r\nLocation: http://memo-log.test:{target}/v.mkv\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let redirector = origin(vec![hop], Duration::ZERO).await;
    let lease = server.register(upstream(redirector, Some(local)), "v.mkv");
    let read = |at: u64| http.get_range(&lease.url, at << 20, 1000, &player);
    assert!(read(0).await.is_ok());
    test_dns::set("memo-log.test", &["10.0.0.1".parse().unwrap()]);
    assert!(read(12).await.is_err());
    test_dns::set("memo-log.test", &home);
    assert!(read(8).await.is_ok());
    assert!(read(14).await.is_err());
    // A request through the core as a proxy for another host.
    let mut socket = tokio::net::TcpStream::connect(server.addr()).await.unwrap();
    let foreign = "GET http://log-foreign.test/x HTTP/1.1\r\nHost: log-foreign.test\r\nConnection: close\r\n\r\n";
    socket.write_all(foreign.as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    socket.read_to_end(&mut reply).await.unwrap();
    assert!(reply.starts_with(b"HTTP/1.1 403"));

    let logs = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    let refusals: Vec<&str> = logs
        .lines()
        .filter(|l| l.contains("refused a request for another host"))
        .collect();
    assert_eq!(refusals.len(), 1, "one line per refusal: {logs}");
    // At debug: a hostile page can send many.
    assert!(refusals[0].contains("DEBUG"), "{}", refusals[0]);
    // Every upstream call site wrote its line, with its fixed words.
    for line in [
        "stream upstream refused: blocked address",
        "stream upstream failed: network",
        "stream source unreachable: network",
        "subtitle upstream failed: network",
        "plain proxy upstream failed: network",
        "stream redirect target failed: blocked address",
        "stream redirect target failed: network",
    ] {
        assert!(logs.contains(line), "no {line:?} in {logs}");
    }
    // The core's own lines, debug included; the HTTP stack's debug lines
    // (which name the address they connect to) are its own.
    let ours: Vec<&str> = logs
        .lines()
        .filter(|l| {
            l.split_whitespace()
                .nth(2)
                .is_some_and(|t| t.starts_with("mq_"))
        })
        .collect();
    let ours = ours.join("\n");
    assert!(ours.contains("DEBUG"), "{ours}");
    for name in [
        "log-origin.test",
        "memo-log.test",
        "10.0.0.1",
        "log-foreign.test",
    ] {
        assert!(!ours.contains(name), "{ours}");
    }
    for port in [refusing, silent, dead, flaky, target, redirector] {
        assert!(!ours.contains(&format!(":{port}")), "{ours}");
    }
}
