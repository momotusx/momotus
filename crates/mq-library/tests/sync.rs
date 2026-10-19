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
    assert_eq!(
        seen.len(),
        28,
        "25 items, the profile, its lock and its access"
    );
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
    assert_eq!(
        page.records.len(),
        4,
        "the profile, its lock, its access and the item"
    );
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
fn a_local_addon_edit_lands_above_a_newer_pulled_list_and_is_queued() {
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
    // The user made the edit after seeing a list, so it goes in just above R
    // and is queued, and A takes it on its next pull: no divergence.
    let l = hlc(5, 2);
    a.set_addons("p", &[row("https://a/manifest.json")], &hlc(9, 1))
        .unwrap();
    b.apply(&all(&a)).unwrap();
    b.set_addons("p", &[row("https://b/manifest.json")], &l)
        .unwrap();
    let urls = |lib: &Library| -> Vec<String> {
        lib.addons("p")
            .unwrap()
            .into_iter()
            .map(|r| r.transport_url)
            .collect()
    };
    assert_eq!(urls(&b), ["https://b/manifest.json"]);
    assert_eq!(b.outbox(10).unwrap().records.len(), 1);
    a.apply(&all(&b)).unwrap();
    assert_eq!(urls(&a), ["https://b/manifest.json"]);
    assert_eq!(all(&b), all(&a));
}

#[test]
fn a_pull_during_an_addon_edit_lands_after_it_and_keeps_the_other_devices_addon() {
    let a = Library::temporary().unwrap();
    let b = Library::temporary().unwrap();
    let row = |url: &str| AddonRow {
        transport_url: url.into(),
        manifest_json: "{}".into(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    let urls = |lib: &Library| -> Vec<String> {
        lib.addons("p")
            .unwrap()
            .into_iter()
            .map(|r| r.transport_url)
            .collect()
    };
    let (x, y, z) = (
        "https://x/manifest.json",
        "https://y/manifest.json",
        "https://z/manifest.json",
    );
    a.set_addons("p", &[row(x)], &hlc(1, 1)).unwrap();
    b.apply(&all(&a)).unwrap();
    // A adds y; B, whose clock is behind, installs z. A's list reaches B
    // after B's edit read [x] and before it wrote.
    a.set_addons("p", &[row(x), row(y)], &hlc(9, 1)).unwrap();
    let pulled = all(&a);
    std::thread::scope(|s| {
        b.edit_addons("p", &hlc(5, 2), |rows| {
            assert_eq!(*rows, [row(x)]);
            let (done, applied) = std::sync::mpsc::channel();
            let (b, pulled) = (&b, &pulled);
            s.spawn(move || {
                b.apply(pulled).unwrap();
                let _ = done.send(());
            });
            // Time enough for the pull to land if anything let it: the
            // edit's transaction holds it until the write commits.
            let landed = applied.recv_timeout(std::time::Duration::from_millis(250));
            assert!(landed.is_err(), "the pull landed inside the edit");
            rows.push(row(z));
            Ok::<_, mq_library::LibraryError>(())
        })
        .unwrap();
    });
    // The pull applied after the edit, and A's newer list won as a whole:
    // y is kept, never overwritten by an edit of the list before it.
    assert_eq!(urls(&b), [x, y]);
}

#[test]
fn cache_and_import_writes_never_override_a_real_edit() {
    let lib = Library::temporary().unwrap();
    lib.upsert_item("p", &item("tt1"), true, &hlc(50, 7))
        .unwrap();
    // A played title cached at the device's zero hlc, and a Stremio import
    // (device 0) with an older time, both lose to the edit.
    let zero = mq_library::Hlc::zero(9).encode();
    lib.upsert_item("p", &item("tt1"), false, &zero).unwrap();
    lib.upsert_item("p", &item("tt1"), false, &hlc(10, 0))
        .unwrap();
    assert!(lib.is_in_library("p", "tt1").unwrap());
    // A real local edit from a slow clock still goes in above it.
    lib.upsert_item("p", &item("tt1"), false, &hlc(10, 9))
        .unwrap();
    assert!(!lib.is_in_library("p", "tt1").unwrap());
}

/// The roster store takes the same entry twice but never a second entry at
/// a taken seq: a conflicting history must not slip by unnoticed.
#[test]
fn a_roster_entry_at_a_taken_seq_is_refused_unless_it_is_the_same() {
    let lib = Library::temporary().unwrap();
    lib.add_roster_entry("g", 0, b"one", b"sig").unwrap();
    lib.add_roster_entry("g", 0, b"one", b"sig").unwrap();
    assert!(lib.add_roster_entry("g", 0, b"two", b"sig").is_err());
    assert_eq!(
        lib.roster_entries("g").unwrap(),
        vec![(b"one".to_vec(), b"sig".to_vec())]
    );
}

/// A dropped device forgets its queue, cursors and roster, but keeps its own
/// id and the hosts relay ids were pinned to.
#[test]
fn forgetting_sync_keeps_the_device_id_and_relay_pins() {
    let lib = Library::temporary().unwrap();
    for key in ["device_id", "relay:07", "pull_seq", "approval:01"] {
        lib.set_sync_state(key, "x").unwrap();
    }
    lib.add_roster_entry("g", 0, b"one", b"sig").unwrap();
    lib.forget_sync().unwrap();
    let kept: Vec<String> = ["device_id", "relay:07", "pull_seq", "approval:01"]
        .into_iter()
        .filter(|key| lib.sync_state(key).unwrap().is_some())
        .map(str::to_owned)
        .collect();
    assert_eq!(kept, ["device_id", "relay:07"]);
    assert!(lib.roster_entries("g").unwrap().is_empty());
}

#[test]
fn moving_a_sync_state_replaces_it_in_one_step() {
    let lib = Library::temporary().unwrap();
    lib.set_sync_state("approval:01", "5:6").unwrap();
    lib.move_sync_state("approval:01", "cancel:01", "1")
        .unwrap();
    assert_eq!(lib.sync_state("approval:01").unwrap(), None);
    assert_eq!(lib.sync_state("cancel:01").unwrap().as_deref(), Some("1"));
}

/// A device clock whose every tick falls in one millisecond, far ahead of
/// the wall time: only the counter orders the writes, as for writes made in
/// the same millisecond.
fn one_millisecond_clock() -> mq_library::HlcClock {
    let clock = mq_library::HlcClock::new(7);
    let ahead = mq_library::Hlc {
        wall_ms: 5_000_000_000_000,
        counter: 0,
        device: 9,
    };
    clock.observe(ahead, 0);
    clock
}

/// 16 threads make 40 writes each to one register, all waiting for the
/// writer at once, stamped by one device clock. Every write must change the
/// register: each queues one outbox entry, so the outbox holds 640. Stamped
/// before it held the writer, a write could commit after a newer one, be
/// stamped again just above it, onto the hlc a third write already held,
/// and that third write was lost as no newer than the register (C31).
fn all_concurrent_writes_land(
    write: impl Fn(&Library, &mq_library::HlcClock, usize) + Sync,
) -> Library {
    let lib = Library::temporary().unwrap();
    let clock = one_millisecond_clock();
    std::thread::scope(|s| {
        for t in 0..16 {
            let (lib, clock, write) = (&lib, &clock, &write);
            s.spawn(move || {
                for i in 0..40 {
                    write(lib, clock, t * 40 + i);
                }
            });
        }
    });
    let outbox = lib.outbox(usize::MAX).unwrap();
    assert_eq!(outbox.last_id, Some(640), "writes were lost");
    lib
}

#[test]
fn concurrent_addon_edits_stamped_in_commit_order_all_land() {
    let lib = all_concurrent_writes_land(|lib, clock, n| {
        lib.edit_addons("p", clock, |rows: &mut Vec<AddonRow>| {
            rows.push(AddonRow {
                transport_url: format!("https://a{n}.example/manifest.json"),
                manifest_json: "{}".into(),
                position: 0,
                enabled: true,
                allow_private: false,
            });
            Ok::<_, mq_library::LibraryError>(())
        })
        .unwrap();
    });
    assert_eq!(lib.addons("p").unwrap().len(), 640);
}

#[test]
fn concurrent_item_writes_all_land() {
    all_concurrent_writes_land(|lib, clock, n| {
        let mut row = item("tt1");
        row.name = format!("name {n}");
        lib.upsert_item("p", &row, n % 2 == 0, clock).unwrap();
    });
}

#[test]
fn concurrent_progress_writes_all_land() {
    all_concurrent_writes_land(|lib, clock, n| {
        let row = ProgressRow {
            video_id: "tt1",
            item_id: "tt1",
            position_ms: n as i64,
            duration_ms: 10_000,
            updated_ms: n as i64,
        };
        lib.set_progress("p", row, clock).unwrap();
    });
}

#[test]
fn concurrent_watched_marks_all_land() {
    all_concurrent_writes_land(|lib, clock, n| {
        lib.set_watched("p", "tt1", "tt1", n % 2 == 0, clock)
            .unwrap();
    });
}

#[test]
fn concurrent_stream_memory_and_settings_writes_all_land() {
    all_concurrent_writes_land(|lib, clock, n| {
        let stream = format!("{{\"key\":\"{n}\"}}");
        lib.set_stream_memory("p", "tt1", Some("g"), &stream, clock)
            .unwrap();
    });
    all_concurrent_writes_land(|lib, clock, n| {
        lib.set_setting("p", "notes", &format!("\"{n}\""), clock)
            .unwrap();
    });
}

#[test]
fn concurrent_profile_and_lock_writes_all_land() {
    all_concurrent_writes_land(|lib, clock, n| {
        lib.update_profile("p", &format!("name {n}"), "", 0, clock)
            .unwrap();
    });
    all_concurrent_writes_land(|lib, clock, n| {
        let hash = format!("$argon2id$v=19$m=1,t=1,p=1$c2FsdA${n}");
        lib.set_pin_hash("p", Some(&hash), clock).unwrap();
    });
}

/// A local write that goes in again just above a newer stored version moves
/// the device clock past what it wrote, so the next local write is stamped
/// above it.
#[test]
fn a_local_write_above_a_newer_version_moves_the_clock_past_it() {
    let lib = Library::temporary().unwrap();
    // Another device, its clock an hour ahead, set the register first.
    let ahead = mq_library::hlc::now_ms() + 3_600_000;
    lib.set_watched("p", "tt1", "tt1", true, &hlc(ahead, 9))
        .unwrap();
    let clock = mq_library::HlcClock::new(7);
    lib.set_watched("p", "tt1", "tt1", false, &clock).unwrap();
    let stored = lib.register(coll::WATCHED, "p\u{1f}tt1").unwrap().unwrap();
    let stored = mq_library::Hlc::decode(&stored.hlc).unwrap();
    assert_eq!((stored.wall_ms, stored.device), (ahead, 7), "{stored:?}");
    let next = clock.tick(clock.wall_now());
    assert!(next > stored, "{next:?} is not after {stored:?}");
}

/// Manifest freshness is not list precedence (plans/m13-default-addons.md 5):
/// a refresh keeps the list's stamp and pushes nothing, and an edit that
/// changes nothing is no edit.
mod freshness {
    use super::hlc;
    use mq_library::{AddonRow, Hlc, HlcClock, Library};

    fn row(url: &str, manifest: &str) -> AddonRow {
        AddonRow {
            transport_url: url.into(),
            manifest_json: manifest.into(),
            position: 0,
            enabled: true,
            allow_private: false,
        }
    }

    #[test]
    fn a_manifest_refresh_keeps_the_lists_stamp_and_pushes_nothing() {
        let lib = Library::temporary().unwrap();
        let zero = Hlc::zero(4).encode();
        lib.set_addons("p", &[row("https://a/manifest.json", "{}")], &zero)
            .unwrap();
        let queued = lib.outbox(100).unwrap().records.len();
        let n = lib
            .refresh_addon_manifests(
                "p",
                &[("https://a/manifest.json".into(), "{\"v\":2}".into())],
            )
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(lib.addons("p").unwrap()[0].manifest_json, "{\"v\":2}");
        assert_eq!(
            lib.addon_list_hlc("p").unwrap().unwrap(),
            zero,
            "untouched stays untouched"
        );
        assert_eq!(
            lib.outbox(100).unwrap().records.len(),
            queued,
            "nothing to push"
        );
        // The same manifest again changes nothing.
        let n = lib
            .refresh_addon_manifests(
                "p",
                &[("https://a/manifest.json".into(), "{\"v\":2}".into())],
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn an_edit_that_changes_nothing_keeps_the_stamp_and_queues_nothing() {
        let lib = Library::temporary().unwrap();
        let clock = HlcClock::new(4);
        let first = hlc(1_000, 2);
        lib.set_addons("p", &[row("https://a/manifest.json", "{}")], &first)
            .unwrap();
        let queued = lib.outbox(100).unwrap().records.len();
        lib.edit_addons("p", &clock, |rows| {
            rows[0].enabled = true;
            Ok::<(), mq_library::LibraryError>(())
        })
        .unwrap();
        assert_eq!(lib.addon_list_hlc("p").unwrap().unwrap(), first);
        assert_eq!(lib.outbox(100).unwrap().records.len(), queued);
        // A real change is an edit.
        lib.edit_addons("p", &clock, |rows| {
            rows[0].enabled = false;
            Ok::<(), mq_library::LibraryError>(())
        })
        .unwrap();
        assert_ne!(lib.addon_list_hlc("p").unwrap().unwrap(), first);
    }
}
