//! Kids mode's data (plans/m11-kids.md 3 and 4.1, phase K1): the access and
//! allow registers through every sync path and arrival order, the rules for
//! the owner, the bounds of an entry, and the policy that fails closed.

use mq_library::{
    AllowedRow, Authorization, Hlc, Library, LibraryError, OWNER, Policy, SyncCursor, SyncRecord,
    Table, coll,
};
use serde_json::{Value, json};

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

fn entry(id: &str, kind: &str, added_ms: i64) -> AllowedRow {
    AllowedRow {
        item_id: id.into(),
        kind: kind.into(),
        name: format!("Title {id}"),
        names: vec![],
        poster: None,
        allowed: true,
        added_ms,
    }
}

fn removal(id: &str) -> AllowedRow {
    AllowedRow {
        allowed: false,
        ..entry(id, "movie", 1)
    }
}

/// A kids profile with `titles` allowed, on a fresh library.
fn kid(titles: &[(&str, &str)]) -> Library {
    let lib = Library::temporary().unwrap();
    lib.create_kids_profile("kid", "Kid", "fox", &hlc(10, 1))
        .unwrap();
    for (i, (id, kind)) in titles.iter().enumerate() {
        lib.write_allowed(
            "kid",
            &entry(id, kind, i as i64 + 1),
            &hlc(20 + i as u64, 1),
        )
        .unwrap();
    }
    lib
}

fn record(coll: u8, key: &str, value: Value, wall: u64) -> SyncRecord {
    SyncRecord {
        coll,
        key: key.into(),
        value,
        hlc: hlc(wall, 2),
    }
}

fn allow_record(profile: &str, value: Value, wall: u64) -> SyncRecord {
    let id = value["itemId"].as_str().unwrap_or_default().to_owned();
    record(coll::ALLOWED, &format!("{profile}\u{1f}{id}"), value, wall)
}

fn generation(lib: &Library) -> u64 {
    lib.policy_checked(OWNER).unwrap().1
}

#[test]
fn kids_registers_take_part_in_every_sync_path() {
    let lib = kid(&[("tt1", "movie")]);
    // The clock is seeded from the newest register, these included.
    lib.write_allowed("kid", &entry("tt2", "series", 9), &hlc(90, 1))
        .unwrap();
    assert_eq!(lib.newest_hlc().unwrap(), Some(hlc(90, 1)));
    lib.make_standard("kid", &hlc(95, 1)).unwrap();
    assert_eq!(lib.newest_hlc().unwrap(), Some(hlc(95, 1)));

    // Export: both collections, keyed as the values say.
    let exported = all(&lib);
    let access = exported
        .iter()
        .find(|r| r.coll == coll::PROFILE_ACCESS && r.key == "kid")
        .unwrap();
    assert_eq!(access.value, json!({"profile": "kid", "kids": false}));
    let allowed = lib
        .register(coll::ALLOWED, "kid\u{1f}tt2")
        .unwrap()
        .unwrap();
    assert_eq!(
        allowed.value,
        json!({"profile": "kid", "itemId": "tt2", "kind": "series", "name": "Title tt2",
               "names": [], "poster": null, "allowed": true, "addedMs": 9})
    );

    // The outbox carries both.
    let queued: Vec<(u8, String)> = lib
        .outbox(1000)
        .unwrap()
        .records
        .into_iter()
        .map(|(_, r)| (r.coll, r.key))
        .collect();
    assert!(queued.contains(&(coll::PROFILE_ACCESS, "kid".into())));
    assert!(queued.contains(&(coll::ALLOWED, "kid\u{1f}tt1".into())));

    // A relay restore queues them again.
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
    assert!(requeued.contains(&coll::PROFILE_ACCESS));
    assert!(requeued.contains(&coll::ALLOWED));

    // A register stamped by a clock that ran ahead is stamped again.
    for (c, key) in [
        (coll::PROFILE_ACCESS, "kid"),
        (coll::ALLOWED, "kid\u{1f}tt1"),
    ] {
        lib.restamp(c, key, &hlc(5, 1)).unwrap();
        assert_eq!(lib.register(c, key).unwrap().unwrap().hlc, hlc(5, 1));
    }

    // Another device applies them all.
    let other = Library::temporary().unwrap();
    other.apply(&all(&lib)).unwrap();
    assert_eq!(all(&other), all(&lib));
    assert_eq!(other.allowed_list("kid").unwrap().counted.len(), 2);
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

/// The profile, lock, access, allow and gone records of kids profile `x`.
fn source_records() -> [SyncRecord; 5] {
    let a = Library::temporary().unwrap();
    a.create_kids_profile("x", "Kid", "fox", &hlc(10, 1))
        .unwrap();
    a.write_allowed("x", &entry("tt1", "movie", 1), &hlc(11, 1))
        .unwrap();
    let find = |c: u8| {
        all(&a)
            .into_iter()
            .find(|r| r.coll == c && r.key.starts_with('x'))
            .unwrap()
    };
    let records = [
        find(coll::PROFILES),
        find(coll::PROFILE_LOCK),
        find(coll::PROFILE_ACCESS),
        find(coll::ALLOWED),
    ];
    a.delete_profile("x", &hlc(12, 1)).unwrap();
    let [p, l, ac, al] = records;
    [p, l, ac, al, find(coll::PROFILE_GONE)]
}

/// What a device keeps of profile `x`, as sync would send it.
fn registers(lib: &Library) -> Vec<(u8, String, Value)> {
    all(lib)
        .into_iter()
        .filter(|r| r.key.starts_with('x') && r.coll != coll::PROFILE_GONE)
        .map(|r| (r.coll, r.key, r.value))
        .collect()
}

#[test]
fn every_arrival_order_converges_and_kids_are_never_ready_early() {
    let records = source_records();
    let gone = coll::PROFILE_GONE;
    for order in permutations(&records.iter().collect::<Vec<_>>()) {
        let kinds: Vec<u8> = order.iter().map(|r| r.coll).collect();
        // One record per pull: at no step is a kids profile usable before
        // its access record, and never as a standard one.
        let b = Library::temporary().unwrap();
        let mut seen = Vec::new();
        for r in &order {
            b.apply(std::slice::from_ref(*r)).unwrap();
            seen.push(r.coll);
            let row = b.profile("x").unwrap();
            let has = |c: u8| seen.contains(&c);
            if has(gone) || !has(coll::PROFILES) {
                assert_eq!(row, None, "after {seen:?}");
                continue;
            }
            let row = row.unwrap();
            let ready = has(coll::PROFILE_LOCK) && has(coll::PROFILE_ACCESS);
            assert_eq!(row.ready(), ready, "after {seen:?}");
            assert!(!ready || row.kids(), "ready only as kids: {seen:?}");
            assert!(b.policy("x").kids(), "never open: {seen:?}");
        }
        // The same records as one page.
        let c = Library::temporary().unwrap();
        c.apply(&order.iter().copied().cloned().collect::<Vec<_>>())
            .unwrap();
        for lib in [&b, &c] {
            assert!(lib.is_gone("x").unwrap(), "{kinds:?}");
            assert!(registers(lib).is_empty(), "{kinds:?}: {:?}", registers(lib));
            assert!(lib.allowed_list("x").unwrap().counted.is_empty());
        }
    }
    // Without the removal, every order ends in the same rows.
    let [profile, lock, access, allow, _] = &records;
    let mut ends = Vec::new();
    for order in permutations(&[profile, lock, access, allow]) {
        let b = Library::temporary().unwrap();
        for r in &order {
            b.apply(std::slice::from_ref(*r)).unwrap();
        }
        let x = b.profile("x").unwrap().unwrap();
        assert!(x.ready() && x.kids());
        assert!(b.policy("x").allows("movie", "tt1"));
        ends.push(registers(&b));
    }
    assert!(ends.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn a_profile_already_here_never_gets_a_guessed_access() {
    let [profile, lock, ..] = source_records();
    let b = Library::temporary().unwrap();
    b.apply(&[profile, lock]).unwrap();
    // Opening it by id (a test core, the CLI) must not make it standard.
    b.ensure_profile("x", "x", &hlc(0, 7)).unwrap();
    let x = b.profile("x").unwrap().unwrap();
    assert_eq!(x.access, None, "still syncing");
    assert!(!x.ready());
    // A profile the call creates is a local, standard one.
    b.ensure_profile("p", "p", &hlc(0, 7)).unwrap();
    assert!(b.profile("p").unwrap().unwrap().ready());
    assert_eq!(b.policy("p"), Policy::Open);
}

#[test]
fn a_removed_profile_takes_both_tables() {
    let local = kid(&[("tt1", "movie")]);
    let synced = Library::temporary().unwrap();
    synced.apply(&all(&local)).unwrap();
    local.delete_profile("kid", &hlc(50, 1)).unwrap();
    let gone = all(&local)
        .into_iter()
        .find(|r| r.coll == coll::PROFILE_GONE)
        .unwrap();
    synced.apply(&[gone]).unwrap();
    for lib in [&local, &synced] {
        let left: Vec<u8> = all(lib)
            .into_iter()
            .filter(|r| r.key.starts_with("kid") && r.coll != coll::PROFILE_GONE)
            .map(|r| r.coll)
            .collect();
        assert!(left.is_empty(), "{left:?}");
        assert!(lib.allowed_list("kid").unwrap().counted.is_empty());
        assert_eq!(lib.policy("kid"), Policy::Kids(Default::default()));
    }
}

#[test]
fn the_owner_is_never_kids() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile(OWNER, "Main", &hlc(0, 1)).unwrap();
    assert!(matches!(
        lib.create_kids_profile(OWNER, "Main", "", &hlc(1, 1)),
        Err(LibraryError::OwnerKids)
    ));
    assert!(matches!(
        lib.write_allowed(OWNER, &entry("tt1", "movie", 1), &hlc(2, 1)),
        Err(LibraryError::OwnerKids)
    ));
    // From another device: skipped, whatever the hlc.
    let forged = [
        record(
            coll::PROFILE_ACCESS,
            OWNER,
            json!({"profile": OWNER, "kids": true}),
            90,
        ),
        allow_record(
            OWNER,
            json!({"profile": OWNER, "itemId": "tt1", "kind": "movie", "name": "One",
                   "allowed": true, "addedMs": 1}),
            90,
        ),
    ];
    assert_eq!(lib.apply(&forged).unwrap(), 0);
    assert_eq!(lib.policy(OWNER), Policy::Open);
    assert!(lib.allowed_list(OWNER).unwrap().counted.is_empty());
    assert!(!lib.profile(OWNER).unwrap().unwrap().kids());
}

#[test]
fn an_access_record_without_kids_changes_nothing() {
    let lib = kid(&[("tt1", "movie")]);
    lib.create_profile("std", "Std", "", &hlc(10, 1)).unwrap();
    // Newer, but without the field: the kid stays kids, the standard
    // profile stays standard, an arriving profile stays syncing.
    let forged = [
        record(coll::PROFILE_ACCESS, "kid", json!({"profile": "kid"}), 90),
        record(coll::PROFILE_ACCESS, "std", json!({"profile": "std"}), 90),
        record(coll::PROFILE_ACCESS, "y", json!({"profile": "y"}), 90),
    ];
    assert_eq!(lib.apply(&forged).unwrap(), 0);
    assert!(lib.profile("kid").unwrap().unwrap().kids());
    assert!(lib.policy("kid").kids());
    assert_eq!(lib.policy("std"), Policy::Open);
    let y = [
        record(
            coll::PROFILES,
            "y",
            json!({"profile": "y", "name": "y"}),
            90,
        ),
        record(
            coll::PROFILE_LOCK,
            "y",
            json!({"profile": "y", "pinHash": null}),
            90,
        ),
    ];
    lib.apply(&y).unwrap();
    assert!(!lib.profile("y").unwrap().unwrap().ready(), "still syncing");
    // An explicit value is a real change.
    let made_standard = record(
        coll::PROFILE_ACCESS,
        "kid",
        json!({"profile": "kid", "kids": false}),
        91,
    );
    assert_eq!(lib.apply(&[made_standard]).unwrap(), 1);
    assert_eq!(lib.policy("kid"), Policy::Open);
}

#[test]
fn allow_values_from_another_device_are_held_to_the_local_rules() {
    let lib = kid(&[]);
    let long = "N".repeat(250);
    let values = [
        // Kept, cleaned and cut: control and format characters out,
        // whitespace folded, at most 200 characters.
        json!({"profile": "kid", "itemId": "tt1", "kind": "movie",
               "name": format!("  Up\u{200b}\u{7}  and\n\taway {long}"),
               "names": ["Up", "Yukarı", "Yukarı", "", "A", "B", "C", "D"],
               "poster": "http://plain.example/p.jpg", "allowed": true, "addedMs": 1}),
        // Missing fields read as none, a missing `allowed` as a removal.
        json!({"profile": "kid", "itemId": "tt2", "kind": "movie"}),
        // Skipped: ids out of bounds.
        json!({"profile": "kid", "itemId": "", "kind": "movie", "allowed": true}),
        json!({"profile": "kid", "itemId": "tt3", "kind": "k".repeat(33), "allowed": true}),
        json!({"profile": "kid", "itemId": "t".repeat(257), "kind": "movie", "allowed": true}),
        json!({"profile": "kid", "itemId": "tt4", "kind": "", "allowed": true}),
        // A poster too long for the rule.
        json!({"profile": "kid", "itemId": "tt5", "kind": "movie", "allowed": true,
               "poster": format!("https://img.example/{}", "p".repeat(500))}),
    ];
    let records: Vec<SyncRecord> = values
        .into_iter()
        .map(|v| allow_record("kid", v, 90))
        .collect();
    assert_eq!(lib.apply(&records).unwrap(), 3);
    let one = lib
        .register(coll::ALLOWED, "kid\u{1f}tt1")
        .unwrap()
        .unwrap();
    let name = one.value["name"].as_str().unwrap();
    assert!(name.starts_with("Up and away NNN"), "{name}");
    assert_eq!(name.chars().count(), 200);
    assert_eq!(one.value["names"], json!(["Up", "Yukarı", "A", "B"]));
    assert_eq!(one.value["poster"], Value::Null);
    let two = lib
        .register(coll::ALLOWED, "kid\u{1f}tt2")
        .unwrap()
        .unwrap();
    assert_eq!(
        two.value,
        json!({"profile": "kid", "itemId": "tt2", "kind": "movie", "name": "",
               "names": [], "poster": null, "allowed": false, "addedMs": 0})
    );
    for id in ["tt3", "tt4"] {
        let key = format!("kid\u{1f}{id}");
        assert_eq!(lib.register(coll::ALLOWED, &key).unwrap(), None, "{id}");
    }
    let five = lib
        .register(coll::ALLOWED, "kid\u{1f}tt5")
        .unwrap()
        .unwrap();
    assert_eq!(five.value["poster"], Value::Null);
    // Local writes take the same rules.
    assert!(matches!(
        lib.write_allowed("kid", &entry("", "movie", 1), &hlc(99, 1)),
        Err(LibraryError::BadEntry(_))
    ));
}

#[test]
fn names_stay_inside_one_kib_of_json_however_they_escape() {
    let lib = kid(&[]);
    // Five different names of 200 characters that each escape to two
    // bytes, with format characters between them that would escape to six
    // if they stayed.
    let heavy = |pair: &str| -> String {
        let pair: Vec<char> = pair.chars().collect();
        (0..200)
            .map(|i| format!("{}\u{200b}\u{feff}", pair[i % 2]))
            .collect()
    };
    let row = AllowedRow {
        name: heavy("\"\""),
        names: vec![
            heavy("\\\\"),
            heavy("\"\\"),
            heavy("\\\""),
            heavy("\"\u{7f}\""),
        ],
        poster: Some(format!("https://img.example/{}", "p".repeat(480))),
        ..entry("tt1", "movie", 1)
    };
    lib.write_allowed("kid", &row, &hlc(30, 1)).unwrap();
    let stored = lib
        .register(coll::ALLOWED, "kid\u{1f}tt1")
        .unwrap()
        .unwrap();
    let names_bytes = serde_json::to_string(&stored.value["name"]).unwrap().len()
        + serde_json::to_string(&stored.value["names"]).unwrap().len();
    assert!(names_bytes <= 1024, "{names_bytes}");
    assert!(stored.value.to_string().len() <= 4096);
    let name = stored.value["name"].as_str().unwrap();
    assert_eq!(name, "\"".repeat(200), "format characters gone first");
    // The same holds for what another device sends.
    let other = kid(&[]);
    other
        .apply(&[allow_record("kid", stored.value.clone(), 31)])
        .unwrap();
    assert_eq!(
        other
            .register(coll::ALLOWED, "kid\u{1f}tt1")
            .unwrap()
            .unwrap()
            .value,
        stored.value
    );
}

#[test]
fn a_slow_allow_never_overtakes_a_later_remove() {
    let lib = kid(&[]);
    // The Allow took its hlc (40) when it arrived; its fetch is slow, and
    // the Remove (50) is written first.
    assert!(
        lib.write_allowed("kid", &removal("tt1"), &hlc(50, 1))
            .unwrap()
    );
    assert!(
        !lib.write_allowed("kid", &entry("tt1", "movie", 1), &hlc(40, 1))
            .unwrap(),
        "the older Allow changes nothing"
    );
    let stored = lib
        .register(coll::ALLOWED, "kid\u{1f}tt1")
        .unwrap()
        .unwrap();
    assert_eq!(stored.hlc, hlc(50, 1), "never stamped again above it");
    assert!(!lib.policy("kid").allows("movie", "tt1"));
    // Another device ends the same way.
    let other = Library::temporary().unwrap();
    other.apply(&all(&lib)).unwrap();
    assert!(!other.policy("kid").allows("movie", "tt1"));
}

#[test]
fn every_name_reaches_the_other_device() {
    let a = kid(&[]);
    let row = AllowedRow {
        name: "Up".into(),
        names: vec!["Yukarı Bak".into(), "Oben".into()],
        ..entry("tt1", "movie", 1)
    };
    a.write_allowed("kid", &row, &hlc(30, 1)).unwrap();
    let b = Library::temporary().unwrap();
    b.apply(&all(&a)).unwrap();
    let counted = b.allowed_list("kid").unwrap().counted;
    assert_eq!(counted[0].names, ["Yukarı Bak", "Oben"]);
}

#[test]
fn every_access_commit_raises_the_generation() {
    let lib = kid(&[]);
    let mut last = generation(&lib);
    let mut raised = |what: &str| {
        let now = generation(&lib);
        assert!(now > last, "{what}: {now} after {last}");
        last = now;
    };
    lib.write_allowed("kid", &entry("tt1", "movie", 1), &hlc(30, 1))
        .unwrap();
    raised("a local allow");
    let access = record(
        coll::PROFILE_ACCESS,
        "kid",
        json!({"profile": "kid", "kids": true}),
        40,
    );
    lib.apply(&[access]).unwrap();
    raised("an applied access record");
    lib.make_standard("kid", &hlc(50, 1)).unwrap();
    raised("make standard");
    lib.delete_profile("kid", &hlc(60, 1)).unwrap();
    raised("a removal");
    // Other tables leave it alone.
    let before = generation(&lib);
    lib.set_setting(OWNER, "k", "1", &hlc(70, 1)).unwrap();
    assert_eq!(generation(&lib), before);
}

#[test]
fn access_changes_are_announced() {
    let lib = kid(&[]);
    let mut changes = lib.subscribe();
    lib.write_allowed("kid", &entry("tt1", "movie", 1), &hlc(30, 1))
        .unwrap();
    assert!(changes.try_recv().unwrap().contains(&Table::Access));
}

#[test]
fn kids_profiles_start_empty_and_read_no_addons_of_their_own() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile(OWNER, "Main", &hlc(0, 1)).unwrap();
    let addon = mq_library::AddonRow {
        transport_url: "https://a/manifest.json".into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    lib.set_addons(OWNER, &[addon], &hlc(1, 1)).unwrap();
    lib.create_kids_profile("kid", "Kid", "", &hlc(2, 1))
        .unwrap();
    let row = lib.profile("kid").unwrap().unwrap();
    assert!(row.ready() && row.kids());
    assert!(lib.addons("kid").unwrap().is_empty());
    assert_eq!(lib.policy("kid"), Policy::Kids(Default::default()));
    // Made standard: open, with a copy of the owner's addons.
    lib.make_standard("kid", &hlc(3, 1)).unwrap();
    assert_eq!(lib.policy("kid"), Policy::Open);
    assert_eq!(lib.addons("kid").unwrap().len(), 1);
    // Standard profiles are open from the start.
    lib.create_profile("std", "Std", "", &hlc(4, 1)).unwrap();
    assert_eq!(lib.policy("std"), Policy::Open);
}

#[test]
fn the_policy_counts_allowed_titles_by_kind() {
    let lib = kid(&[("tt1", "movie"), ("tt2", "series"), ("tt3", "movie")]);
    lib.write_allowed("kid", &removal("tt3"), &hlc(80, 1))
        .unwrap();
    let policy = lib.policy("kid");
    assert!(policy.allows("movie", "tt1"));
    assert!(policy.allows("series", "tt2"));
    assert!(!policy.allows("series", "tt1"), "the kind must match");
    assert!(!policy.allows("movie", "tt3"), "a removal counts nothing");
    assert!(!policy.allows("", "tt1") && !policy.allows("movie", ""));
    assert!(!policy.allows("movie", "tt9"));
    assert!(Policy::Open.allows("movie", "tt9"));
}

#[test]
fn the_cap_counts_the_first_thousand_by_allow_time_on_every_device() {
    let lib = kid(&[]);
    // 1,002 titles, written newest id first so the id order disagrees with
    // the allow time; two share the oldest time and are told apart by id.
    let ids: Vec<String> = (0..1002).map(|i| format!("tt{i:04}")).collect();
    for (i, id) in ids.iter().enumerate().rev() {
        let added = if i == 0 { 1 } else { i as i64 };
        lib.write_allowed("kid", &entry(id, "movie", added), &hlc(100 + i as u64, 1))
            .unwrap();
    }
    let list = lib.allowed_list("kid").unwrap();
    assert_eq!(list.counted.len(), 1000);
    assert_eq!(list.over_count, 2);
    let over: Vec<&str> = list.over.iter().map(|r| r.item_id.as_str()).collect();
    assert_eq!(over, ["tt1001", "tt1000"], "newest first");
    let policy = lib.policy("kid");
    assert!(policy.allows("movie", "tt0000") && policy.allows("movie", "tt0999"));
    assert!(!policy.allows("movie", "tt1000") && !policy.allows("movie", "tt1001"));
    // Another device holding the same rows counts the same ones.
    let other = Library::temporary().unwrap();
    other.apply(&all(&lib)).unwrap();
    assert_eq!(other.policy("kid"), policy);
    // An older allow arriving late takes its place: the newest counted one
    // goes over (3.4, the price of agreeing without coordination).
    let late = AllowedRow {
        added_ms: 0,
        ..entry("tt9999", "movie", 0)
    };
    lib.write_allowed("kid", &late, &hlc(5000, 1)).unwrap();
    let policy = lib.policy("kid");
    assert!(policy.allows("movie", "tt9999"));
    assert!(!policy.allows("movie", "tt0999"), "pushed over");
}

#[test]
fn belonging_needs_the_colon() {
    assert!(Policy::belongs("tt1", "tt1"));
    assert!(Policy::belongs("tt1", "tt1:1:2"));
    assert!(!Policy::belongs("tt1", "tt10"));
    assert!(!Policy::belongs("tt1", "tt10:1:1"));
    assert!(!Policy::belongs("", ":1:1"));
    assert!(Policy::belongs("kitsu:123", "kitsu:123:5"));
}

#[test]
fn authorize_checks_the_title_the_video_and_names_its_generation() {
    let lib = kid(&[("tt1", "series")]);
    let (_, now) = lib.policy_checked("kid").unwrap();
    let authorized = Authorization::Authorized { generation: now };
    let denied = Authorization::Denied { generation: now };
    let auth = |kind, id, video| lib.authorize("kid", kind, id, video).unwrap();
    assert_eq!(auth("series", "tt1", Some("tt1:1:2")), authorized);
    assert_eq!(auth("series", "tt1", None), authorized);
    assert_eq!(auth("series", "tt1", Some("tt10:1:1")), denied);
    assert_eq!(auth("movie", "tt1", None), denied);
    assert_eq!(auth("series", "tt2", None), denied);
    // An open profile plays anything.
    lib.create_profile("std", "Std", "", &hlc(5, 1)).unwrap();
    assert!(matches!(
        lib.authorize("std", "movie", "x", Some("y")).unwrap(),
        Authorization::Authorized { .. }
    ));
}

/// A library in its own file, so a test can damage it from the side.
fn on_disk(name: &str) -> (Library, std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("mq-kids-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("library.sqlite");
    let lib = Library::open(&path).unwrap();
    lib.create_kids_profile("kid", "Kid", "", &hlc(1, 1))
        .unwrap();
    lib.write_allowed("kid", &entry("tt1", "movie", 1), &hlc(2, 1))
        .unwrap();
    lib.create_profile("std", "Std", "", &hlc(3, 1)).unwrap();
    (lib, path, dir)
}

#[test]
fn the_policy_fails_closed() {
    let empty = Policy::Kids(Default::default());
    // No access register: an unknown profile, or one whose record is
    // still on its way.
    let lib = Library::temporary().unwrap();
    assert_eq!(lib.policy("nobody"), empty);
    let profile = record(coll::PROFILES, "y", json!({"profile": "y", "name": "y"}), 9);
    lib.apply(&[profile]).unwrap();
    assert_eq!(lib.policy("y"), empty);

    // An unreadable row.
    let (lib, path, dir) = on_disk("unreadable");
    let side = rusqlite::Connection::open(&path).unwrap();
    side.execute(
        "UPDATE profile_access SET kids = 'yes' WHERE profile_id = 'std'",
        [],
    )
    .unwrap();
    assert_eq!(lib.policy("std"), empty);
    assert!(
        lib.policy_checked("std").is_err(),
        "told apart from a denial"
    );
    side.execute(
        "UPDATE allowed SET names = 'not json' WHERE profile_id = 'kid'",
        [],
    )
    .unwrap();
    assert_eq!(lib.policy("kid"), empty);

    // A read error.
    side.execute_batch("DROP TABLE allowed").unwrap();
    assert_eq!(lib.policy("kid"), empty);
    assert!(lib.policy_checked("kid").is_err());
    assert!(lib.authorize("kid", "movie", "tt1", None).is_err());
    drop((side, lib));
    let _ = std::fs::remove_dir_all(dir);
}
