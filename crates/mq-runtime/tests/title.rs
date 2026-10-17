use futures_util::StreamExt;
use mq_library::{AddonRow, ProgressRow};
use mq_runtime::views::title::{TitleSnapshot, TitleState};
use mq_runtime::{Core, TitleUpdate, TitleView};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn meta_addon(body: &'static str) -> AddonRow {
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
        "id": "meta.test", "version": "1.0.0", "name": "Meta Test",
        "types": ["series"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

const SHOW: &str = r#"{"meta":{
  "id":"tt0944947","type":"series","name":"Game of Thrones","releaseInfo":"2011-2019",
  "description":"Nine noble families fight for control.",
  "background":"https://images.metahub.space/background/medium/tt0944947/img",
  "links":[{"name":"Drama","category":"Genres","url":"stremio:///discover"},{"name":"Emilia Clarke","category":"Cast","url":"stremio:///search"},{"name":"9.2","category":"imdb","url":"https://imdb.com/title/tt0944947"}],
  "videos":[
    {"id":"tt0944947:0:1","title":"Special","season":0,"episode":1,"released":"2011-01-01T00:00:00.000Z"},
    {"id":"tt0944947:1:1","title":"Winter Is Coming","season":1,"episode":1,"released":"2011-04-17T00:00:00.000Z"},
    {"id":"tt0944947:1:2","title":"The Kingsroad","season":1,"episode":2,"released":"2011-04-24T00:00:00.000Z"},
    {"id":"tt0944947:2:1","title":"The North Remembers","season":2,"episode":1,"released":"2012-04-01T00:00:00.000Z"}
  ]}}"#;

async fn settle(view: &TitleView) -> TitleSnapshot {
    let (_, mut updates) = view.subscribe();
    for _ in 0..50 {
        let snap = view.snapshot();
        if snap.header.state != TitleState::Loading {
            return snap;
        }
        let _ = tokio::time::timeout(Duration::from_millis(100), updates.next()).await;
    }
    view.snapshot()
}

#[tokio::test]
async fn resumes_the_season_being_watched() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(SHOW).await], &core.hlc())
        .unwrap();
    core.library()
        .set_watched("p", "tt0944947", "tt0944947:1:1", true, &core.hlc())
        .unwrap();
    core.library()
        .set_watched("p", "tt0944947", "tt0944947:1:2", true, &core.hlc())
        .unwrap();
    let p = ProgressRow {
        video_id: "tt0944947:2:1",
        item_id: "tt0944947",
        position_ms: 1_000,
        duration_ms: 3_000,
        updated_ms: 9,
    };
    core.library().set_progress("p", p, &core.hlc()).unwrap();

    let view = TitleView::open(&core, "series", "tt0944947");
    let snap = settle(&view).await;
    let details = snap.header.details.clone().unwrap();
    assert_eq!(details.genres, ["Drama"]);
    assert_eq!(details.cast, ["Emilia Clarke"]);
    assert_eq!(details.rating.as_deref(), Some("9.2"));
    assert_eq!(snap.header.seasons, [0, 1, 2]);
    assert_eq!(snap.header.selected_season, Some(2));
    let target = snap.header.play_target.unwrap();
    assert_eq!(target.label, "Resume S2:E1");
    assert_eq!(target.resume_ms, Some(1000));
    assert_eq!(snap.episodes.len(), 1);
    assert!((snap.episodes[0].progress.unwrap() - 1.0 / 3.0).abs() < 1e-4);

    view.select_season(1);
    let snap = view.snapshot();
    assert_eq!(
        snap.episodes.iter().map(|e| e.watched).collect::<Vec<_>>(),
        [true, true]
    );
}

#[tokio::test]
async fn watched_and_library_changes_flow_back_live() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(SHOW).await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "series", "tt0944947");
    let snap = settle(&view).await;
    assert_eq!(snap.header.play_target.unwrap().label, "Play S1:E1");
    assert!(!snap.header.in_library);

    let (_, mut updates) = view.subscribe();
    view.set_watched("tt0944947:1:1", true).unwrap();
    view.set_in_library(true).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let snap = view.snapshot();
        if snap.header.in_library && snap.header.play_target.as_ref().unwrap().label == "Play S1:E2"
        {
            assert!(snap.episodes[0].watched);
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "updates did not arrive"
        );
        if let Ok(Some(batch)) =
            tokio::time::timeout(Duration::from_millis(200), updates.next()).await
        {
            assert!(
                batch.iter().all(|u| matches!(
                    u,
                    TitleUpdate::Header { .. } | TitleUpdate::Episodes { .. }
                ))
            );
        }
    }
}

#[tokio::test]
#[ignore = "network"]
async fn real_cinemeta_title() {
    let core = Core::temporary("p").unwrap();
    core.install_addon("https://v3-cinemeta.strem.io/manifest.json")
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let view = TitleView::open(&core, "series", "tt0944947");
    let snap = settle(&view).await;
    eprintln!(
        "title ready in {:?}: {:?} seasons {:?}, {} episodes in S{:?}",
        started.elapsed(),
        snap.header.details.as_ref().map(|d| &d.name),
        snap.header.seasons,
        snap.episodes.len(),
        snap.header.selected_season
    );
    assert_eq!(snap.header.state, TitleState::Ready);
    assert!(snap.episodes.len() >= 10);
}
