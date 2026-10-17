//! The device side against a real relay: recovery after a restore, records
//! the relay refuses, clocks that ran ahead.

use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_library::hlc::{Hlc, HlcClock, now_ms};
use mq_library::{Library, ProgressRow, SyncCursor};
use mq_sync::seal::KeyBundle;
use mq_sync::wire::{AccountResponse, MAX_CLOCK_AHEAD_MS};
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
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
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
    assert!(
        hlc.wall_ms <= now_ms() + MAX_CLOCK_AHEAD_MS + 1000,
        "restamped at the edge of what the relay takes"
    );

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

/// A relay that refuses every record as "clock ahead" and claims a time near
/// the end of what 48 bits hold.
async fn lying_relay() -> String {
    use axum::Json;
    use axum::routing::{get, post};
    use mq_sync::wire::{PullResponse, PushRequest, PushResponse, Refusal, Rejected};
    const FAR: u64 = 0xffff_ffff_fff0;
    let app = axum::Router::new()
        .route(
            "/v1/push",
            post(|Json(request): Json<PushRequest>| async move {
                Json(PushResponse {
                    accepted: 0,
                    stale: 0,
                    stale_at: vec![],
                    rejected: request
                        .records
                        .iter()
                        .map(|r| Rejected {
                            key_id: r.key_id.clone(),
                            reason: "ahead".into(),
                            code: Refusal::ClockAhead,
                            current: None,
                        })
                        .collect(),
                    server_ms: FAR,
                })
            }),
        )
        .route(
            "/v1/pull",
            get(|| async {
                Json(PullResponse {
                    records: vec![],
                    last: 0,
                    head: 0,
                    generation: "g".into(),
                    more: false,
                    server_ms: FAR,
                })
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    base
}

#[tokio::test]
async fn a_lying_relay_cannot_break_clocks_or_drop_writes() {
    let config = SyncConfig {
        server: lying_relay().await,
        device_token: "t".into(),
        keys: KeyBundle::generate(),
    };
    let lib = Library::temporary().unwrap();
    let clock = HlcClock::new(1);
    progress(&lib, "v1", &clock.tick(now_ms()).encode());
    let err = sync_once(&lib, &http(), &config, &clock).await.unwrap_err();
    assert!(matches!(err, SyncError::Rejected(_)), "{err:?}");
    // Every stored and new timestamp still has 32 lowercase hex digits.
    let stored = lib.changed_since(&SyncCursor::default(), 10).unwrap();
    assert!(Hlc::decode(&stored[0].hlc).is_some(), "{:?}", stored[0].hlc);
    let next = clock.tick(now_ms()).encode();
    assert_eq!(next.len(), 32);
    assert!(next > stored[0].hlc);
    // The refused write is still queued for an honest relay.
    assert!(lib.outbox(10).unwrap().last_id.is_some());
}

#[tokio::test]
async fn a_relay_with_a_slow_clock_loses_no_edits() {
    let dir = dir();
    // A Pi whose clock has not synced yet: 30 minutes behind every device.
    let server = serve(
        &dir.0.join("relay.sqlite"),
        Config {
            clock_offset_ms: -30 * 60 * 1000,
            ..Config::default()
        },
    )
    .await;
    let config = account(&server).await;
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    let (clock_a, clock_b) = (HlcClock::new(1), HlcClock::new(2));
    progress(&a, "v1", &clock_a.tick(clock_a.wall_now()).encode());
    let report = sync_once(&a, &http(), &config, &clock_a).await.unwrap();
    assert_eq!(report.pushed, 1, "restamped and accepted: {report:?}");
    sync_once(&b, &http(), &config, &clock_b).await.unwrap();
    assert_eq!(b.progress("p", "v1").unwrap(), Some((5, 10)));

    // B edits the same register later; its edit must win everywhere.
    let row = ProgressRow {
        video_id: "v1",
        item_id: "tt1",
        position_ms: 7,
        duration_ms: 10,
        updated_ms: 2,
    };
    b.set_progress("p", row, &clock_b.tick(clock_b.wall_now()).encode())
        .unwrap();
    let report = sync_once(&b, &http(), &config, &clock_b).await.unwrap();
    assert_eq!(report.pushed, 1, "{report:?}");
    sync_once(&a, &http(), &config, &clock_a).await.unwrap();
    assert_eq!(a.progress("p", "v1").unwrap(), Some((7, 10)));
    assert_eq!(registers(&a), registers(&b));
}

#[tokio::test]
async fn another_devices_version_is_never_restamped() {
    let dir = dir();
    let server = serve(&dir.0.join("relay.sqlite"), Config::default()).await;
    let config = account(&server).await;
    let lib = Library::temporary().unwrap();
    // Pulled from a fast device (99) on an earlier relay, then queued for
    // this one: the relay refuses its clock.
    let foreign = Hlc {
        wall_ms: now_ms() + 3_600_000,
        counter: 0,
        device: 99,
    };
    progress(&lib, "v1", &foreign.encode());
    let err = sync_once(&lib, &http(), &config, &HlcClock::new(1))
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Rejected(_)), "{err:?}");
    let stored = lib.changed_since(&SyncCursor::default(), 10).unwrap();
    assert_eq!(stored[0].hlc, foreign.encode(), "left as it was");
    assert!(lib.outbox(10).unwrap().last_id.is_some(), "kept for later");
}

#[tokio::test]
async fn a_fresh_import_waits_for_a_slow_relay_instead_of_vanishing() {
    let dir = dir();
    let db = dir.0.join("relay.sqlite");
    let slow = Config {
        clock_offset_ms: -30 * 60 * 1000,
        ..Config::default()
    };
    let config = account(&serve(&db, slow).await).await;
    let a = Library::temporary().unwrap();
    // What the user watched five minutes ago, imported from Stremio
    // (device 0 stamps it with Stremio's time).
    let imported = Hlc {
        wall_ms: now_ms() - 5 * 60 * 1000,
        counter: 0,
        device: 0,
    };
    progress(&a, "v1", &imported.encode());
    let clock = HlcClock::new(1);
    let err = sync_once(&a, &http(), &config, &clock).await.unwrap_err();
    assert!(matches!(err, SyncError::Rejected(_)), "{err:?}");
    assert!(
        a.outbox(10).unwrap().last_id.is_some(),
        "still queued, not dropped"
    );

    // Once the relay's clock is right, the import goes through untouched.
    let fixed = serve(&db, Config::default()).await;
    let config = SyncConfig {
        server: fixed,
        ..config
    };
    let report = sync_once(&a, &http(), &config, &clock).await.unwrap();
    assert_eq!(report.pushed, 1, "{report:?}");
    let b = Library::temporary().unwrap();
    sync_once(&b, &http(), &config, &HlcClock::new(2))
        .await
        .unwrap();
    let stored = b.changed_since(&SyncCursor::default(), 10).unwrap();
    assert_eq!(stored[0].hlc, imported.encode(), "never restamped");
}

/// Documents the limit of a relay whose clock is more than 10 minutes slow:
/// fresh edits are stamped at the relay's time, so an older edit made
/// offline can win. Devices see the clock gap and can say so.
#[tokio::test]
async fn with_a_slow_relay_last_writer_falls_back_to_push_order() {
    let dir = dir();
    let server = serve(
        &dir.0.join("relay.sqlite"),
        Config {
            clock_offset_ms: -30 * 60 * 1000,
            ..Config::default()
        },
    )
    .await;
    let config = account(&server).await;
    let (phone, tv) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    let (phone_clock, tv_clock) = (HlcClock::new(1), HlcClock::new(2));
    // The TV edited offline 25 minutes ago; the phone edits now and syncs
    // first.
    let offline = Hlc {
        wall_ms: now_ms() - 25 * 60 * 1000,
        counter: 0,
        device: 2,
    };
    let row = |pos| ProgressRow {
        video_id: "v1",
        item_id: "tt1",
        position_ms: pos,
        duration_ms: 10,
        updated_ms: pos,
    };
    tv.set_progress("p", row(3), &offline.encode()).unwrap();
    phone
        .set_progress(
            "p",
            row(8),
            &phone_clock.tick(phone_clock.wall_now()).encode(),
        )
        .unwrap();
    let report = sync_once(&phone, &http(), &config, &phone_clock)
        .await
        .unwrap();
    assert!(report.clock_gap_ms > 25 * 60 * 1000, "{report:?}");
    sync_once(&tv, &http(), &config, &tv_clock).await.unwrap();
    sync_once(&phone, &http(), &config, &phone_clock)
        .await
        .unwrap();
    // Both agree, on the TV's older edit: the phone's was stamped at the
    // relay's (slow) time.
    assert_eq!(registers(&phone), registers(&tv));
    assert_eq!(phone.progress("p", "v1").unwrap(), Some((3, 10)));
}
