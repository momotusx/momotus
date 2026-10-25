//! Offline downloads' table (plans/offline-downloads.md 4.1, 6.1).

use mq_library::{DownloadRecord, Library};

fn row(id: &str, item: &str, video: &str) -> DownloadRecord {
    DownloadRecord {
        id: id.into(),
        added_by: "default".into(),
        kind: "series".into(),
        item_id: item.into(),
        video_id: video.into(),
        season: Some(1),
        episode: Some(1),
        facts: "{}".into(),
        request: "{}".into(),
        version: None,
        state: "queued".into(),
        reason: None,
        mobile_data: false,
        hand_picked: false,
        bytes_done: 0,
        bytes_total: None,
        moves: 0,
        subtitles: "[]".into(),
        subtitles_state: "pending".into(),
        position: 0,
        created_ms: 1,
        done_ms: None,
        smart: None,
    }
}

#[test]
fn adds_append_to_the_queue_and_one_video_has_one_row() {
    let lib = Library::temporary().unwrap();
    let a = lib.add_download(&row("a", "tt1", "tt1:1:1")).unwrap();
    let b = lib.add_download(&row("b", "tt1", "tt1:1:2")).unwrap();
    assert!(a.position < b.position);
    // The same video again gives the row it has.
    let again = lib.add_download(&row("c", "tt1", "tt1:1:1")).unwrap();
    assert_eq!(again.id, "a");
    let ids: Vec<String> = lib.downloads().unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(
        lib.download_of("tt1", "tt1:1:2").unwrap().map(|r| r.id),
        Some("b".into())
    );
}

#[test]
fn a_save_after_a_delete_writes_nothing() {
    let lib = Library::temporary().unwrap();
    let mut a = lib.add_download(&row("a", "tt1", "tt1")).unwrap();
    a.bytes_done = 8 << 20;
    assert!(lib.save_download(&a).unwrap());
    assert_eq!(lib.downloads().unwrap()[0].bytes_done, 8 << 20);
    assert!(lib.delete_download("a").unwrap());
    // A late checkpoint never brings a cancelled download back.
    assert!(!lib.save_download(&a).unwrap());
    assert!(lib.downloads().unwrap().is_empty());
}

#[test]
fn downloads_never_sync_nor_wake_views_and_outlive_a_profile() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile("kid", "Kid", &"0".repeat(32)).unwrap();
    let queued = lib.outbox(100).unwrap().records.len();
    let mut changes = lib.subscribe();
    let mut a = row("a", "tt1", "tt1");
    a.added_by = "kid".into();
    lib.add_download(&a).unwrap();
    // No sync collection: nothing queued for the relay.
    assert_eq!(lib.outbox(100).unwrap().records.len(), queued);
    // No table any view listens to.
    assert!(changes.try_recv().is_err());
    lib.delete_profile("kid", &"1".repeat(32)[..]).unwrap();
    assert_eq!(lib.downloads().unwrap().len(), 1);
}
