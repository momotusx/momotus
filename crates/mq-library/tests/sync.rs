use mq_library::{AddonRow, Library, LibraryItemRow, ProgressRow, SyncCursor, SyncRecord, coll};

fn hlc(wall: u64, device: u64) -> String {
    mq_library::Hlc {
        wall_ms: wall,
        counter: 0,
        device,
    }
    .encode()
}

fn all(lib: &Library) -> Vec<SyncRecord> {
    lib.changed_since(&SyncCursor::default(), 100_000).unwrap()
}

fn item(id: &str) -> LibraryItemRow {
    LibraryItemRow {
        item_id: id.into(),
        kind: "movie".into(),
        name: id.into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    }
}

#[test]
fn pages_never_skip_rows_that_share_an_hlc() {
    let lib = Library::temporary().unwrap();
    let same = hlc(5, 9);
    for i in 0..25 {
        lib.upsert_item("p", &item(&format!("tt{i:02}")), true, &same)
            .unwrap();
    }
    lib.ensure_profile("p", "p", &hlc(1, 9)).unwrap();
    let mut cursor = SyncCursor::default();
    let mut seen = Vec::new();
    loop {
        let page = lib.changed_since(&cursor, 7).unwrap();
        let Some(last) = page.last() else { break };
        cursor = last.cursor();
        seen.extend(page);
    }
    assert_eq!(seen.len(), 27, "25 items, the profile and its lock");
    assert_eq!(seen, all(&lib), "paged export equals one big export");
}

#[test]
fn apply_is_last_writer_wins_and_idempotent() {
    let a = Library::temporary().unwrap();
    let b = Library::temporary().unwrap();
    let p = |pos| ProgressRow {
        video_id: "v",
        item_id: "tt1",
        position_ms: pos,
        duration_ms: 100,
        updated_ms: pos,
    };
    a.set_progress("p", p(10), &hlc(10, 1)).unwrap();
    b.set_progress("p", p(20), &hlc(20, 2)).unwrap();
    // Older remote state does not win; newer does; applying twice changes nothing.
    assert_eq!(b.apply(&all(&a)).unwrap(), 0);
    assert_eq!(a.apply(&all(&b)).unwrap(), 1);
    assert_eq!(a.apply(&all(&b)).unwrap(), 0);
    assert_eq!(all(&a), all(&b));
    assert_eq!(a.progress("p", "v").unwrap(), Some((20, 100)));
}

#[test]
fn the_addon_list_is_one_register_even_when_empty() {
    let a = Library::temporary().unwrap();
    let b = Library::temporary().unwrap();
    let row = |url: &str| AddonRow {
        transport_url: url.into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    a.set_addons(
        "p",
        &[
            row("https://a/manifest.json"),
            row("https://b/manifest.json"),
        ],
        &hlc(1, 1),
    )
    .unwrap();
    b.apply(&all(&a)).unwrap();
    assert_eq!(b.addons("p").unwrap().len(), 2);

    // B removes every addon later; A's older list must not bring them back.
    b.set_addons("p", &[], &hlc(2, 2)).unwrap();
    assert_eq!(b.apply(&all(&a)).unwrap(), 0);
    a.apply(&all(&b)).unwrap();
    assert!(a.addons("p").unwrap().is_empty());
    let lists: Vec<_> = all(&a)
        .into_iter()
        .filter(|r| r.coll == coll::ADDONS)
        .collect();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].hlc, hlc(2, 2));
}

#[test]
fn unknown_or_damaged_records_are_skipped() {
    let lib = Library::temporary().unwrap();
    let records = vec![
        SyncRecord {
            coll: 99,
            key: "x".into(),
            value: serde_json::json!({}),
            hlc: hlc(1, 1),
        },
        SyncRecord {
            coll: coll::PROGRESS,
            key: "x".into(),
            value: serde_json::json!({"nope": 1}),
            hlc: hlc(1, 1),
        },
    ];
    assert_eq!(lib.apply(&records).unwrap(), 0);
}

#[test]
fn a_write_landing_during_a_push_stays_queued() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile("p", "p", &hlc(1, 1)).unwrap();
    lib.upsert_item("p", &item("tt1"), true, &hlc(2, 1))
        .unwrap();
    let page = lib.outbox(500).unwrap();
    assert_eq!(page.records.len(), 3, "the profile, its lock and the item");
    // Written after the page was read, with an older clock than the item.
    lib.set_progress(
        "p",
        ProgressRow {
            video_id: "v",
            item_id: "tt1",
            position_ms: 1,
            duration_ms: 2,
            updated_ms: 1,
        },
        &hlc(1, 2),
    )
    .unwrap();
    lib.outbox_done(page.last_id.unwrap()).unwrap();
    let next = lib.outbox(500).unwrap();
    assert_eq!(next.records.len(), 1);
    assert_eq!(next.records[0].1.coll, coll::PROGRESS);
    // Pulled registers are never queued: no echo.
    let other = Library::temporary().unwrap();
    other.apply(&all(&lib)).unwrap();
    assert_eq!(other.outbox(500).unwrap().last_id, None);
}

#[test]
fn a_local_addon_edit_lands_above_a_newer_pulled_list_and_is_queued() {
    let a = Library::temporary().unwrap();
    let b = Library::temporary().unwrap();
    let row = |url: &str| AddonRow {
        transport_url: url.into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    // The edit on B took its hlc (L), then a pull applied A's newer list (R).
    // The user made the edit after seeing a list, so it goes in just above R
    // and is queued, and A takes it on its next pull: no divergence.
    let l = hlc(5, 2);
    a.set_addons("p", &[row("https://a/manifest.json")], &hlc(9, 1))
        .unwrap();
    b.apply(&all(&a)).unwrap();
    b.set_addons("p", &[row("https://b/manifest.json")], &l)
        .unwrap();
    let urls = |lib: &Library| -> Vec<String> {
        lib.addons("p")
            .unwrap()
            .into_iter()
            .map(|r| r.transport_url)
            .collect()
    };
    assert_eq!(urls(&b), ["https://b/manifest.json"]);
    assert_eq!(b.outbox(10).unwrap().records.len(), 1);
    a.apply(&all(&b)).unwrap();
    assert_eq!(urls(&a), ["https://b/manifest.json"]);
    assert_eq!(all(&b), all(&a));
}

#[test]
fn cache_and_import_writes_never_override_a_real_edit() {
    let lib = Library::temporary().unwrap();
    lib.upsert_item("p", &item("tt1"), true, &hlc(50, 7))
        .unwrap();
    // A played title cached at the device's zero hlc, and a Stremio import
    // (device 0) with an older time, both lose to the edit.
    let zero = mq_library::Hlc::zero(9).encode();
    lib.upsert_item("p", &item("tt1"), false, &zero).unwrap();
    lib.upsert_item("p", &item("tt1"), false, &hlc(10, 0))
        .unwrap();
    assert!(lib.is_in_library("p", "tt1").unwrap());
    // A real local edit from a slow clock still goes in above it.
    lib.upsert_item("p", &item("tt1"), false, &hlc(10, 9))
        .unwrap();
    assert!(!lib.is_in_library("p", "tt1").unwrap());
}

/// The roster store takes the same entry twice but never a second entry at
/// a taken seq: a conflicting history must not slip by unnoticed.
#[test]
fn a_roster_entry_at_a_taken_seq_is_refused_unless_it_is_the_same() {
    let lib = Library::temporary().unwrap();
    lib.add_roster_entry("g", 0, b"one", b"sig").unwrap();
    lib.add_roster_entry("g", 0, b"one", b"sig").unwrap();
    assert!(lib.add_roster_entry("g", 0, b"two", b"sig").is_err());
    assert_eq!(
        lib.roster_entries("g").unwrap(),
        vec![(b"one".to_vec(), b"sig".to_vec())]
    );
}

/// A dropped device forgets its queue, cursors and roster, but keeps its own
/// id and the hosts relay ids were pinned to.
#[test]
fn forgetting_sync_keeps_the_device_id_and_relay_pins() {
    let lib = Library::temporary().unwrap();
    for key in ["device_id", "relay:07", "pull_seq", "approval:01"] {
        lib.set_sync_state(key, "x").unwrap();
    }
    lib.add_roster_entry("g", 0, b"one", b"sig").unwrap();
    lib.forget_sync().unwrap();
    let kept: Vec<String> = ["device_id", "relay:07", "pull_seq", "approval:01"]
        .into_iter()
        .filter(|key| lib.sync_state(key).unwrap().is_some())
        .map(str::to_owned)
        .collect();
    assert_eq!(kept, ["device_id", "relay:07"]);
    assert!(lib.roster_entries("g").unwrap().is_empty());
}

#[test]
fn moving_a_sync_state_replaces_it_in_one_step() {
    let lib = Library::temporary().unwrap();
    lib.set_sync_state("approval:01", "5:6").unwrap();
    lib.move_sync_state("approval:01", "cancel:01", "1")
        .unwrap();
    assert_eq!(lib.sync_state("approval:01").unwrap(), None);
    assert_eq!(lib.sync_state("cancel:01").unwrap().as_deref(), Some("1"));
}
