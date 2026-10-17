//! Profiles as registers: the lock and gone registers, remove-wins deletion
//! and the purge, in every arrival order.

use mq_library::{
    AddonRow, Hlc, ImportedPlay, IntentOp, Library, LibraryError, LibraryItemRow, NewIntent, OWNER,
    ProgressRow, SyncCursor, SyncRecord, TrackerKey, coll,
};

fn hlc(wall: u64, device: u64) -> String {
    Hlc {
        wall_ms: wall,
        counter: 0,
        device,
    }
    .encode()
}

fn all(lib: &Library) -> Vec<SyncRecord> {
    lib.changed_since(&SyncCursor::default(), 100_000).unwrap()
}

fn addon(url: &str) -> AddonRow {
    AddonRow {
        transport_url: url.into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    }
}

fn progress(video: &str) -> ProgressRow<'_> {
    ProgressRow {
        video_id: video,
        item_id: "tt1",
        position_ms: 5,
        duration_ms: 10,
        updated_ms: 1,
    }
}

fn item() -> LibraryItemRow {
    LibraryItemRow {
        item_id: "tt1".into(),
        kind: "movie".into(),
        name: "One".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    }
}

fn tracker(profile: &str) -> TrackerKey<'_> {
    TrackerKey {
        profile,
        tracker: "trakt",
        account: "acc",
    }
}

/// Every write a profile can have on a device, one of each.
fn fill(lib: &Library, profile: &str, at: u64) {
    let h = hlc(at, 1);
    lib.set_addons(profile, &[addon("https://a/manifest.json")], &h)
        .unwrap();
    lib.upsert_item(profile, &item(), true, &h).unwrap();
    lib.set_progress(profile, progress("tt1"), &h).unwrap();
    lib.set_watched(profile, "tt1", "tt1", true, &h).unwrap();
    lib.set_stream_memory(profile, "tt1", Some("group"), "{}", &h)
        .unwrap();
    lib.set_setting(profile, "streams.quality", "\"1080p\"", &h)
        .unwrap();
    lib.record_search(profile, "query", 1).unwrap();
    lib.set_tracker_account(profile, "trakt", Some("acc"), 1, 1)
        .unwrap();
    lib.put_tracker_intent(
        "tt1",
        "tt1",
        NewIntent {
            key: tracker(profile),
            op: IntentOp::Add,
            watched_ms: 1,
            session: None,
        },
    )
    .unwrap();
    lib.set_tracker_state(tracker(profile), "cursor", "1")
        .unwrap();
    lib.set_pin_failures(profile, 3).unwrap();
}

/// What a device keeps of a profile, as plain values.
fn rows(lib: &Library, profile: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |name: &str, present: bool| {
        if present {
            out.push(name.to_owned());
        }
    };
    push("profile", lib.profile(profile).unwrap().is_some());
    push("addons", !lib.addons(profile).unwrap().is_empty());
    push("library", lib.is_in_library(profile, "tt1").unwrap());
    push("progress", lib.progress(profile, "tt1").unwrap().is_some());
    push(
        "watched",
        !lib.watched_videos(profile, "tt1").unwrap().is_empty(),
    );
    push(
        "stream memory",
        lib.last_binge_group(profile, "tt1").unwrap().is_some(),
    );
    push("settings", !lib.settings(profile).unwrap().is_empty());
    push(
        "search history",
        !lib.recent_searches(profile, 10).unwrap().is_empty(),
    );
    push(
        "tracker account",
        lib.tracker_account(profile, "trakt").unwrap().is_some(),
    );
    push(
        "tracker outbox",
        !lib.ready_tracker_intents(tracker(profile), i64::MAX, 10)
            .unwrap()
            .is_empty(),
    );
    push(
        "tracker state",
        lib.tracker_state(tracker(profile), "cursor")
            .unwrap()
            .is_some(),
    );
    push("pin attempts", lib.pin_failures(profile).unwrap() > 0);
    // Registers other than the gone one, as sync would send them.
    let prefix = format!("{profile}\u{1f}");
    push(
        "registers",
        all(lib).iter().any(|r| {
            r.coll != coll::PROFILE_GONE && (r.key == profile || r.key.starts_with(&prefix))
        }),
    );
    out
}

fn gone_error(result: Result<impl std::fmt::Debug, LibraryError>) {
    assert!(
        matches!(result, Err(LibraryError::ProfileGone(ref p)) if p == "x"),
        "{result:?}"
    );
}

#[test]
fn a_profile_starts_with_its_lock_and_a_copy_of_the_owners_addons() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile(OWNER, OWNER, &Hlc::zero(1).encode())
        .unwrap();
    let owner = lib.profile(OWNER).unwrap().unwrap();
    assert_eq!(owner.lock, Some(None), "the owner bootstraps its lock");
    lib.set_addons(OWNER, &[addon("https://a/manifest.json")], &hlc(5, 1))
        .unwrap();

    lib.create_profile("x", "Kid", "fox:amber", &hlc(10, 1))
        .unwrap();
    let x = lib.profile("x").unwrap().unwrap();
    assert_eq!(
        (x.name.as_str(), x.avatar.as_str(), x.position, x.lock),
        ("Kid", "fox:amber", 1, Some(None))
    );
    assert_eq!(lib.addons("x").unwrap(), lib.addons(OWNER).unwrap());

    // A copy, not a link.
    lib.set_addons(OWNER, &[], &hlc(20, 1)).unwrap();
    assert_eq!(lib.addons("x").unwrap().len(), 1);
}

#[test]
fn deleting_a_profile_purges_every_table_and_keeps_the_others() {
    let lib = Library::temporary().unwrap();
    for p in ["x", "y"] {
        lib.create_profile(p, p, "", &hlc(1, 1)).unwrap();
        lib.set_pin_hash(p, Some("$argon2id$..."), &hlc(2, 1))
            .unwrap();
        fill(&lib, p, 3);
    }
    let full = rows(&lib, "y");
    assert_eq!(full.len(), 13, "every kind of row is there: {full:?}");
    assert_eq!(rows(&lib, "x"), full);

    lib.delete_profile("x", &hlc(4, 1)).unwrap();
    assert!(rows(&lib, "x").is_empty(), "{:?}", rows(&lib, "x"));
    assert_eq!(rows(&lib, "y"), full, "other profiles stay");
    assert!(lib.is_gone("x").unwrap());
    assert_eq!(lib.gone_profiles().unwrap(), ["x"]);
    // The gone register is what syncs.
    let gone: Vec<_> = all(&lib)
        .into_iter()
        .filter(|r| r.key == "x")
        .map(|r| (r.coll, r.value))
        .collect();
    assert_eq!(
        gone,
        [(coll::PROFILE_GONE, serde_json::json!({"profile": "x"}))]
    );
}

#[test]
fn every_local_writer_is_refused_for_a_gone_profile() {
    let lib = Library::temporary().unwrap();
    lib.create_profile("x", "x", "", &hlc(1, 1)).unwrap();
    lib.delete_profile("x", &hlc(2, 1)).unwrap();
    let before = lib.outbox(1000).unwrap();
    let h = hlc(3, 1);
    gone_error(lib.set_addons("x", &[], &h));
    gone_error(lib.upsert_item("x", &item(), true, &h));
    gone_error(lib.set_progress("x", progress("tt1"), &h));
    gone_error(lib.set_watched("x", "tt1", "tt1", true, &h));
    gone_error(lib.set_watched_by_hand("x", "tt1", "tt1", true, &h, None));
    gone_error(lib.set_stream_memory("x", "tt1", None, "{}", &h));
    gone_error(lib.set_setting("x", "k", "1", &h));
    gone_error(lib.record_search("x", "q", 1));
    gone_error(lib.ensure_profile("x", "x", &h));
    gone_error(lib.update_profile("x", "back", "", 0, &h));
    gone_error(lib.set_pin_hash("x", None, &h));
    gone_error(lib.set_tracker_account("x", "trakt", Some("acc"), 1, 1));
    gone_error(lib.set_tracker_state(tracker("x"), "cursor", "1"));
    gone_error(lib.put_tracker_intent(
        "tt1",
        "tt1",
        NewIntent {
            key: tracker("x"),
            op: IntentOp::Add,
            watched_ms: 1,
            session: None,
        },
    ));
    let play = ImportedPlay {
        item_id: "tt1".into(),
        video_id: "tt1".into(),
        kind: "movie".into(),
        name: "One".into(),
        watched_ms: 1,
    };
    gone_error(lib.import_plays("x", &[play], 10, &[]));
    assert!(rows(&lib, "x").is_empty());
    assert_eq!(lib.outbox(1000).unwrap(), before, "nothing queued");
}

#[test]
fn the_owner_is_never_gone() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile(OWNER, OWNER, &hlc(1, 1)).unwrap();
    assert!(matches!(
        lib.delete_profile(OWNER, &hlc(2, 1)),
        Err(LibraryError::OwnerProfile)
    ));
    let forged = SyncRecord {
        coll: coll::PROFILE_GONE,
        key: OWNER.into(),
        value: serde_json::json!({"profile": OWNER}),
        hlc: hlc(3, 2),
    };
    assert_eq!(lib.apply(&[forged]).unwrap(), 0);
    assert!(!lib.is_gone(OWNER).unwrap());
    assert!(lib.profile(OWNER).unwrap().is_some());
}

/// All orders of `items`.
fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, first.clone());
            out.push(tail);
        }
    }
    out
}

/// The profile, lock, progress and gone records of profile `x` as another
/// device sends them.
fn source_records() -> [SyncRecord; 4] {
    let a = Library::temporary().unwrap();
    a.create_profile("x", "Kid", "fox", &hlc(10, 1)).unwrap();
    a.set_pin_hash("x", Some("$argon2id$v=19$stored"), &hlc(11, 1))
        .unwrap();
    a.set_progress("x", progress("tt1"), &hlc(12, 1)).unwrap();
    let find = |c: u8| {
        all(&a)
            .into_iter()
            .find(|r| r.coll == c && r.key.starts_with('x'))
            .unwrap()
    };
    let (profile, lock, child) = (
        find(coll::PROFILES),
        find(coll::PROFILE_LOCK),
        find(coll::PROGRESS),
    );
    a.delete_profile("x", &hlc(13, 1)).unwrap();
    [profile, lock, child, find(coll::PROFILE_GONE)]
}

#[test]
fn profile_lock_and_gone_records_converge_in_every_arrival_order() {
    let [profile, lock, child, gone] = source_records();
    // With the deletion, in any order, one record per pull or one page:
    // the profile ends gone with nothing left.
    for order in permutations(&[&profile, &lock, &child, &gone]) {
        for paged in [false, true] {
            let b = Library::temporary().unwrap();
            if paged {
                b.apply(&order.iter().copied().cloned().collect::<Vec<_>>())
                    .unwrap();
            } else {
                for r in &order {
                    b.apply(std::slice::from_ref(*r)).unwrap();
                }
            }
            let kinds: Vec<u8> = order.iter().map(|r| r.coll).collect();
            assert!(b.is_gone("x").unwrap(), "{kinds:?}");
            assert!(rows(&b, "x").is_empty(), "{kinds:?}: {:?}", rows(&b, "x"));
            assert!(b.profiles().unwrap().is_empty(), "{kinds:?}");
        }
    }
    // Without it: listed once the profile arrives, usable (its PIN known)
    // only once the lock has arrived too.
    for order in permutations(&[&profile, &lock, &child]) {
        let b = Library::temporary().unwrap();
        let mut seen = Vec::new();
        for r in &order {
            b.apply(std::slice::from_ref(*r)).unwrap();
            seen.push(r.coll);
            let row = b.profile("x").unwrap();
            let state = row.map(|p| p.lock.map(|l| l.is_some()));
            let expected = match (
                seen.contains(&coll::PROFILES),
                seen.contains(&coll::PROFILE_LOCK),
            ) {
                (false, _) => None,
                (true, false) => Some(None),
                (true, true) => Some(Some(true)),
            };
            assert_eq!(state, expected, "after {seen:?}");
        }
    }
}

#[test]
fn a_cosmetic_edit_never_carries_an_old_pin_back() {
    let a = Library::temporary().unwrap();
    let b = Library::temporary().unwrap();
    a.create_profile("x", "Kid", "fox", &hlc(10, 1)).unwrap();
    a.set_pin_hash("x", Some("old"), &hlc(11, 1)).unwrap();
    b.apply(&all(&a)).unwrap();
    // A changes the PIN; B, offline, changes the avatar later.
    a.set_pin_hash("x", Some("new"), &hlc(20, 1)).unwrap();
    b.update_profile("x", "Kid", "owl", 0, &hlc(30, 2)).unwrap();
    a.apply(&all(&b)).unwrap();
    b.apply(&all(&a)).unwrap();
    for lib in [&a, &b] {
        let x = lib.profile("x").unwrap().unwrap();
        assert_eq!((x.avatar.as_str(), x.pin_hash()), ("owl", Some("new")));
    }
}

#[test]
fn profile_registers_take_part_in_every_sync_path() {
    let lib = Library::temporary().unwrap();
    lib.create_profile("x", "x", "", &hlc(10, 1)).unwrap();
    lib.create_profile("y", "y", "", &hlc(10, 1)).unwrap();
    // Restart: the clock is seeded from the newest register, lock and gone
    // included.
    lib.set_pin_hash("x", Some("h"), &hlc(50, 1)).unwrap();
    assert_eq!(lib.newest_hlc().unwrap(), Some(hlc(50, 1)));
    lib.delete_profile("y", &hlc(60, 1)).unwrap();
    assert_eq!(lib.newest_hlc().unwrap(), Some(hlc(60, 1)));

    // The outbox and single register reads carry them.
    let queued: Vec<(u8, String)> = lib
        .outbox(1000)
        .unwrap()
        .records
        .into_iter()
        .map(|(_, r)| (r.coll, r.key))
        .collect();
    assert!(queued.contains(&(coll::PROFILE_LOCK, "x".into())));
    assert!(queued.contains(&(coll::PROFILE_GONE, "y".into())));
    let lock = lib.register(coll::PROFILE_LOCK, "x").unwrap().unwrap();
    assert_eq!(
        lock.value,
        serde_json::json!({"profile": "x", "pinHash": "h"})
    );

    // A relay restore queues everything again.
    lib.outbox_done(lib.outbox(1000).unwrap().last_id.unwrap())
        .unwrap();
    lib.queue_all().unwrap();
    let requeued: Vec<u8> = lib
        .outbox(1000)
        .unwrap()
        .records
        .into_iter()
        .map(|(_, r)| r.coll)
        .collect();
    assert!(requeued.contains(&coll::PROFILE_LOCK));
    assert!(requeued.contains(&coll::PROFILE_GONE));

    // A register stamped by a clock that ran ahead can be stamped again.
    for (c, key) in [(coll::PROFILE_LOCK, "x"), (coll::PROFILE_GONE, "y")] {
        lib.restamp(c, key, &hlc(40, 1)).unwrap();
        assert_eq!(lib.register(c, key).unwrap().unwrap().hlc, hlc(40, 1));
    }

    // Failed attempts and the active profile never sync.
    lib.set_pin_failures("x", 2).unwrap();
    lib.set_active_profile("x").unwrap();
    assert_eq!(lib.active_profile().unwrap().as_deref(), Some("x"));
    let other = Library::temporary().unwrap();
    other.apply(&all(&lib)).unwrap();
    assert_eq!(other.pin_failures("x").unwrap(), 0);
    assert_eq!(other.active_profile().unwrap(), None);
    assert_eq!(other.profile("x").unwrap().unwrap().pin_hash(), Some("h"));
    assert!(other.is_gone("y").unwrap());
}
