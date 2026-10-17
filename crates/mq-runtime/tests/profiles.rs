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
