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
    slow_search_addon(id, ids, Duration::ZERO).await
}

/// `search_addon` answering after `delay`.
async fn slow_search_addon(
    id: &str,
    ids: &'static [&'static str],
    delay: Duration,
) -> (AddonRow, Arc<AtomicUsize>) {
    let metas: Vec<_> = ids
        .iter()
        .map(|id| serde_json::json!({"id": id, "type": "movie", "name": id}))
        .collect();
    let body = serde_json::json!({ "metas": metas }).to_string();
    answering_search_addon(id, body, delay).await
}

/// A search addon answering every query with `body` after `delay`.
async fn answering_search_addon(
    id: &str,
    body: String,
    delay: Duration,
) -> (AddonRow, Arc<AtomicUsize>) {
    typed_search_addon(id, "movie", body, delay).await
}

/// `answering_search_addon` whose search catalog has the type `kind`.
async fn typed_search_addon(
    id: &str,
    kind: &str,
    body: String,
    delay: Duration,
) -> (AddonRow, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": id, "version": "1.0.0", "name": id, "types": [kind], "resources": ["catalog"],
        "catalogs": [{"type": kind, "id": "top", "extra": [{"name": "search", "isRequired": true}]}]
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
    // Three "Movies" catalogs are one group, in addon order, each title once.
    assert_eq!(ids, vec![vec!["tt1", "tt2", "tt3"]]);
    assert_eq!(s.rows[0].title, "Movies");

    view.remember(None).unwrap();
    view.set_query("  ");
    assert_eq!(view.snapshot().recent, vec!["sintel".to_string()]);

    // Left within the debounce after the last keystrokes: the text given
    // is remembered, not the query the debounce still holds.
    view.set_query("sintel");
    view.set_query("sintel 2010");
    view.remember(Some("  sintel 2010 ")).unwrap();
    view.set_query("  ");
    let recent = view.snapshot().recent;
    assert!(recent.iter().any(|q| q == "sintel 2010"), "{recent:?}");

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

/// An open search runs again when a search addon is installed, so its
/// results appear without retyping (in the one "Movies" group).
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
    let rows = |want: &[&str]| {
        let s = view.snapshot();
        s.rows.len() == 1
            && s.rows[0].state == RowState::Ready
            && s.rows[0]
                .tiles
                .iter()
                .map(|t| t.id.as_str())
                .eq(want.iter().copied())
    };
    for _ in 0..300 {
        if rows(&["tt1"]) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(rows(&["tt1"]), "{:?}", view.snapshot());
    let mut b = b;
    b.position = 1;
    core.library()
        .set_addons(core.profile(), &[a, b], &core.hlc())
        .unwrap();
    for _ in 0..300 {
        if rows(&["tt1", "tt2"]) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(rows(&["tt1", "tt2"]), "{:?}", view.snapshot());
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

/// Two "Movies" groups never show (the owner, 2026-10-22): catalogs of one
/// kind are one group in addon order, each title once. A later catalog that
/// answers first waits for the earlier one, and one that answers later only
/// adds at the end, so nothing on screen moves.
#[tokio::test]
async fn catalogs_of_one_kind_are_one_group_in_addon_order() {
    let core = Core::temporary("p").unwrap();
    let (slow, _) = slow_search_addon("slow", &["tt1", "tt2"], Duration::from_millis(600)).await;
    let (fast, _) = search_addon("fast", &["tt2", "tt3"]).await;
    let (late, _) = slow_search_addon("late", &["tt4"], Duration::from_millis(1200)).await;
    let mut rows = vec![slow, fast, late];
    for (i, r) in rows.iter_mut().enumerate() {
        r.position = i as i64;
    }
    core.library()
        .set_addons(core.profile(), &rows, &core.hlc())
        .unwrap();
    let view = SearchView::open(&core);
    view.set_query("sintel");
    let mut seen: Vec<(RowState, Vec<String>)> = Vec::new();
    for _ in 0..400 {
        let s = view.snapshot();
        if s.query == "sintel" {
            assert!(s.rows.len() <= 1, "one group: {:?}", s.rows);
            if let Some(row) = s.rows.first() {
                let now = (
                    row.state.clone(),
                    row.tiles.iter().map(|t| t.id.clone()).collect(),
                );
                if seen.last() != Some(&now) {
                    seen.push(now);
                }
                if row.tiles.len() == 4 {
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let filled: Vec<&Vec<String>> = seen
        .iter()
        .filter(|(state, _)| *state == RowState::Ready)
        .map(|(_, tiles)| tiles)
        .collect();
    assert_eq!(
        filled,
        [
            &vec!["tt1".to_string(), "tt2".into(), "tt3".into()],
            &vec!["tt1".to_string(), "tt2".into(), "tt3".into(), "tt4".into()]
        ],
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .take_while(|(state, _)| *state != RowState::Ready)
            .all(|(_, tiles)| tiles.is_empty()),
        "nothing shows before the first catalog answers: {seen:?}"
    );
}

/// The search for "sintel" once every addon was asked and nothing is
/// loading or slow.
async fn answered(core: &Core, hits: &[&AtomicUsize]) -> SearchSnapshot {
    let view = SearchView::open(core);
    view.set_query("sintel");
    for _ in 0..300 {
        let s = view.snapshot();
        if s.query == "sintel"
            && hits.iter().all(|h| h.load(Ordering::SeqCst) > 0)
            && s.rows
                .iter()
                .all(|r| !matches!(r.state, RowState::Loading | RowState::Slow))
        {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("not answered: {:?}", view.snapshot());
}

/// An addon answering `{}` (Trakt Tv, 2026-10-22) found nothing: its group
/// is left out, not failed. An error object fails the group, and when every
/// catalog of the group failed, the group shows failed.
#[tokio::test]
async fn an_empty_answer_finds_nothing_and_error_objects_fail_the_group() {
    let core = Core::temporary("p").unwrap();
    let (empty, asked) = answering_search_addon("empty", "{}".into(), Duration::ZERO).await;
    core.library()
        .set_addons(core.profile(), &[empty], &core.hlc())
        .unwrap();
    assert!(
        answered(&core, &[&asked]).await.rows.is_empty(),
        "nothing found"
    );

    let core = Core::temporary("p").unwrap();
    let (a, a_asked) =
        answering_search_addon("a", r#"{"error": "x"}"#.into(), Duration::ZERO).await;
    let (mut b, b_asked) =
        answering_search_addon("b", r#"{"message": "y"}"#.into(), Duration::ZERO).await;
    b.position = 1;
    core.library()
        .set_addons(core.profile(), &[a, b], &core.hlc())
        .unwrap();
    let s = answered(&core, &[&a_asked, &b_asked]).await;
    assert_eq!(s.rows.len(), 1, "{:?}", s.rows);
    assert!(matches!(s.rows[0].state, RowState::Failed { .. }));
}

/// A search catalog of a type Marquee does not know (Trakt Tv's "trakt")
/// never titles a group with it: each result joins the group of its own
/// kind, after the movies of a catalog before it, and a result of no known
/// kind goes under "Other".
#[tokio::test]
async fn a_catalog_of_an_unknown_type_files_its_results_by_their_own_kind() {
    let core = Core::temporary("p").unwrap();
    let (movies, movies_asked) = search_addon("movies", &["tt1"]).await;
    let (mut trakt, trakt_asked) = typed_search_addon(
        "trakt",
        "trakt",
        serde_json::json!({"metas": [
            {"id": "tt2", "type": "movie", "name": "Two"},
            {"id": "tt3", "type": "series", "name": "Three"},
            {"id": "tt4", "type": "podcast", "name": "Four"}
        ]})
        .to_string(),
        Duration::ZERO,
    )
    .await;
    trakt.position = 1;
    core.library()
        .set_addons(core.profile(), &[movies, trakt], &core.hlc())
        .unwrap();
    let s = answered(&core, &[&movies_asked, &trakt_asked]).await;
    let groups: Vec<(&str, &str, Vec<&str>)> = s
        .rows
        .iter()
        .map(|r| {
            (
                r.row_id.as_str(),
                r.title.as_str(),
                r.tiles.iter().map(|t| t.id.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(
        groups,
        [
            ("movies:movie:top", "Movies", vec!["tt1", "tt2"]),
            ("kind:series", "Series", vec!["tt3"]),
            ("kind:other", "Other", vec!["tt4"])
        ]
    );
}

/// With no catalog of a known kind, an unknown-type catalog's asking and
/// failing show in the "Other" group, so a search never reads as finding
/// nothing while it is asked or when it failed.
#[tokio::test]
async fn with_only_unknown_types_the_other_group_carries_loading_and_failure() {
    let core = Core::temporary("p").unwrap();
    let (trakt, asked) = typed_search_addon(
        "trakt",
        "trakt",
        r#"{"metas": [{"id": "tt2", "type": "movie", "name": "Two"}]}"#.into(),
        Duration::from_millis(600),
    )
    .await;
    core.library()
        .set_addons(core.profile(), &[trakt], &core.hlc())
        .unwrap();
    let view = SearchView::open(&core);
    view.set_query("sintel");
    let mut loading = false;
    for _ in 0..300 {
        let s = view.snapshot();
        if s.query == "sintel" && !s.rows.is_empty() {
            loading = s.rows[0].row_id == "kind:other" && s.rows[0].state == RowState::Loading;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(loading, "{:?}", view.snapshot().rows);
    drop(view);
    let s = answered(&core, &[&asked]).await;
    let ids: Vec<(&str, Vec<&str>)> = s
        .rows
        .iter()
        .map(|r| {
            (
                r.row_id.as_str(),
                r.tiles.iter().map(|t| t.id.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(ids, [("kind:movies", vec!["tt2"])]);

    let core = Core::temporary("p").unwrap();
    let (failing, asked) =
        typed_search_addon("trakt", "trakt", r#"{"error": "x"}"#.into(), Duration::ZERO).await;
    core.library()
        .set_addons(core.profile(), &[failing], &core.hlc())
        .unwrap();
    let s = answered(&core, &[&asked]).await;
    assert_eq!(s.rows.len(), 1, "{:?}", s.rows);
    assert_eq!(s.rows[0].row_id, "kind:other");
    assert!(matches!(s.rows[0].state, RowState::Failed { .. }));

    // Beside a catalog of a known kind its failure stays silent, as before.
    let core = Core::temporary("p").unwrap();
    let (movies, movies_asked) = search_addon("movies", &["tt1"]).await;
    let (mut failing, asked) =
        typed_search_addon("trakt", "trakt", r#"{"error": "x"}"#.into(), Duration::ZERO).await;
    failing.position = 1;
    core.library()
        .set_addons(core.profile(), &[movies, failing], &core.hlc())
        .unwrap();
    let s = answered(&core, &[&movies_asked, &asked]).await;
    let rows: Vec<&str> = s.rows.iter().map(|r| r.row_id.as_str()).collect();
    assert_eq!(rows, ["movies:movie:top"]);
}
