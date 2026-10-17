use futures_util::StreamExt;
use mq_library::{AddonRow, LibraryItemRow, ProgressRow};
use mq_runtime::views::home::{CONTINUE_ROW_ID, HomeRow};
use mq_runtime::{Core, HomeUpdate, HomeView, RowState};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Local catalog addon answering after `delay` with `status` and `body`.
async fn catalog_addon(id: &str, delay: Duration, status: u16, body: &'static str) -> AddonRow {
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
            {"type": "movie", "id": "top", "name": "Popular"},
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
    let slow = catalog_addon("slow", Duration::from_millis(2800), 200, METAS).await;
    let broken = catalog_addon("broken", Duration::ZERO, 500, "{}").await;
    core.library()
        .set_addons("p", &[fast, slow, broken], &core.hlc())
        .unwrap();

    let view = HomeView::open(&core);
    let (snapshot, mut updates) = view.subscribe();
    assert_eq!(snapshot.rows.len(), 3, "search-only catalogs are skipped");
    assert!(snapshot.rows.iter().all(|r| r.title == "Popular Movies"));
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
    let hero = view
        .snapshot()
        .header
        .hero
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
    // The hero features a catalog, never the first Continue Watching tile
    // right below it; with no catalog rows there is none.
    assert!(view.snapshot().header.hero.is_none());
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
    let second = catalog_addon("second", Duration::ZERO, 200, METAS).await;
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
    assert!(view.snapshot().header.hero.is_none(), "no catalog, no hero");
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
    let mut b = catalog_addon("b", Duration::ZERO, 200, METAS).await;
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
async fn two_installations_of_one_addon_are_two_rows() {
    let core = Core::temporary("p").unwrap();
    let mut first = catalog_addon("same", Duration::ZERO, 200, METAS).await;
    let mut second = catalog_addon("same", Duration::ZERO, 200, METAS).await;
    first.position = 0;
    second.position = 1;
    core.library()
        .set_addons("p", &[first, second], &core.hlc())
        .unwrap();
    let ids: Vec<String> = core.home_catalogs().into_iter().map(|c| c.row_id).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    let view = HomeView::open(&core);
    for _ in 0..300 {
        if view
            .snapshot()
            .rows
            .iter()
            .all(|r| r.state == RowState::Ready)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        view.snapshot()
            .rows
            .iter()
            .all(|r| r.state == RowState::Ready && !r.tiles.is_empty())
    );
}
