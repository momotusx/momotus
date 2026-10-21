//! IMDb and Rotten Tomatoes scores from MDBList (plans/research/ratings.md),
//! against a local fake: batching, cache ages, rests, the missing-score rule,
//! and the views that show them.

use mq_library::{AddonRow, RatingRow};
use mq_runtime::ratings::{BATCH, Host, Ratings, Scores, Title};
use mq_runtime::views::discover::DiscoverState;
use mq_runtime::{Core, DiscoverView, HomeView, RowState, SearchView, TitleView};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DAY_MS: i64 = 86_400_000;

/// One call MDBList got.
#[derive(Debug, Clone)]
struct Call {
    path: String,
    key: Option<String>,
    provider: String,
    ids: Vec<String>,
}

/// A status and its `Retry-After` seconds.
type Refusal = (u16, Option<u64>);

#[derive(Clone)]
struct Fake {
    url: String,
    calls: Arc<Mutex<Vec<Call>>>,
    /// While set, every call is refused with this status and `Retry-After`.
    refuse: Arc<Mutex<Option<Refusal>>>,
}

impl Fake {
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn host(&self) -> Host {
        Host {
            url: self.url.clone(),
            key: Some("test-key".into()),
            local: true,
        }
    }
}

/// Reads one HTTP request: its first line and its body.
async fn read_request(socket: &mut tokio::net::TcpStream) -> (String, Vec<u8>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = socket.read(&mut chunk).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buf[..end]).to_string();
        let length = head
            .lines()
            .find_map(|l| {
                let (name, value) = l.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        if buf.len() >= end + 4 + length {
            let line = head.lines().next().unwrap_or_default().to_owned();
            return (line, buf[end + 4..end + 4 + length].to_vec());
        }
    }
    (String::new(), Vec::new())
}

/// A fake MDBList. `scores` maps an id to its `{"imdb": .., "tomatoes": ..}`
/// values as sent (null or out of range on purpose); ids not in it get no
/// entry. Each answer waits `delay`.
async fn mdblist(scores: Value, delay: Duration) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fake = Fake {
        url: format!("http://{}", listener.local_addr().unwrap()),
        calls: Arc::default(),
        refuse: Arc::default(),
    };
    let served = fake.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let (served, scores) = (served.clone(), scores.clone());
            tokio::spawn(async move {
                let (line, body) = read_request(&mut socket).await;
                let target = line.split(' ').nth(1).unwrap_or_default();
                let url = url::Url::parse(&format!("http://x{target}")).unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap_or_default();
                let ids: Vec<String> = body["ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect();
                served.calls.lock().unwrap().push(Call {
                    path: url.path().to_owned(),
                    key: url
                        .query_pairs()
                        .find(|(k, _)| k == "apikey")
                        .map(|(_, v)| v.into_owned()),
                    provider: body["provider"].as_str().unwrap_or_default().to_owned(),
                    ids: ids.clone(),
                });
                tokio::time::sleep(delay).await;
                let refused = *served.refuse.lock().unwrap();
                let resp = match refused {
                    Some((status, retry)) => {
                        let retry =
                            retry.map_or(String::new(), |s| format!("Retry-After: {s}\r\n"));
                        format!(
                            "HTTP/1.1 {status} No\r\n{retry}Content-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                        )
                    }
                    None => {
                        let (kind, source) = url
                            .path()
                            .strip_prefix("/rating/")
                            .and_then(|p| p.split_once('/'))
                            .unwrap_or_default();
                        let ratings: Vec<Value> = ids
                            .iter()
                            .filter(|id| scores.get(id.as_str()).is_some())
                            .map(|id| json!({"id": id, "rating": scores[id.as_str()][source]}))
                            .collect();
                        let answer = json!({"provider_id": "imdb", "provider_rating": source,
                            "mediatype": kind, "ratings": ratings})
                        .to_string();
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-RateLimit-Remaining: 999\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                            answer.len()
                        )
                    }
                };
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    fake
}

fn movie(id: &str) -> Title {
    ("movie".into(), id.into())
}

fn now_ms() -> i64 {
    mq_library::hlc::now_ms() as i64
}

/// Stores a row as if MDBList had answered `age_ms` ago.
fn aged(core: &Core, id: &str, imdb: Option<f64>, rt: Option<i64>, age_ms: i64) {
    let row = RatingRow {
        imdb_id: id.into(),
        kind: "movie".into(),
        imdb,
        rt,
        fetched_ms: now_ms() - age_ms,
    };
    let lib = core.library();
    assert!(lib.put_ratings(&[row], 0, lib.removals()).unwrap());
}

fn ids_asked(fake: &Fake) -> Vec<String> {
    let mut ids: Vec<String> = fake.calls().into_iter().flat_map(|c| c.ids).collect();
    ids.sort();
    ids.dedup();
    ids
}

#[tokio::test]
async fn titles_go_by_kind_a_hundred_a_call_with_the_key() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({"tt1000000": {"imdb": 8.8, "tomatoes": 87}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    let mut titles: Vec<Title> = (0..150)
        .map(|i| movie(&format!("tt{}", 1_000_000 + i)))
        .collect();
    titles.extend((0..3).map(|i| ("show".to_owned(), format!("tt{}", 2_000_000 + i))));

    let known = core.ratings().lookup(&titles).await.expect("asked");
    assert_eq!(
        known.len(),
        153,
        "a title MDBList does not score is stored too"
    );
    assert_eq!(
        known[&movie("tt1000000")],
        Scores {
            imdb: Some(8.8),
            rt: Some(87)
        }
    );
    let calls = fake.calls();
    let mut shapes: Vec<(String, usize)> = calls
        .iter()
        .map(|c| (c.path.clone(), c.ids.len()))
        .collect();
    shapes.sort();
    assert_eq!(
        shapes,
        [
            ("/rating/movie/imdb".into(), 50),
            ("/rating/movie/imdb".into(), BATCH),
            ("/rating/movie/tomatoes".into(), 50),
            ("/rating/movie/tomatoes".into(), BATCH),
            ("/rating/show/imdb".into(), 3),
            ("/rating/show/tomatoes".into(), 3),
        ]
    );
    assert!(
        calls
            .iter()
            .all(|c| c.key.as_deref() == Some("test-key") && c.provider == "imdb")
    );

    // Fresh rows: nothing is asked again, and the caller has nothing new.
    assert_eq!(core.ratings().lookup(&titles).await, None);
    assert_eq!(fake.calls().len(), 6);
}

#[tokio::test]
async fn rows_are_asked_again_after_3_days_or_1_day_without_a_tomatometer() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(json!({}), Duration::ZERO).await;
    core.ratings().set_host(fake.host());
    aged(&core, "tt1", Some(8.0), Some(80), 2 * DAY_MS);
    aged(&core, "tt2", Some(8.0), Some(80), 3 * DAY_MS + 60_000);
    aged(&core, "tt3", Some(8.0), None, DAY_MS + 60_000);
    aged(&core, "tt4", Some(8.0), None, DAY_MS / 2);
    let titles: Vec<Title> = ["tt1", "tt2", "tt3", "tt4", "tt5"].map(movie).into();
    core.ratings().lookup(&titles).await.expect("asked");
    assert_eq!(ids_asked(&fake), ["tt2", "tt3", "tt5"]);
}

#[tokio::test]
async fn a_failed_call_keeps_rows_up_to_30_days_and_rests_mdblist() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({"tt1": {"imdb": 9.0, "tomatoes": 99}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    aged(&core, "tt1", Some(8.1), Some(90), 10 * DAY_MS);
    aged(&core, "tt2", Some(7.0), Some(50), 31 * DAY_MS);
    *fake.refuse.lock().unwrap() = Some((503, None));
    let titles = vec![movie("tt1"), movie("tt2")];

    let known = core.ratings().lookup(&titles).await.expect("asked");
    assert_eq!(
        fake.calls().len(),
        2,
        "one IMDb call and one Tomatometer call"
    );
    assert_eq!(
        known,
        HashMap::from([(
            movie("tt1"),
            Scores {
                imdb: Some(8.1),
                rt: Some(90)
            }
        )]),
        "the 10 day old row still shows, the 31 day old one does not"
    );
    // A 5xx without Retry-After rests MDBList: the next lookup sends nothing.
    *fake.refuse.lock().unwrap() = None;
    assert_eq!(core.ratings().lookup(&titles).await, None);
    assert_eq!(fake.calls().len(), 2);
    assert_eq!(core.ratings().known(&titles).len(), 1);
}

#[tokio::test]
async fn retry_after_is_honoured_for_every_lookup() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({"tt1": {"imdb": 7.7, "tomatoes": 77}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    *fake.refuse.lock().unwrap() = Some((429, Some(1)));
    let titles = vec![movie("tt1")];
    core.ratings().lookup(&titles).await;
    assert_eq!(fake.calls().len(), 2);
    *fake.refuse.lock().unwrap() = None;
    // Another title, another view: still nothing until Retry-After has passed.
    assert_eq!(core.ratings().lookup(&[movie("tt9")]).await, None);
    assert_eq!(fake.calls().len(), 2);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let known = core.ratings().lookup(&titles).await.expect("asked again");
    assert_eq!(known[&movie("tt1")].rt, Some(77));
    assert_eq!(fake.calls().len(), 4);
}

#[tokio::test]
async fn a_missing_or_invalid_score_shows_no_slot() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({
            "tt1": {"imdb": 8.8, "tomatoes": 87},
            "tt2": {"imdb": null, "tomatoes": null},
            "tt3": {"imdb": -1, "tomatoes": 101},
            "tt5": {"imdb": 11, "tomatoes": 0}
        }),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    let titles: Vec<Title> = ["tt1", "tt2", "tt3", "tt4", "tt5"].map(movie).into();
    let known = core.ratings().lookup(&titles).await.expect("asked");
    let row = |id: &str, addon: Option<&str>| Ratings::of(known.get(&movie(id)), addon);
    let shown = |imdb: Option<&str>, rt: Option<&str>| Ratings {
        imdb: imdb.map(str::to_owned),
        rotten_tomatoes: rt.map(str::to_owned),
    };
    assert_eq!(row("tt1", Some("8.7")), shown(Some("8.8"), Some("87%")));
    // MDBList has no IMDb score: the addon's; it never gives the RT one.
    assert_eq!(row("tt2", Some("7.5")), shown(Some("7.5"), None));
    assert_eq!(row("tt3", None), shown(None, None));
    assert_eq!(row("tt4", None), shown(None, None));
    assert_eq!(row("tt5", None), shown(None, Some("0%")));
    // The invalid values were stored as missing, so the 1 day rule asks again.
    let stored = core.library().ratings(&[movie("tt3")]).unwrap();
    assert_eq!((stored[0].imdb, stored[0].rt), (None, None));
}

#[tokio::test]
async fn without_a_key_mdblist_is_never_asked() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({"tt1": {"imdb": 8.8, "tomatoes": 87}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(Host {
        key: None,
        ..fake.host()
    });
    assert_eq!(core.ratings().lookup(&[movie("tt1")]).await, None);
    assert!(fake.calls().is_empty());
    // The addon's IMDb score still shows; Rotten Tomatoes does not.
    assert_eq!(
        Ratings::of(None, Some("8.4")),
        Ratings {
            imdb: Some("8.4".into()),
            rotten_tomatoes: None
        }
    );
}

#[tokio::test]
async fn titles_asked_for_during_a_round_go_together_in_the_next() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(json!({}), Duration::from_millis(300)).await;
    core.ratings().set_host(fake.host());
    let first = {
        let core = core.clone();
        tokio::spawn(async move { core.ratings().lookup(&[movie("tt1")]).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let later: Vec<_> = ["tt2", "tt3"]
        .map(|id| {
            let core = core.clone();
            tokio::spawn(async move { core.ratings().lookup(&[movie(id)]).await })
        })
        .into();
    first.await.unwrap();
    for task in later {
        task.await.unwrap();
    }
    let mut rounds: Vec<Vec<String>> = fake.calls().into_iter().map(|c| c.ids).collect();
    rounds.sort();
    assert_eq!(
        rounds,
        [
            vec!["tt1"],
            vec!["tt1"],
            vec!["tt2", "tt3"],
            vec!["tt2", "tt3"]
        ]
    );
}

/// A movie addon with a catalog, a search catalog and metas.
async fn addon() -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let (line, _) = read_request(&mut socket).await;
                let body = if line.contains("/meta/") { META } else { METAS };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = json!({
        "id": "local", "version": "1.0.0", "name": "Local",
        "types": ["movie"], "resources": ["catalog", "meta"], "idPrefixes": ["tt"],
        "catalogs": [
            {"type": "movie", "id": "top", "name": "Popular"},
            {"type": "movie", "id": "find", "name": "Search", "extra": [{"name": "search", "isRequired": true}]}
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
  {"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","background":"https://images.metahub.space/background/medium/tt0111161/img","links":[{"name":"9.3","category":"imdb","url":"https://imdb.com/title/tt0111161"}]},
  {"id":"tt0068646","type":"movie","name":"The Godfather"},
  {"id":"tt0071562","type":"movie","name":"The Godfather Part II","links":[{"name":"9.0","category":"imdb","url":"https://imdb.com/title/tt0071562"}]}
]}"#;

const META: &str = r#"{"meta":{"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","links":[{"name":"9.3","category":"imdb","url":"https://imdb.com/title/tt0111161"}]}}"#;

/// Shawshank has both, The Godfather only a Tomatometer, Part II nothing.
async fn setup() -> (Core, Fake) {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[addon().await], &core.hlc())
        .unwrap();
    let fake = mdblist(
        json!({"tt0111161": {"imdb": 9.2, "tomatoes": 89}, "tt0068646": {"imdb": null, "tomatoes": 97}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    (core, fake)
}

fn pair(imdb: Option<&str>, rt: Option<&str>) -> Value {
    json!({"imdb": imdb, "rottenTomatoes": rt})
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..300 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{what} never happened");
}

#[tokio::test]
async fn home_tiles_and_the_hero_show_both_scores_and_keep_the_addons_rating() {
    let (core, fake) = setup().await;
    let view = HomeView::open(&core);
    until("ratings on Home", || {
        view.snapshot().rows.first().is_some_and(|r| {
            r.state == RowState::Ready && r.tiles[1].ratings.rotten_tomatoes.is_some()
        })
    })
    .await;
    let snap = view.snapshot();
    let tiles = serde_json::to_value(&snap.rows[0].tiles).unwrap();
    assert_eq!(tiles[0]["ratings"], pair(Some("9.2"), Some("89%")));
    assert_eq!(
        tiles[0]["rating"], "9.3",
        "the addon's own rating stays as written"
    );
    assert_eq!(tiles[1]["ratings"], pair(None, Some("97%")));
    assert_eq!(
        tiles[2]["ratings"],
        pair(Some("9.0"), None),
        "the addon's, MDBList had none"
    );
    until("ratings on the hero", || {
        view.snapshot()
            .header
            .hero
            .is_some_and(|h| h.tile.ratings.rotten_tomatoes.is_some())
    })
    .await;
    let hero = view.snapshot().header.hero.unwrap();
    assert_eq!(hero.tile.id, "tt0111161");
    assert_eq!(
        serde_json::to_value(&hero.tile.ratings).unwrap(),
        pair(Some("9.2"), Some("89%"))
    );
    let asked = fake.calls().len();
    drop(view);

    // Opened again: the row comes with the cached scores, and nothing is asked.
    let view = HomeView::open(&core);
    until("Home again", || {
        view.snapshot()
            .rows
            .first()
            .is_some_and(|r| r.state == RowState::Ready)
    })
    .await;
    assert_eq!(
        view.snapshot().rows[0].tiles[0].ratings.imdb.as_deref(),
        Some("9.2")
    );
    assert_eq!(fake.calls().len(), asked);
}

#[tokio::test]
async fn the_title_page_shows_both_scores_and_links_to_imdb() {
    let (core, _fake) = setup().await;
    let view = TitleView::open(&core, "movie", "tt0111161");
    until("ratings on the title page", || {
        view.snapshot()
            .header
            .details
            .is_some_and(|d| d.ratings.rotten_tomatoes.is_some())
    })
    .await;
    let details = view.snapshot().header.details.unwrap();
    let json = serde_json::to_value(&details).unwrap();
    assert_eq!(json["ratings"], pair(Some("9.2"), Some("89%")));
    assert_eq!(json["rating"], "9.3");
    assert_eq!(json["imdbUrl"], "https://www.imdb.com/title/tt0111161/");
}

#[tokio::test]
async fn discover_and_search_show_both_scores() {
    let (core, _fake) = setup().await;
    let discover = DiscoverView::open(&core);
    until("ratings in Discover", || {
        let s = discover.snapshot();
        s.state != DiscoverState::Loading
            && s.items
                .first()
                .is_some_and(|t| t.ratings.rotten_tomatoes.is_some())
    })
    .await;
    let items = discover.snapshot().items;
    assert_eq!(items[0].ratings.imdb.as_deref(), Some("9.2"));
    assert_eq!(items[1].ratings.imdb, None);
    assert_eq!(items[2].ratings.rotten_tomatoes, None);

    let search = SearchView::open(&core);
    search.set_query("godfather");
    until("ratings in Search", || {
        search.snapshot().rows.first().is_some_and(|r| {
            r.state == RowState::Ready
                && r.tiles.iter().any(|t| t.ratings.rotten_tomatoes.is_some())
        })
    })
    .await;
    let row = &search.snapshot().rows[0];
    let godfather = row.tiles.iter().find(|t| t.id == "tt0068646").unwrap();
    assert_eq!(
        serde_json::to_value(&godfather.ratings).unwrap(),
        pair(None, Some("97%"))
    );
}
