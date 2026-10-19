//! Pairing a new device and recovering an account through a real relay.

use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::Library;
use mq_library::hlc::HlcClock;
use mq_sync::pair::{approve_pairing, cancel_pairing, make_kit, recover, start_pairing};
use mq_sync::{SyncConfig, SyncError, devices, report_lost, sync_once};
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

    /// Syncs until the record settles, storing it like an app does.
    async fn sync(&mut self) -> mq_sync::SyncReport {
        let mut report = None;
        for _ in 0..3 {
            let r = sync_once(&self.library, &http(), &self.record, &self.clock)
                .await
                .unwrap();
            let again = r.again;
            if let Some(record) = r.record.clone() {
                self.record = record;
            }
            report = Some(r);
            if !again {
                break;
            }
        }
        report.unwrap()
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

    // The slot is single use.
    assert!(tv.poll(&http()).await.is_err());
    let again = phone.approve(&tv.qr).await.unwrap_err();
    assert!(again.to_string().contains("expired"), "{again}");
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
    let (kit, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;

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
    let (kit, record) = make_kit(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap();
    phone.record = record;
    let mut laptop = Device::with(recover(&http(), &server, &kit, "Laptop").await.unwrap());
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
    assert_eq!(preview.kits.len(), 1, "the kit shown on the phone goes");
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
    assert_eq!(
        laptop.names().await,
        vec!["Laptop".to_owned(), "TV".to_owned()]
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
    let options = RequestOptions {
        headers: vec![("x-forwarded-for".into(), client.into())],
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
    for _ in 0..20 {
        assert_eq!(opens("198.51.100.7").await, 200);
    }
    assert_eq!(opens("198.51.100.7").await, 429, "over its own budget");
    // Hops before the proxy's are whatever the client wrote.
    assert_eq!(opens("203.0.113.1, 198.51.100.7").await, 429);
    assert_eq!(opens("203.0.113.9").await, 200, "another client");
    // An IPv6 client holds its whole /64.
    for _ in 0..20 {
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
