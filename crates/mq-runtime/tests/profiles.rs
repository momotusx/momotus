//! Profiles across opens of one data directory: shared device state, and
//! which profile a core opens as.

use mq_library::OWNER;
use mq_runtime::{Core, CoreConfig};
use std::path::PathBuf;
use std::time::Duration;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(what: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("mq-runtime-{what}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

fn open(dir: &Dir, hint: &str) -> Core {
    Core::open(CoreConfig {
        data_dir: dir.0.clone(),
        profile_id: hint.into(),
        device_id: 0,
    })
    .unwrap()
}

#[tokio::test]
async fn two_opens_of_one_directory_share_the_device() {
    let dir = dir("shared");
    let a = open(&dir, OWNER);
    // The same directory through another spelling of its path.
    let b = Core::open(CoreConfig {
        data_dir: dir.0.join("."),
        profile_id: OWNER.into(),
        device_id: 0,
    })
    .unwrap();
    assert!(std::ptr::eq(a.library(), b.library()), "one library");
    assert!(std::ptr::eq(a.clock(), b.clock()), "one clock");

    // A deletion through one handle reaches the other's listeners.
    let kid = a.create_profile("Kid", "fox").unwrap();
    let mut changes = b.library().subscribe();
    a.delete_profile(&kid).unwrap();
    tokio::time::timeout(Duration::from_secs(5), changes.recv())
        .await
        .expect("b hears a's deletion")
        .unwrap();

    // One throttle: the fifth try waits whichever handle makes it.
    a.set_pin(OWNER, Some("1357"), None).await.unwrap();
    for _ in 0..4 {
        let wrong = a.check_pin(OWNER, Some("0000")).await.unwrap_err();
        assert_eq!(wrong.code(), Some("wrongPin"));
    }
    let waited = b.check_pin(OWNER, Some("1357")).await.unwrap_err();
    assert_eq!(waited.code(), Some("locked"), "{waited:?}");

    // Closed everywhere, the directory opens again cleanly.
    drop((a, b));
    let c = open(&dir, OWNER);
    assert_eq!(c.library().pin_failures(OWNER).unwrap(), 4);
}

#[tokio::test]
async fn a_stale_stored_profile_opens_the_owner_and_the_hint_counts_only_on_first_run() {
    let dir = dir("active");
    let core = open(&dir, OWNER);
    let kept = core.create_profile("Kept", "fox").unwrap();
    // First run (nothing stored): the app's hint, when it names a profile.
    assert_eq!(open(&dir, &kept).profile(), kept);
    assert_eq!(open(&dir, "nobody").profile(), OWNER);

    // A stored choice that is gone opens the owner, whatever the hint.
    let gone = core.create_profile("Gone", "owl").unwrap();
    core.for_profile(&gone).make_active().unwrap();
    core.delete_profile(&gone).unwrap();
    assert_eq!(open(&dir, &kept).profile(), OWNER);
    assert_eq!(open(&dir, &gone).profile(), OWNER);
    assert!(core.library().profile(&gone).unwrap().is_none());
}

#[tokio::test]
async fn a_pin_fails_closed_unless_known_to_be_absent() {
    let core = Core::temporary(OWNER).unwrap();
    assert!(!core.has_pin(), "the owner has none");
    assert!(
        core.for_profile("nobody").has_pin(),
        "unknown counts as locked"
    );
    let synced = mq_library::SyncRecord {
        coll: mq_library::coll::PROFILES,
        key: "new".into(),
        value: serde_json::json!({"profile": "new", "name": "new"}),
        hlc: core.hlc(),
    };
    core.library().apply(&[synced]).unwrap();
    assert!(core.for_profile("new").has_pin(), "no lock record yet");
}

#[tokio::test]
async fn a_pin_set_by_someone_allowed_starts_the_count_again() {
    let core = Core::temporary(OWNER).unwrap();
    let kid = core.create_profile("Kid", "fox").unwrap();
    let as_kid = core.for_profile(&kid);
    as_kid.set_pin(&kid, Some("1357"), None).await.unwrap();
    for _ in 0..4 {
        let wrong = core.check_pin(&kid, Some("0000")).await.unwrap_err();
        assert_eq!(wrong.code(), Some("wrongPin"));
    }
    let waits = core.check_pin(&kid, Some("1357")).await.unwrap_err();
    assert_eq!(waits.code(), Some("locked"));
    // The owner clears it and the kid sets another: the old tries are gone.
    core.set_pin(&kid, None, None).await.unwrap();
    as_kid.set_pin(&kid, Some("2468"), None).await.unwrap();
    assert_eq!(core.library().pin_failures(&kid).unwrap(), 0);
    let wrong = core.check_pin(&kid, Some("0000")).await.unwrap_err();
    assert_eq!(wrong.code(), Some("wrongPin"), "no wait carried over");
    core.check_pin(&kid, Some("2468")).await.unwrap();
}

#[tokio::test]
async fn avatars_are_short_app_ids() {
    let core = Core::temporary(OWNER).unwrap();
    for bad in [
        "x".repeat(65),
        "<svg onload=x>".into(),
        "fox amber".into(),
        "ü".into(),
    ] {
        let err = core.create_profile("Kid", &bad).unwrap_err();
        assert_eq!(err.code(), Some("badAvatar"), "{bad}");
    }
    let kid = core.create_profile("Kid", "fox:amber_2-b").unwrap();
    core.create_profile("Other", "").unwrap();
    let err = core
        .update_profile(&kid, None, Some("a/b"), None)
        .unwrap_err();
    assert_eq!(err.code(), Some("badAvatar"));
    core.update_profile(&kid, None, Some(&"o".repeat(64)), None)
        .unwrap();
}

/// A catalog addon on loopback whose answers may be cached for ten minutes.
async fn cached_addon() -> mq_library::AddonRow {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let body = r#"{"metas":[]}"#;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: max-age=600\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "cat", "version": "1.0.0", "name": "Cat", "types": ["movie"],
        "resources": ["catalog"], "catalogs": [{"type": "movie", "id": "top"}]
    });
    mq_library::AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

#[tokio::test]
async fn a_removal_forgets_cached_addon_answers() {
    let core = Core::temporary(OWNER).unwrap();
    let row = cached_addon().await;
    core.library()
        .set_addons(OWNER, &[row], &core.hlc())
        .unwrap();
    let addon = core.installed_addons().unwrap().remove(0);
    let path = mq_protocol::ResourcePath {
        resource: "catalog".into(),
        r#type: "movie".into(),
        id: "top".into(),
        extra: vec![],
    };
    let cached = || async {
        core.addons()
            .fetch(&addon, &path, Duration::from_secs(5))
            .await
            .unwrap()
            .from_cache
    };
    assert!(!cached().await);
    assert!(cached().await);
    // Cache keys are addon URLs, which can hold a profile's keys.
    let kid = core.create_profile("Kid", "fox").unwrap();
    core.delete_profile(&kid).unwrap();
    assert!(
        !cached().await,
        "memory and disk copies went with the profile"
    );
}
