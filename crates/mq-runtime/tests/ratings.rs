//! IMDb and Rotten Tomatoes scores from MDBList (plans/research/ratings.md),
//! against a local fake: batching, cache ages, rests, the missing-score rule,
//! and the views that show them. Then the Rotten Tomatoes addon's lists,
//! against a fake of that addon: their mark stays inside and never shows.

use mq_library::{AddonRow, OWNER, RatingRow};
use mq_runtime::ratings::{BATCH, Host, Ratings, Scores, Title};
use mq_runtime::views::discover::DiscoverState;
use mq_runtime::{Core, DiscoverView, HomeView, RowState, SearchView, TitleView};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// A refusal: its status, its `Retry-After` as sent, and the source it
/// is limited to ("tomatoes"), else every call.
#[derive(Clone, Copy)]
struct Refusal {
    status: u16,
    retry: Option<&'static str>,
    only: Option<&'static str>,
}

#[derive(Clone)]
struct Fake {
    url: String,
    calls: Arc<Mutex<Vec<Call>>>,
    refuse: Arc<Mutex<Option<Refusal>>>,
    /// While set, every call is answered 200 with this body.
    answer: Arc<Mutex<Option<&'static str>>>,
}

impl Fake {
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn refuse(&self, status: u16, retry: Option<&'static str>, only: Option<&'static str>) {
        *self.refuse.lock().unwrap() = Some(Refusal {
            status,
            retry,
            only,
        });
    }

    fn accept(&self) {
        *self.refuse.lock().unwrap() = None;
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
        answer: Arc::default(),
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
                let refused = served
                    .refuse
                    .lock()
                    .unwrap()
                    .filter(|r| r.only.is_none_or(|only| url.path().ends_with(only)));
                let answer = *served.answer.lock().unwrap();
                let resp = match (refused, answer) {
                    (Some(Refusal { status, retry, .. }), _) => {
                        let retry =
                            retry.map_or(String::new(), |s| format!("Retry-After: {s}\r\n"));
                        format!(
                            "HTTP/1.1 {status} No\r\n{retry}Content-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                        )
                    }
                    (None, Some(answer)) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                        answer.len()
                    ),
                    (None, None) => {
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
            rt: Some(87),
            fresh: false
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
    fake.refuse(503, None, None);
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
                rt: Some(90),
                fresh: false
            }
        )]),
        "the 10 day old row still shows, the 31 day old one does not"
    );
    // A 5xx without Retry-After rests MDBList: the next lookup sends nothing.
    fake.accept();
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
    fake.refuse(429, Some("1"), None);
    let titles = vec![movie("tt1")];
    core.ratings().lookup(&titles).await;
    assert_eq!(fake.calls().len(), 2);
    fake.accept();
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
        metacritic: None,
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
            rotten_tomatoes: None,
            metacritic: None
        }
    );
}

#[tokio::test]
async fn lookups_a_moment_apart_go_in_one_round() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(json!({}), Duration::ZERO).await;
    core.ratings().set_host(fake.host());
    let mut tasks = Vec::new();
    for id in ["tt1", "tt2", "tt3"] {
        let core = core.clone();
        tasks.push(tokio::spawn(async move {
            core.ratings().lookup(&[movie(id)]).await
        }));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for task in tasks {
        task.await.unwrap();
    }
    let mut rounds: Vec<Vec<String>> = fake.calls().into_iter().map(|c| c.ids).collect();
    rounds.sort();
    assert_eq!(
        rounds,
        [vec!["tt1", "tt2", "tt3"], vec!["tt1", "tt2", "tt3"]],
        "one IMDb call and one Tomatometer call for all three"
    );
}

#[tokio::test]
async fn titles_asked_for_during_a_round_go_together_in_the_next() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(json!({}), Duration::from_millis(400)).await;
    core.ratings().set_host(fake.host());
    let first = {
        let core = core.clone();
        tokio::spawn(async move { core.ratings().lookup(&[movie("tt1")]).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
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

#[tokio::test]
async fn a_round_cancelled_half_way_loses_no_title() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({"tt2": {"imdb": 7.2, "tomatoes": 72}}),
        Duration::from_millis(400),
    )
    .await;
    core.ratings().set_host(fake.host());
    // Both queue within one gathering wait; the first takes both, then its view goes away.
    let first = {
        let core = core.clone();
        tokio::spawn(async move { core.ratings().lookup(&[movie("tt1")]).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let second = {
        let core = core.clone();
        tokio::spawn(async move { core.ratings().lookup(&[movie("tt2")]).await })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    first.abort();
    let known = second.await.unwrap().expect("asked");
    assert_eq!(known[&movie("tt2")].rt, Some(72));
    let mut rounds: Vec<Vec<String>> = fake.calls().into_iter().map(|c| c.ids).collect();
    rounds.sort();
    assert_eq!(
        rounds,
        [
            vec!["tt1", "tt2"],
            vec!["tt1", "tt2"],
            vec!["tt2"],
            vec!["tt2"]
        ]
    );
}

#[tokio::test]
async fn an_answer_of_another_shape_stores_nothing_and_rests_mdblist() {
    for body in [
        r#"{"error": "nope"}"#,
        r#"{"ratings": null}"#,
        r#"{"ratings": [{"imdb_id": "tt1", "rating": 87}]}"#,
        r#"[{"id": "tt1", "rating": 87}]"#,
    ] {
        let core = Core::temporary("p").unwrap();
        let fake = mdblist(json!({}), Duration::ZERO).await;
        core.ratings().set_host(fake.host());
        *fake.answer.lock().unwrap() = Some(body);
        core.ratings().lookup(&[movie("tt1")]).await;
        assert!(
            core.library().ratings(&[movie("tt1")]).unwrap().is_empty(),
            "{body}"
        );
        let left = core.ratings().rest_left().expect("resting");
        assert!(left > Duration::from_secs(290), "{body}: {left:?}");
        assert_eq!(core.ratings().lookup(&[movie("tt1")]).await, None, "{body}");
        assert_eq!(fake.calls().len(), 2, "{body}");
    }
    // An empty array is an answer: the title has no score, and nothing rests.
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(json!({}), Duration::ZERO).await;
    core.ratings().set_host(fake.host());
    *fake.answer.lock().unwrap() = Some(r#"{"ratings": []}"#);
    core.ratings().lookup(&[movie("tt1")]).await.expect("asked");
    let stored = core.library().ratings(&[movie("tt1")]).unwrap();
    assert_eq!((stored[0].imdb, stored[0].rt), (None, None));
    assert_eq!(core.ratings().rest_left(), None);
}

#[tokio::test]
async fn the_imdb_answer_stays_when_only_the_tomatometer_call_fails() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({"tt1": {"imdb": 8.8, "tomatoes": 99}, "tt2": {"imdb": 9.1, "tomatoes": 91}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    aged(&core, "tt1", Some(7.0), Some(90), 4 * DAY_MS);
    fake.refuse(503, None, Some("/tomatoes"));
    let titles = vec![movie("tt1"), movie("tt2")];
    let known = core.ratings().lookup(&titles).await.expect("asked");
    // The new IMDb score, the Tomatometer the row had.
    assert_eq!(
        known[&movie("tt1")],
        Scores {
            imdb: Some(8.8),
            rt: Some(90),
            fresh: false
        }
    );
    assert_eq!(
        known[&movie("tt2")],
        Scores {
            imdb: Some(9.1),
            rt: None,
            fresh: false
        }
    );
    // Both are asked again once the rest is over.
    assert!(core.ratings().rest_left().is_some());
    core.ratings().set_host(fake.host());
    fake.accept();
    core.ratings().lookup(&titles).await.expect("asked again");
    assert_eq!(
        core.ratings().known(&titles)[&movie("tt1")],
        Scores {
            imdb: Some(8.8),
            rt: Some(99),
            fresh: false
        }
    );
}

#[tokio::test]
async fn a_partial_answer_keeps_the_failed_sources_own_age() {
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({"tt1": {"imdb": 8.8, "tomatoes": 99}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    aged(&core, "tt1", Some(7.0), Some(90), 25 * DAY_MS);
    fake.refuse(503, None, Some("/tomatoes"));
    core.ratings().lookup(&[movie("tt1")]).await.expect("asked");
    let stored = &core.library().ratings(&[movie("tt1")]).unwrap()[0];
    assert_eq!((stored.imdb, stored.rt), (Some(8.8), Some(90)));
    // Still 25 days old: the kept Tomatometer goes at 30 days, as any other.
    assert!(now_ms() - stored.fetched_ms >= 25 * DAY_MS - 60_000);
}

#[tokio::test]
async fn requests_to_the_dev_recorder_never_carry_the_key() {
    // Every request to a host off this machine goes to a recorder that is not there; the hosts
    // of this file's other tests are on this machine, so the recorder leaves them alone.
    mq_http::set_dev_replay("http://127.0.0.1:9/replay");
    let core = Core::temporary("p").unwrap();
    core.ratings().set_host(Host {
        url: "https://api.mdblist.com".into(),
        key: Some("test-key".into()),
        local: false,
    });
    assert_eq!(core.ratings().lookup(&[movie("tt1")]).await, None);
    assert_eq!(
        core.ratings().rest_left(),
        None,
        "nothing was asked, so nothing failed"
    );
}

#[tokio::test]
async fn refusals_rest_for_as_long_as_they_say() {
    // (status, Retry-After, the rest it gives, at least and at most)
    let day = Duration::from_secs(86_400);
    let cases: [(u16, Option<&'static str>, Duration, Duration); 6] = [
        (401, None, day - Duration::from_secs(60), day),
        (403, Some("10"), day - Duration::from_secs(60), day),
        (
            400,
            None,
            Duration::from_secs(290),
            Duration::from_secs(300),
        ),
        (
            429,
            Some("0"),
            Duration::from_millis(500),
            Duration::from_secs(1),
        ),
        (
            429,
            Some("18446744073709551615"),
            day - Duration::from_secs(60),
            day,
        ),
        (
            503,
            Some("120"),
            Duration::from_secs(110),
            Duration::from_secs(120),
        ),
    ];
    for (status, retry, least, most) in cases {
        let core = Core::temporary("p").unwrap();
        let fake = mdblist(json!({}), Duration::ZERO).await;
        core.ratings().set_host(fake.host());
        fake.refuse(status, retry, None);
        core.ratings().lookup(&[movie("tt1")]).await;
        let left = core.ratings().rest_left().expect("resting");
        assert!(
            left >= least && left <= most,
            "{status} {retry:?}: rests {left:?}"
        );
        assert_eq!(core.ratings().lookup(&[movie("tt2")]).await, None);
        assert_eq!(fake.calls().len(), 2, "{status} {retry:?}");
    }
}

/// Every log line of the test's thread, at every `tracing` level (reqwest's
/// own lines go through `log`, which is not bridged here; they name the host,
/// never the URL).
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn the_key_never_reaches_a_log() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    // With only one dispatcher alive, tracing decides a log site's interest
    // on the thread that first reaches it, and another test's thread has none:
    // a second dispatcher makes every site ask all of them.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let _guard = tracing::subscriber::set_default(subscriber);
    let core = Core::temporary("p").unwrap();
    let fake = mdblist(
        json!({"tt1": {"imdb": 8.8, "tomatoes": 87}}),
        Duration::ZERO,
    )
    .await;
    let key = "SECRET-mdblist-key-7f3a";
    let host = Host {
        key: Some(key.into()),
        ..fake.host()
    };
    core.ratings().set_host(host.clone());
    core.ratings().lookup(&[movie("tt1")]).await.expect("asked");
    fake.refuse(401, None, None);
    core.ratings().lookup(&[movie("tt2")]).await;
    // A host that does not answer at all.
    core.ratings().set_host(Host {
        url: "http://127.0.0.1:9".into(),
        ..host
    });
    core.ratings().lookup(&[movie("tt3")]).await;
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("MDBList refused the key (401)"), "{text}");
    assert!(
        text.contains("MDBList imdb lookup failed")
            || text.contains("MDBList tomatoes lookup failed"),
        "{text}"
    );
    assert!(!text.contains(key), "{text}");
    // One warning a rest, though both calls of the round failed.
    assert_eq!(text.matches("no ratings lookups").count(), 2, "{text}");
    assert!(fake.calls().iter().all(|c| c.key.as_deref() == Some(key)));
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
    json!({"imdb": imdb, "rottenTomatoes": rt, "metacritic": null})
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
    assert_eq!(json["ratingsFrom"], "IMDb and Rotten Tomatoes via MDBList");

    // No score from MDBList: no credit line; the addon's IMDb score still shows.
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[addon().await], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "movie", "tt0111161");
    until("the title page without MDBList", || {
        view.snapshot().header.details.is_some()
    })
    .await;
    let json = serde_json::to_value(view.snapshot().header.details.unwrap()).unwrap();
    assert_eq!(json["ratings"], pair(Some("9.3"), None));
    assert_eq!(json["ratingsFrom"], Value::Null);
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

/// A fake Rotten Tomatoes addon: Shawshank on the movie list, The Godfather
/// only on its Drama page, Breaking Bad on the series list; any other page
/// answers no titles. Pages can be changed, failed (500) or hung. It keeps
/// the paths it served and the most it answered at once.
#[derive(Clone)]
struct RtAddon {
    row: AddonRow,
    served: Arc<Mutex<Vec<String>>>,
    /// The metas a page answers, by its path.
    pages: Arc<Mutex<HashMap<String, Value>>>,
    /// A whole answer a page gives instead, by its path.
    raw: Arc<Mutex<HashMap<String, String>>>,
    fail: Arc<Mutex<HashSet<String>>>,
    hang: Arc<Mutex<HashSet<String>>>,
    busy: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

impl RtAddon {
    fn served(&self) -> Vec<String> {
        self.served.lock().unwrap().clone()
    }

    fn url(&self) -> &str {
        &self.row.transport_url
    }

    fn list(&self, path: &str, titles: &[(&str, &str)]) {
        let metas: Vec<Value> = titles
            .iter()
            .map(|(kind, id)| json!({"id": id, "type": kind, "name": id}))
            .collect();
        self.pages
            .lock()
            .unwrap()
            .insert(path.to_owned(), Value::Array(metas));
    }

    fn answer(&self, path: &str, body: &str) {
        self.raw
            .lock()
            .unwrap()
            .insert(path.to_owned(), body.to_owned());
    }

    fn fail(&self, path: &str, on: bool) {
        let mut fail = self.fail.lock().unwrap();
        if on {
            fail.insert(path.to_owned());
        } else {
            fail.remove(path);
        }
    }
}

const RT_MOVIES: &str = "/catalog/movie/rtfresh_movie.json";
const RT_DRAMA: &str = "/catalog/movie/rtfresh_movie/genre=Drama.json";
const RT_COMEDY: &str = "/catalog/movie/rtfresh_movie/genre=Comedy.json";
const SHAWSHANK: &str = "tt0111161";
const GODFATHER: &str = "tt0068646";
const PART_II: &str = "tt0071562";

async fn rt_addon() -> RtAddon {
    rt_addon_with(&["Drama", "Comedy"], Duration::ZERO).await
}

/// The fake with these genres on both lists, each answer `delay` late. Its
/// manifest also has a search catalog, which needs an extra.
async fn rt_addon_with(genres: &[&str], delay: Duration) -> RtAddon {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let genre = json!([{"name": "genre", "isRequired": false, "options": genres}]);
    let manifest = json!({
        "id": "pw.ers.rottentomatoes", "version": "1.0.9", "name": "Rotten Tomatoes",
        "types": ["movie", "series"], "resources": ["catalog"], "idPrefixes": ["tt"],
        "catalogs": [
            {"type": "movie", "id": "rtfresh_movie", "name": "RT: Certified Fresh", "extra": genre},
            {"type": "series", "id": "rtfresh_series", "name": "RT: Fresh TV Shows", "extra": genre},
            {"type": "movie", "id": "rtsearch", "name": "Search", "extra": [{"name": "search", "isRequired": true}]}
        ]
    });
    let rt = RtAddon {
        row: AddonRow {
            transport_url: format!("http://{addr}/manifest.json"),
            manifest_json: manifest.to_string(),
            position: 1,
            enabled: true,
            allow_private: true,
        },
        served: Arc::default(),
        pages: Arc::default(),
        raw: Arc::default(),
        fail: Arc::default(),
        hang: Arc::default(),
        busy: Arc::default(),
        most: Arc::default(),
    };
    rt.list(RT_MOVIES, &[("movie", SHAWSHANK)]);
    rt.list(RT_DRAMA, &[("movie", GODFATHER)]);
    rt.list(
        "/catalog/series/rtfresh_series.json",
        &[("series", "tt0903747")],
    );
    let fake = rt.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let fake = fake.clone();
            tokio::spawn(async move {
                let (line, _) = read_request(&mut socket).await;
                let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
                fake.served.lock().unwrap().push(path.clone());
                let now = fake.busy.fetch_add(1, Ordering::SeqCst) + 1;
                fake.most.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(delay).await;
                if fake.hang.lock().unwrap().contains(&path) {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
                let metas = fake.pages.lock().unwrap().get(&path).cloned();
                let raw = fake.raw.lock().unwrap().get(&path).cloned();
                let body =
                    raw.unwrap_or_else(|| json!({"metas": metas.unwrap_or(json!([]))}).to_string());
                let status = if fake.fail.lock().unwrap().contains(&path) {
                    "500 No"
                } else {
                    "200 OK"
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                fake.busy.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    rt
}

fn show(id: &str) -> Title {
    ("show".into(), id.into())
}

/// Whether the Rotten Tomatoes addon's lists name the title (the mark kept
/// inside, never shown).
fn listed(known: &HashMap<Title, Scores>, title: &Title) -> bool {
    known.get(title).is_some_and(|s| s.fresh)
}

fn install(core: &Core, rows: &[AddonRow]) {
    core.library()
        .set_addons(core.profile(), rows, &core.hlc())
        .unwrap();
}

/// A lookup starts a read of the lists when they are due; this waits for
/// it to end.
async fn read(core: &Core, titles: &[Title]) {
    core.ratings().lookup(titles).await;
    for _ in 0..500 {
        if !core.ratings().reading_lists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the read of the lists never ended");
}

const DAY: Duration = Duration::from_secs(86_400);

#[tokio::test]
async fn the_addons_lists_mark_their_titles_fresh_and_are_read_once_a_day() {
    let core = Core::temporary("p").unwrap();
    let rt = rt_addon().await;
    install(&core, std::slice::from_ref(&rt.row));
    let titles = vec![
        movie(SHAWSHANK),
        movie(GODFATHER),
        show("tt0903747"),
        movie(PART_II),
        show(SHAWSHANK),
    ];
    // Nothing read yet: nothing to show, and the lookup does not wait.
    assert_eq!(core.ratings().lookup(&titles).await, None);
    read(&core, &titles).await;
    let known = core.ratings().lookup(&titles).await.expect("marked");
    assert!(listed(&known, &movie(SHAWSHANK)));
    assert!(listed(&known, &movie(GODFATHER)), "from the Drama page");
    assert!(listed(&known, &show("tt0903747")));
    assert!(!listed(&known, &movie(PART_II)), "not listed");
    assert!(!listed(&known, &show(SHAWSHANK)), "listed as a movie");
    // Listed or not, Rotten Tomatoes shows a number or nothing.
    assert!(
        known
            .values()
            .all(|s| Ratings::of(Some(s), None).rotten_tomatoes.is_none())
    );
    let mut pages = rt.served();
    pages.sort();
    assert_eq!(
        pages,
        [
            "/catalog/movie/rtfresh_movie.json",
            "/catalog/movie/rtfresh_movie/genre=Comedy.json",
            "/catalog/movie/rtfresh_movie/genre=Drama.json",
            "/catalog/series/rtfresh_series.json",
            "/catalog/series/rtfresh_series/genre=Comedy.json",
            "/catalog/series/rtfresh_series/genre=Drama.json",
        ],
        "each list as is and by each genre, never the catalog that needs an extra"
    );
    // Within the day nothing is read again.
    read(&core, &titles).await;
    core.ratings().age_lists(Duration::from_secs(23 * 3600));
    read(&core, &titles).await;
    assert_eq!(rt.served().len(), 6);
    assert_eq!(
        core.ratings().known(&titles)[&movie(SHAWSHANK)],
        Scores {
            imdb: None,
            rt: None,
            fresh: true
        }
    );
    core.ratings().age_lists(Duration::from_secs(3600));
    read(&core, &titles).await;
    assert_eq!(rt.served().len(), 12, "a day on, read again");
    // A title they do not list is answered too, unmarked.
    let known = core
        .ratings()
        .lookup(&[movie(PART_II)])
        .await
        .expect("read");
    assert_eq!(known[&movie(PART_II)], Scores::default());
}

/// A page answering `{}` (Trakt Tv's empty answer) is an answer with no
/// titles, read again a day on; one answering an error object failed, and
/// the read is due again after the rest.
#[tokio::test]
async fn an_empty_page_is_an_answer_and_an_error_object_a_failure() {
    let core = Core::temporary("p").unwrap();
    let rt = rt_addon().await;
    rt.answer(RT_COMEDY, "{}");
    install(&core, std::slice::from_ref(&rt.row));
    let titles = vec![movie(SHAWSHANK), movie(GODFATHER)];
    read(&core, &titles).await;
    let served = rt.served().len();
    core.ratings().age_lists(Duration::from_secs(300));
    read(&core, &titles).await;
    assert_eq!(
        rt.served().len(),
        served,
        "an answer: no read after the rest"
    );
    rt.answer(RT_COMEDY, r#"{"err": "upstream failed"}"#);
    core.ratings().age_lists(DAY);
    read(&core, &titles).await;
    assert_eq!(rt.served().len(), 2 * served);
    core.ratings().age_lists(Duration::from_secs(300));
    read(&core, &titles).await;
    assert_eq!(
        rt.served().len(),
        3 * served,
        "a failure: read again after the rest"
    );
}

#[tokio::test]
async fn a_failed_or_emptied_page_keeps_only_its_own_titles_for_a_while() {
    let core = Core::temporary("p").unwrap();
    let rt = rt_addon().await;
    install(&core, std::slice::from_ref(&rt.row));
    let titles = vec![movie(SHAWSHANK), movie(GODFATHER), movie(PART_II)];
    let fresh = |id: &str| core.ratings().known(&titles)[&movie(id)].fresh;
    read(&core, &titles).await;
    assert!(fresh(SHAWSHANK) && fresh(GODFATHER));

    // Shawshank leaves the movie list while the Comedy page fails: the
    // failed page keeps only what it listed, so Shawshank loses the mark.
    rt.list(RT_MOVIES, &[("movie", PART_II)]);
    rt.fail(RT_COMEDY, true);
    core.ratings().age_lists(DAY);
    read(&core, &titles).await;
    assert!(!fresh(SHAWSHANK), "de-listed");
    assert!(fresh(PART_II) && fresh(GODFATHER));
    // A page failed: read again after a rest, not a day.
    let served = rt.served().len();
    read(&core, &titles).await;
    assert_eq!(rt.served().len(), served, "resting");
    rt.fail(RT_COMEDY, false);

    // An empty answer after titles counts as a failure: The Godfather keeps
    // its mark, and the read is due again after the rest.
    rt.list(RT_DRAMA, &[]);
    core.ratings().age_lists(Duration::from_secs(300));
    read(&core, &titles).await;
    assert_eq!(rt.served().len(), served + 6);
    assert!(fresh(GODFATHER), "an empty answer is not believed at once");
    core.ratings().age_lists(Duration::from_secs(300));
    read(&core, &titles).await;
    assert_eq!(rt.served().len(), served + 12);
    // Three days on, the empty answer stands.
    core.ratings().age_lists(3 * DAY);
    read(&core, &titles).await;
    assert!(!fresh(GODFATHER), "kept 3 days at most");
}

#[tokio::test]
async fn mdblists_numbers_never_wait_on_the_lists_and_a_lookup_that_goes_away_leaves_the_read() {
    let core = Core::temporary("p").unwrap();
    let rt = rt_addon_with(&["Drama", "Comedy"], Duration::from_millis(400)).await;
    rt.hang.lock().unwrap().insert(RT_COMEDY.to_owned());
    install(&core, std::slice::from_ref(&rt.row));
    let fake = mdblist(
        json!({SHAWSHANK: {"imdb": 9.2, "tomatoes": 89}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    let titles = vec![movie(SHAWSHANK), movie(GODFATHER)];
    // Three lookups at once, gone before any page answers: one read, which
    // goes on.
    let lookups: Vec<_> = (0..3)
        .map(|_| {
            let (core, titles) = (core.clone(), titles.clone());
            tokio::spawn(async move { core.ratings().lookup(&titles).await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(50)).await;
    for lookup in &lookups {
        lookup.abort();
    }
    assert!(core.ratings().reading_lists());
    // MDBList's numbers come while a page hangs.
    let started = std::time::Instant::now();
    let known = core.ratings().lookup(&titles).await.expect("asked");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(known[&movie(SHAWSHANK)].rt, Some(89));
    assert!(
        core.ratings().reading_lists(),
        "the hung page holds only the read"
    );
    // One read: each page asked once, though three lookups came.
    until("every page asked", || rt.served().len() == 6).await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let mut once = rt.served();
    once.sort();
    once.dedup();
    assert_eq!((rt.served().len(), once.len()), (6, 6));
}

#[tokio::test]
async fn a_lookup_gives_no_mark_once_the_addon_went_off_meanwhile() {
    let core = Core::temporary("p").unwrap();
    let rt = rt_addon().await;
    install(&core, std::slice::from_ref(&rt.row));
    let titles = vec![movie(SHAWSHANK)];
    read(&core, &titles).await;
    let fake = mdblist(
        json!({SHAWSHANK: {"imdb": 9.2, "tomatoes": null}}),
        Duration::from_millis(300),
    )
    .await;
    core.ratings().set_host(fake.host());
    let lookup = {
        let (core, titles) = (core.clone(), titles.clone());
        tokio::spawn(async move { core.ratings().lookup(&titles).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    core.set_addon_enabled(rt.url(), false).unwrap();
    let known = lookup.await.unwrap().expect("asked");
    assert_eq!(known[&titles[0]].imdb, Some(9.2));
    assert!(!known[&titles[0]].fresh, "the addon as it is now");
}

#[tokio::test]
async fn four_pages_at_a_time_and_at_most_64_pages_of_500_titles() {
    let core = Core::temporary("p").unwrap();
    let genres = ["Drama", "Comedy", "Action", "Horror", "War"];
    let rt = rt_addon_with(&genres, Duration::from_millis(50)).await;
    install(&core, std::slice::from_ref(&rt.row));
    read(&core, &[movie(SHAWSHANK)]).await;
    assert_eq!(rt.served().len(), 12);
    let most = rt.most.load(Ordering::SeqCst);
    assert!((2..=4).contains(&most), "{most} pages at once");

    // A manifest naming 200 genres is read 64 pages deep, and a page of
    // 600 titles gives 500.
    let core = Core::temporary("p").unwrap();
    let genres: Vec<String> = (0..200).map(|i| format!("G{i}")).collect();
    let genres: Vec<&str> = genres.iter().map(String::as_str).collect();
    let rt = rt_addon_with(&genres, Duration::ZERO).await;
    let many: Vec<String> = (0..600).map(|i| format!("tt9{i:06}")).collect();
    let listed: Vec<(&str, &str)> = many.iter().map(|id| ("movie", id.as_str())).collect();
    rt.list(RT_MOVIES, &listed);
    install(&core, std::slice::from_ref(&rt.row));
    read(&core, &[movie(SHAWSHANK)]).await;
    assert_eq!(rt.served().len(), 64);
    let titles = [movie(&many[499]), movie(&many[500])];
    let known = core.ratings().known(&titles);
    assert!(known[&titles[0]].fresh);
    assert!(!known[&titles[1]].fresh, "past 500 titles a page");
}

#[tokio::test]
async fn mdblists_tomatometer_wins_over_the_mark() {
    let core = Core::temporary("p").unwrap();
    let rt = rt_addon().await;
    install(&core, std::slice::from_ref(&rt.row));
    let fake = mdblist(
        json!({SHAWSHANK: {"imdb": 9.2, "tomatoes": 89}, GODFATHER: {"imdb": 9.1, "tomatoes": null}}),
        Duration::ZERO,
    )
    .await;
    core.ratings().set_host(fake.host());
    let titles = vec![movie(SHAWSHANK), movie(GODFATHER)];
    read(&core, &titles).await;
    let known = core.ratings().known(&titles);
    let row = |title: &Title| Ratings::of(known.get(title), None);
    assert_eq!(
        row(&movie(SHAWSHANK)),
        Ratings {
            imdb: Some("9.2".into()),
            rotten_tomatoes: Some("89%".into()),
            metacritic: None
        }
    );
    assert_eq!(
        row(&movie(GODFATHER)),
        Ratings {
            imdb: Some("9.1".into()),
            rotten_tomatoes: None,
            metacritic: None
        },
        "MDBList had no Tomatometer: nothing, the mark never shows"
    );
}

#[tokio::test]
async fn a_disabled_addon_or_a_kids_profile_reads_no_list_and_shows_no_mark() {
    let owner = Core::temporary(OWNER).unwrap();
    owner.set_pin(OWNER, Some("7351"), None).await.unwrap();
    let rt = rt_addon().await;
    let off = AddonRow {
        enabled: false,
        ..rt.row.clone()
    };
    install(&owner, &[off]);
    let titles = vec![movie(SHAWSHANK)];
    assert_eq!(owner.ratings().lookup(&titles).await, None);
    assert!(!owner.ratings().reading_lists());
    assert!(rt.served().is_empty());

    // A kids profile reads the owner's addons, but never these lists, and
    // never asks MDBList.
    owner.set_addon_enabled(rt.url(), true).unwrap();
    let fake = mdblist(
        json!({SHAWSHANK: {"imdb": 9.2, "tomatoes": 89}}),
        Duration::ZERO,
    )
    .await;
    owner.ratings().set_host(fake.host());
    let kid = owner.for_profile(&owner.create_kids_profile("Kid", "fox").unwrap());
    assert_eq!(kid.ratings().lookup(&titles).await, None);
    assert!(!kid.ratings().reading_lists());
    assert!(rt.served().is_empty());
    assert!(fake.calls().is_empty(), "no MDBList call for a kid");
    // Nor shows the mark once the owner's Home has read them.
    read(&owner, &titles).await;
    assert!(owner.ratings().known(&titles)[&titles[0]].fresh);
    let kids = kid.ratings().known(&titles);
    assert!(!kids.get(&titles[0]).is_some_and(|s| s.fresh));

    // Turned off again: the mark goes.
    owner.set_addon_enabled(rt.url(), false).unwrap();
    assert!(!owner.ratings().known(&titles)[&titles[0]].fresh);
    assert_eq!(owner.ratings().lookup(&titles).await, None);
}

/// The owner, 2026-10-23: Rotten Tomatoes shows a number or nothing. Once
/// the addon's lists are read and name the titles, no tile, hero, title
/// page, Discover or Search item shows a word for them, and the title page
/// credits no list.
#[tokio::test]
async fn the_lists_mark_never_shows_on_any_view() {
    let core = Core::temporary("p").unwrap();
    let rt = rt_addon().await;
    install(&core, &[addon().await, rt.row.clone()]);
    let home = HomeView::open(&core);
    until("Home ready", || {
        home.snapshot()
            .rows
            .first()
            .is_some_and(|r| r.state == RowState::Ready)
    })
    .await;
    read(&core, &[movie(SHAWSHANK), movie(GODFATHER)]).await;
    let known = core.ratings().known(&[movie(SHAWSHANK), movie(GODFATHER)]);
    assert!(listed(&known, &movie(SHAWSHANK)) && listed(&known, &movie(GODFATHER)));
    // Give every open view the time to put on what holds after the read.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let tiles = serde_json::to_value(&home.snapshot().rows[0].tiles).unwrap();
    assert_eq!(tiles[0]["ratings"]["rottenTomatoes"], Value::Null);
    assert_eq!(tiles[1]["ratings"]["rottenTomatoes"], Value::Null);
    assert!(
        home.snapshot()
            .header
            .hero
            .is_none_or(|h| h.tile.ratings.rotten_tomatoes.is_none())
    );
    let title = TitleView::open(&core, "movie", SHAWSHANK);
    until("the title page", || {
        title.snapshot().header.details.is_some()
    })
    .await;
    let json = serde_json::to_value(title.snapshot().header.details.unwrap()).unwrap();
    assert_eq!(json["ratings"]["rottenTomatoes"], Value::Null);
    assert_eq!(json["ratingsFrom"], Value::Null, "no list credited");
    let discover = DiscoverView::open(&core);
    until("Discover", || {
        let s = discover.snapshot();
        s.state != DiscoverState::Loading && !s.items.is_empty()
    })
    .await;
    assert!(
        discover
            .snapshot()
            .items
            .iter()
            .all(|t| t.ratings.rotten_tomatoes.is_none())
    );
    let search = SearchView::open(&core);
    search.set_query("godfather");
    until("Search", || {
        search
            .snapshot()
            .rows
            .first()
            .is_some_and(|r| r.state == RowState::Ready)
    })
    .await;
    assert!(
        search
            .snapshot()
            .rows
            .iter()
            .flat_map(|r| &r.tiles)
            .all(|t| t.ratings.rotten_tomatoes.is_none())
    );
}

#[tokio::test]
async fn the_addon_turned_off_while_its_lists_are_read_never_brings_the_mark_back() {
    let core = Core::temporary("p").unwrap();
    let rt = rt_addon_with(&["Drama", "Comedy"], Duration::from_millis(300)).await;
    install(&core, &[addon().await, rt.row.clone()]);
    let home = HomeView::open(&core);
    until("Home with the read on its way", || {
        core.ratings().reading_lists()
            && home
                .snapshot()
                .rows
                .first()
                .is_some_and(|r| r.state == RowState::Ready)
    })
    .await;
    core.set_addon_enabled(rt.url(), false).unwrap();
    until("the read's end", || !core.ratings().reading_lists()).await;
    // Home has put on what holds after the read: no mark.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let snap = home.snapshot();
    assert!(
        snap.rows
            .iter()
            .flat_map(|r| &r.tiles)
            .all(|t| t.ratings.rotten_tomatoes.is_none())
    );
    let titles = [movie(SHAWSHANK)];
    assert!(core.ratings().known(&titles).is_empty());
    assert_eq!(core.ratings().lookup(&titles).await, None);
}

/// The defaults against the real addons: how many titles of a Home of
/// their catalogs the Rotten Tomatoes addon's lists name, and that no tile
/// shows a word for them. Prints one JSON line for
/// `bench/core/ratings.jsonl`.
///
/// `cargo test -p mq-runtime --test ratings -- --ignored --nocapture`
#[tokio::test]
#[ignore = "network: the default addons"]
async fn a_home_of_the_default_catalogs_shows_no_word_for_the_listed_titles() {
    let core = Core::temporary(OWNER).unwrap();
    let all_on: Vec<mq_runtime::defaults::DefaultChoice> = mq_runtime::defaults::list()
        .iter()
        .map(|d| serde_json::from_value(json!({ "key": d.key, "on": true })).unwrap())
        .collect();
    core.install_defaults(&all_on).unwrap();
    let started = std::time::Instant::now();
    let home = HomeView::open(&core);
    let settled = || {
        let rows = home.snapshot().rows;
        !rows.is_empty()
            && rows
                .iter()
                .all(|r| !matches!(r.state, RowState::Loading | RowState::Slow))
    };
    while !settled() && started.elapsed() < Duration::from_secs(60) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Home's first lookup started the read of the lists.
    while core.ratings().reading_lists() && started.elapsed() < Duration::from_secs(120) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let snap = home.snapshot();
    let tiles: Vec<&mq_runtime::views::home::Tile> =
        snap.rows.iter().flat_map(|r| &r.tiles).collect();
    let titles: Vec<Title> = tiles
        .iter()
        .filter_map(|t| match t.kind.as_str() {
            "movie" => Some(("movie".to_owned(), t.id.clone())),
            "series" => Some(("show".to_owned(), t.id.clone())),
            _ => None,
        })
        .collect();
    let known = core.ratings().known(&titles);
    let listed_titles = titles.iter().filter(|t| listed(&known, t)).count();
    let words = tiles
        .iter()
        .filter(|t| {
            t.ratings
                .rotten_tomatoes
                .as_deref()
                .is_some_and(|rt| !rt.ends_with('%'))
        })
        .count();
    println!(
        "{}",
        json!({
            "date": "2026-10-23", "kind": "live", "milestone": "ratings",
            "what": "a Home of the defaults, all on, no MDBList key",
            "tiles": tiles.len(), "listed_titles": listed_titles, "rt_words_shown": words,
        })
    );
    assert!(listed_titles > 0, "no title listed");
    assert_eq!(words, 0, "Rotten Tomatoes shows a number or nothing");
}

/// A read whose runtime goes away (the app's, at its end, or a panic)
/// clears its flag: the next lookup starts a read again.
#[test]
fn a_read_that_never_ends_lets_the_next_one_start() {
    let server = tokio::runtime::Runtime::new().unwrap();
    let rt = server.block_on(async {
        let rt = rt_addon().await;
        rt.hang.lock().unwrap().insert(RT_MOVIES.to_owned());
        rt
    });
    let core = Core::temporary("p").unwrap();
    install(&core, std::slice::from_ref(&rt.row));
    let titles = [movie(SHAWSHANK)];
    let first = tokio::runtime::Runtime::new().unwrap();
    first.block_on(core.ratings().lookup(&titles));
    assert!(core.ratings().reading_lists());
    drop(first);
    assert!(
        !core.ratings().reading_lists(),
        "the read went with its runtime"
    );
    let asked = rt.served().len();
    let second = tokio::runtime::Runtime::new().unwrap();
    second.block_on(async {
        core.ratings().lookup(&titles).await;
        assert!(core.ratings().reading_lists(), "a new read started");
        for _ in 0..300 {
            if rt.served().len() > asked {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the new read asked nothing");
    });
}

/// The library's tiles carry both scores as every other tile does: what the
/// cache holds at once, MDBList's when its lookup lands.
#[tokio::test]
async fn library_tiles_show_both_scores() {
    let (core, _fake) = setup().await;
    for (id, name) in [
        ("tt0111161", "The Shawshank Redemption"),
        ("tt0068646", "The Godfather"),
    ] {
        let row = mq_library::LibraryItemRow {
            item_id: id.into(),
            kind: "movie".into(),
            name: name.into(),
            poster: None,
            poster_shape: None,
            added_ms: 1,
        };
        core.library()
            .upsert_item(core.profile(), &row, true, &core.hlc())
            .unwrap();
    }
    let view = mq_runtime::LibraryView::open(&core);
    let scores = |id: &str| {
        view.snapshot()
            .items
            .iter()
            .find(|t| t.id == id)
            .map(|t| serde_json::to_value(&t.ratings).unwrap())
    };
    until("the library's tiles are rated", || {
        scores("tt0111161") == Some(pair(Some("9.2"), Some("89%")))
            && scores("tt0068646") == Some(pair(None, Some("97%")))
    })
    .await;
}
