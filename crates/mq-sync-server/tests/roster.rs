//! The roster through a real relay: what a relay (or a token that never
//! joined) cannot do, removals, sign-out, cancelled approvals, keys that
//! arrive late, and restores from backups.

use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::hlc::{HlcClock, now_ms};
use mq_library::{Library, ProgressRow};
use mq_sync::pair::{approve_pairing, make_kit, start_pairing};
use mq_sync::roster::{Entry, Hex, Roster, Sponsor};
use mq_sync::wire::{AppendRequest, PushRequest, ReplayRequest};
use mq_sync::{SyncConfig, SyncError, SyncReport, sync_once};
use mq_sync_server::{Config, router};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

fn token(token: &str) -> RequestOptions {
    RequestOptions {
        allow_private: true,
        headers: vec![("authorization".into(), format!("Bearer {token}"))],
        ..Default::default()
    }
}

fn fresh_dir() -> PathBuf {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-roster-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A relay on its own database, with its roster journal next to it.
struct Relay {
    url: String,
    db: PathBuf,
    offset: Arc<AtomicI64>,
}

impl Relay {
    async fn start(db: PathBuf, offset: Arc<AtomicI64>, new_generation: bool) -> Self {
        let config = Config {
            clock_offset_ms: offset.clone(),
            new_generation,
            ..Config::default()
        };
        let app = router(&db, config).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
        Self { url, db, offset }
    }

    async fn new() -> Self {
        Self::start(fresh_dir().join("relay.sqlite"), Arc::default(), false).await
    }

    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.db).unwrap()
    }

    fn rows(&self) -> i64 {
        self.sql()
            .query_row("SELECT COUNT(*) FROM roster", [], |r| r.get(0))
            .unwrap()
    }

    fn entries(&self) -> Vec<Entry> {
        let sql = self.sql();
        let mut stmt = sql
            .prepare("SELECT body, sig FROM roster ORDER BY seq")
            .unwrap();
        stmt.query_map([], |r| {
            let sig: Vec<u8> = r.get(1)?;
            Ok(Entry {
                body: r.get(0)?,
                sig: Hex(sig.try_into().unwrap()),
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    /// Puts `entry` at its seq in place of what the relay holds there.
    fn force(&self, entry: &Entry) {
        self.force_at(entry.parse().unwrap().seq as i64, entry);
    }

    fn force_at(&self, seq: i64, entry: &Entry) {
        self.sql()
            .execute(
                "INSERT INTO roster (account, seq, hash, body, sig)
                 SELECT id, ?1, ?2, ?3, ?4 FROM account WHERE true
                 ON CONFLICT(account, seq) DO UPDATE SET
                   hash = excluded.hash, body = excluded.body, sig = excluded.sig",
                rusqlite::params![
                    seq,
                    entry.hash().0.to_vec(),
                    entry.body,
                    entry.sig.0.to_vec()
                ],
            )
            .unwrap();
    }

    fn snapshot(&self) -> PathBuf {
        let path = fresh_dir().join("snapshot.sqlite");
        self.sql()
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        path
    }

    /// The operator restores `snapshot`, with this relay's journal or
    /// without it, and starts a new generation.
    async fn restored(&self, snapshot: &PathBuf, journal: bool) -> Self {
        let db = fresh_dir().join("relay.sqlite");
        std::fs::copy(snapshot, &db).unwrap();
        if journal {
            std::fs::copy(
                self.db.with_extension("roster.jsonl"),
                db.with_extension("roster.jsonl"),
            )
            .unwrap();
        }
        Self::start(db, self.offset.clone(), true).await
    }
}

struct Device {
    record: SyncConfig,
    library: Library,
    clock: HlcClock,
}

impl Device {
    async fn first(relay: &Relay) -> Self {
        let record = mq_sync::create_account(&http(), &relay.url, None, "Phone")
            .await
            .unwrap();
        Self::with(record)
    }

    fn with(record: SyncConfig) -> Self {
        Self {
            record,
            library: Library::temporary().unwrap(),
            clock: HlcClock::new(1),
        }
    }

    fn id(&self) -> Hex<16> {
        self.record.secret().id()
    }

    async fn round(&mut self) -> Result<SyncReport, SyncError> {
        let report = sync_once(&self.library, &http(), &self.record, &self.clock).await?;
        if let Some(record) = &report.record {
            self.record = record.clone();
        }
        Ok(report)
    }

    /// Rounds until the record settles, as an app does.
    async fn sync(&mut self) -> SyncReport {
        let mut last = self.round().await.unwrap();
        for _ in 0..3 {
            if !last.again {
                break;
            }
            last = self.round().await.unwrap();
        }
        last
    }

    async fn pair(&mut self, relay: &Relay, name: &str) -> Device {
        let joining = start_pairing(&http(), &relay.url).await.unwrap();
        let (_, record) = approve_pairing(
            &self.library,
            &http(),
            &self.record,
            &self.clock,
            &joining.qr,
        )
        .await
        .unwrap();
        self.record = record;
        let offer = joining.poll(&http()).await.unwrap().expect("approved");
        Device::with(joining.claim(&http(), offer, name).await.unwrap())
    }

    async fn remove(&mut self, ids: Vec<Hex<16>>) {
        self.record =
            mq_sync::remove_devices(&self.library, &http(), &self.record, &self.clock, ids)
                .await
                .unwrap();
    }

    fn watch(&self, video: &str, position: i64) {
        let row = ProgressRow {
            video_id: video,
            item_id: "tt1",
            position_ms: position,
            duration_ms: 1000,
            updated_ms: position,
        };
        let hlc = self.clock.tick(self.clock.wall_now()).encode();
        self.library.set_progress("p", row, &hlc).unwrap();
    }

    fn position(&self, video: &str) -> Option<i64> {
        self.library
            .progress("p", video)
            .unwrap()
            .map(|(position, _)| position)
    }
}

/// Builds an entry on the relay's roster up to (and without) `seq`.
fn prefix(relay: &Relay, seq: usize) -> Roster {
    let mut roster = Roster::default();
    for entry in &relay.entries()[..seq] {
        roster.apply(entry).unwrap();
    }
    roster
}

#[tokio::test]
async fn a_relay_cannot_slip_in_a_device_or_change_an_entry() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    tv.sync().await;

    // The relay appends a device of its own, vouched for by nobody.
    let roster = prefix(&relay, relay.rows() as usize);
    let intruder = mq_sync::roster::MemberSecret::generate();
    let fake = Sponsor {
        by: phone.id(),
        sig: Hex([1; 64]),
    };
    relay.force(&roster.add_device(&intruder, fake, "x".into(), now_ms()));
    let err = tv.round().await.unwrap_err();
    assert!(matches!(err, SyncError::Roster(_)), "{err:?}");

    // A device reading the roster from the start (a new install) refuses
    // an entry whose bytes changed.
    let mut entry = relay.entries()[1].clone();
    entry.body[10] ^= 1;
    relay.force_at(1, &entry);
    let mut fresh = Device::with(phone.record.clone());
    let err = fresh.round().await.unwrap_err();
    assert!(matches!(err, SyncError::Roster(_)), "{err:?}");
}

#[tokio::test]
async fn a_relay_that_lost_entries_gets_them_back_and_a_fork_is_caught() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    tv.sync().await;
    let (_, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    let held = relay.rows();

    // The relay drops the newest entry; the device that holds it sends it
    // back on its next sync.
    relay
        .sql()
        .execute("DELETE FROM roster WHERE seq = ?1", [held - 1])
        .unwrap();
    phone.sync().await;
    assert_eq!(relay.rows(), held);

    // The relay serves another entry at that seq, validly signed by the
    // phone (as if it rotated instead). The phone, which holds the other
    // entry, stops and says so, and refuses to push under either key; the
    // TV never saw the kit and simply follows the relay's branch (the split
    // view the design documents).
    let roster = prefix(&relay, (held - 1) as usize);
    let keys = roster
        .adopt(&phone.record.secret(), &phone.record.keys)
        .unwrap();
    let (other, _) = roster
        .revoke(&phone.record.secret(), &keys, vec![], now_ms())
        .unwrap();
    relay.force(&other);
    phone.watch("v1", 10);
    // Reading the roster alone (the devices screen) already finds it.
    let err = mq_sync::devices(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Fork), "{err:?}");
    let err = phone.round().await.unwrap_err();
    assert!(matches!(err, SyncError::Fork), "{err:?}");
    assert!(phone.library.outbox(10).unwrap().last_id.is_some(), "kept");
    tv.sync().await;
    assert_eq!(tv.record.keys.epoch, 1);
}

#[tokio::test]
async fn tokens_that_have_not_joined_can_only_join() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let (kit, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    // Redeem by hand: a pending token.
    let pending = redeem(&relay, &kit).await;
    let refused = |e: HttpError| match e {
        HttpError::Status { status, .. } => status,
        other => panic!("{other:?}"),
    };
    let push = http()
        .post_json(
            &format!("{}/v1/push", relay.url),
            &PushRequest {
                records: vec![],
                head: phone.record.checkpoint.hash,
            },
            &token(&pending),
        )
        .await
        .unwrap_err();
    assert_eq!(refused(push), 403);
    let pull = http()
        .get(&format!("{}/v1/pull", relay.url), &token(&pending))
        .await
        .unwrap_err();
    assert_eq!(refused(pull), 403);
    // Joining under an existing member's keys is refused, and the token
    // stays pending.
    let roster = prefix(&relay, relay.rows() as usize);
    let member = phone.record.secret();
    let sponsor = Sponsor::new(&member, &roster, member.public());
    let again = roster.add_device(&member, sponsor, "x".into(), now_ms());
    let err = http()
        .post_json(
            &format!("{}/v1/roster", relay.url),
            &AppendRequest { entry: again },
            &token(&pending),
        )
        .await
        .unwrap_err();
    assert_eq!(refused(err), 400);
    let pull = http()
        .get(&format!("{}/v1/pull", relay.url), &token(&pending))
        .await
        .unwrap_err();
    assert_eq!(refused(pull), 403);
}

/// The kit's secret derivation, as the app does it.
fn kit_secret(kit: &str) -> hkdf::Hkdf<sha2::Sha256> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut buffer, mut bits, mut secret) = (0u32, 0, Vec::new());
    for c in kit.chars().filter(|c| c.is_ascii_alphanumeric()) {
        let v = alphabet.iter().position(|&a| a == c as u8).unwrap() as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            secret.push((buffer >> bits) as u8);
        }
    }
    hkdf::Hkdf::new(None, &secret)
}

/// A kit is a roster member but not a device: it cannot sign for a token.
#[tokio::test]
async fn a_kit_cannot_sign_for_a_token() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let (kit, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    let mut seed = [0u8; 32];
    kit_secret(&kit)
        .expand(b"marquee/recovery/member/v1", &mut seed)
        .unwrap();
    let kit = mq_sync::roster::MemberSecret::from_seed(seed);
    let info: mq_sync::wire::RelayInfo = http()
        .get(&format!("{}/v1/relay", relay.url), &token(""))
        .await
        .unwrap()
        .json()
        .unwrap();
    let request = mq_sync::roster::TokenRequest::new(
        info.relay_id,
        phone.record.keys.account,
        kit.id(),
        info.server_ms,
    );
    let grant = mq_sync::wire::TokenGrant {
        sig: kit.sign_token(&request),
        request,
    };
    let asked = http()
        .post_json(&format!("{}/v1/tokens", relay.url), &grant, &token(""))
        .await;
    assert!(
        matches!(asked, Err(HttpError::Status { status: 403, .. })),
        "{asked:?}"
    );
}

async fn redeem(relay: &Relay, kit: &str) -> String {
    let hk = kit_secret(kit);
    let (mut lookup, mut auth) = ([0u8; 16], [0u8; 32]);
    hk.expand(b"marquee/recovery/lookup/v1", &mut lookup)
        .unwrap();
    hk.expand(b"marquee/recovery/auth/v1", &mut auth).unwrap();
    let redeemed: mq_sync::wire::Redeemed = http()
        .post_json(
            &format!("{}/v1/recovery/redeem", relay.url),
            &serde_json::json!({ "lookup": hex::encode(lookup), "auth": hex::encode(auth) }),
            &token(""),
        )
        .await
        .unwrap()
        .json()
        .unwrap();
    redeemed.device_token
}

#[tokio::test]
async fn a_dropped_device_reads_its_removal_and_keeps_its_library() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    tv.watch("v1", 300);
    tv.sync().await;
    tv.watch("v2", 400);
    phone.remove(vec![tv.id()]).await;
    let removal = relay.rows();
    let (_, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;

    // Its token reads the roster up to its removal and nothing after.
    let (seen, head) = fetch(&relay, &tv.record.device_token).await;
    assert_eq!((seen, head), (removal as usize, None));
    let err = tv.round().await.unwrap_err();
    assert!(matches!(err, SyncError::Dropped), "{err:?}");
    // Keys and sync state are gone; what it watched is not.
    assert_eq!(tv.position("v2"), Some(400));
    assert_eq!(tv.library.outbox(10).unwrap().last_id, None);
    assert_eq!(tv.library.sync_state("pull_seq").unwrap(), None);
    // And it opens nothing written after the removal.
    phone.watch("v3", 500);
    phone.sync().await;
    assert_ne!(tv.record.keys, phone.record.keys);
}

async fn fetch(relay: &Relay, with: &str) -> (usize, Option<mq_sync::wire::RosterHead>) {
    let page: mq_sync::wire::RosterPage = http()
        .get(&format!("{}/v1/roster?from=0", relay.url), &token(with))
        .await
        .unwrap()
        .json()
        .unwrap();
    (page.entries.len(), page.head)
}

#[tokio::test]
async fn an_identical_entry_sent_again_after_the_head_moved_is_accepted() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let (_, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    let kit = relay.entries().last().unwrap().clone();
    phone.remove(vec![]).await;
    let again = http()
        .post_json(
            &format!("{}/v1/roster", relay.url),
            &AppendRequest { entry: kit },
            &token(&phone.record.device_token),
        )
        .await;
    assert!(again.is_ok(), "{again:?}");
}

#[tokio::test]
async fn a_relay_claiming_a_pending_rotation_makes_no_device_rotate() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    phone.watch("v1", 100);
    let rows = relay.rows();
    relay
        .sql()
        .execute("UPDATE account SET rotation_pending = 1", [])
        .unwrap();
    let report = phone.sync().await;
    assert_eq!(report.pushed, 0, "{report:?}");
    assert_eq!(relay.rows(), rows, "no rotation");
    assert_eq!(phone.record.keys.epoch, 0);
    assert!(phone.library.outbox(10).unwrap().last_id.is_some(), "kept");
}

#[tokio::test]
async fn signing_out_sends_what_is_queued_then_the_others_rotate() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    tv.sync().await;
    tv.watch("v1", 700);
    mq_sync::sign_out(&tv.library, &http(), &tv.record, &tv.clock, false)
        .await
        .unwrap();
    phone.sync().await;
    assert_eq!(phone.position("v1"), Some(700), "sent before leaving");
    assert_eq!(phone.record.keys.epoch, 1, "a member left: a new key");
    let err = tv.round().await.unwrap_err();
    // Its token reads up to its own leaving: it knows it is out.
    assert!(matches!(err, SyncError::Dropped), "{err:?}");
}

#[tokio::test]
async fn an_approval_that_times_out_is_cancelled_by_a_rotation() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let joining = start_pairing(&http(), &relay.url).await.unwrap();
    let (_, record) = approve_pairing(
        &phone.library,
        &http(),
        &phone.record,
        &phone.clock,
        &joining.qr,
    )
    .await
    .unwrap();
    phone.record = record;
    // The new device collected the payload but never joined.
    joining.poll(&http()).await.unwrap().expect("approved");
    phone.sync().await;
    assert_eq!(phone.record.keys.epoch, 0, "not timed out yet");
    relay.offset.store(11 * 60 * 1000, Ordering::Relaxed);
    phone.sync().await;
    assert_eq!(phone.record.keys.epoch, 1, "cancelled by a rotation");
}

/// A cancellation decided while the device's joining was still on its way
/// (here: decided, then held back by the grace period after a restore)
/// drops the device once it lands.
#[tokio::test]
async fn a_cancellation_drops_a_device_whose_joining_landed_meanwhile() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let restored = relay.restored(&relay.snapshot(), true).await;
    phone.record.server = restored.url.clone();
    let mut tv = phone.pair(&restored, "TV").await;
    tv.sync().await;
    phone
        .library
        .set_sync_state(&format!("cancel:{}", hex::encode(tv.id().0)), "1")
        .unwrap();
    let report = phone.sync().await;
    assert!(report.rotation_wait_ms > 0, "{report:?}");
    assert_eq!(phone.record.keys.epoch, 0, "waits out the grace period");
    restored.offset.store(62 * 60 * 1000, Ordering::Relaxed);
    phone.sync().await;
    assert_eq!(phone.record.keys.epoch, 1);
    let err = tv.round().await.unwrap_err();
    assert!(matches!(err, SyncError::Dropped), "{err:?}");
}

#[tokio::test]
async fn a_record_waits_for_its_key_and_arrives_with_it() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    tv.sync().await;
    phone.sync().await;
    // The TV rotates and writes under the new key.
    tv.remove(vec![]).await;
    tv.sync().await;
    tv.watch("v1", 900);
    tv.sync().await;
    // The relay withholds the rotation from the phone.
    let rotation = relay.entries().last().unwrap().clone();
    let seq = rotation.parse().unwrap().seq;
    relay
        .sql()
        .execute("DELETE FROM roster WHERE seq = ?1", [seq as i64])
        .unwrap();
    relay
        .sql()
        .execute("UPDATE account SET epoch = 0", [])
        .unwrap();
    let report = phone.round().await.unwrap();
    assert_eq!(report.parked, 1, "{report:?}");
    assert_eq!(phone.position("v1"), None);
    // The rotation comes back: the parked record is read.
    relay.force(&rotation);
    relay
        .sql()
        .execute("UPDATE account SET epoch = 1", [])
        .unwrap();
    phone.sync().await;
    assert_eq!(phone.position("v1"), Some(900));
}

#[tokio::test]
async fn a_restore_without_the_journal_is_repaired_by_the_devices() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let before = relay.snapshot();
    let mut tv = phone.pair(&relay, "TV").await;
    tv.watch("v1", 120);
    tv.sync().await;
    phone.sync().await;
    // Restored more than 15 relay minutes later, with the TV's joining and
    // its token lost.
    let restored = relay.restored(&before, false).await;
    restored.offset.store(16 * 60 * 1000, Ordering::Relaxed);
    for device in [&mut phone, &mut tv] {
        device.record.server = restored.url.clone();
    }
    // The TV first: its token is unknown there, so it sends back the roster
    // it holds (its own joining included) and signs for a fresh token.
    tv.sync().await;
    phone.sync().await;
    assert_eq!(restored.rows(), relay.rows(), "the roster is whole again");
    tv.watch("v2", 240);
    tv.sync().await;
    phone.sync().await;
    assert_eq!(phone.position("v2"), Some(240));
}

#[tokio::test]
async fn a_database_restored_alone_keeps_the_roster() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let before = relay.snapshot();
    let mut tv = phone.pair(&relay, "TV").await;
    tv.sync().await;
    phone.remove(vec![tv.id()]).await;
    let restored = relay.restored(&before, true).await;
    assert_eq!(restored.rows(), relay.rows(), "from the journal");
    // So the TV is still out, and cannot come back as a member.
    tv.record.server = restored.url.clone();
    let err = tv.round().await.unwrap_err();
    assert!(matches!(err, SyncError::Dropped), "{err:?}");
    // Signing for a fresh token gets it a token that reads up to its
    // removal and cannot pull or push.
    let info: mq_sync::wire::RelayInfo = http()
        .get(&format!("{}/v1/relay", restored.url), &token(""))
        .await
        .unwrap()
        .json()
        .unwrap();
    let member = tv.record.secret();
    let request = mq_sync::roster::TokenRequest::new(
        info.relay_id,
        tv.record.keys.account,
        member.id(),
        info.server_ms,
    );
    let grant = mq_sync::wire::TokenGrant {
        sig: member.sign_token(&request),
        request,
    };
    let fresh: mq_sync::wire::AccountResponse = http()
        .post_json(&format!("{}/v1/tokens", restored.url), &grant, &token(""))
        .await
        .unwrap()
        .json()
        .unwrap();
    let pull = http()
        .get(
            &format!("{}/v1/pull", restored.url),
            &token(&fresh.device_token),
        )
        .await;
    assert!(
        matches!(pull, Err(HttpError::Status { status: 401, .. })),
        "{pull:?}"
    );
}

/// A relay at another host claiming a relay id this device met elsewhere
/// is refused (it could pass on what this device signs for that relay).
#[tokio::test]
async fn a_relay_id_belongs_to_one_host() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    phone.sync().await;
    phone.record.server = relay.url.replace("127.0.0.1", "localhost");
    let err = phone.round().await.unwrap_err();
    assert!(matches!(err, SyncError::WrongRelay), "{err:?}");
}

/// A record under an old key, stamped long after the rotation that replaced
/// that key: an honest relay never takes one, so the report warns.
#[tokio::test]
async fn a_late_write_under_an_old_key_raises_the_warning() {
    use mq_sync::seal::AccountKey;
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let old = phone.record.keys.clone();
    phone.remove(vec![]).await;
    phone.sync().await;
    // A holder of the old key, with the relay's help, adds a record.
    let key = AccountKey::from_bundle(&old);
    let late = mq_library::Hlc {
        wall_ms: now_ms() + 20 * 60 * 1000,
        counter: 0,
        device: 9,
    }
    .encode();
    let key_id = key.key_id(3, "p\u{1f}v9");
    let payload = serde_json::json!({
        "k": "p\u{1f}v9",
        "v": {"itemId": "tt1", "positionMs": 1, "durationMs": 2, "updatedMs": 1},
        "s": 1,
    });
    let body = key.seal(3, &key_id, &late, 0, &serde_json::to_vec(&payload).unwrap());
    relay
        .sql()
        .execute(
            "INSERT INTO record (account, coll, key_id, hlc, epoch, seq, body)
             SELECT id, 3, ?1, ?2, 0, 9223372036854775000, ?3 FROM account",
            rusqlite::params![key_id.to_vec(), late, body],
        )
        .unwrap();
    let report = phone.round().await.unwrap();
    assert_eq!(report.old_epoch_writes, 1, "{report:?}");
}

#[tokio::test]
async fn an_older_library_waits_for_the_checkpoint() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let before = relay.snapshot();
    phone.remove(vec![]).await;
    phone.sync().await;
    // A restored (older, here empty) library and a relay restored to before
    // the rotation, without its journal: nothing syncs.
    let restored = relay.restored(&before, false).await;
    let mut older = Device::with(SyncConfig {
        server: restored.url.clone(),
        ..phone.record.clone()
    });
    let err = older.round().await.unwrap_err();
    assert!(matches!(err, SyncError::RelayBehind), "{err:?}");
    // With the relay that holds it, the same library catches up.
    older.record.server = relay.url.clone();
    older.sync().await;
    assert_eq!(older.record.keys, phone.record.keys);
}

#[tokio::test]
async fn an_older_record_is_brought_back_up_to_date() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let old = phone.record.clone();
    phone.remove(vec![]).await;
    let newer = phone.record.clone();
    // The app stored the older record after the newer one.
    phone.record = old;
    phone.watch("v1", 5);
    let report = phone.round().await.unwrap();
    assert!(report.again);
    assert_eq!(report.pushed, 0, "nothing published before it is stored");
    assert_eq!(report.record.clone().unwrap().keys, newer.keys);
    phone.record = report.record.unwrap();
    assert_eq!(phone.round().await.unwrap().pushed, 1);
}

/// A restore that lost the roster journal too: the phone's removal of the
/// laptop is gone, and the TV (which never saw it) rotates at the same seq.
/// The phone and the laptop's other holder find the fork; taking the
/// server's roster drops the laptop again and a kit only that branch has.
#[tokio::test]
async fn after_a_fork_the_servers_roster_is_taken_and_the_drops_redone() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    let mut laptop = phone.pair(&relay, "Laptop").await;
    let mut tablet = phone.pair(&relay, "Tablet").await;
    for device in [&mut tv, &mut laptop, &mut tablet, &mut phone] {
        device.sync().await;
    }
    let before = relay.snapshot();
    phone.remove(vec![laptop.id()]).await;
    phone.watch("v1", 50);
    phone.sync().await;
    tablet.sync().await;
    let restored = relay.restored(&before, false).await;
    for device in [&mut phone, &mut tv, &mut laptop, &mut tablet] {
        device.record.server = restored.url.clone();
    }
    // The TV rotates on the restored relay and makes a kit there.
    tv.remove(vec![]).await;
    let (_, record) = make_kit(&tv.library, &http(), &tv.record, &tv.clock)
        .await
        .unwrap();
    tv.record = record;
    for device in [&mut phone, &mut tablet] {
        let err = device.round().await.unwrap_err();
        assert!(matches!(err, SyncError::Fork), "{err:?}");
    }
    // The phone takes the server's roster; the tablet does too, after it.
    for device in [&mut phone, &mut tablet] {
        device.record = mq_sync::use_server_roster(
            &device.library,
            &http(),
            &device.record,
            &device.clock,
            vec![],
        )
        .await
        .unwrap();
    }
    for _ in 0..2 {
        for device in [&mut phone, &mut tablet, &mut tv] {
            device.sync().await;
        }
    }
    let names: Vec<String> = mq_sync::devices(&tv.library, &http(), &tv.record, &tv.clock)
        .await
        .unwrap()
        .members
        .into_iter()
        .map(|m| m.name.unwrap_or_else(|| "kit".into()))
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(!names.contains(&"Laptop".to_owned()) && !names.contains(&"kit".to_owned()));
    assert_eq!(tv.record.keys, phone.record.keys);
    assert_eq!(tv.position("v1"), Some(50), "the phone's own edit arrives");
    let err = laptop.round().await.unwrap_err();
    assert!(matches!(err, SyncError::Dropped), "{err:?}");
}

#[tokio::test]
async fn a_token_from_a_replay_request_needs_no_token() {
    let relay = Relay::new().await;
    let phone = Device::first(&relay).await;
    // Anyone may send entries that continue the head; the relay checks them.
    let ok = http()
        .post_json(
            &format!("{}/v1/roster/replay", relay.url),
            &ReplayRequest {
                account: phone.record.keys.account,
                entries: relay.entries(),
            },
            &token(""),
        )
        .await;
    assert!(ok.is_ok(), "{ok:?}");
    let mut bad = relay.entries()[0].clone();
    bad.body[5] ^= 1;
    let refused = http()
        .post_json(
            &format!("{}/v1/roster/replay", relay.url),
            &ReplayRequest {
                account: phone.record.keys.account,
                entries: vec![bad],
            },
            &token(""),
        )
        .await;
    assert!(refused.is_err());
}

/// The journal lost lines (a crash between commit and journal write): the
/// next start writes the table's entries back, so a later database-only
/// restore still keeps them. A torn last line is cut, not fatal.
#[tokio::test]
async fn a_journal_missing_lines_is_completed_at_startup() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let before = relay.snapshot();
    let (_, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    let journal = relay.db.with_extension("roster.jsonl");
    let lines = std::fs::read_to_string(&journal).unwrap();
    let first = lines.lines().next().unwrap().to_owned();
    std::fs::write(&journal, format!("{first}\n{{\"torn")).unwrap();
    let restarted = Relay::start(relay.db.clone(), relay.offset.clone(), false).await;
    let completed = std::fs::read_to_string(&journal).unwrap();
    assert_eq!(
        completed.lines().count() as i64,
        relay.rows(),
        "{completed}"
    );
    let restored = restarted.restored(&before, true).await;
    assert_eq!(restored.rows(), relay.rows());
}
