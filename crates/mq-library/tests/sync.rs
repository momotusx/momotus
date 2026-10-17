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
    assert_eq!(seen.len(), 26, "25 items and the profile");
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
    assert_eq!(page.records.len(), 2);
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
fn a_local_addon_edit_older_than_a_pulled_list_does_not_replace_it() {
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
    let l = hlc(5, 2);
    a.set_addons("p", &[row("https://a/manifest.json")], &hlc(9, 1))
        .unwrap();
    b.apply(&all(&a)).unwrap();
    b.set_addons("p", &[row("https://b/manifest.json")], &l)
        .unwrap();
    let urls: Vec<_> = b
        .addons("p")
        .unwrap()
        .into_iter()
        .map(|r| r.transport_url)
        .collect();
    assert_eq!(urls, ["https://a/manifest.json"]);
    assert_eq!(all(&b), all(&a));
}
