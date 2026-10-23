use futures_util::StreamExt;
use mq_library::{AddonRow, ProgressRow};
use mq_runtime::views::title::{TitleSnapshot, TitleState};
use mq_runtime::{Core, TitleUpdate, TitleView};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn meta_addon(body: &'static str) -> AddonRow {
    addon_answering("200 OK", body).await
}

/// A meta addon whose every answer has this status line and body.
async fn addon_answering(status: &'static str, body: &'static str) -> AddonRow {
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
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "meta.test", "version": "1.0.0", "name": "Meta Test",
        "types": ["series", "movie"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
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

/// Core backlog C37: the title's year line has no dash ("2011-2019" from
/// the addon).
#[tokio::test]
async fn the_years_read_from_to() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(SHOW).await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "series", "tt0944947");
    let snap = settle(&view).await;
    let details = snap.header.details.expect("details");
    assert_eq!(details.release_info.as_deref(), Some("2011 to 2019"));
}

/// Core backlog C41: a series is Watched once every aired episode of its
/// regular seasons is (the special left out), and `setWatched` on the
/// series' id marks or unmarks them all.
#[tokio::test]
async fn a_series_is_watched_as_a_whole() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(SHOW).await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "series", "tt0944947");
    let snap = settle(&view).await;
    assert!(!snap.header.watched);
    let watched = |core: &Core| {
        let mut v = core.library().watched_videos("p", "tt0944947").unwrap();
        v.sort();
        v
    };
    view.set_watched("tt0944947", true).unwrap();
    assert_eq!(
        watched(&core),
        ["tt0944947:1:1", "tt0944947:1:2", "tt0944947:2:1"],
        "every regular episode, not the special"
    );
    let (_, mut updates) = view.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !view.snapshot().header.watched {
        assert!(tokio::time::Instant::now() < deadline, "not marked watched");
        let _ = tokio::time::timeout(Duration::from_millis(200), updates.next()).await;
    }
    // One episode unmarked: the series is not watched any more.
    view.set_watched("tt0944947:2:1", false).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while view.snapshot().header.watched {
        assert!(tokio::time::Instant::now() < deadline, "still watched");
        let _ = tokio::time::timeout(Duration::from_millis(200), updates.next()).await;
    }
    view.set_watched("tt0944947", false).unwrap();
    assert!(watched(&core).is_empty());
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
    assert_eq!(target.left_ms, Some(2000), "from the stored duration");
    assert_eq!(snap.episodes.len(), 1);
    assert!((snap.episodes[0].progress.unwrap() - 1.0 / 3.0).abs() < 1e-4);
    assert_eq!(snap.episodes[0].left_ms, Some(2000));

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

const MOVIE: &str = r#"{"meta":{"id":"tt1727587","type":"movie","name":"Sintel",
  "trailerStreams":[{"title":"Official trailer","ytId":"eRsGyueVLvQ"}]}}"#;

/// A movie page can offer its trailer and Mark watched.
#[tokio::test]
async fn a_movie_offers_its_trailer_and_a_watched_flag() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(MOVIE).await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "movie", "tt1727587");
    let snap = settle(&view).await;
    let details = snap.header.details.expect("details");
    assert_eq!(details.trailers.len(), 1, "{:?}", details.trailers);
    let trailer = &details.trailers[0];
    assert_eq!(trailer.youtube_id, "eRsGyueVLvQ");
    assert_eq!(trailer.url, "https://www.youtube.com/watch?v=eRsGyueVLvQ");
    assert_eq!(trailer.title, "Official trailer");
    assert!(!snap.header.watched);

    view.set_watched("tt1727587", true).unwrap();
    for _ in 0..100 {
        if view.snapshot().header.watched {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the watched flag never came back");
}

/// A movie marked watched after a partial play starts over; a rewatch
/// started after the mark resumes again, as Continue Watching shows it.
#[tokio::test]
async fn a_watched_movie_plays_again_until_a_rewatch_resumes() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(MOVIE).await], &core.hlc())
        .unwrap();
    let progress = |position_ms, updated_ms| ProgressRow {
        video_id: "tt1727587",
        item_id: "tt1727587",
        position_ms,
        duration_ms: 888_000,
        updated_ms,
    };
    core.library()
        .set_progress("p", progress(400_000, 1), &core.hlc())
        .unwrap();
    core.library()
        .set_watched("p", "tt1727587", "tt1727587", true, &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "movie", "tt1727587");
    let target = settle(&view).await.header.play_target.unwrap();
    assert_eq!(target.label, "Play again");
    assert_eq!(target.resume_ms, None);
    assert_eq!(target.left_ms, None);

    core.library()
        .set_progress("p", progress(120_000, 2), &core.hlc())
        .unwrap();
    for _ in 0..100 {
        let target = view.snapshot().header.play_target.unwrap();
        if target.label == "Resume" {
            assert_eq!(target.resume_ms, Some(120_000));
            assert_eq!(target.left_ms, Some(768_000));
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the rewatch never offered Resume");
}

/// What the title page says when no addon gives details, one case per kind.
#[tokio::test]
async fn a_title_that_cannot_load_says_why() {
    async fn reason(addons: &[AddonRow]) -> String {
        let core = Core::temporary("p").unwrap();
        core.library().set_addons("p", addons, &core.hlc()).unwrap();
        let view = TitleView::open(&core, "series", "tt0944947");
        match settle(&view).await.header.state {
            TitleState::Failed { reason } => reason,
            other => panic!("not failed: {other:?}"),
        }
    }
    assert_eq!(
        reason(&[]).await,
        "None of your addons has details for this title"
    );
    assert_eq!(
        reason(&[addon_answering("404 Not Found", "{}").await]).await,
        "Meta Test has no details for this title (404)"
    );
    assert_eq!(
        reason(&[meta_addon(r#"{"metas":[]}"#).await]).await,
        "Meta Test has no details for this title"
    );
    // `{}` is an empty answer only for a catalog: a meta has no empty form.
    assert_eq!(
        reason(&[meta_addon("{}").await]).await,
        "Meta Test sent details Marquee cannot read"
    );
    assert_eq!(
        reason(&[meta_addon("not json").await]).await,
        "Meta Test sent details Marquee cannot read"
    );
}

/// A meta that gives its IMDb rating as `imdbRating` beside links of its own
/// (genres) still shows it: stremio-core reads the field only when there are
/// no links.
#[tokio::test]
async fn an_imdb_rating_field_beside_links_shows() {
    const RATED: &str = r#"{"meta":{"id":"tt1727587","type":"movie","name":"Sintel","imdbRating":"7.4","links":[{"name":"Animation","category":"Genres","url":"stremio:///discover"}]}}"#;
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(RATED).await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "movie", "tt1727587");
    let details = settle(&view).await.header.details.expect("details");
    assert_eq!(details.ratings.imdb.as_deref(), Some("7.4"));
}
