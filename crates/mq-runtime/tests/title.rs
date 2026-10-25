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
        "types": ["series", "movie", "anime"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
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

/// A series announced, none of its episodes out yet.
const UPCOMING: &str = r#"{"meta":{
  "id":"tt9999991","type":"series","name":"Upcoming","releaseInfo":"2099-",
  "videos":[
    {"id":"tt9999991:1:1","title":"One","season":1,"episode":1,"released":"2099-01-01T00:00:00.000Z"},
    {"id":"tt9999991:1:2","title":"Two","season":1,"episode":2,"released":"2099-01-08T00:00:00.000Z"}
  ]}}"#;

/// C41: a series with no aired episode never reads Watched, and marking
/// it marks nothing.
#[tokio::test]
async fn a_series_with_nothing_aired_is_never_watched() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(UPCOMING).await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "series", "tt9999991");
    let snap = settle(&view).await;
    assert!(snap.header.details.is_some());
    assert!(!snap.header.watched);
    view.set_watched("tt9999991", true).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        core.library()
            .watched_videos("p", "tt9999991")
            .unwrap()
            .is_empty()
    );
    assert!(!view.snapshot().header.watched);
}

/// An anime with seasons, as a Kitsu-style addon serves it.
const ANIME: &str = r#"{"meta":{
  "id":"tt7777771","type":"anime","name":"Anime",
  "videos":[
    {"id":"tt7777771:0:1","title":"OVA","season":0,"episode":1,"released":"2015-01-01T00:00:00.000Z"},
    {"id":"tt7777771:1:1","title":"One","season":1,"episode":1,"released":"2015-04-01T00:00:00.000Z"},
    {"id":"tt7777771:1:2","title":"Two","season":1,"episode":2,"released":"2015-04-08T00:00:00.000Z"}
  ]}}"#;

/// An anime whose episodes carry no season, one not out yet.
const SEASONLESS: &str = r#"{"meta":{
  "id":"tt7777772","type":"anime","name":"Seasonless",
  "videos":[
    {"id":"tt7777772:1","title":"One","released":"2016-01-01T00:00:00.000Z"},
    {"id":"tt7777772:2","title":"Two","released":"2016-01-08T00:00:00.000Z"},
    {"id":"tt7777772:3","title":"Three","released":"2099-01-01T00:00:00.000Z"}
  ]}}"#;

/// Marks a title with episodes as one on its own id and returns the
/// videos marked, once the header reads Watched.
async fn mark_as_one(body: &'static str, kind: &str, id: &str) -> Vec<String> {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(body).await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, kind, id);
    assert!(!settle(&view).await.header.watched);
    view.set_watched(id, true).unwrap();
    let (_, mut updates) = view.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while !view.snapshot().header.watched {
        assert!(tokio::time::Instant::now() < deadline, "not marked watched");
        let _ = tokio::time::timeout(Duration::from_millis(200), updates.next()).await;
    }
    let mut marked = core.library().watched_videos("p", id).unwrap();
    marked.sort();
    marked
}

/// C41 review P2: any kind with episodes is watched as a whole, not only
/// `series`.
#[tokio::test]
async fn an_anime_is_watched_as_a_whole() {
    let marked = mark_as_one(ANIME, "anime", "tt7777771").await;
    assert_eq!(marked, ["tt7777771:1:1", "tt7777771:1:2"], "not the OVA");
}

/// C41 review P3-3: episodes with no season count as regular ones, so a
/// seasonless anime can be Watched; the one not out yet is left out.
#[tokio::test]
async fn a_seasonless_anime_is_watched_as_a_whole() {
    let marked = mark_as_one(SEASONLESS, "anime", "tt7777772").await;
    assert_eq!(marked, ["tt7777772:1", "tt7777772:2"]);
}

/// C41 review P3-1: a title-level mark sent before the meta is in is
/// refused, never a silent no-op.
#[tokio::test]
async fn marking_a_series_before_it_loads_is_refused() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(SHOW).await], &core.hlc())
        .unwrap();
    // The fetch is spawned and this test's runtime has one thread: it has
    // not run yet.
    let view = TitleView::open(&core, "series", "tt0944947");
    let refused = view.set_watched("tt0944947", true);
    assert!(
        matches!(refused, Err(mq_runtime::CoreError::NotLoaded)),
        "{refused:?}"
    );
    assert!(
        core.library()
            .watched_videos("p", "tt0944947")
            .unwrap()
            .is_empty()
    );
    settle(&view).await;
    view.set_watched("tt0944947", true).unwrap();
    assert_eq!(
        core.library()
            .watched_videos("p", "tt0944947")
            .unwrap()
            .len(),
        3
    );
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
        "Meta Test sent details Momotus cannot read"
    );
    assert_eq!(
        reason(&[meta_addon("not json").await]).await,
        "Meta Test sent details Momotus cannot read"
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

/// A subtitles addon answering `body` for every video, recording the paths
/// it was asked.
async fn subtitles_addon(
    body: &'static str,
) -> (AddonRow, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    slow_subtitles_addon("subs.test", body, Duration::ZERO).await
}

/// `subtitles_addon` with its own id, answering after `delay`.
async fn slow_subtitles_addon(
    id: &str,
    body: &'static str,
    delay: Duration,
) -> (AddonRow, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let path = String::from_utf8_lossy(&buf[..n])
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                log.lock().unwrap().push(path);
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
        "types": ["series", "movie"], "resources": ["subtitles"], "catalogs": [], "idPrefixes": ["tt"]
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 1,
        enabled: true,
        allow_private: true,
    };
    (row, asked)
}

/// Core backlog C34: the header says what the subtitles addons list for
/// the video Play plays (the series' first episode here): its languages,
/// each once, and whether a track is for the hard of hearing, read from
/// its file name as the player reads it. With no subtitles addon it says
/// nothing.
#[tokio::test]
async fn the_header_counts_the_subtitles_of_the_video_play_plays() {
    let (subs, asked) = subtitles_addon(
        r#"{"subtitles":[
            {"id":"1","lang":"eng","url":"https://subs.test/1.srt","subtitleFileName":"Game.of.Thrones.S01E01.720p.HDTV.SDH.srt"},
            {"id":"2","lang":"eng","url":"https://subs.test/2.srt","subtitleFileName":"Game.of.Thrones.S01E01.720p.HDTV.srt"},
            {"id":"3","lang":"fre","url":"https://subs.test/3.srt"}
        ]}"#,
    )
    .await;
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(SHOW).await, subs], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "series", "tt0944947");
    let (_, mut updates) = view.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while view.snapshot().header.captions.is_none() {
        assert!(tokio::time::Instant::now() < deadline, "no captions");
        let _ = tokio::time::timeout(Duration::from_millis(200), updates.next()).await;
    }
    let header = view.snapshot().header;
    let captions = header.captions.unwrap();
    assert_eq!((captions.languages, captions.hearing_impaired), (2, true));
    let video = header.play_target.unwrap().video_id;
    assert_eq!(video, "tt0944947:1:1");
    assert!(
        asked
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.contains("tt0944947:1:1") || p.contains("tt0944947%3A1%3A1")),
        "{:?}",
        asked.lock().unwrap()
    );

    // No subtitles addon: nothing to say.
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(SHOW).await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "series", "tt0944947");
    settle(&view).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(view.snapshot().header.captions, None);
}

/// Review P2: each addon's answer shows as it comes, so a slow addon never
/// holds back a quick one's: English at once, French when the slow one
/// answers.
#[tokio::test]
async fn a_slow_subtitles_addon_does_not_hold_back_a_quick_one() {
    const EN: &str = r#"{"subtitles":[{"id":"1","lang":"eng","url":"https://subs.test/1.srt"}]}"#;
    const FR: &str = r#"{"subtitles":[{"id":"2","lang":"fre","url":"https://subs.test/2.srt"}]}"#;
    let (quick, _) = slow_subtitles_addon("quick", EN, Duration::ZERO).await;
    let (slow, _) = slow_subtitles_addon("slow", FR, Duration::from_secs(3)).await;
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(SHOW).await, quick, slow], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "series", "tt0944947");
    let opened = tokio::time::Instant::now();
    let languages = |v: &TitleView| v.snapshot().header.captions.map(|c| c.languages);
    let (_, mut updates) = view.subscribe();
    while languages(&view).is_none() {
        assert!(
            opened.elapsed() < Duration::from_secs(2),
            "the quick answer waited"
        );
        let _ = tokio::time::timeout(Duration::from_millis(100), updates.next()).await;
    }
    assert_eq!(languages(&view), Some(1), "English, before the slow addon");
    while languages(&view) != Some(2) {
        assert!(opened.elapsed() < Duration::from_secs(10), "no French");
        let _ = tokio::time::timeout(Duration::from_millis(200), updates.next()).await;
    }
}

/// Review P3: an open page asks again when the addons change: a subtitles
/// addon installed adds its languages, and with none left the header says
/// nothing.
#[tokio::test]
async fn the_captions_follow_the_installed_addons() {
    const EN: &str = r#"{"subtitles":[{"id":"1","lang":"eng","url":"https://subs.test/1.srt"}]}"#;
    const FR: &str = r#"{"subtitles":[{"id":"2","lang":"fre","url":"https://subs.test/2.srt"}]}"#;
    let (en, _) = slow_subtitles_addon("en", EN, Duration::ZERO).await;
    let (fr, _) = slow_subtitles_addon("fr", FR, Duration::ZERO).await;
    let meta = meta_addon(SHOW).await;
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta.clone(), en.clone()], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "series", "tt0944947");
    let (_, mut updates) = view.subscribe();
    let languages = |v: &TitleView| v.snapshot().header.captions.map(|c| c.languages);
    for (want, then) in [
        (Some(1), Some(vec![meta.clone(), en, fr])),
        (Some(2), Some(vec![meta])),
        (None, None),
    ] {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while languages(&view) != want {
            assert!(
                tokio::time::Instant::now() < deadline,
                "never {want:?}: {:?}",
                languages(&view)
            );
            let _ = tokio::time::timeout(Duration::from_millis(100), updates.next()).await;
        }
        if let Some(rows) = then {
            core.library().set_addons("p", &rows, &core.hlc()).unwrap();
        }
    }
}
