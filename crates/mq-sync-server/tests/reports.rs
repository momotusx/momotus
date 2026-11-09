//! `POST /v1/reports` through a real relay (plans/error-reports.md 7, 8).

use mq_reports::{App, Batch, Channel, CoreBuild, Device, Form, Kind, Report};
use mq_sync_server::{Config, router};
use std::path::PathBuf;

/// A relay behind the trusted proxy with reports at `reports.sqlite`; every
/// request comes from `from` as Cloudflare names it.
async fn relay(with_reports: bool, from: &'static str) -> (String, PathBuf) {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-reports-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    let reports = dir.join("reports.sqlite");
    let config = Config {
        trust_proxy: true,
        reports: with_reports.then(|| reports.clone()),
        ..Config::default()
    };
    let app = router(&dir.join("relay.sqlite"), config)
        .unwrap()
        .layer(axum::middleware::from_fn(
            move |mut request: axum::extract::Request, next: axum::middleware::Next| {
                request
                    .headers_mut()
                    .insert("cf-connecting-ip", from.parse().unwrap());
                next.run(request)
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    (base, reports)
}

fn device() -> Device {
    Device {
        app: App::Android,
        channel: Channel::Release,
        version: "0.14.2".into(),
        build: "d8b973d8".into(),
        core: "d8b973d8".into(),
        os: "Android 15".into(),
        model: "Google Pixel 9 Pro".into(),
        abi: "arm64-v8a".into(),
        form: Form::Phone,
        core_build: CoreBuild::Own,
    }
}

fn batch(n: usize) -> Vec<u8> {
    let reports: Vec<Report> = (0..n)
        .map(|i| {
            let mut r = Report::new(Kind::Panic, i as u64);
            r.message = Some(format!("boom {i}"));
            r
        })
        .collect();
    serde_json::to_vec(&Batch::build(device(), &reports).0).unwrap()
}

/// The tests' browser. reqwest takes rustls without a crypto provider here;
/// mq-http's client installs the one the workspace uses (ring), once.
fn browser() -> reqwest::Client {
    let _ = mq_http::HttpClient::new(mq_http::ClientConfig::default());
    reqwest::Client::new()
}

async fn post(base: &str, body: Vec<u8>) -> (u16, Option<String>, serde_json::Value) {
    let response = browser()
        .post(format!("{base}/v1/reports"))
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let retry = response
        .headers()
        .get("retry-after")
        .map(|v| v.to_str().unwrap().to_owned());
    let text = response.text().await.unwrap_or_default();
    (
        status,
        retry,
        serde_json::from_str(&text).unwrap_or_default(),
    )
}

fn rows(db: &PathBuf) -> Vec<String> {
    let db = rusqlite::Connection::open(db).unwrap();
    let mut stmt = db.prepare("SELECT body FROM report ORDER BY id").unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[tokio::test]
async fn without_the_flag_there_is_no_route() {
    let (base, _) = relay(false, "203.0.113.9").await;
    assert_eq!(post(&base, batch(1)).await.0, 404);
}

#[tokio::test]
async fn a_batch_is_stored_with_no_address() {
    let (base, db) = relay(true, "203.0.113.9").await;
    let (status, _, answer) = post(&base, batch(2)).await;
    assert_eq!((status, answer["accepted"].as_u64()), (200, Some(2)));
    let stored = rows(&db);
    assert_eq!(stored.len(), 2);
    // Neither the database nor its write-ahead log holds the sender's address.
    for file in [db.clone(), db.with_extension("sqlite-wal")] {
        let bytes = std::fs::read(&file).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("203.0.113.9"), "{}", file.display());
    }
}

/// The relay scrubs again: a body that skipped the device's scrub (an old
/// or a hostile client) never stores a secret.
#[tokio::test]
async fn the_relay_scrubs_what_the_device_did_not() {
    let (base, db) = relay(true, "203.0.113.9").await;
    let key = "ABCDEFGH1234abcdefgh5678";
    let raw = serde_json::json!({
        "v": 1,
        "device": device(),
        "reports": [{
            "kind": "core",
            "at": 1,
            "message": format!("failed: https://torrentio.strem.fun/realdebrid={key}/manifest.json for tt0111161"),
            "stack": [format!("open /Users/robin/{key}")],
        }],
    });
    let (status, _, _) = post(&base, serde_json::to_vec(&raw).unwrap()).await;
    assert_eq!(status, 200);
    let stored = rows(&db).join("\n");
    assert!(!stored.contains(key), "{stored}");
    assert!(!stored.contains("tt0111161"), "{stored}");
    assert!(!stored.contains("robin"), "{stored}");
}

#[tokio::test]
async fn refusals_say_whether_to_keep_the_queue() {
    let (base, db) = relay(true, "203.0.113.9").await;
    let mut value: serde_json::Value = serde_json::from_slice(&batch(1)).unwrap();
    value["v"] = 2.into();
    let (status, _, answer) = post(&base, serde_json::to_vec(&value).unwrap()).await;
    assert_eq!((status, answer["code"].as_str()), (400, Some("badVersion")));
    let mut value: serde_json::Value = serde_json::from_slice(&batch(1)).unwrap();
    value["install"] = "00ff".into();
    let (status, _, answer) = post(&base, serde_json::to_vec(&value).unwrap()).await;
    assert_eq!((status, answer["code"].as_str()), (400, Some("badReport")));
    let big = vec![b' '; mq_reports::MAX_BATCH_BYTES + 1];
    assert_eq!(post(&base, big).await.0, 413);
    assert!(rows(&db).is_empty());
}

#[tokio::test]
async fn one_address_has_a_budget_a_minute() {
    let (base, _) = relay(true, "203.0.113.9").await;
    for _ in 0..6 {
        assert_eq!(post(&base, batch(1)).await.0, 200);
    }
    let (status, retry, _) = post(&base, batch(1)).await;
    assert_eq!((status, retry.as_deref()), (429, Some("60")));
}

#[tokio::test]
async fn one_address_stores_at_most_a_hundred_a_day() {
    let (base, db) = relay(true, "203.0.113.9").await;
    let mut accepted = 0;
    for _ in 0..6 {
        let (status, _, answer) = post(&base, batch(20)).await;
        assert_eq!(status, 200);
        accepted += answer["accepted"].as_u64().unwrap();
    }
    assert_eq!(accepted, 100);
    assert_eq!(rows(&db).len(), 100);
}

#[tokio::test]
async fn a_full_store_waits_a_day() {
    let (base, db) = relay(true, "203.0.113.9").await;
    // Make the file, then fill today's count straight in the database.
    assert_eq!(post(&base, batch(1)).await.0, 200);
    let today = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        / (24 * 60 * 60 * 1000)) as i64;
    {
        let mut conn = rusqlite::Connection::open(&db).unwrap();
        let tx = conn.transaction().unwrap();
        for _ in 0..20_000 {
            tx.execute(
                "INSERT INTO report (day, hour, batch, app, version, kind, grp, count, body)
                 VALUES (?1, 0, 1, 'android', '1', 'panic', 'g', 1, '{}')",
                [today],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    let (status, retry, answer) = post(&base, batch(1)).await;
    assert_eq!(
        (status, retry.as_deref(), answer["code"].as_str()),
        (503, Some("86400"), Some("reportsFull"))
    );
}

#[tokio::test]
async fn a_reports_file_that_does_not_open_leaves_sync_running() {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-reports-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    let config = Config {
        // A directory is not a database file.
        reports: Some(dir.clone()),
        ..Config::default()
    };
    let app = router(&dir.join("relay.sqlite"), config).expect("the relay starts");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let info = browser()
        .get(format!("{base}/v1/relay"))
        .send()
        .await
        .unwrap();
    assert_eq!(info.status().as_u16(), 200);
    assert_eq!(post(&base, batch(1)).await.0, 404);
}
