//! Escrow mode (`--escrow`) through a real relay and the mock provider: the
//! kit put and taken, tickets, the rules that keep a sign-in working (no
//! removal drops the escrowed kit), sign-out here, the 7-day deletion,
//! rekey; and two flags every relay has, the quota in MiB and the pull
//! page's byte cap.

#[path = "support/mock_oidc.rs"]
mod mock_oidc;

use mock_oidc::{Mock, Next, browse};
use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::hlc::{Hlc, HlcClock, now_ms};
use mq_library::{Library, ProgressRow, SyncCursor};
use mq_sync::identity::start_brokered;
use mq_sync::pair::{Recovery, make_kit, recover_with};
use mq_sync::roster::Id;
use mq_sync::wire::{PullResponse, PushRequest, Taken, WireRecord};
use mq_sync::{
    SyncConfig, SyncError, create_account_with, escrow, leave_here, remove_devices, sync_once,
};
use mq_sync_server::{Config, IdentityFile, router};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ESCROW: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const NEW_ESCROW: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

fn fresh_dir() -> PathBuf {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-escrow-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn identity(mock: &Mock, relay: &str, escrow_key: Option<&str>) -> IdentityFile {
    serde_json::from_value(json!({
        "key": KEY,
        "publicUrl": relay,
        "google": { "webClientId": "google-web", "webClientSecret": "gs", "issuer": mock.url },
        "escrowKey": escrow_key,
    }))
    .unwrap()
}

fn status(e: &SyncError) -> Option<u16> {
    match e {
        SyncError::Http(HttpError::Status { status, .. }) => Some(*status),
        _ => None,
    }
}

struct Relay {
    url: String,
    db: PathBuf,
    offset: Arc<AtomicI64>,
    mock: Mock,
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(dir) = self.db.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl Relay {
    /// A brokered sign-in as `sub`, the browser emulated.
    async fn sign_in(&self, sub: &str) -> Taken {
        sign_in(&self.mock, &self.url, sub).await
    }

    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.db).unwrap()
    }
}

/// A relay with the mock as Google, on `db`: its URL and clock offset.
async fn start(
    mock: &Mock,
    db: &Path,
    escrow_key: Option<&str>,
    escrow_on: bool,
) -> Result<(String, Arc<AtomicI64>), String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let offset = Arc::<AtomicI64>::default();
    let config = Config {
        clock_offset_ms: offset.clone(),
        public_host: Some("127.0.0.1".into()),
        identity: Some(identity(mock, &url, escrow_key)),
        escrow: escrow_on,
        housekeeping_every: Duration::from_millis(50),
        ..Config::default()
    };
    let app = router(db, config)?;
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    Ok((url, offset))
}

async fn sign_in(mock: &Mock, url: &str, sub: &str) -> Taken {
    mock.next(Next::Pick(sub.into()));
    let flow = start_brokered(&http(), url, "google", "momotus://auth", false)
        .await
        .unwrap();
    let fragment = browse(&flow.url).await;
    flow.finish(&http(), &fragment).await.unwrap()
}

struct Device {
    record: SyncConfig,
    library: Library,
    clock: HlcClock,
}

impl Device {
    /// A new account made after a sign-in as `sub`, its kit in escrow
    /// before the account counts as ready.
    async fn create(relay: &str, taken: &Taken) -> Self {
        let record = create_account_with(
            &http(),
            relay,
            None,
            "Mac",
            Some(&taken.ticket),
            false,
            Some("computer"),
        )
        .await
        .unwrap();
        let mut device = Self {
            record,
            library: Library::temporary().unwrap(),
            clock: HlcClock::new(1),
        };
        device.escrow_new_kit().await;
        device
    }

    /// Joins the account a sign-in found, with its escrowed kit only.
    async fn join(relay: &str, taken: &Taken, device: u64) -> Result<Self, SyncError> {
        assert!(taken.account.is_some(), "the sign-in finds the account");
        let kit = escrow::take(&http(), relay, &taken.ticket).await?;
        let (record, _) =
            recover_with(&http(), relay, Recovery::Kit(kit), "Phone", Some("phone")).await?;
        let mut joined = Self {
            record,
            library: Library::temporary().unwrap(),
            clock: HlcClock::new(device),
        };
        joined.sync().await;
        Ok(joined)
    }

    /// Makes a kit and puts it in escrow (replacing the one held).
    async fn escrow_new_kit(&mut self) {
        let (kit, _, record) = make_kit(&self.library, &http(), &self.record, &self.clock)
            .await
            .unwrap();
        self.record = record;
        if let Some(record) = escrow::put(&self.library, &http(), &self.record, &self.clock, &kit)
            .await
            .unwrap()
        {
            self.record = record;
        }
    }

    fn id(&self) -> Id {
        self.record.secret().id()
    }

    async fn sync(&mut self) -> mq_sync::SyncReport {
        loop {
            let report = sync_once(&self.library, &http(), &self.record, &self.clock)
                .await
                .unwrap();
            if let Some(record) = report.record.clone() {
                self.record = record;
            }
            if !report.again {
                return report;
            }
        }
    }

    async fn remove(&mut self, ids: Vec<Id>) -> Result<(), SyncError> {
        self.record =
            remove_devices(&self.library, &http(), &self.record, &self.clock, ids).await?;
        Ok(())
    }

    fn watch(&self, video: &str) {
        let row = ProgressRow {
            video_id: video,
            item_id: "tt1",
            position_ms: 5,
            duration_ms: 10,
            updated_ms: 1,
        };
        self.library
            .set_progress("p", row, &self.clock.tick(now_ms()).encode())
            .unwrap();
    }

    fn has(&self, video: &str) -> bool {
        self.library
            .changed_since(&SyncCursor::default(), 100_000)
            .unwrap()
            .iter()
            .any(|r| r.key.ends_with(video))
    }
}

#[tokio::test]
async fn a_sign_in_takes_the_escrowed_kit_once_per_ticket() {
    let relay = escrow_relay().await;
    let mut mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    mac.watch("v1");
    mac.sync().await;

    // The relay publishes its key; a kit that is not the account's is refused.
    let info: mq_sync::wire::EscrowInfo = http()
        .get(
            &format!("{}/v1/escrow", relay.url),
            &RequestOptions {
                allow_private: true,
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_ne!(info.public_key.0, [0; 32]);
    let stranger = mq_sync::pair::new_recovery_kit();
    let refused = escrow::put(&mac.library, &http(), &mac.record, &mac.clock, &stranger)
        .await
        .unwrap_err();
    assert_eq!(status(&refused), Some(403), "{refused:?}");

    // The sign-in alone brings the library to a new device.
    let taken = relay.sign_in("alice").await;
    let phone = Device::join(&relay.url, &taken, 2).await.unwrap();
    assert!(phone.has("v1"));
    // The ticket was spent; one never issued opens nothing either.
    let spent = escrow::take(&http(), &relay.url, &taken.ticket)
        .await
        .unwrap_err();
    assert!(matches!(spent, SyncError::TicketExpired), "{spent:?}");
    let forged = escrow::take(&http(), &relay.url, "not-a-ticket")
        .await
        .unwrap_err();
    assert!(matches!(forged, SyncError::TicketExpired), "{forged:?}");

    // A sign-in that finds no account keeps its ticket, for making one.
    let bob = relay.sign_in("bob").await;
    let none = escrow::take(&http(), &relay.url, &bob.ticket)
        .await
        .unwrap_err();
    assert_eq!(status(&none), Some(404), "{none:?}");
    Device::create(&relay.url, &bob).await;
    let stored: i64 = relay
        .sql()
        .query_row("SELECT COUNT(*) FROM escrow", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, 2);
}

#[tokio::test]
async fn a_relay_without_escrow_has_no_escrow_routes() {
    let mock = Mock::start(0).await;
    let dir = fresh_dir();
    let (url, _) = start(&mock, &dir.join("relay.sqlite"), Some(ESCROW), false)
        .await
        .unwrap();
    let answer = http()
        .get(
            &format!("{url}/v1/escrow"),
            &RequestOptions {
                allow_private: true,
                ..Default::default()
            },
        )
        .await;
    assert!(
        matches!(
            answer,
            Err(HttpError::Status {
                status: 404 | 405,
                ..
            })
        ),
        "{answer:?}"
    );
    // Escrow without a key in the identity file does not start.
    let refused = start(&mock, &dir.join("other.sqlite"), None, true)
        .await
        .unwrap_err();
    assert!(refused.contains("escrowKey"), "{refused}");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_kit_maker_leaves_only_after_another_kit_is_in_escrow() {
    let relay = escrow_relay().await;
    let mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    let mut phone = Device::join(&relay.url, &relay.sign_in("alice").await, 2)
        .await
        .unwrap();

    // Removing the Mac would drop the escrowed kit it made: refused, and so
    // is the Mac leaving by itself.
    let refused = phone.remove(vec![mac.id()]).await.unwrap_err();
    assert!(matches!(refused, SyncError::EscrowKit), "{refused:?}");
    let leave = mq_sync::sign_out(&mac.library, &http(), &mac.record, &mac.clock, true)
        .await
        .unwrap_err();
    assert!(matches!(leave, SyncError::EscrowKit), "{leave:?}");

    // Remove on the phone: a new kit in escrow first, then the Mac goes,
    // which rotates the key.
    phone.escrow_new_kit().await;
    let epoch = phone.record.keys.epoch;
    phone.remove(vec![mac.id()]).await.unwrap();
    assert!(
        phone.record.keys.epoch > epoch,
        "the removal rotated the key"
    );
    phone.watch("after");
    phone.sync().await;
    assert!(
        sync_once(&mac.library, &http(), &mac.record, &mac.clock)
            .await
            .is_err(),
        "the Mac is out"
    );

    // A new device still joins by the sign-in and reads the new key's records.
    let tablet = Device::join(&relay.url, &relay.sign_in("alice").await, 3)
        .await
        .unwrap();
    assert!(tablet.has("after"));
}

#[tokio::test]
async fn the_last_device_signs_out_here_and_a_sign_in_still_joins() {
    let relay = escrow_relay().await;
    let mut mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    mac.watch("v1");
    mac.sync().await;
    // Sign out here: nothing goes to the relay; the app deletes its record.
    leave_here(&mac.library, &mac.record).unwrap();
    drop(mac);

    let phone = Device::join(&relay.url, &relay.sign_in("alice").await, 2)
        .await
        .unwrap();
    assert!(phone.has("v1"));
    let devices = mq_sync::devices(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    let listed = devices.members.iter().filter(|m| !m.kit).count();
    assert_eq!(listed, 2, "the signed-out Mac stays listed until Remove");
}

#[tokio::test]
async fn a_device_signed_out_here_keeps_nothing_for_the_next_key() {
    let relay = escrow_relay().await;
    let mut mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    let phone = Device::join(&relay.url, &relay.sign_in("alice").await, 2)
        .await
        .unwrap();
    let tablet = Device::join(&relay.url, &relay.sign_in("alice").await, 3)
        .await
        .unwrap();

    leave_here(&phone.library, &phone.record).unwrap();
    // The Mac removes the tablet: a new key, boxed to every member left,
    // the signed-out phone's entry too.
    let epoch = mac.record.keys.epoch;
    mac.remove(vec![tablet.id()]).await.unwrap();
    assert!(mac.record.keys.epoch > epoch);
    mac.watch("secret");
    mac.sync().await;

    // What the phone's store holds after sign-out: no roster, no cursor,
    // no queue, nothing of the account; the keys were only in the record
    // the app deleted.
    assert_eq!(phone.library.roster_genesis().unwrap(), None);
    let left: Vec<String> = phone
        .library
        .sync_states("")
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert!(
        left.iter()
            .all(|k| k == "device_id" || k.starts_with("relay:")),
        "{left:?}"
    );
    assert_eq!(phone.library.outbox(10).unwrap().last_id, None);
    assert!(!phone.has("secret"));
}

#[tokio::test]
async fn an_app_deletion_waits_seven_days_and_takes_the_escrow_with_it() {
    let relay = escrow_relay().await;
    let mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    let mut phone = Device::join(&relay.url, &relay.sign_in("alice").await, 2)
        .await
        .unwrap();

    // From an app: scheduled, not done; every device sees it and may cancel.
    mq_sync::delete_account(&mac.library, &http(), &mac.record, &mac.clock, None)
        .await
        .unwrap();
    let due = phone.sync().await.deletion.expect("scheduled").due_ms;
    let week = 7 * DAY_MS as u64;
    assert!(due.abs_diff(now_ms() + week) < 60_000, "due in 7 days");
    mq_sync::identity::cancel_deletion(&phone.library, &http(), &phone.record, &phone.clock, false)
        .await
        .unwrap();
    assert!(phone.sync().await.deletion.is_none());

    // Asked again and left to run: after 7 days the account goes, its
    // escrowed kit with it, and the sign-in finds nothing.
    mq_sync::delete_account(&phone.library, &http(), &phone.record, &phone.clock, None)
        .await
        .unwrap();
    relay
        .offset
        .fetch_add(7 * DAY_MS + 60_000, Ordering::Relaxed);
    for _ in 0..100 {
        let gone: i64 = relay
            .sql()
            .query_row("SELECT COUNT(*) FROM gone", [], |r| r.get(0))
            .unwrap();
        if gone == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let left: i64 = relay
        .sql()
        .query_row("SELECT COUNT(*) FROM escrow", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0);
    assert_eq!(relay.sign_in("alice").await.account, None);
}

#[tokio::test]
async fn rekey_moves_every_escrowed_kit_to_a_new_key() {
    let relay = escrow_relay().await;
    let mut mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    mac.watch("v1");
    mac.sync().await;

    let old = identity(&relay.mock, &relay.url, Some(ESCROW));
    let new = identity(&relay.mock, &relay.url, Some(NEW_ESCROW));
    // The stopped relay's file (a copy: the test's relay keeps running).
    let db = relay.db.with_file_name("stopped.sqlite");
    copy_database(&relay.db, &db);
    assert_eq!(mq_sync_server::rekey(&db, &old, &new).unwrap(), 1);
    // Done once: the old key no longer names this database.
    assert!(mq_sync_server::rekey(&db, &old, &new).is_err());
    assert!(
        start(&relay.mock, &db, Some(ESCROW), true)
            .await
            .unwrap_err()
            .contains("escrow key")
    );

    // The relay started on the new key gives the kit back.
    let (url, _) = start(&relay.mock, &db, Some(NEW_ESCROW), true)
        .await
        .unwrap();
    let phone = Device::join(&url, &sign_in(&relay.mock, &url, "alice").await, 2)
        .await
        .unwrap();
    assert!(phone.has("v1"));
}

async fn escrow_relay() -> Relay {
    let mock = Mock::start(0).await;
    let db = fresh_dir().join("relay.sqlite");
    let (url, offset) = start(&mock, &db, Some(ESCROW), true).await.unwrap();
    Relay {
        url,
        db,
        offset,
        mock,
    }
}

// ---- every relay: the quota flag and the pull page's byte cap ----

fn bearer(token: &str) -> RequestOptions {
    RequestOptions {
        allow_private: true,
        max_bytes: 64 * 1024 * 1024,
        headers: vec![("authorization".into(), format!("Bearer {token}"))],
        ..Default::default()
    }
}

/// `n` records with bodies of `size` bytes, stamped now.
fn large(from: u8, n: u8, size: usize) -> Vec<WireRecord> {
    use base64::Engine as _;
    let at = Hlc {
        wall_ms: now_ms(),
        counter: 0,
        device: 1,
    }
    .encode();
    (from..from + n)
        .map(|k| WireRecord {
            coll: 3,
            key_id: hex::encode([k; 16]),
            hlc: at.clone(),
            epoch: 0,
            body: base64::engine::general_purpose::STANDARD.encode(vec![k; size]),
            seq: 0,
        })
        .collect()
}

async fn push(url: &str, account: &SyncConfig, records: Vec<WireRecord>) -> Result<(), HttpError> {
    http()
        .post_json(
            &format!("{url}/v1/push"),
            &PushRequest {
                records,
                head: account.checkpoint.hash,
            },
            &bearer(&account.device_token),
        )
        .await
        .map(drop)
}

async fn pull(url: &str, token: &str, after: u64) -> PullResponse {
    http()
        .get(
            &format!("{url}/v1/pull?after={after}&limit=500"),
            &bearer(token),
        )
        .await
        .unwrap()
        .json()
        .unwrap()
}

#[tokio::test]
async fn a_pull_page_stops_at_eight_mib_of_bodies() {
    let dir = fresh_dir();
    let app = router(&dir.join("relay.sqlite"), Config::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let account = mq_sync::create_account(&http(), &url, None, "t")
        .await
        .unwrap();
    // 13 large covers in a row, 900 KiB each.
    for (from, n) in [(1, 5), (6, 5), (11, 3)] {
        push(&url, &account, large(from, n, 900 * 1024))
            .await
            .unwrap();
    }
    let first = pull(&url, &account.device_token, 0).await;
    let bytes: usize = first.records.iter().map(|r| r.body.len() * 3 / 4).sum();
    assert_eq!(first.records.len(), 9, "9 fit in 8 MiB, the 10th does not");
    assert!(bytes <= 8 * 1024 * 1024 && first.more);
    let second = pull(&url, &account.device_token, first.last).await;
    assert_eq!(second.records.len(), 4);
    assert!(!second.more);
    let _ = std::fs::remove_dir_all(dir);
}

/// The relay process, stopped however the test ends.
struct Stopped(std::process::Child);

impl Drop for Stopped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn quota_mib_sets_what_one_account_may_store() {
    use std::io::BufRead;
    let dir = fresh_dir();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut relay = Stopped(
        std::process::Command::new(env!("CARGO_BIN_EXE_mq-sync-server"))
            .args(["--db", dir.join("relay.sqlite").to_str().unwrap()])
            .args(["--listen", &format!("127.0.0.1:{port}")])
            .args(["--quota-mib", "1"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut ready = String::new();
    std::io::BufReader::new(relay.0.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(ready.contains("sync_server_ready"), "{ready}");
    let url = format!("http://127.0.0.1:{port}");
    let account = mq_sync::create_account(&http(), &url, None, "t")
        .await
        .unwrap();
    push(&url, &account, large(1, 1, 600 * 1024)).await.unwrap();
    let over = push(&url, &account, large(2, 1, 600 * 1024))
        .await
        .unwrap_err();
    assert!(
        matches!(over, HttpError::Status { status: 413, .. }),
        "{over:?}"
    );
    drop(relay);
    let _ = std::fs::remove_dir_all(dir);
}

// ---- review round 1: replay, the last link, a dead escrow row, rekey ----

fn raw() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        ..Default::default()
    }
}

/// The relay's roster of `device`'s account, checked entry by entry.
async fn relay_roster(url: &str, device: &Device) -> mq_sync::roster::Roster {
    let page: mq_sync::wire::RosterPage = http()
        .get(
            &format!("{url}/v1/roster?from=0"),
            &bearer(&device.record.device_token),
        )
        .await
        .unwrap()
        .json()
        .unwrap();
    assert!(!page.more);
    let mut roster = mq_sync::roster::Roster::default();
    for entry in &page.entries {
        roster.apply(entry).unwrap();
    }
    roster
}

#[tokio::test]
async fn a_replay_cannot_drop_the_escrowed_kit_either() {
    let relay = escrow_relay().await;
    let mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    let mut thief = Device::join(&relay.url, &relay.sign_in("alice").await, 2)
        .await
        .unwrap();
    thief.sync().await;
    // A valid revoke of the Mac and its kits, sent where no token is asked.
    let roster = relay_roster(&relay.url, &thief).await;
    let dropped = [vec![mac.id()], roster.kits_of(&[mac.id()])].concat();
    let (entry, _) = roster
        .revoke(
            &thief.record.secret(),
            &thief.record.keys,
            dropped,
            now_ms(),
        )
        .unwrap();
    let replay = mq_sync::wire::ReplayRequest {
        account: thief.record.keys.account,
        entries: vec![entry],
    };
    let refused = http()
        .post_json(&format!("{}/v1/roster/replay", relay.url), &replay, &raw())
        .await
        .unwrap_err();
    let HttpError::Status { status, body, .. } = &refused else {
        panic!("{refused:?}");
    };
    assert_eq!(*status, 403);
    assert!(
        String::from_utf8_lossy(body).contains("escrowKit"),
        "{refused:?}"
    );
    // The listener's sign-in still opens the account.
    Device::join(&relay.url, &relay.sign_in("alice").await, 3)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_last_sign_in_link_stays_with_the_account() {
    let relay = escrow_relay().await;
    let mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    let links = mq_sync::identity::identities(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap()
        .0;
    assert_eq!(links.len(), 1);

    // Unlinked from a device: refused.
    let unlink = mq_sync::identity::unlink_identity(
        &mac.library,
        &http(),
        &mac.record,
        &mac.clock,
        links[0].link,
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(unlink, SyncError::LastLink), "{unlink:?}");

    // Moved to a new account by a sign-up with a fresh sign-in: refused.
    let ticket = relay.sign_in("alice").await.ticket;
    let moved = create_account_with(&http(), &relay.url, None, "X", Some(&ticket), true, None)
        .await
        .unwrap_err();
    assert!(matches!(moved, SyncError::LastLink), "{moved:?}");

    // Moved by another account's link: refused.
    let other = Device::create(&relay.url, &relay.sign_in("bob").await).await;
    let ticket = relay.sign_in("alice").await.ticket;
    let linked = mq_sync::identity::link_identity(
        &other.library,
        &http(),
        &other.record,
        &other.clock,
        &ticket,
        true,
    )
    .await
    .unwrap_err();
    assert!(matches!(linked, SyncError::LastLink), "{linked:?}");

    // Cancel and unlink of a deletion the web page asked for: the deletion
    // goes, the link stays.
    let asked = relay.sign_in("alice").await.ticket;
    http()
        .post_json(
            &format!("{}/v1/account/delete-request", relay.url),
            &json!({ "ticket": asked }),
            &raw(),
        )
        .await
        .unwrap();
    let mut mac = mac;
    assert!(mac.sync().await.deletion.is_some());
    mq_sync::identity::cancel_deletion(&mac.library, &http(), &mac.record, &mac.clock, true)
        .await
        .unwrap();
    assert!(mac.sync().await.deletion.is_none());
    assert!(
        relay.sign_in("alice").await.account.is_some(),
        "still linked"
    );
    let kept: i64 = relay
        .sql()
        .query_row("SELECT COUNT(*) FROM identity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 2, "alice's and bob's links");
}

#[tokio::test]
async fn a_device_puts_a_new_kit_when_the_escrowed_one_is_gone() {
    let relay = escrow_relay().await;
    let mut mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    mac.watch("v1");
    mac.sync().await;
    let (held, _) = escrow::status(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap();
    assert!(
        held.current && !held.stale && held.kit_id.is_some(),
        "{held:?}"
    );
    assert_eq!(
        escrow::ensure(&mac.library, &http(), &mac.record, &mac.clock)
            .await
            .unwrap(),
        None,
        "nothing to do"
    );

    // A restore left the row naming a kit the roster no longer has.
    relay
        .sql()
        .execute("UPDATE escrow SET kit_id = ?1", [vec![9u8; 16]])
        .unwrap();
    let (held, _) = escrow::status(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap();
    assert!(!held.current, "{held:?}");
    // A sign-in then finds no kit and keeps its ticket.
    let taken = relay.sign_in("alice").await;
    let none = escrow::take(&http(), &relay.url, &taken.ticket)
        .await
        .unwrap_err();
    assert!(matches!(none, SyncError::NoEscrow), "{none:?}");

    // The Mac's next check puts a new kit; the same ticket now joins.
    mac.record = escrow::ensure(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap()
        .expect("a new kit");
    let (held, _) = escrow::status(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap();
    assert!(held.current, "{held:?}");
    let phone = Device::join(&relay.url, &taken, 2).await.unwrap();
    assert!(phone.has("v1"));
}

#[tokio::test]
async fn after_a_rekey_every_account_replaces_and_drops_its_kit() {
    let relay = escrow_relay().await;
    let mut mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    mac.watch("v1");
    mac.sync().await;
    let (before, _) = escrow::status(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap();

    let old = identity(&relay.mock, &relay.url, Some(ESCROW));
    let new = identity(&relay.mock, &relay.url, Some(NEW_ESCROW));
    // The relay still has the database open: refused.
    let busy = mq_sync_server::rekey(&relay.db, &old, &new).unwrap_err();
    assert!(busy.contains("stop the relay"), "{busy}");
    drop(relay.sql());
    // Stopped (a relay process on another file stands in for the restart).
    let copy = relay.db.with_file_name("stopped.sqlite");
    copy_database(&relay.db, &copy);
    assert_eq!(mq_sync_server::rekey(&copy, &old, &new).unwrap(), 1);
    let (url, _) = start(&relay.mock, &copy, Some(NEW_ESCROW), true)
        .await
        .unwrap();
    let mac_there = SyncConfig {
        server: url.clone(),
        ..mac.record.clone()
    };
    // The relay's id moved with the file: the Mac follows it there.
    let (held, _) = escrow::status(&mac.library, &http(), &mac_there, &mac.clock)
        .await
        .unwrap();
    assert!(held.stale && held.current, "{held:?}");

    // The next check: a new kit in escrow, the old kit out of the roster.
    mac.record = escrow::ensure(&mac.library, &http(), &mac_there, &mac.clock)
        .await
        .unwrap()
        .expect("a new kit");
    let (after, _) = escrow::status(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap();
    assert!(after.current && !after.stale, "{after:?}");
    assert_ne!(after.kit_id, before.kit_id);
    let roster = relay_roster(&url, &mac).await;
    assert!(!roster.members().contains_key(&before.kit_id.unwrap()));
    let phone = Device::join(&url, &sign_in(&relay.mock, &url, "alice").await, 2)
        .await
        .unwrap();
    assert!(phone.has("v1"));
}

/// A consistent copy of a database another connection has open.
fn copy_database(from: &Path, to: &Path) {
    let source = rusqlite::Connection::open(from).unwrap();
    source
        .execute("VACUUM INTO ?1", [to.to_str().unwrap()])
        .unwrap();
    // Its roster journal too, or the copy starts without its entries.
    let journal = |db: &Path| db.with_extension("roster.jsonl");
    std::fs::copy(journal(from), journal(to)).unwrap();
}

#[tokio::test]
async fn the_first_link_stays_when_a_second_one_is_added() {
    let relay = escrow_relay().await;
    let mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    // A stolen session adds an identity it holds.
    let ticket = relay.sign_in("mallory").await.ticket;
    mq_sync::identity::link_identity(
        &mac.library,
        &http(),
        &mac.record,
        &mac.clock,
        &ticket,
        false,
    )
    .await
    .unwrap();
    let links = mq_sync::identity::identities(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap()
        .0;
    assert_eq!(links.len(), 2);
    let first = links
        .iter()
        .min_by_key(|l| (l.created_ms, l.link))
        .unwrap()
        .link;
    let added = links.iter().find(|l| l.link != first).unwrap().link;
    let client = http();
    let unlink = |link| {
        mq_sync::identity::unlink_identity(
            &mac.library,
            &client,
            &mac.record,
            &mac.clock,
            link,
            None,
        )
    };
    // The listener's sign-in neither goes nor moves with two links.
    let refused = unlink(first).await.unwrap_err();
    assert!(matches!(refused, SyncError::LastLink), "{refused:?}");
    let ticket = relay.sign_in("alice").await.ticket;
    let moved = create_account_with(&http(), &relay.url, None, "X", Some(&ticket), true, None)
        .await
        .unwrap_err();
    assert!(matches!(moved, SyncError::LastLink), "{moved:?}");
    // The one added later can go.
    unlink(added).await.unwrap();
    assert!(relay.sign_in("alice").await.account.is_some());
    assert!(relay.sign_in("mallory").await.account.is_none());
}

#[tokio::test]
async fn kits_a_device_made_beside_the_escrowed_one_are_removed() {
    let relay = escrow_relay().await;
    let mut mac = Device::create(&relay.url, &relay.sign_in("alice").await).await;
    // A check that made a kit and never reached its put.
    let (_, stray, record) = make_kit(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap();
    mac.record = record;
    let (held, _) = escrow::status(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap();
    assert!(held.current && !held.stale);
    mac.record = escrow::ensure(&mac.library, &http(), &mac.record, &mac.clock)
        .await
        .unwrap()
        .expect("the stray went");
    let roster = relay_roster(&relay.url, &mac).await;
    assert!(!roster.members().contains_key(&stray));
    assert!(
        roster.members().contains_key(&held.kit_id.unwrap()),
        "the escrowed kit stays"
    );
    // Nothing left to do on the next check.
    assert_eq!(
        escrow::ensure(&mac.library, &http(), &mac.record, &mac.clock)
            .await
            .unwrap(),
        None
    );
    Device::join(&relay.url, &relay.sign_in("alice").await, 2)
        .await
        .unwrap();
}
