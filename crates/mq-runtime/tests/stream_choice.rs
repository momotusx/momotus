//! Stream choice in the runtime (plans/m13-stream-choice.md 4.2, 3.11):
//! what the view reads (the app's facts, the meta, the cache check) and
//! what it says when nothing can play.

#[path = "../../mq-debrid/tests/common/mod.rs"]
mod common;

use common::{Server, route};
use mq_library::AddonRow;
use mq_runtime::{BlockedReason, Core, StoreId, TitleFacts, VersionsParams, VersionsView};
use mq_streams::{DeviceCaps, SourceClass, StreamPrefs, Target};
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
/// hidden, and the view says an account opens them, with those versions
/// ranked as they would be.
#[tokio::test]
async fn without_engine_or_account_versions_say_an_account_opens_them() {
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
    assert!(s.recommended.is_none() && s.rows.is_empty(), "{s:?}");
    let blocked = s.blocked.expect("blocked");
    assert_eq!(
        (blocked.reason, blocked.count),
        (BlockedReason::NeedsAccount, 2)
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
    // `done` does not wait for the counts; they rank the rows when they come.
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

/// Learning (3.9): two 1080p torrents, R small and well seeded (the
/// recommendation), C three times its size.
fn sizes() -> String {
    serde_json::json!({ "streams": [
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-SMALL\n👤 900 💾 1.2 GB",
          "infoHash": HASH_A, "fileIdx": 0 },
        { "name": "Torrentio\n1080p", "title": "Show.S01E01.1080p.WEB-DL.x264-LARGE\n👤 300 💾 4.0 GB",
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
    view.pick(&recommended).handle.failed();
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
    // A long episode, so the 4 GB file keeps up and its size is the news.
    let facts = TitleFacts::read(Some("120 min"), Some("2019-"), Some("2019-05-01"), Some(2));
    let view = VersionsView::open(
        &core,
        VersionsParams {
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
    let trying = s.trying.expect("the version it moved on to");
    assert_eq!((&trying.key, trying.label.as_str()), (fine, "720p WEB"));
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

/// The recommendation moves on at most twice, then stops with the
/// failure card: the fourth version is never started.
#[tokio::test]
async fn the_recommendation_moves_on_at_most_twice() {
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
    let failed = || playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
    until("the last move to fail", failed).await;
    let s = playback.snapshot();
    let tried: Vec<&String> = s.attempts.iter().map(|a| &a.key).collect();
    assert_eq!(tried, [&keys[0], &keys[1], &keys[2]]);
    assert_eq!(s.trying.map(|t| t.key), Some(keys[2].clone()));
    assert!(s.offer.is_none());
}

/// Only a failure of the version's own moves on: the player's "decode"
/// does, a network failure does not.
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
    let view = versions_for(&core, caps.clone()).await;
    assert_eq!(view.snapshot().recommended.unwrap().key, keys[0]);
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.fail_over(None, Some("network"), None, "offline");
    let failed = || playback.snapshot().phase == mq_runtime::PlaybackPhase::Failed;
    until("the version to fail", failed).await;
    let s = playback.snapshot();
    assert!(s.trying.is_none() && s.offer.is_none());
    drop((playback, view));

    let view = versions_for(&core, caps).await;
    let playback = play(&core, &view, &keys[0]);
    until("ready", || playback.snapshot().url.is_some()).await;
    playback.fail_over(None, Some("decode"), None, "no frame");
    let moved = || playback.snapshot().key.as_ref() == Some(&keys[1]);
    until("the next version", moved).await;
    assert_eq!(
        playback.snapshot().trying.map(|t| t.key),
        Some(keys[1].clone())
    );
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
    until("done", || view.snapshot().done).await;
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
}

/// When every version is a swarm whose trackers count one seeder or none,
/// Versions says so before Play and the attempt gives up after the first
/// 30 s window instead of 60 s (race 2: Big Buck Bunny waited 60 s with
/// every row at 0 or 1). One healthy version clears it.
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
    let (_server, view, playback) = open([(HASH_A, 1), (HASH_B, 1)]).await;
    assert!(view.snapshot().nobody_sharing, "{:?}", view.snapshot());
    assert_eq!(playback.start_deadline(), Duration::from_secs(30));
    drop((playback, view));

    let (_server, view, playback) = open([(HASH_A, 1), (HASH_B, 12)]).await;
    assert!(!view.snapshot().nobody_sharing, "one healthy version");
    assert_eq!(playback.start_deadline(), Duration::from_secs(60));
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
