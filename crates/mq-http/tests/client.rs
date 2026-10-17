use mq_http::{Body, ClientConfig, HttpClient, HttpError, Method, Priority, RequestOptions};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves one canned response per connection and returns the base URL and the
/// first request head it received.
async fn serve(
    response: &'static [u8],
    delay: Duration,
) -> (String, tokio::sync::oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n = socket.read(&mut buf).await.unwrap();
        let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
        tokio::time::sleep(delay).await;
        let _ = socket.write_all(response).await;
    });
    (format!("http://{addr}"), rx)
}

fn local_opts() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        timeout: Duration::from_secs(2),
        ..Default::default()
    }
}

#[tokio::test]
async fn reads_json_and_sends_range() {
    let (base, head) = serve(
        b"HTTP/1.1 206 Partial Content\r\nContent-Length: 11\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
        Duration::ZERO,
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let resp = client
        .get_range(&format!("{base}/f.mkv"), 100, 50, &local_opts())
        .await
        .unwrap();
    assert_eq!(resp.status, 206);
    let value: serde_json::Value = resp.json().unwrap();
    assert_eq!(value["ok"], true);
    let head = head.await.unwrap().to_ascii_lowercase();
    assert!(head.contains("range: bytes=100-149"), "{head}");
    assert!(head.contains("user-agent: marquee/"), "{head}");
}

#[tokio::test]
async fn caps_body_size() {
    let (base, _) = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789", Duration::ZERO).await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let opts = RequestOptions {
        max_bytes: 10,
        ..local_opts()
    };
    let err = client.get(&base, &opts).await.unwrap_err();
    assert!(matches!(err, HttpError::TooLarge { limit: 10 }), "{err:?}");
}

#[tokio::test]
async fn times_out() {
    let (base, _) = serve(
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
        Duration::from_secs(5),
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let opts = RequestOptions {
        timeout: Duration::from_millis(200),
        ..local_opts()
    };
    let err = client.get(&base, &opts).await.unwrap_err();
    assert!(matches!(err, HttpError::Timeout(_)), "{err:?}");
}

#[tokio::test]
async fn reports_status_and_blocks_private_by_default() {
    let (base, _) = serve(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\n\r\nnope",
        Duration::ZERO,
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let err = client
        .get(&base, &RequestOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::BlockedPrivate(_)), "{err:?}");
    let err = client.get(&base, &local_opts()).await.unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 404, .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn posts_json_body() {
    let (base, head) = serve(
        b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
        Duration::ZERO,
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let resp = client
        .post_json(
            &format!("{base}/api/getUser"),
            &serde_json::json!({"authKey": "k"}),
            &local_opts(),
        )
        .await
        .unwrap();
    assert_eq!(resp.json::<serde_json::Value>().unwrap()["ok"], true);
    let head = head.await.unwrap().to_ascii_lowercase();
    assert!(head.starts_with("post /api/getuser"), "{head}");
    assert!(head.contains("content-type: application/json"), "{head}");
    assert!(head.ends_with("{\"authkey\":\"k\"}"), "{head}");
}

#[tokio::test]
#[ignore = "network"]
async fn fetches_real_cinemeta_manifest() {
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let opts = RequestOptions {
        priority: Priority::Addon,
        ..Default::default()
    };
    let manifest: serde_json::Value = client
        .get_json("https://v3-cinemeta.strem.io/manifest.json", &opts)
        .await
        .unwrap();
    assert_eq!(manifest["id"], "com.linvo.cinemeta");
}

#[tokio::test]
async fn posts_form_json_and_multipart_bodies() {
    let cases = [
        (
            Body::Form(vec![("a".into(), "x y".into()), ("b".into(), "1".into())]),
            "content-type: application/x-www-form-urlencoded",
            "a=x+y&b=1",
        ),
        (
            Body::Json(serde_json::json!({"k": 1})),
            "content-type: application/json",
            "{\"k\":1}",
        ),
        (
            Body::Multipart(vec![("magnet".into(), "magnet:?xt=urn:btih:abc".into())]),
            "content-type: multipart/form-data; boundary=",
            "name=\"magnet\"\r\n\r\nmagnet:?xt=urn:btih:abc\r\n",
        ),
    ];
    for (body, content_type, expected) in cases {
        let (base, head) = serve(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
            Duration::ZERO,
        )
        .await;
        let client = HttpClient::new(ClientConfig::default()).unwrap();
        let resp = client
            .request(Method::POST, &base, Some(body), &local_opts())
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        let head = head.await.unwrap();
        assert!(head.starts_with("POST "), "{head}");
        assert!(head.to_ascii_lowercase().contains(content_type), "{head}");
        assert!(head.contains(expected), "{head}");
    }
}

#[tokio::test]
async fn surfaces_retry_after_on_status_errors() {
    let (base, _) = serve(
        b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nContent-Length: 0\r\n\r\n",
        Duration::ZERO,
    )
    .await;
    let client = HttpClient::new(ClientConfig::default()).unwrap();
    let err = client.get(&base, &local_opts()).await.unwrap_err();
    match err {
        HttpError::Status {
            status: 429,
            retry_after,
            ..
        } => assert_eq!(retry_after, Some(Duration::from_secs(7))),
        other => panic!("{other:?}"),
    }
}
