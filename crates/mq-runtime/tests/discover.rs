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
            // A name that starts with its addon's ("Cat"), which the list shows beside it.
            {"type": "movie", "id": "new", "name": "Cat: New",
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

/// Core backlog C40: a kind and a genre with no catalog (a title page's
/// genre chip) opens the first catalog of that kind that lists the genre,
/// keeping it; with none, the kind's catalog without it, and the snapshot
/// names the genre it could not show.
#[tokio::test]
async fn a_genre_opens_the_first_catalog_that_lists_it() {
    let core = Core::temporary("p").unwrap();
    let (row, asked) = addon().await;
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    let names = |s: &mq_runtime::DiscoverSnapshot| {
        let id = s.catalog.clone();
        s.catalogs
            .iter()
            .find(|c| Some(&c.id) == id.as_ref())
            .map(|c| c.name.clone())
    };
    // "2025" is only New's (a required genre); "Drama" is Popular's.
    view.select(Some("movie"), None, Some(Some("2025")));
    settle(&view).await;
    let s = view.snapshot();
    assert_eq!(
        (names(&s).as_deref(), s.genre.as_deref()),
        (Some("New"), Some("2025"))
    );
    assert_eq!(s.missing_genre, None);
    assert!(asked.lock().unwrap().last().unwrap().contains("genre=2025"));
    view.select(Some("movie"), None, Some(Some("Drama")));
    settle(&view).await;
    let s = view.snapshot();
    assert_eq!(
        (names(&s).as_deref(), s.genre.as_deref()),
        (Some("Popular"), Some("Drama"))
    );
    // No catalog of the kind lists it: the kind's first catalog, and the
    // genre named, even with another catalog of the kind open.
    let new = view.snapshot().catalogs[1].id.clone();
    view.select(None, Some(&new), None);
    settle(&view).await;
    view.select(Some("movie"), None, Some(Some("Western")));
    settle(&view).await;
    let s = view.snapshot();
    assert_eq!(s.kind.as_deref(), Some("movie"));
    assert_eq!(names(&s).as_deref(), Some("Popular"));
    assert_eq!(s.genre, None);
    assert_eq!(s.missing_genre.as_deref(), Some("Western"));
    // Any other choice clears it.
    view.select(None, None, Some(None));
    settle(&view).await;
    assert_eq!(view.snapshot().missing_genre, None);
    // Matched whatever the case, shown as the catalog spells it.
    view.select(Some("movie"), None, Some(Some("drama")));
    settle(&view).await;
    let s = view.snapshot();
    assert_eq!(
        (names(&s).as_deref(), s.genre.as_deref()),
        (Some("Popular"), Some("Drama"))
    );
    assert_eq!(s.missing_genre, None);
}

/// C40: with no catalog listing the genre, the kind's first catalog that
/// needs no genre opens, so no other genre stands in for the missing one;
/// only when every catalog needs one does its first option show, beside the
/// missing genre's name.
#[tokio::test]
async fn a_missing_genre_never_shows_another_in_its_place() {
    let manifest = serde_json::json!({
        "id": "req", "version": "1.0.0", "name": "Req", "types": ["movie", "series"],
        "resources": ["catalog"],
        "catalogs": [
            {"type": "movie", "id": "years", "name": "By year",
             "extra": [{"name": "genre", "isRequired": true, "options": ["2026", "2025"]}]},
            {"type": "movie", "id": "all", "name": "All",
             "extra": [{"name": "genre", "options": ["Drama"]}]},
            {"type": "series", "id": "only", "name": "Only years",
             "extra": [{"name": "genre", "isRequired": true, "options": ["2026"]}]}
        ]
    });
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[serving(manifest).await], &core.hlc())
        .unwrap();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    let name = |s: &mq_runtime::DiscoverSnapshot| {
        let id = s.catalog.clone();
        s.catalogs
            .iter()
            .find(|c| Some(&c.id) == id.as_ref())
            .map(|c| c.name.clone())
    };
    view.select(Some("movie"), None, Some(Some("Western")));
    settle(&view).await;
    let s = view.snapshot();
    assert_eq!(
        (name(&s).as_deref(), s.genre.as_deref()),
        (Some("All"), None)
    );
    assert_eq!(s.missing_genre.as_deref(), Some("Western"));
    // Every series catalog needs a genre: its first option, and the missing
    // one named beside it.
    view.select(Some("series"), None, Some(Some("Western")));
    settle(&view).await;
    let s = view.snapshot();
    assert_eq!(
        (name(&s).as_deref(), s.genre.as_deref()),
        (Some("Only years"), Some("2026"))
    );
    assert_eq!(s.missing_genre.as_deref(), Some("Western"));
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
    // Titled apart from Cat's, so no catalog of it is a twin.
    other.manifest_json = other
        .manifest_json
        .replace("\"cat\"", "\"other\"")
        .replace("\"Cat\"", "\"Other\"")
        .replace("\"Popular\"", "\"Trending\"");
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

/// A catalog titled as one before it in addon order shows no chip, as its
/// row is off Home: only the addon's name would tell the two apart (the
/// owner, 2026-10-22). Turned on in Customize Home, it gets its chip.
#[tokio::test]
async fn a_twin_catalog_shows_only_once_it_is_turned_on_in_customize_home() {
    let core = Core::temporary("p").unwrap();
    let (cat, _) = addon().await;
    let mut other = cat.clone();
    other.transport_url = other
        .transport_url
        .replace("/manifest.json", "/other/manifest.json");
    other.manifest_json = other
        .manifest_json
        .replace("\"cat\"", "\"other\"")
        .replace("\"Cat\"", "\"Other\"");
    core.library()
        .set_addons("p", &[cat, other], &core.hlc())
        .unwrap();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    let names = |s: &mq_runtime::DiscoverSnapshot| {
        s.catalogs
            .iter()
            .map(|c| (c.id.clone(), c.name.clone()))
            .collect::<Vec<_>>()
    };
    // Other's "Cat: New" is no twin: its name does not start with its own addon's.
    assert_eq!(
        names(&view.snapshot()),
        [
            ("cat|movie|top".to_owned(), "Popular".to_owned()),
            ("cat|movie|new".to_owned(), "New".to_owned()),
            ("other|movie|new".to_owned(), "Cat: New".to_owned()),
        ]
    );

    let twin = core
        .home_catalogs()
        .into_iter()
        .find(|c| c.source == "Other" && c.title == "Popular Movies")
        .unwrap();
    assert!(twin.hidden, "the twin's row is off Home too");
    core.set_home_row_hidden(&twin.row_id, false).unwrap();
    until(&view, |s| s.catalogs.len() == 4).await;
    assert_eq!(
        names(&view.snapshot())[2],
        ("other|movie|top".to_owned(), "Popular".to_owned()),
        "in addon order"
    );
    assert_eq!(view.snapshot().catalog.as_deref(), Some("cat|movie|top"));
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

/// A paging catalog whose first page lists tt0 and tt1 and whose later
/// pages answer `later` as is.
async fn later_pages(later: &'static str) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let body = if head.contains("skip=") {
                later.to_owned()
            } else {
                r#"{"metas": [{"id": "tt0", "type": "movie", "name": "M0"}, {"id": "tt1", "type": "movie", "name": "M1"}]}"#.to_owned()
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    let manifest = serde_json::json!({
        "id": "pages", "version": "1.0.0", "name": "Pages", "types": ["movie"], "resources": ["catalog"],
        "catalogs": [{"type": "movie", "id": "top", "name": "Popular", "extra": [{"name": "skip"}]}]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

/// A later page answering `{}` (Trakt Tv's empty answer) ends the catalog;
/// one answering an error object fails, so the page can be asked again.
#[tokio::test]
async fn an_empty_page_ends_the_catalog_and_an_error_object_fails_it() {
    for (later, end) in [("{}", true), (r#"{"err": "upstream failed"}"#, false)] {
        let core = Core::temporary("p").unwrap();
        core.library()
            .set_addons(core.profile(), &[later_pages(later).await], &core.hlc())
            .unwrap();
        let view = DiscoverView::open(&core);
        until(&view, |s| s.state == DiscoverState::Ready).await;
        view.load_more();
        if end {
            until(&view, |s| s.state == DiscoverState::End).await;
        } else {
            until(&view, |s| matches!(s.state, DiscoverState::Failed { .. })).await;
        }
        assert_eq!(ids(&view), ["tt0", "tt1"], "{later}");
    }
}

/// A catalog of a type Marquee does not know (Trakt Tv's "trakt") is filed
/// under the kind its name says, beside that kind's other catalogs, or
/// under "Other": the raw type is never a kind. It is still asked with its
/// own type.
#[tokio::test]
async fn a_catalog_of_an_unknown_type_is_filed_by_its_name() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            log.lock().unwrap().push(
                head.split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned(),
            );
            let body = r#"{"metas": [{"id": "tt0", "type": "movie", "name": "M0"}]}"#;
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    let manifest = serde_json::json!({
        "id": "lists", "version": "1.0.0", "name": "Lists", "types": ["movie", "trakt"],
        "resources": ["catalog"],
        "catalogs": [
            {"type": "movie", "id": "top", "name": "Top"},
            {"type": "trakt", "id": "trakt_popular_movies", "name": "Popular movies"},
            {"type": "trakt", "id": "picks", "name": "Top picks"}
        ]
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    let core = Core::temporary("p").unwrap();
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    let kinds = view.snapshot().kinds;
    assert_eq!(
        kinds,
        [
            ("movie".to_string(), "Movies".to_string()),
            ("other".into(), "Other".into())
        ]
    );
    let names: Vec<String> = view
        .snapshot()
        .catalogs
        .iter()
        .map(|c| c.name.clone())
        .collect();
    // "Top", not "Popular": two lists both read "Popular Movies" would be twins,
    // and Discover shows only the first of twins.
    assert_eq!(names, ["Top", "Popular movies"]);
    view.select(None, Some("lists|trakt|trakt_popular_movies"), None);
    settle(&view).await;
    assert_eq!(view.snapshot().kind.as_deref(), Some("movie"));
    assert!(
        asked
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.ends_with("/catalog/trakt/trakt_popular_movies.json")),
        "{:?}",
        asked.lock().unwrap()
    );
    view.select(Some("other"), None, None);
    settle(&view).await;
    assert_eq!(
        view.snapshot().catalog.as_deref(),
        Some("lists|trakt|picks")
    );
}

/// An addon serving `manifest` whose every catalog answers one movie.
async fn serving(manifest: serde_json::Value) -> AddonRow {
    serving_metas(
        manifest,
        r#"{"metas": [{"id": "tt0", "type": "movie", "name": "M0"}]}"#,
        Arc::default(),
    )
    .await
}

/// An addon serving `manifest` whose every catalog answers `body`,
/// counting the requests in `asked`.
async fn serving_metas(
    manifest: serde_json::Value,
    body: &'static str,
    asked: Arc<std::sync::atomic::AtomicUsize>,
) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let _ = socket.read(&mut buf).await;
            asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

/// The name that files a catalog is its own, not its addon's: "Anime Lists
/// - Best" from Anime Lists is under Other. Inside a kind, catalogs of a
/// known type come first, so picking the kind with a genre lands on one that
/// offers the genre.
#[tokio::test]
async fn a_name_files_without_its_addon_and_known_types_come_first() {
    let row = serving(serde_json::json!({
        "id": "lists", "version": "1.0.0", "name": "Anime Lists", "types": ["movie", "lists"],
        "resources": ["catalog"],
        "catalogs": [
            {"type": "lists", "id": "popular", "name": "Popular movies"},
            {"type": "movie", "id": "top", "name": "Top",
             "extra": [{"name": "genre", "options": ["Action"]}]},
            {"type": "lists", "id": "best", "name": "Anime Lists - Best"}
        ]
    }))
    .await;
    let core = Core::temporary("p").unwrap();
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    let kinds: Vec<String> = view.snapshot().kinds.into_iter().map(|(k, _)| k).collect();
    assert_eq!(kinds, ["movie", "other"]);
    let names: Vec<String> = view
        .snapshot()
        .catalogs
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names, ["Top", "Popular movies"]);
    assert_eq!(view.snapshot().catalog.as_deref(), Some("lists|movie|top"));
    view.select(Some("other"), None, None);
    settle(&view).await;
    assert_eq!(view.snapshot().catalog.as_deref(), Some("lists|lists|best"));
    // The genre chip of the movie kind.
    view.select(Some("movie"), None, Some(Some("Action")));
    settle(&view).await;
    let s = view.snapshot();
    assert_eq!(
        (s.catalog.as_deref(), s.genre.as_deref()),
        (Some("lists|movie|top"), Some("Action"))
    );
}

/// Core backlog C44a: `preview {id}` gives the item's synopsis, up to three
/// genres and runtime from the catalog's own answer, with no request; an id
/// not listed, a null id or a new list clears it.
#[tokio::test]
async fn a_preview_carries_the_item_s_details_with_no_request() {
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let row = serving_metas(
        serde_json::json!({
            "id": "cat", "version": "1.0.0", "name": "Cat", "types": ["movie"], "resources": ["catalog"],
            "catalogs": [{"type": "movie", "id": "top", "name": "Popular",
                "extra": [{"name": "genre", "options": ["Action", "Drama"]}]}]
        }),
        r#"{"metas": [
            {"id": "tt1", "type": "movie", "name": "Sintel", "description": "A girl looks for her dragon.",
             "runtime": "15 min", "links": [
                {"name": "Animation", "category": "Genres", "url": "stremio:///discover"},
                {"name": "Fantasy", "category": "Genres", "url": "stremio:///discover"},
                {"name": "Short", "category": "Genres", "url": "stremio:///discover"},
                {"name": "Drama", "category": "Genres", "url": "stremio:///discover"},
                {"name": "7.4", "category": "imdb", "url": "https://imdb.com/title/tt1727587"}]},
            {"id": "tt2", "type": "movie", "name": "Bare"}
        ]}"#,
        asked.clone(),
    )
    .await;
    let core = Core::temporary("p").unwrap();
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    assert_eq!(view.snapshot().preview, None);
    let before = asked.load(std::sync::atomic::Ordering::SeqCst);

    view.preview(Some("tt1"));
    let preview = view.snapshot().preview.expect("details");
    assert_eq!(
        (
            preview.id.as_str(),
            preview.description.as_deref(),
            preview.runtime.as_deref()
        ),
        ("tt1", Some("A girl looks for her dragon."), Some("15 min"))
    );
    assert_eq!(
        preview.genres,
        ["Animation", "Fantasy", "Short"],
        "three at most"
    );
    view.preview(Some("tt2"));
    let bare = view.snapshot().preview.expect("an item with no details");
    assert_eq!(
        (bare.description, bare.genres.len(), bare.runtime),
        (None, 0, None)
    );
    assert_eq!(
        asked.load(std::sync::atomic::Ordering::SeqCst),
        before,
        "no request for a preview"
    );

    view.preview(Some("tt9"));
    assert_eq!(view.snapshot().preview, None, "not listed yet");
    view.preview(Some("tt1"));
    view.preview(None);
    assert_eq!(view.snapshot().preview, None, "cleared");
    view.preview(Some("tt1"));
    view.select(None, None, Some(Some("Drama")));
    assert_eq!(view.snapshot().preview, None, "a new list starts with none");
}

/// Review note 1: a preview named before the items come is kept and filled
/// in when its page lands; only a select or a `None` clears it.
#[tokio::test]
async fn a_preview_named_before_the_page_is_filled_when_it_comes() {
    let row = serving_metas(
        serde_json::json!({
            "id": "cat", "version": "1.0.0", "name": "Cat", "types": ["movie"], "resources": ["catalog"],
            "catalogs": [{"type": "movie", "id": "top", "name": "Popular"}]
        }),
        r#"{"metas": [{"id": "tt1", "type": "movie", "name": "Sintel", "description": "A girl looks for her dragon."}]}"#,
        Arc::default(),
    )
    .await;
    let core = Core::temporary("p").unwrap();
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let view = DiscoverView::open(&core);
    view.preview(Some("tt1"));
    settle(&view).await;
    let filled = view.snapshot().preview.expect("filled when the page came");
    assert_eq!(
        filled.description.as_deref(),
        Some("A girl looks for her dragon.")
    );
}

/// Core backlog C43: the genre grid lists each kind's genres in catalog
/// order, each once whatever its case and no year, with the backdrop of the first item
/// of the first catalog listing it. The art is fetched once: a grid opened
/// again shows it at once and asks the addon nothing.
#[tokio::test]
async fn the_genre_grid_has_each_genre_once_with_its_first_catalog_s_art() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
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
                log.lock().unwrap().push(path.clone());
                let art = |name: &str| {
                    serde_json::json!({"metas": [{"id": "tt1", "type": "movie", "name": "First",
                        "background": format!("https://img.test/{name}.jpg")}]})
                };
                let body = if path.contains("/movie/top/genre=Action") {
                    art("top-action")
                } else if path.contains("/movie/top/genre=Drama") {
                    art("top-drama")
                } else if path.contains("/movie/new/genre=Comedy") {
                    serde_json::json!({"metas": [{"id": "tt2", "type": "movie", "name": "Bare"}]})
                } else if path.contains("/series/feed/genre=Drama") {
                    art("feed-drama")
                } else {
                    serde_json::json!({"metas": []})
                }
                .to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "cat", "version": "1.0.0", "name": "Cat", "types": ["movie", "series"], "resources": ["catalog"],
        "catalogs": [
            {"type": "movie", "id": "top", "name": "Popular",
             "extra": [{"name": "genre", "options": ["Action", "Drama"]}]},
            {"type": "movie", "id": "new", "name": "New",
             "extra": [{"name": "genre", "options": ["drama", "Comedy", "2026", "1980s"]}]},
            {"type": "series", "id": "feed", "name": "Series",
             "extra": [{"name": "genre", "options": ["Drama"]}]}
        ]
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    let core = Core::temporary("p").unwrap();
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let art_count = |s: &mq_runtime::GenresSnapshot| {
        s.kinds
            .iter()
            .flat_map(|k| &k.genres)
            .filter(|g| g.art.is_some())
            .count()
    };
    let grid = mq_runtime::GenresView::open(&core);
    for _ in 0..300 {
        if art_count(&grid.snapshot()) == 3 && asked.lock().unwrap().len() == 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let s = grid.snapshot();
    type Grid<'a> = Vec<(&'a str, Vec<(&'a str, Option<&'a str>)>)>;
    let read: Grid = s
        .kinds
        .iter()
        .map(|k| {
            (
                k.kind.as_str(),
                k.genres
                    .iter()
                    .map(|g| (g.genre.as_str(), g.art.as_deref()))
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (
                "movie",
                vec![
                    ("Action", Some("https://img.test/top-action.jpg")),
                    ("Drama", Some("https://img.test/top-drama.jpg")),
                    ("Comedy", None),
                ]
            ),
            (
                "series",
                vec![("Drama", Some("https://img.test/feed-drama.jpg"))]
            ),
        ]
    );
    assert_eq!(s.kinds[0].label, "Movies");
    let asked_once = asked.lock().unwrap().len();
    assert_eq!(
        asked_once,
        4,
        "one lookup per genre: {:?}",
        asked.lock().unwrap()
    );

    drop(grid);
    let again = mq_runtime::GenresView::open(&core);
    assert_eq!(art_count(&again.snapshot()), 3, "the art shows at once");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        asked.lock().unwrap().len(),
        asked_once,
        "no request on the second open"
    );
}

/// Core backlog C44b: the preview reads and sets the Library from what the
/// catalog gave, with no title view: the addon is asked for nothing more
/// (no meta, no streams, so no swarm warm-up), the row keeps the catalog's
/// name, kind and poster, and the preview follows a change made elsewhere.
#[tokio::test]
async fn the_preview_sets_the_library_with_no_title_view() {
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let row = serving_metas(
        serde_json::json!({
            "id": "cat", "version": "1.0.0", "name": "Cat", "types": ["movie"], "resources": ["catalog", "meta", "stream"],
            "catalogs": [{"type": "movie", "id": "top", "name": "Popular"}]
        }),
        r#"{"metas": [{"id": "tt1", "type": "movie", "name": "Sintel", "poster": "https://img.test/sintel.jpg"}]}"#,
        asked.clone(),
    )
    .await;
    let core = Core::temporary("p").unwrap();
    core.library().set_addons("p", &[row], &core.hlc()).unwrap();
    let view = DiscoverView::open(&core);
    settle(&view).await;
    let before = asked.load(std::sync::atomic::Ordering::SeqCst);
    view.preview(Some("tt1"));
    assert!(!view.snapshot().preview.unwrap().in_library);

    view.set_in_library("tt1", true).unwrap();
    until(&view, |s| s.preview.as_ref().is_some_and(|p| p.in_library)).await;
    let kept = core.library().library("p").unwrap();
    let item = &kept[0].item;
    assert_eq!(
        (
            item.item_id.as_str(),
            item.kind.as_str(),
            item.name.as_str(),
            item.poster.as_deref()
        ),
        (
            "tt1",
            "movie",
            "Sintel",
            Some("https://img.test/sintel.jpg")
        )
    );
    assert_eq!(
        asked.load(std::sync::atomic::Ordering::SeqCst),
        before,
        "no meta or stream request: no title view, no warm-up"
    );

    // Taken out elsewhere (the title page, another device): the preview follows.
    core.library()
        .upsert_item("p", item, false, core.clock())
        .unwrap();
    until(&view, |s| s.preview.as_ref().is_some_and(|p| !p.in_library)).await;
    view.set_in_library("tt9", true).unwrap();
    assert!(
        !core.library().is_in_library("p", "tt9").unwrap(),
        "not listed: nothing"
    );
}
