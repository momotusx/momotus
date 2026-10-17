//! Profiles through the real engine and relay: a deletion beats an offline
//! device's edits, a cosmetic edit never restores an old PIN, and a relay
//! restore or a new device carries the profile registers.

use mq_http::{ClientConfig, HttpClient};
use mq_library::hlc::{HlcClock, now_ms};
use mq_library::{AddonRow, Library, OWNER, ProgressRow, SyncCursor, SyncRecord, coll};
use mq_sync::{SyncConfig, sync_once};
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
    let path = std::env::temp_dir().join(format!("mq-profiles-{}", hex::encode(b)));
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

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

/// One device: its library and clock.
struct Device {
    lib: Library,
    clock: HlcClock,
}

impl Device {
    fn new(id: u64) -> Self {
        let lib = Library::temporary().unwrap();
        let clock = HlcClock::new(id);
        lib.ensure_profile(OWNER, OWNER, &mq_library::Hlc::zero(id).encode())
            .unwrap();
        Self { lib, clock }
    }

    fn hlc(&self) -> String {
        self.clock.tick(now_ms()).encode()
    }

    async fn sync(&self, config: &SyncConfig) {
        sync_once(&self.lib, &http(), config, &self.clock)
            .await
            .unwrap();
    }

    /// Registers that name `profile`, other than its gone register.
    fn registers_of(&self, profile: &str) -> Vec<SyncRecord> {
        let prefix = format!("{profile}\u{1f}");
        self.lib
            .changed_since(&SyncCursor::default(), 100_000)
            .unwrap()
            .into_iter()
            .filter(|r| r.coll != coll::PROFILE_GONE)
            .filter(|r| r.key == profile || r.key.starts_with(&prefix))
            .collect()
    }
}

fn progress(video: &str) -> ProgressRow<'_> {
    ProgressRow {
        video_id: video,
        item_id: "tt1",
        position_ms: 5,
        duration_ms: 10,
        updated_ms: 1,
    }
}

async fn relay() -> (Dir, String, SyncConfig) {
    let dir = dir();
    let server = serve(&dir.0.join("relay.sqlite"), Config::default()).await;
    let config = mq_sync::create_account(&http(), &server, None, "test")
        .await
        .unwrap();
    (dir, server, config)
}

#[tokio::test]
async fn a_profile_deleted_while_a_device_is_offline_stays_deleted() {
    let (_dir, _server, config) = relay().await;
    let a = Device::new(1);
    let b = Device::new(2);
    a.lib.create_profile("x", "Kid", "fox", &a.hlc()).unwrap();
    a.lib.set_progress("x", progress("v1"), &a.hlc()).unwrap();
    a.sync(&config).await;
    b.sync(&config).await;
    assert!(b.lib.profile("x").unwrap().is_some_and(|p| p.ready()));

    // A deletes x; B, offline, keeps using it: progress, a rename, its
    // addons and a setting, all stamped after the deletion.
    a.lib.delete_profile("x", &a.hlc()).unwrap();
    a.sync(&config).await;
    b.lib.set_progress("x", progress("v2"), &b.hlc()).unwrap();
    b.lib
        .update_profile("x", "Renamed", "owl", 3, &b.hlc())
        .unwrap();
    let addon = AddonRow {
        transport_url: "https://a/manifest.json".into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    b.lib.set_addons("x", &[addon], &b.hlc()).unwrap();
    b.lib.set_setting("x", "k", "1", &b.hlc()).unwrap();
    b.lib.set_pin_hash("x", None, &b.hlc()).unwrap();

    // B pushes its edits, then pulls the deletion; A pulls B's edits.
    b.sync(&config).await;
    a.sync(&config).await;
    b.sync(&config).await;
    for (name, device) in [("a", &a), ("b", &b)] {
        assert!(device.lib.is_gone("x").unwrap(), "{name}");
        assert!(device.lib.profile("x").unwrap().is_none(), "{name}");
        assert!(
            device.registers_of("x").is_empty(),
            "{name}: {:?}",
            device.registers_of("x")
        );
        assert!(device.lib.progress("x", "v2").unwrap().is_none(), "{name}");
    }

    // A new device reading everything the relay ever kept gets no x either.
    let c = Device::new(3);
    c.sync(&config).await;
    assert!(c.lib.is_gone("x").unwrap());
    assert!(c.registers_of("x").is_empty(), "{:?}", c.registers_of("x"));

    // The owner cannot be deleted locally, and B's edits to it still sync.
    assert!(b.lib.delete_profile(OWNER, &b.hlc()).is_err());
    b.lib
        .update_profile(OWNER, "Owner", "cat", 0, &b.hlc())
        .unwrap();
    b.sync(&config).await;
    a.sync(&config).await;
    assert_eq!(a.lib.profile(OWNER).unwrap().unwrap().name, "Owner");
}

#[tokio::test]
async fn an_offline_avatar_edit_never_brings_back_an_old_pin() {
    let (_dir, _server, config) = relay().await;
    let a = Device::new(1);
    let b = Device::new(2);
    a.lib.create_profile("x", "Kid", "fox", &a.hlc()).unwrap();
    a.lib.set_pin_hash("x", Some("old"), &a.hlc()).unwrap();
    a.sync(&config).await;
    b.sync(&config).await;

    a.lib.set_pin_hash("x", Some("new"), &a.hlc()).unwrap();
    a.sync(&config).await;
    // B never saw the new PIN when it edited the avatar, later.
    b.lib
        .update_profile("x", "Kid", "owl", 0, &b.hlc())
        .unwrap();
    b.sync(&config).await;
    a.sync(&config).await;
    for device in [&a, &b] {
        let x = device.lib.profile("x").unwrap().unwrap();
        assert_eq!((x.avatar.as_str(), x.pin_hash()), ("owl", Some("new")));
    }
}

#[tokio::test]
async fn a_restored_relay_and_a_new_device_get_the_profile_registers() {
    let dir = dir();
    let db = dir.0.join("relay.sqlite");
    let server = serve(&db, Config::default()).await;
    let config = mq_sync::create_account(&http(), &server, None, "test")
        .await
        .unwrap();
    let a = Device::new(1);
    a.lib.create_profile("x", "Kid", "fox", &a.hlc()).unwrap();
    a.lib.set_pin_hash("x", Some("pin"), &a.hlc()).unwrap();
    a.lib.create_profile("y", "Gone", "owl", &a.hlc()).unwrap();
    a.lib.delete_profile("y", &a.hlc()).unwrap();
    a.sync(&config).await;

    // The relay comes back from a backup holding no records at all.
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute("DELETE FROM record", [])
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
    a.sync(&config).await;

    let c = Device::new(3);
    c.sync(&config).await;
    let x = c.lib.profile("x").unwrap().unwrap();
    assert_eq!((x.name.as_str(), x.pin_hash()), ("Kid", Some("pin")));
    assert!(c.lib.is_gone("y").unwrap());
    assert!(c.lib.profile("y").unwrap().is_none());
}
