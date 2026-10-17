//! The device side against a real relay: recovery after a restore, records
//! the relay refuses, clocks that ran ahead.

use mq_http::{ClientConfig, HttpClient};
use mq_library::hlc::{Hlc, HlcClock, now_ms};
use mq_library::{Library, ProgressRow, SyncCursor};
use mq_sync::roster::{Entry, Hex, MemberSecret, Roster};
use mq_sync::wire::{MAX_CLOCK_AHEAD_MS, RosterHead};
use mq_sync::{SyncConfig, SyncError, sync_once};
use mq_sync_server::{Config, router};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

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
    mq_sync::create_account(&http(), server, None, "test")
        .await
        .unwrap()
}

/// A record for an account that exists only here (no relay was asked).
fn offline(server: &str) -> (SyncConfig, Entry) {
    let member = MemberSecret::generate();
    let (entry, keys) = Roster::genesis_entry(&member, Hex([1; 16]), "t", now_ms());
    let config = SyncConfig {
        server: server.into(),
        device_token: "t".into(),
        member: Hex(member.seed()),
        keys,
        checkpoint: RosterHead {
            seq: 0,
            hash: entry.hash(),
        },
        relay_id: Hex([7; 16]),
        revision: 1,
    };
    (config, entry)
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
    let (config, _) = offline("http://sync.example.com");
    let lib = Library::temporary().unwrap();
    let err = sync_once(&lib, &http(), &config, &HlcClock::new(1))
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Insecure), "{err:?}");
}

/// A relay that refuses every record as "clock ahead" and claims a time near
/// the end of what 48 bits hold. It serves `genesis` as the roster.
async fn lying_relay(genesis: Entry) -> String {
    use axum::Json;
    use axum::routing::{get, post};
    use mq_sync::wire::{
        PullResponse, PushRequest, PushResponse, Refusal, Rejected, RelayInfo, RosterPage,
    };
    const FAR: u64 = 0xffff_ffff_fff0;
    let head = RosterHead {
        seq: 0,
        hash: genesis.hash(),
    };
    let app = axum::Router::new()
        .route(
            "/v1/relay",
            get(|| async {
                Json(RelayInfo {
                    relay_id: Hex([7; 16]),
                    generation: "g".into(),
                    generation_since_ms: 0,
                    server_ms: FAR,
                })
            }),
        )
        .route(
            "/v1/roster",
            get(move || {
                let entries = vec![genesis.clone()];
                async move {
                    Json(RosterPage {
                        from: 0,
                        entries,
                        head: Some(head),
                        more: false,
                    })
                }
            }),
        )
        .route(
            "/v1/push",
            post(move |Json(request): Json<PushRequest>| async move {
                Json(PushResponse {
                    epoch: 0,
                    roster: Some(head),
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
            get(move || async move {
                Json(PullResponse {
                    records: vec![],
                    last: 0,
                    head: 0,
                    generation: "g".into(),
                    more: false,
                    server_ms: FAR,
                    epoch: 0,
                    roster: Some(head),
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
    let (config, genesis) = offline("http://127.0.0.1");
    let config = SyncConfig {
        server: lying_relay(genesis).await,
        ..config
    };
    let lib = Library::temporary().unwrap();
    let clock = HlcClock::new(1);
    progress(&lib, "v1", &clock.tick(now_ms()).encode());
    let report = sync_once(&lib, &http(), &config, &clock).await.unwrap();
    assert_eq!(report.waiting, 1, "{report:?}");
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
            clock_offset_ms: Arc::new((-30 * 60 * 1000).into()),
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
    let report = sync_once(&lib, &http(), &config, &HlcClock::new(1))
        .await
        .unwrap();
    assert_eq!((report.waiting, report.refused), (1, 0), "{report:?}");
    let stored = lib.changed_since(&SyncCursor::default(), 10).unwrap();
    assert_eq!(stored[0].hlc, foreign.encode(), "left as it was");
    assert!(lib.outbox(10).unwrap().last_id.is_some(), "kept for later");
}

#[tokio::test]
async fn a_fresh_import_waits_for_a_slow_relay_instead_of_vanishing() {
    let dir = dir();
    let db = dir.0.join("relay.sqlite");
    let slow = Config {
        clock_offset_ms: Arc::new((-30 * 60 * 1000).into()),
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
    let report = sync_once(&a, &http(), &config, &clock).await.unwrap();
    assert_eq!((report.waiting, report.pushed), (1, 0), "{report:?}");
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
            clock_offset_ms: Arc::new((-30 * 60 * 1000).into()),
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

/// A relay that ran three hours fast (devices took its time, so registers
/// are stamped ahead) keeps taking edits to those registers once its clock
/// is fixed, instead of making them wait three hours.
#[tokio::test]
async fn a_register_written_while_the_relay_ran_fast_still_takes_edits() {
    let dir = dir();
    let offset = Arc::new(AtomicI64::new(3 * 3_600_000));
    let fast = Config {
        clock_offset_ms: offset.clone(),
        ..Config::default()
    };
    let config = account(&serve(&dir.0.join("relay.sqlite"), fast).await).await;
    let lib = Library::temporary().unwrap();
    let clock = HlcClock::new(1);
    sync_once(&lib, &http(), &config, &clock).await.unwrap();
    progress(&lib, "v1", &clock.tick(clock.wall_now()).encode());
    let report = sync_once(&lib, &http(), &config, &clock).await.unwrap();
    assert_eq!(report.pushed, 1, "{report:?}");

    // The operator fixes the relay's clock; the device's clock never goes
    // back, so its next edit is still stamped three hours ahead.
    offset.store(0, Ordering::Relaxed);
    progress(&lib, "v1", &clock.tick(clock.wall_now()).encode());
    let report = sync_once(&lib, &http(), &config, &clock).await.unwrap();
    assert_eq!((report.pushed, report.waiting), (1, 0), "{report:?}");
}

/// On a slow relay, an edit made after seeing a newer version than the
/// relay's time (an import from 25 minutes ago) is restamped just above
/// that version, so it still wins.
#[tokio::test]
async fn a_restamp_lands_above_the_version_it_replaces() {
    let dir = dir();
    let server = serve(
        &dir.0.join("relay.sqlite"),
        Config {
            clock_offset_ms: Arc::new((-30 * 60 * 1000).into()),
            ..Config::default()
        },
    )
    .await;
    let config = account(&server).await;
    let (tv, phone) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    let (tv_clock, phone_clock) = (HlcClock::new(1), HlcClock::new(2));
    let row = |pos| ProgressRow {
        video_id: "v1",
        item_id: "tt1",
        position_ms: pos,
        duration_ms: 10,
        updated_ms: pos,
    };
    let imported = Hlc {
        wall_ms: now_ms() - 25 * 60 * 1000,
        counter: 0,
        device: 0,
    };
    tv.set_progress("p", row(3), &imported.encode()).unwrap();
    sync_once(&tv, &http(), &config, &tv_clock).await.unwrap();
    sync_once(&phone, &http(), &config, &phone_clock)
        .await
        .unwrap();
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
    assert_eq!((report.pushed, report.waiting), (1, 0), "{report:?}");
    sync_once(&tv, &http(), &config, &tv_clock).await.unwrap();
    assert_eq!(tv.progress("p", "v1").unwrap(), Some((8, 10)));
    assert_eq!(registers(&phone), registers(&tv));
}

/// A relay whose pull answers name another roster at the device's head
/// (it moved to another history mid-round) is a fork, also when the roster
/// read before looked fine.
#[tokio::test]
async fn a_pull_naming_another_roster_at_the_head_is_a_fork() {
    use axum::Json;
    use axum::routing::get;
    use mq_sync::wire::{PullResponse, RelayInfo, RosterPage};
    let (config, genesis) = offline("http://127.0.0.1");
    let head = RosterHead {
        seq: 0,
        hash: genesis.hash(),
    };
    let app = axum::Router::new()
        .route(
            "/v1/relay",
            get(|| async {
                Json(RelayInfo {
                    relay_id: Hex([7; 16]),
                    generation: "g".into(),
                    generation_since_ms: 0,
                    server_ms: now_ms(),
                })
            }),
        )
        .route(
            "/v1/roster",
            get(move || {
                let entries = vec![genesis.clone()];
                async move {
                    Json(RosterPage {
                        from: 0,
                        entries,
                        head: Some(head),
                        more: false,
                    })
                }
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
                    server_ms: now_ms(),
                    epoch: 0,
                    roster: Some(RosterHead {
                        seq: 0,
                        hash: Hex([3; 32]),
                    }),
                })
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let config = SyncConfig { server, ..config };
    let lib = Library::temporary().unwrap();
    let err = mq_sync::pull_changes(&lib, &http(), &config, &HlcClock::new(1))
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Fork), "{err:?}");
}

/// Serves `app` as a relay with id 7s, answering `/v1/relay` itself.
async fn fake(app: axum::Router) -> String {
    use axum::Json;
    use axum::routing::get;
    let app = app.route(
        "/v1/relay",
        get(|| async {
            Json(mq_sync::wire::RelayInfo {
                relay_id: Hex([7; 16]),
                generation: "g".into(),
                generation_since_ms: 0,
                server_ms: now_ms(),
            })
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    base
}

/// A push answer naming another roster at this device's head is a fork:
/// nothing counts as sent, and every later call fails the same way (also
/// after a restart) until the user settles it, whatever the relay says.
#[tokio::test]
async fn a_push_answered_for_another_roster_is_a_lasting_fork() {
    use axum::Json;
    use axum::routing::{get, post};
    use mq_sync::wire::{PushResponse, RosterPage};
    let (config, genesis) = offline("http://127.0.0.1");
    let head = RosterHead {
        seq: 0,
        hash: genesis.hash(),
    };
    let app = axum::Router::new()
        .route(
            "/v1/roster",
            get(move || {
                let entries = vec![genesis.clone()];
                async move {
                    Json(RosterPage {
                        from: 0,
                        entries,
                        head: Some(head),
                        more: false,
                    })
                }
            }),
        )
        .route(
            "/v1/push",
            post(|| async {
                Json(PushResponse {
                    accepted: 1,
                    stale: 0,
                    stale_at: vec![],
                    rejected: vec![],
                    server_ms: now_ms(),
                    epoch: 0,
                    roster: Some(RosterHead {
                        seq: 0,
                        hash: Hex([3; 32]),
                    }),
                })
            }),
        );
    let config = SyncConfig {
        server: fake(app).await,
        ..config
    };
    let dir = dir();
    let path = dir.0.join("library.sqlite");
    let lib = Library::open(&path).unwrap();
    let clock = HlcClock::new(1);
    progress(&lib, "v1", &clock.tick(now_ms()).encode());
    let err = mq_sync::push_changes(&lib, &http(), &config, &clock)
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Fork), "{err:?}");
    assert!(lib.outbox(10).unwrap().last_id.is_some(), "not sent");
    // The relay's history is kept next to the device's own.
    let theirs = format!("fork:{}", hex::encode(config.keys.genesis.0));
    assert_eq!(lib.roster_entries(&theirs).unwrap().len(), 1);
    drop(lib);
    let lib = Library::open(&path).unwrap();
    let err = mq_sync::pull_changes(&lib, &http(), &config, &clock)
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Fork), "{err:?}");
}

/// A relay that says "more" without serving entries, and names a head it
/// never serves, is not followed for ever.
#[tokio::test]
async fn a_relay_naming_entries_it_never_serves_is_not_followed() {
    use axum::Json;
    use axum::extract::Query;
    use axum::routing::get;
    use mq_sync::wire::RosterPage;
    use std::collections::HashMap;
    let (config, genesis) = offline("http://127.0.0.1");
    let app = axum::Router::new().route(
        "/v1/roster",
        get(move |Query(query): Query<HashMap<String, u64>>| {
            let from = query.get("from").copied().unwrap_or(0);
            let entries = if from == 0 {
                vec![genesis.clone()]
            } else {
                vec![]
            };
            async move {
                Json(RosterPage {
                    from,
                    entries,
                    head: Some(RosterHead {
                        seq: 5,
                        hash: Hex([9; 32]),
                    }),
                    more: true,
                })
            }
        }),
    );
    let config = SyncConfig {
        server: fake(app).await,
        ..config
    };
    let lib = Library::temporary().unwrap();
    let done = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        mq_sync::pull_changes(&lib, &http(), &config, &HlcClock::new(1)),
    )
    .await
    .expect("gave up");
    assert!(matches!(done, Err(SyncError::Roster(_))), "{done:?}");
}

/// A relay that never takes the entries it lost is behind for good, after a
/// few tries.
#[tokio::test]
async fn a_relay_that_keeps_refusing_lost_entries_is_behind() {
    use axum::Json;
    use axum::routing::{get, post};
    use mq_sync::wire::RosterPage;
    use std::sync::atomic::AtomicUsize;
    let (config, genesis) = offline("http://127.0.0.1");
    let head = RosterHead {
        seq: 0,
        hash: genesis.hash(),
    };
    // The device holds a second entry the relay lost.
    let member = config.secret();
    let mut roster = Roster::default();
    roster.apply(&genesis).unwrap();
    let (second, _) = roster
        .revoke(&member, &config.keys, vec![], now_ms())
        .unwrap();
    let lib = Library::temporary().unwrap();
    let key = hex::encode(config.keys.genesis.0);
    for (seq, entry) in [&genesis, &second].into_iter().enumerate() {
        lib.add_roster_entry(&key, seq as u64, &entry.body, &entry.sig.0)
            .unwrap();
    }
    let replays = Arc::new(AtomicUsize::new(0));
    let counted = replays.clone();
    let app = axum::Router::new()
        .route(
            "/v1/roster",
            get(move || async move {
                Json(RosterPage {
                    from: 0,
                    entries: vec![],
                    head: Some(head),
                    more: false,
                })
            }),
        )
        .route(
            "/v1/roster/replay",
            post(move || {
                counted.fetch_add(1, Ordering::Relaxed);
                async { Json(serde_json::json!({})) }
            }),
        );
    let config = SyncConfig {
        server: fake(app).await,
        ..config
    };
    let done = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        mq_sync::pull_changes(&lib, &http(), &config, &HlcClock::new(1)),
    )
    .await
    .expect("gave up");
    assert!(matches!(done, Err(SyncError::RelayBehind)), "{done:?}");
    assert_eq!(replays.load(Ordering::Relaxed), 3);
}
