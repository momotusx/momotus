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

/// Playing a watched episode again puts it back in Continue Watching until
/// it is marked again; it stops counting as finished meanwhile.
#[test]
fn rewatch_shows_in_continue_watching() {
    let lib = Library::temporary().unwrap();
    lib.upsert_item("p", &item("tt1", "Show One"), true, "0001")
        .unwrap();
    lib.set_progress("p", progress("tt1:1:1", "tt1", 950, 1000, 10), "0002")
        .unwrap();
    lib.set_watched("p", "tt1", "tt1:1:1", true, "0003")
        .unwrap();
    let cw = |lib: &Library| -> Vec<String> {
        lib.continue_watching("p", 10)
            .unwrap()
            .into_iter()
            .map(|r| r.video_id)
            .collect()
    };
    let done = |lib: &Library| -> Vec<String> {
        lib.finished_series("p", 10)
            .unwrap()
            .into_iter()
            .map(|r| r.video_id)
            .collect()
    };
    assert!(cw(&lib).is_empty());
    assert_eq!(done(&lib), ["tt1:1:1"]);

    lib.set_progress("p", progress("tt1:1:1", "tt1", 300, 1000, 20), "0004")
        .unwrap();
    assert_eq!(cw(&lib), ["tt1:1:1"]);
    assert!(done(&lib).is_empty());

    lib.set_progress("p", progress("tt1:1:1", "tt1", 960, 1000, 30), "0005")
        .unwrap();
    lib.set_watched("p", "tt1", "tt1:1:1", true, "0006")
        .unwrap();
    assert!(cw(&lib).is_empty());
    assert_eq!(done(&lib), ["tt1:1:1"]);
}

/// Journey reviews 2026-10-20 (web P2-10, Apple P2-3, Android P2-1): a
/// title taken off Continue Watching or Next Up stays off until it is
/// played again; an older progress register arriving late does not bring
/// it back.
#[test]
fn a_dismissed_title_stays_off_until_it_is_played_again() {
    let lib = Library::temporary().unwrap();
    lib.upsert_item("p", &item("tt1", "Show One"), true, "0001")
        .unwrap();
    lib.upsert_item("p", &item("tt2", "Show Two"), true, "0001")
        .unwrap();
    lib.set_progress("p", progress("tt1:1:1", "tt1", 300, 1000, 10), "0002")
        .unwrap();
    // Finished: a Next Up candidate.
    lib.set_progress("p", progress("tt2:1:1", "tt2", 990, 1000, 20), "0003")
        .unwrap();
    let rows = |lib: &Library| -> (Vec<String>, Vec<String>) {
        let ids =
            |rows: Vec<mq_library::ContinueItem>| rows.into_iter().map(|r| r.item_id).collect();
        (
            ids(lib.continue_watching("p", 10).unwrap()),
            ids(lib.finished_series("p", 10).unwrap()),
        )
    };
    assert_eq!(rows(&lib), (vec!["tt1".into()], vec!["tt2".into()]));
    lib.dismiss("p", "tt1", "0004").unwrap();
    lib.dismiss("p", "tt2", "0005").unwrap();
    assert_eq!(rows(&lib), (vec![], vec![]));
    lib.set_progress("p", progress("tt1:1:2", "tt1", 100, 1000, 50), "0003a")
        .unwrap();
    assert_eq!(rows(&lib), (vec![], vec![]), "an older register stays off");
    lib.set_progress("p", progress("tt1:1:2", "tt1", 200, 1000, 60), "0006")
        .unwrap();
    lib.set_progress("p", progress("tt2:1:2", "tt2", 100, 1000, 70), "0007")
        .unwrap();
    assert_eq!(rows(&lib), (vec!["tt2".into(), "tt1".into()], vec![]));
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

/// A table's version moves with each committed write that touches it, and
/// only then; two libraries never share one.
#[test]
fn a_table_version_moves_only_with_its_writes() {
    let lib = Library::temporary().unwrap();
    let other = Library::temporary().unwrap();
    assert_ne!(lib.version(Table::Addons), other.version(Table::Addons));
    let (addons, played) = (lib.version(Table::Addons), lib.version(Table::Progress));
    lib.set_progress("p", progress("v", "i", 1, 2, 3), "0001")
        .unwrap();
    assert_eq!(lib.version(Table::Addons), addons);
    assert_ne!(lib.version(Table::Progress), played);
    lib.set_addons("p", &[], "0002").unwrap(); // deletes nothing, inserts nothing
    assert_eq!(lib.version(Table::Addons), addons);
    let row = AddonRow {
        transport_url: "https://a.example/manifest.json".into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    lib.set_addons("p", &[row], "0003").unwrap();
    assert_ne!(lib.version(Table::Addons), addons);
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
    lib.cache_put("https://a/catalog.json", &value, lib.removals())
        .unwrap();
    assert_eq!(
        lib.cache_get("https://a/catalog.json").unwrap(),
        Some(value)
    );
    assert_eq!(lib.cache_get("missing").unwrap(), None);
}

#[test]
fn search_history_newest_first_and_capped() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile("p", "p", "0").unwrap();
    for i in 0..60 {
        lib.record_search("p", &format!("q{i}"), i).unwrap();
    }
    lib.record_search("p", "q3", 100).unwrap();
    let recent = lib.recent_searches("p", 100).unwrap();
    assert_eq!(recent.len(), 50);
    assert_eq!(&recent[..2], ["q3", "q59"]);

    // A pasted page is kept as its first 200 characters, cut on a
    // character, never inside one.
    let long = "é".repeat(150) + &"x".repeat(150);
    lib.record_search("p", &long, 200).unwrap();
    let kept = &lib.recent_searches("p", 1).unwrap()[0];
    assert_eq!(kept.chars().count(), 200);
    assert_eq!(*kept, "é".repeat(150) + &"x".repeat(50));
}

#[test]
fn clearing_searches_forgets_one_profiles_only() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile("p", "p", "0").unwrap();
    lib.ensure_profile("q", "q", "0").unwrap();
    lib.record_search("p", "dune", 1).unwrap();
    lib.record_search("q", "cars", 1).unwrap();
    lib.clear_searches("p").unwrap();
    assert!(lib.recent_searches("p", 10).unwrap().is_empty());
    assert_eq!(lib.recent_searches("q", 10).unwrap(), ["cars"]);
}

#[test]
fn library_lists_added_titles_with_their_newest_progress() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile("p", "p", "0").unwrap();
    lib.upsert_item("p", &item("tt1", "One"), true, "1")
        .unwrap();
    lib.upsert_item("p", &item("tt2", "Two"), false, "1")
        .unwrap();
    lib.set_progress("p", progress("tt1:1:1", "tt1", 10, 100, 5), "2")
        .unwrap();
    lib.set_progress("p", progress("tt1:1:2", "tt1", 20, 100, 9), "3")
        .unwrap();
    let entries = lib.library("p").unwrap();
    assert_eq!(entries.len(), 1, "titles not in the library are left out");
    assert_eq!(entries[0].item.name, "One");
    assert_eq!(entries[0].last_watched_ms, Some(9));
    assert_eq!((entries[0].position_ms, entries[0].duration_ms), (20, 100));
}

/// Run by `a_leaked_library_of_an_ended_process_is_swept` in a child
/// process: a library that is never dropped, as a test that forgets a view
/// holding its core leaves one.
#[test]
#[ignore = "a child process of the sweep test"]
fn leak_a_temporary_library() {
    std::mem::forget(Library::temporary().unwrap());
}

/// A leaked temporary library keeps its files after its process ends; the
/// next process's first temporary library removes them. Files of a live
/// process, and this process's own, stay.
#[test]
fn a_leaked_library_of_an_ended_process_is_swept() {
    // A directory of its own, so no other process's files are touched.
    let dir = std::env::temp_dir().join(format!("mq-sweep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let files = |pid: u32| -> Vec<std::path::PathBuf> {
        let prefix = format!("mq-library-{pid}-");
        std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .map(|e| e.path())
            .collect()
    };
    let me = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(&me)
        .args(["--exact", "leak_a_temporary_library", "--ignored"])
        .env("TMPDIR", &dir)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let ended = child.id();
    assert!(child.wait().unwrap().success());
    assert!(!files(ended).is_empty(), "the leaking case left its files");

    let mut live = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let mine = dir.join(format!("mq-library-{}-999999.sqlite", std::process::id()));
    let theirs = dir.join(format!("mq-library-{}-0.sqlite", live.id()));
    std::fs::write(&mine, b"").unwrap();
    std::fs::write(&theirs, b"").unwrap();

    mq_library::sweep_temporaries(&dir);
    assert!(files(ended).is_empty(), "swept: {:?}", files(ended));
    assert!(
        mine.exists() && theirs.exists(),
        "live processes keep theirs"
    );

    live.kill().unwrap();
    live.wait().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Code review 2026-10-20 r3 P3-7: a temporary library starts empty even
/// where an ended process with this pid left its files: no rows of that
/// run, no stale `-wal`.
#[test]
fn a_temporary_library_starts_empty_over_old_files() {
    let dir = std::env::temp_dir().join(format!("mq-reuse-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mq-library-1-0.sqlite");
    {
        let old = Library::open(&path).unwrap();
        old.ensure_profile("p", "p", "0").unwrap();
        old.record_search("p", "left behind", 10).unwrap();
        std::mem::forget(old);
    }
    let fresh = Library::temporary_at(path).unwrap();
    assert!(fresh.recent_searches("p", 10).unwrap().is_empty());
    drop(fresh);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Web journey review P3-20: an anime (Kitsu's kind) is a Next Up
/// candidate like a series; a movie still is not.
#[test]
fn a_finished_anime_episode_is_a_next_up_candidate() {
    let lib = Library::temporary().unwrap();
    for (id, kind) in [("kitsu:9", "anime"), ("tt1", "movie")] {
        let row = LibraryItemRow {
            kind: kind.into(),
            ..item(id, id)
        };
        lib.upsert_item("p", &row, true, "0001").unwrap();
    }
    lib.set_progress("p", progress("kitsu:9:1", "kitsu:9", 990, 1000, 10), "0002")
        .unwrap();
    lib.set_progress("p", progress("tt1", "tt1", 990, 1000, 20), "0003")
        .unwrap();
    let done: Vec<(String, String)> = lib
        .finished_series("p", 10)
        .unwrap()
        .into_iter()
        .map(|r| (r.item_id, r.kind))
        .collect();
    assert_eq!(done, [("kitsu:9".to_string(), "anime".to_string())]);
}

#[test]
fn imdb_ids_round_trip_and_go_with_a_removed_profile() {
    use mq_library::Hlc;
    let lib = Library::temporary().unwrap();
    let pair = |id: &str, imdb: &str| (id.to_owned(), imdb.to_owned());
    lib.put_imdb_ids(
        &[pair("kitsu:1", "tt1"), pair("kitsu:2", "tt2")],
        lib.removals(),
    )
    .unwrap();
    // A later answer replaces the kept id.
    lib.put_imdb_ids(&[pair("kitsu:2", "tt3")], lib.removals())
        .unwrap();
    let kept = lib.imdb_ids(&["kitsu:1", "kitsu:2", "kitsu:9"]).unwrap();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept["kitsu:1"], "tt1");
    assert_eq!(kept["kitsu:2"], "tt3");

    // A profile removal clears them, and a write it overtook stores nothing.
    let hlc = |wall_ms: u64| {
        Hlc {
            wall_ms,
            counter: 0,
            device: 1,
        }
        .encode()
    };
    lib.create_profile("x", "x", "", hlc(1).as_str()).unwrap();
    let before = lib.removals();
    lib.delete_profile("x", hlc(2).as_str()).unwrap();
    assert!(lib.imdb_ids(&["kitsu:1", "kitsu:2"]).unwrap().is_empty());
    lib.put_imdb_ids(&[pair("kitsu:1", "tt1")], before).unwrap();
    assert!(lib.imdb_ids(&["kitsu:1"]).unwrap().is_empty());
}

#[test]
fn imdb_ids_keep_the_last_written() {
    let lib = Library::temporary().unwrap();
    let n = mq_library::IMDB_ID_ROWS as usize;
    let pairs: Vec<(String, String)> = (0..=n)
        .map(|i| (format!("kitsu:{i}"), format!("tt{i}")))
        .collect();
    lib.put_imdb_ids(&pairs, lib.removals()).unwrap();
    // One past the cap: the first written goes.
    assert!(lib.imdb_ids(&["kitsu:0"]).unwrap().is_empty());
    assert_eq!(
        lib.imdb_ids(&["kitsu:1", &format!("kitsu:{n}")])
            .unwrap()
            .len(),
        2
    );
    // Written again, a row is new: the oldest other one goes in its place.
    lib.put_imdb_ids(&[("kitsu:1".into(), "tt1".into())], lib.removals())
        .unwrap();
    lib.put_imdb_ids(&[("kitsu:x".into(), "tt9".into())], lib.removals())
        .unwrap();
    assert_eq!(lib.imdb_ids(&["kitsu:1"]).unwrap().len(), 1);
    assert!(lib.imdb_ids(&["kitsu:2"]).unwrap().is_empty());
}
