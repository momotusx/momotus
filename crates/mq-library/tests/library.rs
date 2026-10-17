use mq_library::{AddonRow, Library, LibraryItemRow, ProgressRow, Table};

fn item(id: &str, name: &str) -> LibraryItemRow {
    LibraryItemRow {
        item_id: id.into(),
        kind: "series".into(),
        name: name.into(),
        poster: Some(format!("https://images.example/{id}.jpg")),
        poster_shape: None,
        added_ms: 1,
    }
}

fn progress<'a>(video: &'a str, item: &'a str, pos: i64, dur: i64, at: i64) -> ProgressRow<'a> {
    ProgressRow {
        video_id: video,
        item_id: item,
        position_ms: pos,
        duration_ms: dur,
        updated_ms: at,
    }
}

#[test]
fn addons_keep_order_and_replace_atomically() {
    let lib = Library::temporary().unwrap();
    let row = |url: &str| AddonRow {
        transport_url: url.into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    lib.set_addons(
        "p1",
        &[
            row("https://b/manifest.json"),
            row("https://a/manifest.json"),
        ],
        "0001",
    )
    .unwrap();
    let urls: Vec<_> = lib
        .addons("p1")
        .unwrap()
        .into_iter()
        .map(|a| a.transport_url)
        .collect();
    assert_eq!(urls, ["https://b/manifest.json", "https://a/manifest.json"]);
    lib.set_addons("p1", &[row("https://c/manifest.json")], "0002")
        .unwrap();
    assert_eq!(lib.addons("p1").unwrap().len(), 1);
    assert!(
        lib.addons("p2").unwrap().is_empty(),
        "profiles are isolated"
    );
}

#[test]
fn progress_is_last_writer_wins_by_hlc() {
    let lib = Library::temporary().unwrap();
    lib.set_progress("p", progress("tt1:1:1", "tt1", 1000, 9000, 10), "0005")
        .unwrap();
    // An older clock from an offline device does not overwrite newer progress.
    lib.set_progress("p", progress("tt1:1:1", "tt1", 50, 9000, 99), "0003")
        .unwrap();
    assert_eq!(lib.progress("p", "tt1:1:1").unwrap(), Some((1000, 9000)));
    lib.set_progress("p", progress("tt1:1:1", "tt1", 2000, 9000, 11), "0006")
        .unwrap();
    assert_eq!(lib.progress("p", "tt1:1:1").unwrap(), Some((2000, 9000)));
}

#[test]
fn continue_watching_newest_unfinished_per_title() {
    let lib = Library::temporary().unwrap();
    lib.upsert_item("p", &item("tt1", "Show One"), true, "0001")
        .unwrap();
    lib.upsert_item("p", &item("tt2", "Show Two"), true, "0001")
        .unwrap();
    lib.set_progress("p", progress("tt1:1:1", "tt1", 500, 1000, 10), "0002")
        .unwrap();
    lib.set_progress("p", progress("tt1:1:2", "tt1", 300, 1000, 20), "0003")
        .unwrap();
    lib.set_progress("p", progress("tt2:1:1", "tt2", 990, 1000, 30), "0004")
        .unwrap(); // finished
    lib.set_progress("p", progress("tt3", "tt3", 100, 1000, 40), "0005")
        .unwrap(); // not in library
    let rows = lib.continue_watching("p", 10).unwrap();
    let ids: Vec<_> = rows
        .iter()
        .map(|r| (r.item_id.as_str(), r.video_id.as_str()))
        .collect();
    assert_eq!(ids, [("tt3", "tt3"), ("tt1", "tt1:1:2")]);
    assert_eq!(rows[1].name, "Show One");

    lib.set_watched("p", "tt1", "tt1:1:2", true, "0006")
        .unwrap();
    let ids: Vec<_> = lib
        .continue_watching("p", 10)
        .unwrap()
        .into_iter()
        .map(|r| r.video_id)
        .collect();
    assert_eq!(ids, ["tt3"]);
    assert_eq!(lib.watched_videos("p", "tt1").unwrap(), ["tt1:1:2"]);
}

#[tokio::test]
async fn one_notification_per_transaction_with_touched_tables() {
    let lib = Library::temporary().unwrap();
    let mut rx = lib.subscribe();
    lib.set_progress("p", progress("v", "i", 1, 2, 3), "0001")
        .unwrap();
    assert_eq!(rx.recv().await.unwrap(), vec![Table::Progress]);
    lib.set_addons("p", &[], "0002").unwrap(); // deletes nothing, inserts nothing
    lib.upsert_item("p", &item("i", "Item"), true, "0003")
        .unwrap();
    assert_eq!(rx.recv().await.unwrap(), vec![Table::LibraryItems]);
}

#[test]
fn reopen_persists_data() {
    let dir = std::env::temp_dir().join(format!("mq-lib-reopen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("lib.sqlite");
    {
        let lib = Library::open(&path).unwrap();
        lib.set_progress("p", progress("v", "i", 42, 100, 1), "0001")
            .unwrap();
    }
    let lib = Library::open(&path).unwrap();
    assert_eq!(lib.progress("p", "v").unwrap(), Some((42, 100)));
    drop(lib);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn http_cache_round_trip_and_trim() {
    let lib = Library::temporary().unwrap();
    let value = mq_library::CachedResponse {
        body: b"{\"metas\":[]}".to_vec(),
        stored_ms: 1,
        max_age_s: 60,
        stale_revalidate_s: 3600,
        stale_error_s: 0,
    };
    lib.cache_put("https://a/catalog.json", &value).unwrap();
    assert_eq!(
        lib.cache_get("https://a/catalog.json").unwrap(),
        Some(value)
    );
    assert_eq!(lib.cache_get("missing").unwrap(), None);
}
