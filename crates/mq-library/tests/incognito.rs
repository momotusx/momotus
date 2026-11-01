//! Incognito in the library (plans/batch-10-incognito.md 2.6 and 7.1): a
//! scope's outbox drops every insert (wall 2), a pulled record naming
//! `incognito` is dropped in every collection, and a closed library refuses
//! its writes.

use mq_library::{
    AddonRow, INCOGNITO, Library, LibraryError, LibraryItemRow, ProgressRow, SyncCursor, SyncRecord,
};

fn hlc(wall: u64) -> String {
    mq_library::Hlc {
        wall_ms: wall,
        counter: 0,
        device: 7,
    }
    .encode()
}

fn all(lib: &Library) -> Vec<SyncRecord> {
    lib.changed_since(&SyncCursor::default(), 100_000).unwrap()
}

/// One write of every kind a profile makes, all for `profile`.
fn write_everything(lib: &Library, profile: &str) {
    lib.ensure_profile(profile, profile, &hlc(1)).unwrap();
    let item = LibraryItemRow {
        item_id: "tt1".into(),
        kind: "movie".into(),
        name: "Title".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    lib.upsert_item(profile, &item, true, &hlc(2)).unwrap();
    let progress = ProgressRow {
        video_id: "tt1",
        item_id: "tt1",
        position_ms: 10,
        duration_ms: 100,
        updated_ms: 10,
    };
    lib.set_progress(profile, progress, &hlc(3)).unwrap();
    lib.set_watched(profile, "tt2", "tt2", true, &hlc(4))
        .unwrap();
    lib.set_setting(profile, "subtitles", "[\"en\"]", &hlc(5))
        .unwrap();
    lib.dismiss(profile, "tt3", &hlc(6)).unwrap();
    lib.set_stream_memory(profile, "tt1", None, "{}", &hlc(7))
        .unwrap();
    let addon = AddonRow {
        transport_url: "https://addon.example/manifest.json".into(),
        manifest_json: "{\"id\":\"x\"}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    lib.set_addons(profile, &[addon], &hlc(8)).unwrap();
    lib.set_pin_hash(profile, None, &hlc(9)).unwrap();
}

/// Wall 2: a scope library queues nothing, though every write lands; a
/// device library queues the same writes.
#[test]
fn a_scope_library_drops_every_outbox_insert_and_only_there() {
    let scope = Library::temporary().unwrap();
    scope.drop_outbox().unwrap();
    write_everything(&scope, INCOGNITO);
    // Positive control: the scope held each write.
    assert_eq!(scope.progress(INCOGNITO, "tt1").unwrap(), Some((10, 100)));
    assert_eq!(scope.addons(INCOGNITO).unwrap().len(), 1);
    assert!(scope.is_in_library(INCOGNITO, "tt1").unwrap());
    assert!(scope.outbox(1000).unwrap().records.is_empty());
    scope.queue_all().unwrap();
    assert!(scope.outbox(1000).unwrap().records.is_empty());

    let device = Library::temporary().unwrap();
    write_everything(&device, "p");
    assert!(!device.outbox(1000).unwrap().records.is_empty());
}

/// A pulled record for `incognito` is dropped in every collection (a forged
/// id), while the same records for a real profile land.
#[test]
fn a_pulled_incognito_record_is_dropped_in_every_collection() {
    let source = Library::temporary().unwrap();
    write_everything(&source, INCOGNITO);
    let forged = all(&source);
    let colls: std::collections::BTreeSet<u8> = forged.iter().map(|r| r.coll).collect();
    assert!(colls.len() >= 9, "{colls:?}");

    let target = Library::temporary().unwrap();
    assert_eq!(target.apply(&forged).unwrap(), 0);
    assert!(all(&target).is_empty(), "{:?}", all(&target));
    assert_eq!(target.progress(INCOGNITO, "tt1").unwrap(), None);

    // The control: a real profile's records of the same kinds land.
    let real = Library::temporary().unwrap();
    write_everything(&real, "p");
    assert!(target.apply(&all(&real)).unwrap() >= 9);
    assert_eq!(target.progress("p", "tt1").unwrap(), Some((10, 100)));
}

/// An ended scope's library refuses every write; what it held still reads.
#[test]
fn a_closed_library_refuses_every_write() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile("p", "p", &hlc(1)).unwrap();
    lib.record_search("p", "before", 1).unwrap();
    lib.close();
    assert!(matches!(
        lib.record_search("p", "after", 2),
        Err(LibraryError::Closed)
    ));
    assert!(matches!(
        lib.set_setting("p", "k", "1", &hlc(2)),
        Err(LibraryError::Closed)
    ));
    assert_eq!(
        lib.recent_searches("p", 10).unwrap(),
        vec!["before".to_owned()]
    );
}
