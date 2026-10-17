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
  {"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","poster":"https://images.metahub.space/poster/medium/tt0111161/img","background":"https://images.metahub.space/background/medium/tt0111161/img","releaseInfo":"1994"},
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
