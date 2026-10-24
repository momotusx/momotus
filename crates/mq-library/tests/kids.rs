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
    lib.set_pin_hash(OWNER, Some("h"), &hlc(1, 1)).unwrap();
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
    a.set_pin_hash(OWNER, Some("h"), &hlc(1, 1)).unwrap();
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

/// A test core opening `x` by id before its creator's records arrive: `x`
/// is standard at the zero hlc until they do, then kids (plans/m11-kids.md
/// 3.5).
#[test]
fn a_profile_ensure_creates_yields_to_its_creators_access() {
    let [profile, lock, access, ..] = source_records();
    let b = Library::temporary().unwrap();
    b.ensure_profile("x", "x", &hlc(0, 7)).unwrap();
    assert!(b.profile("x").unwrap().unwrap().ready());
    assert_eq!(b.policy("x"), Policy::Open);
    b.apply(&[profile, lock, access]).unwrap();
    let x = b.profile("x").unwrap().unwrap();
    assert!(x.ready() && x.kids());
    assert_eq!(b.policy("x"), Policy::Kids(Default::default()));
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
    // bytes, with format characters between them, which show as nothing
    // and would only take room if they stayed.
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
    lib.set_pin_hash(OWNER, Some("h"), &hlc(1, 1)).unwrap();
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
    // 1,001 titles whose ids run against their allow times (the oldest
    // allow has the highest id), and the last two allows share one
    // millisecond across the cap: the id decides, never the order of
    // writing (the tie is written with the higher id first). This device
    // admits 1,000; the last comes from another device.
    let id = |i: usize| format!("tt{:04}", 1000 - i);
    let added = |i: usize| if i == 1000 { 999 } else { i as i64 };
    for i in 0..1000 {
        let row = entry(&id(i), "movie", added(i));
        lib.write_allowed("kid", &row, &hlc(100 + i as u64, 1))
            .unwrap();
    }
    let last = json!({"profile": "kid", "itemId": id(1000), "kind": "movie",
                      "name": "T", "allowed": true, "addedMs": added(1000)});
    lib.apply(&[allow_record("kid", last, 1100)]).unwrap();
    let list = lib.allowed_list("kid").unwrap();
    assert_eq!((list.counted.len(), list.over_count), (1000, 1));
    // tt0001 (written first) and tt0000 share the time: tt0000 counts.
    let over: Vec<&str> = list.over.iter().map(|r| r.item_id.as_str()).collect();
    assert_eq!(over, ["tt0001"], "the tie goes to the lower id");
    let policy = lib.policy("kid");
    assert!(policy.allows("movie", "tt1000"), "the oldest allow counts");
    assert!(
        policy.allows("movie", "tt0000"),
        "the tie's lower id counts"
    );
    assert!(!policy.allows("movie", "tt0001"));
    // Another device holding the same rows counts the same ones.
    let other = Library::temporary().unwrap();
    other.apply(&all(&lib)).unwrap();
    assert_eq!(other.policy("kid"), policy);
    // An older allow arriving late takes its place: the newest counted one
    // goes over (3.4, the price of agreeing without coordination).
    let late = json!({"profile": "kid", "itemId": "tt9999", "kind": "movie",
                      "name": "T", "allowed": true, "addedMs": 0});
    lib.apply(&[allow_record("kid", late, 5000)]).unwrap();
    let policy = lib.policy("kid");
    assert!(policy.allows("movie", "tt9999"));
    assert!(!policy.allows("movie", "tt0000"), "pushed over");
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
    assert_eq!(auth("series", "tt1", None), denied, "a series as a whole");
    assert_eq!(auth("movie", "tt1", Some("tt1")), denied, "another kind");
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
    lib.set_pin_hash(OWNER, Some("h"), &hlc(1, 1)).unwrap();
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

#[test]
fn a_remove_from_another_device_beats_a_delayed_allow() {
    let a = kid(&[]);
    let b = Library::temporary().unwrap();
    b.apply(&all(&a)).unwrap();
    // A's Allow took its hlc (40) when the owner pressed it; while its
    // fetch runs, B's Remove (50) arrives by sync.
    let allow_hlc = hlc(40, 1);
    b.write_allowed("kid", &removal("tt1"), &hlc(50, 2))
        .unwrap();
    let remove = b.register(coll::ALLOWED, "kid\u{1f}tt1").unwrap().unwrap();
    a.apply(&[remove]).unwrap();
    assert!(
        !a.write_allowed("kid", &entry("tt1", "movie", 1), &allow_hlc)
            .unwrap(),
        "the delayed Allow lands below the Remove"
    );
    assert!(!a.policy("kid").allows("movie", "tt1"));
    b.apply(&all(&a)).unwrap();
    assert!(!b.policy("kid").allows("movie", "tt1"));
}

#[test]
fn each_kind_of_access_change_raises_the_generation() {
    let (lib, path, dir) = on_disk("triggers");
    let side = rusqlite::Connection::open(&path).unwrap();
    let mut last = generation(&lib);
    let mut raised = |what: &str| {
        let now = generation(&lib);
        assert!(now > last, "{what}: {now} after {last}");
        last = now;
    };
    lib.set_pin_hash(OWNER, Some("h"), &hlc(1, 1)).unwrap();
    lib.create_kids_profile("k2", "K2", "", &hlc(10, 1))
        .unwrap();
    raised("an access row inserted");
    let access = record(
        coll::PROFILE_ACCESS,
        "k2",
        json!({"profile": "k2", "kids": false}),
        20,
    );
    lib.apply(&[access]).unwrap();
    raised("an access row updated");
    side.execute("DELETE FROM profile_access WHERE profile_id = 'k2'", [])
        .unwrap();
    raised("an access row deleted");
    lib.write_allowed("kid", &entry("tt2", "movie", 2), &hlc(30, 1))
        .unwrap();
    raised("an allow row inserted");
    lib.write_allowed("kid", &removal("tt2"), &hlc(40, 1))
        .unwrap();
    raised("an allow row updated");
    side.execute("DELETE FROM allowed WHERE item_id = 'tt2'", [])
        .unwrap();
    raised("an allow row deleted");
    drop((side, lib));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn allowing_again_keeps_the_allow_time() {
    let lib = kid(&[]);
    lib.write_allowed("kid", &entry("tt1", "movie", 5), &hlc(30, 1))
        .unwrap();
    // Allow pressed again later: the title keeps its place.
    lib.write_allowed("kid", &entry("tt1", "movie", 99), &hlc(31, 1))
        .unwrap();
    assert_eq!(lib.allowed_list("kid").unwrap().counted[0].added_ms, 5);
    // After a removal, an Allow is a new one.
    lib.write_allowed("kid", &removal("tt1"), &hlc(32, 1))
        .unwrap();
    lib.write_allowed("kid", &entry("tt1", "movie", 120), &hlc(33, 1))
        .unwrap();
    assert_eq!(lib.allowed_list("kid").unwrap().counted[0].added_ms, 120);
}

#[test]
fn a_profile_whose_access_is_unknown_reads_as_kids() {
    let lib = Library::temporary().unwrap();
    let profile = record(coll::PROFILES, "y", json!({"profile": "y", "name": "y"}), 9);
    lib.apply(&[profile]).unwrap();
    let y = lib.profile("y").unwrap().unwrap();
    assert_eq!(y.access, None);
    assert!(y.kids(), "fails closed");
    assert!(!y.ready());
}

#[test]
fn kids_edits_read_the_state_they_change() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile(OWNER, "Main", &hlc(0, 1)).unwrap();
    // A kids profile is always new: an existing one keeps its PIN and kind.
    lib.create_profile("std", "Std", "", &hlc(1, 1)).unwrap();
    lib.set_pin_hash("std", Some("h"), &hlc(2, 1)).unwrap();
    assert!(matches!(
        lib.create_kids_profile("std", "Std", "", &hlc(3, 1)),
        Err(LibraryError::ProfileExists(_))
    ));
    let std = lib.profile("std").unwrap().unwrap();
    assert!(!std.kids() && std.pin_hash() == Some("h"));
    // Made standard twice: the second time changes nothing, so the
    // profile's own addon edits stay.
    lib.set_pin_hash(OWNER, Some("h"), &hlc(1, 1)).unwrap();
    lib.create_kids_profile("kid", "Kid", "", &hlc(4, 1))
        .unwrap();
    assert!(lib.make_standard("kid", &hlc(5, 1)).unwrap());
    let own = mq_library::AddonRow {
        transport_url: "https://own/manifest.json".into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    lib.set_addons("kid", std::slice::from_ref(&own), &hlc(6, 1))
        .unwrap();
    assert!(!lib.make_standard("kid", &hlc(7, 1)).unwrap());
    let kept: Vec<String> = lib
        .addons("kid")
        .unwrap()
        .into_iter()
        .map(|a| a.transport_url)
        .collect();
    assert_eq!(kept, [own.transport_url]);
    assert!(!lib.make_standard("std", &hlc(8, 1)).unwrap());
}

/// make_standard reports what its write did: a fixed stamp at the stored
/// access version is kept out, so nothing changed and no addons are copied.
#[test]
fn make_standard_reports_a_write_its_stamp_kept_out() {
    let lib = Library::temporary().unwrap();
    lib.ensure_profile(OWNER, "Main", &hlc(0, 1)).unwrap();
    let owners = mq_library::AddonRow {
        transport_url: "https://owner/manifest.json".into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    lib.set_addons(OWNER, std::slice::from_ref(&owners), &hlc(1, 1))
        .unwrap();
    lib.set_pin_hash(OWNER, Some("h"), &hlc(1, 1)).unwrap();
    lib.create_kids_profile("kid", "Kid", "", &hlc(4, 1))
        .unwrap();
    assert!(!lib.make_standard("kid", &hlc(4, 1)).unwrap());
    assert!(lib.profile("kid").unwrap().unwrap().kids());
    assert!(lib.addons("kid").unwrap().is_empty(), "no copy");
    assert!(lib.make_standard("kid", &hlc(5, 1)).unwrap());
    assert!(!lib.profile("kid").unwrap().unwrap().kids());
    assert_eq!(lib.addons("kid").unwrap().len(), 1);
}

/// Removing the owner's PIN and creating a kids profile race: each was
/// checked before the other wrote (the runtime's checks both passed), and
/// the write that comes second is refused, in either order.
#[test]
fn kids_and_no_owner_pin_never_stand_together() {
    let owner = || {
        let lib = Library::temporary().unwrap();
        lib.ensure_profile(OWNER, "Main", &hlc(0, 1)).unwrap();
        lib.set_pin_hash(OWNER, Some("h"), &hlc(1, 1)).unwrap();
        lib
    };
    // The kids profile first: the PIN stays.
    let lib = owner();
    lib.create_kids_profile("kid", "Kid", "", &hlc(2, 1))
        .unwrap();
    assert!(matches!(
        lib.clear_owner_pin(&hlc(3, 1)),
        Err(LibraryError::KidsNeedPin)
    ));
    assert_eq!(lib.profile(OWNER).unwrap().unwrap().pin_hash(), Some("h"));
    // The removal first: no kids profile.
    let lib = owner();
    lib.clear_owner_pin(&hlc(2, 1)).unwrap();
    assert!(matches!(
        lib.create_kids_profile("kid", "Kid", "", &hlc(3, 1)),
        Err(LibraryError::KidsNeedPin)
    ));
    assert!(lib.profile("kid").unwrap().is_none());
    // A profile whose access has not arrived may be kids: the PIN stays.
    let lib = owner();
    let arriving = record(coll::PROFILES, "y", json!({"profile": "y", "name": "y"}), 9);
    lib.apply(&[arriving]).unwrap();
    assert!(matches!(
        lib.clear_owner_pin(&hlc(3, 1)),
        Err(LibraryError::KidsNeedPin)
    ));
    // With none left, it goes.
    let lib = owner();
    lib.create_profile("std", "Std", "", &hlc(2, 1)).unwrap();
    lib.clear_owner_pin(&hlc(3, 1)).unwrap();
    assert_eq!(lib.profile(OWNER).unwrap().unwrap().pin_hash(), None);
}

/// Two Allows that both passed the runtime's check at 999: the write that
/// comes second is refused, and a title already on the list is allowed
/// again with its allow time.
#[test]
fn two_allows_at_nine_hundred_ninety_nine_admit_one() {
    let lib = kid(&[]);
    for i in 0..999 {
        let row = entry(&format!("tt{i:04}"), "movie", i);
        lib.write_allowed("kid", &row, &hlc(100 + i as u64, 1))
            .unwrap();
    }
    assert!(
        lib.write_allowed("kid", &entry("tta", "movie", 2000), &hlc(3000, 1))
            .unwrap()
    );
    assert!(matches!(
        lib.write_allowed("kid", &entry("ttb", "movie", 2001), &hlc(3001, 1)),
        Err(LibraryError::ListFull)
    ));
    let list = lib.allowed_list("kid").unwrap();
    assert_eq!((list.counted.len(), list.over_count), (1000, 0));
    assert!(!lib.policy("kid").allows("movie", "ttb"));
    assert!(
        lib.write_allowed("kid", &entry("tta", "movie", 9000), &hlc(3002, 1))
            .unwrap()
    );
    assert_eq!(lib.allowed_list("kid").unwrap().counted[999].added_ms, 2000);
}

/// On a full list, an Allow older than the stored edit of its title loses
/// quietly, as it would on any list; a newer one is refused for the cap.
#[test]
fn an_older_allow_on_a_full_list_loses_quietly() {
    let lib = kid(&[]);
    for i in 0..1000 {
        let row = entry(&format!("tt{i:04}"), "movie", i);
        lib.write_allowed("kid", &row, &hlc(100 + i as u64, 1))
            .unwrap();
    }
    lib.write_allowed("kid", &removal("tt9999"), &hlc(5000, 1))
        .unwrap();
    assert!(
        !lib.write_allowed("kid", &entry("tt9999", "movie", 1), &hlc(4000, 1))
            .unwrap()
    );
    assert!(matches!(
        lib.write_allowed("kid", &entry("tt9999", "movie", 1), &hlc(6000, 1)),
        Err(LibraryError::ListFull)
    ));
}

/// A kids Library holds the counted 1,000, the same ones the policy
/// counts: the newest allow past the cap is not in it.
#[test]
fn the_kids_library_stops_at_the_counted_thousand() {
    let lib = kid(&[]);
    for i in 0..1000 {
        let row = entry(&format!("tt{i:04}"), "movie", i);
        lib.write_allowed("kid", &row, &hlc(100 + i as u64, 1))
            .unwrap();
    }
    let extra = json!({"profile": "kid", "itemId": "tt5000", "kind": "movie",
                       "name": "T", "allowed": true, "addedMs": 5000});
    lib.apply(&[allow_record("kid", extra, 5000)]).unwrap();
    let entries = lib.allowed_entries("kid").unwrap();
    assert_eq!(entries.len(), 1000);
    assert!(entries.iter().all(|e| e.item.item_id != "tt5000"));
    let counted: Vec<String> = lib
        .allowed_list("kid")
        .unwrap()
        .counted
        .into_iter()
        .map(|r| r.item_id)
        .collect();
    let mut shown: Vec<String> = entries.into_iter().map(|e| e.item.item_id).collect();
    shown.sort();
    assert_eq!(shown, counted);
}
