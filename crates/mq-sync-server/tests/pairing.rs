//! Pairing a new device and recovering an account through a real relay.

use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::Library;
use mq_library::hlc::HlcClock;
use mq_sync::pair::{approve_pairing, cancel_pairing, make_kit, recover, start_pairing};
use mq_sync::{
    Announced, Announcement, SyncConfig, SyncError, ack_announcements, devices, remove_devices,
    report_lost, sync_once,
};
use mq_sync_server::{Config, router};
use std::path::PathBuf;

async fn relay() -> (String, PathBuf) {
    relay_with(Config::default()).await
}

async fn relay_with(config: Config) -> (String, PathBuf) {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-pair-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("relay.sqlite");
    let app = router(&db, config).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    (base, db)
}

/// Tokens that can push: one per device that joined.
fn active(db: &PathBuf) -> i64 {
    rusqlite::Connection::open(db)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM device WHERE state = 'active'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

fn local() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        ..Default::default()
    }
}

/// A device with its own account on the relay, and its library and clock.
struct Device {
    record: SyncConfig,
    library: Library,
    clock: HlcClock,
}

impl Device {
    async fn first(server: &str, name: &str) -> Self {
        let record = mq_sync::create_account(&http(), server, None, name)
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

    /// Syncs until the record settles, storing it like an app does. The
    /// announcements of every round are kept once each, by id, as an app
    /// keeps them.
    async fn sync(&mut self) -> mq_sync::SyncReport {
        let mut report = None;
        let mut heard: Vec<Announcement> = Vec::new();
        for _ in 0..3 {
            let r = sync_once(&self.library, &http(), &self.record, &self.clock)
                .await
                .unwrap();
            let again = r.again;
            if let Some(record) = r.record.clone() {
                self.record = record;
            }
            for a in &r.announcements {
                if !heard.iter().any(|h| h.id == a.id) {
                    heard.push(a.clone());
                }
            }
            report = Some(r);
            if !again {
                break;
            }
        }
        mq_sync::SyncReport {
            announcements: heard,
            ..report.unwrap()
        }
    }

    /// What the next sync announces, each then acknowledged as an app's OK.
    async fn heard(&mut self) -> Vec<(Announced, Option<String>)> {
        let report = self.sync().await;
        let ids: Vec<String> = report.announcements.iter().map(|a| a.id.clone()).collect();
        ack_announcements(&self.library, &ids).unwrap();
        report
            .announcements
            .into_iter()
            .map(|a| (a.kind, a.name))
            .collect()
    }

    async fn approve(&mut self, qr: &str) -> Result<String, SyncError> {
        let (code, record) =
            approve_pairing(&self.library, &http(), &self.record, &self.clock, qr).await?;
        self.record = record;
        Ok(code)
    }

    async fn pair(&mut self, server: &str, name: &str) -> Device {
        let tv = start_pairing(&http(), server).await.unwrap();
        let code = self.approve(&tv.qr).await.unwrap();
        let offer = tv.poll(&http()).await.unwrap().expect("offer");
        assert_eq!(offer.code, code);
        Device::with(tv.claim(&http(), offer, name).await.unwrap())
    }

    async fn names(&mut self) -> Vec<String> {
        let mut names: Vec<String> = devices(&self.library, &http(), &self.record, &self.clock)
            .await
            .unwrap()
            .members
            .into_iter()
            .map(|m| m.name.unwrap_or_else(|| "kit".into()))
            .collect();
        names.sort();
        names
    }
}

/// Only a phone or a tablet approves (D6, phase 2 web review A P3-5): the core
/// refuses a device that said it is a TV, a computer or the web before
/// anything is sent, whatever its app shows; a phone, a tablet and an older
/// core that said nothing approve. The form is each device's own, sealed when
/// it made the account or joined it.
#[tokio::test]
async fn only_a_phone_or_tablet_approves() {
    let (server, _db) = relay().await;
    let made = |form: Option<&'static str>| {
        let server = server.clone();
        async move {
            let record =
                mq_sync::create_account_with(&http(), &server, None, "Device", None, false, form)
                    .await
                    .unwrap();
            Device::with(record)
        }
    };
    for form in ["tv", "computer", "web"] {
        let mut screen = made(Some(form)).await;
        let code = start_pairing(&http(), &server).await.unwrap();
        let refused = screen.approve(&code.qr).await.unwrap_err();
        assert_eq!(refused.code(), Some("cannotApprove"), "{form}: {refused}");
        assert_eq!(
            code.poll(&http()).await.unwrap(),
            None,
            "{form}: nothing was sent"
        );
    }
    for form in [Some("phone"), Some("tablet"), None] {
        made(form).await.pair(&server, "TV").await;
    }
    // A TV that joined by pairing is refused in turn.
    let mut phone = made(Some("phone")).await;
    let pairing = start_pairing(&http(), &server).await.unwrap();
    phone.approve(&pairing.qr).await.unwrap();
    let offer = pairing.poll(&http()).await.unwrap().expect("offer");
    let record = pairing
        .claim_as(&http(), offer, "TV", Some("tv"))
        .await
        .unwrap();
    let mut tv = Device::with(record);
    tv.sync().await;
    let another = start_pairing(&http(), &server).await.unwrap();
    assert_eq!(
        tv.approve(&another.qr).await.unwrap_err().code(),
        Some("cannotApprove")
    );
}

/// What each device hears (3.2, `SyncReport.announcements`): another
/// member's join, kit or removal, until acknowledged, whichever call caught
/// up; never what it did or vouched for itself, nor anything from before it
/// joined.
#[tokio::test]
async fn each_device_hears_once_what_the_others_changed() {
    let (server, _) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let mut tv = phone.pair(&server, "TV").await;
    assert_eq!(phone.heard().await, [], "it vouched for the TV");
    assert_eq!(tv.heard().await, [], "all before its own join");

    let mut laptop = tv.pair(&server, "Laptop").await;
    assert_eq!(
        laptop.heard().await,
        [],
        "the TV's join came before its own"
    );
    let listed = devices(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    let laptop_id = listed
        .members
        .iter()
        .find(|m| m.name.as_deref() == Some("Laptop"))
        .unwrap()
        .id;
    let joined = [(Announced::Device, Some("Laptop".to_owned()))];
    assert_eq!(phone.heard().await, joined, "kept since the list caught up");
    assert_eq!(phone.heard().await, [], "once");
    assert_eq!(tv.heard().await, [], "it vouched for the laptop");

    let (_, _, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    phone.record = remove_devices(
        &phone.library,
        &http(),
        &phone.record,
        &phone.clock,
        vec![laptop_id],
    )
    .await
    .unwrap();
    let changed = [
        (Announced::Kit, None),
        (Announced::Removed, Some("Laptop".to_owned())),
    ];
    assert_eq!(tv.heard().await, changed);
    assert_eq!(phone.heard().await, [], "its own kit and removal");
    drop(laptop);
}

/// An announcement stays until the app acknowledges it: a lost answer, a
/// web reload (the next round) and a restart of the store all bring it back
/// under the same id.
#[tokio::test]
async fn an_announcement_stays_until_acknowledged() {
    let (server, db) = relay().await;
    let path = db.with_file_name("phone.sqlite");
    let mut phone = Device::first(&server, "Phone").await;
    phone.library = Library::open(&path).unwrap();
    let mut tv = phone.pair(&server, "TV").await;
    phone.sync().await;
    let laptop = tv.pair(&server, "Laptop").await;

    let first = phone.sync().await.announcements;
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(
        phone.sync().await.announcements,
        first,
        "the answer was lost"
    );
    phone.library = Library::open(&path).unwrap();
    assert_eq!(phone.sync().await.announcements, first, "after a restart");

    ack_announcements(&phone.library, &[first[0].id.clone()]).unwrap();
    assert_eq!(phone.sync().await.announcements, []);
    // An id acknowledged twice, or never made, changes nothing.
    ack_announcements(&phone.library, &[first[0].id.clone(), "9.x".into()]).unwrap();
    drop(laptop);
}

/// A store rebuilt from the relay (the keystore record kept) reads every
/// entry again, but announces only what came after the record's checkpoint.
#[tokio::test]
async fn a_rebuilt_store_announces_only_what_is_new() {
    let (server, _) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let mut tv = phone.pair(&server, "TV").await;
    let _laptop = tv.pair(&server, "Laptop").await;
    let joined = [(Announced::Device, Some("Laptop".to_owned()))];
    assert_eq!(phone.heard().await, joined);

    phone.library = Library::temporary().unwrap();
    assert_eq!(phone.heard().await, [], "nothing it saw before");
    let _desk = tv.pair(&server, "Desk").await;
    let joined = [(Announced::Device, Some("Desk".to_owned()))];
    assert_eq!(phone.heard().await, joined);
}

/// A flood of more than 100 changes by another member (kits made and
/// dropped) never hides an earlier join: kit and removal notices go first,
/// and one `dropped` announcement counts what went (A P3-1).
#[tokio::test]
async fn a_flood_never_hides_a_join_silently() {
    let (server, _) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let mut tv = phone.pair(&server, "TV").await;
    let _laptop = tv.pair(&server, "Laptop").await;
    for _ in 0..51 {
        let (_, kit, record) = make_kit(&tv.library, &http(), &tv.record, &tv.clock)
            .await
            .unwrap();
        tv.record = record;
        tv.record = remove_devices(&tv.library, &http(), &tv.record, &tv.clock, vec![kit])
            .await
            .unwrap();
    }
    // The laptop's join, then 102 notices: 3 too many.
    let said = phone.sync().await.announcements;
    let kept = said.iter().filter(|a| a.kind != Announced::Dropped).count();
    assert_eq!(kept, 100);
    assert_eq!((said[0].kind, said[0].count), (Announced::Dropped, Some(3)));
    let laptop = said.iter().find(|a| a.kind == Announced::Device);
    assert_eq!(laptop.and_then(|a| a.name.as_deref()), Some("Laptop"));
    let kits = said.iter().filter(|a| a.kind == Announced::Kit).count();
    assert_eq!(kits, 49, "the three oldest kit notices went first");
}

#[tokio::test]
async fn a_tv_joins_by_qr_once_after_matching_codes() {
    let (server, db) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    assert!(tv.qr.starts_with("marquee:pair?v=2&"), "{}", tv.qr);
    assert_eq!(tv.poll(&http()).await.unwrap(), None, "waiting");

    let shown_on_phone = phone.approve(&tv.qr).await.unwrap();
    let offer = tv.poll(&http()).await.unwrap().expect("offer");
    assert_eq!(
        offer.code, shown_on_phone,
        "both screens show the same code"
    );
    assert_eq!(active(&db), 1, "no token before the user confirms");

    let record = tv.claim(&http(), offer, "Living room TV").await.unwrap();
    assert_eq!(record.keys, phone.record.keys);
    assert_eq!(record.server, server);
    assert_ne!(
        record.device_token, phone.record.device_token,
        "its own token"
    );
    assert_ne!(record.member, phone.record.member, "its own keys");
    assert_eq!(active(&db), 2);
    let mut tv_device = Device::with(record);
    tv_device.sync().await;
    assert_eq!(
        phone.names().await,
        vec!["Living room TV".to_owned(), "Phone".to_owned()]
    );

    // The slot is single use, and a code naming a device the account holds
    // is refused before anything is sent.
    assert!(tv.poll(&http()).await.is_err());
    let again = phone.approve(&tv.qr).await.unwrap_err();
    assert!(
        again.to_string().contains("already in the account"),
        "{again}"
    );
}

#[tokio::test]
async fn someone_else_who_saw_the_qr_cannot_slip_in() {
    let (server, db) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let mut stranger = Device::first(&server, "Stranger").await;
    let tv = start_pairing(&http(), &server).await.unwrap();

    // The stranger answers first; the owner's phone is told so, and shows
    // no code that could match the one on the TV.
    let strangers_code = stranger.approve(&tv.qr).await.unwrap();
    let err = phone.approve(&tv.qr).await.unwrap_err();
    assert!(err.to_string().contains("someone else"), "{err}");
    let offer = tv.poll(&http()).await.unwrap().expect("offer");
    assert_eq!(offer.code, strangers_code);
    // The user rejects: nothing is claimed, so no token exists for the TV.
    drop(offer);
    assert_eq!(active(&db), 2, "only the two accounts' own devices");

    // Knowing the slot id from the QR is not enough to collect the blob.
    let slot = tv
        .qr
        .split("&p=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    let err = http()
        .post_json(
            &format!("{server}/v1/pairings/{slot}/take"),
            &serde_json::json!({ "take": "00", "claim": true }),
            &local(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, HttpError::Status { status: 404, .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_blob_sealed_to_another_key_does_not_open() {
    let (server, _) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    let other = start_pairing(&http(), &server).await.unwrap();
    // The TV's slot, but the other device's keys.
    let keys = other.qr.split("&k=").nth(1).unwrap();
    let forged = tv.qr.split("&k=").next().unwrap().to_owned() + "&k=" + keys;
    phone.approve(&forged).await.unwrap();
    let err = tv.poll(&http()).await.unwrap_err();
    assert!(matches!(err, SyncError::Pairing(_)), "{err:?}");
    // Codes for another relay are refused before anything is sent.
    let elsewhere = tv.qr.replace("127.0.0.1", "127.0.0.2");
    assert!(phone.approve(&elsewhere).await.is_err());
}

#[tokio::test]
async fn a_recovery_kit_follows_key_rotations() {
    let (server, _) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let (kit, id, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    // The id answered is the kit's own entry in the device list, made by this phone.
    let devices = mq_sync::devices(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    let entry = devices
        .members
        .iter()
        .find(|m| m.id == id)
        .expect("the kit is listed");
    assert!(entry.kit);
    assert_eq!(entry.sponsor, Some(phone.record.secret().id()));

    let laptop = recover(&http(), &server, &kit.to_lowercase(), "Laptop")
        .await
        .unwrap();
    assert_eq!(laptop.keys, phone.record.keys);
    assert_ne!(laptop.device_token, phone.record.device_token);
    let mut laptop = Device::with(laptop);
    laptop.sync().await;

    // After a rotation (the laptop is removed), the kit opens the new key.
    phone.record = mq_sync::remove_devices(
        &phone.library,
        &http(),
        &phone.record,
        &phone.clock,
        vec![laptop.record.secret().id()],
    )
    .await
    .unwrap();
    assert_eq!(phone.record.keys.epoch, 1);
    let tablet = recover(&http(), &server, &kit, "Tablet").await.unwrap();
    assert_eq!(tablet.keys, phone.record.keys);

    // A wrong kit finds nothing.
    let wrong = mq_sync::pair::new_recovery_kit();
    assert!(recover(&http(), &server, &wrong, "x").await.is_err());
}

/// The usual recovery: the phone that made the kit is lost, the user
/// recovers on a laptop and reports the phone lost, keeping the TV the phone
/// paired. The phone and its kit go; the laptop and the TV keep syncing.
#[tokio::test]
async fn reporting_a_lost_phone_after_recovering_keeps_what_the_user_holds() {
    let (server, _) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let mut tv = phone.pair(&server, "TV").await;
    tv.sync().await;
    let (kit, _, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    let mut laptop = Device::with(recover(&http(), &server, &kit, "Laptop").await.unwrap());
    // The TV made a kit of its own: it stays if the TV is kept.
    let (_, tv_kit, record) = make_kit(&tv.library, &http(), &tv.record, &tv.clock)
        .await
        .unwrap();
    tv.record = record;
    laptop.sync().await;

    let phone_id = phone.record.secret().id();
    let tv_id = tv.record.secret().id();
    let preview = mq_sync::lost_preview(
        &laptop.library,
        &http(),
        &laptop.record,
        &laptop.clock,
        phone_id,
    )
    .await
    .unwrap();
    assert_eq!(preview.devices, vec![tv_id], "the TV is listed to untick");
    assert_eq!(
        preview.kits.len(),
        2,
        "the phone's kit and the TV's, with both ticked"
    );
    assert!(preview.kits.contains(&tv_kit));
    // Each device to untick with the kits that stay with it.
    assert_eq!(
        preview.device_kits,
        vec![mq_sync::DeviceKits {
            device: tv_id,
            kits: vec![tv_kit]
        }]
    );
    laptop.record = report_lost(
        &laptop.library,
        &http(),
        &laptop.record,
        &laptop.clock,
        phone_id,
        vec![tv_id],
    )
    .await
    .unwrap();
    // The TV is kept, and its own kit with it.
    assert_eq!(
        laptop.names().await,
        vec!["Laptop".to_owned(), "TV".to_owned(), "kit".to_owned()]
    );

    // The TV takes the new key and keeps syncing; the phone is out.
    tv.sync().await;
    assert_eq!(tv.record.keys, laptop.record.keys);
    let err = sync_once(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Dropped), "{err:?}");
    // The kit is dropped with it: it no longer recovers anything.
    assert!(recover(&http(), &server, &kit, "Thief").await.is_err());
}

/// The status of a POST from `client`, as a trusted proxy would forward it.
async fn status_from(url: &str, body: serde_json::Value, client: &str) -> u16 {
    status_with(url, body, &[("x-forwarded-for", client)]).await
}

/// The status of a POST carrying `headers`.
async fn status_with(url: &str, body: serde_json::Value, headers: &[(&str, &str)]) -> u16 {
    let options = RequestOptions {
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect(),
        ..local()
    };
    match http().post_json(url, &body, &options).await {
        Ok(_) => 200,
        Err(HttpError::Status { status, .. }) => status,
        Err(e) => panic!("{e:?}"),
    }
}

#[tokio::test]
async fn one_flooding_client_does_not_block_anyone_else() {
    let config = Config {
        trust_proxy: true,
        ..Config::default()
    };
    let (server, _) = relay_with(config).await;
    let open = format!("{server}/v1/pairings");
    let opens = |client: &'static str| status_from(&open, serde_json::json!({}), client);
    // Ten open slots per address (PAIRINGS_PER_CLIENT), under the minute's 20.
    for _ in 0..10 {
        assert_eq!(opens("198.51.100.7").await, 200);
    }
    assert_eq!(opens("198.51.100.7").await, 429, "over its own slots");
    // Hops before the proxy's are whatever the client wrote.
    assert_eq!(opens("203.0.113.1, 198.51.100.7").await, 429);
    assert_eq!(opens("203.0.113.9").await, 200, "another client");
    // An IPv6 client holds its whole /64.
    for _ in 0..10 {
        assert_eq!(opens("2001:db8::1").await, 200);
    }
    assert_eq!(opens("2001:db8::2").await, 429);
    assert_eq!(opens("2001:db8:0:1::1").await, 200);

    // Requests refused for one client do not use up the relay's budget
    // (600 recoveries a minute).
    let redeem = format!("{server}/v1/recovery/redeem");
    let body = serde_json::json!({ "lookup": "00", "auth": "00" });
    for _ in 0..700 {
        status_from(&redeem, body.clone(), "198.51.100.7").await;
    }
    assert_eq!(status_from(&redeem, body, "203.0.113.9").await, 404);
}

/// The approver sees other digits and ends the approval before the device
/// confirms: the key rotates at once and the device's join no longer fits.
#[tokio::test]
async fn an_approval_ended_before_the_device_confirms_lets_nothing_in() {
    let (server, db) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    phone.approve(&tv.qr).await.unwrap();
    let epoch = phone.record.keys.epoch;
    phone.record = cancel_pairing(&phone.library, &http(), &phone.record, &phone.clock, &tv.qr)
        .await
        .unwrap();
    assert_eq!(phone.record.keys.epoch, epoch + 1, "the key rotated");
    assert!(
        phone.library.sync_states("approval:").unwrap().is_empty(),
        "no approval waits any more"
    );
    let offer = tv.poll(&http()).await.unwrap().expect("offer");
    let err = tv.claim(&http(), offer, "TV").await.unwrap_err();
    assert!(err.to_string().contains("changed while pairing"), "{err}");
    assert_eq!(active(&db), 1, "no token for the TV");
    assert_eq!(phone.names().await, vec!["Phone".to_owned()]);
}

/// Ended after the device joined: it is dropped with the rotation.
#[tokio::test]
async fn an_approval_ended_after_the_join_drops_the_device() {
    let (server, _db) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    phone.approve(&tv.qr).await.unwrap();
    let offer = tv.poll(&http()).await.unwrap().expect("offer");
    let mut tv_device = Device::with(tv.claim(&http(), offer, "TV").await.unwrap());
    tv_device.sync().await;
    phone.record = cancel_pairing(&phone.library, &http(), &phone.record, &phone.clock, &tv.qr)
        .await
        .unwrap();
    assert_eq!(phone.names().await, vec!["Phone".to_owned()]);
    let err = sync_once(
        &tv_device.library,
        &http(),
        &tv_device.record,
        &tv_device.clock,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, SyncError::Dropped), "{err:?}");
}

/// Ended after the device joined and vouched for another: both go, since
/// the user never approved either.
#[tokio::test]
async fn an_approval_ended_late_drops_the_devices_it_let_in() {
    let (server, _db) = relay().await;
    let mut phone = Device::first(&server, "Phone").await;
    let tv = start_pairing(&http(), &server).await.unwrap();
    phone.approve(&tv.qr).await.unwrap();
    let offer = tv.poll(&http()).await.unwrap().expect("offer");
    let mut tv_device = Device::with(tv.claim(&http(), offer, "TV").await.unwrap());
    tv_device.sync().await;
    let laptop = start_pairing(&http(), &server).await.unwrap();
    tv_device.approve(&laptop.qr).await.unwrap();
    let offer = laptop.poll(&http()).await.unwrap().expect("offer");
    let mut laptop_device = Device::with(laptop.claim(&http(), offer, "Laptop").await.unwrap());
    laptop_device.sync().await;
    phone.sync().await;
    assert_eq!(phone.names().await.len(), 3, "the TV let the laptop in");
    phone.record = cancel_pairing(&phone.library, &http(), &phone.record, &phone.clock, &tv.qr)
        .await
        .unwrap();
    assert_eq!(phone.names().await, vec!["Phone".to_owned()]);
    for device in [&tv_device, &laptop_device] {
        let err = sync_once(&device.library, &http(), &device.record, &device.clock)
            .await
            .unwrap_err();
        assert!(matches!(err, SyncError::Dropped), "{err:?}");
    }
}

/// A handful of addresses can no longer hold every pairing slot open: each
/// holds at most ten, and an ended slot frees its place.
#[tokio::test]
async fn one_address_holds_ten_pairing_slots_at_most() {
    let offset = std::sync::Arc::<std::sync::atomic::AtomicI64>::default();
    let config = Config {
        trust_proxy: true,
        clock_offset_ms: offset.clone(),
        ..Config::default()
    };
    let relay = relay_with(config).await;
    let open = format!("{}/v1/pairings", relay.0);
    let opens = |client: &'static str| status_from(&open, serde_json::json!({}), client);
    for _ in 0..10 {
        assert_eq!(opens("198.51.100.7").await, 200);
    }
    assert_eq!(opens("198.51.100.7").await, 429, "its eleventh open slot");
    assert_eq!(
        opens("203.0.113.9").await,
        200,
        "another address still opens one"
    );
    // The count lives in memory only: no address in the database, so none in
    // a backup (the privacy page says the relay keeps none).
    let (_, db) = &relay;
    for file in [db.clone(), db.with_extension("sqlite-wal")] {
        let bytes = std::fs::read(&file).unwrap_or_default();
        for address in ["198.51.100.7", "203.0.113.9"] {
            assert!(
                !bytes
                    .windows(address.len())
                    .any(|w| w == address.as_bytes()),
                "{address} in {}",
                file.display()
            );
        }
    }
    // Ten minutes on, the slots have ended and the address may open again.
    offset.store(11 * 60 * 1000, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(opens("198.51.100.7").await, 200);
}

/// Behind the trusted proxy the client is Cloudflare's `CF-Connecting-IP`,
/// then the proxy's own `X-Forwarded-For` hop; with neither the request is
/// refused rather than counted against the proxy's address, which every
/// client would share.
#[tokio::test]
async fn behind_the_proxy_the_client_is_cloudflares_address() {
    let config = Config {
        trust_proxy: true,
        ..Config::default()
    };
    let (server, _) = relay_with(config).await;
    let open = format!("{server}/v1/pairings");
    let body = || serde_json::json!({});
    // CF-Connecting-IP names the client; an X-Forwarded-For that changes on
    // every request does not make it another one.
    for n in 0..10 {
        let hop = format!("192.0.2.{n}");
        let headers = [
            ("cf-connecting-ip", "198.51.100.7"),
            ("x-forwarded-for", hop.as_str()),
        ];
        assert_eq!(status_with(&open, body(), &headers).await, 200);
    }
    let headers = [
        ("cf-connecting-ip", "198.51.100.7"),
        ("x-forwarded-for", "192.0.2.99"),
    ];
    assert_eq!(status_with(&open, body(), &headers).await, 429);
    // Without it, the last X-Forwarded-For hop, as before.
    assert_eq!(
        status_with(&open, body(), &[("x-forwarded-for", "203.0.113.9")]).await,
        200
    );
    // Neither: refused, for every budgeted route.
    assert_eq!(status_with(&open, body(), &[]).await, 400);
    let redeem = format!("{server}/v1/recovery/redeem");
    let kit = serde_json::json!({ "lookup": "00", "auth": "00" });
    assert_eq!(status_with(&redeem, kit, &[]).await, 400);
    // A relay with no proxy in front counts the peer and needs no header.
    let (direct, _) = relay().await;
    assert_eq!(
        status_with(&format!("{direct}/v1/pairings"), body(), &[]).await,
        200
    );
}
