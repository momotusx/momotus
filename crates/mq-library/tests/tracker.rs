use mq_library::{
    Hlc, ImportedPlay, IntentOp, Library, LibraryItemRow, NewIntent, ProgressRow, TrackerKey,
};

const NOW: i64 = 1_800_000_000_000;

fn key(account: &str) -> TrackerKey<'_> {
    TrackerKey {
        profile: "p",
        tracker: "trakt",
        account,
    }
}

fn play(video: &str, kind: &str, at: i64) -> ImportedPlay {
    let item = video.split(':').next().unwrap();
    ImportedPlay {
        item_id: item.into(),
        video_id: video.into(),
        kind: kind.into(),
        name: format!("Title {item}"),
        watched_ms: at,
    }
}

fn hlc(wall: u64, device: u64) -> String {
    Hlc {
        wall_ms: wall,
        counter: 0,
        device,
    }
    .encode()
}

fn watched(lib: &Library, item: &str) -> Vec<String> {
    lib.watched_videos("p", item).unwrap()
}

#[test]
fn an_older_trakt_play_never_beats_a_local_unmark() {
    let lib = Library::temporary().unwrap();
    // The user unmarked here after the play Trakt still has.
    lib.set_watched("p", "tt1", "tt1:1:1", false, &hlc(2_000, 5))
        .unwrap();
    let report = lib
        .import_plays("p", &[play("tt1:1:1", "series", 1_000)], NOW, &[])
        .unwrap();
    assert_eq!(report.applied, 0);
    assert!(watched(&lib, "tt1").is_empty());
    // A newer play does win.
    lib.import_plays("p", &[play("tt1:1:1", "series", 3_000)], NOW, &[])
        .unwrap();
    assert_eq!(watched(&lib, "tt1"), ["tt1:1:1"]);
}

#[test]
fn two_devices_importing_the_same_plays_write_the_same_registers() {
    let plays = [
        play("tt1:1:1", "series", 1_000),
        play("tt1:1:2", "series", 2_000),
        play("tt9", "movie", 1_500),
    ];
    let pages = |lib: &Library| {
        lib.import_plays("p", &plays, NOW, &[]).unwrap();
        lib.outbox(100)
            .unwrap()
            .records
            .into_iter()
            .map(|(_, r)| (r.coll, r.key, r.value.to_string(), r.hlc))
            .collect::<Vec<_>>()
    };
    let a = Library::temporary().unwrap();
    let b = Library::temporary().unwrap();
    let (mut ra, mut rb) = (pages(&a), pages(&b));
    ra.sort();
    rb.sort();
    assert!(!ra.is_empty());
    assert_eq!(ra, rb);
}

#[test]
fn an_imported_series_shows_next_up_and_a_movie_does_not() {
    let lib = Library::temporary().unwrap();
    lib.import_plays(
        "p",
        &[
            play("tt1:1:1", "series", 1_000),
            play("tt1:1:2", "series", 2_000),
            play("tt9", "movie", 3_000),
        ],
        NOW,
        &[],
    )
    .unwrap();
    let done = lib.finished_series("p", 10).unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].video_id, "tt1:1:2");
    assert_eq!(done[0].kind, "series");
    assert_eq!(done[0].name, "Title tt1");
    assert!(lib.continue_watching("p", 10).unwrap().is_empty());
}

#[test]
fn an_anchor_never_hides_later_progress_of_the_title() {
    let lib = Library::temporary().unwrap();
    let show = LibraryItemRow {
        item_id: "tt1".into(),
        kind: "series".into(),
        name: "Show".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    lib.upsert_item("p", &show, true, &hlc(10, 5)).unwrap();
    let row = ProgressRow {
        video_id: "tt1:1:5",
        item_id: "tt1",
        position_ms: 400,
        duration_ms: 1000,
        updated_ms: 5_000,
    };
    lib.set_progress("p", row, &hlc(5_000, 5)).unwrap();
    lib.import_plays("p", &[play("tt1:1:3", "series", 3_000)], NOW, &[])
        .unwrap();
    let cw = lib.continue_watching("p", 10).unwrap();
    assert_eq!(cw.len(), 1);
    assert_eq!(cw[0].video_id, "tt1:1:5");
    assert_eq!(
        lib.progress("p", "tt1:1:3").unwrap(),
        None,
        "no anchor written"
    );
}

#[test]
fn future_plays_are_deferred() {
    let lib = Library::temporary().unwrap();
    let report = lib
        .import_plays("p", &[play("tt1:1:1", "series", NOW + 3_600_000)], NOW, &[])
        .unwrap();
    assert_eq!(report.deferred, ["tt1:1:1"]);
    assert!(watched(&lib, "tt1").is_empty());
}

#[test]
fn a_held_write_waits_for_its_session() {
    let lib = Library::temporary().unwrap();
    let k = key("acct");
    let intent = |session| NewIntent {
        key: k,
        op: IntentOp::Add,
        watched_ms: 100,
        session,
    };
    lib.put_tracker_intent("tt1", "tt1:1:1", intent(Some("s1")))
        .unwrap();
    lib.put_tracker_intent("tt2", "tt2", intent(Some("s2")))
        .unwrap();
    assert!(lib.ready_tracker_intents(k, 10).unwrap().is_empty());
    // s1's stop reached Trakt as a play: its write is dropped.
    lib.end_tracker_session("s1", true).unwrap();
    // s2's process died: at start its write is released.
    assert_eq!(lib.release_orphan_tracker_intents(&[]).unwrap(), 1);
    let ready = lib.ready_tracker_intents(k, 10).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].video_id, "tt2");
}

#[test]
fn an_old_acknowledgement_keeps_a_newer_write() {
    let lib = Library::temporary().unwrap();
    let k = key("acct");
    let rev1 = lib
        .put_tracker_intent(
            "tt1",
            "tt1:1:1",
            NewIntent {
                key: k,
                op: IntentOp::Add,
                watched_ms: 100,
                session: None,
            },
        )
        .unwrap();
    // The user unmarks while the add is in flight.
    let rev2 = lib
        .put_tracker_intent(
            "tt1",
            "tt1:1:1",
            NewIntent {
                key: k,
                op: IntentOp::Remove,
                watched_ms: 200,
                session: None,
            },
        )
        .unwrap();
    assert!(rev2 > rev1);
    assert!(!lib.ack_tracker_intent(k, "tt1:1:1", rev1).unwrap());
    let ready = lib.ready_tracker_intents(k, 10).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!((ready[0].op, ready[0].rev), (IntentOp::Remove, rev2));
    assert!(lib.ack_tracker_intent(k, "tt1:1:1", rev2).unwrap());
    assert!(lib.ready_tracker_intents(k, 10).unwrap().is_empty());
}

#[test]
fn a_hand_mark_queues_a_write_only_when_it_flips() {
    let lib = Library::temporary().unwrap();
    let k = key("acct");
    let flipped = lib
        .set_watched_by_hand("p", "tt1", "tt1:1:1", true, &hlc(10, 5), Some((k, 10)))
        .unwrap();
    assert!(flipped);
    // Marking it again changes nothing and queues nothing new.
    let again = lib
        .set_watched_by_hand("p", "tt1", "tt1:1:1", true, &hlc(20, 5), Some((k, 20)))
        .unwrap();
    assert!(!again);
    let ready = lib.ready_tracker_intents(k, 10).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(
        (ready[0].op, ready[0].watched_ms, ready[0].rev),
        (IntentOp::Add, 10, 1)
    );
}

#[test]
fn another_account_starts_clean() {
    let lib = Library::temporary().unwrap();
    lib.set_tracker_account("p", "trakt", Some("a"), 1, 1)
        .unwrap();
    lib.put_tracker_intent(
        "tt1",
        "tt1",
        NewIntent {
            key: key("a"),
            op: IntentOp::Add,
            watched_ms: 1,
            session: None,
        },
    )
    .unwrap();
    lib.set_tracker_state(key("a"), "history_cursor", "2026-10-01")
        .unwrap();
    lib.set_tracker_account("p", "trakt", Some("b"), 2, 2)
        .unwrap();
    assert!(lib.ready_tracker_intents(key("a"), 10).unwrap().is_empty());
    assert_eq!(lib.tracker_state(key("a"), "history_cursor").unwrap(), None);
    assert_eq!(
        lib.tracker_account("p", "trakt").unwrap(),
        Some(("b".into(), 2))
    );
}
