//! The device side against a real relay: recovery after a restore, records
//! the relay refuses, clocks that ran ahead.

use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_library::hlc::{Hlc, HlcClock, now_ms};
use mq_library::{Library, ProgressRow, SyncCursor};
use mq_sync::seal::KeyBundle;
use mq_sync::wire::AccountResponse;
use mq_sync::{SyncConfig, SyncError, sync_once};
use mq_sync_server::{Config, router};
use std::path::{Path, PathBuf};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir() -> Dir {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let path = std::env::temp_dir().join(format!("mq-engine-{}", hex::encode(b)));
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

async fn serve(db: &Path, config: Config) -> String {
    let app = router(db, config).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

async fn account(server: &str) -> SyncConfig {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let a: AccountResponse = http
        .post_json(
            &format!("{server}/v1/accounts"),
            &serde_json::json!({}),
            &local,
        )
        .await
        .unwrap()
        .json()
        .unwrap();
    SyncConfig {
        server: server.to_owned(),
        device_token: a.device_token,
        keys: KeyBundle::generate(),
    }
}

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

fn progress(lib: &Library, video: &str, at: &str) {
    let row = ProgressRow {
        video_id: video,
        item_id: "tt1",
        position_ms: 5,
        duration_ms: 10,
        updated_ms: 1,
    };
    lib.set_progress("p", row, at).unwrap();
}

fn registers(lib: &Library) -> usize {
    lib.changed_since(&SyncCursor::default(), 100_000)
        .unwrap()
        .len()
}

#[tokio::test]
async fn a_relay_restored_from_backup_gets_everything_back() {
    let dir = dir();
    let db = dir.0.join("relay.sqlite");
    let server = serve(&db, Config::default()).await;
    let config = account(&server).await;
    let a = Library::temporary().unwrap();
    let clock = HlcClock::new(1);
    for v in ["v1", "v2", "v3"] {
        progress(&a, v, &clock.tick(now_ms()).encode());
    }
    sync_once(&a, &http(), &config, &clock).await.unwrap();

    // The operator restores a backup that lost two records and starts the
    // relay with a new generation.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute(
            "DELETE FROM record WHERE rowid IN (SELECT rowid FROM record LIMIT 2)",
            [],
        )
        .unwrap();
    let restored = serve(
        &db,
        Config {
            new_generation: true,
            ..Config::default()
        },
    )
    .await;
    let config = SyncConfig {
        server: restored,
        ..config
    };
    let report = sync_once(&a, &http(), &config, &clock).await.unwrap();
    assert!(report.pushed >= 2, "{report:?}");

    let b = Library::temporary().unwrap();
    sync_once(&b, &http(), &config, &HlcClock::new(2))
        .await
        .unwrap();
    assert_eq!(registers(&b), registers(&a));
}

#[tokio::test]
async fn a_refused_record_does_not_cut_a_device_off() {
    let dir = dir();
    let server = serve(&dir.0.join("relay.sqlite"), Config::default()).await;
    let config = account(&server).await;
    let a = Library::temporary().unwrap();
    let clock = HlcClock::new(1);
    // Larger than any body the relay takes.
    let huge = serde_json::to_string(&"x".repeat(1_200_000)).unwrap();
    a.set_setting("p", "notes", &huge, &clock.tick(now_ms()).encode())
        .unwrap();
    progress(&a, "v1", &clock.tick(now_ms()).encode());
    let report = sync_once(&a, &http(), &config, &clock).await.unwrap();
    assert_eq!((report.refused, report.pushed), (1, 1), "{report:?}");
    // Dropped, not retried forever.
    assert_eq!(a.outbox(500).unwrap().last_id, None);

    let b = Library::temporary().unwrap();
    progress(&b, "v2", &HlcClock::new(2).tick(now_ms()).encode());
    let report = sync_once(&b, &http(), &config, &HlcClock::new(2))
        .await
        .unwrap();
    assert_eq!(report.applied, 1, "B still pulls A's progress");
}

#[tokio::test]
async fn a_write_from_a_clock_far_ahead_is_restamped_and_delivered() {
    let dir = dir();
    let server = serve(&dir.0.join("relay.sqlite"), Config::default()).await;
    let config = account(&server).await;
    let a = Library::temporary().unwrap();
    let clock = HlcClock::new(1);
    let ahead = Hlc {
        wall_ms: now_ms() + 3_600_000,
        counter: 0,
        device: 1,
    };
    progress(&a, "v1", &ahead.encode());
    let report = sync_once(&a, &http(), &config, &clock).await.unwrap();
    assert_eq!(report.pushed, 1, "{report:?}");
    let stored = a.changed_since(&SyncCursor::default(), 10).unwrap();
    let hlc = Hlc::decode(&stored[0].hlc).unwrap();
    assert!(hlc.wall_ms <= now_ms() + 1000, "restamped near relay time");

    let b = Library::temporary().unwrap();
    sync_once(&b, &http(), &config, &HlcClock::new(2))
        .await
        .unwrap();
    assert_eq!(b.progress("p", "v1").unwrap(), Some((5, 10)));
}

#[tokio::test]
async fn bearer_tokens_never_go_to_a_public_relay_in_clear_text() {
    let config = SyncConfig {
        server: "http://sync.example.com".into(),
        device_token: "t".into(),
        keys: KeyBundle::generate(),
    };
    let lib = Library::temporary().unwrap();
    let err = sync_once(&lib, &http(), &config, &HlcClock::new(1))
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Insecure), "{err:?}");
}
