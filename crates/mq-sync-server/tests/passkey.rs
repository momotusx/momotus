//! Passkey kits through a real relay (plans/accounts.md 3.1 and 3.2): saved
//! from a PRF output and opened with it again, how long each kind of kit
//! stays, what Report lost takes, and the refusals (another passkey, a
//! squatted lookup, a kit dropped before, a revoke that keeps an orphan).

use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::Library;
use mq_library::hlc::{HlcClock, now_ms};
use mq_sync::pair::{
    Recovery, Via, approve_pairing, cancel_pairing, make_kit, passkey_kit, recover_with,
    save_passkey, start_pairing,
};
use mq_sync::roster::{Entry, Hex, Id, Roster};
use mq_sync::wire::AppendRequest;
use mq_sync::{MemberInfo, SyncConfig, SyncError, sync_once};
use mq_sync_server::{Config, router};
use std::path::PathBuf;

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

fn fresh_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mq-passkey-{}", hex::encode(prf())));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A passkey's PRF output.
fn prf() -> [u8; 32] {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).unwrap();
    b
}

struct Relay {
    url: String,
    db: PathBuf,
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(dir) = self.db.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl Relay {
    async fn start(db: PathBuf, new_generation: bool) -> Self {
        let config = Config {
            new_generation,
            public_host: Some("127.0.0.1".into()),
            ..Config::default()
        };
        let app = router(&db, config).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
        Self { url, db }
    }

    async fn new() -> Self {
        Self::start(fresh_dir().join("relay.sqlite"), false).await
    }

    /// A backup of the database alone, restored as a new generation.
    async fn restored_from(snapshot: &PathBuf) -> Self {
        let db = fresh_dir().join("relay.sqlite");
        std::fs::rename(snapshot, &db).unwrap();
        Self::start(db, true).await
    }

    fn snapshot(&self) -> PathBuf {
        let path = fresh_dir().join("snapshot.sqlite");
        rusqlite::Connection::open(&self.db)
            .unwrap()
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        path
    }

    /// `account`'s roster as the relay holds it.
    fn roster(&self, account: &Id) -> Roster {
        let sql = rusqlite::Connection::open(&self.db).unwrap();
        let mut stmt = sql
            .prepare("SELECT body, sig FROM roster WHERE account = ?1 ORDER BY seq")
            .unwrap();
        let mut roster = Roster::default();
        let entries = stmt
            .query_map([hex::encode(account.0)], |r| {
                let sig: Vec<u8> = r.get(1)?;
                Ok(Entry {
                    body: r.get(0)?,
                    sig: Hex(sig.try_into().unwrap()),
                })
            })
            .unwrap();
        for entry in entries {
            roster.apply(&entry.unwrap()).unwrap();
        }
        roster
    }

    /// Appends `entry` as the holder of `token` would.
    async fn append(&self, token: &str, entry: Entry) -> Result<mq_http::Response, HttpError> {
        let options = RequestOptions {
            allow_private: true,
            headers: vec![("authorization".into(), format!("Bearer {token}"))],
            ..Default::default()
        };
        http()
            .post_json(
                &format!("{}/v1/roster", self.url),
                &AppendRequest { entry },
                &options,
            )
            .await
    }
}

struct Device {
    record: SyncConfig,
    library: Library,
    clock: HlcClock,
}

impl Device {
    async fn first(relay: &Relay) -> Self {
        let record = mq_sync::create_account_with(
            &http(),
            &relay.url,
            None,
            "Phone",
            None,
            false,
            Some("phone"),
        )
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

    fn id(&self) -> Id {
        self.record.secret().id()
    }

    async fn round(&mut self) -> Result<(), SyncError> {
        let report = sync_once(&self.library, &http(), &self.record, &self.clock).await?;
        if let Some(record) = report.record {
            self.record = record;
        }
        Ok(())
    }

    async fn sync(&mut self) {
        for _ in 0..3 {
            self.round().await.unwrap();
        }
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

    async fn save(&mut self, prf: &[u8; 32]) -> Result<Id, SyncError> {
        let (kit, record) = save_passkey(
            &self.library,
            &http(),
            &self.record,
            &self.clock,
            prf,
            "Passkey, saved on the phone",
        )
        .await?;
        self.record = record;
        Ok(kit)
    }

    async fn remove(&mut self, ids: Vec<Id>) {
        self.record =
            mq_sync::remove_devices(&self.library, &http(), &self.record, &self.clock, ids)
                .await
                .unwrap();
    }

    async fn sign_out(self) {
        mq_sync::sign_out(&self.library, &http(), &self.record, &self.clock, true)
            .await
            .unwrap();
    }

    async fn members(&self) -> Vec<MemberInfo> {
        mq_sync::devices(&self.library, &http(), &self.record, &self.clock)
            .await
            .unwrap()
            .members
    }

    async fn has(&self, id: &Id) -> bool {
        self.members().await.iter().any(|m| m.id == *id)
    }
}

async fn recover(
    relay: &Relay,
    way: Recovery,
    form: Option<&str>,
) -> Result<(Device, Via), SyncError> {
    let (record, via) = recover_with(&http(), &relay.url, way, "Laptop", form).await?;
    Ok((Device::with(record), via))
}

async fn unknown(relay: &Relay, prf: &[u8; 32]) -> bool {
    matches!(
        recover(relay, Recovery::Passkey(*prf), None).await,
        Err(SyncError::KitUnknown)
    )
}

/// Saved from a PRF output, opened with it on a new device: the way in is
/// `passkey`, the row shows the label and who used it last, the new device
/// its form and that it joined with a passkey. Another passkey opens
/// nothing; a recovery kit still says `kit`.
#[tokio::test]
async fn a_passkey_saves_and_opens_the_account_again() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let key = prf();
    let kit = phone.save(&key).await.unwrap();
    let (mut laptop, via) = recover(&relay, Recovery::Passkey(key), Some("computer"))
        .await
        .unwrap();
    assert_eq!(via, Via::Passkey);
    laptop.sync().await;
    phone.sync().await;
    let members = phone.members().await;
    let row = members.iter().find(|m| m.id == kit).unwrap();
    assert!(row.kit && row.passkey);
    assert_eq!(row.name.as_deref(), Some("Passkey, saved on the phone"));
    assert_eq!(row.last_used_by, Some(laptop.id()));
    let joined = members.iter().find(|m| m.id == laptop.id()).unwrap();
    assert!(joined.via_passkey);
    assert_eq!(joined.form.as_deref(), Some("computer"));
    let me = members.iter().find(|m| m.this_device).unwrap();
    assert_eq!((me.form.as_deref(), me.via_passkey), (Some("phone"), false));
    assert!(
        unknown(&relay, &prf()).await,
        "another passkey opens nothing"
    );
    let (text, _, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    let (_, via) = recover(&relay, Recovery::Kit(text), None).await.unwrap();
    assert_eq!(via, Via::Kit);
    let odd = recover(&relay, Recovery::Passkey(key), Some("toaster")).await;
    assert!(matches!(odd, Err(SyncError::Pairing(_))), "an unknown form");
}

/// Another account holds this passkey's lookup (it squatted it, without the
/// tag key): saving is `kitTaken`, and opening it finds keys that are not
/// this account's: `kitUnknown`, and nothing is used.
#[tokio::test]
async fn a_lookup_squatted_in_another_account_opens_nothing() {
    let relay = Relay::new().await;
    let mut victim = Device::first(&relay).await;
    let attacker = Device::first(&relay).await;
    let key = prf();
    let mut squat = passkey_kit(&key, None);
    squat.tag_key = [7; 32];
    let roster = relay.roster(&attacker.record.keys.account);
    let entry = roster
        .add_kit(
            &attacker.record.secret(),
            &attacker.record.keys,
            &squat,
            now_ms(),
        )
        .unwrap();
    relay
        .append(&attacker.record.device_token, entry)
        .await
        .unwrap();
    let taken = victim.save(&key).await.unwrap_err();
    assert!(matches!(taken, SyncError::KitTaken), "{taken:?}");
    assert_eq!(taken.code(), Some("kitTaken"));
    assert!(unknown(&relay, &key).await);
    let attackers = relay.roster(&attacker.record.keys.account);
    assert_eq!(
        attackers.members().len(),
        2,
        "no device joined the attacker's account"
    );
}

/// The phone saves a passkey, the new phone joins with it, the old one
/// signs out: the passkey stays. One saved on the TV, which no device
/// joined through, goes when the TV signs out. A recovery kit goes with its
/// device either way (today's rule).
#[tokio::test]
async fn a_passkey_stays_with_the_device_that_joined_through_it() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    let key = prf();
    let kit = phone.save(&key).await.unwrap();
    let (text, recovery_kit, record) =
        make_kit(&phone.library, &http(), &phone.record, &phone.clock)
            .await
            .unwrap();
    phone.record = record;
    let (mut new_phone, _) = recover(&relay, Recovery::Passkey(key), None).await.unwrap();
    let (mut desk, _) = recover(&relay, Recovery::Kit(text.clone()), None)
        .await
        .unwrap();
    let tv_key = prf();
    tv.save(&tv_key).await.unwrap();
    for device in [&mut new_phone, &mut desk, &mut tv] {
        device.sync().await;
    }
    phone.sign_out().await;
    tv.sign_out().await;
    new_phone.sync().await;
    desk.sync().await;
    assert!(
        new_phone.has(&kit).await,
        "the passkey stays with the new phone"
    );
    assert!(
        !new_phone.has(&recovery_kit).await,
        "the recovery kit went with the phone"
    );
    assert!(
        desk.has(&desk.id()).await,
        "the device the kit let in stays"
    );
    assert!(
        unknown(&relay, &tv_key).await,
        "the TV's passkey went with it"
    );
    let again = recover(&relay, Recovery::Kit(text), None).await;
    assert!(
        matches!(again, Err(SyncError::KitUnknown)),
        "{:?}",
        again.err()
    );
    assert!(recover(&relay, Recovery::Passkey(key), None).await.is_ok());
}

/// Removing a device, or cancelling its approval, drops the passkeys it
/// saved; so does taking the relay's side after a fork, for a passkey only
/// the relay's branch has.
#[tokio::test]
async fn removal_cancel_and_fork_drop_a_passkey() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    let key = prf();
    tv.save(&key).await.unwrap();
    phone.remove(vec![tv.id()]).await;
    assert!(unknown(&relay, &key).await, "removed");

    // Approved, joined, saved a passkey: the user saw other digits.
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
    let offer = joining.poll(&http()).await.unwrap().unwrap();
    let mut stranger = Device::with(joining.claim(&http(), offer, "X").await.unwrap());
    let key = prf();
    stranger.save(&key).await.unwrap();
    phone.record = cancel_pairing(
        &phone.library,
        &http(),
        &phone.record,
        &phone.clock,
        &joining.qr,
    )
    .await
    .unwrap();
    assert!(unknown(&relay, &key).await, "cancelled");

    // A fork: the relay was restored and the TV saved a passkey there.
    let mut tv = phone.pair(&relay, "TV again").await;
    tv.sync().await;
    phone.sync().await;
    let before = relay.snapshot();
    phone.remove(vec![]).await;
    let restored = Relay::restored_from(&before).await;
    for device in [&mut phone, &mut tv] {
        device.record.server = restored.url.clone();
    }
    tv.remove(vec![]).await;
    let key = prf();
    let kit = tv.save(&key).await.unwrap();
    let fork = phone.round().await.unwrap_err();
    assert!(matches!(fork, SyncError::Fork), "{fork:?}");
    phone.record = mq_sync::use_server_roster(
        &phone.library,
        &http(),
        &phone.record,
        &phone.clock,
        vec![],
        None,
    )
    .await
    .unwrap();
    assert!(
        !phone.has(&kit).await,
        "a passkey only the relay's side had"
    );
    assert!(unknown(&restored, &key).await);
}

/// A revoke that drops a device and keeps its passkey (no device joined
/// through it) is refused by the relay, as the core refuses it.
#[tokio::test]
async fn a_revoke_that_keeps_a_dropped_devices_passkey_is_refused() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    tv.save(&prf()).await.unwrap();
    phone.sync().await;
    let roster = relay.roster(&phone.record.keys.account);
    let (keeps, _) = roster
        .revoke(
            &phone.record.secret(),
            &phone.record.keys,
            vec![tv.id()],
            now_ms(),
        )
        .unwrap();
    assert!(roster.clone().apply(&keeps).is_err(), "the core refuses it");
    let refused = relay.append(&phone.record.device_token, keeps).await;
    match refused {
        Err(HttpError::Status {
            status: 400, body, ..
        }) => {
            let words = String::from_utf8_lossy(&body);
            assert!(words.contains("a kit outlives"), "{words}");
        }
        other => panic!("{other:?}"),
    }
}

/// Report lost takes every passkey, whoever saved it, and lists ticked
/// every device that joined through one, newest first, never the device
/// reporting.
#[tokio::test]
async fn report_lost_takes_every_passkey_and_ticks_who_joined_through_one() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let mut tv = phone.pair(&relay, "TV").await;
    let mine = phone.save(&prf()).await.unwrap();
    let theirs_key = prf();
    let theirs = tv.save(&theirs_key).await.unwrap();
    let mine_key = prf();
    let mine_other = phone.save(&mine_key).await.unwrap();
    let (mut desk, _) = recover(&relay, Recovery::Passkey(mine_key), None)
        .await
        .unwrap();
    let (mut laptop, _) = recover(&relay, Recovery::Passkey(theirs_key), None)
        .await
        .unwrap();
    for device in [&mut desk, &mut laptop, &mut tv, &mut phone] {
        device.sync().await;
    }
    async fn preview(device: &Device, lost: Id) -> mq_sync::LostPreview {
        mq_sync::lost_preview(
            &device.library,
            &http(),
            &device.record,
            &device.clock,
            lost,
        )
        .await
        .unwrap()
    }
    let seen = preview(&phone, tv.id()).await;
    assert_eq!(seen.passkey_joined, vec![laptop.id(), desk.id()]);
    assert!(seen.devices.contains(&desk.id()) && seen.devices.contains(&laptop.id()));
    for kit in [mine, theirs, mine_other] {
        assert!(seen.kits.contains(&kit), "every passkey");
    }
    let from_desk = preview(&desk, tv.id()).await;
    assert_eq!(
        from_desk.passkey_joined,
        vec![laptop.id()],
        "never the reporter"
    );
    assert!(!from_desk.devices.contains(&desk.id()));
    phone.record = mq_sync::report_lost(
        &phone.library,
        &http(),
        &phone.record,
        &phone.clock,
        tv.id(),
        vec![],
    )
    .await
    .unwrap();
    let left: Vec<Id> = phone.members().await.iter().map(|m| m.id).collect();
    assert_eq!(
        left,
        vec![phone.id()],
        "the phone alone, every passkey gone"
    );
}

/// What a save answers before anything is sent (3.1, 3.2): a passkey that
/// already opens the account is `kitHere` (the save probe), one dropped
/// once is `kitDropped` (never let in again: the user saves a new one), and
/// a ninth kit is `tooManyKits`.
#[tokio::test]
async fn a_save_says_here_dropped_or_too_many() {
    let relay = Relay::new().await;
    let mut phone = Device::first(&relay).await;
    let key = prf();
    let kit = phone.save(&key).await.unwrap();
    let here = phone.save(&key).await.unwrap_err();
    assert!(matches!(here, SyncError::KitHere), "{here:?}");
    phone.remove(vec![kit]).await;
    assert!(unknown(&relay, &key).await);
    let again = phone.save(&key).await.unwrap_err();
    assert!(matches!(again, SyncError::KitDropped), "{again:?}");
    let rows = relay.roster(&phone.record.keys.account).head();
    for _ in 0..8 {
        phone.save(&prf()).await.unwrap();
    }
    let full = phone.save(&prf()).await.unwrap_err();
    assert!(matches!(full, SyncError::TooManyKits), "{full:?}");
    assert_eq!(full.code(), Some("tooManyKits"));
    let after = relay.roster(&phone.record.keys.account).head();
    assert_eq!(
        after.map(|(seq, _)| seq),
        rows.map(|(seq, _)| seq + 8),
        "refusals send nothing"
    );
}

/// History is never refused for a lookup another account holds now: A
/// saved a passkey and dropped it, then B saved the same one. A's device
/// sending its roster again (as after a relay restore) is taken, and the
/// lookup stays B's.
#[tokio::test]
async fn history_is_never_refused_for_a_lookup_held_since() {
    let relay = Relay::new().await;
    let mut a = Device::first(&relay).await;
    let mut b = Device::first(&relay).await;
    let key = prf();
    let kit = a.save(&key).await.unwrap();
    a.remove(vec![kit]).await;
    b.save(&key).await.unwrap();
    let account = a.record.keys.account;
    let sql = rusqlite::Connection::open(&relay.db).unwrap();
    let mut stmt = sql
        .prepare("SELECT body, sig FROM roster WHERE account = ?1 ORDER BY seq")
        .unwrap();
    let entries: Vec<Entry> = stmt
        .query_map([hex::encode(account.0)], |r| {
            let sig: Vec<u8> = r.get(1)?;
            Ok(Entry {
                body: r.get(0)?,
                sig: Hex(sig.try_into().unwrap()),
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let replayed = http()
        .post_json(
            &format!("{}/v1/roster/replay", relay.url),
            &mq_sync::wire::ReplayRequest { account, entries },
            &RequestOptions {
                allow_private: true,
                ..Default::default()
            },
        )
        .await;
    assert!(replayed.is_ok(), "{:?}", replayed.err());
    let (joined, _) = recover(&relay, Recovery::Passkey(key), None).await.unwrap();
    assert_eq!(
        joined.record.keys.account, b.record.keys.account,
        "the lookup is still B's"
    );
}
