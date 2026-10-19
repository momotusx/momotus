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

fn core_with(server: &Server) -> Core {
    let core = Core::temporary("p").unwrap();
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
/// pick, ready on the service. Small files, so an engine build's free space
/// check never hides the other.
#[tokio::test]
async fn a_torrent_the_check_finds_cached_is_ready_before_done() {
    let small = torrents()
        .replace("2.0 GB", "200 MB")
        .replace("1.5 GB", "150 MB");
    let cached =
        format!(r#"{{"success":true,"error":null,"detail":"ok","data":[{{"hash":"{HASH_B}"}}]}}"#);
    let server = Server::start(vec![
        route("GET", "/stream/series/tt1%3A1%3A1.json", 200, &small),
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

/// A loopback origin serving `len` bytes with `Range` support, about
/// 12 MB/s, so a read is in flight long enough to count as a sample.
async fn throttled_origin(len: usize) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
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
                let chunk = vec![0u8; 64 << 10];
                let mut left = body_len;
                while left > 0 {
                    let n = left.min(chunk.len());
                    if socket.write_all(&chunk[..n]).await.is_err() {
                        return;
                    }
                    left -= n;
                    tokio::time::sleep(Duration::from_millis(5)).await;
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
    let base = throttled_origin(6 << 20).await;
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
