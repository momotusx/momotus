use futures_util::StreamExt;
use mq_library::{AddonRow, LibraryItemRow, ProgressRow};
use mq_runtime::views::home::{CONTINUE_ROW_ID, HomeRow};
use mq_runtime::{Core, HomeUpdate, HomeView, RowState};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Local catalog addon answering after `delay` with `status` and `body`.
async fn catalog_addon(id: &str, delay: Duration, status: u16, body: &'static str) -> AddonRow {
    named_catalog_addon(id, "Popular", delay, status, body).await
}

/// `catalog_addon` with its movie catalog called `name`.
async fn named_catalog_addon(
    id: &str,
    name: &str,
    delay: Duration,
    status: u16,
    body: &'static str,
) -> AddonRow {
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
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": id, "version": "1.0.0", "name": format!("{id} addon"),
        "types": ["movie"], "resources": ["catalog"],
        "catalogs": [
            {"type": "movie", "id": "top", "name": name},
            {"type": "movie", "id": "search", "name": "Search", "extra": [{"name": "search", "isRequired": true}]}
        ]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

/// The Rotten Tomatoes addon as the defaults install it: its link carries
/// RPDB's free key, the key every IMDb cover's ratings band is drawn with.
/// No catalogs, so nothing is read from it.
fn rt_addon() -> AddonRow {
    AddonRow {
        transport_url: "https://7a82163c306e-rottentomatoes.baby-beamup.club/%7B%22rpdb_key%22%3A%22t0-free-rpdb%22%7D/manifest.json".into(),
        manifest_json: serde_json::json!({
            "id": "pw.ers.rottentomatoes", "version": "1.0.9", "name": "Rotten Tomatoes",
            "types": ["movie", "series"], "resources": ["catalog"], "catalogs": []
        })
        .to_string(),
        position: 0,
        enabled: true,
        allow_private: false,
    }
}

const METAS: &str = r#"{"metas":[
  {"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","poster":"https://images.metahub.space/poster/medium/tt0111161/img","background":"https://images.metahub.space/background/medium/tt0111161/img","releaseInfo":"1994","runtime":"142 min","description":"A banker convicted of uxoricide forms a friendship over a quarter century with a hardened convict.","links":[{"name":"9.3","category":"imdb","url":"https://imdb.com/title/tt0111161"},{"name":"Drama","category":"Genres","url":"stremio:///discover"},{"name":"Crime","category":"Genres","url":"stremio:///discover"},{"name":"Prison","category":"Genres","url":"stremio:///discover"},{"name":"Classic","category":"Genres","url":"stremio:///discover"},{"name":"Tim Robbins","category":"Cast","url":"stremio:///search"}]},
  {"id":"tt0068646","type":"movie","name":"The Godfather","poster":"https://images.metahub.space/poster/medium/tt0068646/img"}
]}"#;

fn apply(rows: &mut Vec<HomeRow>, updates: Vec<HomeUpdate>) {
    for update in updates {
        if let HomeUpdate::Rows { diffs } = update {
            for diff in diffs {
                diff.apply(rows);
            }
        }
    }
}

#[tokio::test]
async fn rows_fill_in_place_and_mirror_matches_view() {
    let core = Core::temporary("p").unwrap();
    let fast = catalog_addon("fast", Duration::ZERO, 200, METAS).await;
    let slow = named_catalog_addon("slow", "Slow", Duration::from_millis(2800), 200, METAS).await;
    let broken = named_catalog_addon("broken", "Broken", Duration::ZERO, 500, "{}").await;
    core.library()
        .set_addons("p", &[fast, slow, broken, rt_addon()], &core.hlc())
        .unwrap();

    let view = HomeView::open(&core);
    let (snapshot, mut updates) = view.subscribe();
    assert_eq!(snapshot.rows.len(), 3, "search-only catalogs are skipped");
    let titles: Vec<&str> = snapshot.rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, ["Popular Movies", "Slow Movies", "Broken Movies"]);
    let mut mirror = snapshot.rows.clone();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    let mut saw_slow = false;
    while tokio::time::Instant::now() < deadline {
        let Ok(Some(batch)) = tokio::time::timeout(Duration::from_secs(4), updates.next()).await
        else {
            break;
        };
        apply(&mut mirror, batch);
        saw_slow |= mirror.iter().any(|r| r.state == RowState::Slow);
        if mirror
            .iter()
            .all(|r| !matches!(r.state, RowState::Loading | RowState::Slow))
        {
            break;
        }
    }
    assert!(saw_slow, "the slow addon is reported before it answers");
    assert_eq!(
        mirror,
        view.snapshot().rows,
        "diffs reproduce the view state"
    );
    let states: Vec<_> = mirror.iter().map(|r| r.state.clone()).collect();
    assert_eq!(states[0], RowState::Ready);
    assert_eq!(states[1], RowState::Ready);
    assert!(matches!(states[2], RowState::Failed { .. }));
    assert_eq!(mirror[0].tiles[0].name, "The Shawshank Redemption");
    // RPDB's cover with the ratings band, the addon's poster as its fallback.
    assert_eq!(
        mirror[0].tiles[0].poster.as_deref(),
        Some(
            "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0111161.jpg?fallback=true"
        )
    );
    assert_eq!(
        mirror[0].tiles[0].poster_fallback.as_deref(),
        Some("https://images.metahub.space/poster/medium/tt0111161/img")
    );
    let hero = view
        .snapshot()
        .header
        .heroes
        .first()
        .cloned()
        .expect("hero picked from artwork");
    assert_eq!(hero.tile.id, "tt0111161");
    // The hero carries what a featured title shows; rows carry only the rating.
    assert_eq!(hero.tile.rating.as_deref(), Some("9.3"));
    assert_eq!(hero.genres, ["Drama", "Crime", "Prison"]);
    assert_eq!(hero.runtime.as_deref(), Some("142 min"));
    assert!(hero.description.as_deref().unwrap().starts_with("A banker"));
    let row_json = serde_json::to_value(&mirror[0]).unwrap();
    assert_eq!(row_json["tiles"][0]["rating"], "9.3");
    assert!(
        row_json["tiles"][0].get("blurb").is_none()
            && row_json["tiles"][0].get("description").is_none()
    );
}

#[tokio::test]
async fn continue_watching_appears_live_at_the_top() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[rt_addon()], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    let (snapshot, mut updates) = view.subscribe();
    let mut mirror = snapshot.rows.clone();
    assert!(mirror.is_empty());

    let item = LibraryItemRow {
        item_id: "tt0944947".into(),
        kind: "series".into(),
        name: "Game of Thrones".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("p", &item, true, &core.hlc())
        .unwrap();
    let progress = ProgressRow {
        video_id: "tt0944947:1:3",
        item_id: "tt0944947",
        position_ms: 600_000,
        duration_ms: 3_000_000,
        updated_ms: 5,
    };
    core.library()
        .set_progress("p", progress, &core.hlc())
        .unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while mirror.is_empty() && tokio::time::Instant::now() < deadline {
        if let Ok(Some(batch)) = tokio::time::timeout(Duration::from_secs(1), updates.next()).await
        {
            apply(&mut mirror, batch);
        }
    }
    assert_eq!(mirror[0].row_id, CONTINUE_ROW_ID);
    let tile = &mirror[0].tiles[0];
    assert_eq!(tile.video_id.as_deref(), Some("tt0944947:1:3"));
    assert!((tile.progress.unwrap() - 0.2).abs() < 1e-4);
    assert_eq!(
        tile.poster.as_deref(),
        Some(
            "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0944947.jpg?fallback=true"
        )
    );
}

/// Journey reviews 2026-10-20 (web P2-10, Apple P2-3, Android P2-1):
/// "Remove from Continue Watching" takes the tile off live; playing the
/// title again brings it back.
#[tokio::test]
async fn a_title_removed_from_continue_watching_leaves_the_row_live() {
    let core = Core::temporary("p").unwrap();
    let view = HomeView::open(&core);
    let (snapshot, mut updates) = view.subscribe();
    let mut mirror = snapshot.rows.clone();
    let play = |position_ms| {
        let progress = ProgressRow {
            video_id: "tt1",
            item_id: "tt1",
            position_ms,
            duration_ms: 3_000_000,
            updated_ms: position_ms,
        };
        core.library()
            .set_progress("p", progress, &core.hlc())
            .unwrap();
    };
    play(600_000);
    continue_shown(&mut mirror, &mut updates, true).await;
    core.remove_from_continue("tt1").unwrap();
    continue_shown(&mut mirror, &mut updates, false).await;
    play(700_000);
    continue_shown(&mut mirror, &mut updates, true).await;
}

/// Waits up to 3 s for the Continue Watching row to be there or not.
async fn continue_shown(
    mirror: &mut Vec<HomeRow>,
    updates: &mut (impl futures_util::Stream<Item = Vec<HomeUpdate>> + Unpin),
    want: bool,
) {
    let shown = |rows: &[HomeRow]| rows.iter().any(|r| r.row_id == CONTINUE_ROW_ID);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while shown(mirror) != want && tokio::time::Instant::now() < deadline {
        if let Ok(Some(batch)) =
            tokio::time::timeout(Duration::from_millis(200), updates.next()).await
        {
            apply(mirror, batch);
        }
    }
    assert_eq!(shown(mirror), want);
}

#[tokio::test]
#[ignore = "network"]
async fn real_cinemeta_home() {
    let core = Core::temporary("p").unwrap();
    core.install_addon("https://v3-cinemeta.strem.io/manifest.json")
        .await
        .unwrap();
    let view = HomeView::open(&core);
    let started = std::time::Instant::now();
    let (_, mut updates) = view.subscribe();
    while let Ok(Some(_)) = tokio::time::timeout(Duration::from_secs(8), updates.next()).await {
        if view.snapshot().rows.iter().all(|r| {
            r.state == RowState::Ready
                || matches!(r.state, RowState::Failed { .. } | RowState::Empty)
        }) {
            break;
        }
    }
    let rows = view.snapshot().rows;
    eprintln!(
        "cinemeta home: {} rows in {:?}",
        rows.len(),
        started.elapsed()
    );
    for r in &rows {
        eprintln!("  {:?} {} ({} tiles)", r.state, r.title, r.tiles.len());
    }
    assert!(rows.iter().filter(|r| r.state == RowState::Ready).count() >= 2);
}

#[tokio::test]
async fn addons_move_disable_remove_and_the_view_follows() {
    use mq_runtime::AddonsView;
    let core = Core::temporary("p").unwrap();
    let row = |name: &str, i: i64| mq_library::AddonRow {
        transport_url: format!(
            "https://{name}.example/realdebrid=0123456789abcdefghij0123456789/manifest.json"
        ),
        manifest_json:
            serde_json::json!({"id": name, "version": "1.0.0", "name": name, "types": ["movie"],
            "resources": ["stream"], "catalogs": []})
            .to_string(),
        position: i,
        enabled: true,
        allow_private: false,
    };
    core.library()
        .set_addons("p", &[row("a", 0), row("b", 1), row("c", 2)], &core.hlc())
        .unwrap();
    let view = AddonsView::open(&core);
    let names = |v: &AddonsView| -> Vec<String> {
        v.snapshot().addons.iter().map(|a| a.name.clone()).collect()
    };
    assert_eq!(names(&view), ["a", "b", "c"]);
    let urls: Vec<String> = view
        .snapshot()
        .addons
        .iter()
        .map(|a| a.url.clone())
        .collect();
    let (url_a, url_c) = (&urls[0], &urls[2]);

    core.move_addon(url_c, 0).unwrap();
    core.set_addon_enabled(url_c, false).unwrap();
    core.remove_addon(url_a).unwrap();
    for _ in 0..100 {
        if names(&view) == ["c", "b"] {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(names(&view), ["c", "b"]);
    assert!(!view.snapshot().addons[0].enabled);
    // Disabled addons are not asked for anything.
    assert_eq!(core.installed_addons().unwrap().len(), 1);

    // Errors never carry the key in the URL.
    let err = core
        .remove_addon("https://x.example/realdebrid=0123456789abcdefghij0123456789/manifest.json");
    let text = err.unwrap_err().to_string();
    assert!(!text.contains("0123456789abcdef"), "{text}");
}

const SERIES_META: &str = r#"{"meta":{"id":"tt9","type":"series","name":"Show",
  "background":"https://images.example/tt9-bg.jpg","videos":[
  {"id":"tt9:1:1","title":"Pilot","season":1,"episode":1,"released":"2020-01-01T00:00:00Z"},
  {"id":"tt9:1:2","title":"Second","season":1,"episode":2,"released":"2020-01-08T00:00:00Z"}]}}"#;

#[tokio::test]
async fn a_finished_episode_leaves_its_next_one_in_continue_watching() {
    let core = Core::temporary("p").unwrap();
    let mut meta = catalog_addon("meta", Duration::ZERO, 200, SERIES_META).await;
    meta.manifest_json = serde_json::json!({
        "id": "meta", "version": "1.0.0", "name": "Meta",
        "types": ["series"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    })
    .to_string();
    core.library()
        .set_addons("p", &[meta, rt_addon()], &core.hlc())
        .unwrap();
    let show = LibraryItemRow {
        item_id: "tt9".into(),
        kind: "series".into(),
        name: "Show".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("p", &show, true, &core.hlc())
        .unwrap();
    core.library()
        .set_progress(
            "p",
            ProgressRow {
                video_id: "tt9:1:1",
                item_id: "tt9",
                position_ms: 990,
                duration_ms: 1000,
                updated_ms: 5,
            },
            &core.hlc(),
        )
        .unwrap();

    let view = HomeView::open(&core);
    let mut tile = None;
    for _ in 0..200 {
        tile = view
            .snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id == CONTINUE_ROW_ID)
            .and_then(|r| r.tiles.into_iter().next());
        if tile.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let tile = tile.expect("Next Up tile");
    assert_eq!(tile.video_id.as_deref(), Some("tt9:1:2"));
    assert_eq!(tile.release_info.as_deref(), Some("Next: S1:E2"));
    assert_eq!(tile.progress, None);
    assert_eq!(
        tile.poster.as_deref(),
        Some(
            "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt9.jpg?fallback=true"
        )
    );
    // The hero features a catalog, never the first Continue Watching tile
    // right below it; with no catalog rows there is none.
    assert!(view.snapshot().header.heroes.is_empty());
}

/// Kitsu's answer for an anime: a series meta with season 1 episodes.
const ANIME_META: &str = r#"{"meta":{"id":"kitsu:9","type":"series","name":"Frieren","videos":[
  {"id":"kitsu:9:1","title":"The Journey's End","season":1,"episode":1,"released":"2023-09-29T14:00:00.000Z"},
  {"id":"kitsu:9:2","title":"It Didn't Have to Be Magic","season":1,"episode":2,"released":"2023-09-29T14:00:00.000Z"}]}}"#;

/// Web journey review P3-20: an anime (Kitsu's kind and ids) gets Next Up
/// like a series, read from the addon that answers for its kind.
#[tokio::test]
async fn a_finished_anime_episode_leaves_its_next_one_in_continue_watching() {
    let core = Core::temporary("p").unwrap();
    let mut kitsu = catalog_addon("kitsu", Duration::ZERO, 200, ANIME_META).await;
    kitsu.manifest_json = serde_json::json!({
        "id": "kitsu", "version": "1.0.0", "name": "Kitsu",
        "types": ["anime"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["kitsu"]
    })
    .to_string();
    core.library()
        .set_addons("p", &[kitsu], &core.hlc())
        .unwrap();
    let anime = LibraryItemRow {
        item_id: "kitsu:9".into(),
        kind: "anime".into(),
        name: "Frieren".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("p", &anime, true, &core.hlc())
        .unwrap();
    let progress = ProgressRow {
        video_id: "kitsu:9:1",
        item_id: "kitsu:9",
        position_ms: 990,
        duration_ms: 1000,
        updated_ms: 5,
    };
    core.library()
        .set_progress("p", progress, &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    let mut tile = None;
    for _ in 0..200 {
        tile = view
            .snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id == CONTINUE_ROW_ID)
            .and_then(|r| r.tiles.into_iter().next());
        if tile.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let tile = tile.expect("Next Up tile for the anime");
    assert_eq!(tile.video_id.as_deref(), Some("kitsu:9:2"));
    assert_eq!(tile.release_info.as_deref(), Some("Next: S1:E2"));
}

#[tokio::test]
#[ignore = "network"]
async fn real_cinemeta_next_up() {
    let core = Core::temporary("p").unwrap();
    core.install_addon("https://v3-cinemeta.strem.io/manifest.json")
        .await
        .unwrap();
    let got = LibraryItemRow {
        item_id: "tt0944947".into(),
        kind: "series".into(),
        name: "Game of Thrones".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("p", &got, true, &core.hlc())
        .unwrap();
    let done = ProgressRow {
        video_id: "tt0944947:1:1",
        item_id: "tt0944947",
        position_ms: 3_500_000,
        duration_ms: 3_600_000,
        updated_ms: 5,
    };
    core.library().set_progress("p", done, &core.hlc()).unwrap();
    let started = std::time::Instant::now();
    let view = HomeView::open(&core);
    for _ in 0..1000 {
        let tile = view
            .snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id == CONTINUE_ROW_ID)
            .and_then(|r| r.tiles.into_iter().next());
        if let Some(tile) = tile {
            eprintln!(
                "next up: {:?} {:?} after {} ms",
                tile.video_id,
                tile.release_info,
                started.elapsed().as_millis()
            );
            assert_eq!(tile.video_id.as_deref(), Some("tt0944947:1:2"));
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no Next Up tile");
}

/// Installing or removing an addon reshapes an open Home: rows that stay keep
/// their tiles (no flash back to loading), new catalogs load, removed ones go.
#[tokio::test]
async fn home_follows_addon_changes_without_reopening() {
    let core = Core::temporary("p").unwrap();
    let first = catalog_addon("first", Duration::ZERO, 200, METAS).await;
    let second = named_catalog_addon("second", "Trending", Duration::ZERO, 200, METAS).await;
    core.library()
        .set_addons("p", std::slice::from_ref(&first), &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    let (snapshot, mut updates) = view.subscribe();
    let mut mirror = snapshot.rows.clone();
    let settle = |mirror: &Vec<HomeRow>, n: usize| {
        mirror.len() == n && mirror.iter().all(|r| r.state == RowState::Ready)
    };
    async fn until(
        updates: &mut (impl futures_util::Stream<Item = Vec<HomeUpdate>> + Unpin),
        mirror: &mut Vec<HomeRow>,
        done: impl Fn(&Vec<HomeRow>) -> bool,
        mut each: impl FnMut(&Vec<HomeRow>),
    ) {
        while !done(mirror) {
            let batch = tokio::time::timeout(Duration::from_secs(4), updates.next())
                .await
                .expect("an update")
                .expect("open");
            apply(mirror, batch);
            each(mirror);
        }
    }
    until(&mut updates, &mut mirror, |m| settle(m, 1), |_| {}).await;

    let first_id = mirror[0].row_id.clone();
    core.library()
        .set_addons("p", &[first.clone(), second], &core.hlc())
        .unwrap();
    until(
        &mut updates,
        &mut mirror,
        |m| settle(m, 2),
        |m| {
            let kept = m.iter().find(|r| r.row_id == first_id).expect("kept");
            assert_eq!(kept.state, RowState::Ready, "the kept row never reloads");
        },
    )
    .await;
    assert_eq!(mirror[0].row_id, first_id, "addon order");

    core.library().set_addons("p", &[], &core.hlc()).unwrap();
    until(&mut updates, &mut mirror, |m| m.is_empty(), |_| {}).await;
    assert_eq!(mirror, view.snapshot().rows);
    assert!(
        view.snapshot().header.heroes.is_empty(),
        "no catalog, no hero"
    );
}

const SERIES_WITH_STILLS: &str = r#"{"meta":{"id":"tt9","type":"series","name":"Show",
  "background":"https://images.example/tt9-bg.jpg","logo":"https://images.example/tt9-logo.png","videos":[
  {"id":"tt9:1:1","title":"Pilot","season":1,"episode":1,"released":"2020-01-01T00:00:00Z","thumbnail":"https://images.example/tt9-1-1.jpg"},
  {"id":"tt9:1:2","title":"Second","season":1,"episode":2,"released":"2020-01-08T00:00:00Z"}]}}"#;

/// An unfinished episode in Continue Watching shows its own still (and the
/// title's logo), not only the poster.
#[tokio::test]
async fn continue_watching_tiles_carry_the_episode_still() {
    let core = Core::temporary("p").unwrap();
    let mut meta = catalog_addon("meta", Duration::ZERO, 200, SERIES_WITH_STILLS).await;
    meta.manifest_json = serde_json::json!({
        "id": "meta", "version": "1.0.0", "name": "Meta",
        "types": ["series"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    })
    .to_string();
    core.library()
        .set_addons("p", &[meta], &core.hlc())
        .unwrap();
    let show = LibraryItemRow {
        item_id: "tt9".into(),
        kind: "series".into(),
        name: "Show".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("p", &show, true, &core.hlc())
        .unwrap();
    core.library()
        .set_progress(
            "p",
            ProgressRow {
                video_id: "tt9:1:1",
                item_id: "tt9",
                position_ms: 400,
                duration_ms: 1000,
                updated_ms: 5,
            },
            &core.hlc(),
        )
        .unwrap();
    let view = HomeView::open(&core);
    let mut tile = None;
    for _ in 0..200 {
        tile = view
            .snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id == CONTINUE_ROW_ID)
            .and_then(|r| r.tiles.into_iter().next())
            .filter(|t| t.background.is_some());
        if tile.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let tile = tile.expect("Continue Watching tile with artwork");
    assert_eq!(tile.video_id.as_deref(), Some("tt9:1:1"));
    assert_eq!(
        tile.background.as_deref(),
        Some("https://images.example/tt9-1-1.jpg")
    );
    assert_eq!(
        tile.logo.as_deref(),
        Some("https://images.example/tt9-logo.png")
    );
    assert_eq!(tile.progress, Some(0.4));
    // One caption rule for every app: where you are and what is left.
    assert_eq!(
        tile.release_info.as_deref(),
        Some("S1:E1 \u{b7} 1 min left")
    );
}

/// The row is on screen before a slow meta addon answers; the artwork
/// arrives as an update to the same row.
#[tokio::test]
async fn continue_watching_shows_before_its_metas_answer() {
    let core = Core::temporary("p").unwrap();
    let mut meta = catalog_addon("meta", Duration::from_secs(2), 200, SERIES_WITH_STILLS).await;
    meta.manifest_json = serde_json::json!({
        "id": "meta", "version": "1.0.0", "name": "Meta",
        "types": ["series"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    })
    .to_string();
    core.library()
        .set_addons("p", &[meta], &core.hlc())
        .unwrap();
    let show = LibraryItemRow {
        item_id: "tt9".into(),
        kind: "series".into(),
        name: "Show".into(),
        poster: Some("https://images.example/tt9-poster.jpg".into()),
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("p", &show, true, &core.hlc())
        .unwrap();
    core.library()
        .set_progress(
            "p",
            ProgressRow {
                video_id: "tt9:1:1",
                item_id: "tt9",
                position_ms: 400,
                duration_ms: 1000,
                updated_ms: 5,
            },
            &core.hlc(),
        )
        .unwrap();
    let view = HomeView::open(&core);
    let first_tile = |view: &HomeView| {
        view.snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id == CONTINUE_ROW_ID)
            .and_then(|r| r.tiles.into_iter().next())
    };
    let mut early = None;
    for _ in 0..50 {
        early = first_tile(&view);
        if early.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let early = early.expect("row within 500 ms, long before the meta answers");
    assert_eq!(early.video_id.as_deref(), Some("tt9:1:1"));
    assert_eq!(early.background, None);
    let mut late = None;
    for _ in 0..400 {
        late = first_tile(&view).filter(|t| t.background.is_some());
        if late.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        late.expect("artwork after the meta answers")
            .background
            .as_deref(),
        Some("https://images.example/tt9-1-1.jpg")
    );
}

const SERIES_WITH_TVDB: &str = r#"{"meta":{"id":"tt7","type":"series","name":"Show","videos":[
  {"id":"tt7:1:1","title":"Pilot","season":1,"episode":1,"tvdb_id":349232},
  {"id":"tt7:1:2","title":"Second","season":1,"episode":2,"tvdb_id":349235},
  {"id":"tt7:1:3","title":"Third","season":1,"episode":3}]}}"#;

/// stremio-core's `Video` has no `tvdb_id`; the core reads it from the raw
/// answer for Trakt, and episodes without one are simply absent.
#[tokio::test]
async fn meta_keeps_tvdb_ids() {
    let core = Core::temporary("p").unwrap();
    let mut meta = catalog_addon("meta", Duration::ZERO, 200, SERIES_WITH_TVDB).await;
    meta.manifest_json = serde_json::json!({
        "id": "meta", "version": "1.0.0", "name": "Meta",
        "types": ["series"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    })
    .to_string();
    core.library()
        .set_addons("p", &[meta], &core.hlc())
        .unwrap();
    let ids = core.episode_tvdb_ids("tt7").await.unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get("tt7:1:1"), Some(&349232));
    assert_eq!(ids.get("tt7:1:2"), Some(&349235));
    // The typed meta still decodes from the same answer.
    let (decoded, _) = core.fetch_meta("series", "tt7").await.unwrap();
    assert_eq!(decoded.videos.len(), 3);
}

/// Hiding and moving Home rows is a preference; Home follows it live, a
/// moved row keeps its tiles, and the ids of catalogs that are gone survive
/// every edit.
#[tokio::test]
async fn home_rows_hide_and_move_live() {
    let core = Core::temporary("p").unwrap();
    let mut a = catalog_addon("a", Duration::ZERO, 200, METAS).await;
    let mut b = named_catalog_addon("b", "Trending", Duration::ZERO, 200, METAS).await;
    a.position = 0;
    b.position = 1;
    core.library()
        .set_addons("p", &[a, b], &core.hlc())
        .unwrap();
    let all: Vec<String> = core.home_catalogs().into_iter().map(|c| c.row_id).collect();
    assert_eq!(all.len(), 2);
    let (ra, rb) = (all[0].clone(), all[1].clone());
    assert!(ra.starts_with("a@") && rb.starts_with("b@"), "{all:?}");

    let view = HomeView::open(&core);
    let wait = |want: Vec<String>| {
        let view = &view;
        async move {
            for _ in 0..300 {
                let s = view.snapshot();
                let got: Vec<String> = s.rows.iter().map(|r| r.row_id.clone()).collect();
                if got == want && s.rows.iter().all(|r| r.state == RowState::Ready) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("wanted {want:?}, got {:?}", view.snapshot().rows);
        }
    };
    wait(vec![ra.clone(), rb.clone()]).await;

    let listed = core.set_home_row_hidden(&ra, true).unwrap();
    wait(vec![rb.clone()]).await;
    assert_eq!(
        listed
            .iter()
            .map(|c| (c.row_id.clone(), c.hidden))
            .collect::<Vec<_>>(),
        [(ra.clone(), true), (rb.clone(), false)]
    );

    // An id from an addon that is gone stays through edits.
    core.set_prefs(&serde_json::json!({"homeRows": {"order": ["gone@00000000:movie:top"]}}))
        .unwrap();
    core.set_home_row_hidden(&ra, false).unwrap();
    core.move_home_row(&rb, -1).unwrap();
    wait(vec![rb.clone(), ra.clone()]).await;
    // The moved row kept its tiles: it never went back to loading.
    assert!(!view.snapshot().rows[0].tiles.is_empty());
    // Extreme moves clamp instead of overflowing.
    core.move_home_row(&ra, i64::MIN).unwrap();
    wait(vec![ra.clone(), rb.clone()]).await;
    // The second row moved by the largest step stays last.
    core.move_home_row(&rb, i64::MAX).unwrap();
    wait(vec![ra.clone(), rb.clone()]).await;
    // The id of the catalog that is gone survived every move.
    assert_eq!(
        core.prefs().home_rows.order,
        [ra.clone(), rb.clone(), "gone@00000000:movie:top".into()]
    );
}

/// Two configurations of one addon (same manifest id) are two rows, and
/// each catalog lands in its own row.
#[tokio::test]
async fn two_installations_of_one_addon_are_two_catalogs_and_home_shows_the_first() {
    let core = Core::temporary("p").unwrap();
    let mut first = catalog_addon("same", Duration::ZERO, 200, METAS).await;
    let mut second = catalog_addon("same", Duration::ZERO, 200, METAS).await;
    first.position = 0;
    second.position = 1;
    core.library()
        .set_addons("p", &[first, second], &core.hlc())
        .unwrap();
    let catalogs = core.home_catalogs();
    assert_eq!(catalogs.len(), 2);
    assert_ne!(catalogs[0].row_id, catalogs[1].row_id);
    // Same titles: the second is a twin, off Home until turned on.
    assert_eq!(
        catalogs.iter().map(|c| c.hidden).collect::<Vec<_>>(),
        [false, true]
    );
    let view = HomeView::open(&core);
    rows_until(&view, &[&catalogs[0].row_id]).await;
    core.set_home_row_hidden(&catalogs[1].row_id, false)
        .unwrap();
    rows_until(&view, &[&catalogs[0].row_id, &catalogs[1].row_id]).await;
    assert!(view.snapshot().rows.iter().all(|r| !r.tiles.is_empty()));
}

/// Live (web journey review P3-20): a real Kitsu series through the core
/// with the default Anime Kitsu addon. Its next to last aired episode is
/// finished and marked watched: Next Up must name the last aired one, and
/// the calendar must list the series. `KITSU_ID` picks another series.
/// Run with `--ignored --nocapture`; prints a bench row.
#[tokio::test]
#[ignore = "network"]
async fn live_kitsu_next_up_and_calendar() {
    use mq_runtime::CalendarView;
    let core = Core::temporary("p").unwrap();
    core.install_addon("https://anime-kitsu.strem.fun/manifest.json")
        .await
        .unwrap();
    let id = std::env::var("KITSU_ID").unwrap_or_else(|_| "kitsu:49847".into());
    let now_ms = mq_library::hlc::now_ms() as i64;
    let (meta, source) = core.fetch_meta("anime", &id).await.unwrap();
    let mut aired: Vec<_> = meta
        .videos
        .iter()
        .filter_map(|v| Some((v, v.series_info.clone()?)))
        .filter(|(v, s)| s.season > 0 && v.released.is_some_and(|r| r.timestamp_millis() <= now_ms))
        .collect();
    aired.sort_by_key(|(_, s)| (s.season, s.episode));
    let [.., (done, _), (next, next_info)] = aired.as_slice() else {
        panic!("{id}: fewer than two aired episodes");
    };
    let item = LibraryItemRow {
        item_id: id.clone(),
        kind: "anime".into(),
        name: meta.preview.name.clone(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    let library = core.library();
    library.upsert_item("p", &item, true, &core.hlc()).unwrap();
    let progress = ProgressRow {
        video_id: &done.id,
        item_id: &id,
        position_ms: 1_400_000,
        duration_ms: 1_440_000,
        updated_ms: now_ms,
    };
    library.set_progress("p", progress, &core.hlc()).unwrap();
    library
        .set_watched("p", &id, &done.id, true, &core.hlc())
        .unwrap();

    let started = std::time::Instant::now();
    let home = HomeView::open(&core);
    let mut tile = None;
    for _ in 0..2000 {
        tile = home
            .snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id == CONTINUE_ROW_ID)
            .and_then(|r| {
                r.tiles
                    .into_iter()
                    .find(|t| t.id == id && t.release_info.is_some())
            });
        if tile.as_ref().is_some_and(|t| {
            t.release_info
                .as_deref()
                .is_some_and(|r| r.starts_with("Next"))
        }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let next_up_ms = started.elapsed().as_millis() as u64;
    let tile = tile.expect("a Continue Watching tile for the series");

    let days = now_ms / 86_400_000;
    let today =
        chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap() + chrono::Days::new(days as u64);
    let started = std::time::Instant::now();
    let calendar = CalendarView::open(&core, today);
    let mut s = calendar.snapshot();
    for _ in 0..2000 {
        s = calendar.snapshot();
        if s.pending == 0 && (!s.days.is_empty() || !s.failed.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let calendar_ms = started.elapsed().as_millis() as u64;
    let listed: Vec<String> = s
        .days
        .iter()
        .flat_map(|d| {
            d.items
                .iter()
                .filter(|i| i.item_id == id)
                .map(|i| format!("{} {}", d.date, i.video_id))
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({
            "series": id, "name": meta.preview.name, "meta_from": source, "today": today.to_string(),
            "watched": done.id, "next_up_tile": tile.video_id, "next_up_label": tile.release_info,
            "expected_next": next.id, "next_up_ms": next_up_ms,
            "calendar_items": listed, "calendar_failed": s.failed.len(), "calendar_ms": calendar_ms,
        })
    );
    assert_eq!(tile.video_id.as_deref(), Some(next.id.as_str()));
    assert_eq!(
        tile.release_info.as_deref(),
        Some(format!("Next: S{}:E{}", next_info.season, next_info.episode).as_str())
    );
    assert!(!listed.is_empty(), "the series is in the calendar");
}

/// A catalog addon answering its requests with `statuses` in turn (the
/// last one again after that), with `METAS` on 200; counts its requests.
async fn flaky_addon(id: &str, statuses: &'static [u16]) -> (AddonRow, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(AtomicUsize::new(0));
    let count = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let n = count.fetch_add(1, Ordering::SeqCst);
            let status = statuses[n.min(statuses.len() - 1)];
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let body = if status == 200 { METAS } else { "{}" };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": id, "version": "1.0.0", "name": format!("{id} addon"),
        "types": ["movie"], "resources": ["catalog"],
        "catalogs": [{"type": "movie", "id": "top", "name": id}]
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    (row, asked)
}

fn row(view: &HomeView, id: &str) -> HomeRow {
    view.snapshot()
        .rows
        .into_iter()
        .find(|r| r.row_id == id)
        .unwrap()
}

/// Waits up to 3 s for row `id` to meet `done`.
async fn row_until(view: &HomeView, id: &str, done: impl Fn(&HomeRow) -> bool) {
    for _ in 0..300 {
        if done(&row(view, id)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{:?}", row(view, id));
}

/// Apple journey review r2 P3-8: Try again on one failed row asks only
/// that catalog again; the row shows loading, then fails again or fills.
#[tokio::test]
async fn retrying_a_row_asks_only_its_catalog() {
    let core = Core::temporary("p").unwrap();
    let (flaky, flaky_asked) = flaky_addon("flaky", &[500, 500, 200]).await;
    let (steady, steady_asked) = flaky_addon("steady", &[200]).await;
    let mut addons = [flaky, steady];
    addons[1].position = 1;
    core.library()
        .set_addons("p", &addons, &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    // Row ids are `<addon>@<install tag>:<type>:<catalog>`.
    let id_of = |addon: &str| {
        let prefix = format!("{addon}@");
        view.snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id.starts_with(&prefix))
            .unwrap()
            .row_id
    };
    let (flaky_id, steady_id) = (id_of("flaky"), id_of("steady"));
    let failed = |r: &HomeRow| matches!(r.state, RowState::Failed { .. });
    row_until(&view, &flaky_id, failed).await;
    row_until(&view, &steady_id, |r| r.state == RowState::Ready).await;
    assert!(view.retry_row(&flaky_id));
    assert_eq!(row(&view, &flaky_id).state, RowState::Loading);
    row_until(&view, &flaky_id, failed).await;
    assert!(view.retry_row(&flaky_id));
    assert_eq!(row(&view, &flaky_id).state, RowState::Loading);
    row_until(&view, &flaky_id, |r| r.state == RowState::Ready).await;
    assert_eq!(row(&view, &flaky_id).tiles.len(), 2);
    assert_eq!(flaky_asked.load(Ordering::SeqCst), 3);
    assert_eq!(
        steady_asked.load(Ordering::SeqCst),
        1,
        "the other row is not asked"
    );
    assert!(!view.retry_row(CONTINUE_ROW_ID));
    assert!(!view.retry_row("gone:movie:top"));
}

/// The reason of the first hero Home shows.
async fn first_hero(view: &HomeView) -> String {
    for _ in 0..300 {
        if let Some(hero) = view.snapshot().header.heroes.first().cloned() {
            return hero.reason;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("no hero");
}

/// Waits up to 3 s for Home to show exactly the rows `want`, all filled.
async fn rows_until(view: &HomeView, want: &[&str]) {
    for _ in 0..300 {
        let rows = view.snapshot().rows;
        if rows
            .iter()
            .map(|r| r.row_id.as_str())
            .eq(want.iter().copied())
            && rows.iter().all(|r| r.state == RowState::Ready)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("wanted {want:?}, got {:?}", view.snapshot().rows);
}

/// No addon's name anywhere on Home (the owner, 2026-10-22): a row titled as
/// one before it in addon order, in any case, stays off Home until the user
/// turns it on in Customize Home, and the hero says only its row's title.
#[tokio::test]
async fn a_later_twin_row_stays_off_home_until_turned_on() {
    let core = Core::temporary("p").unwrap();
    let mut first = catalog_addon("first", Duration::ZERO, 200, METAS).await;
    let mut second = catalog_addon("second", Duration::ZERO, 200, METAS).await;
    let mut lower =
        named_catalog_addon("lower", "popular movies", Duration::ZERO, 200, METAS).await;
    (first.position, second.position, lower.position) = (0, 1, 2);
    core.library()
        .set_addons("p", &[first, second, lower], &core.hlc())
        .unwrap();
    let listed = |core: &Core| -> Vec<(String, bool)> {
        core.home_catalogs()
            .into_iter()
            .map(|c| (c.title, c.hidden))
            .collect()
    };
    assert_eq!(
        listed(&core),
        [
            ("Popular Movies".into(), false),
            ("Popular Movies".into(), true),
            ("popular movies".into(), true)
        ]
    );
    let ids: Vec<String> = core.home_catalogs().into_iter().map(|c| c.row_id).collect();
    let view = HomeView::open(&core);
    rows_until(&view, &[&ids[0]]).await;
    assert_eq!(first_hero(&view).await, "Popular Movies");

    // Turned on, the twin shows; the hero still names no addon.
    core.set_home_row_hidden(&ids[1], false).unwrap();
    rows_until(&view, &[&ids[0], &ids[1]]).await;
    assert_eq!(view.snapshot().header.heroes[0].reason, "Popular Movies");
    assert_eq!(core.prefs().home_rows.shown, [ids[1].clone()]);
    // Turned off again, it goes, and the choice to show it with it.
    core.set_home_row_hidden(&ids[1], true).unwrap();
    rows_until(&view, &[&ids[0]]).await;
    assert!(core.prefs().home_rows.shown.is_empty());
    assert_eq!(
        listed(&core).iter().map(|(_, h)| *h).collect::<Vec<_>>(),
        [false, true, true]
    );
}

/// The Godfather first with a backdrop, then The Shawshank Redemption.
const GODFATHER_FIRST: &str = r#"{"metas":[
  {"id":"tt0068646","type":"movie","name":"The Godfather","background":"https://images.metahub.space/background/medium/tt0068646/img"},
  {"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","background":"https://images.metahub.space/background/medium/tt0111161/img"}
]}"#;

/// The Godfather alone.
const GODFATHER_ONLY: &str = r#"{"metas":[
  {"id":"tt0068646","type":"movie","name":"The Godfather","background":"https://images.metahub.space/background/medium/tt0068646/img"}
]}"#;

/// Waits up to 3 s for the hero to be `id` with `reason`.
async fn hero_until(view: &HomeView, id: &str, reason: &str) {
    for _ in 0..300 {
        if view
            .snapshot()
            .header
            .heroes
            .first()
            .is_some_and(|h| h.tile.id == id && h.reason == reason)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "wanted {id} {reason:?}, got {:?}",
        view.snapshot().header.heroes.first()
    );
}

/// A kept hero whose row goes never stays on another row's word: another
/// row that lists it lower down does not keep it, the first candidate takes
/// its place with its own reason.
#[tokio::test]
async fn the_kept_hero_goes_with_its_row() {
    let core = Core::temporary("p").unwrap();
    let mut first = catalog_addon("first", Duration::ZERO, 200, METAS).await;
    let mut trending =
        named_catalog_addon("trending", "Trending", Duration::ZERO, 200, GODFATHER_FIRST).await;
    (first.position, trending.position) = (0, 1);
    core.library()
        .set_addons("p", &[first, trending], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    hero_until(&view, "tt0111161", "Popular Movies").await;
    let own = view.snapshot().rows[0].row_id.clone();
    core.set_home_row_hidden(&own, true).unwrap();
    hero_until(&view, "tt0068646", "Trending Movies").await;
}

/// The kept hero's row must still hold it: a row of the same title that no
/// longer lists it (its addon replaced by another) gives the hero up.
#[tokio::test]
async fn the_kept_hero_needs_its_row_to_hold_it() {
    let core = Core::temporary("p").unwrap();
    let first = catalog_addon("first", Duration::ZERO, 200, METAS).await;
    let other = catalog_addon("other", Duration::ZERO, 200, GODFATHER_ONLY).await;
    core.library()
        .set_addons("p", &[first], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    hero_until(&view, "tt0111161", "Popular Movies").await;
    core.library()
        .set_addons("p", &[other], &core.hlc())
        .unwrap();
    hero_until(&view, "tt0068646", "Popular Movies").await;
}

/// A clash that appears after the hero shows changes nothing on screen: the
/// new twin stays off Home and the hero keeps its tile and its words.
#[tokio::test]
async fn a_twin_installed_after_the_hero_shows_changes_nothing() {
    let core = Core::temporary("p").unwrap();
    let mut first = catalog_addon("first", Duration::ZERO, 200, METAS).await;
    let mut second = catalog_addon("second", Duration::ZERO, 200, GODFATHER_ONLY).await;
    (first.position, second.position) = (0, 1);
    core.library()
        .set_addons("p", std::slice::from_ref(&first), &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    hero_until(&view, "tt0111161", "Popular Movies").await;
    let shown = view.snapshot().rows[0].row_id.clone();
    core.library()
        .set_addons("p", &[first, second], &core.hlc())
        .unwrap();
    for _ in 0..300 {
        if core.home_catalogs().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        core.home_catalogs()
            .iter()
            .map(|c| c.hidden)
            .collect::<Vec<_>>(),
        [false, true]
    );
    // Give an open Home the time to follow the install.
    tokio::time::sleep(Duration::from_millis(300)).await;
    rows_until(&view, &[&shown]).await;
    let hero = view.snapshot().header.heroes[0].clone();
    assert_eq!(
        (hero.tile.id.as_str(), hero.reason.as_str()),
        ("tt0111161", "Popular Movies")
    );
}

/// The only row's state once it is no longer loading; `None` once it left
/// Home.
async fn settled_state(body: &'static str) -> Option<RowState> {
    let core = Core::temporary("p").unwrap();
    let addon = catalog_addon("trakt", Duration::ZERO, 200, body).await;
    core.library()
        .set_addons("p", &[addon], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    for _ in 0..300 {
        let state = view.snapshot().rows.first().map(|r| r.state.clone());
        if state != Some(RowState::Loading) {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the row never answered");
}

/// Trakt Tv's "Trending movies" answered `{}` (2026-10-22): the row leaves
/// Home (the owner's rule: empty sections collapse), never "This row did not
/// load". An error object still fails the row.
#[tokio::test]
async fn an_empty_answer_leaves_home_and_an_error_object_fails_it() {
    const TRAKT_TRENDING: &str =
        include_str!("../../mq-protocol/tests/fixtures/trakt_trending_movies.json");
    assert_eq!(settled_state(TRAKT_TRENDING).await, None);
    assert!(matches!(
        settled_state(r#"{"err": "upstream failed"}"#).await,
        Some(RowState::Failed { .. })
    ));
}

/// A history catalog lists a series once per episode watched (Trakt History
/// Series showed DARK five times, 2026-10-28): the row shows each title
/// once, where it first shows, and still fills from the titles after it.
#[tokio::test]
async fn a_title_a_catalog_repeats_shows_once_where_it_first_shows() {
    const HISTORY: &str = r#"{"metas":[
  {"id":"tt5753856","type":"series","name":"Dark"},
  {"id":"tt5753856","type":"series","name":"Dark"},
  {"id":"tt0903747","type":"series","name":"Breaking Bad"},
  {"id":"tt5753856","type":"series","name":"Dark"},
  {"id":"tt5753856","type":"series","name":"Dark"},
  {"id":"tt5753856","type":"series","name":"Dark"},
  {"id":"tt0944947","type":"series","name":"Game of Thrones"}
]}"#;
    let core = Core::temporary("p").unwrap();
    let addon = named_catalog_addon("trakt", "History", Duration::ZERO, 200, HISTORY).await;
    core.library()
        .set_addons("p", &[addon], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    let id = view.snapshot().rows[0].row_id.clone();
    row_until(&view, &id, |r| r.state == RowState::Ready).await;
    let ids: Vec<String> = row(&view, &id).tiles.into_iter().map(|t| t.id).collect();
    assert_eq!(ids, ["tt5753856", "tt0903747", "tt0944947"]);
}

/// Home shows no row `id` at any look over the next 200 ms.
async fn absent_for_200ms(view: &HomeView, id: &str) {
    for _ in 0..20 {
        let rows = view.snapshot().rows;
        assert!(rows.iter().all(|r| r.row_id != id), "{rows:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The owner's Trakt Watchlist and Collection catalogs answer 200 with no
/// metas (2026-10-28): an empty catalog never reaches Home as a blank row,
/// the rows around it keep their places, and a later edit of the board
/// (another row hidden, then shown) does not bring it back to load again.
#[tokio::test]
async fn an_empty_catalog_never_reaches_home() {
    let core = Core::temporary("p").unwrap();
    let mut full = named_catalog_addon("full", "Popular", Duration::ZERO, 200, METAS).await;
    // Slow to answer, so a reload would show its loading row for a while.
    let mut quiet = named_catalog_addon(
        "quiet",
        "Watchlist",
        Duration::from_millis(300),
        200,
        r#"{"metas":[]}"#,
    )
    .await;
    let mut other = named_catalog_addon("other", "Other", Duration::ZERO, 200, METAS).await;
    (full.position, quiet.position, other.position) = (0, 1, 2);
    core.library()
        .set_addons("p", &[full, quiet, other], &core.hlc())
        .unwrap();
    let ids: Vec<String> = core.home_catalogs().into_iter().map(|c| c.row_id).collect();
    let view = HomeView::open(&core);
    rows_until(&view, &[&ids[0], &ids[2]]).await;
    // Watched right after each edit: a reload would show its loading row
    // for the quiet catalog's 300 ms.
    core.set_home_row_hidden(&ids[2], true).unwrap();
    absent_for_200ms(&view, &ids[1]).await;
    rows_until(&view, &[&ids[0]]).await;
    core.set_home_row_hidden(&ids[2], false).unwrap();
    absent_for_200ms(&view, &ids[1]).await;
    rows_until(&view, &[&ids[0], &ids[2]]).await;
}

/// A catalog that gives its IMDb titles a landscape shape still shows each
/// as RPDB's 2:3 cover with the ratings band, shape "poster", its own wide
/// image kept as the fallback and its background untouched.
#[tokio::test]
async fn a_landscape_catalog_tile_of_an_imdb_title_gets_the_cover() {
    const WIDE: &str = r#"{"metas":[
  {"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","posterShape":"landscape",
   "poster":"https://img.example/wide.jpg","background":"https://img.example/bg.jpg"}
]}"#;
    let core = Core::temporary("p").unwrap();
    let addon = catalog_addon("wide", Duration::ZERO, 200, WIDE).await;
    core.library()
        .set_addons("p", &[addon, rt_addon()], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    for _ in 0..300 {
        if view.snapshot().rows[0].state == RowState::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let tile = view.snapshot().rows[0].tiles[0].clone();
    assert_eq!(
        tile.poster.as_deref(),
        Some(
            "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0111161.jpg?fallback=true"
        )
    );
    assert_eq!(tile.poster_shape, "poster");
    assert_eq!(
        tile.poster_fallback.as_deref(),
        Some("https://img.example/wide.jpg")
    );
    assert_eq!(
        tile.background.as_deref(),
        Some("https://img.example/bg.jpg")
    );
}

/// The band comes from the addons the user added (the owner, 2026-10-23):
/// with no enabled addon configured with an RPDB key (none, off, keyless),
/// every tile keeps its addon's poster; with one, whichever addon it is,
/// the key in its link draws the band.
#[tokio::test]
async fn the_band_follows_an_enabled_addons_rpdb_key() {
    let core = Core::temporary("p").unwrap();
    let catalog = catalog_addon("fast", Duration::ZERO, 200, METAS).await;
    let mut off = rt_addon();
    off.enabled = false;
    let mut keyless = rt_addon();
    keyless.transport_url =
        "https://7a82163c306e-rottentomatoes.baby-beamup.club/manifest.json".into();
    let paid = AddonRow {
        transport_url: "https://7a82163c306e-rottentomatoes.baby-beamup.club/%7B%22rpdb_key%22%3A%22t1-paid%22%7D/manifest.json".into(),
        ..rt_addon()
    };
    let other = AddonRow {
        transport_url: "https://other.example/%7B%22rpdb_key%22%3A%22t2-other%22%7D/manifest.json"
            .into(),
        manifest_json: serde_json::json!({
            "id": "other.posters", "version": "1.0.0", "name": "Other",
            "types": ["movie"], "resources": ["catalog"], "catalogs": []
        })
        .to_string(),
        ..rt_addon()
    };
    let own = "https://images.metahub.space/poster/medium/tt0111161/img";
    for (rt, cover) in [
        (None, own.to_owned()),
        (Some(off), own.to_owned()),
        (Some(keyless), own.to_owned()),
        (
            Some(paid),
            "https://api.ratingposterdb.com/t1-paid/imdb/poster-default/tt0111161.jpg?fallback=true"
                .to_owned(),
        ),
        (
            Some(other),
            "https://api.ratingposterdb.com/t2-other/imdb/poster-default/tt0111161.jpg?fallback=true"
                .to_owned(),
        ),
    ] {
        let label = format!("{:?}", rt.as_ref().map(|r| (&r.transport_url, r.enabled)));
        let addons: Vec<AddonRow> = std::iter::once(catalog.clone()).chain(rt).collect();
        core.library().set_addons("p", &addons, &core.hlc()).unwrap();
        let view = HomeView::open(&core);
        for _ in 0..300 {
            if view.snapshot().rows[0].state == RowState::Ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let tile = view.snapshot().rows[0].tiles[0].clone();
        assert_eq!(tile.poster.as_deref(), Some(cover.as_str()), "{label}");
    }
}

/// Waits until the first row's first tile, and the hero when there is one,
/// show `poster`.
async fn until_cover(view: &HomeView, poster: &str, what: &str) {
    for _ in 0..300 {
        let snap = view.snapshot();
        let tile = snap.rows.first().and_then(|r| r.tiles.first());
        let hero = snap.header.heroes.first().map(|h| h.tile.poster.clone());
        if tile.is_some_and(|t| t.poster.as_deref() == Some(poster))
            && hero.is_none_or(|h| h.as_deref() == Some(poster))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "{what}: {:?}",
        view.snapshot()
            .rows
            .first()
            .map(|r| r.tiles.first().cloned())
    );
}

/// Home stays open while the addons change: removing the addon that gives
/// the RPDB key takes the band off every tile already drawn, the hero's too,
/// and adding it back draws the band again, on the same live view. A wide
/// tile gets its own shape back with its own picture.
#[tokio::test]
async fn a_live_home_follows_the_rpdb_key_as_the_addon_comes_and_goes() {
    const WIDE: &str = r#"{"metas":[
  {"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","posterShape":"landscape",
   "poster":"https://images.metahub.space/poster/medium/tt0111161/img"}
]}"#;
    let core = Core::temporary("p").unwrap();
    let catalog = catalog_addon("fast", Duration::ZERO, 200, WIDE).await;
    let own = "https://images.metahub.space/poster/medium/tt0111161/img";
    let band = "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0111161.jpg?fallback=true";
    let set = |with_key: bool| {
        let addons: Vec<AddonRow> = std::iter::once(catalog.clone())
            .chain(with_key.then(rt_addon))
            .collect();
        core.library()
            .set_addons("p", &addons, &core.hlc())
            .unwrap();
    };
    set(true);
    let view = HomeView::open(&core);
    until_cover(&view, band, "the band with the key").await;
    set(false);
    until_cover(&view, own, "the addon's poster once the key is gone").await;
    let plain = view.snapshot().rows[0].tiles[0].clone();
    assert_eq!(plain.poster_fallback, None, "no fallback left behind");
    assert_eq!(plain.poster_shape, "landscape", "its own shape back");
    set(true);
    until_cover(&view, band, "the band again with the key back").await;
    let banded = view.snapshot().rows[0].tiles[0].clone();
    assert_eq!(banded.poster_fallback.as_deref(), Some(own));
    assert_eq!(banded.poster_shape, "poster");
}

/// A catalog body of `n` movies with backdrops, ids `tt<prefix>00` up; the
/// first `bare` have no backdrop, each has two genres and a trailer.
fn movies(prefix: &str, n: usize, bare: usize) -> &'static str {
    let metas: Vec<String> = (0..n)
        .map(|i| {
            let id = format!("tt{prefix}{i:02}");
            let bg = if i < bare {
                String::new()
            } else {
                format!(r#","background":"https://images.metahub.space/background/medium/{id}/img""#)
            };
            format!(
                r#"{{"id":"{id}","type":"movie","name":"{id}"{bg},"links":[{{"name":"Drama","category":"Genres","url":"stremio:///discover"}},{{"name":"Crime","category":"Genres","url":"stremio:///discover"}}],"trailerStreams":[{{"title":"{id}","ytId":"cdx31ak4KbQ"}}]}}"#
            )
        })
        .collect();
    Box::leak(format!(r#"{{"metas":[{}]}}"#, metas.join(",")).into_boxed_str())
}

fn hero_ids(view: &HomeView) -> Vec<String> {
    view.snapshot()
        .header
        .heroes
        .iter()
        .map(|h| h.tile.id.clone())
        .collect()
}

/// Waits up to 5 s for `ok` on the snapshot.
async fn view_until(view: &HomeView, what: &str, ok: impl Fn(&HomeView) -> bool) {
    for _ in 0..500 {
        if ok(view) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let rows: Vec<_> = view
        .snapshot()
        .rows
        .into_iter()
        .map(|r| (r.title, r.state))
        .collect();
    panic!("{what}: {:?} {rows:?}", view.snapshot().header.heroes);
}

/// The carousel: rows count only up to the first one still loading, so a
/// fast row further down never jumps ahead; round one takes each row's first
/// backdrop, and only once nothing loads do later rounds take a second, at
/// most 2 from a row and 8 in all. Pages only grow, and moving a row moves
/// none of them.
#[tokio::test]
async fn heroes_take_rows_in_order_two_at_most_eight_in_all() {
    let core = Core::temporary("p").unwrap();
    let mut addons = Vec::new();
    for (i, prefix) in ["10", "20", "30", "40", "50"].into_iter().enumerate() {
        let delay = if i == 1 {
            Duration::from_millis(600)
        } else {
            Duration::ZERO
        };
        let mut addon = named_catalog_addon(
            &format!("a{i}"),
            &format!("List {i}"),
            delay,
            200,
            movies(prefix, 3, 0),
        )
        .await;
        addon.position = i as i64;
        addons.push(addon);
    }
    core.library()
        .set_addons("p", &addons, &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    view_until(
        &view,
        "the first row alone, the second still loading",
        |v| {
            let rows = v.snapshot().rows;
            rows[0].state == RowState::Ready && rows[2].state == RowState::Ready
        },
    )
    .await;
    assert_eq!(rows_loading(&view), 1, "the slow row still loads");
    assert_eq!(hero_ids(&view), ["tt1000"], "nothing past a loading row");
    view_until(&view, "every row in", |v| {
        v.snapshot().header.heroes.len() == 8
    })
    .await;
    assert_eq!(
        hero_ids(&view),
        [
            "tt1000", "tt2000", "tt3000", "tt4000", "tt5000", "tt1001", "tt2001", "tt3001"
        ]
    );
    let heroes = view.snapshot().header.heroes;
    assert_eq!(
        heroes[1].reason, "List 1 Movies",
        "no number outside a ranking"
    );
    assert_eq!(heroes[0].trailer.as_deref(), Some("cdx31ak4KbQ"));
    // Moving a row moves no page.
    let third = view.snapshot().rows[2].row_id.clone();
    core.move_home_row(&third, -2).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(view.snapshot().rows[0].row_id, third, "the row moved");
    assert_eq!(
        hero_ids(&view),
        [
            "tt1000", "tt2000", "tt3000", "tt4000", "tt5000", "tt1001", "tt2001", "tt3001"
        ]
    );
}

fn rows_loading(view: &HomeView) -> usize {
    view.snapshot()
        .rows
        .iter()
        .filter(|r| r.state == RowState::Loading)
        .count()
}

/// Cinemeta's Popular is a ranking: its row reads "Top 10 Movies" and holds
/// 10 tiles with their place and first genre, and a hero from it names the
/// catalog with its real place ("#3": the first two have no backdrop).
/// Another addon's catalog of the same id is no ranking.
#[tokio::test]
async fn a_ranked_catalog_is_a_top_10_row_with_real_places() {
    let core = Core::temporary("p").unwrap();
    let mut cinemeta = catalog_addon(
        "com.linvo.cinemeta",
        Duration::ZERO,
        200,
        movies("60", 12, 2),
    )
    .await;
    // Named apart, so it is no twin of Cinemeta's Popular Movies.
    let mut other = named_catalog_addon(
        "other",
        "Trending",
        Duration::ZERO,
        200,
        movies("70", 12, 2),
    )
    .await;
    (cinemeta.position, other.position) = (0, 1);
    core.library()
        .set_addons("p", &[cinemeta, other], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    view_until(&view, "both rows in", |v| {
        v.snapshot().rows.iter().all(|r| r.state == RowState::Ready)
    })
    .await;
    let rows = view.snapshot().rows;
    assert_eq!(rows[0].title, "Top 10 Movies");
    assert!(rows[0].ranked);
    assert_eq!(rows[0].tiles.len(), 10);
    let places: Vec<Option<u8>> = rows[0].tiles.iter().map(|t| t.rank).collect();
    assert_eq!(places, (1..=10).map(Some).collect::<Vec<_>>());
    assert_eq!(rows[0].tiles[0].genre.as_deref(), Some("Drama"));
    assert!(!rows[0].title.to_lowercase().contains("cinemeta"));
    // The other addon's catalog keeps its own name and draws no numerals.
    assert_eq!(rows[1].title, "Trending Movies");
    assert!(!rows[1].ranked);
    assert_eq!(rows[1].tiles.len(), 12);
    assert!(
        rows[1]
            .tiles
            .iter()
            .all(|t| t.rank.is_none() && t.genre.is_none())
    );
    let heroes = view.snapshot().header.heroes;
    assert_eq!(
        (heroes[0].tile.id.as_str(), heroes[0].reason.as_str()),
        ("tt6002", "#3 in Popular Movies")
    );
    assert_eq!(heroes[1].reason, "Trending Movies");
    // Customize Home keeps the row's id, so a hidden Top 10 stays hidden.
    let id = rows[0].row_id.clone();
    assert!(
        core.home_catalogs()
            .iter()
            .any(|c| c.row_id == id && c.title == "Top 10 Movies")
    );
    core.set_home_row_hidden(&id, true).unwrap();
    rows_until(&view, &[&rows[1].row_id]).await;
}

/// A trailer id goes into a player's page, so only YouTube's own shape
/// passes: anything else leaves the hero without a trailer.
#[tokio::test]
async fn only_a_checked_youtube_id_is_a_hero_trailer() {
    const BODY: &str = r#"{"metas":[
      {"id":"tt0111161","type":"movie","name":"A","background":"https://images.metahub.space/background/medium/tt0111161/img","trailerStreams":[{"title":"A","ytId":"cdx31ak4KbQ&list=PLx"}]}
    ]}"#;
    let core = Core::temporary("p").unwrap();
    let addon = catalog_addon("a", Duration::ZERO, 200, BODY).await;
    core.library()
        .set_addons("p", &[addon], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    view_until(&view, "a hero", |v| !v.snapshot().header.heroes.is_empty()).await;
    assert_eq!(view.snapshot().header.heroes[0].trailer, None);
}

/// Two rows of five backdrops give four pages, two from each: one catalog
/// never fills the carousel.
#[tokio::test]
async fn a_row_gives_the_carousel_two_pages_at_most() {
    let core = Core::temporary("p").unwrap();
    let mut a = named_catalog_addon("a", "List a", Duration::ZERO, 200, movies("80", 5, 0)).await;
    let mut b = named_catalog_addon("b", "List b", Duration::ZERO, 200, movies("90", 5, 0)).await;
    (a.position, b.position) = (0, 1);
    core.library()
        .set_addons("p", &[a, b], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    view_until(&view, "both rows in", |v| {
        v.snapshot().rows.iter().all(|r| r.state == RowState::Ready)
    })
    .await;
    view_until(&view, "four pages", |v| {
        v.snapshot().header.heroes.len() >= 4
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(hero_ids(&view), ["tt8000", "tt9000", "tt8001", "tt9001"]);
}

/// A Top 10 row keeps the twin rules its catalog had (review r1, M1): another
/// addon's "Popular Movies" after Cinemeta's Popular stays a hidden twin,
/// though Cinemeta's row now reads "Top 10 Movies"; and in the other order
/// Cinemeta's is the twin.
#[tokio::test]
async fn a_top_10_row_leaves_the_twins_as_they_were() {
    let core = Core::temporary("p").unwrap();
    let mut cinemeta = catalog_addon(
        "com.linvo.cinemeta",
        Duration::ZERO,
        200,
        movies("60", 12, 0),
    )
    .await;
    let mut tmdb = catalog_addon("tmdb", Duration::ZERO, 200, movies("70", 12, 0)).await;
    let listed = |core: &Core| -> Vec<(String, bool)> {
        core.home_catalogs()
            .into_iter()
            .map(|c| (c.title, c.hidden))
            .collect()
    };
    (cinemeta.position, tmdb.position) = (0, 1);
    core.library()
        .set_addons("p", &[cinemeta.clone(), tmdb.clone()], &core.hlc())
        .unwrap();
    assert_eq!(
        listed(&core),
        [
            ("Top 10 Movies".into(), false),
            ("Popular Movies".into(), true)
        ]
    );
    (cinemeta.position, tmdb.position) = (1, 0);
    core.library()
        .set_addons("p", &[tmdb, cinemeta], &core.hlc())
        .unwrap();
    assert_eq!(
        listed(&core),
        [
            ("Popular Movies".into(), false),
            ("Top 10 Movies".into(), true)
        ]
    );
}

/// One addon with a series and a movie catalog, each one title with a
/// backdrop, and the series' meta (`meta`, answered with `status` after
/// `delay`); the count of meta requests.
async fn hero_play_addon(
    status: u16,
    meta: &'static str,
    delay: Duration,
) -> (AddonRow, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let count = asked.clone();
    const SHOW: &str = r#"{"metas":[{"id":"tt9","type":"series","name":"Show","background":"https://images.example/tt9-bg.jpg"}]}"#;
    const FILM: &str = r#"{"metas":[{"id":"tt1","type":"movie","name":"Film","background":"https://images.example/tt1-bg.jpg"}]}"#;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let count = count.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                let path = head.split_whitespace().nth(1).unwrap_or("");
                let (code, body) = if path.starts_with("/catalog/series/") {
                    (200, SHOW)
                } else if path.starts_with("/catalog/movie/") {
                    (200, FILM)
                } else if path.starts_with("/meta/series/tt9") {
                    count.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(delay).await;
                    (status, meta)
                } else {
                    (404, "{}")
                };
                // The socket closes after one answer: the client must not reuse it.
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "heroes", "version": "1.0.0", "name": "Heroes",
        "types": ["movie", "series"], "resources": ["catalog", "meta"], "idPrefixes": ["tt"],
        "catalogs": [
            {"type": "series", "id": "top", "name": "Popular"},
            {"type": "movie", "id": "top", "name": "Popular"}
        ]
    });
    let addon = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    (addon, asked)
}

/// A hero test's Home on one `hero_play_addon`.
async fn hero_home(
    status: u16,
    meta: &'static str,
    delay: Duration,
) -> (Core, HomeView, Arc<AtomicUsize>) {
    let core = Core::temporary("p").unwrap();
    let (addon, asked) = hero_play_addon(status, meta, delay).await;
    core.library()
        .set_addons("p", &[addon], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    (core, view, asked)
}

const SHOW_META: &str = r#"{"meta":{"id":"tt9","type":"series","name":"Show","videos":[
  {"id":"tt9:2:1","title":"Return","season":2,"episode":1,"released":"2021-01-01T00:00:00Z"},
  {"id":"tt9:1:1","title":"Pilot","season":1,"episode":1,"released":"2020-01-01T00:00:00Z"},
  {"id":"tt9:1:2","title":"Second","season":1,"episode":2,"released":"2020-01-08T00:00:00Z"}]}}"#;

/// `SHOW_META` as an addon may write it: the meta's own id spelled
/// otherwise than the id it was asked for.
const SHOW_META_OTHER_ID: &str = r#"{"meta":{"id":"tt0000009","type":"series","name":"Show","videos":[
  {"id":"tt9:1:1","title":"Pilot","season":1,"episode":1,"released":"2020-01-01T00:00:00Z"},
  {"id":"tt9:1:2","title":"Second","season":1,"episode":2,"released":"2020-01-08T00:00:00Z"}]}}"#;

/// Play reads progress under the id the page asked for, as the title page
/// does, whatever id the addon writes in its meta (review r5, R5-2).
#[tokio::test]
async fn a_hero_reads_progress_under_its_own_id() {
    let (core, view, _) = hero_home(200, SHOW_META_OTHER_ID, Duration::ZERO).await;
    progress(&core, "tt9:1:2", "tt9", 400);
    view_until(&view, "the episode in progress", |v| {
        hero_play(v, "tt9") == Some(("tt9:1:2".to_owned(), "Resume S1:E2".to_owned(), Some(400)))
    })
    .await;
}

/// S1:E1 out, S1:E2 out only in 2099.
const UNAIRED_META: &str = r#"{"meta":{"id":"tt9","type":"series","name":"Show","videos":[
  {"id":"tt9:1:1","title":"Pilot","season":1,"episode":1,"released":"2020-01-01T00:00:00Z"},
  {"id":"tt9:1:2","title":"Later","season":1,"episode":2,"released":"2099-01-01T00:00:00Z"}]}}"#;

/// A hero page's Play, by title id: (video, label, resume position).
fn hero_play(view: &HomeView, id: &str) -> Option<(String, String, Option<i64>)> {
    let hero = view
        .snapshot()
        .header
        .heroes
        .into_iter()
        .find(|h| h.tile.id == id)?;
    hero.play.map(|p| (p.video_id, p.label, p.resume_ms))
}

fn progress(core: &Core, video: &'static str, item: &'static str, position_ms: i64) {
    core.library()
        .set_progress(
            "p",
            ProgressRow {
                video_id: video,
                item_id: item,
                position_ms,
                duration_ms: 1000,
                updated_ms: position_ms,
            },
            &core.hlc(),
        )
        .unwrap();
}

/// Every hero page carries the main action the title page's button has
/// (the owner, 2026-10-26: Carrie showed Details alone): a series never
/// watched plays S1:E1, one in progress resumes its episode, one finished to
/// a season's last episode plays the next season's first; a movie plays or
/// resumes. It follows progress live, with Home open.
#[tokio::test]
async fn every_hero_page_plays_what_its_title_page_would() {
    let (core, view, asked) = hero_home(200, SHOW_META, Duration::ZERO).await;
    let want = |id: &'static str, play: (&'static str, &'static str, Option<i64>)| {
        move |v: &HomeView| hero_play(v, id) == Some((play.0.to_owned(), play.1.to_owned(), play.2))
    };
    view_until(
        &view,
        "a series never watched",
        want("tt9", ("tt9:1:1", "Play S1:E1", None)),
    )
    .await;
    view_until(&view, "a movie", want("tt1", ("tt1", "Play", None))).await;

    progress(&core, "tt9:1:2", "tt9", 400);
    view_until(
        &view,
        "a series in progress",
        want("tt9", ("tt9:1:2", "Resume S1:E2", Some(400))),
    )
    .await;

    progress(&core, "tt9:1:2", "tt9", 990);
    view_until(
        &view,
        "the next season",
        want("tt9", ("tt9:2:1", "Play S2:E1", None)),
    )
    .await;

    progress(&core, "tt1", "tt1", 300);
    view_until(
        &view,
        "a movie in progress",
        want("tt1", ("tt1", "Resume", Some(300))),
    )
    .await;

    // Watched to its last episode: from the first again, in words that fit
    // the hero's button.
    progress(&core, "tt9:2:1", "tt9", 990);
    view_until(
        &view,
        "a finished series",
        want("tt9", ("tt9:1:1", "Play again", None)),
    )
    .await;
    // The meta was read once; progress reads Play again from it.
    assert_eq!(asked.load(Ordering::SeqCst), 1, "meta requests");
}

/// One slow series meta holds back its own page alone (review r4, W1): the
/// movie's Play comes at once, the series' page says its Play may still come,
/// then gets it.
#[tokio::test]
async fn a_slow_series_meta_holds_back_only_its_page() {
    let (_core, view, _) = hero_home(200, SHOW_META, Duration::from_secs(3)).await;
    view_until(&view, "the movie's Play", |v| hero_play(v, "tt1").is_some()).await;
    let series = view
        .snapshot()
        .header
        .heroes
        .into_iter()
        .find(|h| h.tile.id == "tt9")
        .expect("the series page");
    assert_eq!((series.play, series.play_pending), (None, true));
    view_until(&view, "the series' Play", |v| hero_play(v, "tt9").is_some()).await;
}

/// A series whose next episode is not out offers Details alone, as its title
/// page offers no Play (it says when the episode airs).
#[tokio::test]
async fn a_hero_series_waiting_for_its_next_episode_offers_details_alone() {
    let (core, view, _) = hero_home(200, UNAIRED_META, Duration::ZERO).await;
    view_until(&view, "S1:E1 first", |v| hero_play(v, "tt9").is_some()).await;
    progress(&core, "tt9:1:1", "tt9", 990);
    view_until(&view, "no Play", |v| {
        v.snapshot()
            .header
            .heroes
            .iter()
            .any(|h| h.tile.id == "tt9" && h.play.is_none() && !h.play_pending)
    })
    .await;
}

/// A series whose episodes the hero does not know yet reads them from its
/// meta (above); with no meta answer it has no Play and offers Details
/// alone, and a meta that lists no episodes plays the title as one, as the
/// title page does.
#[tokio::test]
async fn a_hero_series_without_episodes_offers_what_its_meta_allows() {
    for (status, meta, want) in [
        (404, "{}", None),
        (
            200,
            r#"{"meta":{"id":"tt9","type":"series","name":"Show"}}"#,
            Some(("tt9".to_owned(), "Play".to_owned(), None)),
        ),
    ] {
        let (core, view, asked) = hero_home(status, meta, Duration::ZERO).await;
        view_until(&view, "the series page settled", |v| {
            v.snapshot()
                .header
                .heroes
                .iter()
                .any(|h| h.tile.id == "tt9" && !h.play_pending)
        })
        .await;
        assert_eq!(hero_play(&view, "tt9"), want, "meta answered {status}");
        // A meta that did not answer is not asked again on every progress write.
        let before = asked.load(Ordering::SeqCst);
        progress(&core, "tt1", "tt1", 100);
        progress(&core, "tt1", "tt1", 200);
        view_until(&view, "the movie resumes", |v| {
            hero_play(v, "tt1").is_some_and(|p| p.2 == Some(200))
        })
        .await;
        assert_eq!(
            asked.load(Ordering::SeqCst),
            before,
            "meta asked again ({status})"
        );
        if status == 404 {
            // And asked again on its own once the minute has passed (review r5, R5-1), with nothing else
            // happening on Home.
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(61)).await;
            tokio::time::resume();
            for _ in 0..500 {
                if asked.load(Ordering::SeqCst) > before {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(
                asked.load(Ordering::SeqCst) > before,
                "a failed meta asked again after a minute"
            );
        }
    }
}

/// A TMDB fake answering after `delay`: each request line starting `GET <prefix>` gets its body; anything else a
/// 404.
async fn fake_tmdb(delay: Duration, answers: &[(&str, &'static str)]) -> String {
    let answers: Vec<(String, &'static str)> = answers
        .iter()
        .map(|(p, b)| (format!("GET {p}"), *b))
        .collect();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let answers = answers.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                tokio::time::sleep(delay).await;
                let head = String::from_utf8_lossy(&buf[..n]);
                let resp = match answers.iter().find(|(p, _)| head.starts_with(p.as_str())) {
                    Some((_, body)) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    ),
                    None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_owned(),
                };
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

/// The hero's backdrop follows first run's still (plans/batch-2.md F7): a
/// page shows its addon's background at once, then TMDB's textless scene
/// frame of its title once TMDB answers; a title TMDB holds only titled
/// backdrops for keeps the addon's.
#[tokio::test]
async fn a_hero_backdrop_becomes_tmdbs_textless_frame_without_waiting_for_it() {
    const BODY: &str = r#"{"metas":[
      {"id":"tt0000001","type":"series","name":"A","background":"https://images.metahub.space/background/medium/tt0000001/img"},
      {"id":"tt0000002","type":"movie","name":"B","background":"https://images.metahub.space/background/medium/tt0000002/img"}
    ]}"#;
    let core = Core::temporary("p").unwrap();
    let tmdb = fake_tmdb(
        Duration::from_millis(300),
        &[
            ("/3/find/tt0000001?", r#"{"movie_results":[],"tv_results":[{"id":10,"vote_average":7.1,"vote_count":90}]}"#),
            ("/3/find/tt0000002?", r#"{"movie_results":[{"id":20,"vote_average":6.4,"vote_count":40}],"tv_results":[]}"#),
            (
                "/3/tv/10/images?include_image_language=null ",
                r#"{"backdrops":[{"file_path":"/scene.jpg","iso_639_1":null,"width":1920,"aspect_ratio":1.778}]}"#,
            ),
            (
                "/3/movie/20/images?include_image_language=null ",
                r#"{"backdrops":[{"file_path":"/keyart.jpg","iso_639_1":"en","width":3840,"aspect_ratio":1.778}]}"#,
            ),
        ],
    )
    .await;
    core.set_tmdb_host(mq_runtime::TmdbHost {
        url: tmdb,
        token: Some("test".into()),
        local: true,
    });
    let addon = catalog_addon("a", Duration::ZERO, 200, BODY).await;
    core.library()
        .set_addons("p", &[addon], &core.hlc())
        .unwrap();
    let view = HomeView::open(&core);
    let backdrops = |v: &HomeView| -> Vec<Option<String>> {
        v.snapshot()
            .header
            .heroes
            .into_iter()
            .map(|h| h.tile.background)
            .collect()
    };
    view_until(&view, "two pages", |v| {
        v.snapshot().header.heroes.len() == 2
    })
    .await;
    assert_eq!(
        backdrops(&view),
        [
            Some("https://images.metahub.space/background/medium/tt0000001/img".to_owned()),
            Some("https://images.metahub.space/background/medium/tt0000002/img".to_owned()),
        ],
        "the pages came before TMDB answered, with the addon's backdrops"
    );
    view_until(&view, "A's scene frame", |v| {
        backdrops(v)[0].as_deref() == Some("https://image.tmdb.org/t/p/w1280/scene.jpg")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        backdrops(&view)[1].as_deref(),
        Some("https://images.metahub.space/background/medium/tt0000002/img"),
        "B has only titled backdrops on TMDB"
    );
}
