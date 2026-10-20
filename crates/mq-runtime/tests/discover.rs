use mq_library::AddonRow;
use mq_runtime::{Core, DiscoverState, DiscoverView};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Catalog addon: `top` pages 3, 2, then nothing (one repeat per page to
/// check dedupe); `new` needs a genre. Records the paths it was asked.
async fn addon() -> (AddonRow, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                log.lock().unwrap().push(path.clone());
                let ids: Vec<u32> = if path.contains("skip=5") {
                    vec![]
                } else if path.contains("skip=3") {
                    vec![2, 3, 4]
                } else {
                    vec![0, 1, 2]
                };
                let metas: Vec<_> = ids
                    .iter()
                    .map(|i| serde_json::json!({"id": format!("tt{i}"), "type": "movie", "name": format!("M{i}")}))
                    .collect();
                let body = serde_json::json!({ "metas": metas }).to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "cat", "version": "1.0.0", "name": "Cat", "types": ["movie"], "resources": ["catalog"],
        "catalogs": [
            {"type": "movie", "id": "top", "name": "Popular",
             "extra": [{"name": "genre", "options": ["Action", "Drama"]}, {"name": "skip"}]},
            {"type": "movie", "id": "new", "name": "New",
             "extra": [{"name": "genre", "isRequired": true, "options": ["2026", "2025"]}]},
            {"type": "movie", "id": "find", "extra": [{"name": "search", "isRequired": true}]},
            {"type": "series", "id": "feed", "extra": [{"name": "lastVideosIds", "isRequired": true}]}
        ]
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

async fn settle(view: &DiscoverView) {
    for _ in 0..300 {
        if view.snapshot().state != DiscoverState::Loading {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("discover never settled");
}

fn ids(view: &DiscoverView) -> Vec<String> {
    view.snapshot().items.iter().map(|t| t.id.clone()).collect()
}

#[tokio::test]
async fn browses_pages_genres_and_required_filters() {
    let core = Core::temporary("p").unwrap();
    let (row, asked) = addon().await;
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();

    let view = DiscoverView::open(&core);
    settle(&view).await;
    let s = view.snapshot();
    // Search-only catalogs and id-list feeds are not browsable.
    assert_eq!(s.kinds, [("movie".to_string(), "Movies".to_string())]);
    let names: Vec<_> = s.catalogs.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Popular", "New"]);
    assert_eq!(ids(&view), ["tt0", "tt1", "tt2"]);
    assert_eq!(view.snapshot().state, DiscoverState::Ready);

    view.load_more();
    settle(&view).await;
    assert_eq!(
        ids(&view),
        ["tt0", "tt1", "tt2", "tt3", "tt4"],
        "repeats dropped"
    );
    view.load_more();
    settle(&view).await;
    assert_eq!(view.snapshot().state, DiscoverState::End);

    view.select(None, None, Some(Some("Drama")));
    settle(&view).await;
    assert_eq!(view.snapshot().genre.as_deref(), Some("Drama"));
    assert!(
        asked
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .contains("genre=Drama")
    );

    // A catalog that needs a genre starts on its first option.
    let new = view.snapshot().catalogs[1].id.clone();
    view.select(None, Some(&new), None);
    settle(&view).await;
    let s = view.snapshot();
    assert!(s.genre_required);
    assert_eq!(s.genre.as_deref(), Some("2026"));
    assert_eq!(s.state, DiscoverState::End, "no skip extra, so one page");
}

#[tokio::test]
#[ignore = "network"]
async fn real_cinemeta_discover() {
    let core = Core::temporary("p").unwrap();
    core.install_addon("https://v3-cinemeta.strem.io/manifest.json")
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    let first = view.snapshot();
    eprintln!(
        "first page: {} items in {} ms, catalogs {:?}, {} genres",
        first.items.len(),
        started.elapsed().as_millis(),
        first.catalogs.iter().map(|c| &c.name).collect::<Vec<_>>(),
        first.genres.len()
    );
    assert!(first.items.len() >= 20);
    let t = std::time::Instant::now();
    view.load_more();
    settle(&view).await;
    eprintln!(
        "after load more: {} items in {} ms",
        view.snapshot().items.len(),
        t.elapsed().as_millis()
    );
    assert!(view.snapshot().items.len() > first.items.len());
    view.select(Some("series"), None, Some(Some("Drama")));
    settle(&view).await;
    let s = view.snapshot();
    eprintln!(
        "series drama: {} items, first {:?}",
        s.items.len(),
        s.items.first().map(|t| &t.name)
    );
    assert_eq!(s.kind.as_deref(), Some("series"));
    assert!(!s.items.is_empty());
}

async fn until(view: &DiscoverView, done: impl Fn(&mq_runtime::DiscoverSnapshot) -> bool) {
    for _ in 0..400 {
        let snapshot = view.snapshot();
        if done(&snapshot) && snapshot.state != DiscoverState::Loading {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("discover never got there: {:?}", view.snapshot());
}

/// Discover follows the installed addons: what is on screen stays when its
/// catalog stays, and it starts again on the first catalog when it goes.
#[tokio::test]
async fn discover_follows_addon_changes() {
    let core = Core::temporary("p").unwrap();
    let (cat, _) = addon().await;
    core.library()
        .set_addons("p", std::slice::from_ref(&cat), &core.hlc())
        .unwrap();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    let before = view.snapshot();
    assert_eq!(ids(&view), ["tt0", "tt1", "tt2"]);

    let mut other = cat.clone();
    other.transport_url = other
        .transport_url
        .replace("/manifest.json", "/other/manifest.json");
    other.manifest_json = other
        .manifest_json
        .replace("\"cat\"", "\"other\"")
        .replace("\"Cat\"", "\"Other\"");
    core.library()
        .set_addons("p", &[cat.clone(), other.clone()], &core.hlc())
        .unwrap();
    until(&view, |s| s.catalogs.len() == before.catalogs.len() * 2).await;
    let after = view.snapshot();
    assert_eq!(after.catalog, before.catalog, "the catalog on screen stays");
    assert_eq!(after.items, before.items, "and is not loaded again");

    core.library()
        .set_addons("p", std::slice::from_ref(&other), &core.hlc())
        .unwrap();
    until(&view, |s| {
        s.catalog != before.catalog && !s.items.is_empty()
    })
    .await;
    assert!(view.snapshot().catalog.unwrap().contains("other"));

    core.library().set_addons("p", &[], &core.hlc()).unwrap();
    until(&view, |s| s.catalog.is_none()).await;
    let empty = view.snapshot();
    assert!(
        empty.kinds.is_empty() && empty.items.is_empty(),
        "{empty:?}"
    );
}

/// A paging catalog whose every page fails once, then answers: the first
/// page has tt0 to tt2, `skip=3` has tt3, `skip=4` nothing. Records paths.
async fn flaky_addon() -> (AddonRow, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            let again = log.lock().unwrap().contains(&path);
            log.lock().unwrap().push(path.clone());
            let ids: Vec<u32> = if path.contains("skip=4") {
                vec![]
            } else if path.contains("skip=3") {
                vec![3]
            } else {
                vec![0, 1, 2]
            };
            let metas: Vec<_> = ids
                .iter()
                .map(|i| serde_json::json!({"id": format!("tt{i}"), "type": "movie", "name": format!("M{i}")}))
                .collect();
            let body = serde_json::json!({ "metas": metas }).to_string();
            let status = if again || path.contains("skip=4") {
                "200 OK"
            } else {
                "500 Internal Server Error"
            };
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    let manifest = serde_json::json!({
        "id": "flaky", "version": "1.0.0", "name": "Flaky", "types": ["movie"], "resources": ["catalog"],
        "catalogs": [{"type": "movie", "id": "top", "name": "Popular", "extra": [{"name": "skip"}]}]
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

/// Web journey review r3 P3-4: a failed page, and a failed catalog, are
/// asked again by `load_more`; the items already shown stay.
#[tokio::test]
async fn a_failed_page_is_asked_again() {
    let core = Core::temporary("p").unwrap();
    let (addon, asked) = flaky_addon().await;
    core.library()
        .set_addons(core.profile(), &[addon], &core.hlc())
        .unwrap();
    let view = DiscoverView::open(&core);
    let failed = |s: &mq_runtime::DiscoverSnapshot| matches!(&s.state, DiscoverState::Failed { reason } if !reason.is_empty());
    until(&view, failed).await;
    assert!(view.snapshot().items.is_empty(), "the catalog failed");
    view.load_more();
    until(&view, |s| s.state == DiscoverState::Ready).await;
    assert_eq!(ids(&view), ["tt0", "tt1", "tt2"]);
    view.load_more();
    until(&view, failed).await;
    assert_eq!(
        ids(&view),
        ["tt0", "tt1", "tt2"],
        "a failed page keeps the items"
    );
    view.load_more();
    until(&view, |s| s.state == DiscoverState::Ready).await;
    assert_eq!(ids(&view), ["tt0", "tt1", "tt2", "tt3"]);
    view.load_more();
    until(&view, |s| s.state == DiscoverState::End).await;
    view.load_more();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let asked: Vec<String> = asked
        .lock()
        .unwrap()
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(
        asked,
        [
            "top.json",
            "top.json",
            "skip=3.json",
            "skip=3.json",
            "skip=4.json"
        ],
        "the end asks nothing more"
    );
}
