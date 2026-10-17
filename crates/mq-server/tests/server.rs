use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_server::{StreamServer, Upstream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Origin that serves `bytes=10-19` of a 100-byte file and checks a header.
async fn origin() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let resp: Vec<u8> = if !head.contains("x-token: abc") {
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".to_vec()
                } else if head.contains("range: bytes=10-19") {
                    let mut r = b"HTTP/1.1 206 Partial Content\r\nContent-Type: video/x-matroska\r\nContent-Range: bytes 10-19/100\r\nContent-Length: 10\r\n\r\n".to_vec();
                    r.extend_from_slice(b"0123456789");
                    r
                } else {
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello".to_vec()
                };
                let _ = socket.write_all(&resp).await;
            });
        }
    });
    format!("http://{addr}/video.mkv")
}

fn local() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        ..Default::default()
    }
}

#[tokio::test]
async fn proxies_ranges_with_injected_headers_and_releases_tokens() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let url = server.register(
        Upstream::Http {
            url: origin().await,
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
    assert_eq!(resp.header("content-type"), Some("video/x-matroska"));

    server.release(&url);
    let err = http.get(&url, &local()).await.unwrap_err();
    assert!(
        matches!(err, mq_http::HttpError::Status { status: 404, .. }),
        "{err:?}"
    );
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

    let url = server.register(
        Upstream::Http {
            url: origin().await,
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
