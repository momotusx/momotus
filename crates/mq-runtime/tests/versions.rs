use mq_library::AddonRow;
use mq_runtime::{Core, VersionsParams, VersionsView};
use mq_streams::{DeviceCaps, Reason, StreamPrefs, Target};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn stream_addon(id: &str, delay: Duration, body: &'static str) -> AddonRow {
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
        "id": id, "version": "1.0.0", "name": id,
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
