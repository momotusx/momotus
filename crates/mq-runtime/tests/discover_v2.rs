//! Discover v2 (plans/discover-v2.md): kinds, services, sorts made by the
//! core over complete sets, the window, late lists, cancellation, decades,
//! links and the remembered choice, against fake addons.

use mq_library::AddonRow;
use mq_runtime::{Core, DiscoverChoice, DiscoverSnapshot, DiscoverState, DiscoverView};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A route's answer: a delay, an HTTP status and a body.
type Answer = (u64, u16, String);
type Routes = Arc<dyn Fn(&str) -> Answer + Send + Sync>;

/// One server for every fake addon, each under its own prefix; records the
/// paths asked.
async fn serve(routes: Routes) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (routes, log) = (routes.clone(), log.clone());
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                log.lock().unwrap().push(path.clone());
                let (delay, status, body) = routes(&path);
                tokio::time::sleep(Duration::from_millis(delay)).await;
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), asked)
}

fn row(base: &str, prefix: &str, manifest: serde_json::Value, position: i64) -> AddonRow {
    AddonRow {
        transport_url: format!("{base}/{prefix}/manifest.json"),
        manifest_json: manifest.to_string(),
        position,
        enabled: true,
        allow_private: true,
    }
}

fn meta(
    id: &str,
    kind: &str,
    imdb: Option<f32>,
    released: &str,
    genres: &[&str],
) -> serde_json::Value {
    let mut links: Vec<serde_json::Value> = genres
        .iter()
        .map(|g| serde_json::json!({"name": g, "category": "Genres", "url": "stremio:///discover/x"}))
        .collect();
    if let Some(r) = imdb {
        links.push(serde_json::json!({"name": format!("{r:.1}"), "category": "imdb", "url": format!("https://imdb.com/title/{id}")}));
    }
    serde_json::json!({"id": id, "type": kind, "name": format!("T{id}"), "links": links,
        "released": format!("{released}T00:00:00.000Z"), "releaseInfo": &released[..4]})
}

fn metas(items: Vec<serde_json::Value>) -> String {
    serde_json::json!({ "metas": items }).to_string()
}

/// A world of the default catalogs, in small: a popular list, a service
/// addon with Netflix, Hulu and Crunchyroll lists, and a Certified Fresh
/// list. `hulu` answers the Hulu movie list; `slow` delays a path.
struct World {
    rows: Vec<AddonRow>,
    asked: Arc<Mutex<Vec<String>>>,
}

async fn world(hulu: impl Fn() -> Answer + Send + Sync + 'static) -> World {
    world_with(hulu, 0).await
}

/// The world with every service list answering after `slow` ms at least.
async fn world_with(hulu: impl Fn() -> Answer + Send + Sync + 'static, slow: u64) -> World {
    let hulu = Arc::new(hulu);
    let routes: Routes = Arc::new(move |path: &str| {
        let (delay, status, body) = answer(&hulu, path);
        let delay = if path.starts_with("/sc/") {
            delay.max(slow)
        } else {
            delay
        };
        (delay, status, body)
    });
    world_of(routes).await
}

fn answer(hulu: &Arc<impl Fn() -> Answer + Send + Sync + 'static>, path: &str) -> Answer {
    {
        let ok = |body: String| (0, 200, body);
        match path {
            p if p.starts_with("/cm/catalog/movie/top") => ok(metas(
                (0..10)
                    .map(|i| {
                        meta(
                            &format!("tt9{i}"),
                            "movie",
                            Some(6.0),
                            "2001-01-01",
                            &["Drama"],
                        )
                    })
                    .collect(),
            )),
            "/sc/catalog/movie/nfx.json" => ok(metas(vec![
                meta("tt1", "movie", Some(7.0), "1995-05-05", &["Drama"]),
                meta("tt2", "movie", Some(8.5), "2021-01-01", &["Comedy"]),
                meta("tt3", "movie", None, "2015-01-01", &["Drama"]),
            ])),
            "/sc/catalog/movie/hlu.json" => hulu(),
            "/sc/catalog/movie/cru.json" => ok(metas(vec![meta(
                "tt50",
                "movie",
                Some(8.0),
                "2016-01-01",
                &["Animation"],
            )])),
            "/sc/catalog/series/cru.json" => ok(metas(vec![meta(
                "tt51",
                "series",
                Some(9.0),
                "2013-01-01",
                &["Animation"],
            )])),
            "/sc/catalog/series/nfx.json" => ok(metas(vec![meta(
                "tt60",
                "series",
                Some(8.0),
                "2020-01-01",
                &["Drama"],
            )])),
            p if p.starts_with("/rt/catalog/movie/rtfresh_movie") => ok(metas(vec![
                meta("tt1", "movie", None, "1995-05-05", &[]),
                meta("tt70", "movie", None, "2010-01-01", &[]),
            ])),
            p if p.starts_with("/rt/catalog/series/rtfresh_series") => {
                ok(metas(vec![meta("tt71", "series", None, "2010-01-01", &[])]))
            }
            _ => (0, 404, "{}".into()),
        }
    }
}

async fn world_of(routes: Routes) -> World {
    let (base, asked) = serve(routes).await;
    let cinemeta = serde_json::json!({
        "id": "com.linvo.cinemeta", "version": "1.0.0", "name": "Cinemeta", "types": ["movie"], "resources": ["catalog"],
        "catalogs": [{"type": "movie", "id": "top", "name": "Popular",
            "extra": [{"name": "genre", "options": ["Drama", "Comedy"]}, {"name": "skip"}]}]
    });
    let services = serde_json::json!({
        "id": "pw.ers.netflix-catalog", "version": "1.0.0", "name": "Streaming Catalogs", "types": ["movie", "series"],
        "resources": ["catalog"],
        "catalogs": [
            {"type": "movie", "id": "nfx", "name": "Netflix"},
            {"type": "series", "id": "nfx", "name": "Netflix"},
            {"type": "movie", "id": "hlu", "name": "Hulu"},
            {"type": "movie", "id": "cru", "name": "Crunchyroll"},
            {"type": "series", "id": "cru", "name": "Crunchyroll"}
        ]
    });
    let rt = serde_json::json!({
        "id": "pw.ers.rottentomatoes", "version": "1.0.0", "name": "Rotten Tomatoes", "types": ["movie", "series"],
        "resources": ["catalog"],
        "catalogs": [
            {"type": "movie", "id": "rtfresh_movie", "name": "RT: Certified Fresh",
             "extra": [{"name": "genre", "options": ["Drama", "Anime"]}]},
            {"type": "series", "id": "rtfresh_series", "name": "RT: Fresh TV Shows"}
        ]
    });
    World {
        rows: vec![
            row(&base, "cm", cinemeta, 0),
            row(&base, "sc", services, 1),
            row(&base, "rt", rt, 2),
        ],
        asked,
    }
}

fn hulu_list(n: usize) -> impl Fn() -> Answer + Send + Sync + 'static {
    move || {
        (
            0,
            200,
            metas(
                (0..n)
                    .map(|i| {
                        meta(
                            &format!("tt{}", 1000 + i),
                            "movie",
                            Some(5.0 + (i % 40) as f32 / 10.0),
                            "2011-01-01",
                            &["Drama"],
                        )
                    })
                    .collect(),
            ),
        )
    }
}

async fn open(rows: &[AddonRow]) -> (Core, DiscoverView) {
    let core = Core::temporary("p").unwrap();
    core.library().set_addons("p", rows, &core.hlc()).unwrap();
    let view = DiscoverView::open(&core);
    (core, view)
}

async fn until(view: &DiscoverView, done: impl Fn(&DiscoverSnapshot) -> bool) -> DiscoverSnapshot {
    for _ in 0..1200 {
        let s = view.snapshot();
        if done(&s) && s.state != DiscoverState::Loading {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("discover never got there: {:?}", view.snapshot());
}

fn ids(s: &DiscoverSnapshot) -> Vec<&str> {
    s.items.iter().map(|t| t.id.as_str()).collect()
}

fn sort_ids(s: &DiscoverSnapshot) -> Vec<&str> {
    s.sorts.iter().map(|o| o.id.as_str()).collect()
}

fn choose(view: &DiscoverView, f: impl FnOnce(&mut DiscoverChoice)) {
    let mut c = DiscoverChoice::default();
    f(&mut c);
    view.choose(c);
}

#[tokio::test]
async fn kinds_services_and_sorts_come_from_the_installed_catalogs() {
    let w = world(hulu_list(2)).await;
    let (_core, view) = open(&w.rows).await;
    let s = until(&view, |s| !s.items.is_empty()).await;
    let kinds: Vec<&str> = s.kinds.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        kinds,
        ["movie", "series", "anime"],
        "Movies, Series, Anime first"
    );
    assert_eq!(s.sort.as_deref(), Some("popular"));
    let platforms: Vec<&str> = s.platforms.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        platforms,
        ["netflix", "hulu"],
        "Crunchyroll is on Anime only"
    );
    assert_eq!(
        sort_ids(&s),
        ["popular", "imdb", "fresh", "newest", "oldest", "az"]
    );
    assert!(
        s.lists.is_empty(),
        "every catalog backs a sort or is a service"
    );
    // No addon's name reaches a label.
    let labels: Vec<&str> = s.sorts.iter().map(|o| o.label.as_str()).collect();
    assert!(
        labels
            .iter()
            .all(|l| !l.contains("RT") && !l.contains("Cinemeta")),
        "{labels:?}"
    );

    choose(&view, |c| c.kind = Some("anime".into()));
    let s = until(&view, |s| s.kind.as_deref() == Some("anime")).await;
    let platforms: Vec<&str> = s.platforms.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(platforms, ["crunchyroll"]);
    // No Kitsu here: Anime offers its Certified Fresh list (the Anime genre).
    assert_eq!(sort_ids(&s), ["fresh"]);
    assert_eq!(
        s.decades,
        Vec::<String>::new(),
        "Certified Fresh has no dates"
    );
}

#[tokio::test]
async fn imdb_on_all_services_is_the_union_with_its_scope() {
    let w = world(|| (0, 500, "{}".into())).await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| c.sort = Some("imdb".into()));
    let s = until(&view, |s| {
        s.sort.as_deref() == Some("imdb") && !s.items.is_empty()
    })
    .await;
    // Netflix answered, Hulu failed (a set's list is asked once: one 12 s budget).
    assert_eq!(ids(&s), ["tt2", "tt1", "tt3"], "by score, unscored last");
    assert_eq!(s.scope.titles, Some(3));
    assert_eq!(s.scope.failed, 1);
    assert_eq!(s.scope.note.as_deref(), Some("on your services"));
    assert_eq!(s.state, DiscoverState::End);
    // The screen before v2 still reads its fields.
    assert_eq!(s.catalog, None);
    assert!(!s.catalogs.is_empty());
    assert_eq!(s.genres, ["Comedy", "Drama"]);
    let hulu = w
        .asked
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.contains("hlu"))
        .count();
    assert_eq!(hulu, 1, "a set's list is not asked again");
}

#[tokio::test]
async fn another_order_genre_or_decade_asks_nothing() {
    let w = world(hulu_list(0)).await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| c.sort = Some("imdb".into()));
    until(&view, |s| {
        s.sort.as_deref() == Some("imdb") && !s.items.is_empty()
    })
    .await;
    let before = w.asked.lock().unwrap().len();
    choose(&view, |c| c.genre = Some(Some("Drama".into())));
    let s = until(&view, |s| s.genre.as_deref() == Some("Drama")).await;
    assert_eq!(ids(&s), ["tt1", "tt3"]);
    choose(&view, |c| {
        c.sort = Some("oldest".into());
        c.decade = Some(Some("1990s".into()));
    });
    let s = until(&view, |s| s.sort.as_deref() == Some("oldest")).await;
    assert_eq!(ids(&s), ["tt1"]);
    assert_eq!(w.asked.lock().unwrap().len(), before);
}

#[tokio::test]
async fn certified_fresh_on_a_service_keeps_its_titles_and_leaves_when_none() {
    let w = world(hulu_list(3)).await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| {
        c.platform = Some(Some("netflix".into()));
        c.sort = Some("fresh".into());
    });
    let s = until(&view, |s| {
        s.platform.as_deref() == Some("netflix") && s.sort.as_deref() == Some("fresh")
    })
    .await;
    assert_eq!(
        ids(&s),
        ["tt1"],
        "Netflix's titles also on the list, in Netflix's order"
    );
    assert_eq!(s.scope.note, None, "one service is its own scope");
    // Hulu has none: its menu leaves Certified Fresh out once known.
    choose(&view, |c| {
        c.platform = Some(Some("hulu".into()));
        c.sort = Some("trending".into());
    });
    let s = until(&view, |s| {
        s.platform.as_deref() == Some("hulu") && !s.items.is_empty()
    })
    .await;
    assert_eq!(sort_ids(&s), ["trending", "imdb", "newest", "oldest", "az"]);
}

#[tokio::test]
async fn a_complete_set_shows_a_window_at_a_time() {
    let w = world(hulu_list(200)).await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| {
        c.platform = Some(Some("hulu".into()));
        c.sort = Some("imdb".into());
    });
    let s = until(&view, |s| {
        s.platform.as_deref() == Some("hulu") && !s.items.is_empty()
    })
    .await;
    assert_eq!((s.items.len(), s.scope.titles), (80, Some(200)));
    assert_eq!(s.state, DiscoverState::Ready);
    let first = s.items[0].id.clone();
    view.preview(Some(&first));
    assert_eq!(
        view.snapshot().items.len(),
        80,
        "a preview does not grow the list"
    );
    view.load_more();
    let s = until(&view, |s| s.items.len() == 160).await;
    assert_eq!(s.items[0].id, first, "in order");
    view.load_more();
    let s = until(&view, |s| s.items.len() == 200).await;
    assert_eq!(s.state, DiscoverState::End);
}

#[tokio::test]
async fn a_late_list_merges_in_once_after_the_soft_deadline() {
    let w = world(|| {
        (
            4500,
            200,
            metas(vec![meta(
                "tt400",
                "movie",
                Some(9.9),
                "2012-01-01",
                &["Drama"],
            )]),
        )
    })
    .await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| c.sort = Some("imdb".into()));
    let early = until(&view, |s| {
        s.sort.as_deref() == Some("imdb") && !s.items.is_empty()
    })
    .await;
    assert_eq!(early.scope.pending, 1, "Hulu still loading");
    assert_eq!(ids(&early), ["tt2", "tt1", "tt3"]);
    let late = until(&view, |s| s.scope.pending == 0 && s.items.len() == 4).await;
    assert_eq!(ids(&late), ["tt400", "tt2", "tt1", "tt3"]);
    assert_eq!(late.scope.failed, 0);
}

#[tokio::test]
async fn a_newer_choice_drops_an_older_load() {
    let w = world(|| {
        (
            800,
            200,
            metas(vec![meta(
                "tt500",
                "movie",
                Some(9.0),
                "2012-01-01",
                &["Drama"],
            )]),
        )
    })
    .await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| {
        c.platform = Some(Some("hulu".into()));
        c.sort = Some("imdb".into());
    });
    choose(&view, |c| c.platform = Some(Some("netflix".into())));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let s = until(&view, |s| s.platform.as_deref() == Some("netflix")).await;
    assert_eq!(ids(&s), ["tt2", "tt1", "tt3"], "none of Hulu's");
    assert_eq!(s.scope.titles, Some(3));
}

#[tokio::test]
async fn a_link_starts_from_a_clean_slate() {
    let w = world(hulu_list(2)).await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| {
        c.platform = Some(Some("netflix".into()));
        c.sort = Some("oldest".into());
        c.decade = Some(Some("1990s".into()));
    });
    until(&view, |s| s.platform.as_deref() == Some("netflix")).await;
    // A title page's genre chip.
    view.select(Some("movie"), None, Some(Some("drama")));
    let s = until(&view, |s| s.platform.is_none()).await;
    assert_eq!(
        (s.sort.as_deref(), s.genre.as_deref(), s.decade.as_deref()),
        (Some("popular"), Some("Drama"), None)
    );
}

#[tokio::test]
async fn the_choice_is_remembered_and_fitted_on_open() {
    let w = world(hulu_list(2)).await;
    let (core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| c.platform = Some(Some("hulu".into())));
    choose(&view, |c| c.sort = Some("newest".into()));
    tokio::time::sleep(Duration::from_millis(800)).await;
    let d = core.prefs().discover;
    assert_eq!(
        (d.platform.as_deref(), d.sort.as_deref()),
        (Some("hulu"), Some("newest"))
    );
    drop(view);
    let again = DiscoverView::open(&core);
    let s = until(&again, |s| !s.items.is_empty()).await;
    assert_eq!(
        (s.platform.as_deref(), s.sort.as_deref()),
        (Some("hulu"), Some("newest"))
    );
    drop(again);

    // Hulu's list is gone: the remembered service opens All.
    let mut rows = w.rows.clone();
    rows[1].manifest_json = rows[1].manifest_json.replace("\"Hulu\"", "\"Picks\"");
    core.library().set_addons("p", &rows, &core.hlc()).unwrap();
    let after = DiscoverView::open(&core);
    let s = until(&after, |s| s.kind.is_some()).await;
    assert_eq!(s.platform, None);
}

#[tokio::test]
async fn only_the_changed_leaves_are_written_once_it_settles() {
    let w = world(hulu_list(2)).await;
    let (core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    let settings = || core.library().settings(core.profile()).unwrap();
    let before = settings();
    for sort in ["imdb", "newest", "oldest"] {
        choose(&view, |c| c.sort = Some(sort.into()));
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(settings(), before, "nothing before it settles");
    tokio::time::sleep(Duration::from_millis(700)).await;
    let mut written: Vec<String> = settings()
        .into_iter()
        .filter(|r| !before.contains(r))
        .map(|(k, _)| k)
        .collect();
    written.sort();
    // The kind was never written before; the sort settled on "oldest".
    assert_eq!(written, ["discover.kind", "discover.sort"]);
    assert_eq!(core.prefs().discover.sort.as_deref(), Some("oldest"));
}

#[tokio::test]
async fn a_decade_looks_through_a_paged_list() {
    // A paged list whose pages of ten hold one 1990s title each, ten pages.
    let routes: Routes = Arc::new(|path: &str| {
        let skip: usize = path
            .split("skip=")
            .nth(1)
            .and_then(|s| s.trim_end_matches(".json").parse().ok())
            .unwrap_or(0);
        if skip >= 100 || !path.contains("/catalog/") {
            return (0, 200, metas(vec![]));
        }
        let items = (skip..skip + 10)
            .map(|i| {
                meta(
                    &format!("tt{i}"),
                    "movie",
                    None,
                    if i % 10 == 0 {
                        "1994-01-01"
                    } else {
                        "2004-01-01"
                    },
                    &[],
                )
            })
            .collect();
        (0, 200, metas(items))
    });
    let (base, _) = serve(routes).await;
    let manifest = serde_json::json!({
        "id": "list", "version": "1.0.0", "name": "List", "types": ["movie"], "resources": ["catalog"],
        "catalogs": [{"type": "movie", "id": "all", "name": "Everything", "extra": [{"name": "skip"}]}]
    });
    let (_core, view) = open(&[row(&base, "l", manifest, 0)]).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| c.decade = Some(Some("1990s".into())));
    let s = until(&view, |s| s.decade.as_deref() == Some("1990s")).await;
    assert_eq!(
        ids(&s),
        [
            "tt0", "tt10", "tt20", "tt30", "tt40", "tt50", "tt60", "tt70", "tt80", "tt90"
        ]
    );
    assert_eq!(s.scope.looked, Some(100));
    assert_eq!(s.state, DiscoverState::End);
}

/// The rail's logos are TMDB's watch provider logos, asked once (the owner,
/// 2026-10-26); a service TMDB has none for keeps its name only, and with no
/// TMDB token nothing is asked and no logo shows.
#[tokio::test]
async fn the_rail_carries_tmdbs_logos() {
    let w = world(hulu_list(2)).await;
    let routes: Routes = Arc::new(|path: &str| match path {
        "/3/watch/providers/movie" => (
            0,
            200,
            serde_json::json!({"results": [
                {"provider_id": 8, "provider_name": "Netflix", "logo_path": "/netflix.png"},
                {"provider_id": 99, "provider_name": "Other", "logo_path": "/other.png"}
            ]})
            .to_string(),
        ),
        _ => (0, 404, "{}".into()),
    });
    let (tmdb, asked) = serve(routes).await;
    let core = Core::temporary("p").unwrap();
    core.set_tmdb_host(mq_runtime::TmdbHost {
        url: tmdb,
        token: Some("test-token".into()),
        local: true,
    });
    core.library()
        .set_addons("p", &w.rows, &core.hlc())
        .unwrap();
    let view = DiscoverView::open(&core);
    let s = until(&view, |s| s.platforms.iter().any(|p| p.logo.is_some())).await;
    let logo = |id: &str| {
        s.platforms
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.logo.clone())
    };
    assert_eq!(
        logo("netflix").as_deref(),
        Some("https://image.tmdb.org/t/p/w154/netflix.png")
    );
    assert_eq!(logo("hulu"), None, "TMDB gave none: the name only");
    drop(view);
    let again = DiscoverView::open(&core);
    until(&again, |s| s.platforms.iter().any(|p| p.logo.is_some())).await;
    assert_eq!(asked.lock().unwrap().len(), 1, "kept, not asked again");

    // No token: nothing is asked, no logo.
    let plain = Core::temporary("p").unwrap();
    plain
        .library()
        .set_addons("p", &w.rows, &plain.hlc())
        .unwrap();
    let view = DiscoverView::open(&plain);
    let s = until(&view, |s| !s.items.is_empty()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(s.platforms.iter().all(|p| p.logo.is_none()));
}

/// Code review r1 M2: a service's Certified Fresh is judged on every list
/// answered, never on a partial one at the soft deadline.
#[tokio::test]
async fn certified_fresh_is_judged_only_when_every_list_answered() {
    let w = world(|| {
        (
            4500,
            200,
            metas(vec![meta(
                "tt70",
                "movie",
                Some(7.0),
                "2010-01-01",
                &["Drama"],
            )]),
        )
    })
    .await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| {
        c.platform = Some(Some("hulu".into()));
        c.sort = Some("imdb".into());
    });
    // Past the soft deadline, before Hulu answers: nothing is judged yet.
    tokio::time::sleep(Duration::from_millis(4000)).await;
    let early = view.snapshot();
    assert_eq!(early.scope.pending, 1, "{:?}", early.scope);
    assert!(
        sort_ids(&early).contains(&"fresh"),
        "judged on a partial answer: {:?}",
        sort_ids(&early)
    );
    let late = until(&view, |s| {
        s.platform.as_deref() == Some("hulu") && s.scope.pending == 0 && !s.items.is_empty()
    })
    .await;
    assert!(sort_ids(&late).contains(&"fresh"), "{:?}", sort_ids(&late));
}

/// A catalog of "More lists" with a Period extra, beside the world.
fn picks(base: &str) -> AddonRow {
    row(
        base,
        "picks",
        serde_json::json!({
            "id": "picks", "version": "1.0.0", "name": "Picks", "types": ["movie"], "resources": ["catalog"],
            "catalogs": [{"type": "movie", "id": "top", "name": "Seeded",
                "extra": [{"name": "genre", "options": ["This Week", "All Time"]}]}]
        }),
        3,
    )
}

/// Code review r1 M4 and m9: a list is not on a service (choosing one
/// clears the service), and a Period carried to a complete set is dropped
/// and named.
#[tokio::test]
async fn a_list_clears_the_service_and_its_period_stays_with_it() {
    let w = world(hulu_list(2)).await;
    let base = w.rows[0]
        .transport_url
        .trim_end_matches("/cm/manifest.json")
        .to_owned();
    let mut rows = w.rows.clone();
    rows.push(picks(&base));
    let (_core, view) = open(&rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| c.platform = Some(Some("netflix".into())));
    until(&view, |s| s.platform.as_deref() == Some("netflix")).await;
    let list = view.snapshot().lists[0].id.clone();
    choose(&view, |c| {
        c.sort = Some(format!("list:{list}"));
        c.genre = Some(Some("This Week".into()));
    });
    let s = until(&view, |s| {
        s.sort.as_deref().is_some_and(|x| x.starts_with("list:"))
    })
    .await;
    assert_eq!(s.platform, None, "a list is not on a service");
    assert_eq!(s.genre_label, "Period");
    choose(&view, |c| c.sort = Some("imdb".into()));
    let s = until(&view, |s| {
        s.sort.as_deref() == Some("imdb") && !s.items.is_empty()
    })
    .await;
    assert_eq!(s.genre, None, "the period stays with its list");
}

/// An addon whose link carries an RPDB key, with no catalogs.
fn keyed(key: &str) -> AddonRow {
    AddonRow {
        transport_url: format!("https://keyed.example/%7B%22rpdb_key%22%3A%22{key}%22%7D/manifest.json"),
        manifest_json: serde_json::json!({
            "id": "keyed", "version": "1.0.0", "name": "Keyed", "types": ["movie"], "resources": ["catalog"], "catalogs": []
        })
        .to_string(),
        position: 9,
        enabled: true,
        allow_private: false,
    }
}

/// Code review r2 N1: a key taken away reaches the set's titles not shown
/// yet, not only the window on screen: none of them keeps its band.
#[tokio::test]
async fn a_key_taken_away_reaches_the_titles_not_shown_yet() {
    let w = world(hulu_list(120)).await;
    let mut keyed_rows = w.rows.clone();
    keyed_rows.push(keyed("key-a"));
    let (core, view) = open(&keyed_rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    choose(&view, |c| {
        c.platform = Some(Some("hulu".into()));
        c.sort = Some("imdb".into());
    });
    let banded = |t: &mq_runtime::views::home::Tile| {
        t.poster.as_deref().is_some_and(|p| p.contains("/key-a/"))
    };
    let s = until(&view, |s| {
        s.platform.as_deref() == Some("hulu") && s.items.len() == 80
    })
    .await;
    assert!(banded(&s.items[0]), "{:?}", s.items[0].poster);
    core.library()
        .set_addons("p", &w.rows, &core.hlc())
        .unwrap();
    until(&view, |s| s.items.first().is_some_and(|t| !banded(t))).await;
    view.load_more();
    let s = until(&view, |s| s.items.len() > 80).await;
    let stale: Vec<_> = s.items[80..]
        .iter()
        .filter(|t| banded(t))
        .map(|t| &t.id)
        .collect();
    assert!(stale.is_empty(), "{stale:?}");
}

/// Code review r1 m3: at most two loads run. Three loads in a row (two slow
/// services, then Popular) abort the first, so its list is asked again when
/// it is chosen again; the second finishes into the cache.
#[tokio::test]
async fn flicking_through_services_keeps_two_loads() {
    let w = world_with(hulu_list(3), 1500).await;
    let (_core, view) = open(&w.rows).await;
    until(&view, |s| !s.items.is_empty()).await;
    let asked = |list: &str| {
        let path = format!("/sc/catalog/movie/{list}.json");
        w.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|p| **p == path)
            .count()
    };
    let before = (asked("hlu"), asked("nfx"));
    let service = |name: &'static str| {
        move |c: &mut DiscoverChoice| {
            c.platform = Some(Some(name.into()));
            c.sort = Some("imdb".into());
        }
    };
    choose(&view, service("hulu"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    choose(&view, service("netflix"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    // The third: Popular on all services, a paged list that answers at once.
    choose(&view, |c| {
        c.platform = Some(None);
        c.sort = Some("popular".into());
    });
    until(&view, |s| {
        s.platform.is_none() && s.sort.as_deref() == Some("popular") && !s.items.is_empty()
    })
    .await;
    // Netflix's load, the one before the last, finishes into the cache.
    tokio::time::sleep(Duration::from_millis(1800)).await;
    for name in ["netflix", "hulu"] {
        choose(&view, service(name));
        until(&view, |s| {
            s.platform.as_deref() == Some(name) && !s.items.is_empty()
        })
        .await;
    }
    assert_eq!(
        asked("nfx") - before.1,
        1,
        "the one before the last was kept"
    );
    assert_eq!(asked("hlu") - before.0, 2, "the first load was aborted");
}
