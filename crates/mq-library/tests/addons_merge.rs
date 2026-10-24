//! A profile's addon list merges per addon (plans/accounts-social-sync.md
//! 13): each addon is a register of its own, removals are kept, and an edit
//! stamps only the addons it changed.

use mq_library::{AddonRow, Library, SyncCursor, SyncRecord, coll};
use serde_json::json;

fn hlc(wall: u64, device: u64) -> String {
    mq_library::Hlc {
        wall_ms: wall,
        counter: 0,
        device,
    }
    .encode()
}

fn row(url: &str) -> AddonRow {
    AddonRow {
        transport_url: url.into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    }
}

const X: &str = "https://x/manifest.json";
const Y: &str = "https://y/manifest.json";
const Z: &str = "https://z/manifest.json";
const W: &str = "https://w/manifest.json";

/// An addon's register key half: the hex SHA-256 of its link.
fn hash(url: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(url.as_bytes()))
}

fn urls(lib: &Library) -> Vec<String> {
    lib.addons("p")
        .unwrap()
        .into_iter()
        .map(|a| a.transport_url)
        .collect()
}

fn all(lib: &Library) -> Vec<SyncRecord> {
    lib.changed_since(&SyncCursor::default(), 100_000).unwrap()
}

/// Both devices take everything the other has.
fn exchange(a: &Library, b: &Library) {
    let (from_a, from_b) = (all(a), all(b));
    a.apply(&from_b).unwrap();
    b.apply(&from_a).unwrap();
}

fn add(lib: &Library, url: &str, at: &str) {
    lib.edit_addons("p", at, |rows: &mut Vec<AddonRow>| {
        rows.push(row(url));
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
}

fn remove(lib: &Library, url: &str, at: &str) {
    lib.edit_addons("p", at, |rows: &mut Vec<AddonRow>| {
        rows.retain(|r| r.transport_url != url);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
}

/// The owner's ask: an addon added on one device is never lost to an addon
/// added on another before either synced.
#[test]
fn two_devices_adding_different_addons_offline_keep_both() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    exchange(&a, &b);
    add(&a, Y, &hlc(5, 1));
    add(&b, Z, &hlc(6, 2));
    exchange(&a, &b);
    assert_eq!(
        urls(&a),
        [X, Y, Z],
        "both kept, after the one both had, the two new ones by link"
    );
    assert_eq!(urls(&b), urls(&a));
    assert_eq!(all(&a), all(&b));
}

#[test]
fn a_newer_removal_wins_over_an_older_change_and_a_newer_add_installs_again() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    add(&a, Y, &hlc(2, 1));
    exchange(&a, &b);
    // A switches X off, B removes it later: removed on both.
    a.edit_addons("p", &hlc(4, 1), |rows: &mut Vec<AddonRow>| {
        rows[0].enabled = false;
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    remove(&b, X, &hlc(5, 2));
    exchange(&a, &b);
    assert_eq!(urls(&a), [Y]);
    assert_eq!(urls(&b), [Y]);
    // A adds it again after the removal: installed on both.
    add(&a, X, &hlc(9, 1));
    exchange(&a, &b);
    assert_eq!(urls(&a), [Y, X]);
    assert_eq!(urls(&b), [Y, X]);
}

#[test]
fn a_switch_on_one_device_and_an_add_on_another_both_stay() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    exchange(&a, &b);
    a.edit_addons("p", &hlc(5, 1), |rows: &mut Vec<AddonRow>| {
        rows[0].enabled = false;
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    add(&b, Y, &hlc(6, 2));
    exchange(&a, &b);
    for lib in [&a, &b] {
        let addons = lib.addons("p").unwrap();
        assert_eq!(urls(lib), [X, Y]);
        assert!(!addons[0].enabled, "the switch stays");
    }
}

/// A move stamps only the addon that moved, so another device's edits to
/// the others stay.
#[test]
fn a_move_stamps_only_the_moved_addon() {
    let lib = Library::temporary().unwrap();
    for (i, url) in [X, Y, Z, W].into_iter().enumerate() {
        add(&lib, url, &hlc(i as u64 + 1, 1));
    }
    let queued = lib.outbox(1000).unwrap();
    lib.outbox_done(queued.last_id.unwrap()).unwrap();
    lib.edit_addons("p", &hlc(9, 1), |rows: &mut Vec<AddonRow>| {
        let w = rows.pop().unwrap();
        rows.insert(0, w);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    assert_eq!(urls(&lib), [W, X, Y, Z]);
    let moved: Vec<(u8, String)> = lib
        .outbox(1000)
        .unwrap()
        .records
        .into_iter()
        .map(|(_, r)| (r.coll, r.key))
        .collect();
    assert_eq!(
        moved,
        [(coll::ADDON_ORDER, format!("p\u{1f}{}", hash(W)))],
        "one place, no state"
    );
    // An edit that changes nothing stamps nothing.
    lib.edit_addons("p", &hlc(10, 1), |_: &mut Vec<AddonRow>| {
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    assert_eq!(lib.outbox(1000).unwrap().records.len(), 1);
}

/// A whole list from an older build is skipped, and none is written.
#[test]
fn an_older_builds_list_is_skipped() {
    let lib = Library::temporary().unwrap();
    add(&lib, X, &hlc(5, 1));
    let list = SyncRecord {
        coll: coll::ADDONS,
        key: "p".into(),
        value: json!({ "profile": "p", "addons": [{ "url": Y, "manifest": "{}", "enabled": true, "allowPrivate": false }] }),
        hlc: hlc(9, 2),
    };
    // In a page with another record: skipped, and the page goes on.
    let other = Library::temporary().unwrap();
    add(&other, Z, &hlc(8, 3));
    let mut page = vec![list];
    page.extend(all(&other));
    assert_eq!(
        lib.apply(&page).unwrap(),
        2,
        "Z's state and place, the list skipped"
    );
    assert_eq!(urls(&lib), [X, Z]);
    assert!(all(&lib).iter().all(|r| r.coll != coll::ADDONS));
}

/// A move on one device and a switch of the same addon on another both
/// stand: place and state are registers of their own.
#[test]
fn a_move_and_a_switch_of_the_same_addon_both_stand() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    add(&a, Y, &hlc(2, 1));
    exchange(&a, &b);
    a.edit_addons("p", &hlc(5, 1), |rows: &mut Vec<AddonRow>| {
        let y = rows.pop().unwrap();
        rows.insert(0, y);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    b.edit_addons("p", &hlc(6, 2), |rows: &mut Vec<AddonRow>| {
        rows[1].enabled = false;
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    exchange(&a, &b);
    for lib in [&a, &b] {
        assert_eq!(urls(lib), [Y, X], "the move stands");
        assert!(!lib.addons("p").unwrap()[0].enabled, "the switch stands");
    }
}

/// The same records applied in any order give the same library, and twice
/// change nothing.
#[test]
fn any_order_of_the_same_records_gives_the_same_list() {
    let (a, b, c) = (
        Library::temporary().unwrap(),
        Library::temporary().unwrap(),
        Library::temporary().unwrap(),
    );
    add(&a, X, &hlc(1, 1));
    exchange(&a, &b);
    exchange(&a, &c);
    add(&a, Y, &hlc(3, 1));
    remove(&b, X, &hlc(4, 2));
    add(&c, Z, &hlc(2, 3));
    c.edit_addons("p", &hlc(5, 3), |rows: &mut Vec<AddonRow>| {
        let z = rows.pop().unwrap();
        rows.insert(0, z);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    let sets = [all(&a), all(&b), all(&c)];
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut results = Vec::new();
    for order in orders {
        let lib = Library::temporary().unwrap();
        for i in order {
            lib.apply(&sets[i]).unwrap();
        }
        for i in order {
            assert_eq!(
                lib.apply(&sets[i]).unwrap(),
                0,
                "applied twice changes nothing"
            );
        }
        results.push((urls(&lib), all(&lib)));
    }
    assert!(results.iter().all(|r| *r == results[0]), "{results:?}");
    assert_eq!(results[0].0, [Z, Y]);
}

/// A removal keeps neither the link, which can carry a key, nor the
/// manifest: its registers are keyed by the link's hash.
#[test]
fn a_removal_keeps_neither_link_nor_manifest() {
    let lib = Library::temporary().unwrap();
    let keyed = "https://addon.example/realdebrid=SECRETKEY123/manifest.json";
    lib.edit_addons("p", &hlc(1, 1), |rows: &mut Vec<AddonRow>| {
        rows.push(AddonRow {
            manifest_json: "{\"id\":\"x\"}".into(),
            ..row(keyed)
        });
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    // Its cached answers are keyed by request links that carry its key.
    let cached = "https://addon.example/realdebrid=SECRETKEY123/catalog/movie/top.json";
    let other = "https://other.example/catalog/movie/top.json";
    for link in [cached, other] {
        lib.cache_put(
            link,
            &mq_library::CachedResponse {
                body: b"{}".to_vec(),
                stored_ms: 1,
                max_age_s: 60,
                stale_revalidate_s: 0,
                stale_error_s: 0,
            },
            lib.removals(),
        )
        .unwrap();
    }
    remove(&lib, keyed, &hlc(2, 1));
    assert!(
        lib.cache_get(cached).unwrap().is_none(),
        "the removed addon's answers go"
    );
    assert!(
        lib.cache_get(other).unwrap().is_some(),
        "other addons' answers stay"
    );
    let records = all(&lib);
    let text = serde_json::to_string(&records).unwrap();
    assert!(!text.contains("SECRETKEY123"), "{text}");
    let x = records.iter().find(|r| r.coll == coll::ADDON).unwrap();
    assert_eq!(x.key, format!("p\u{1f}{}", hash(keyed)));
    assert_eq!(x.value["installed"], json!(false));
    assert!(
        x.value.get("url").is_none() && x.value.get("manifest").is_none(),
        "{x:?}"
    );
    // Added again later: the same register, installed with its link.
    add(&lib, keyed, &hlc(3, 1));
    assert_eq!(urls(&lib), [keyed]);
}

/// Imported addons go before the list: places below the first, and no
/// other addon's place is written.
#[test]
fn addons_put_first_take_places_below_the_first_and_move_nothing_else() {
    let lib = Library::temporary().unwrap();
    add(&lib, X, &hlc(1, 1));
    add(&lib, Y, &hlc(2, 1));
    let done = lib.outbox(1000).unwrap().last_id.unwrap();
    lib.outbox_done(done).unwrap();
    lib.edit_addons("p", &hlc(3, 1), |rows: &mut Vec<AddonRow>| {
        rows.splice(0..0, [row(Z), row(W)]);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    assert_eq!(urls(&lib), [Z, W, X, Y]);
    let written: Vec<String> = lib
        .outbox(1000)
        .unwrap()
        .records
        .into_iter()
        .map(|(_, r)| r.key)
        .collect();
    for url in [X, Y] {
        assert!(
            !written.contains(&format!("p\u{1f}{}", hash(url))),
            "{url} was written"
        );
    }
}

/// Many moves into one gap never place the whole list again: only the
/// neighbourhood that ran out of room gets new places, and the list shows
/// the order the edits made.
#[test]
fn moves_into_one_gap_place_only_the_neighbourhood_again() {
    let lib = Library::temporary().unwrap();
    let links: Vec<String> = (0..12)
        .map(|i| format!("https://a{i:02}.example/manifest.json"))
        .collect();
    lib.set_addons(
        "p",
        &links.iter().map(|u| row(u)).collect::<Vec<_>>(),
        &hlc(1, 1),
    )
    .unwrap();
    let place = |url: &str| -> SyncRecord {
        all(&lib)
            .into_iter()
            .find(|r| r.coll == coll::ADDON_ORDER && r.key == format!("p\u{1f}{}", hash(url)))
            .unwrap()
    };
    // The last addon moved to the top first: a place below the others, so
    // placing the whole list again would change it.
    lib.edit_addons("p", &hlc(5, 1), |rows: &mut Vec<AddonRow>| {
        let moved = rows.pop().unwrap();
        rows.insert(0, moved);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    let mut links = links;
    let top = links.pop().unwrap();
    links.insert(0, top);
    let (first, last) = (place(&links[0]), place(&links[11]));
    // Forty moves, each putting the list's last addon right after the fifth.
    for n in 0..40u64 {
        lib.edit_addons("p", &hlc(10 + n, 1), |rows: &mut Vec<AddonRow>| {
            let moved = rows.pop().unwrap();
            rows.insert(5, moved);
            Ok::<_, mq_library::LibraryError>(())
        })
        .unwrap();
    }
    let mut want: Vec<String> = links.clone();
    for _ in 0..40 {
        let moved = want.pop().unwrap();
        want.insert(5, moved);
    }
    assert_eq!(urls(&lib), want);
    assert_eq!(
        place(&links[0]),
        first,
        "the first addon's place never moved"
    );
    let _ = last;
}

/// Upkeep that moves a default to its new link is stamped just above the
/// old link's own register, so a person's removal of the new link on
/// another device, made after that, wins.
#[test]
fn a_moved_default_link_loses_to_a_later_removal_elsewhere() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    exchange(&a, &b);
    // A moves X to Y and its person then removes Y at 5.
    assert!(a.rename_addon("p", X, Y, "{}", 1).unwrap());
    remove(&a, Y, &hlc(5, 1));
    // B, which had not synced, runs the same move later in real time.
    assert!(b.rename_addon("p", X, Y, "{}", 2).unwrap());
    assert_eq!(urls(&b), [Y]);
    exchange(&a, &b);
    assert!(
        urls(&a).is_empty() && urls(&b).is_empty(),
        "the removal stands"
    );
}

#[test]
fn the_list_has_a_time_once_any_addon_did_and_keeps_it_after_the_last_goes() {
    let lib = Library::temporary().unwrap();
    assert_eq!(lib.addon_list_hlc("p").unwrap(), None);
    add(&lib, X, &hlc(1, 1));
    remove(&lib, X, &hlc(2, 1));
    assert!(urls(&lib).is_empty());
    assert_eq!(lib.addon_list_hlc("p").unwrap(), Some(hlc(2, 1)));
}

/// A device that joins an account takes the account's list: what it only
/// had as untouched recommended defaults (the defaults step's wall time 0)
/// is dropped and never added; what a person did on it before joining
/// merges (plans/accounts-social-sync.md 13).
#[test]
fn joining_takes_the_accounts_list_and_merges_a_persons_edits() {
    let v = "https://v.example/manifest.json";
    // The account: Y added and later removed, Z added.
    let a = Library::temporary().unwrap();
    add(&a, Y, &hlc(2, 1));
    remove(&a, Y, &hlc(4, 1));
    add(&a, Z, &hlc(5, 1));
    // The joining device answered the defaults step (X, Y, V untouched),
    // then its person added W and switched X off.
    let c = Library::temporary().unwrap();
    c.set_addons("p", &[row(X), row(Y), row(v)], &hlc(0, 3))
        .unwrap();
    add(&c, W, &hlc(6, 3));
    c.edit_addons("p", &hlc(7, 3), |rows: &mut Vec<AddonRow>| {
        rows[0].enabled = false;
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    // The join: the first pull, then the settle, before anything is sent.
    c.mark_join_defaults(3).unwrap();
    c.apply(&all(&a)).unwrap();
    c.settle_join_defaults(3).unwrap();
    assert!(!c.join_defaults_pending().unwrap());
    // Nothing of the dropped defaults is sent (what goes out for Y is the
    // account's own removal, back to it); the switched one keeps its place.
    let sent: Vec<SyncRecord> = c
        .outbox(1000)
        .unwrap()
        .records
        .into_iter()
        .map(|(_, r)| r)
        .collect();
    assert!(
        sent.iter()
            .all(|r| r.coll != coll::ADDON || !r.hlc.starts_with("000000000000")),
        "a dropped default is sent: {sent:?}"
    );
    assert!(
        sent.iter().all(|r| !r.key.ends_with(&hash(v))),
        "the untouched V is sent"
    );
    assert!(
        sent.iter()
            .any(|r| r.coll == coll::ADDON_ORDER && r.key == format!("p\u{1f}{}", hash(X))),
        "X's place stays"
    );
    exchange(&a, &c);
    for lib in [&a, &c] {
        let list = lib.addons("p").unwrap();
        let links: Vec<&str> = list.iter().map(|r| r.transport_url.as_str()).collect();
        assert!(
            links.contains(&X) && links.contains(&Z) && links.contains(&W),
            "{links:?}"
        );
        assert!(!links.contains(&Y), "the account's removal stands");
        assert!(!links.contains(&v), "an untouched default is not added");
        assert!(
            !list.iter().find(|r| r.transport_url == X).unwrap().enabled,
            "the switch merges"
        );
    }
    assert_eq!(urls(&a), urls(&c));
}

/// An account with no addon register: the joining device keeps its
/// defaults, so a join never ends with no addons, and they become the
/// account's.
#[test]
fn joining_an_account_with_no_addons_keeps_the_defaults() {
    let a = Library::temporary().unwrap();
    let c = Library::temporary().unwrap();
    c.set_addons("p", &[row(X), row(Y)], &hlc(0, 3)).unwrap();
    c.mark_join_defaults(3).unwrap();
    c.apply(&all(&a)).unwrap();
    c.settle_join_defaults(3).unwrap();
    assert_eq!(urls(&c), [X, Y]);
    exchange(&a, &c);
    assert_eq!(urls(&a), [X, Y]);
}

/// The account's untouched defaults win on the joiner's first pull over
/// the joiner's own untouched copy of the same addon, whichever device id
/// is higher, so the settle never takes the account's away (code review A
/// P1-1).
#[test]
fn a_join_keeps_the_accounts_untouched_defaults_whatever_the_device_ids() {
    for (creator, real) in [(1u64, true), (9, true), (1, false), (9, false)] {
        // The account: X and Y untouched from its creator, and maybe one real add.
        let a = Library::temporary().unwrap();
        a.set_addons("p", &[row(X), row(Y)], &hlc(0, creator))
            .unwrap();
        if real {
            add(&a, Z, &hlc(5, creator));
        }
        // The joiner (device 3) kept the same defaults untouched.
        let c = Library::temporary().unwrap();
        c.set_addons("p", &[row(X), row(Y)], &hlc(0, 3)).unwrap();
        c.mark_join_defaults(3).unwrap();
        c.apply(&all(&a)).unwrap();
        c.settle_join_defaults(3).unwrap();
        assert_eq!(urls(&c), urls(&a), "creator {creator}, real {real}");
        let want: &[&str] = if real { &[X, Y, Z] } else { &[X, Y] };
        assert_eq!(urls(&c), want, "creator {creator}, real {real}");
    }
}

/// A move writes only places, and still tells the addons views to read
/// again, here and on a device that pulls it (code review B P1-1).
#[test]
fn a_move_alone_is_announced_as_an_addons_change() {
    let (a, b) = (Library::temporary().unwrap(), Library::temporary().unwrap());
    add(&a, X, &hlc(1, 1));
    add(&a, Y, &hlc(2, 1));
    exchange(&a, &b);
    let mut here = a.subscribe();
    a.edit_addons("p", &hlc(5, 1), |rows: &mut Vec<AddonRow>| {
        let y = rows.pop().unwrap();
        rows.insert(0, y);
        Ok::<_, mq_library::LibraryError>(())
    })
    .unwrap();
    assert!(
        here.try_recv()
            .unwrap()
            .contains(&mq_library::Table::Addons)
    );
    let moved: Vec<SyncRecord> = all(&a)
        .into_iter()
        .filter(|r| r.coll == coll::ADDON_ORDER)
        .collect();
    let mut there = b.subscribe();
    b.apply(&moved).unwrap();
    assert!(
        there
            .try_recv()
            .unwrap()
            .contains(&mq_library::Table::Addons)
    );
    assert_eq!(urls(&b), [Y, X]);
}

/// Defaults a join leaves in place get their own time back, an onboarding
/// choice's counter with it: after a join into an account with no addons,
/// and after leaving before the first sync (code review B r2 items 1, 2).
#[test]
fn defaults_a_join_leaves_in_place_keep_their_own_time() {
    let chose = mq_library::Hlc {
        wall_ms: 0,
        counter: 1,
        device: 3,
    }
    .encode();
    let times = |lib: &Library| -> Vec<String> {
        all(lib)
            .into_iter()
            .filter(|r| r.coll == coll::ADDON || r.coll == coll::ADDON_ORDER)
            .map(|r| r.hlc)
            .collect()
    };
    // A kept join: the account holds no addons.
    let c = Library::temporary().unwrap();
    c.set_addons("p", &[row(X), row(Y)], &chose).unwrap();
    c.mark_join_defaults(3).unwrap();
    c.apply(&all(&Library::temporary().unwrap())).unwrap();
    c.settle_join_defaults(3).unwrap();
    assert!(times(&c).iter().all(|t| *t == chose), "{:?}", times(&c));
    // Left before the first sync.
    let d = Library::temporary().unwrap();
    d.set_addons("p", &[row(X), row(Y)], &chose).unwrap();
    d.mark_join_defaults(3).unwrap();
    d.forget_sync().unwrap();
    assert!(!d.join_defaults_pending().unwrap());
    assert!(times(&d).iter().all(|t| *t == chose), "{:?}", times(&d));
}
