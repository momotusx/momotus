use mq_addons::{AddonClient, AddonEvent, Deadlines, InstalledAddon};
use mq_http::{ClientConfig, HttpClient};
use mq_protocol::{Manifest, ResourcePath};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

/// Local addon that answers every request after `delay` with `body`.
async fn addon(id: &str, delay: Duration, body: &'static str) -> InstalledAddon {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest: Manifest = serde_json::from_value(serde_json::json!({
        "id": id, "version": "1.0.0", "name": id,
        "types": ["movie"], "resources": ["stream"], "catalogs": [], "idPrefixes": ["tt"]
    }))
    .unwrap();
    InstalledAddon {
        transport_url: Url::parse(&format!("http://{addr}/manifest.json")).unwrap(),
        manifest,
        allow_private: true,
    }
}

fn stream_path() -> ResourcePath {
    ResourcePath {
        resource: "stream".into(),
        r#type: "movie".into(),
        id: "tt0111161".into(),
        extra: vec![],
    }
}

#[tokio::test]
async fn fast_slow_and_timed_out_addons() {
    let fast = addon("fast", Duration::ZERO, r#"{"streams":[]}"#).await;
    let slow = addon("slow", Duration::from_millis(250), r#"{"streams":[]}"#).await;
    let dead = addon("dead", Duration::from_secs(30), r#"{"streams":[]}"#).await;
    let mut late = [slow.transport_url.clone(), dead.transport_url.clone()];
    late.sort();
    let client = AddonClient::new(HttpClient::new(ClientConfig::default()).unwrap());
    let deadlines = Deadlines {
        soft: Duration::from_millis(120),
        hard: Duration::from_millis(600),
    };
    let mut fanout = client.fan_out(&[fast, slow, dead], &stream_path(), deadlines);

    let mut log = Vec::new();
    while let Some(event) = fanout.next().await {
        let entry = match event {
            AddonEvent::Arrived { addon_id, .. } => format!("arrived:{addon_id}"),
            AddonEvent::Slow {
                mut addon_ids,
                mut transport_urls,
            } => {
                // The same installs as the ids, by their own URLs.
                transport_urls.sort();
                assert_eq!(transport_urls, late);
                addon_ids.sort();
                format!("slow:{}", addon_ids.join(","))
            }
            AddonEvent::Failed {
                addon_id, error, ..
            } => format!("failed:{addon_id}:{error}"),
            AddonEvent::Done => "done".into(),
        };
        log.push(entry);
    }
    assert_eq!(
        log,
        vec![
            "arrived:fast",
            "slow:dead,slow",
            "arrived:slow",
            "failed:dead:timed out",
            "done"
        ]
    );
    let health = client.health().get("fast").unwrap();
    assert_eq!(health.samples, 1);
}

#[tokio::test]
async fn duplicate_manifest_ids_keep_their_installs() {
    // Two installs (two transport URLs) of one manifest id; the second answers
    // later, after the first left the pending list.
    let first = addon("same", Duration::ZERO, r#"{"streams":[]}"#).await;
    let second = addon("same", Duration::from_millis(150), r#"{"streams":[]}"#).await;
    let urls = [first.transport_url.clone(), second.transport_url.clone()];
    let client = AddonClient::new(HttpClient::new(ClientConfig::default()).unwrap());
    let deadlines = Deadlines {
        soft: Duration::from_secs(2),
        hard: Duration::from_secs(4),
    };
    let mut fanout = client.fan_out(&[first, second], &stream_path(), deadlines);
    let mut arrived = Vec::new();
    while let Some(event) = tokio::time::timeout(Duration::from_secs(5), fanout.next())
        .await
        .expect("the fan-out ends within 5 s")
    {
        match event {
            AddonEvent::Arrived {
                addon_id,
                transport_url,
                ..
            } => arrived.push((addon_id, transport_url)),
            AddonEvent::Done => break,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        arrived,
        urls.map(|url| ("same".to_owned(), url)),
        "both installs answer, each under its own URL"
    );
}

#[tokio::test]
async fn unsupported_addons_are_skipped_and_cache_serves_repeat_calls() {
    let cached = addon(
        "cached",
        Duration::ZERO,
        r#"{"streams":[],"cacheMaxAge":60}"#,
    )
    .await;
    let client = AddonClient::new(HttpClient::new(ClientConfig::default()).unwrap());
    let series = ResourcePath {
        r#type: "series".into(),
        ..stream_path()
    };
    let mut none = client.fan_out(std::slice::from_ref(&cached), &series, Deadlines::default());
    assert!(matches!(none.next().await, Some(AddonEvent::Done)));

    let first = client
        .fetch(&cached, &stream_path(), Duration::from_secs(2))
        .await
        .unwrap();
    let second = client
        .fetch(&cached, &stream_path(), Duration::from_secs(2))
        .await
        .unwrap();
    assert!(!first.from_cache);
    assert!(second.from_cache);
}

#[tokio::test]
#[ignore = "network"]
async fn real_cinemeta_and_torrentio() {
    let client = AddonClient::new(HttpClient::new(ClientConfig::default()).unwrap());
    let cinemeta = client
        .fetch_manifest("https://v3-cinemeta.strem.io/manifest.json")
        .await
        .unwrap();
    let torrentio = client
        .fetch_manifest("stremio://torrentio.strem.fun/manifest.json")
        .await
        .unwrap();
    let mut fanout = client.fan_out(&[cinemeta, torrentio], &stream_path(), Deadlines::default());
    let mut arrived = Vec::new();
    while let Some(event) = fanout.next().await {
        if let AddonEvent::Arrived { addon_id, body, .. } = event {
            let streams = mq_protocol::decode_streams(&body).unwrap();
            arrived.push((addon_id, streams.streams.len()));
        }
    }
    assert!(
        arrived
            .iter()
            .any(|(id, n)| id.contains("torrentio") && *n > 0),
        "{arrived:?}"
    );
}
