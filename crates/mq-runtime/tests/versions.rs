use mq_library::AddonRow;
use mq_runtime::{Core, VersionsParams, VersionsView};
use mq_streams::{DeviceCaps, Reason, StreamPrefs, Target};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn stream_addon(id: &str, delay: Duration, body: &'static str) -> AddonRow {
    named_addon(id, id, delay, body).await
}

async fn named_addon(id: &str, name: &str, delay: Duration, body: &'static str) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": id, "version": "1.0.0", "name": name,
        "types": ["series"], "resources": ["stream"], "catalogs": [], "idPrefixes": ["tt"]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

const TORRENTS: &str = r#"{"streams":[
 {"name":"Torrentio\n1080p","title":"Game.of.Thrones.S01E01.1080p.BluRay.x264-DEMAND\n👤 300 💾 3.1 GB ⚙️ 1337x","infoHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","fileIdx":0},
 {"name":"Torrentio\n720p","title":"Game.of.Thrones.S01E02.720p.HDTV.x264\n👤 20 💾 1.1 GB","infoHash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","fileIdx":0}
]}"#;
const DEBRID: &str = r#"{"streams":[
 {"name":"RD 1080p","title":"Game.of.Thrones.S01E01.1080p.BluRay.x264-DEMAND","clientResolve":{"type":"debrid","service":"realdebrid","infoHash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","fileIdx":0,"isCached":true},"behaviorHints":{"cached":true,"service":"realdebrid","videoSize":3100000000}}
]}"#;

fn params() -> VersionsParams {
    VersionsParams {
        kind: "series".into(),
        video_id: "tt0944947:1:1".into(),
        target: Target {
            title: "Game of Thrones".into(),
            year: None,
            season: Some(1),
            episode: Some(1),
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
    }
}

#[tokio::test]
async fn ranks_union_progressively_and_freezes_the_pick() {
    mq_relparse::warm_up();
    let core = Core::temporary("p").unwrap();
    let torrents = stream_addon("torrents", Duration::ZERO, TORRENTS).await;
    let debrid = stream_addon("debrid", Duration::from_millis(800), DEBRID).await;
    let installs = [debrid.transport_url.clone(), torrents.transport_url.clone()];
    core.library()
        .set_addons("p", &[torrents, debrid], &core.hlc())
        .unwrap();

    let view = VersionsView::open(&core, params());
    let (first, mut updates) = view.subscribe();
    use futures_util::StreamExt;
    let mut seen = vec![first];
    while !seen.last().unwrap().done {
        let batch = tokio::time::timeout(Duration::from_secs(15), updates.next())
            .await
            .expect("update")
            .expect("stream open");
        seen.extend(batch);
    }
    // First the torrent addon answered: its 1080p release was recommended.
    let early = seen
        .iter()
        .find(|s| s.pending_addons == ["debrid"] && s.recommended.is_some())
        .expect("a snapshot with only the torrent result");
    assert_eq!(early.recommended.as_ref().unwrap().via, "torrents");
    assert_eq!(early.hidden.count, 1, "S01E02 is the wrong episode");
    // Then the cached debrid copy of the same release won and absorbed it.
    let last = seen.last().unwrap();
    let best = last.recommended.as_ref().unwrap();
    assert_eq!(best.via, "Real-Debrid");
    assert!(best.reasons.contains(&Reason::ReadyNow));
    assert_eq!(best.failover_count, 1);
    assert!(
        last.rows.is_empty(),
        "the torrent duplicate collapsed into the pick"
    );
    assert_eq!(&view.snapshot(), last, "the stream ends on the view state");
    // The pick and its failover each carry the install that listed them.
    let listed_by: Vec<String> = view
        .candidates(&best.key)
        .iter()
        .map(|c| {
            c.addon_url
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(listed_by, installs);
}

const FIRST: &str = r#"{"streams":[
 {"name":"First","title":"Game.of.Thrones.S01E01.1080p.BluRay.x264-AAA","infoHash":"1111111111111111111111111111111111111111","fileIdx":0}
]}"#;
const SECOND: &str = r#"{"streams":[
 {"name":"Second","title":"Game.of.Thrones.S01E01.1080p.WEB-DL.x264-BBB","infoHash":"2222222222222222222222222222222222222222","fileIdx":0}
]}"#;

#[tokio::test]
async fn installs_sharing_a_manifest_id_keep_their_own_name_rank_and_progress() {
    mq_relparse::warm_up();
    let core = Core::temporary("p").unwrap();
    // The first install answers after the fan-out's 2.5 s soft deadline, so
    // it alone becomes slow; it is not the last install with that id.
    let first = named_addon("same", "First", Duration::from_secs(3), FIRST).await;
    let second = named_addon("same", "Second", Duration::ZERO, SECOND).await;
    let (first_url, second_url) = (first.transport_url.clone(), second.transport_url.clone());
    // A third install of the same id that nobody serves: it fails.
    let manifest = serde_json::json!({
        "id": "same", "version": "1.0.0", "name": "Broken",
        "types": ["series"], "resources": ["stream"], "catalogs": [], "idPrefixes": ["tt"]
    });
    let broken = AddonRow {
        transport_url: "http://127.0.0.1:9/manifest.json".into(),
        manifest_json: manifest.to_string(),
        position: 1,
        enabled: true,
        allow_private: true,
    };
    let second = AddonRow {
        position: 2,
        ..second
    };
    core.library()
        .set_addons("p", &[first, broken, second], &core.hlc())
        .unwrap();

    let view = VersionsView::open(&core, params());
    let (first_snapshot, mut updates) = view.subscribe();
    use futures_util::StreamExt;
    let mut seen = vec![first_snapshot];
    while !seen.last().unwrap().done {
        let batch = tokio::time::timeout(Duration::from_secs(15), updates.next())
            .await
            .expect("update")
            .expect("stream open");
        seen.extend(batch);
    }
    // The others answered or failed first: only the first is still pending,
    // then slow, until its own arrival clears it.
    let progress: Vec<_> = seen
        .iter()
        .map(|s| (&s.pending_addons, &s.slow_addons))
        .collect();
    assert!(
        seen.iter().any(|s| s.pending_addons == ["First"]),
        "{progress:?}"
    );
    assert!(
        seen.iter().any(|s| s.slow_addons == ["First"]),
        "{progress:?}"
    );
    let last = seen.last().unwrap();
    assert!(last.pending_addons.is_empty() && last.slow_addons.is_empty());
    assert_eq!(last.failed_addons, ["Broken"]);
    // Every candidate carries its own install's URL, name and rank.
    let keys = last
        .recommended
        .iter()
        .chain(&last.rows)
        .map(|r| r.key.clone());
    let candidates: Vec<_> = keys.flat_map(|k| view.candidates(&k)).collect();
    let mut by_install: Vec<(String, String, u16)> = candidates
        .iter()
        .map(|c| {
            let url = c
                .addon_url
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default();
            (url, c.addon_name.clone(), c.addon_rank)
        })
        .collect();
    by_install.sort();
    let mut want = vec![
        (first_url, "First".to_owned(), 0),
        (second_url, "Second".to_owned(), 2),
    ];
    want.sort();
    assert_eq!(by_install, want);
}

#[tokio::test]
async fn frozen_pick_survives_late_results() {
    mq_relparse::warm_up();
    let core = Core::temporary("p").unwrap();
    let torrents = stream_addon("torrents", Duration::ZERO, TORRENTS).await;
    let debrid = stream_addon("debrid", Duration::from_millis(800), DEBRID).await;
    core.library()
        .set_addons("p", &[torrents, debrid], &core.hlc())
        .unwrap();
    let view = VersionsView::open(&core, params());
    while view.snapshot().recommended.is_none() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    view.freeze();
    while !view.snapshot().done {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let snap = view.snapshot();
    assert!(snap.frozen);
    assert_eq!(
        snap.recommended.unwrap().via,
        "torrents",
        "the pick under focus did not move"
    );
    assert!(view.recommended().is_some());
}

/// Focus can move before any addon answered; that must not freeze an empty
/// pick for good.
#[tokio::test]
async fn freezing_before_any_answer_keeps_the_first_pick_coming() {
    mq_relparse::warm_up();
    let core = Core::temporary("p").unwrap();
    let torrents = stream_addon("torrents", Duration::from_millis(300), TORRENTS).await;
    core.library()
        .set_addons("p", &[torrents], &core.hlc())
        .unwrap();
    let view = VersionsView::open(&core, params());
    view.freeze();
    assert!(!view.snapshot().frozen);
    while !view.snapshot().done {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(view.snapshot().recommended.unwrap().via, "torrents");
}

#[tokio::test]
#[ignore = "network"]
async fn real_torrentio_versions() {
    let core = Core::temporary("p").unwrap();
    core.install_addon("https://torrentio.strem.fun/manifest.json")
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let view = VersionsView::open(&core, params());
    while !view.snapshot().done && started.elapsed() < Duration::from_secs(15) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let snap = view.snapshot();
    eprintln!(
        "versions in {:?}: {} rows, hidden {}",
        started.elapsed(),
        snap.rows.len(),
        snap.hidden.count
    );
    let best = snap.recommended.expect("a pick");
    eprintln!(
        "pick: {} {:?} {:?} {} reasons {:?}",
        best.quality, best.features, best.size_bytes, best.release_name, best.reasons
    );
    for row in snap.rows.iter().take(5) {
        eprintln!(
            "  {:?} {} {:?} {}",
            row.group, row.quality, row.features, row.release_name
        );
    }
    assert!(snap.rows.len() > 5);
}

const TWO_GROUPS: &str = r#"{"streams":[
 {"name":"Torrentio\n1080p","title":"Game.of.Thrones.S01E01.1080p.BluRay.x264-FLUX\n👤 900 💾 3.0 GB","infoHash":"cccccccccccccccccccccccccccccccccccccccc","fileIdx":0,"behaviorHints":{"bingeGroup":"torrentio|1080p|FLUX"}},
 {"name":"Torrentio\n1080p","title":"Game.of.Thrones.S01E01.1080p.BluRay.x264-NTb\n👤 40 💾 3.1 GB","infoHash":"dddddddddddddddddddddddddddddddddddddddd","fileIdx":0,"behaviorHints":{"bingeGroup":"torrentio|1080p|NTb"}}
]}"#;

#[tokio::test]
async fn remembers_the_release_line_played_last_for_the_title() {
    mq_relparse::warm_up();
    let core = Core::temporary("p").unwrap();
    let addon = stream_addon("torrents", Duration::ZERO, TWO_GROUPS).await;
    core.library()
        .set_addons("p", &[addon], &core.hlc())
        .unwrap();
    let pick = |core: &Core| {
        let core = core.clone();
        async move {
            let view = VersionsView::open(&core, params());
            while !view.snapshot().done {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            view.snapshot().recommended.unwrap()
        }
    };
    // With no memory the better seeded release wins.
    assert!(pick(&core).await.release_name.contains("FLUX"));
    // After an episode played from the NTb line, that line is preferred.
    core.library()
        .set_stream_memory(
            "p",
            "tt0944947",
            Some("torrentio|1080p|NTb"),
            "{}",
            &core.hlc(),
        )
        .unwrap();
    let best = pick(&core).await;
    assert!(best.release_name.contains("NTb"), "{best:?}");
    assert!(best.reasons.contains(&Reason::SameReleaseAsLast));
}

/// Waits until `check` holds for the view's snapshot, or fails after 10 s.
async fn until(
    view: &VersionsView,
    what: &str,
    check: impl Fn(&mq_runtime::views::versions::VersionsSnapshot) -> bool,
) {
    for _ in 0..200 {
        if check(&view.snapshot()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("never: {what}; last {:?}", view.snapshot());
}

/// A source installed while the title is open shows up without reopening it,
/// and a source removed takes its versions with it.
#[tokio::test]
async fn versions_follow_installed_addons() {
    mq_relparse::warm_up();
    let core = Core::temporary("p").unwrap();
    let view = VersionsView::open(&core, params());
    until(&view, "done with no addon", |s| {
        s.done && s.recommended.is_none()
    })
    .await;

    let first = stream_addon("first", Duration::ZERO, FIRST).await;
    core.library()
        .set_addons("p", std::slice::from_ref(&first), &core.hlc())
        .unwrap();
    until(&view, "a pick from the new install", |s| {
        s.done && s.recommended.as_ref().is_some_and(|r| r.via == "first")
    })
    .await;

    core.library().set_addons("p", &[], &core.hlc()).unwrap();
    until(&view, "nothing once it is removed", |s| {
        s.done && s.recommended.is_none() && s.rows.is_empty()
    })
    .await;
}

/// A pick under focus stays through another install, and lets go when its own
/// addon is removed.
#[tokio::test]
async fn a_frozen_pick_stays_until_its_addon_goes() {
    mq_relparse::warm_up();
    let core = Core::temporary("p").unwrap();
    let first = stream_addon("first", Duration::ZERO, FIRST).await;
    core.library()
        .set_addons("p", std::slice::from_ref(&first), &core.hlc())
        .unwrap();
    let view = VersionsView::open(&core, params());
    until(&view, "the first pick", |s| s.recommended.is_some()).await;
    view.freeze();

    let second = stream_addon("second", Duration::from_millis(100), SECOND).await;
    core.library()
        .set_addons("p", &[second.clone(), first.clone()], &core.hlc())
        .unwrap();
    until(&view, "both answered", |s| {
        s.done && s.pending_addons.is_empty() && !s.rows.is_empty()
    })
    .await;
    let snap = view.snapshot();
    assert!(snap.frozen);
    assert_eq!(
        snap.recommended.unwrap().via,
        "first",
        "the pick under focus did not move"
    );

    core.library()
        .set_addons("p", std::slice::from_ref(&second), &core.hlc())
        .unwrap();
    until(&view, "a new pick from what is left", |s| {
        s.done && !s.frozen && s.recommended.as_ref().is_some_and(|r| r.via == "second")
    })
    .await;
}
