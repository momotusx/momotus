//! What listeners and caches see while a write commits: a change is
//! announced, and a removal counted, only once the rows are visible.

use mq_library::{CachedResponse, Hlc, Library, Table};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn hlc(wall: u64) -> String {
    Hlc {
        wall_ms: wall,
        counter: 0,
        device: 1,
    }
    .encode()
}

/// A profile with megabytes of rows, so removing it takes milliseconds to
/// commit: the window an early announcement would show in.
fn heavy(lib: &Library, id: &str) {
    lib.create_profile(id, id, "", &hlc(1)).unwrap();
    let big = format!("\"{}\"", "x".repeat(1 << 20));
    for i in 0..4 {
        lib.set_setting(id, &format!("k{i}"), &big, &hlc(2 + i))
            .unwrap();
    }
}

#[test]
fn a_listener_woken_by_a_change_reads_it() {
    for _ in 0..3 {
        let lib = Arc::new(Library::temporary().unwrap());
        heavy(&lib, "x");
        let mut changes = lib.subscribe();
        let listener = {
            let lib = lib.clone();
            std::thread::spawn(move || {
                let tables = changes.blocking_recv().unwrap();
                (tables.contains(&Table::Profiles), lib.is_gone("x").unwrap())
            })
        };
        lib.delete_profile("x", &hlc(10)).unwrap();
        assert_eq!(
            listener.join().unwrap(),
            (true, true),
            "announced before it was visible"
        );
    }
}

#[test]
fn a_new_removal_count_never_shows_with_the_old_rows() {
    let url = "https://addon.example/catalog";
    let row = CachedResponse {
        body: b"{}".to_vec(),
        stored_ms: 1,
        max_age_s: 600,
        stale_revalidate_s: 0,
        stale_error_s: 0,
    };
    for _ in 0..3 {
        let lib = Arc::new(Library::temporary().unwrap());
        heavy(&lib, "x");
        assert!(lib.cache_put(url, &row, lib.removals()).unwrap());
        let before = lib.removals();
        let done = Arc::new(AtomicBool::new(false));
        let watcher = {
            let (lib, done) = (lib.clone(), done.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::SeqCst) {
                    let counted = lib.removals() != before;
                    if counted && lib.cache_get(url).unwrap().is_some() {
                        return true;
                    }
                }
                false
            })
        };
        lib.delete_profile("x", &hlc(10)).unwrap();
        done.store(true, Ordering::SeqCst);
        assert!(
            !watcher.join().unwrap(),
            "a reader saw the new count beside the old cached answer"
        );
        assert_eq!(lib.cache_get(url).unwrap(), None);
    }
}
