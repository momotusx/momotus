//! A profile's addon list merges per addon (plans/accounts-social-sync.md
//! 13): each addon is a register of its own, removals are kept, and an edit
//! stamps only the addons it changed.

use mq_library::{AddonRow, Library, SyncCursor, SyncRecord, coll};
use serde_json::json;

fn hlc(wall: u64, device: u64) -> String {
    mq_library::Hlc {
        wall_ms: wall,
        counter: 0,
        device,
    }
    .encode()
}

fn row(url: &str) -> AddonRow {
    AddonRow {
        transport_url: url.into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    }
}

const X: &str = "https://x/manifest.json";
const Y: &str = "https://y/manifest.json";
const Z: &str = "https://z/manifest.json";
const W: &str = "https://w/manifest.json";

fn urls(lib: &Library) -> Vec<String> {
    lib.addons("p")
        .unwrap()
        .into_iter()
        .map(|a| a.transport_url)
        .collect()
}

fn all(lib: &Library) -> Vec<SyncRecord> {
    lib.changed_since(&SyncCursor::default(), 100_000).unwrap()
}

/// Both devices take everything the other has.
fn exchange(a: &Library, b: &Library) {
    let (from_a, from_b) = (all(a), all(b));
    a.apply(&from_b).unwrap();
    b.apply(&from_a).unwrap();
}

fn add(lib: &Library, url: &str, at: &str) {
    lib.edit_addons("p", at, |rows: &mut Vec<AddonRow>| {
        rows.push(row(url));
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
}

fn remove(lib: &Library, url: &str, at: &str) {
    lib.edit_addons("p", at, |rows: &mut Vec<AddonRow>| {
        rows.retain(|r| r.transport_url != url);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
}

/// The owner's ask: an addon added on one device is never lost to an addon
/// added on another before either synced.
#[test]
fn two_devices_adding_different_addons_offline_keep_both() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    exchange(&a, &b);
    add(&a, Y, &hlc(5, 1));
    add(&b, Z, &hlc(6, 2));
    exchange(&a, &b);
    assert_eq!(
        urls(&a),
        [X, Y, Z],
        "both kept, after the one both had, the two new ones by link"
    );
    assert_eq!(urls(&b), urls(&a));
    assert_eq!(all(&a), all(&b));
}

#[test]
fn a_newer_removal_wins_over_an_older_change_and_a_newer_add_installs_again() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    add(&a, Y, &hlc(2, 1));
    exchange(&a, &b);
    // A switches X off, B removes it later: removed on both.
    a.edit_addons("p", &hlc(4, 1), |rows: &mut Vec<AddonRow>| {
        rows[0].enabled = false;
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    remove(&b, X, &hlc(5, 2));
    exchange(&a, &b);
    assert_eq!(urls(&a), [Y]);
    assert_eq!(urls(&b), [Y]);
    // A adds it again after the removal: installed on both.
    add(&a, X, &hlc(9, 1));
    exchange(&a, &b);
    assert_eq!(urls(&a), [Y, X]);
    assert_eq!(urls(&b), [Y, X]);
}

#[test]
fn a_switch_on_one_device_and_an_add_on_another_both_stay() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    exchange(&a, &b);
    a.edit_addons("p", &hlc(5, 1), |rows: &mut Vec<AddonRow>| {
        rows[0].enabled = false;
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    add(&b, Y, &hlc(6, 2));
    exchange(&a, &b);
    for lib in [&a, &b] {
        let addons = lib.addons("p").unwrap();
        assert_eq!(urls(lib), [X, Y]);
        assert!(!addons[0].enabled, "the switch stays");
    }
}

/// A move stamps only the addon that moved, so another device's edits to
/// the others stay.
#[test]
fn a_move_stamps_only_the_moved_addon() {
    let lib = Library::temporary().unwrap();
    for (i, url) in [X, Y, Z, W].into_iter().enumerate() {
        add(&lib, url, &hlc(i as u64 + 1, 1));
    }
    let queued = lib.outbox(1000).unwrap();
    lib.outbox_done(queued.last_id.unwrap()).unwrap();
    lib.edit_addons("p", &hlc(9, 1), |rows: &mut Vec<AddonRow>| {
        let w = rows.pop().unwrap();
        rows.insert(0, w);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    assert_eq!(urls(&lib), [W, X, Y, Z]);
    let moved: Vec<(u8, String)> = lib
        .outbox(1000)
        .unwrap()
        .records
        .into_iter()
        .map(|(_, r)| (r.coll, r.key))
        .collect();
    assert_eq!(moved, [(coll::ADDON, format!("p\u{1f}{W}"))]);
    // An edit that changes nothing stamps nothing.
    lib.edit_addons("p", &hlc(10, 1), |_: &mut Vec<AddonRow>| {
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    assert_eq!(lib.outbox(1000).unwrap().records.len(), 1);
}

/// A whole list from an older build is a write to each addon at its hlc.
#[test]
fn an_older_builds_list_is_a_write_to_each_addon() {
    let lib = Library::temporary().unwrap();
    add(&lib, X, &hlc(5, 1));
    add(&lib, Y, &hlc(5, 1));
    let list = |addons: &[&str], at: String| SyncRecord {
        coll: coll::ADDONS,
        key: "p".into(),
        value: json!({
            "profile": "p",
            "addons": addons.iter().map(|u| json!({ "url": u, "manifest": "{}", "enabled": true, "allowPrivate": false })).collect::<Vec<_>>(),
        }),
        hlc: at,
    };
    // Older than both addons: X listed and Y absent change nothing.
    lib.apply(&[list(&[X], hlc(3, 2))]).unwrap();
    assert_eq!(urls(&lib), [X, Y]);
    // Newer: Y and Z listed, X absent, so X is removed.
    lib.apply(&[list(&[Y, Z], hlc(9, 2))]).unwrap();
    assert_eq!(urls(&lib), [Y, Z]);
    let x = all(&lib)
        .into_iter()
        .find(|r| r.coll == coll::ADDON && r.key == format!("p\u{1f}{X}"))
        .expect("the removal is kept and travels");
    assert_eq!(x.value["installed"], false);
    // Lists are no longer written.
    assert!(all(&lib).iter().all(|r| r.coll != coll::ADDONS));
}

#[test]
fn the_list_has_a_time_once_any_addon_did_and_keeps_it_after_the_last_goes() {
    let lib = Library::temporary().unwrap();
    assert_eq!(lib.addon_list_hlc("p").unwrap(), None);
    add(&lib, X, &hlc(1, 1));
    remove(&lib, X, &hlc(2, 1));
    assert!(urls(&lib).is_empty());
    assert_eq!(lib.addon_list_hlc("p").unwrap(), Some(hlc(2, 1)));
}
