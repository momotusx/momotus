//! Zap's pick (plans/zap.md 1.1 and 1.4): pools per dial, the kind, the
//! episode, the start, no repeats, the fallbacks.

use mq_library::{AddonRow, LibraryItemRow, ProgressRow};
use mq_runtime::{Core, Dial, TraktConfig, TraktHosts, ZapMiss};
use std::collections::HashSet;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Films tt100 to tt119 in the movie catalog (any genre), series tt200 to
/// tt204 in the series catalog; tt1 and tt2 are the profile's own films,
/// tt9 its series. Trakt relates tt1 to tt1, tt2, tt150 and tt151.
async fn fake() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let body = answer(&path);
                let status = if body.is_empty() { 404 } else { 200 };
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    base
}

fn answer(path: &str) -> String {
    let metas = |kind: &str, ids: std::ops::Range<u32>| {
        let metas: Vec<_> = ids
            .map(|i| serde_json::json!({"id": format!("tt{i}"), "type": kind, "name": format!("T{i}")}))
            .collect();
        serde_json::json!({ "metas": metas }).to_string()
    };
    if path.starts_with("/catalog/movie/top") {
        // The profile's own films are listed too: Far must leave them out.
        let mut list: serde_json::Value = serde_json::from_str(&metas("movie", 100..120)).unwrap();
        for own in ["tt1", "tt2"] {
            list["metas"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"id": own, "type": "movie", "name": own}));
        }
        return list.to_string();
    }
    if path.starts_with("/catalog/series/top") {
        return metas("series", 200..205);
    }
    if let Some(id) = path
        .strip_prefix("/meta/movie/")
        .and_then(|p| p.strip_suffix(".json"))
    {
        // tt404's meta is gone (its addon removed); tt119 runs 6 minutes, too short to zap.
        if id == "tt404" {
            return String::new();
        }
        let runtime = if id == "tt119" { "6 min" } else { "100 min" };
        return serde_json::json!({"meta": {"id": id, "type": "movie", "name": format!("Film {id}"), "runtime": runtime}}).to_string();
    }
    if let Some(id) = path
        .strip_prefix("/meta/series/")
        .and_then(|p| p.strip_suffix(".json"))
    {
        // tt204 has only a special: never zapped.
        let videos = if id == "tt204" {
            serde_json::json!([{"id": format!("{id}:0:1"), "title": "Special", "season": 0, "episode": 1, "released": "2020-01-01T00:00:00Z"}])
        } else {
            serde_json::json!([
                {"id": format!("{id}:0:1"), "title": "Special", "season": 0, "episode": 1, "released": "2020-01-01T00:00:00Z"},
                {"id": format!("{id}:1:1"), "title": "Pilot", "season": 1, "episode": 1, "released": "2020-02-01T00:00:00Z"},
                {"id": format!("{id}:1:2"), "title": "Second", "season": 1, "episode": 2, "released": "2020-02-08T00:00:00Z"},
                {"id": format!("{id}:9:1"), "title": "Later", "season": 9, "episode": 1, "released": "2999-01-01T00:00:00Z"}
            ])
        };
        return serde_json::json!({"meta": {"id": id, "type": "series", "name": format!("Show {id}"), "runtime": "44 min", "videos": videos}}).to_string();
    }
    if path.starts_with("/movies/tt1/related") || path.starts_with("/movies/tt2/related") {
        return r#"[{"title":"Own","ids":{"imdb":"tt1"}},{"title":"Own2","ids":{"imdb":"tt2"}},{"title":"A","ids":{"imdb":"tt150"}},{"title":"B","ids":{"imdb":"tt151"}}]"#.into();
    }
    if path.starts_with("/movies/") || path.starts_with("/shows/") {
        return "[]".into();
    }
    String::new()
}

fn addon(base: &str, catalogs: bool) -> AddonRow {
    let list = match catalogs {
        true => serde_json::json!([
            {"type": "movie", "id": "top", "name": "Popular", "extra": [{"name": "genre", "options": ["Drama", "Comedy"]}]},
            {"type": "series", "id": "top", "name": "Popular", "extra": [{"name": "genre", "options": ["Drama"]}]}
        ]),
        false => serde_json::json!([]),
    };
    AddonRow {
        transport_url: format!("{base}/manifest.json"),
        manifest_json: serde_json::json!({
            "id": "meta", "version": "1.0.0", "name": "Meta",
            "types": ["movie", "series"], "resources": ["catalog", "meta"],
            "idPrefixes": ["tt"], "catalogs": list
        })
        .to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

async fn core(dial: &str, catalogs: bool) -> Core {
    let base = fake().await;
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[addon(&base, catalogs)], &core.hlc())
        .unwrap();
    core.set_trakt_config(Some(TraktConfig {
        client_id: "client".into(),
        redirect_uri: "https://marquee.example/trakt".into(),
        hosts: TraktHosts {
            api: base.clone(),
            auth: base.clone(),
            local: true,
        },
    }));
    for (id, kind) in [("tt1", "movie"), ("tt2", "movie"), ("tt9", "series")] {
        let item = LibraryItemRow {
            item_id: id.into(),
            kind: kind.into(),
            name: id.into(),
            poster: None,
            poster_shape: None,
            added_ms: 1,
        };
        core.library()
            .upsert_item("p", &item, true, &core.hlc())
            .unwrap();
    }
    core.set_prefs(&serde_json::json!({ "zapDial": dial }))
        .unwrap();
    core
}

async fn picks(core: &Core, kind: Option<&str>, n: usize) -> Vec<mq_runtime::ZapPick> {
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(core.zap(kind).await.expect("a zap"));
    }
    out
}

#[tokio::test]
async fn same_draws_only_from_the_library_and_history() {
    let core = core("same", true).await;
    for p in picks(&core, None, 12).await {
        assert!(["tt1", "tt2", "tt9"].contains(&p.item_id.as_str()), "{p:?}");
        assert_eq!(p.from, Dial::Same);
    }
}

#[tokio::test]
async fn a_history_episode_filed_as_a_film_is_a_series() {
    let core = core("same", true).await;
    // Played, never in the library: its stored kind falls back to a film.
    let row = ProgressRow {
        video_id: "tt7:1:1",
        item_id: "tt7",
        position_ms: 1,
        duration_ms: 10,
        updated_ms: 1,
    };
    core.library().set_progress("p", row, &core.hlc()).unwrap();
    let seven: Vec<_> = picks(&core, Some("series"), 30)
        .await
        .into_iter()
        .filter(|p| p.item_id == "tt7")
        .collect();
    assert!(!seven.is_empty(), "tt7 never drawn as a series");
    assert!(
        seven
            .iter()
            .all(|p| p.kind == "series" && p.season == Some(1))
    );
}

#[tokio::test]
async fn near_never_deals_a_same_title() {
    let core = core("near", true).await;
    for p in picks(&core, Some("movie"), 8).await {
        assert!(["tt150", "tt151"].contains(&p.item_id.as_str()), "{p:?}");
        assert_eq!(p.from, Dial::Near);
    }
}

#[tokio::test]
async fn far_never_deals_a_same_title_and_keeps_the_kind() {
    let core = core("far", true).await;
    for p in picks(&core, Some("movie"), 20).await {
        assert!(
            !["tt1", "tt2", "tt9"].contains(&p.item_id.as_str()),
            "{p:?}"
        );
        assert_eq!((p.kind.as_str(), p.from), ("movie", Dial::Far));
        assert_eq!(p.video_id, p.item_id);
    }
}

#[tokio::test]
async fn a_series_lands_on_a_released_episode_past_the_specials() {
    let core = core("far", true).await;
    for p in picks(&core, Some("series"), 12).await {
        assert_ne!(p.item_id, "tt204", "a series with only specials");
        assert_eq!(p.season, Some(1), "{p:?}");
        assert!(matches!(p.episode, Some(1 | 2)), "{p:?}");
        assert!(p.episode_name.is_some());
        assert!(p.name.starts_with("Show "));
    }
}

#[tokio::test]
async fn mixed_deals_both_kinds() {
    let core = core("far", true).await;
    let kinds: HashSet<_> = picks(&core, None, 40)
        .await
        .into_iter()
        .map(|p| p.kind)
        .collect();
    assert_eq!(
        kinds,
        HashSet::from(["movie".to_owned(), "series".to_owned()])
    );
}

#[tokio::test]
async fn the_start_sits_in_the_middle() {
    let core = core("far", true).await;
    for p in picks(&core, None, 20).await {
        let runtime = if p.kind == "movie" { 100 } else { 44 } * 60_000;
        assert!(p.start_ms >= runtime * 15 / 100, "{p:?}");
        assert!(p.start_ms <= runtime * 75 / 100, "{p:?}");
    }
}

#[tokio::test]
async fn no_repeat_while_the_pool_holds_more() {
    let core = core("far", true).await;
    let ids: Vec<_> = picks(&core, Some("movie"), 15)
        .await
        .into_iter()
        .map(|p| p.item_id)
        .collect();
    let unique: HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "{ids:?}");
    // The pool is spent: zaps still answer.
    picks(&core, Some("movie"), 3).await;
}

#[tokio::test]
async fn an_empty_library_falls_to_far_and_no_addon_says_so() {
    let base = fake().await;
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[addon(&base, true)], &core.hlc())
        .unwrap();
    let p = core.zap(Some("movie")).await.unwrap();
    assert_eq!(p.from, Dial::Far);

    let bare = Core::temporary("p").unwrap();
    bare.library()
        .set_addons("p", &[addon(&base, false)], &bare.hlc())
        .unwrap();
    assert_eq!(bare.zap(None).await.unwrap_err(), ZapMiss::NoAddons);
}

#[tokio::test]
async fn a_file_too_short_for_the_hold_is_never_dealt() {
    let core = core("far", true).await;
    for p in picks(&core, Some("movie"), 25).await {
        assert_ne!(p.item_id, "tt119", "a 6 minute film");
    }
}

#[tokio::test]
async fn a_dead_title_on_same_moves_the_zap_to_the_next_pool() {
    let base = fake().await;
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[addon(&base, true)], &core.hlc())
        .unwrap();
    let gone = LibraryItemRow {
        item_id: "tt404".into(),
        kind: "movie".into(),
        name: "Gone".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("p", &gone, true, &core.hlc())
        .unwrap();
    core.set_prefs(&serde_json::json!({ "zapDial": "same" }))
        .unwrap();
    let p = core
        .zap(Some("movie"))
        .await
        .expect("a zap from another pool");
    assert_ne!(p.item_id, "tt404");
    assert_eq!(p.from, Dial::Far);
}

/// Zaps on a fresh core with the default addons, live (Pixel walk 2026-11-18:
/// a second zap said "Nothing came up."). One JSON line per zap, and the
/// zap's debug lines for every candidate that dealt nothing.
///
/// `cargo test -p mq-runtime --test zap live -- --ignored --nocapture`
#[tokio::test]
#[ignore = "network: the default addons"]
async fn live_zaps_on_the_default_addons() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .with_test_writer()
        .try_init();
    let core = Core::temporary(mq_library::OWNER).unwrap();
    let all_on: Vec<mq_runtime::defaults::DefaultChoice> = mq_runtime::defaults::list()
        .iter()
        .map(|d| serde_json::from_value(serde_json::json!({ "key": d.key, "on": true })).unwrap())
        .collect();
    core.install_defaults(&all_on).unwrap();
    let mut dealt = 0;
    for (i, dial) in [
        "near", "far", "far", "near", "far", "far", "near", "far", "far", "far",
    ]
    .iter()
    .enumerate()
    {
        core.set_prefs(&serde_json::json!({ "zapDial": dial }))
            .unwrap();
        let started = std::time::Instant::now();
        let got = core.zap(None).await;
        dealt += got.is_ok() as usize;
        println!(
            "{}",
            serde_json::json!({
                "zap": i, "dial": dial, "ok": got.is_ok(), "ms": started.elapsed().as_millis(),
                "from": got.as_ref().ok().map(|p| format!("{:?}", p.from)),
                "kind": got.as_ref().ok().map(|p| p.kind.clone()),
                "miss": got.as_ref().err().map(|m| format!("{m:?}")),
            })
        );
    }
    assert!(dealt >= 9, "only {dealt} of 10 zaps dealt a title");
}

/// Reads `url` (the core's loopback stream) as a player would, until `stop`.
async fn read_stream(url: String, stop: tokio::sync::watch::Receiver<bool>) -> u64 {
    let rest = url.trim_start_matches("http://");
    let (host, path) = rest.split_once('/').unwrap();
    let mut socket = tokio::net::TcpStream::connect(host).await.unwrap();
    let ask = format!("GET /{path} HTTP/1.1\r\nHost: {host}\r\nRange: bytes=0-\r\n\r\n");
    socket.write_all(ask.as_bytes()).await.unwrap();
    let mut buf = vec![0u8; 1 << 16];
    let mut read = 0u64;
    while !*stop.borrow() {
        match tokio::time::timeout(std::time::Duration::from_millis(500), socket.read(&mut buf))
            .await
        {
            Ok(Ok(0)) | Ok(Err(_)) => break,
            Ok(Ok(n)) => read += n as u64,
            Err(_) => {}
        }
    }
    read
}

/// The Pixel walk's slow Zap again (2026-11-18), measured here: three zaps
/// with nothing playing, then a torrent zap played for 30 s and closed, then
/// three zaps at once. One JSON line per step.
///
/// `cargo test -p mq-runtime --features torrent --test zap live_zap_after -- --ignored --nocapture`
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: the default addons and a torrent"]
async fn live_zap_after_a_torrent_play() {
    use mq_runtime::{PlayTarget, PlaybackPhase, PlaybackView, VersionsParams, VersionsView};
    use mq_streams::{DeviceCaps, StreamPrefs, Target};
    let core = Core::temporary(mq_library::OWNER).unwrap();
    let all_on: Vec<mq_runtime::defaults::DefaultChoice> = mq_runtime::defaults::list()
        .iter()
        .map(|d| serde_json::from_value(serde_json::json!({ "key": d.key, "on": true })).unwrap())
        .collect();
    core.install_defaults(&all_on).unwrap();
    core.set_prefs(&serde_json::json!({ "zapDial": "far" }))
        .unwrap();
    let timed = |label: &'static str| {
        let core = core.clone();
        async move {
            let at = std::time::Instant::now();
            let got = core.zap(Some("movie")).await;
            println!(
                "{}",
                serde_json::json!({"step": label, "ok": got.is_ok(), "ms": at.elapsed().as_millis()})
            );
            got
        }
    };
    for _ in 0..3 {
        timed("idle").await.ok();
    }
    // A torrent zap that plays: the first pick whose recommendation reaches ready.
    let mut played = None;
    for _ in 0..5 {
        let Ok(pick) = timed("pick to play").await else {
            continue;
        };
        let versions = VersionsView::open(
            &core,
            VersionsParams {
                kind: pick.kind.clone(),
                video_id: pick.video_id.clone(),
                target: Target {
                    title: pick.name.clone(),
                    year: None,
                    season: None,
                    episode: None,
                },
                caps: DeviceCaps::basic_1080p(),
                prefs: StreamPrefs::default(),
                last_binge_group: None,
                facts: Default::default(),
                device_language: None,
            },
        );
        let mut key = None;
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let s = versions.snapshot();
            if let Some(r) = s.recommended.as_ref().filter(|_| s.done) {
                key = Some(r.key.clone());
                break;
            }
        }
        let Some(key) = key else { continue };
        let target = PlayTarget {
            kind: pick.kind.clone(),
            item_id: pick.item_id.clone(),
            video_id: pick.video_id.clone(),
            name: Some(pick.name.clone()),
            ..PlayTarget::default()
        };
        let playback = PlaybackView::open_zap(
            &core,
            versions.pick(&key),
            target,
            &versions.plays(),
            Some(pick.start_ms),
        );
        let started = std::time::Instant::now();
        while playback.snapshot().phase == PlaybackPhase::Resolving
            && started.elapsed().as_secs() < 90
        {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        if let Some(url) = playback
            .snapshot()
            .url
            .filter(|_| playback.snapshot().phase == PlaybackPhase::Ready)
        {
            played = Some((versions, playback, url));
            break;
        }
    }
    let (versions, playback, url) = played.expect("no torrent zap reached ready");
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let reader = tokio::spawn(read_stream(url, stopped));
    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
    // As Android asks: the next zap while the playing one is still open and read.
    for _ in 0..3 {
        timed("during a play").await.ok();
    }
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    stop.send(true).unwrap();
    let bytes = reader.await.unwrap();
    println!("{}", serde_json::json!({"step": "played", "bytes": bytes}));
    drop(playback);
    drop(versions);
    for _ in 0..3 {
        timed("after a play").await.ok();
    }
}
