//! Stream choice in the runtime (plans/m13-stream-choice.md 4.2, 3.11):
//! what the view reads (the app's facts, the meta, the cache check) and
//! what it says when nothing can play.

#[path = "../../mq-debrid/tests/common/mod.rs"]
mod common;
mod golden;
use golden::golden;

use common::{Server, route};
use mq_library::AddonRow;
use mq_runtime::{
    BlockedReason, Core, EmptyAction, EmptyKind, SectionQuality, StoreId, TitleFacts,
    VersionsParams, VersionsView,
};
use mq_streams::{DeviceCaps, HideReason, SourceClass, SourceKind, StreamPrefs, Target};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Two torrents of one episode: A well seeded, B less so.
fn torrents() -> String {
    serde_json::json!({ "streams": [
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-AAA\n👤 900 💾 2.0 GB",
          "infoHash": HASH_A, "fileIdx": 0 },
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-BBB\n👤 100 💾 1.5 GB",
          "infoHash": HASH_B, "fileIdx": 1 },
    ]})
    .to_string()
}

/// A stream and meta addon at `server`.
fn addon(server: &Server) -> AddonRow {
    let manifest = serde_json::json!({
        "id": "test", "version": "1.0.0", "name": "Test",
        "types": ["series"], "resources": ["stream", "meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    AddonRow {
        transport_url: format!("{}/manifest.json", server.base),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

fn meta() -> String {
    serde_json::json!({ "meta": {
        "id": "tt1", "type": "series", "name": "Show", "runtime": "52 min", "releaseInfo": "2019-",
        "videos": [
            { "id": "tt1:1:1", "title": "One", "season": 1, "episode": 1, "released": "2019-05-01T00:00:00.000Z" },
            { "id": "tt1:1:2", "title": "Two", "season": 1, "episode": 2, "released": "2019-05-08T00:00:00.000Z" },
        ]
    }})
    .to_string()
}

fn params(facts: TitleFacts) -> VersionsParams {
    VersionsParams {
        device_language: None,
        kind: "series".into(),
        video_id: "tt1:1:1".into(),
        target: Target {
            title: "Show".into(),
            year: None,
            season: Some(1),
            episode: Some(1),
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
        facts,
    }
}

/// Free space the cores here read: an engine build hides a swarm larger
/// than the free space, and no test depends on the host's disk.
const FREE: u64 = 1 << 40;

fn core_with(server: &Server) -> Core {
    let core = Core::temporary("p").unwrap();
    core.set_free_bytes(FREE);
    core.library()
        .set_addons("p", &[addon(server)], &core.hlc())
        .unwrap();
    core
}

/// Waits until `done` holds, or fails after 60 s (a loaded host).
async fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// 3.11, mutation 16: with no engine and no account every torrent is
/// hidden, with those versions ranked as they would be. The view says so
/// quietly, with no action: an account is set up in Settings only.
#[tokio::test]
async fn without_engine_or_account_versions_say_so_with_no_action() {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents()),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    if core.has_engine() {
        // Full builds open torrents themselves.
        return;
    }
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("done", || view.snapshot().done).await;
    let s = view.snapshot();
    golden("versions-empty-needsAccount", &s);
    assert!(s.recommended.is_none() && s.rows.is_empty(), "{s:?}");
    let blocked = s.blocked.expect("blocked");
    assert_eq!(
        (blocked.reason, blocked.count),
        (BlockedReason::NeedsAccount, 2)
    );
    let empty = s.empty.expect("empty");
    assert_eq!(
        (empty.kind, empty.action, empty.count, empty.rows.len()),
        (EmptyKind::NeedsAccount, EmptyAction::None, 2, 2)
    );
    let names: Vec<&str> = blocked
        .rows
        .iter()
        .map(|r| r.release_name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Show.S01E01.1080p.WEB-DL.x264-AAA",
            "Show.S01E01.1080p.WEB-DL.x264-BBB"
        ],
        "ranked by seeders, as with an account"
    );
    assert!(
        blocked
            .rows
            .iter()
            .all(|r| r.source_class == SourceClass::Unknown)
    );

    // An account opens them: nothing is blocked.
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("done", || view.snapshot().done).await;
    let s = view.snapshot();
    assert!(s.blocked.is_none() && s.recommended.is_some(), "{s:?}");
}

/// 4.2: the app sent no runtime, so the view reads the meta and `done`
/// waits for it; the rows then carry their bitrate. With the facts sent,
/// the meta is never asked.
#[tokio::test]
async fn done_waits_for_the_meta_the_app_did_not_send() {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents()),
        route("GET", "/meta/series/tt1.json", 200, &meta()).delayed(Duration::from_millis(600)),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    let opened = Instant::now();
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("done", || view.snapshot().done).await;
    assert!(
        opened.elapsed() >= Duration::from_millis(600),
        "done before the meta"
    );
    let s = view.snapshot();
    assert_eq!(s.conditions.runtime_min, Some(52));
    let best = s.recommended.expect("a pick");
    // 2.0 GiB over 52 minutes.
    let mbps = best.bitrate_mbps.expect("a bitrate");
    assert!((5.0..6.0).contains(&mbps), "{mbps}");
    assert_eq!(server.count("GET", "/meta/"), 1);

    let facts = TitleFacts::read(Some("52 min"), Some("2019-"), Some("2019-05-01"), Some(2));
    let view = VersionsView::open(&core, params(facts));
    until("done", || view.snapshot().done).await;
    assert_eq!(view.snapshot().conditions.runtime_min, Some(52));
    assert_eq!(server.count("GET", "/meta/"), 1, "the app sent every fact");
}

/// r4 N4: the cache check races `done`. TorBox says the less seeded
/// torrent is cached; the view waits for the answer, so that row is the
/// pick, ready on the service. The free space is the core's, so a full
/// host disk never hides the other.
#[tokio::test]
async fn a_torrent_the_check_finds_cached_is_ready_before_done() {
    let cached =
        format!(r#"{{"success":true,"error":null,"detail":"ok","data":[{{"hash":"{HASH_B}"}}]}}"#);
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents()),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
        route("POST", "/v1/api/torrents/checkcached", 200, &cached)
            .delayed(Duration::from_millis(500)),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_origin(&server.base);
    core.set_debrid_keys(&[(StoreId::TorBox, "tb".into())]);
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("done", || view.snapshot().done).await;
    let s = view.snapshot();
    assert_eq!(s.conditions.free_bytes, Some(FREE), "the core's free space");
    let best = s
        .recommended
        .clone()
        .unwrap_or_else(|| panic!("no pick: {s:?}"));
    assert_eq!(best.release_name, "Show.S01E01.1080p.WEB-DL.x264-BBB");
    assert_eq!(best.source_class, SourceClass::Ready);
    // The other one is not cached: the engine fetches it, else the
    // service has to download it first.
    let other = s.rows.first().unwrap_or_else(|| panic!("one row: {s:?}"));
    let expected = if core.has_engine() {
        SourceClass::Swarm
    } else {
        SourceClass::Fetch
    };
    assert_eq!(other.source_class, expected);
    // The pick is then warmed, which asks the service once more.
    assert!(server.count("POST", "/v1/api/torrents/checkcached") >= 1);
}

/// Live seeders (C21 scrape): a swarm's count from its trackers stands
/// over the addon's number. A listed 900 its own trackers count at 0 may
/// not start; a swarm no answer told anything about keeps the addon's
/// number (the engine gives none for a default tracker's 0/0: its test
/// `an_answer_counts_from_an_own_tracker_or_with_peers`).
#[tokio::test]
async fn live_seeders_stand_over_the_addons_number() {
    const HASH_C: &str = "cccccccccccccccccccccccccccccccccccccccc";
    let own = ["tracker:udp://tracker.example:1337/announce"];
    let streams = serde_json::json!({ "streams": [
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-AAA\n👤 900 💾 2.0 GB",
          "infoHash": HASH_A, "fileIdx": 0, "sources": own },
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-BBB\n👤 100 💾 1.5 GB",
          "infoHash": HASH_B, "fileIdx": 1 },
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-CCC\n👤 60 💾 1.4 GB",
          "infoHash": HASH_C, "fileIdx": 2, "sources": own },
    ]});
    let server = Server::start(vec![
        route(
            "GET",
            "/stream/series/tt1%3A1%3A1.json",
            200,
            &streams.to_string(),
        ),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    // HASH_B: no count, as for a default tracker's 0/0.
    let live = [(HASH_A.to_owned(), 0)];
    core.set_live_seeders(std::collections::HashMap::from(live));
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    // The counts rank the rows when they come; `done` waits for them.
    let counted = || {
        let s = view.snapshot();
        s.done
            && s.rows.iter().chain(&s.recommended).any(|r| {
                r.release_name.ends_with("AAA")
                    && matches!(
                        r.status,
                        mq_runtime::VersionStatus::Torrent { seeds: Some(0) }
                    )
            })
    };
    until("the live counts", counted).await;
    let s = view.snapshot();
    let best = s
        .recommended
        .clone()
        .unwrap_or_else(|| panic!("no pick: {s:?}"));
    assert_eq!(best.release_name, "Show.S01E01.1080p.WEB-DL.x264-BBB");
    let seeds = |r: &mq_runtime::VersionRow| match r.status {
        mq_runtime::VersionStatus::Torrent { seeds } => seeds,
        _ => panic!("a torrent row: {r:?}"),
    };
    assert_eq!(seeds(&best), Some(100), "no count: the addon's number");
    let row = |name: &str| {
        s.rows
            .iter()
            .find(|r| r.release_name == format!("Show.S01E01.1080p.WEB-DL.x264-{name}"))
            .unwrap_or_else(|| panic!("row {name}: {s:?}"))
    };
    assert_eq!(seeds(row("CCC")), Some(60), "no answer: the addon's number");
    assert_eq!(seeds(row("AAA")), Some(0));
    assert_eq!(
        row("AAA").line,
        mq_streams::ReasonLine::VeryFew { seeders: 0 }
    );
}

/// Code review 2026-10-20 P3: after a failed hand pick, a later pick in
/// the view (an offer taken) teaches nothing.
#[tokio::test]
async fn a_pick_after_a_failed_hand_pick_teaches_nothing() {
    let (core, _server) = learning_core().await;
    let view = settled_view(&core).await;
    view.pick(LARGE).handle.failed(LARGE);
    view.freeze();
    assert!(view.pick(LARGE).note.is_none());
    assert_eq!(core.prefs().learned.fhd.size, 0);
}

/// Learning (3.9): two 1080p torrents, R small and well seeded (the
/// recommendation), C five times its size, over the 5 GB default limit (in
/// it, the bigger file of the same source is the pick,
/// plans/stream-ranking.md 4.9 rule 3).
fn sizes() -> String {
    serde_json::json!({ "streams": [
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-SMALL\n👤 900 💾 1.2 GB",
          "infoHash": HASH_A, "fileIdx": 0 },
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-LARGE\n👤 300 💾 6.0 GB",
          "infoHash": HASH_B, "fileIdx": 0 },
    ]})
    .to_string()
}

const LARGE: &str = "bt:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:0";

async fn learning_core() -> (Core, Server) {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &sizes()),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    (core, server)
}

async fn settled_view(core: &Core) -> VersionsView {
    let facts = TitleFacts::read(Some("52 min"), Some("2019-"), Some("2019-05-01"), Some(2));
    let view = VersionsView::open(core, params(facts));
    until("done", || view.snapshot().done).await;
    view
}

/// One counter per pick, only from a frozen view, and only the first pick
/// of a view; at 3 the note comes with the preferences now in effect, and
/// the next view applies them.
#[tokio::test]
async fn three_bigger_picks_raise_the_size_limit_with_a_note() {
    let (core, _server) = learning_core().await;
    let view = settled_view(&core).await;
    assert_ne!(view.snapshot().recommended.unwrap().key, LARGE);
    // Not frozen: the pick teaches nothing.
    assert!(view.pick(LARGE).note.is_none());
    assert_eq!(core.prefs().learned.fhd.size, 0);
    let mut notes = Vec::new();
    for _ in 0..3 {
        let view = settled_view(&core).await;
        view.freeze();
        notes.push(view.pick(LARGE).note);
        // A second pick in the same view does not count.
        view.pick(LARGE);
    }
    let learned = core.prefs().learned.fhd;
    assert_eq!((learned.size, learned.height, learned.ready), (3, 0, 0));
    assert!(notes[0].is_none() && notes[1].is_none());
    let note = notes[2].clone().expect("a note at 3");
    assert!(note.up && note.screen_class == 1080);
    assert_eq!(note.prefs.size_limit_bytes, Some(10 << 30));
    let view = settled_view(&core).await;
    assert_eq!(view.snapshot().conditions.size_limit_bytes, Some(10 << 30));
}

/// Mutation 18: picking the stream last played for the title again is a
/// replay; mutation 19: a pick after the recommendation failed here.
#[tokio::test]
async fn replays_and_picks_after_a_failure_teach_nothing() {
    let (core, _server) = learning_core().await;
    let memory = serde_json::json!({ "key": LARGE }).to_string();
    core.library()
        .set_stream_memory("p", "tt1", None, &memory, core.clock())
        .unwrap();
    let view = settled_view(&core).await;
    view.freeze();
    view.pick(LARGE);
    assert_eq!(core.prefs().learned.fhd.size, 0, "a replay");

    core.library()
        .set_stream_memory("p", "tt1", None, "{\"key\":\"other\"}", core.clock())
        .unwrap();
    let view = settled_view(&core).await;
    view.freeze();
    let recommended = view.snapshot().recommended.unwrap().key;
    view.pick(&recommended).handle.failed(&recommended);
    view.pick(LARGE);
    assert_eq!(
        core.prefs().learned.fhd.size,
        0,
        "the recommendation failed"
    );

    let view = settled_view(&core).await;
    view.freeze();
    view.pick(LARGE);
    assert_eq!(core.prefs().learned.fhd.size, 1, "a real choice");
}

/// Mutation 20: "Forget what was learned" raises the epoch above every
/// stamp, so a counter an offline device wrote before the reset reads as
/// zero when it arrives.
#[tokio::test]
async fn a_reset_survives_an_offline_device() {
    let (core, _server) = learning_core().await;
    let now = mq_library::hlc::now_ms();
    let old = serde_json::json!({ "learned": { "fhd": { "size": -3, "epoch": 0, "atMs": now } } });
    core.set_prefs(&old).unwrap();
    let view = settled_view(&core).await;
    assert_eq!(view.snapshot().conditions.size_limit_bytes, Some(5 << 29));
    let prefs = core.forget_learned().unwrap();
    assert_eq!(prefs.learned_epoch, 1);
    // The offline device's counter lands after the reset.
    core.set_prefs(&old).unwrap();
    let view = settled_view(&core).await;
    assert_eq!(view.snapshot().conditions.size_limit_bytes, Some(5 << 30));
    // An older epoch leaf does not lower it either.
    core.set_prefs(&serde_json::json!({ "learnedEpoch": 0 }))
        .unwrap();
    let view = settled_view(&core).await;
    assert_eq!(view.snapshot().conditions.size_limit_bytes, Some(5 << 30));
}

/// 3.9: a preference set by hand zeroes its counter; Undo of a note mutes
/// that choice.
#[tokio::test]
async fn a_hand_set_limit_zeroes_its_counter_and_undo_mutes() {
    let (core, _server) = learning_core().await;
    core.set_prefs(&serde_json::json!({ "learned": { "fhd": { "size": 2, "height": 1 } } }))
        .unwrap();
    core.set_prefs(&serde_json::json!({ "streams": { "sizeLimitBytes": 2147483648u64 } }))
        .unwrap();
    let learned = core.prefs().learned.fhd;
    assert_eq!((learned.size, learned.height), (0, 1));
    let prefs = core
        .undo_learned(1080, mq_runtime::learn::LearnedChoice::Height)
        .unwrap();
    assert_eq!(prefs.learned.fhd.height, 0);
    assert_eq!(
        prefs.learned.fhd.muted,
        [mq_runtime::learn::LearnedChoice::Height]
    );
}

/// 4.1: an older device's `quality` is read as the new fields, and the
/// first stream write stores them, so it is mapped once.
#[tokio::test]
async fn the_old_quality_is_mapped_once() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_setting("p", "streams.quality", "\"dataSaver\"", core.clock())
        .unwrap();
    let streams = core.prefs().streams;
    assert_eq!(streams.size_limit_bytes, Some(2 << 30));
    assert_eq!(streams.max_height, Some(720));
    core.set_prefs(&serde_json::json!({ "streams": { "languages": ["tr"] } }))
        .unwrap();
    // Changing the old field later moves nothing: the mapping is stored.
    core.library()
        .set_setting("p", "streams.quality", "\"best\"", core.clock())
        .unwrap();
    let streams = core.prefs().streams;
    assert_eq!(streams.size_limit_bytes, Some(2 << 30));
    assert_eq!(streams.max_height, Some(720));
    assert_eq!(streams.languages, ["tr"]);

    let fresh = Core::temporary("q").unwrap();
    assert_eq!(
        fresh.prefs().streams.size_limit_bytes,
        Some(5 << 30),
        "no old field: v2"
    );
}

/// 3.7: a size limit the user set is named as chosen on a large row; the
/// default limit is not.
#[tokio::test]
async fn a_limit_the_user_set_is_named_as_chosen() {
    let (core, _server) = learning_core().await;
    let large_line = |view: &VersionsView| {
        let s = view.snapshot();
        let row = s
            .recommended
            .iter()
            .chain(&s.rows)
            .find(|r| r.key == LARGE)
            .cloned()
            .expect("the large row");
        row.line
    };
    core.set_prefs(&serde_json::json!({ "streams": { "sizeLimitBytes": 2147483648u64 } }))
        .unwrap();
    // A long episode, so the 6 GB file keeps up and its size is the news.
    let facts = TitleFacts::read(Some("120 min"), Some("2019-"), Some("2019-05-01"), Some(2));
    let view = VersionsView::open(
        &core,
        VersionsParams {
            device_language: None,
            prefs: core.prefs().streams,
            ..params(facts)
        },
    );
    until("done", || view.snapshot().done).await;
    assert!(
        matches!(
            large_line(&view),
            mq_streams::ReasonLine::Large { chosen: true, .. }
        ),
        "{:?}",
        large_line(&view)
    );
}

/// Code review r1 P3: a pick made while the recommendation is still
/// starting teaches nothing, since its failure may follow; once it started,
/// a pick is a real choice.
#[tokio::test]
async fn a_pick_while_the_recommendation_starts_waits_for_it() {
    let (core, _server) = learning_core().await;
    let view = settled_view(&core).await;
    view.freeze();
    let recommended = view.snapshot().recommended.unwrap().key;
    let first = view.pick(&recommended);
    view.pick(LARGE);
    assert_eq!(core.prefs().learned.fhd.size, 0, "still starting");
    first.handle.ready();
    view.pick(LARGE);
    assert_eq!(
        core.prefs().learned.fhd.size,
        1,
        "it started: a real choice"
    );
}

/// A loopback origin serving `body` with `Range` support, 64 KiB at a
/// time with `pause` between (5 ms: about 12 MB/s, so a read is in flight
/// long enough to count as a sample).
async fn range_origin(body: Vec<u8>, pause: Duration) -> String {
    let up = Arc::new(AtomicBool::new(true));
    range_origin_while(body, pause, up).await
}

/// `range_origin` that, while `up` is false, drops every connection it
/// accepts (a lost network).
async fn range_origin_while(body: Vec<u8>, pause: Duration, up: Arc<AtomicBool>) -> String {
    let body = std::sync::Arc::new(body);
    let len = body.len();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            if !up.load(Ordering::Relaxed) {
                drop(socket);
                continue;
            }
            let body = body.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 2048];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&head).to_ascii_lowercase();
                let start: usize = head
                    .split("range: bytes=")
                    .nth(1)
                    .and_then(|r| r.split('-').next()?.trim().parse().ok())
                    .unwrap_or(0);
                let body_len = len - start;
                let answer = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Type: video/mp4\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {body_len}\r\n\r\n",
                    len - 1
                );
                if socket.write_all(answer.as_bytes()).await.is_err() {
                    return;
                }
                for chunk in body[start..].chunks(64 << 10) {
                    if socket.write_all(chunk).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(pause).await;
                }
            });
        }
    });
    base
}

/// Android's finding: a short direct trailer never counted toward the
/// speed. Now one playback's HTTP capacity sample is taken when the view
/// closes before 30 s, and the first sample is the estimate.
#[tokio::test]
async fn one_short_playback_measures_the_connection() {
    let base = range_origin(vec![0; 6 << 20], Duration::from_millis(5)).await;
    let core = Core::temporary("p").unwrap();
    assert_eq!(core.speed(mq_runtime::speed::Kind::Http).samples, 0);
    let mut stream = mq_streams::normalize(
        &mq_protocol::decode_streams(
            format!(r#"{{"streams":[{{"url":"{base}/trailer.mp4"}}]}}"#).as_bytes(),
        )
        .unwrap()
        .streams[0],
        "t",
        "T",
        0,
        0,
    );
    // Listed by an install on the same loopback host: its reach.
    stream.addon_url = Some(url::Url::parse(&format!("{base}/manifest.json")).unwrap());
    let view =
        mq_runtime::PlaybackView::open(&core, vec![stream], mq_runtime::PlayTarget::default());
    until("ready", || view.snapshot().url.is_some()).await;
    // Read it whole, as a player does.
    let url = view.snapshot().url.unwrap();
    let loopback = mq_http::RequestOptions {
        allow_private: true,
        timeout: Duration::from_secs(60),
        ..Default::default()
    };
    let body = core.http().get(&url, &loopback).await.unwrap();
    assert_eq!(body.body.len(), 6 << 20);
    drop(view);
    let speed = core.speed(mq_runtime::speed::Kind::Http);
    assert_eq!(speed.samples, 1, "{speed:?}");
    assert!(speed.mbps > 1.0, "{speed:?}");
}

/// A synthetic MP4 from mq-media's fixtures (ffmpeg's testsrc, 64x64, 1 s).
fn media_fixture(name: &str) -> Vec<u8> {
    let dir = env!("CARGO_MANIFEST_DIR");
    std::fs::read(format!("{dir}/../mq-media/tests/fixtures/{name}")).unwrap()
}

/// An hev1 file whose `hvcC` has no parameter sets: Apple shows no
/// picture from it, even served as hvc1.
fn bare_hev1() -> Vec<u8> {
    let mut bare = media_fixture("hev1-moov-tail.mp4");
    let record = bare.windows(4).position(|w| w == b"hvcC").unwrap() + 4;
    bare[record + 22] = 0; // numOfArrays
    bare
}

/// A core whose addon lists each `(title, file)` as a direct link, each
/// from its own loopback origin; the keys in the same order.
async fn direct_rows(rows: Vec<(&str, Vec<u8>)>) -> (Core, Server, Vec<String>) {
    let (mut streams, mut keys) = (Vec::new(), Vec::new());
    for (i, (title, body)) in rows.into_iter().enumerate() {
        let url = format!("{}/v{i}.mp4", range_origin(body, Duration::ZERO).await);
        keys.push(format!("url:{url}"));
        streams.push(serde_json::json!({ "name": "Direct", "title": title, "url": url }));
    }
    let streams = serde_json::json!({ "streams": streams }).to_string();
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &streams),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    (core_with(&server), server, keys)
}

async fn versions_for(core: &Core, caps: DeviceCaps) -> VersionsView {
    let facts = TitleFacts::read(Some("52 min"), Some("2019-"), Some("2019-05-01"), Some(2));
    let view = VersionsView::open(
        core,
        VersionsParams {
            device_language: None,
            caps,
            ..params(facts)
        },
    );
    until("done", || view.snapshot().done).await;
    view
}

fn play(core: &Core, view: &VersionsView, key: &str) -> mq_runtime::PlaybackView {
    let target = mq_runtime::PlayTarget::default();
    mq_runtime::PlaybackView::open_picked(core, view.pick(key), target, &view.plays())
}

fn apple() -> DeviceCaps {
    DeviceCaps {
        hvc1_only: true,
        ..DeviceCaps::basic_1080p()
    }
}

const NO_PICTURE: &str = "This file's video shows no picture on this device";

/// On an hvc1-only device a no-picture file does not start, and the
/// attempt says why; the recommendation moves on by itself to the
/// ranking's next version and names it. The next Versions view ranks the
/// file last with its line. A device that shows it plays it.
#[tokio::test]
async fn a_file_with_no_picture_here_does_not_start_and_ranks_last() {
    let (core, _server, keys) = direct_rows(vec![
        ("Show.S01E01.1080p.WEB-DL.HEVC-BARE", bare_hev1()),
        (
            "Show.S01E01.720p.WEB-DL.HEVC-FINE",
            media_fixture("hvc1.mp4"),
        ),
    ])
    .await;
    let (bare, fine) = (&keys[0], &keys[1]);

    let view = versions_for(&core, apple()).await;
    assert_eq!(&view.snapshot().recommended.unwrap().key, bare);
    let playback = play(&core, &view, bare);
    until("the next version", || playback.snapshot().url.is_some()).await;
    let s = playback.snapshot();
    assert_eq!(s.key.as_ref(), Some(fine));
    assert_eq!(s.attempts.len(), 1, "{:?}", s.attempts);
    assert_eq!(&s.attempts[0].key, bare);
    assert_eq!(s.attempts[0].error, NO_PICTURE);
    // Named while it started, cleared once it did (code review 2026-10-20
    // P3: a later source failover is not a next version).
    assert!(s.trying.is_none(), "{:?}", s.trying);
    assert!(s.offer.is_none());
    drop((playback, view));

    let view = versions_for(&core, apple()).await;
    let snapshot = view.snapshot();
    assert_eq!(&snapshot.recommended.unwrap().key, fine);
    let row = snapshot.rows.iter().find(|r| &r.key == bare).unwrap();
    assert_eq!(row.line, mq_streams::ReasonLine::NoPicture);
    drop(view);

    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert_eq!(&view.snapshot().recommended.unwrap().key, bare);
    let playback = play(&core, &view, bare);
    until("the file where it shows", || {
        playback.snapshot().url.is_some()
    })
    .await;
    assert_eq!(playback.snapshot().key.as_ref(), Some(bare));
    assert!(playback.snapshot().attempts.is_empty());
}

/// Pioneer One on an iPhone (journey walk AQD6): a torrent listed with no
/// file name is an MKV file, on a device whose player opens no Matroska
/// and has no second engine. Once the service names the file it fails as
/// the version's own, its container in the words, before the player gets
/// it, and nothing says it did not play: Versions hides it on that device
/// from then on. A device that opens MKV keeps it.
#[tokio::test]
async fn a_file_the_device_never_opens_never_reaches_the_player() {
    const FILE: &str = "/Show.S01E01.720p.x264-VODO.mkv";
    let link = serde_json::json!({
        "filename": &FILE[1..], "filesize": 1000, "download": format!("https://cdn.example{FILE}"),
    });
    let info = serde_json::json!({
        "id": "RD1", "filename": "Show", "hash": HASH_A, "progress": 100, "status": "downloaded",
        "files": [{ "id": 1, "path": FILE, "bytes": 1000, "selected": 1 }],
        "links": ["https://real-debrid.com/d/AAA"],
    });
    let streams = serde_json::json!({ "streams": [
        { "name": "Torrentio\n720p", "title": "Show S01E01 720p x264 VODO\n👤 6 💾 1.12 GB",
          "infoHash": HASH_A },
    ]});
    let server = Server::start(vec![
        route(
            "GET",
            "/stream/series/tt1%3A1%3A1.json",
            200,
            &streams.to_string(),
        ),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
        route(
            "POST",
            "/rest/1.0/torrents/addMagnet",
            201,
            r#"{"id":"RD1"}"#,
        ),
        route("GET", "/rest/1.0/torrents/info/RD1", 200, &info.to_string()),
        route("POST", "/rest/1.0/unrestrict/link", 200, &link.to_string()),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_origin(&server.base);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    let iphone = DeviceCaps {
        containers: ["mp4", "m4v", "mov", "m3u8"].map(str::to_owned).to_vec(),
        has_fallback_engine: false,
        ..apple()
    };
    let unplayable = |view: &VersionsView| {
        let s = view.snapshot();
        let hidden = s.hidden.by_reason.contains(&(HideReason::Unplayable, 1));
        s.recommended.is_none() && s.rows.is_empty() && hidden
    };

    let view = versions_for(&core, iphone.clone()).await;
    let key = view
        .snapshot()
        .recommended
        .expect("its container unstated")
        .key;
    let target = mq_runtime::PlayTarget {
        kind: "series".into(),
        item_id: "tt1".into(),
        video_id: "tt1:1:1".into(),
        season: Some(1),
        episode: Some(1),
        ..mq_runtime::PlayTarget::default()
    };
    let playback =
        mq_runtime::PlaybackView::open_picked(&core, view.pick(&key), target, &view.plays());
    let failed = || playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
    until("the version to fail", failed).await;
    let s = playback.snapshot();
    assert!(s.url.is_none());
    assert_eq!(s.attempts.len(), 1, "{:?}", s.attempts);
    assert_eq!(s.attempts[0].error, "This device cannot open MKV files");
    assert_eq!(server.count("POST", "/rest/1.0/unrestrict/link"), 1);
    assert!(
        unplayable(&view),
        "offered: {:?}",
        view.snapshot().recommended.map(|r| r.line)
    );
    // The open view says why at once, from the ranking now, not from
    // before the failure (review 2026-10-23 P2-2).
    let s = view.snapshot();
    assert_eq!(
        s.blocked.map(|b| b.reason),
        Some(BlockedReason::NothingPlaysHere)
    );
    assert_eq!(s.empty.map(|e| e.kind), Some(EmptyKind::NothingPlaysHere));
    drop((playback, view));

    let view = versions_for(&core, iphone).await;
    assert!(
        unplayable(&view),
        "offered: {:?}",
        view.snapshot().recommended.map(|r| r.line)
    );
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let row = view
        .snapshot()
        .recommended
        .expect("offered where MKV opens");
    assert_eq!(row.key, key);
    assert_ne!(row.line, mq_streams::ReasonLine::DidNotPlay);
}

/// A version picked by hand that fails for a reason of its own does not
/// move on: the failure card offers the ranking's next version.
#[tokio::test]
async fn a_version_picked_by_hand_offers_the_next_one() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.HEVC-FINE",
            media_fixture("hvc1.mp4"),
        ),
        ("Show.S01E01.720p.WEB-DL.HEVC-BARE", bare_hev1()),
    ])
    .await;
    let (fine, bare) = (&keys[0], &keys[1]);
    let view = versions_for(&core, apple()).await;
    assert_eq!(&view.snapshot().recommended.unwrap().key, fine);
    let playback = play(&core, &view, bare);
    let failed = || playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
    until("the version to fail", failed).await;
    let s = playback.snapshot();
    assert!(s.url.is_none() && s.trying.is_none());
    assert_eq!(s.attempts.len(), 1, "{:?}", s.attempts);
    assert_eq!(s.attempts[0].error, NO_PICTURE);
    let offer = s.offer.expect("the next version, offered");
    assert_eq!((&offer.key, offer.label.as_str()), (fine, "1080p WEB"));
}

/// The recommendation moves on by itself, within the start's budget, until
/// a version plays (plans/play-every-version.md 2e: no count of moves).
#[tokio::test]
async fn the_recommendation_moves_on_until_a_version_plays() {
    let (core, _server, keys) = direct_rows(vec![
        ("Show.S01E01.1080p.WEB-DL.HEVC-A", bare_hev1()),
        ("Show.S01E01.720p.WEB-DL.HEVC-B", bare_hev1()),
        ("Show.S01E01.480p.WEB-DL.HEVC-C", bare_hev1()),
        ("Show.S01E01.360p.WEB-DL.HEVC-D", media_fixture("hvc1.mp4")),
    ])
    .await;
    let view = versions_for(&core, apple()).await;
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[0]);
    let playback = play(&core, &view, &keys[0]);
    until("a version to play", || playback.snapshot().url.is_some()).await;
    let s = playback.snapshot();
    let tried: Vec<&String> = s.attempts.iter().map(|a| &a.key).collect();
    assert_eq!(tried, [&keys[0], &keys[1], &keys[2]]);
    assert_eq!(s.key.as_ref(), Some(&keys[3]));
    assert!(s.trying.is_none() && s.offer.is_none());
}

/// A version that showed no picture here did not play on this device, as
/// a decode failure did not: `moved` says so, not "did not start here".
#[tokio::test]
async fn a_move_past_no_picture_says_it_did_not_play_here() {
    let (core, _server, keys) = direct_rows(vec![
        ("Show.S01E01.1080p.WEB-DL.HEVC-A", bare_hev1()),
        ("Show.S01E01.720p.WEB-DL.HEVC-B", media_fixture("hvc1.mp4")),
    ])
    .await;
    let view = versions_for(&core, apple()).await;
    let playback = play(&core, &view, &keys[0]);
    until("the next version", || playback.snapshot().url.is_some()).await;
    let s = playback.snapshot();
    assert_eq!(s.key.as_ref(), Some(&keys[1]));
    assert_eq!(
        s.moved.as_deref(),
        Some("Playing 720p WEB: the 1080p WEB did not play on this device")
    );
}

/// Only a failure of the version's own moves on: the player's "decode"
/// does; a network failure, the kinds the apps send for a fetch that
/// failed or a busy decoder ("unreadable", "other") and a kind this core
/// does not know do not, and none of them is kept.
#[tokio::test]
async fn only_a_failure_of_the_versions_own_moves_on() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.x264-ONE",
            media_fixture("h264.mp4"),
        ),
        (
            "Show.S01E01.720p.WEB-DL.x264-TWO",
            media_fixture("h264.mp4"),
        ),
    ])
    .await;
    let caps = DeviceCaps::basic_1080p();
    for kind in ["network", "unreadable", "other", "a-kind-from-a-newer-app"] {
        let view = versions_for(&core, caps.clone()).await;
        assert_eq!(
            view.snapshot().recommended.unwrap().key,
            keys[0],
            "{kind}: not kept"
        );
        let playback = play(&core, &view, &keys[0]);
        until("ready", || playback.snapshot().url.is_some()).await;
        playback.fail_over(None, Some(kind), None, "the player failed");
        let failed = || playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
        until("the version to fail", failed).await;
        let s = playback.snapshot();
        assert!(s.trying.is_none() && s.offer.is_none(), "{kind}");
        assert_eq!(s.attempts.len(), 1, "{kind}: words, never refused");
    }

    let view = versions_for(&core, caps).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.fail_over(None, Some("decode"), None, "no frame");
    let moved = || playback.snapshot().key.as_ref() == Some(&keys[1]);
    until("the next version", moved).await;
    let s = playback.snapshot();
    assert_eq!(s.attempts.len(), 1, "the decode failure, then the next");
}

/// Five direct versions: the pick from two sources of one release, a
/// second 1080p sharing its 10-bit HEVC and E-AC-3, a cam copy and a 720p
/// H.264 with AAC.
async fn apart_rows() -> (Core, Server, Vec<String>) {
    let row = |name| (name, media_fixture("h264.mp4"));
    direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.DDP5.1.HEVC.10bit-ONE"),
        row("Show.S01E01.1080p.WEB-DL.DDP5.1.HEVC.10bit-ONE"),
        row("Show.S01E01.1080p.WEB-DL.DDP5.1.HEVC.10bit-TWO"),
        row("Show.S01E01.720p.HDCAM.x264.AAC-CAM"),
        row("Show.S01E01.720p.WEB-DL.AAC.x264-THREE"),
    ])
    .await
}

/// The line of row `key` in a view.
fn line_of(view: &VersionsView, key: &str) -> mq_streams::ReasonLine {
    let s = view.snapshot();
    s.recommended
        .into_iter()
        .chain(s.rows)
        .find(|r| r.key == key)
        .unwrap_or_else(|| panic!("no row {key}"))
        .line
}

/// A device whose output takes E-AC-3 as it is (a receiver).
fn passing_eac3() -> DeviceCaps {
    DeviceCaps {
        passthrough: vec![mq_relparse::Audio::Eac3],
        ..DeviceCaps::basic_1080p()
    }
}

/// After a version that did not play here, the next one differs in what
/// failed (pick audit 8.3, Ted Lasso S1:E1: its first two 1080p versions
/// share 10-bit HEVC and E-AC-3): a decode failure passes over the next
/// 1080p that shares the video and the audio for one that shares neither,
/// an audio failure of a format the output takes as it is for another
/// audio codec. The version's other source, the same file, is not tried.
/// Once it plays, the playback says the version playing is not the one
/// announced; a Try again clears it and starts the pick afresh. Only the
/// decode failure is kept on the device, against every source of the
/// version, since they carry the same file.
#[tokio::test]
async fn the_next_version_differs_in_what_did_not_play() {
    for kind in ["decode", "audio"] {
        let (core, _server, keys) = apart_rows().await;
        let view = versions_for(&core, passing_eac3()).await;
        let s = view.snapshot();
        assert_eq!(s.recommended.unwrap().key, keys[0], "{kind}");
        assert_eq!(s.rows[0].key, keys[2], "{kind}: the next 1080p ranks next");
        let playback = play(&core, &view, &keys[0]);
        until("ready", || playback.snapshot().url.is_some()).await;
        assert_eq!(playback.snapshot().moved, None, "{kind}: the announced one");
        playback.fail_over(None, Some(kind), None, "no frame");
        let moved = || playback.snapshot().key.is_some_and(|k| k != keys[0]);
        until("the next version", moved).await;
        let s = playback.snapshot();
        assert_eq!(s.key.as_ref(), Some(&keys[4]), "{kind}");
        let tried: Vec<&String> = s.attempts.iter().map(|a| &a.key).collect();
        assert_eq!(
            tried,
            [&keys[0]],
            "{kind}: the same file from its other source"
        );
        assert_eq!(
            s.moved.as_deref(),
            Some("Playing 720p WEB: the 1080p WEB did not play on this device"),
            "{kind}"
        );
        let next = versions_for(&core, passing_eac3()).await;
        let kept = line_of(&next, &keys[0]);
        let did_not_play = kept == mq_streams::ReasonLine::DidNotPlay;
        assert_eq!(did_not_play, kind == "decode", "{kind}: {kept:?}");
        // Kept against the version's other source too: the same file.
        let pick = next.snapshot().recommended.unwrap().key;
        assert_ne!(pick, keys[1], "{kind}: the same file recommended");
        playback.fail_over(None, Some("network"), None, "gone");
        let failed = || playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
        until("the playback to fail", failed).await;
        assert_eq!(playback.snapshot().moved, None, "{kind}: none while failed");
        playback.retry();
        assert_eq!(
            playback.snapshot().moved,
            None,
            "{kind}: Try again clears it"
        );
        let again = || playback.snapshot().key.as_ref() == Some(&keys[0]);
        until("the pick again", again).await;
        assert_eq!(playback.snapshot().moved, None, "{kind}: the pick plays");
    }
}

/// With no other version to move to, a version that did not play here
/// still tries its other sources, and the decode failure is kept against
/// the failed one only: the other source played (the chain is one release
/// name, not one set of codecs).
#[tokio::test]
async fn with_no_other_version_the_other_sources_are_tried() {
    let row = || {
        (
            "Show.S01E01.1080p.WEB-DL.DDP5.1.HEVC.10bit-ONE",
            media_fixture("h264.mp4"),
        )
    };
    let (core, _server, keys) = direct_rows(vec![row(), row()]).await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.fail_over(None, Some("decode"), None, "no frame");
    let other = || playback.snapshot().key.as_ref() == Some(&keys[1]);
    until("the version's other source", other).await;
    let next = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert_ne!(
        line_of(&next, &keys[1]),
        mq_streams::ReasonLine::DidNotPlay,
        "the source that played"
    );
}

/// A move has settled: the playback plays a source or has failed.
fn settled(playback: &mq_runtime::PlaybackView) -> bool {
    let s = playback.snapshot();
    s.key.is_some() || s.phase == mq_runtime::PlaybackPhase::Failed
}

/// With no other version left to move to, a version that did not play
/// here still tries its other sources.
#[tokio::test]
async fn after_the_last_move_the_other_sources_are_tried() {
    let row = |name| (name, media_fixture("h264.mp4"));
    let (core, _server, keys) = direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.AAC.x264-A"),
        row("Show.S01E01.720p.WEB-DL.AAC.x264-B"),
        row("Show.S01E01.480p.WEB-DL.AAC.x264-C"),
        row("Show.S01E01.480p.WEB-DL.AAC.x264-C"),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let playback = play(&core, &view, &keys[0]);
    for key in [&keys[0], &keys[1], &keys[2]] {
        let on = || playback.snapshot().key.as_ref() == Some(key);
        until("the next version", on).await;
        playback.fail_over(None, Some("decode"), None, "no frame");
    }
    until("a source or the failure", || settled(&playback)).await;
    assert_eq!(playback.snapshot().key.as_ref(), Some(&keys[3]));
}

/// A version picked by hand that did not play here tries its other
/// sources: it never moves on by itself, so nothing follows them.
#[tokio::test]
async fn a_hand_pick_that_did_not_play_tries_its_other_sources() {
    let row = |name| (name, media_fixture("h264.mp4"));
    let (core, _server, keys) = direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.AAC.x264-A"),
        row("Show.S01E01.720p.WEB-DL.DDP5.1.HEVC.10bit-B"),
        row("Show.S01E01.720p.WEB-DL.DDP5.1.HEVC.10bit-B"),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[0]);
    let playback = play(&core, &view, &keys[1]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.fail_over(None, Some("decode"), None, "no frame");
    until("a source or the failure", || settled(&playback)).await;
    assert_eq!(playback.snapshot().key.as_ref(), Some(&keys[2]));
}

/// An audio output that fails on decoded audio is the moment's (another
/// app, a route change): the playback does not move on. One that fails on
/// a format the device passes through, before the source played, moves on,
/// and nothing is kept: the next view still recommends that version. Once
/// the source played, an audio failure is the moment's again.
#[tokio::test]
async fn an_audio_failure_moves_on_only_for_a_passed_format_and_is_not_kept() {
    let (core, _server, keys) = apart_rows().await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.fail_over(None, Some("audio"), None, "audio track");
    let other = || playback.snapshot().key.as_ref() == Some(&keys[1]);
    until("the version's other source", other).await;
    assert!(
        playback.snapshot().trying.is_none(),
        "decoded audio moved on"
    );

    let view = versions_for(&core, passing_eac3()).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.fail_over(None, Some("audio"), None, "audio track");
    let moved = || playback.snapshot().key.as_ref() == Some(&keys[4]);
    until("the next version", moved).await;
    let view = versions_for(&core, passing_eac3()).await;
    assert_eq!(
        view.snapshot().recommended.unwrap().key,
        keys[0],
        "an audio failure was kept"
    );

    // Once the source has played, its audio failing is the moment's.
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.report_progress(1_000, 2_700_000, false);
    playback.fail_over(None, Some("audio"), None, "audio track");
    let other = || playback.snapshot().key.as_ref() == Some(&keys[1]);
    until("the version's other source", other).await;
    assert!(
        playback.snapshot().trying.is_none(),
        "a route change moved on"
    );
}

/// The version that differs most is chosen only among rows on the same
/// footing as the ranking's next: a cam copy that shares nothing with the
/// failed version never wins on difference alone.
#[tokio::test]
async fn the_next_version_apart_stays_on_the_same_footing() {
    let (core, _server, keys) = apart_rows().await;
    let view = versions_for(&core, passing_eac3()).await;
    let failed = view.candidates(&keys[0]).remove(0).release;
    let pick = view.pick(&keys[0]);
    let unlike = mq_runtime::views::versions::FailedOn::Decode(failed);
    // Past the pick and the 720p WEB: the second 1080p, which shares
    // everything, stays ahead of the cam, which shares nothing.
    let tried = [keys[0].clone(), keys[1].clone(), keys[4].clone()];
    let next = pick.handle.next_version(&tried, Some(&unlike)).unwrap();
    assert_eq!(next.key, keys[2]);
}

/// `done` waits for the live counts at most 3 s (`SIDE_WAIT`) once every
/// addon is in; counts that come later still rank the rows.
#[tokio::test]
async fn done_waits_three_seconds_at_most_for_the_live_counts() {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents()),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    core.set_live_seeders(std::collections::HashMap::from([(HASH_A.to_owned(), 0)]));
    core.delay_live_seeders(Duration::from_secs(5));
    let opened = Instant::now();
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("done", || view.snapshot().done).await;
    let waited = opened.elapsed();
    assert!(
        (Duration::from_millis(2500)..Duration::from_millis(4500)).contains(&waited),
        "{waited:?}"
    );
    assert!(view.counting(), "the counts are still on the way");
    let name = |view: &VersionsView| view.snapshot().recommended.unwrap().release_name;
    assert_eq!(
        name(&view),
        "Show.S01E01.1080p.WEB-DL.x264-AAA",
        "the listed 900"
    );
    until("the counts", || !view.counting()).await;
    assert_eq!(name(&view), "Show.S01E01.1080p.WEB-DL.x264-BBB");
}

/// A ready file leading the list is not held for the live counts: they rank
/// torrents, which come after it.
#[tokio::test]
async fn done_does_not_wait_for_the_counts_when_a_ready_file_leads() {
    let streams = serde_json::json!({ "streams": [
        { "name": "Direct", "title": "Show.S01E01.1080p.WEB-DL.x264-LINK",
          "url": "https://cdn.example/link.mkv" },
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-AAA\n👤 900 💾 2.0 GB",
          "infoHash": HASH_A, "fileIdx": 0 },
    ]});
    let server = Server::start(vec![
        route(
            "GET",
            "/stream/series/tt1%3A1%3A1.json",
            200,
            &streams.to_string(),
        ),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    core.set_live_seeders(std::collections::HashMap::from([(HASH_A.to_owned(), 0)]));
    core.delay_live_seeders(Duration::from_secs(5));
    let opened = Instant::now();
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("done", || view.snapshot().done).await;
    let s = view.snapshot();
    assert_eq!(s.recommended.unwrap().source_class, SourceClass::Ready);
    assert!(
        opened.elapsed() < Duration::from_millis(2000),
        "{:?}",
        opened.elapsed()
    );
    assert!(view.counting(), "the counts are still on the way");
}

/// Code review 2026-10-20 P1-1: "Try again" is a fresh start of the pick
/// itself after any moves on: its chain, nothing tried, no moves made. No
/// version shows a picture here, so every move fails, and the retry starts
/// with the pick again. The failure says why (play-every-version 2e).
#[tokio::test]
async fn try_again_starts_the_pick_afresh() {
    let (core, _server, keys) = direct_rows(vec![
        ("Show.S01E01.1080p.WEB-DL.HEVC-A", bare_hev1()),
        ("Show.S01E01.720p.WEB-DL.HEVC-B", bare_hev1()),
        ("Show.S01E01.480p.WEB-DL.HEVC-C", bare_hev1()),
    ])
    .await;
    let view = versions_for(&core, apple()).await;
    let playback = play(&core, &view, &keys[0]);
    let failed = || playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
    until("the last move to fail", failed).await;
    assert_eq!(playback.snapshot().empty, Some(EmptyKind::NothingPlaysHere));
    golden("playback-empty-nothingPlaysHere", &playback.snapshot());
    playback.retry();
    let s = playback.snapshot();
    assert!(s.trying.is_none() && s.attempts.is_empty() && s.empty.is_none());
    assert_eq!(s.retries, 1, "a retry that refails still differs");
    until("the retry to fail", failed).await;
    let s = playback.snapshot();
    let tried: Vec<&String> = s.attempts.iter().map(|a| &a.key).collect();
    // The others are known now to show no picture here: none follows.
    assert_eq!(tried, [&keys[0]], "the pick first again");
}

/// Code review 2026-10-20 P2-1: the open Versions view learns its own
/// playback's failure: the version that did not decode ranks below the
/// others there at once, and the frozen pick that failed is let go.
#[tokio::test]
async fn the_open_view_learns_its_playbacks_failure() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.x264-ONE",
            media_fixture("h264.mp4"),
        ),
        (
            "Show.S01E01.720p.WEB-DL.x264-TWO",
            media_fixture("h264.mp4"),
        ),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    view.freeze();
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[0]);
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.fail_over(None, Some("decode"), None, "no frame");
    until("the next version", || {
        playback.snapshot().key.as_ref() == Some(&keys[1])
    })
    .await;
    let s = view.snapshot();
    assert!(!s.frozen, "the failed pick is let go");
    assert_eq!(s.recommended.unwrap().key, keys[1]);
    let row = s.rows.iter().find(|r| r.key == keys[0]).unwrap();
    assert_eq!(row.line, mq_streams::ReasonLine::DidNotPlay);
}

/// Code review 2026-10-20 P2-2: a decode error after good playback is not
/// the version's: it is not kept, and the playback does not move on.
#[tokio::test]
async fn a_decode_error_after_good_playback_is_not_kept() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.x264-ONE",
            media_fixture("h264.mp4"),
        ),
        (
            "Show.S01E01.720p.WEB-DL.x264-TWO",
            media_fixture("h264.mp4"),
        ),
    ])
    .await;
    let caps = DeviceCaps::basic_1080p();
    let view = versions_for(&core, caps.clone()).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    // A minute of playback, reported every 5 s from where it started.
    for at in (0..=12).map(|i| 600_000 + i * 5_000) {
        playback.report_progress(at, 3_600_000, false);
    }
    playback.fail_over(None, Some("decode"), None, "decoder lost");
    let failed = || playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
    until("the version to fail", failed).await;
    assert!(playback.snapshot().trying.is_none(), "no move on");
    drop((playback, view));

    let view = versions_for(&core, caps).await;
    let s = view.snapshot();
    assert_eq!(s.recommended.unwrap().key, keys[0], "not kept");
}

/// Code review 2026-10-20 r2 P3-1: playback counts from the first position
/// reported, in forward steps: a jump of a minute (a seek, or a source that
/// started further on) is not playback, so a decode error after it still
/// counts against the version.
#[tokio::test]
async fn a_jump_is_not_playback() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.x264-ONE",
            media_fixture("h264.mp4"),
        ),
        (
            "Show.S01E01.720p.WEB-DL.x264-TWO",
            media_fixture("h264.mp4"),
        ),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.report_progress(0, 3_600_000, false);
    playback.report_progress(60_000, 3_600_000, false);
    playback.fail_over(None, Some("decode"), None, "no frame");
    until("the next version", || {
        playback.snapshot().key.as_ref() == Some(&keys[1])
    })
    .await;
    let row = view
        .snapshot()
        .rows
        .into_iter()
        .find(|r| r.key == keys[0])
        .unwrap();
    assert_eq!(row.line, mq_streams::ReasonLine::DidNotPlay);
}

/// plans/stream-ranking.md 4.6: a speed measured while the view is open
/// (a play that stalled) ranks the rows again when the view relearns.
#[tokio::test]
async fn relearn_ranks_with_a_speed_measured_since_the_view_opened() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.x264-ONE\n💾 2.9 GB",
            media_fixture("h264.mp4"),
        ),
        (
            "Show.S01E01.720p.WEB-DL.x264-TWO\n💾 0.3 GB",
            media_fixture("h264.mp4"),
        ),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    // Unmeasured: 8 Mbit/s is under the 15 Mbit/s ceiling.
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[0]);
    let sample = mq_runtime::speed::Sample {
        kind: mq_runtime::speed::Kind::Http,
        bytes: 5_000_000,
        busy: Duration::from_secs(20),
        seeders: None,
    };
    let now = mq_library::hlc::now_ms();
    assert!(mq_runtime::speed::record(
        core.library(),
        core.network(),
        &sample,
        50,
        now
    ));
    // A play's handle relearns, as a stall or failure does (playback.rs).
    view.pick(&keys[1]).handle.relearn();
    // 2 Mbit/s measured: the 8 Mbit/s 1080p cannot keep up, the 720p can.
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[1]);
}

/// plans/stream-ranking.md 4.5 and 4.6, torrents: the line is the higher of
/// the last two p2p samples, so one slow torrent play (40 then 1 Mbit/s)
/// leaves the pick where it is (that swarm is kept back by the failure
/// memory, not the line); a second slow play moves it.
#[cfg(feature = "torrent")]
#[tokio::test]
async fn relearn_after_one_slow_torrent_play_keeps_the_line() {
    let streams = serde_json::json!({ "streams": [
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-ONE\n👤 900 💾 3.0 GB",
          "infoHash": HASH_A, "fileIdx": 0 },
        { "name": "Torrentio\n720p", "title": "Show.S01E01.720p.WEB-DL.x264-TWO\n👤 600 💾 0.5 GB",
          "infoHash": HASH_B, "fileIdx": 0 },
    ]});
    let server = Server::start(vec![
        route(
            "GET",
            "/stream/series/tt1%3A1%3A1.json",
            200,
            &streams.to_string(),
        ),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    let (one, two) = (format!("bt:{HASH_A}:0"), format!("bt:{HASH_B}:0"));
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    // Unmeasured: 7.7 Mbit/s is under the 15 Mbit/s ceiling.
    assert_eq!(view.snapshot().recommended.unwrap().key, one);
    let play_at = |mbps: u64| {
        let sample = mq_runtime::speed::Sample {
            kind: mq_runtime::speed::Kind::P2p,
            // 60 s, so a 1 Mbit/s play still moves the 4 MiB a sample needs.
            bytes: mbps * 60 * 1_000_000 / 8,
            busy: Duration::from_secs(60),
            seeders: Some(900),
        };
        let now = mq_library::hlc::now_ms();
        assert!(mq_runtime::speed::record(
            core.library(),
            core.network(),
            &sample,
            50,
            now
        ));
        view.pick(&two).handle.relearn();
        view.snapshot().recommended.unwrap().key
    };
    assert_eq!(play_at(40), one, "40 Mbit/s measured");
    assert_eq!(play_at(1), one, "40 then 1: the line is still 40");
    // 1 and 1: the line is 1, the 1080p cannot keep up, the 720p pauses least.
    assert_eq!(play_at(1), two, "a second slow play");
}

/// Code review 2026-10-20 r2 P3-3: a torrent file that is not the version
/// listed is kept like a file with no picture, and its version ranks below
/// the ones that can start, read as "Did not play on this device".
#[tokio::test]
async fn a_wrong_file_is_kept_and_ranks_last() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.x264-ONE",
            media_fixture("h264.mp4"),
        ),
        (
            "Show.S01E01.720p.WEB-DL.x264-TWO",
            media_fixture("h264.mp4"),
        ),
    ])
    .await;
    let now = mq_library::hlc::now_ms();
    let episode = (Some(1), Some(1));
    mq_runtime::speed::record_wrong_file(core.library(), &keys[0], episode, now);
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let s = view.snapshot();
    assert_eq!(s.recommended.unwrap().key, keys[1]);
    let row = s.rows.iter().find(|r| r.key == keys[0]).unwrap();
    assert_eq!(row.line, mq_streams::ReasonLine::DidNotPlay);
}

/// A swarm that falls short is cut at its first window when something else
/// can play: for the recommendation with moves left, the next version. A
/// version picked by hand, or one with nothing after it, gets the full
/// deadline.
#[tokio::test]
async fn a_next_version_lets_the_recommendation_cut_a_dead_swarm_early() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.x264-ONE",
            media_fixture("h264.mp4"),
        ),
        (
            "Show.S01E01.720p.WEB-DL.x264-TWO",
            media_fixture("h264.mp4"),
        ),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[0]);
    let recommended = play(&core, &view, &keys[0]);
    until("ready", || recommended.snapshot().url.is_some()).await;
    assert!(recommended.may_cut_early(), "the next version waits");
    let by_hand = play(&core, &view, &keys[1]);
    until("ready", || by_hand.snapshot().url.is_some()).await;
    assert!(
        !by_hand.may_cut_early(),
        "picked by hand: the full deadline"
    );

    let (core, _server, keys) = direct_rows(vec![(
        "Show.S01E01.1080p.WEB-DL.x264-ONE",
        media_fixture("h264.mp4"),
    )])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let only = play(&core, &view, &keys[0]);
    until("ready", || only.snapshot().url.is_some()).await;
    assert!(!only.may_cut_early(), "nothing after it: the full deadline");
}

/// Android walk NV-F2: a version the player could not decode was tried
/// first again on every play. It is remembered on this device: the next
/// Versions view ranks it below the versions that can start, with its
/// line. A network failure is not remembered.
#[tokio::test]
async fn a_version_this_device_could_not_decode_is_not_tried_first_again() {
    let (core, _server, keys) = direct_rows(vec![
        (
            "Show.S01E01.1080p.WEB-DL.x264-ONE",
            media_fixture("h264.mp4"),
        ),
        (
            "Show.S01E01.720p.WEB-DL.x264-TWO",
            media_fixture("h264.mp4"),
        ),
    ])
    .await;
    let caps = DeviceCaps::basic_1080p();
    let fail = |kind: &'static str| {
        let (core, keys, caps) = (core.clone(), keys.clone(), caps.clone());
        async move {
            let view = versions_for(&core, caps).await;
            let playback = play(&core, &view, &keys[0]);
            until("ready", || playback.snapshot().url.is_some()).await;
            playback.fail_over(None, Some(kind), None, "the player failed");
            let moved = || playback.snapshot().attempts.len() == 1;
            until("the failure", moved).await;
        }
    };
    fail("network").await;
    let view = versions_for(&core, caps.clone()).await;
    assert_eq!(
        view.snapshot().recommended.unwrap().key,
        keys[0],
        "a network failure is not kept"
    );
    drop(view);

    fail("decode").await;
    let view = versions_for(&core, caps).await;
    let s = view.snapshot();
    assert_eq!(s.recommended.unwrap().key, keys[1]);
    let row = s.rows.iter().find(|r| r.key == keys[0]).unwrap();
    assert_eq!(row.line, mq_streams::ReasonLine::DidNotPlay);
}

/// Android walk EW-1: the first Versions open after an install kept the
/// addon's numbers. A cold first scrape that answers nothing is asked again
/// a moment later; its counts then reach the rows and, in a frozen view,
/// the pick's own facts, while the pick stays.
#[tokio::test]
async fn a_cold_first_scrape_is_asked_again_and_reaches_a_frozen_pick() {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents()),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    core.set_live_seeders(std::collections::HashMap::from([(HASH_A.to_owned(), 37)]));
    core.miss_live_seeders(1);
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("a pick", || view.snapshot().recommended.is_some()).await;
    let seeds = |r: &mq_runtime::VersionRow| match r.status {
        mq_runtime::VersionStatus::Torrent { seeds } => seeds,
        _ => panic!("a torrent row: {r:?}"),
    };
    let pick = view.snapshot().recommended.unwrap();
    assert_eq!(pick.release_name, "Show.S01E01.1080p.WEB-DL.x264-AAA");
    assert_eq!(
        seeds(&pick),
        Some(900),
        "the addon's number until the counts come"
    );
    assert!(view.counting(), "the second ask is on the way");
    assert!(!view.snapshot().done, "done waits for the counts");
    view.freeze();
    let counted = || {
        view.snapshot()
            .recommended
            .is_some_and(|p| seeds(&p) == Some(37))
    };
    until("the second scrape's counts", counted).await;
    let pick = view.snapshot().recommended.unwrap();
    assert_eq!(
        pick.release_name, "Show.S01E01.1080p.WEB-DL.x264-AAA",
        "frozen: the pick stays"
    );
    assert_eq!(pick.line, mq_streams::ReasonLine::Few { seeders: 37 });
    until("the asks to end", || !view.counting()).await;
    until("done", || view.snapshot().done).await;
}

/// The apps play the pick at `done` (`PlayPick`, the web player, Apple's
/// `settledPick`), so `done` waits for the live counts once every addon
/// answered: the pick it gives is ranked on them. Before, every pick of
/// the audit was read about a second before the counts
/// (plans/research/pick-audit.md).
#[tokio::test]
async fn done_waits_for_the_live_counts() {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents()),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    // A listed 900 its trackers count at 0, the counts a second late as
    // after a cold first scrape.
    core.set_live_seeders(std::collections::HashMap::from([(HASH_A.to_owned(), 0)]));
    core.miss_live_seeders(1);
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("done", || view.snapshot().done).await;
    assert!(!view.counting(), "done came before the counts");
    let pick = view.snapshot().recommended.unwrap();
    assert_eq!(pick.release_name, "Show.S01E01.1080p.WEB-DL.x264-BBB");
}

/// When every version is a swarm whose trackers count one seeder or none,
/// Versions says so before Play. Its attempt keeps the stall windows and
/// the 60 s deadline, a swarm where no peer unchoked by 30 s is let go,
/// and with nothing else to play the deadline does not cut a swarm still
/// sending (race 2: Big Buck Bunny waited 60 s with every row at 0 or 1;
/// code review 2026-10-20 r3 P2-1). One healthy version clears it.
#[cfg(feature = "torrent")]
#[tokio::test]
async fn almost_nobody_sharing_is_said_before_play() {
    let open = |live: [(&'static str, u32); 2]| async move {
        let server = Server::start(vec![
            route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents()),
            route("GET", "/meta/series/tt1.json", 200, &meta()),
        ])
        .await;
        let core = core_with(&server);
        core.set_live_seeders(live.map(|(h, n)| (h.to_owned(), n)).into());
        let view = VersionsView::open(&core, params(TitleFacts::default()));
        until("the live counts", || {
            view.snapshot().done && !view.counting()
        })
        .await;
        let key = view.snapshot().recommended.unwrap().key;
        let target = mq_runtime::PlayTarget::default();
        let playback =
            mq_runtime::PlaybackView::open_picked(&core, view.pick(&key), target, &view.plays());
        (server, view, playback)
    };
    // The start itself follows the first play's policy (a dead swarm
    // ends at 20 s, plans/play-every-version.md 2e), whoever shares.
    let (_server, view, _playback) = open([(HASH_A, 1), (HASH_B, 1)]).await;
    assert!(view.snapshot().nobody_sharing, "{:?}", view.snapshot());
    drop(view);

    let (_server, view, _playback) = open([(HASH_A, 1), (HASH_B, 12)]).await;
    assert!(!view.snapshot().nobody_sharing, "one healthy version");
}

/// Code review 2026-10-20 r4 P3-2, P3-8: once a source of a "Try again" run
/// plays, the run's patience and budget end: a later failover is judged as
/// on a first play (the window cut on, the full cap).
#[cfg(feature = "torrent")]
#[tokio::test]
async fn a_source_that_plays_ends_the_try_again_run() {
    let base = range_origin(media_fixture("h264.mp4"), Duration::ZERO).await;
    let link = |url: String, install: Option<&str>| {
        let json = format!(r#"{{"streams":[{{"url":"{url}"}}]}}"#);
        let wire = mq_protocol::decode_streams(json.as_bytes()).unwrap();
        let mut s = mq_streams::normalize(&wire.streams[0], "t", "T", 0, 0);
        s.addon_url = install.map(|i| url::Url::parse(i).unwrap());
        s
    };
    let install = format!("{base}/manifest.json");
    let sources = vec![
        // From no install, a loopback link is refused at once.
        link("http://127.0.0.1:9/dead.mp4".into(), None),
        link(format!("{base}/a.mp4"), Some(&install)),
        link(format!("{base}/b.mp4"), Some(&install)),
    ];
    let b = sources[2].key.clone();
    let core = Core::temporary("p").unwrap();
    let view = mq_runtime::PlaybackView::open(&core, sources, mq_runtime::PlayTarget::default());
    until("a plays", || view.snapshot().url.is_some()).await;
    view.fail_over(None, Some("network"), None, "gone");
    until("b plays", || view.snapshot().key.as_ref() == Some(&b)).await;
    view.fail_over(None, Some("network"), None, "gone");
    let failed = || view.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
    until("every source failed", failed).await;
    view.retry();
    until("a plays again", || view.snapshot().url.is_some()).await;
    let (windows, _, _, cap) = view.start_rule();
    assert!(
        windows,
        "no longer patient: a source falling short may be cut"
    );
    assert_eq!(cap, Duration::from_secs(180), "no budget left over");
}

/// After a restart the core reads the network the app last reported, not
/// "other", until the app reports one again.
#[tokio::test]
async fn a_restart_keeps_the_last_network() {
    use mq_runtime::speed::Network;
    let dir = std::env::temp_dir().join(format!("mq-network-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let open = || {
        Core::open(mq_runtime::CoreConfig {
            data_dir: dir.clone(),
            profile_id: "p".into(),
            device_id: 0,
        })
        .unwrap()
    };
    let core = open();
    assert_eq!(core.stream_conditions().network, Network::Other);
    core.set_network(Network::Wifi);
    drop(core);
    let core = open();
    assert_eq!(core.stream_conditions().network, Network::Wifi);
    drop(core);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Live: how long the first scrape takes when it also starts the engine,
/// against a second one on a running engine (legally free Blender films;
/// a scrape only, nothing is downloaded). Prints one JSON line for
/// bench/m13/live-seeders.jsonl.
#[cfg(feature = "torrent")]
#[tokio::test]
#[ignore = "live trackers"]
async fn live_the_first_scrape_starts_the_engine() {
    // An opened core asks the real trackers; a temporary one never does.
    let dir = std::env::temp_dir().join(format!("mq-live-{}", std::process::id()));
    let core = Core::open(mq_runtime::CoreConfig {
        data_dir: dir.clone(),
        profile_id: "p".into(),
        device_id: 0,
    })
    .unwrap();
    let swarm = |hash: &str| vec![(hash.to_owned(), Vec::new())];
    let at = Instant::now();
    let sintel = core
        .live_seeders(&swarm("08ada5a7a6183aae1e09d831df6748d566095a10"))
        .await;
    let first_ms = at.elapsed().as_millis();
    let at = Instant::now();
    let tears = core
        .live_seeders(&swarm("209c8226b299b308beaf2b9cd3fb49212dbd13ec"))
        .await;
    let second_ms = at.elapsed().as_millis();
    println!(
        "{}",
        serde_json::json!({
            "first_scrape_ms": first_ms, "second_scrape_ms": second_ms,
            "sintel_seeders": sintel[0], "tears_of_steel_seeders": tears[0],
        })
    );
    drop(core);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Live: what live seeders cost the Versions view on a real answer. Sintel
/// (a legally free film) from the shipped Torrentio link, two rounds, each
/// a fresh core with the trackers' counts and one without: the time to
/// `done`, the pick, and the rows whose count changed. Prints JSON lines
/// for bench/m13/live-seeders.jsonl; nothing is downloaded.
#[cfg(feature = "torrent")]
#[tokio::test]
#[ignore = "live addon and trackers"]
async fn live_versions_done_with_live_seeders() {
    let link = "https://torrentio.strem.fun/providers=yts,eztv,rarbg,1337x,ext,thepiratebay,kickasstorrents,torrentgalaxy,magnetdl,horriblesubs,nyaasi,tokyotosho,anidex,nekobt|qualityfilter=threed,scr,cam|sizefilter=15GB,5GB/manifest.json";
    for round in 0..2 {
        let mut seeds = Vec::new();
        for live in [false, true] {
            let dir =
                std::env::temp_dir().join(format!("mq-done-{}-{round}-{live}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let core = Core::open(mq_runtime::CoreConfig {
                data_dir: dir.clone(),
                profile_id: "p".into(),
                device_id: 0,
            })
            .unwrap();
            core.set_free_bytes(FREE);
            if !live {
                core.set_live_seeders(std::collections::HashMap::new());
            }
            core.install_addon(link).await.unwrap();
            let params = VersionsParams {
                device_language: None,
                kind: "movie".into(),
                video_id: "tt1727587".into(),
                target: Target {
                    title: "Sintel".into(),
                    year: Some(2010),
                    season: None,
                    episode: None,
                },
                caps: DeviceCaps::basic_1080p(),
                prefs: StreamPrefs::default(),
                last_binge_group: None,
                facts: TitleFacts::read(Some("15 min"), Some("2010"), None, None),
            };
            let at = Instant::now();
            let view = VersionsView::open(&core, params);
            until("done", || view.snapshot().done).await;
            let done_ms = at.elapsed().as_millis();
            let rows_of = |s: &mq_runtime::VersionsSnapshot| -> Vec<(String, Option<u32>)> {
                s.rows
                    .iter()
                    .map(|r| match r.status {
                        mq_runtime::VersionStatus::Torrent { seeds } => (r.key.clone(), seeds),
                        _ => (r.key.clone(), None),
                    })
                    .collect()
            };
            // The counts come after `done`: the last change in the rows.
            let mut rows = rows_of(&view.snapshot());
            let mut counted_ms = done_ms;
            while at.elapsed() < Duration::from_millis(2500) {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let now = rows_of(&view.snapshot());
                if now != rows {
                    rows = now;
                    counted_ms = at.elapsed().as_millis();
                }
            }
            let s = view.snapshot();
            println!(
                "{}",
                serde_json::json!({
                    "round": round, "live": live, "done_ms": done_ms, "counted_ms": counted_ms,
                    "rows": rows.len(), "pick": s.recommended.map(|r| r.release_name),
                })
            );
            seeds.push(rows);
            drop(view);
            drop(core);
            let _ = std::fs::remove_dir_all(&dir);
        }
        let changed: Vec<_> = seeds[0]
            .iter()
            .filter_map(|(key, listed)| {
                let live = seeds[1].iter().find(|(k, _)| k == key)?.1;
                (live != *listed).then(|| serde_json::json!({ "listed": listed, "live": live }))
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({ "round": round, "changed": changed })
        );
    }
}

/// Plans/play-every-version.md 2a: a core whose one addon answers
/// `status` with `streams`.
async fn answering(status: u16, streams: &str) -> (Core, Server) {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", status, streams),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    (core_with(&server), server)
}

/// What the view names when nothing can play, with its one action.
async fn empty_of(view: &VersionsView) -> mq_runtime::EmptyVersions {
    let s = view.snapshot();
    assert!(s.recommended.is_none() && s.rows.is_empty(), "{s:?}");
    s.empty.expect("an empty state")
}

fn kind_and_action(e: &mq_runtime::EmptyVersions) -> (EmptyKind, EmptyAction) {
    (e.kind, e.action)
}

/// 2a: no stream addon installed sends the viewer to Settings, Addons; a
/// version listed names nothing.
#[tokio::test]
async fn with_no_source_the_view_says_add_one() {
    let core = Core::temporary("p").unwrap();
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert_eq!(
        kind_and_action(&empty_of(&view).await),
        (EmptyKind::NoSource, EmptyAction::SettingsAddons)
    );
    golden("versions-empty-noSource", &view.snapshot());
    let (core, _server) = answering(200, &torrents()).await;
    if !core.has_engine() {
        core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    }
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert!(view.snapshot().empty.is_none());
}

/// 2a: no addon answering (offline too: no connection, or none in time)
/// offers Try again; with nothing kept on this device there is nothing to
/// play from it. A status is an answer: a 404 or a 500 reads "No source has
/// this yet" (review 2026-10-23 P2-3).
#[tokio::test]
async fn with_no_source_answering_the_view_offers_try_again() {
    let unreached = |empty: &mq_runtime::EmptyVersions| {
        assert_eq!(
            kind_and_action(empty),
            (EmptyKind::Unreachable, EmptyAction::TryAgain)
        );
        assert_eq!(empty.key, None);
    };
    // No connection: the addon's port is closed.
    let core = Core::temporary("p").unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let server = Server::start(vec![]).await;
    let mut row = addon(&server);
    row.transport_url = format!("http://127.0.0.1:{port}/manifest.json");
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    unreached(&empty_of(&view).await);
    golden("versions-empty-unreachable", &view.snapshot());
    // None in time: past the fan-out's hard deadline (12 s).
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents())
            .delayed(Duration::from_secs(14)),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    unreached(&empty_of(&versions_for(&core, DeviceCaps::basic_1080p()).await).await);
    // An answer, though an error.
    for status in [404, 500] {
        let (core, _server) = answering(status, "{}").await;
        let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
        assert_eq!(
            kind_and_action(&empty_of(&view).await),
            (EmptyKind::NoResults, EmptyAction::None),
            "{status}"
        );
    }
}

/// 2a: nothing answered, or only another episode, reads "No source has
/// this yet", with no action.
#[tokio::test]
async fn with_no_version_of_this_video_no_source_has_it() {
    let other = serde_json::json!({ "streams": [
        { "name": "Torrentio\n1080p", "title": "Show.S01E05.1080p.WEB-DL.x264-AAA\n👤 900 💾 2.0 GB",
          "infoHash": HASH_A, "fileIdx": 0 },
    ]});
    for streams in [r#"{"streams":[]}"#.to_owned(), other.to_string()] {
        let (core, _server) = answering(200, &streams).await;
        core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
        let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
        assert_eq!(
            kind_and_action(&empty_of(&view).await),
            (EmptyKind::NoResults, EmptyAction::None),
            "{streams}"
        );
        if streams == r#"{"streams":[]}"# {
            golden("versions-empty-noResults", &view.snapshot());
        }
    }
}

/// 2a: versions found that none of this device's players open are
/// offered dimmed under Choose a version, each still startable.
#[tokio::test]
async fn with_nothing_this_device_plays_each_version_stays_startable() {
    let streams = serde_json::json!({ "streams": [
        { "name": "Direct", "title": "Show.S01E01.1080p.WEB-DL.x265-AAA", "url": "http://127.0.0.1:9/v.mp4" },
    ]});
    let (core, _server) = answering(200, &streams.to_string()).await;
    let caps = DeviceCaps {
        decoders: vec![mq_relparse::VideoCodec::H264],
        has_fallback_engine: false,
        ..DeviceCaps::basic_1080p()
    };
    let view = versions_for(&core, caps).await;
    let empty = empty_of(&view).await;
    assert_eq!(
        (kind_and_action(&empty), empty.count),
        ((EmptyKind::NothingPlaysHere, EmptyAction::ChooseVersion), 1)
    );
    golden("versions-empty-nothingPlaysHere", &view.snapshot());
    let key = &empty.rows[0].key;
    assert_eq!(view.candidates(key).len(), 1, "startable");
}

/// plans/streaming-window-chunks.md: with no free space at all every
/// torrent version still shows, as on Stremio's Fire TV: a ring plays it.
/// Nothing is hidden or blocked for space.
#[tokio::test]
async fn with_no_free_space_every_version_still_shows() {
    let (core, _server) = answering(200, &torrents()).await;
    if !core.has_engine() {
        // Store builds never fetch a swarm.
        return;
    }
    core.set_free_bytes(0);
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let s = view.snapshot();
    let shown = s.rows.len() + usize::from(s.recommended.is_some());
    assert_eq!(
        (shown, s.empty.is_none(), s.blocked.is_none()),
        (2, true, true),
        "{s:?}"
    );
    assert!(s.hidden.by_reason.is_empty(), "{:?}", s.hidden);
}

/// Plans/play-every-version.md 2e, mid-play stalls: the recommendation,
/// playing steadily, stalls; 30 s on, with the network up, it moves to the
/// ranking's next version at the stall's position, once. A second stall
/// shows "Choose another version" instead.
#[tokio::test]
async fn a_stall_of_30s_moves_the_recommendation_on_once_with_its_position() {
    let row = |name| (name, media_fixture("h264.mp4"));
    let (core, _server, keys) = direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.x264-ONE"),
        row("Show.S01E01.720p.WEB-DL.x264-TWO"),
        row("Show.S01E01.480p.WEB-DL.x264-THREE"),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    for at in [10_000, 12_000] {
        playback.report_progress(at, 600_000, false);
    }
    let begun = Instant::now();
    playback.stalled(true, 12_500);
    // A player repeating it while the stall lasts keeps its clock.
    tokio::time::sleep(Duration::from_secs(10)).await;
    playback.stalled(true, 12_500);
    until("the move", || {
        playback.snapshot().key.as_ref() == Some(&keys[1])
    })
    .await;
    assert!(
        begun.elapsed() < Duration::from_secs(36),
        "{:?}",
        begun.elapsed()
    );
    assert!(
        begun.elapsed() >= Duration::from_secs(30),
        "{:?}",
        begun.elapsed()
    );
    let s = playback.snapshot();
    assert_eq!(s.resume_ms, Some(12_500), "the position carries over");
    assert_eq!(
        (&s.attempts[0].key, s.attempts[0].error.as_str()),
        (&keys[0], "The video stopped loading")
    );
    assert_eq!(
        s.moved.as_deref(),
        Some("Playing 720p WEB: the 1080p WEB stopped loading here")
    );
    assert!(!s.choose_another);

    // At most one move a play.
    for at in [13_000, 14_000] {
        playback.report_progress(at, 600_000, false);
    }
    playback.stalled(true, 14_500);
    until("choose another", || playback.snapshot().choose_another).await;
    assert_eq!(playback.snapshot().key.as_ref(), Some(&keys[1]));
    // Playing again takes it away.
    playback.stalled(false, 14_500);
    assert!(!playback.snapshot().choose_another);
}

/// 2e, review 2026-10-23: a stall's watcher stands down when the source
/// changes. The recommendation stalls, then fails over to its other source
/// with no "stalled: false"; 30 s on, the healthy second source is never
/// moved, and the play keeps its one move.
#[tokio::test]
async fn a_stall_ends_with_its_source() {
    let row = |name| (name, media_fixture("h264.mp4"));
    let (core, _server, keys) = direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.x264-ONE"),
        row("Show.S01E01.1080p.WEB-DL.x264-ONE"),
        row("Show.S01E01.720p.WEB-DL.x264-TWO"),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let pick = view.snapshot().recommended.unwrap();
    assert_eq!(pick.failover_count, 1, "two sources of one version");
    let playback = play(&core, &view, &pick.key);
    until("ready", || playback.snapshot().url.is_some()).await;
    for at in [10_000, 12_000] {
        playback.report_progress(at, 600_000, false);
    }
    playback.stalled(true, 12_500);
    let first = playback.snapshot().key.unwrap();
    playback.fail_over(None, Some("network"), None, "gone");
    until("the other source", || {
        playback
            .snapshot()
            .key
            .as_ref()
            .is_some_and(|k| *k != first)
    })
    .await;
    let second = playback.snapshot().key.unwrap();
    assert!(keys[..2].contains(&second), "{second}");
    tokio::time::sleep(Duration::from_secs(33)).await;
    let s = playback.snapshot();
    assert_eq!(s.key.as_ref(), Some(&second), "the healthy source moved");
    assert!(
        s.attempts
            .iter()
            .all(|a| a.error != "The video stopped loading"),
        "{:?}",
        s.attempts
    );
    assert!(!s.choose_another);
}

/// 2e, review 2026-10-23: playing on ends a stall whose "stalled: false"
/// never came. The first stall's clock stops, and a later real stall gets
/// its own 30 s instead of being taken for a repeat.
#[tokio::test]
async fn playing_on_ends_a_stall_the_player_did_not_end() {
    let row = |name| (name, media_fixture("h264.mp4"));
    let (core, _server, keys) = direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.x264-ONE"),
        row("Show.S01E01.720p.WEB-DL.x264-TWO"),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    for at in [10_000, 12_000] {
        playback.report_progress(at, 600_000, false);
    }
    let begun = Instant::now();
    playback.stalled(true, 12_500);
    // It plays on with no "stalled: false", then stalls for real.
    tokio::time::sleep(Duration::from_secs(5)).await;
    for at in [14_000, 16_000] {
        playback.report_progress(at, 600_000, false);
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    playback.stalled(true, 16_500);
    until("the move", || {
        playback.snapshot().key.as_ref() == Some(&keys[1])
    })
    .await;
    let took = begun.elapsed();
    assert!(
        took >= Duration::from_secs(39),
        "the first stall's clock: {took:?}"
    );
    assert_eq!(playback.snapshot().resume_ms, Some(16_500));

    // The one move is spent: the next stall shows the button, and playing
    // on with no "stalled: false" takes it away.
    for at in [17_000, 19_000] {
        playback.report_progress(at, 600_000, false);
    }
    playback.stalled(true, 19_500);
    until("choose another", || playback.snapshot().choose_another).await;
    golden("playback-chooseAnother", &playback.snapshot());
    for at in [20_000, 22_000] {
        playback.report_progress(at, 600_000, false);
    }
    assert!(
        !playback.snapshot().choose_another,
        "the button over a playing video"
    );
}

/// 2e, bench EW-3's shape: a stall right after a seek (17.2 s there) never
/// moves the version, however long it lasts; "Choose another version"
/// shows after 30 s. A version picked by hand never moves either.
#[tokio::test]
async fn a_stall_after_a_seek_or_on_a_hand_pick_never_moves() {
    let row = |name| (name, media_fixture("h264.mp4"));
    let (core, _server, keys) = direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.x264-ONE"),
        row("Show.S01E01.720p.WEB-DL.x264-TWO"),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let seek = play(&core, &view, &keys[0]);
    let hand = play(&core, &view, &keys[1]);
    for p in [&seek, &hand] {
        until("ready", || p.snapshot().url.is_some()).await;
        for at in [10_000, 12_000] {
            p.report_progress(at, 600_000, false);
        }
    }
    // The seek, reported, then the wait for its data.
    seek.report_progress(300_000, 600_000, false);
    seek.stalled(true, 300_000);
    hand.stalled(true, 12_500);
    let begun = Instant::now();
    until("choose another", || {
        seek.snapshot().choose_another && hand.snapshot().choose_another
    })
    .await;
    assert!(
        begun.elapsed() >= Duration::from_secs(29),
        "{:?}",
        begun.elapsed()
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(seek.snapshot().key.as_ref(), Some(&keys[0]));
    assert_eq!(hand.snapshot().key.as_ref(), Some(&keys[1]));
}

/// 2e: a Resume of the remembered release never moves on a stall; the
/// button shows after 30 s.
#[tokio::test]
async fn a_stall_of_the_resume_release_never_moves() {
    let row = |name| (name, media_fixture("h264.mp4"));
    let (core, _server, keys) = direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.x264-ONE"),
        row("Show.S01E01.720p.WEB-DL.x264-TWO"),
    ])
    .await;
    let target = || mq_runtime::PlayTarget {
        kind: "series".into(),
        item_id: "tt1".into(),
        video_id: "tt1:1:1".into(),
        season: Some(1),
        episode: Some(1),
        ..mq_runtime::PlayTarget::default()
    };
    let open = |view: &VersionsView| {
        mq_runtime::PlaybackView::open_picked(&core, view.pick(&keys[0]), target(), &view.plays())
    };
    // Played before, to the middle: the title remembers this release.
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let first = open(&view);
    until("ready", || first.snapshot().url.is_some()).await;
    first.report_progress(600_000, 3_000_000, false);
    drop((first, view));
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let resume = open(&view);
    until("ready", || resume.snapshot().url.is_some()).await;
    assert!(resume.snapshot().resume_ms.is_some(), "a resume");
    for at in [600_000, 602_000] {
        resume.report_progress(at, 3_000_000, false);
    }
    resume.stalled(true, 602_500);
    until("choose another", || resume.snapshot().choose_another).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(resume.snapshot().key.as_ref(), Some(&keys[0]));
}

/// QA r1 Q5 row 23: a file far off the title's runtime (52 min here) is
/// another title, and its position is not saved as the title's progress;
/// one near it is.
#[tokio::test]
async fn a_file_far_off_the_runtime_saves_no_progress() {
    let (core, _server, keys) = direct_rows(vec![(
        "Show.S01E01.1080p.WEB-DL.x264-ONE",
        media_fixture("h264.mp4"),
    )])
    .await;
    let target = || mq_runtime::PlayTarget {
        kind: "series".into(),
        item_id: "tt1".into(),
        video_id: "tt1:1:1".into(),
        season: Some(1),
        episode: Some(1),
        ..mq_runtime::PlayTarget::default()
    };
    let resumes = |duration_ms: u64| {
        let core = &core;
        let key = &keys[0];
        async move {
            let view = versions_for(core, DeviceCaps::basic_1080p()).await;
            let play = mq_runtime::PlaybackView::open_picked(
                core,
                view.pick(key),
                target(),
                &view.plays(),
            );
            until("ready", || play.snapshot().url.is_some()).await;
            play.report_progress(600_000, duration_ms, false);
            drop((play, view));
            let view = versions_for(core, DeviceCaps::basic_1080p()).await;
            let again = mq_runtime::PlaybackView::open_picked(
                core,
                view.pick(key),
                target(),
                &view.plays(),
            );
            until("ready", || again.snapshot().url.is_some()).await;
            again.snapshot().resume_ms
        }
    };
    // Four times the runtime: not saved.
    assert_eq!(resumes(4 * 52 * 60_000).await, None);
    // A 50 min file: saved.
    assert!(resumes(50 * 60_000).await.is_some());
}

/// QA r2 Q5 row 10: the saved position is this title's, so a file far off
/// its runtime (52 min here) does not start there either: the first report
/// of its length takes `resumeMs` away and the apps go to the file's start.
/// One near the runtime keeps it.
#[tokio::test]
async fn a_file_far_off_the_runtime_does_not_start_at_the_saved_position() {
    let (core, _server, keys) = direct_rows(vec![(
        "Show.S01E01.1080p.WEB-DL.x264-ONE",
        media_fixture("h264.mp4"),
    )])
    .await;
    let target = || mq_runtime::PlayTarget {
        kind: "series".into(),
        item_id: "tt1".into(),
        video_id: "tt1:1:1".into(),
        season: Some(1),
        episode: Some(1),
        ..mq_runtime::PlayTarget::default()
    };
    let open = || async {
        let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
        let play = mq_runtime::PlaybackView::open_picked(
            &core,
            view.pick(&keys[0]),
            target(),
            &view.plays(),
        );
        until("ready", || play.snapshot().url.is_some()).await;
        (play, view)
    };
    // A saved position, from a file of the title's length.
    let (play, view) = open().await;
    play.report_progress(600_000, 50 * 60_000, false);
    drop((play, view));
    let (play, _view) = open().await;
    assert_eq!(play.snapshot().resume_ms, Some(600_000));
    play.report_progress(600_000, 50 * 60_000, false);
    assert_eq!(play.snapshot().resume_ms, Some(600_000), "near the runtime");
    drop(play);
    let (play, _view) = open().await;
    play.report_progress(600_000, 4 * 52 * 60_000, false);
    assert_eq!(play.snapshot().resume_ms, None, "four times the runtime");
}

/// 2e: with the network lost (every connection dropped, the addon's too)
/// a stalled recommendation waits instead of moving, the button showing;
/// once a source answers again it moves on.
#[tokio::test]
async fn a_stall_with_the_network_lost_waits_then_moves() {
    let up = Arc::new(AtomicBool::new(true));
    let (mut streams, mut keys) = (Vec::new(), Vec::new());
    for (i, title) in [
        "Show.S01E01.1080p.WEB-DL.x264-ONE",
        "Show.S01E01.720p.WEB-DL.x264-TWO",
    ]
    .into_iter()
    .enumerate()
    {
        let origin =
            range_origin_while(media_fixture("h264.mp4"), Duration::ZERO, up.clone()).await;
        let url = format!("{origin}/v{i}.mp4");
        keys.push(format!("url:{url}"));
        streams.push(serde_json::json!({ "name": "Direct", "title": title, "url": url }));
    }
    let streams = serde_json::json!({ "streams": streams }).to_string();
    let addon = answers_while(streams, meta(), up.clone()).await;
    let core = Core::temporary("p").unwrap();
    core.set_free_bytes(FREE);
    let manifest = serde_json::json!({
        "id": "test", "version": "1.0.0", "name": "Test",
        "types": ["series"], "resources": ["stream", "meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    let row = AddonRow {
        transport_url: format!("{addon}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[0]);
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    for at in [10_000, 12_000] {
        playback.report_progress(at, 600_000, false);
    }
    up.store(false, Ordering::Relaxed);
    playback.stalled(true, 12_500);
    until("choose another", || playback.snapshot().choose_another).await;
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(playback.snapshot().key.as_ref(), Some(&keys[0]), "waits");
    up.store(true, Ordering::Relaxed);
    until("the move", || {
        playback.snapshot().key.as_ref() == Some(&keys[1])
    })
    .await;
}

/// An addon answering `streams` for a stream path and `meta` for anything
/// else while `up`; else it drops every connection.
async fn answers_while(streams: String, meta: String, up: Arc<AtomicBool>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            if !up.load(Ordering::Relaxed) {
                continue;
            }
            let (streams, meta) = (streams.clone(), meta.clone());
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                let body = if head.contains("/stream/") {
                    streams
                } else {
                    meta
                };
                let answer = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(answer.as_bytes()).await;
            });
        }
    });
    base
}

/// Plans/play-every-version.md 2f: links of every quality, one of them the
/// title's last release line.
fn graded() -> String {
    let link = |title: &str, gb: f64, binge: Option<&str>| {
        let mut hints = serde_json::json!({ "videoSize": (gb * 1e9) as u64 });
        if let Some(group) = binge {
            hints["bingeGroup"] = group.into();
        }
        let file = title.replace(' ', ".");
        serde_json::json!({ "name": "Direct", "title": title,
            "url": format!("https://cdn.example/{file}.mkv"), "behaviorHints": hints })
    };
    serde_json::json!({ "streams": [
        link("Show.S01E01.1080p.WEB-DL.x264-AAA", 4.0, None),
        link("Show.S01E01.720p.WEB-DL.x264-BBB", 2.0, Some("bbb-720")),
        link("Show.S01E01.2160p.WEB-DL.x265-CCC", 15.0, None),
        link("Show.S01E01.480p.WEB-DL.x264-EEE", 0.5, None),
        link("Show.S01E01.WEB-DL.x264-DDD", 0.8, None),
    ]})
    .to_string()
}

fn key_of(release: &str) -> String {
    format!("url:https://cdn.example/{release}.mkv")
}

/// 2f: Up Next keeps the title's release line first (its binge group, the
/// 720p one here); "Sharper" is the 1080p one (the 4K one is past this
/// screen), "Smaller file" the first under half the pick's size; the rows
/// fall into the quality sections in the core's order.
#[tokio::test]
async fn up_next_keeps_its_release_line_with_its_shortcuts_and_sections() {
    let (core, _server) = answering(200, &graded()).await;
    let view = VersionsView::open(
        &core,
        VersionsParams {
            device_language: None,
            last_binge_group: Some("bbb-720".into()),
            ..params(TitleFacts::default())
        },
    );
    until("done", || view.snapshot().done).await;
    let s = view.snapshot();
    let name = |r: &Option<mq_runtime::VersionRow>| r.as_ref().map(|r| r.release_name.clone());
    assert_eq!(
        name(&s.recommended),
        Some("Show.S01E01.720p.WEB-DL.x264-BBB".into())
    );
    assert_eq!(
        name(&s.sharper),
        Some("Show.S01E01.1080p.WEB-DL.x264-AAA".into())
    );
    assert_eq!(
        name(&s.smaller),
        Some("Show.S01E01.480p.WEB-DL.x264-EEE".into())
    );
    let sections: Vec<(SectionQuality, Vec<String>)> = s
        .sections
        .iter()
        .map(|x| (x.quality, x.keys.clone()))
        .collect();
    assert_eq!(
        sections,
        [
            (
                SectionQuality::Uhd,
                vec![key_of("Show.S01E01.2160p.WEB-DL.x265-CCC")]
            ),
            (
                SectionQuality::Fhd,
                vec![key_of("Show.S01E01.1080p.WEB-DL.x264-AAA")]
            ),
            (
                SectionQuality::Hd,
                vec![key_of("Show.S01E01.480p.WEB-DL.x264-EEE")]
            ),
            (
                SectionQuality::Other,
                vec![key_of("Show.S01E01.WEB-DL.x264-DDD")]
            ),
        ]
    );

    // A first play: the ranking's best, nothing sharper within this screen.
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    let s = view.snapshot();
    assert_eq!(
        name(&s.recommended),
        Some("Show.S01E01.1080p.WEB-DL.x264-AAA".into())
    );
    assert_eq!(name(&s.sharper), None);
    assert_eq!(
        name(&s.smaller),
        Some("Show.S01E01.480p.WEB-DL.x264-EEE".into())
    );
}

/// 2f: the shortcuts freeze with the pick: a sharper version a slow addon
/// sends after the freeze joins its section, not the shortcut.
#[tokio::test]
async fn the_shortcuts_freeze_with_the_pick() {
    let all: serde_json::Value = serde_json::from_str(&graded()).unwrap();
    let streams = all["streams"].as_array().unwrap();
    let (late, early): (Vec<_>, Vec<_>) = streams
        .iter()
        .cloned()
        .partition(|s| s["title"].as_str().unwrap().contains("1080p"));
    for freeze in [true, false] {
        let fast = Server::start(vec![
            route(
                "GET",
                "/stream/series/tt1%3A1%3A1.json",
                200,
                &serde_json::json!({ "streams": early }).to_string(),
            ),
            route("GET", "/meta/series/tt1.json", 200, &meta()),
        ])
        .await;
        let slow = Server::start(vec![
            route(
                "GET",
                "/stream/series/tt1%3A1%3A1.json",
                200,
                &serde_json::json!({ "streams": late }).to_string(),
            )
            .delayed(Duration::from_secs(2)),
        ])
        .await;
        let core = Core::temporary("p").unwrap();
        core.set_free_bytes(FREE);
        let mut second = addon(&slow);
        second.position = 1;
        second.manifest_json = second
            .manifest_json
            .replace(r#""id":"test""#, r#""id":"slow""#);
        core.library()
            .set_addons("p", &[addon(&fast), second], &core.hlc())
            .unwrap();
        let view = VersionsView::open(
            &core,
            VersionsParams {
                device_language: None,
                last_binge_group: Some("bbb-720".into()),
                ..params(TitleFacts::default())
            },
        );
        until("the pick", || view.snapshot().recommended.is_some()).await;
        assert!(view.snapshot().sharper.is_none());
        if freeze {
            view.freeze();
        }
        until("done", || view.snapshot().done).await;
        let s = view.snapshot();
        assert!(
            s.rows.iter().any(|r| r.release_name.contains("1080p")),
            "{freeze}"
        );
        assert_eq!(s.sharper.is_some(), !freeze, "frozen: {freeze}");
    }
}

/// 2f: a Resume keeps the release it resumes first, though another ranks
/// higher.
#[tokio::test]
async fn a_resume_keeps_the_release_it_resumes() {
    let row = |name| (name, media_fixture("h264.mp4"));
    let (core, _server, keys) = direct_rows(vec![
        row("Show.S01E01.1080p.WEB-DL.x264-ONE"),
        row("Show.S01E01.720p.WEB-DL.x264-TWO"),
    ])
    .await;
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[0]);
    let target = mq_runtime::PlayTarget {
        kind: "series".into(),
        item_id: "tt1".into(),
        video_id: "tt1:1:1".into(),
        season: Some(1),
        episode: Some(1),
        ..mq_runtime::PlayTarget::default()
    };
    let by_hand =
        mq_runtime::PlaybackView::open_picked(&core, view.pick(&keys[1]), target, &view.plays());
    until("ready", || by_hand.snapshot().url.is_some()).await;
    by_hand.report_progress(600_000, 3_000_000, false);
    drop((by_hand, view));
    let view = versions_for(&core, DeviceCaps::basic_1080p()).await;
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[1]);
}

/// 2e: the trackers counted no seeder for any version before Play, so
/// nothing else can start; the one attempt, with no peer and no unchoke,
/// ends at its 20 s dead window instead of the budget's end, and the
/// playback fails as notStarted.
#[cfg(feature = "torrent")]
#[tokio::test]
async fn versions_counted_empty_end_at_20s() {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &torrents()),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    core.set_live_seeders([(HASH_A.to_owned(), 0), (HASH_B.to_owned(), 0)].into());
    let view = VersionsView::open(&core, params(TitleFacts::default()));
    until("the live counts", || {
        view.snapshot().done && !view.counting()
    })
    .await;
    assert!(view.snapshot().nobody_sharing);
    let key = view.snapshot().recommended.unwrap().key;
    let target = mq_runtime::PlayTarget::default();
    let begun = Instant::now();
    let playback =
        mq_runtime::PlaybackView::open_picked(&core, view.pick(&key), target, &view.plays());
    until("the failure", || {
        playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed
    })
    .await;
    let took = begun.elapsed();
    let s = playback.snapshot();
    assert!(
        (Duration::from_secs(19)..Duration::from_secs(26)).contains(&took),
        "{took:?} {:?}",
        s.attempts
    );
    assert_eq!(s.empty, Some(EmptyKind::NotStarted), "{:?}", s.attempts);
    golden::golden_torrent("playback-empty-notStarted", &s);
}

// The kind chips (plans/plugins-and-marketplace.md 4.2, 4.3).

const LINK: &str = "https://cdn.example/Show.S01E01.1080p.WEB-DL.x264-AAA.mkv";

/// One release as a torrent and as a plain link (one row of two kinds), and
/// another torrent: with an account, the torrents are debrid.
fn mixed() -> String {
    serde_json::json!({ "streams": [
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-AAA\n👤 900 💾 2.0 GB",
          "infoHash": HASH_A, "fileIdx": 0 },
        { "name": "Direct", "title": "Show.S01E01.1080p.WEB-DL.x264-AAA\n💾 2.0 GB", "url": LINK },
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-BBB\n👤 100 💾 1.5 GB",
          "infoHash": HASH_B, "fileIdx": 1 },
    ]})
    .to_string()
}

async fn mixed_view() -> (Core, Server, VersionsView) {
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &mixed()),
        route("GET", "/meta/series/tt1.json", 200, &meta()),
    ])
    .await;
    let core = core_with(&server);
    core.set_debrid_keys(&[(StoreId::RealDebrid, "rd".into())]);
    let view = settled_view(&core).await;
    (core, server, view)
}

#[tokio::test]
async fn a_row_of_two_kinds_lists_under_both_chips_and_plays_its_own_source() {
    let (_core, _server, view) = mixed_view().await;
    let all = view.snapshot();
    // The release folds into one row in the pass of every kind.
    assert_eq!(all.rows.len() + 1, 2, "{:?}", all.rows);
    // Chips count rows holding a source of a kind: the mixed row under both.
    let counts: Vec<_> = all.kinds.iter().map(|k| (k.kind, k.count)).collect();
    assert_eq!(counts, [(SourceKind::Direct, 1), (SourceKind::Debrid, 2)]);
    // The torrents name no service, and the one account resolves them: the chip names it.
    assert_eq!(all.kinds[1].service.as_deref(), Some("Real-Debrid"));
    let torrent_a = format!("bt:{HASH_A}:0");

    view.set_kind(Some(SourceKind::Direct));
    let direct = view.snapshot();
    assert_eq!(direct.selected_kind, Some(SourceKind::Direct));
    let card = direct.recommended.clone().unwrap();
    assert_eq!(card.kind, SourceKind::Direct);
    // Its own pass's class, never the torrent head's it was folded with.
    assert_eq!(card.source_class, SourceClass::Ready);
    assert!(direct.rows.is_empty() && direct.smaller.is_none() && direct.sharper.is_none());
    let chain = view.candidates(&card.key);
    assert_eq!(
        chain.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(),
        [card.key.as_str()]
    );
    assert!(card.key.starts_with("url:"), "{}", card.key);

    view.set_kind(Some(SourceKind::Debrid));
    let debrid = view.snapshot();
    let card = debrid.recommended.clone().unwrap();
    assert_eq!(card.key, torrent_a, "the same release, led by its torrent");
    assert_eq!(card.source_class, SourceClass::Unknown);
    assert_eq!(debrid.rows.len(), 1);
    let chain = view.candidates(&torrent_a);
    assert!(chain.iter().all(|s| s.key.starts_with("bt:")), "{chain:?}");
    // The counts stay those of every kind.
    assert_eq!(debrid.kinds, all.kinds);

    view.set_kind(None);
    let back = view.snapshot();
    assert_eq!(back.selected_kind, None);
    assert_eq!(back.rows.len() + 1, 2);
}

#[tokio::test]
async fn a_kind_with_no_rows_lists_every_kind() {
    let (_core, _server, view) = learning_core_view().await;
    view.set_kind(Some(SourceKind::Direct));
    assert_eq!(view.snapshot().selected_kind, None);
    assert!(view.snapshot().recommended.is_some());
}

async fn learning_core_view() -> (Core, Server, VersionsView) {
    let (core, server) = learning_core().await;
    let view = settled_view(&core).await;
    (core, server, view)
}

#[tokio::test]
async fn choosing_a_kind_freezes_the_pick_and_a_pick_under_it_teaches_nothing() {
    let (core, _server, view) = learning_core_view().await;
    assert!(!view.snapshot().frozen);
    view.set_kind(Some(SourceKind::Debrid));
    assert!(view.snapshot().frozen);
    assert_eq!(view.snapshot().selected_kind, Some(SourceKind::Debrid));
    assert!(view.pick(LARGE).note.is_none());
    assert_eq!(core.prefs().learned.fhd.size, 0, "a pick among one kind");
    // The same pick with every kind listed counts.
    let view = settled_view(&core).await;
    view.freeze();
    view.pick(LARGE);
    assert_eq!(core.prefs().learned.fhd.size, 1);
}

#[tokio::test]
async fn under_a_kind_the_next_version_is_of_that_kind() {
    let (_core, _server, view) = mixed_view().await;
    view.set_kind(Some(SourceKind::Direct));
    let link = view.snapshot().recommended.unwrap().key;
    // The only direct link tried: nothing of that kind is left, though a
    // debrid torrent is.
    let pick = view.pick(&link);
    assert!(
        pick.handle
            .next_version(std::slice::from_ref(&link), None)
            .is_none()
    );
    // With every kind listed, the torrent is next.
    view.set_kind(None);
    let pick = view.pick(&link);
    let next = pick
        .handle
        .next_version(std::slice::from_ref(&link), None)
        .unwrap();
    assert!(next.key.starts_with("bt:"), "{}", next.key);
}

#[tokio::test]
async fn an_account_change_lists_every_kind_again() {
    let (core, _server, view) = mixed_view().await;
    view.set_kind(Some(SourceKind::Direct));
    assert_eq!(view.snapshot().selected_kind, Some(SourceKind::Direct));
    // The account goes: the addons are asked again, and the old kind pass
    // goes with what it ranked.
    core.set_debrid_keys(&[]);
    until("every kind listed", || {
        view.snapshot().selected_kind.is_none()
    })
    .await;
    until("done", || view.snapshot().done).await;
    let s = view.snapshot();
    assert_eq!(s.selected_kind, None);
    // No account: an engine build fetches the torrents itself, a store build hides them.
    let listed: Vec<_> = s.kinds.iter().map(|k| k.kind).collect();
    if cfg!(feature = "torrent") {
        assert_eq!(listed, [SourceKind::Torrent, SourceKind::Direct]);
    } else {
        assert_eq!(listed, [SourceKind::Direct]);
    }
}

#[tokio::test]
async fn an_addon_change_lists_every_kind_again() {
    let (core, server, view) = mixed_view().await;
    view.set_kind(Some(SourceKind::Direct));
    assert_eq!(view.snapshot().selected_kind, Some(SourceKind::Direct));
    // The addon is turned off: the old kind pass goes with what it ranked.
    let off = AddonRow {
        enabled: false,
        ..addon(&server)
    };
    core.library().set_addons("p", &[off], &core.hlc()).unwrap();
    until("every kind listed", || {
        view.snapshot().selected_kind.is_none()
    })
    .await;
    let s = view.snapshot();
    assert!(s.recommended.is_none() && s.rows.is_empty(), "{s:?}");
}
