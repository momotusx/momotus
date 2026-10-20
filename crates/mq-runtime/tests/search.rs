use mq_library::AddonRow;
use mq_runtime::views::search::{SearchSnapshot, TOP_ROW_ID};
use mq_runtime::{Core, RowState, SearchView, TraktConfig, TraktHosts};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A search addon answering every query with `ids`; counts its requests.
async fn search_addon(id: &str, ids: &'static [&'static str]) -> (AddonRow, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let metas: Vec<_> = ids
                    .iter()
                    .map(|id| serde_json::json!({"id": id, "type": "movie", "name": id}))
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
        "id": id, "version": "1.0.0", "name": id, "types": ["movie"], "resources": ["catalog"],
        "catalogs": [{"type": "movie", "id": "top", "extra": [{"name": "search", "isRequired": true}]}]
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    (row, hits)
}

#[tokio::test]
async fn debounces_dedupes_and_drops_empty_rows() {
    let core = Core::temporary("p").unwrap();
    let (a, hits) = search_addon("a", &["tt1", "tt2"]).await;
    let (b, _) = search_addon("b", &["tt2", "tt3"]).await;
    let (c, _) = search_addon("c", &[]).await;
    let mut rows = vec![a, b, c];
    for (i, r) in rows.iter_mut().enumerate() {
        r.position = i as i64;
    }
    core.library()
        .set_addons(core.profile(), &rows, &core.hlc())
        .unwrap();

    let view = SearchView::open(&core);
    for typed in ["s", "si", "sin", "sintel"] {
        view.set_query(typed);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let mut settled = None;
    for _ in 0..300 {
        let s = view.snapshot();
        if s.query == "sintel"
            && s.rows.iter().all(|r| r.state == RowState::Ready)
            && !s.rows.is_empty()
        {
            settled = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let s = settled.expect("search settled");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "typing is debounced");
    let ids: Vec<Vec<&str>> = s
        .rows
        .iter()
        .map(|r| r.tiles.iter().map(|t| t.id.as_str()).collect())
        .collect();
    assert_eq!(ids, vec![vec!["tt1", "tt2"], vec!["tt3"]]);
    assert_eq!(s.rows[0].title, "Movies");

    view.remember().unwrap();
    view.set_query("  ");
    assert_eq!(view.snapshot().recent, vec!["sintel".to_string()]);

    // A title named exactly what was typed moves to the front of its row.
    view.set_query("TT2");
    let mut first = None;
    for _ in 0..300 {
        let s = view.snapshot();
        if s.query == "TT2"
            && !s.rows.is_empty()
            && s.rows.iter().all(|r| r.state == RowState::Ready)
        {
            first = Some(s.rows[0].tiles[0].id.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(first.as_deref(), Some("tt2"));
}

/// An open search runs again when a search addon is installed, so its row
/// appears without retyping.
#[tokio::test]
async fn an_open_search_picks_up_a_new_addon() {
    let core = Core::temporary("p").unwrap();
    let (a, _) = search_addon("a", &["tt1"]).await;
    let (b, _) = search_addon("b", &["tt2"]).await;
    core.library()
        .set_addons(core.profile(), std::slice::from_ref(&a), &core.hlc())
        .unwrap();
    let view = SearchView::open(&core);
    view.set_query("sintel");
    let rows = |n: usize| {
        let s = view.snapshot();
        s.rows.len() == n && s.rows.iter().all(|r| r.state == RowState::Ready)
    };
    for _ in 0..300 {
        if rows(1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(rows(1), "{:?}", view.snapshot());
    let mut b = b;
    b.position = 1;
    core.library()
        .set_addons(core.profile(), &[a, b], &core.hlc())
        .unwrap();
    for _ in 0..300 {
        if rows(2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(rows(2), "{:?}", view.snapshot());
    assert_eq!(view.snapshot().query, "sintel");
}

/// A search and meta addon: its search answers `ids` (named by `names`), its
/// meta answers any id named there.
async fn meta_addon(
    ids: &'static [&'static str],
    names: &'static [(&'static str, &'static str)],
    search_delay: Duration,
) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let name = |id: &str| {
        names
            .iter()
            .find(|(i, _)| *i == id)
            .map_or(id, |(_, n)| *n)
            .to_owned()
    };
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let body = if let Some(rest) = path.strip_prefix("/meta/") {
                    let id = rest
                        .split('/')
                        .nth(1)
                        .unwrap_or_default()
                        .trim_end_matches(".json");
                    serde_json::json!({"meta": {"id": id, "type": "movie", "name": name(id)}})
                } else {
                    tokio::time::sleep(search_delay).await;
                    let metas: Vec<_> = ids
                        .iter()
                        .map(|id| serde_json::json!({"id": id, "type": "movie", "name": name(id)}))
                        .collect();
                    serde_json::json!({ "metas": metas })
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
        "id": "meta", "version": "1.0.0", "name": "Meta", "types": ["movie"],
        "resources": ["catalog", "meta"], "idPrefixes": ["tt"],
        "catalogs": [{"type": "movie", "id": "top", "extra": [{"name": "search", "isRequired": true}]}]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

/// A Trakt host answering every search with `status` and `body`; returns its
/// base and the queries it was sent.
async fn trakt(status: &'static str, body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let queries = Arc::new(Mutex::new(Vec::new()));
    let sent = queries.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let query = head
                .split_whitespace()
                .nth(1)
                .and_then(|p| p.split("query=").nth(1))
                .map(|q| q.split('&').next().unwrap_or_default().to_owned())
                .unwrap_or_default();
            sent.lock().unwrap().push(query);
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    (base, queries)
}

fn with_trakt(core: &Core, base: &str) {
    core.set_trakt_config(Some(TraktConfig {
        client_id: "client".into(),
        redirect_uri: "https://marquee.example/trakt".into(),
        hosts: TraktHosts {
            api: base.into(),
            auth: base.into(),
            local: true,
        },
    }));
}

const TRAKT_SINTEL: &str = r#"[
  {"type":"movie","movie":{"title":"Sintel","year":2010,"ids":{"imdb":"tt1727587"}}},
  {"type":"movie","movie":{"title":"Interstellar","year":2014,"ids":{"imdb":"tt0816692"}}}
]"#;

/// Waits until nothing is loading for `query` and `done` holds.
async fn settled(
    view: &SearchView,
    query: &str,
    done: impl Fn(&SearchSnapshot) -> bool,
) -> SearchSnapshot {
    for _ in 0..500 {
        let s = view.snapshot();
        if s.query == query
            && !s.rows.is_empty()
            && s.rows.iter().all(|r| r.state == RowState::Ready)
            && done(&s)
        {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("not settled: {:?}", view.snapshot());
}

/// plans/search-quality.md: "Top results" leads with the title Cinemeta's
/// search misses, an addon row that fills after it does not repeat it, and
/// Trakt hears only text that rested 400 ms and holds 2 or more characters.
#[tokio::test]
async fn top_results_lead_with_the_title_the_addons_miss() {
    let core = Core::temporary("p").unwrap();
    let addon = meta_addon(
        &["tt2", "tt1727587"],
        &[("tt1727587", "Sintel"), ("tt2", "Santee")],
        Duration::from_millis(1500),
    )
    .await;
    core.library()
        .set_addons(core.profile(), &[addon], &core.hlc())
        .unwrap();
    let (base, sent) = trakt("200 OK", TRAKT_SINTEL).await;
    with_trakt(&core, &base);
    let view = SearchView::open(&core);
    view.set_query("s");
    tokio::time::sleep(Duration::from_millis(500)).await;
    view.set_query("sin");
    tokio::time::sleep(Duration::from_millis(300)).await;
    view.set_query("sintel");
    let s = settled(&view, "sintel", |s| s.rows[0].row_id == TOP_ROW_ID).await;
    assert_eq!(
        *sent.lock().unwrap(),
        ["sintel"],
        "one character, and text that did not rest, are not sent"
    );
    assert_eq!(s.rows[0].title, "Top results");
    let ids: Vec<Vec<&str>> = s
        .rows
        .iter()
        .map(|r| r.tiles.iter().map(|t| t.id.as_str()).collect())
        .collect();
    assert_eq!(ids, vec![vec!["tt1727587"], vec!["tt2"]]);
    assert_eq!(s.rows[0].tiles[0].name, "Sintel");
}

/// Web journey review r2 P2-4: the layout holds while Top results fills.
/// Its row is there, loading, from the query's first snapshot on, and a
/// title an addon row already shows stays in that row when Top results
/// names it too.
#[tokio::test]
async fn a_title_on_screen_stays_when_top_results_fill() {
    let core = Core::temporary("p").unwrap();
    let addon = meta_addon(
        &["tt2", "tt1727587"],
        &[("tt1727587", "Sintel"), ("tt2", "Santee")],
        Duration::ZERO,
    )
    .await;
    core.library()
        .set_addons(core.profile(), &[addon], &core.hlc())
        .unwrap();
    let (base, _) = trakt("200 OK", TRAKT_SINTEL).await;
    with_trakt(&core, &base);
    let view = SearchView::open(&core);
    view.set_query("sintel");
    let ids = |s: &SearchSnapshot| -> Vec<(String, Vec<String>)> {
        s.rows
            .iter()
            .map(|r| {
                (
                    r.row_id.clone(),
                    r.tiles.iter().map(|t| t.id.clone()).collect(),
                )
            })
            .collect()
    };
    let mut seen = Vec::new();
    for _ in 0..1000 {
        let s = view.snapshot();
        if s.query == "sintel" {
            assert_eq!(s.rows[0].row_id, TOP_ROW_ID, "top first, from the start");
            seen.push(ids(&s));
            if s.rows.iter().all(|r| r.state == RowState::Ready) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let last = seen.last().cloned().unwrap();
    let addon_row = |rows: &[(String, Vec<String>)]| {
        rows.iter()
            .find(|(id, tiles)| id != TOP_ROW_ID && !tiles.is_empty())
            .map(|(_, tiles)| tiles.clone())
    };
    // The addon row filled before Top results did, and never changed.
    let first_filled = seen.iter().find_map(|rows| addon_row(rows)).unwrap();
    assert_eq!(first_filled, ["tt1727587", "tt2"]);
    assert!(
        seen.iter()
            .filter_map(|rows| addon_row(rows))
            .all(|t| t == first_filled)
    );
    assert_eq!(
        last,
        [
            (TOP_ROW_ID.to_string(), vec!["tt1727587".to_string()]),
            (
                "meta:movie:top".into(),
                vec!["tt1727587".into(), "tt2".into()]
            ),
        ]
    );
}

/// Trakt failing (500, 429) changes nothing on screen: no row, no failure.
#[tokio::test]
async fn a_failing_trakt_leaves_search_as_it_was() {
    for status in ["500 Internal Server Error", "429 Too Many Requests"] {
        let core = Core::temporary("p").unwrap();
        let addon = meta_addon(&["tt2"], &[("tt2", "Santee")], Duration::ZERO).await;
        core.library()
            .set_addons(core.profile(), &[addon], &core.hlc())
            .unwrap();
        let (base, sent) = trakt(status, "{}").await;
        with_trakt(&core, &base);
        let view = SearchView::open(&core);
        view.set_query("sintel");
        let s = settled(&view, "sintel", |_| !sent.lock().unwrap().is_empty()).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let s2 = view.snapshot();
        assert_eq!(s2, s, "{status}");
        let rows: Vec<&str> = s.rows.iter().map(|r| r.row_id.as_str()).collect();
        assert_eq!(rows, ["meta:movie:top"], "{status}");
    }
}

/// The 30 queries of plans/search-quality.md, [kind, query, accepted ids].
const QUERIES: [(&str, &str, &[&str]); 30] = [
    ("exact", "Sintel", &["tt1727587"]),
    ("exact", "Up", &["tt1049413"]),
    ("exact", "Dune", &["tt1160419", "tt0087182"]),
    ("exact", "Heat", &["tt0113277"]),
    ("exact", "Her", &["tt1798709"]),
    ("exact", "Big Buck Bunny", &["tt1254207"]),
    ("exact", "Tears of Steel", &["tt2285752"]),
    ("exact", "Severance", &["tt11280740"]),
    ("exact", "Breaking Bad", &["tt0903747"]),
    ("exact", "The Office", &["tt0386676", "tt0290978"]),
    ("prefix", "interst", &["tt0816692"]),
    ("prefix", "breaking b", &["tt0903747"]),
    ("prefix", "game of thr", &["tt0944947"]),
    ("prefix", "oppenh", &["tt15398776"]),
    ("prefix", "the godf", &["tt0068646"]),
    ("prefix", "succes", &["tt7660850"]),
    ("non-English", "Amélie", &["tt0211915"]),
    (
        "non-English",
        "Le fabuleux destin d'Amélie Poulain",
        &["tt0211915"],
    ),
    ("non-English", "La casa de papel", &["tt6468322"]),
    ("non-English", "Gisaengchung", &["tt6751668"]),
    ("non-English", "Kış Uykusu", &["tt2758880"]),
    ("non-English", "Das Boot", &["tt0082096"]),
    ("anime", "Attack on Titan", &["tt2560140"]),
    ("anime", "Shingeki no Kyojin", &["tt2560140"]),
    ("anime", "Spirited Away", &["tt0245429"]),
    ("anime", "Frieren", &["tt22248376"]),
    ("typo", "intersteller", &["tt0816692"]),
    ("typo", "brekaing bad", &["tt0903747"]),
    ("typo", "godfater", &["tt0068646"]),
    ("typo", "sintell", &["tt1727587"]),
];

/// Live: the 30 queries through the core with Cinemeta installed and the
/// build's Trakt app; prints whether the first tile on screen is the wanted
/// title, and how long each search took to settle. Build with
/// MARQUEE_TRAKT_CLIENT_ID and MARQUEE_TRAKT_REDIRECT_URI set.
#[tokio::test]
#[ignore = "network and the build's Trakt app"]
async fn live_first_tile_over_thirty_queries() {
    let core = Core::temporary("p").unwrap();
    core.install_addon("https://v3-cinemeta.strem.io/manifest.json")
        .await
        .unwrap();
    let view = SearchView::open(&core);
    let (mut right, mut top_rows, mut times) = (0, 0, Vec::new());
    for (kind, query, want) in QUERIES {
        let started = std::time::Instant::now();
        view.set_query(query);
        let mut s = view.snapshot();
        for _ in 0..2000 {
            s = view.snapshot();
            let busy = s
                .rows
                .iter()
                .any(|r| matches!(r.state, RowState::Loading | RowState::Slow));
            if s.query == query && !s.rows.is_empty() && !busy {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let ms = started.elapsed().as_millis() as u64;
        let first = s.rows.first().and_then(|r| r.tiles.first());
        let ok = first.is_some_and(|t| want.contains(&t.id.as_str()));
        let top = s.rows.first().is_some_and(|r| r.row_id == TOP_ROW_ID);
        right += usize::from(ok);
        top_rows += usize::from(top);
        times.push(ms);
        eprintln!(
            "{kind:12} {query:38} {ms:6} ms first {} {} (top row {top})",
            if ok { "right" } else { "wrong" },
            first.map_or("none".into(), |t| format!("{} {}", t.name, t.id))
        );
        view.set_query("");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    times.sort_unstable();
    let p = |q: f64| times[((q * times.len() as f64).ceil() as usize).max(1) - 1];
    println!(
        "{}",
        serde_json::json!({
            "first_right": right, "of": QUERIES.len(), "top_row": top_rows,
            "settled_p50_ms": p(0.5), "settled_p90_ms": p(0.9), "settled_max_ms": p(1.0)
        })
    );
    assert!(top_rows > 0, "no top row: is the build's Trakt app set?");
}

/// Waits up to 3 s for `done`.
async fn until(view: &SearchView, done: impl Fn(&SearchSnapshot) -> bool) {
    for _ in 0..300 {
        if done(&view.snapshot()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{:?}", view.snapshot());
}

/// Web journey review P3-10: an empty result says whether any installed
/// addon can search, before and after a query, and follows an install.
#[tokio::test]
async fn the_snapshot_says_whether_any_addon_can_search() {
    let core = Core::temporary("p").unwrap();
    let view = SearchView::open(&core);
    assert_eq!(view.snapshot().searchable, 0);
    view.set_query("sintel");
    until(&view, |s| s.query == "sintel").await;
    assert_eq!(
        (view.snapshot().searchable, view.snapshot().rows.len()),
        (0, 0)
    );
    let (a, _) = search_addon("a", &[]).await;
    core.library()
        .set_addons(core.profile(), &[a], &core.hlc())
        .unwrap();
    until(&view, |s| s.searchable == 1 && s.rows.is_empty()).await;
    view.set_query("");
    assert_eq!(view.snapshot().searchable, 1, "known before typing");
}
