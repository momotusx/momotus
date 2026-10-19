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

async fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
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
/// pick, ready on the service.
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
    let best = s.recommended.expect("a pick");
    assert_eq!(best.release_name, "Show.S01E01.1080p.WEB-DL.x264-BBB");
    assert_eq!(best.source_class, SourceClass::Ready);
    // The other one is not cached: the engine fetches it, else the
    // service has to download it first.
    let other = &s.rows[0];
    let expected = if core.has_engine() {
        SourceClass::Swarm
    } else {
        SourceClass::Fetch
    };
    assert_eq!(other.source_class, expected);
    // The pick is then warmed, which asks the service once more.
    assert!(server.count("POST", "/v1/api/torrents/checkcached") >= 1);
}
