//! Its own test binary: tracing caches whether a log line is wanted per call
//! site, process-wide, so other tests running alongside could hide these
//! lines from this test's subscriber.

use mq_http::{ClientConfig, HttpClient, Url, test_dns};
use mq_server::{StreamServer, Upstream};
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

/// An origin that answers every request with `reply` after `delay`.
async fn origin(reply: &'static str, delay: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
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

#[tokio::test]
async fn upstream_logs_never_name_the_origin() {
    // The server's tasks run on this test's current-thread runtime, so their
    // lines land here.
    let logs = Arc::new(Mutex::new(Vec::new()));
    let sink = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || Sink(sink.clone()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    test_dns::set("log-origin.test", &["127.0.0.1".parse().unwrap()]);
    let local = Url::parse("http://127.0.0.1:9/manifest.json").unwrap();
    let refusing = origin(
        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        Duration::ZERO,
    )
    .await;
    let silent = origin("", Duration::from_secs(30)).await;
    // A port nothing listens on: an in-scope host refusing the connection.
    let dead = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let open = |port: u16, addon_url: Option<Url>, deadline: Duration| {
        let upstream = Upstream::Http {
            url: format!("http://log-origin.test:{port}/v.mkv"),
            headers: vec![],
            addon_url,
        };
        server.open(upstream, "v.mkv", deadline)
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
    let down = open(dead, Some(local), second).await.unwrap_err();
    assert_eq!(down, "The source could not be reached");

    let logs = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    // The lines are there, with their fixed words.
    assert!(logs.contains("blocked address"), "{logs}");
    assert!(logs.contains("stream upstream failed: network"), "{logs}");
    assert!(!logs.contains("log-origin.test"), "{logs}");
    for port in [refusing, silent, dead] {
        assert!(!logs.contains(&format!(":{port}")), "{logs}");
    }
}
