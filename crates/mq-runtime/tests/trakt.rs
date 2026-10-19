//! Trakt in the runtime (plans/m7-trakt.md, P3 to P5), against a local
//! fake Trakt that also serves the meta addon the episodes are mapped with.

use mq_library::{IntentOp, NewIntent, TrackerKey};
use mq_runtime::{
    Core, PlayTarget, PlaybackView, TitleView, TraktConfig, TraktHosts, TraktLoginSnapshot,
    TraktLoginView, TraktPlayback, TraktRecord, TraktState, TraktView,
};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SHOW: &str = "tt100";
const MOVIE: &str = "tt200";

/// One request as the fake saw it.
#[derive(Debug, Clone)]
struct Req {
    method: String,
    path: String,
    query: String,
    body: Value,
    at: std::time::Instant,
    /// The bearer token it carried.
    token: String,
}

#[derive(Debug, Clone)]
struct Reply {
    status: u16,
    body: String,
    delay: Duration,
    /// Extra header lines, each ending in CRLF.
    headers: String,
}

fn reply(status: u16, body: Value) -> Reply {
    Reply {
        status,
        body: body.to_string(),
        delay: Duration::ZERO,
        headers: String::new(),
    }
}

#[derive(Clone)]
struct Fake {
    account: String,
    /// Replies to send first, by "METHOD path".
    queued: Arc<Mutex<HashMap<String, VecDeque<Reply>>>>,
    /// Paged lists, by path.
    lists: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    /// Trakt shrinks pages to this (0: the asked limit).
    page: Arc<AtomicUsize>,
    activities: Arc<Mutex<Value>>,
    /// The show each TVDB episode id resolves to.
    shows: Arc<Mutex<HashMap<u64, String>>>,
    /// Overrides the action a stop answers.
    stop_action: Arc<Mutex<Option<String>>>,
    /// The meta addon answers 503.
    meta_down: Arc<std::sync::atomic::AtomicBool>,
    seen: Arc<Mutex<Vec<Req>>>,
}

impl Fake {
    fn new(account: &str) -> Self {
        let shows = (1001..=1006).map(|t| (t, SHOW.to_owned())).collect();
        Self {
            account: account.to_owned(),
            queued: Arc::default(),
            lists: Arc::default(),
            page: Arc::default(),
            activities: Arc::new(Mutex::new(activities("2026-01-01T00:00:00.000Z"))),
            shows: Arc::new(Mutex::new(shows)),
            stop_action: Arc::default(),
            meta_down: Arc::default(),
            seen: Arc::default(),
        }
    }

    fn queue(&self, route: &str, replies: Vec<Reply>) {
        self.queued
            .lock()
            .unwrap()
            .entry(route.into())
            .or_default()
            .extend(replies);
    }

    fn list(&self, path: &str, items: Vec<Value>) {
        self.lists.lock().unwrap().insert(path.into(), items);
    }

    fn seen(&self, method: &str, path: &str) -> Vec<Req> {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .filter(|r| r.method == method && r.path == path)
            .cloned()
            .collect()
    }

    /// The scrobble actions sent, in order, with their progress.
    fn scrobbles(&self) -> Vec<(String, f64)> {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .filter_map(|r| {
                let action = r.path.strip_prefix("/scrobble/")?;
                Some((
                    action.to_owned(),
                    r.body["progress"].as_f64().unwrap_or(-1.0),
                ))
            })
            .collect()
    }

    async fn wait_for(&self, what: &str, check: impl Fn(&Fake) -> bool) {
        self.wait_within(what, 10, check).await;
    }

    /// For what rides on retries and backoff: a deadline far past it, so a
    /// slow machine only waits longer.
    async fn wait_within(&self, what: &str, secs: u64, check: impl Fn(&Fake) -> bool) {
        for _ in 0..secs * 40 {
            if check(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("waited {secs} s for {what}: {:?}", self.scrobbles());
    }

    fn answer(&self, req: &Req) -> Reply {
        let route = format!("{} {}", req.method, req.path);
        // A device code's own polls first ("route#code").
        let keyed = req.body["code"]
            .as_str()
            .map(|code| format!("{route}#{code}"));
        let mut queued = self.queued.lock().unwrap();
        for route in keyed.iter().chain([&route]) {
            if let Some(r) = queued.get_mut(route).and_then(VecDeque::pop_front) {
                return r;
            }
        }
        drop(queued);
        if req.path.starts_with("/meta/") && self.meta_down.load(Ordering::SeqCst) {
            return reply(503, json!({}));
        }
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/manifest.json") => reply(
                200,
                json!({
                    "id": "meta", "version": "1.0.0", "name": "Meta",
                    "types": ["series", "movie"], "resources": ["meta"], "catalogs": [],
                    "idPrefixes": ["tt"]
                }),
            ),
            ("GET", path) if path.starts_with("/meta/") => reply(200, meta(path)),
            ("GET", "/sync/last_activities") => reply(200, self.activities.lock().unwrap().clone()),
            ("GET", "/users/settings") => reply(
                200,
                json!({"user": {"username": "tester", "ids": {"slug": "tester", "uuid": self.account}}}),
            ),
            ("POST", path) if path.starts_with("/scrobble/") => {
                let progress = req.body["progress"].as_f64().unwrap_or(0.0);
                let action = match &path[10..] {
                    "stop" => self.stop_action.lock().unwrap().clone().unwrap_or_else(|| {
                        if progress >= 80.0 {
                            "scrobble"
                        } else {
                            "pause"
                        }
                        .to_owned()
                    }),
                    other => other.to_owned(),
                };
                let mut body = json!({"id": 1, "action": action, "progress": progress});
                if let Some(imdb) = req.body["movie"]["ids"]["imdb"].as_str() {
                    body["movie"] = json!({"title": "Film", "ids": {"imdb": imdb}});
                }
                if let Some(tvdb) = req.body["episode"]["ids"]["tvdb"].as_u64() {
                    let show = self.shows.lock().unwrap().get(&tvdb).cloned();
                    body["episode"] = json!({"season": 1, "number": 1, "ids": {"tvdb": tvdb}});
                    body["show"] = json!({"title": "Show", "ids": {"imdb": show}});
                }
                reply(201, body)
            }
            // As Trakt counts: every item sent is added (or deleted once).
            ("POST", path @ ("/sync/history" | "/sync/history/remove")) => {
                let count = |list: &str| req.body[list].as_array().map_or(0, Vec::len);
                let counts = json!({"movies": count("movies"), "episodes": count("episodes")});
                let field = if path.ends_with("remove") {
                    "deleted"
                } else {
                    "added"
                };
                let mut body = json!({"not_found": {"movies": [], "episodes": []}});
                body[field] = counts;
                reply(if field == "added" { 201 } else { 200 }, body)
            }
            ("POST", "/oauth/revoke") => reply(200, json!({})),
            ("GET", path) => {
                let items = self
                    .lists
                    .lock()
                    .unwrap()
                    .get(path)
                    .cloned()
                    .unwrap_or_default();
                let q = |k: &str| {
                    req.query
                        .split('&')
                        .find_map(|p| p.strip_prefix(&format!("{k}=")))
                        .and_then(|v| v.parse::<usize>().ok())
                };
                let mut limit = q("limit").unwrap_or(100);
                let cap = self.page.load(Ordering::SeqCst);
                if cap > 0 {
                    limit = limit.min(cap);
                }
                let page = q("page").unwrap_or(1);
                let start = (page - 1) * limit;
                let slice: Vec<Value> = items.into_iter().skip(start).take(limit).collect();
                reply(200, json!(slice))
            }
            _ => reply(404, json!({})),
        }
    }

    async fn start(&self) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let fake = self.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let fake = fake.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    let (head, body) = loop {
                        let n = socket.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let head = text[..end].to_owned();
                            let len = head
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse::<usize>().ok())?
                                })
                                .unwrap_or(0);
                            if buf.len() >= end + 4 + len {
                                break (head, text[end + 4..end + 4 + len].to_owned());
                            }
                        }
                    };
                    let mut words = head.split_whitespace();
                    let method = words.next().unwrap_or_default().to_owned();
                    let target = words.next().unwrap_or_default();
                    let (path, query) = target.split_once('?').unwrap_or((target, ""));
                    let req = Req {
                        method,
                        path: path.to_owned(),
                        query: query.to_owned(),
                        body: serde_json::from_str(&body).unwrap_or(Value::Null),
                        at: std::time::Instant::now(),
                        token: head
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("authorization")
                                    .then(|| v.trim().trim_start_matches("Bearer ").to_owned())
                            })
                            .unwrap_or_default(),
                    };
                    let answer = fake.answer(&req);
                    fake.seen.lock().unwrap().push(req);
                    tokio::time::sleep(answer.delay).await;
                    let resp = format!(
                        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{}",
                        answer.status,
                        answer.body.len(),
                        answer.headers,
                        answer.body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });
        base
    }
}

/// The meta addon's answers: a show with six episodes (TVDB ids 1001 to
/// 1006, the fourth without one) and a special, and a movie.
fn meta(path: &str) -> Value {
    if path.contains(MOVIE) {
        return json!({"meta": {"id": MOVIE, "type": "movie", "name": "Film", "runtime": "100 min"}});
    }
    let mut videos: Vec<Value> = (1..=6)
        .map(|e| {
            let mut v = json!({"id": format!("{SHOW}:1:{e}"), "title": format!("E{e}"), "season": 1, "episode": e});
            if e != 4 {
                v["tvdb_id"] = json!(1000 + e);
            }
            v
        })
        .collect();
    videos.push(json!({"id": format!("{SHOW}:0:1"), "title": "Special", "season": 0, "episode": 1, "tvdb_id": 9001}));
    json!({"meta": {"id": SHOW, "type": "series", "name": "Show", "runtime": "40 min", "videos": videos}})
}

fn activities(at: &str) -> Value {
    json!({
        "all": at,
        "movies": {"watched_at": at, "paused_at": at},
        "episodes": {"watched_at": at, "paused_at": at},
    })
}

fn record(account: &str) -> TraktRecord {
    let now = mq_library::hlc::now_ms();
    serde_json::from_value(json!({
        "accessToken": "access", "refreshToken": "refresh", "createdAtMs": now,
        "expiresAtMs": now + 7 * 86_400_000, "scope": "public", "account": account,
        "user": "tester", "revision": 0, "generation": 1
    }))
    .unwrap()
}

/// A core whose Trakt is the fake, with the fake's meta addon installed.
async fn setup(account: &str) -> (Core, Fake) {
    let fake = Fake::new(account);
    let base = fake.start().await;
    let core = Core::temporary("p").unwrap();
    core.set_trakt_config(Some(TraktConfig {
        client_id: "client".into(),
        redirect_uri: "https://marquee.example/trakt".into(),
        hosts: TraktHosts {
            api: base.clone(),
            auth: base.clone(),
            local: true,
        },
    }));
    core.install_addon(&format!("{base}/manifest.json"))
        .await
        .unwrap();
    (core, fake)
}

/// Signed in, with the rounds that follow done (the scheduler's and one
/// more), so a test's own requests are the only ones after.
async fn signed_in(account: &str) -> (Core, Fake) {
    let (core, fake) = setup(account).await;
    core.set_trakt(Some(record(account))).await.unwrap();
    core.trakt_sync().await.unwrap();
    fake.wait_for("the scheduler's round", |f| {
        f.seen("GET", "/sync/last_activities").len() >= 2
    })
    .await;
    core.trakt_sync().await.unwrap();
    (core, fake)
}

fn episode(e: u32) -> PlayTarget {
    PlayTarget {
        kind: "series".into(),
        item_id: SHOW.into(),
        video_id: format!("{SHOW}:1:{e}"),
        name: Some("Show".into()),
        poster: None,
        season: Some(1),
        episode: Some(e),
    }
}

fn key(account: &str) -> TrackerKey<'_> {
    TrackerKey {
        profile: "p",
        tracker: "trakt",
        account,
    }
}

const MINUTES: u64 = 40 * 60 * 1000;

fn at(percent: u64) -> u64 {
    MINUTES * percent / 100
}

fn watched(core: &Core, video: &str) -> bool {
    core.library()
        .watched_among("p", &[video])
        .unwrap()
        .contains(video)
}

#[tokio::test]
async fn start_pause_and_stop_follow_the_player() {
    let (core, fake) = signed_in("a-follow").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(10), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    // A seek in the same state sends nothing.
    view.report_progress(at(30), MINUTES, false);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        fake.scrobbles().len(),
        1,
        "a seek sent {:?}",
        fake.scrobbles()
    );
    view.report_progress(at(31), MINUTES, true);
    fake.wait_for("pause", |f| f.scrobbles().len() == 2).await;
    view.report_progress(at(40), MINUTES, false);
    fake.wait_for("start again", |f| f.scrobbles().len() == 3)
        .await;
    view.report_progress(at(50), MINUTES, false);
    drop(view);
    fake.wait_for("stop", |f| f.scrobbles().len() == 4).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let sent = fake.scrobbles();
    let actions: Vec<&str> = sent.iter().map(|(a, _)| a.as_str()).collect();
    assert_eq!(actions, ["start", "pause", "start", "stop"], "{sent:?}");
    assert_eq!(sent[0].1, 10.0);
    assert_eq!(sent[1].1, 31.0);
    assert_eq!(sent[3].1, 50.0, "the stop carries the last position");
    let body = &fake.seen("POST", "/scrobble/start")[0].body;
    assert_eq!(body["episode"]["ids"]["tvdb"], 1001, "{body}");
    // Trakt answered `pause` (under 80 percent): no mark.
    assert!(!watched(&core, "tt100:1:1"));
}

fn actions(fake: &Fake) -> Vec<String> {
    fake.scrobbles().into_iter().map(|(a, _)| a).collect()
}

fn pending(core: &Core, account: &str) -> usize {
    core.library().tracker_pending(key(account)).unwrap().0
}

fn ready(core: &Core, account: &str) -> Vec<mq_library::Intent> {
    core.library()
        .ready_tracker_intents(key(account), i64::MAX, 100)
        .unwrap()
}

/// History writes sent, each as (path, its episodes' TVDB ids).
fn history_writes(fake: &Fake) -> Vec<(String, Vec<u64>)> {
    let seen = fake.seen.lock().unwrap();
    seen.iter()
        .filter(|r| r.method == "POST" && r.path.starts_with("/sync/history"))
        .map(|r| {
            let ids = r.body["episodes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["ids"]["tvdb"].as_u64())
                .collect();
            (r.path.clone(), ids)
        })
        .collect()
}

async fn until(what: &str, check: impl Fn() -> bool) {
    until_within(what, 10, check).await;
}

async fn until_within(what: &str, secs: u64, check: impl Fn() -> bool) {
    for _ in 0..secs * 20 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("waited {secs} s for {what}");
}

#[tokio::test]
async fn pause_then_start_sends_only_the_newest() {
    let (core, fake) = signed_in("a-newest").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    // A burst before the worker runs: only the last state goes.
    view.report_progress(at(10), MINUTES, true);
    view.report_progress(at(10), MINUTES, false);
    view.report_progress(at(11), MINUTES, true);
    view.report_progress(at(12), MINUTES, false);
    fake.wait_for("start", |f| !f.scrobbles().is_empty()).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(fake.scrobbles(), [("start".to_owned(), 12.0)]);
    std::mem::forget(view);
}

#[tokio::test]
async fn background_pauses_and_resume_starts() {
    let (core, fake) = signed_in("a-background").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(20), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    view.report_progress(at(25), MINUTES, false);
    // Answers once the pause is sent, with the true position: playback may
    // go on in picture in picture.
    core.trakt_background().await;
    assert_eq!(fake.scrobbles().last(), Some(&("pause".to_owned(), 25.0)));
    view.report_progress(at(26), MINUTES, false);
    fake.wait_for("start again", |f| f.scrobbles().len() == 3)
        .await;
    assert_eq!(actions(&fake), ["start", "pause", "start"]);
    std::mem::forget(view);
}

#[tokio::test]
async fn close_sends_one_stop_after_the_view_is_gone() {
    let (core, fake) = signed_in("a-close").await;
    // Under one percent: the close sends nothing.
    let short = PlaybackView::open(&core, Vec::new(), episode(1));
    short.report_progress(10_000, MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    drop(short);
    let view = PlaybackView::open(&core, Vec::new(), episode(2));
    view.report_progress(at(60), MINUTES, false);
    fake.wait_for("second start", |f| f.scrobbles().len() == 2)
        .await;
    drop(view);
    fake.wait_for("stop", |f| f.scrobbles().len() == 3).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(actions(&fake), ["start", "start", "stop"]);
    assert_eq!(fake.scrobbles()[2].1, 60.0);
}

#[tokio::test]
async fn a_play_to_the_end_is_held_then_confirmed_by_its_stop() {
    let (core, fake) = signed_in("a-held").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(2));
    view.report_progress(at(95), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    // The local mark is set, and its write waits for the session.
    assert!(watched(&core, "tt100:1:2"));
    assert_eq!(pending(&core, "a-held"), 1);
    assert!(ready(&core, "a-held").is_empty(), "held while it plays");
    core.trakt_sync().await.unwrap();
    assert!(history_writes(&fake).is_empty());
    drop(view);
    fake.wait_for("stop", |f| f.scrobbles().len() == 2).await;
    until("the held write to go", || pending(&core, "a-held") == 0).await;
    // Trakt took the play: nothing else is sent.
    core.trakt_sync().await.unwrap();
    assert!(
        history_writes(&fake).is_empty(),
        "{:?}",
        history_writes(&fake)
    );
    assert!(watched(&core, "tt100:1:2"));
}

#[tokio::test]
async fn a_stop_answered_pause_releases_the_held_write() {
    let (core, fake) = signed_in("a-release").await;
    *fake.stop_action.lock().unwrap() = Some("pause".into());
    let view = PlaybackView::open(&core, Vec::new(), episode(2));
    view.report_progress(at(95), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    drop(view);
    fake.wait_for("stop", |f| f.scrobbles().len() == 2).await;
    until("the release", || ready(&core, "a-release").len() == 1).await;
    core.trakt_sync().await.unwrap();
    assert_eq!(
        history_writes(&fake),
        [("/sync/history".to_owned(), vec![1002])]
    );
    assert_eq!(pending(&core, "a-release"), 0);
}

#[tokio::test]
async fn an_unanswered_stop_checks_the_minutes_around_it() {
    let (core, fake) = signed_in("a-ambiguous").await;
    let down = || Reply {
        status: 503,
        body: "{}".into(),
        delay: Duration::ZERO,
        headers: String::new(),
    };
    fake.queue("POST /scrobble/stop", (0..8).map(|_| down()).collect());
    let three = PlaybackView::open(&core, Vec::new(), episode(3));
    let five = PlaybackView::open(&core, Vec::new(), episode(5));
    three.report_progress(at(95), MINUTES, false);
    five.report_progress(at(95), MINUTES, false);
    fake.wait_for("starts", |f| f.scrobbles().len() >= 2).await;
    // Trakt has episode 3's play in the minutes around its stop, not 5's.
    fake.list(
        "/sync/history",
        vec![json!({
            "watched_at": iso(now()), "type": "episode",
            "episode": {"season": 1, "number": 3, "ids": {"tvdb": 1003}},
            "show": {"title": "Show", "ids": {"imdb": SHOW}}
        })],
    );
    drop(three);
    drop(five);
    // Each released write is checked, then sent or dropped, by the rounds
    // the stops start; waits ride on the backoff, so the deadlines are long.
    fake.wait_within("every stop tried", 120, |f| {
        f.seen("POST", "/scrobble/stop").len() >= 8
    })
    .await;
    until_within("both handled", 60, || pending(&core, "a-ambiguous") == 0).await;
    core.trakt_sync().await.unwrap();
    let checks: Vec<String> = fake
        .seen("GET", "/sync/history")
        .into_iter()
        .map(|r| r.query)
        .filter(|q| q.contains("end_at") && q.contains("page=1&"))
        .collect();
    assert_eq!(checks.len(), 2, "{checks:?}");
    assert_eq!(
        history_writes(&fake),
        [("/sync/history".to_owned(), vec![1005])]
    );
    assert_eq!(pending(&core, "a-ambiguous"), 0);
}

/// An RFC 3339 time, as Trakt writes them.
fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        ms.rem_euclid(1000)
    )
}

#[tokio::test]
async fn an_unmark_after_the_start_wins_over_the_scrobble() {
    let (core, fake) = signed_in("a-unmark").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(95), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    assert!(watched(&core, "tt100:1:1"));
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:1", false).unwrap();
    drop(view);
    fake.wait_for("stop", |f| f.scrobbles().len() == 2).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!watched(&core, "tt100:1:1"), "the user's unmark stays");
    // And Trakt gets the removal.
    core.trakt_sync().await.unwrap();
    assert_eq!(
        history_writes(&fake),
        [("/sync/history/remove".to_owned(), vec![1001])]
    );
}

fn status(core: &Core) -> mq_runtime::TraktStatus {
    TraktView::open(core).snapshot()
}

#[tokio::test]
async fn a_wrong_show_answer_marks_it_unmapped() {
    let (core, fake) = signed_in("a-wrong").await;
    fake.shows.lock().unwrap().insert(1001, "tt999".into());
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(50), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    until("unmapped", || {
        view.snapshot().trakt == TraktPlayback::Unmapped
    })
    .await;
    drop(view);
    // Another episode of that show: nothing is sent for this account.
    let next = PlaybackView::open(&core, Vec::new(), episode(2));
    next.report_progress(at(50), MINUTES, false);
    drop(next);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(actions(&fake), ["start"], "no stop and no second start");
    let unmapped = status(&core).unmapped;
    assert_eq!(unmapped.len(), 1, "{unmapped:?}");
    assert_eq!(unmapped[0].item_id, SHOW);
    assert_eq!(unmapped[0].reason, "Trakt matched it to another title");
}

#[tokio::test]
async fn episodes_trakt_cannot_name_are_unmapped() {
    let (core, fake) = signed_in("a-unknown").await;
    // The fourth episode has no TVDB id; the special is season 0.
    let mut special = episode(1);
    special.video_id = "tt100:0:1".into();
    for target in [episode(4), special] {
        let view = PlaybackView::open(&core, Vec::new(), target);
        view.report_progress(at(50), MINUTES, false);
        until("unmapped", || {
            view.snapshot().trakt == TraktPlayback::Unmapped
        })
        .await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(fake.scrobbles().is_empty());
    let reasons: Vec<String> = status(&core)
        .unmapped
        .into_iter()
        .map(|u| u.reason)
        .collect();
    assert!(
        reasons.iter().any(|r| r == "a special (season 0)"),
        "{reasons:?}"
    );
}

#[tokio::test]
async fn an_episode_change_stops_the_old_session_first() {
    let (core, fake) = signed_in("a-change").await;
    let one = PlaybackView::open(&core, Vec::new(), episode(1));
    one.report_progress(at(90), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    // The player moves on before it closes the old view.
    let two = PlaybackView::open(&core, Vec::new(), episode(2));
    two.report_progress(at(1), MINUTES, false);
    fake.wait_for("stop and start", |f| f.scrobbles().len() == 3)
        .await;
    drop(one);
    two.report_progress(at(40), MINUTES, false);
    drop(two);
    fake.wait_for("last stop", |f| f.scrobbles().len() == 4)
        .await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let sent: Vec<(String, u64)> = fake
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.path.starts_with("/scrobble/"))
        .map(|r| {
            (
                r.path[10..].to_owned(),
                r.body["episode"]["ids"]["tvdb"].as_u64().unwrap(),
            )
        })
        .collect();
    let expected = [
        ("start", 1001),
        ("stop", 1001),
        ("start", 1002),
        ("stop", 1002),
    ];
    assert_eq!(
        sent,
        expected.map(|(a, t)| (a.to_owned(), t)),
        "the old episode's stop goes before the new one's start"
    );
}

#[tokio::test]
async fn writes_held_by_a_killed_process_are_released() {
    let core = Core::temporary("p").unwrap();
    let lib = core.library();
    lib.set_tracker_account("p", "trakt", Some("a-killed"), 1, 1)
        .unwrap();
    let intent = NewIntent {
        key: key("a-killed"),
        op: IntentOp::Add,
        watched_ms: 5,
        session: Some("gone-1"),
    };
    lib.put_tracker_intent(SHOW, "tt100:1:1", intent).unwrap();
    assert!(ready(&core, "a-killed").is_empty());
    // Trakt's first use in this process releases them.
    core.set_trakt_config(None);
    assert_eq!(ready(&core, "a-killed").len(), 1);
}

#[tokio::test]
async fn scrobbles_wait_for_the_record() {
    let (core, fake) = setup("a-wait").await;
    // Another device signed the profile in; this one has no record yet.
    core.library()
        .set_setting("p", "trakt.account", "\"a-wait\"", core.clock())
        .unwrap();
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    assert_eq!(view.snapshot().trakt, TraktPlayback::ThisDeviceOff);
    view.report_progress(at(20), MINUTES, false);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(fake.scrobbles().is_empty());
    core.set_trakt(Some(record("a-wait"))).await.unwrap();
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    until("on", || view.snapshot().trakt == TraktPlayback::On).await;
    std::mem::forget(view);
}

#[tokio::test]
async fn a_failed_start_is_tried_again_unless_overtaken() {
    let (core, fake) = signed_in("a-retry").await;
    let down = || Reply {
        status: 503,
        body: "{}".into(),
        delay: Duration::ZERO,
        headers: String::new(),
    };
    fake.queue("POST /scrobble/start", vec![down()]);
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(20), MINUTES, false);
    fake.wait_for("retried start", |f| f.scrobbles().len() == 2)
        .await;
    assert_eq!(actions(&fake), ["start", "start"]);
    // A failed start overtaken by a pause is not tried again.
    fake.queue("POST /scrobble/start", vec![down()]);
    let other = PlaybackView::open(&core, Vec::new(), episode(2));
    other.report_progress(at(20), MINUTES, false);
    fake.wait_for("third start", |f| f.scrobbles().len() == 3)
        .await;
    other.report_progress(at(21), MINUTES, true);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let starts_of_two = fake
        .seen("POST", "/scrobble/start")
        .iter()
        .filter(|r| r.body["episode"]["ids"]["tvdb"] == 1002)
        .count();
    assert_eq!(starts_of_two, 1, "{:?}", fake.scrobbles());
    std::mem::forget((view, other));
}

#[tokio::test]
async fn a_refused_scrobble_is_not_tried_again() {
    let (core, fake) = signed_in("a-limit").await;
    fake.queue(
        "POST /scrobble/start",
        vec![Reply {
            status: 420,
            body: String::new(),
            delay: Duration::ZERO,
            headers: String::new(),
        }],
    );
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(20), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(actions(&fake), ["start"]);
    assert_eq!(
        status(&core).last_error.as_deref(),
        Some("status 420 (account limit)")
    );
    std::mem::forget(view);
}

fn now() -> i64 {
    mq_library::hlc::now_ms() as i64
}

fn episode_row(e: u64, at_ms: i64) -> Value {
    json!({
        "watched_at": iso(at_ms), "type": "episode",
        "episode": {"season": 1, "number": e, "ids": {"tvdb": 1000 + e}},
        "show": {"title": "Show", "ids": {"imdb": SHOW, "trakt": 77}}
    })
}

fn movie_row(imdb: &str, at_ms: i64) -> Value {
    json!({
        "watched_at": iso(at_ms), "last_watched_at": iso(at_ms), "type": "movie",
        "movie": {"title": "Film", "ids": {"imdb": imdb}}
    })
}

fn state(core: &Core, account: &str, name: &str) -> Option<String> {
    core.library().tracker_state(key(account), name).unwrap()
}

#[tokio::test]
async fn hand_marks_go_out_in_one_batch_and_unmarks_as_removals() {
    let (core, fake) = signed_in("a-hand").await;
    let title = TitleView::open(&core, "series", SHOW);
    for e in 1..=3 {
        title.set_watched(&format!("tt100:1:{e}"), true).unwrap();
    }
    assert_eq!(pending(&core, "a-hand"), 3);
    let report = core.trakt_sync().await.unwrap();
    assert_eq!(
        history_writes(&fake),
        [("/sync/history".to_owned(), vec![1001, 1002, 1003])]
    );
    let body = &fake.seen("POST", "/sync/history")[0].body;
    assert!(body["episodes"][0]["watched_at"].is_string(), "{body}");
    assert_eq!((report.pushed, report.pending), (3, 0));
    title.set_watched("tt100:1:2", false).unwrap();
    // A mark that does not flip queues nothing.
    title.set_watched("tt100:1:1", true).unwrap();
    assert_eq!(pending(&core, "a-hand"), 1);
    core.trakt_sync().await.unwrap();
    let writes = history_writes(&fake);
    assert_eq!(writes[1], ("/sync/history/remove".to_owned(), vec![1002]));
    let body = &fake.seen("POST", "/sync/history/remove")[0].body;
    assert!(body["episodes"][0].get("watched_at").is_none(), "{body}");
}

#[tokio::test]
async fn history_not_found_is_unmapped_and_a_200_acknowledges() {
    let (core, fake) = signed_in("a-notfound").await;
    fake.queue(
        "POST /sync/history",
        vec![reply(
            200,
            json!({"added": {"movies": 0, "episodes": 1}, "not_found": {"movies": [], "episodes": [{"ids": {"tvdb": 1002}}]}}),
        )],
    );
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:1", true).unwrap();
    title.set_watched("tt100:1:2", true).unwrap();
    let report = core.trakt_sync().await.unwrap();
    assert_eq!((report.pushed, report.pending, report.unmapped), (1, 0, 1));
    let unmapped = status(&core).unmapped;
    assert_eq!(unmapped[0].item_id, SHOW);
    assert_eq!(unmapped[0].reason, "Trakt does not know it");
}

#[tokio::test]
async fn a_failed_history_write_waits_and_an_old_one_shows_failing() {
    let (core, fake) = signed_in("a-failing").await;
    let down = Reply {
        status: 503,
        body: "{}".into(),
        delay: Duration::ZERO,
        headers: String::new(),
    };
    fake.queue("POST /sync/history", vec![down]);
    let tracker = Some((key("a-failing"), now() - 31 * 60 * 1000));
    core.library()
        .set_watched_by_hand("p", SHOW, "tt100:1:1", true, core.clock(), tracker)
        .unwrap();
    core.trakt_sync().await.unwrap();
    assert_eq!(pending(&core, "a-failing"), 1, "kept for a retry");
    let waiting = core
        .library()
        .ready_tracker_intents(key("a-failing"), now(), 10)
        .unwrap();
    assert!(waiting.is_empty(), "not before its retry time");
    match status(&core).state {
        TraktState::Failing { reason, .. } => {
            assert_eq!(reason, "Trakt did not answer: status 503")
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_round_reads_history_from_its_cursor_day() {
    let (core, fake) = signed_in("a-history").await;
    let cursor = state(&core, "a-history", "history_day").expect("set by the first round");
    fake.list(
        "/sync/history",
        vec![
            episode_row(1, now() - 3_600_000),
            movie_row(MOVIE, now() - 7_200_000),
        ],
    );
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    let report = core.trakt_sync().await.unwrap();
    let asked = fake.seen("GET", "/sync/history");
    assert!(
        asked[0]
            .query
            .starts_with(&format!("start_at={cursor}T00:00:00.000Z&page=1")),
        "{}",
        asked[0].query
    );
    assert!(watched(&core, "tt100:1:1") && watched(&core, MOVIE));
    assert_eq!(report.applied, 2);
    // Nothing moved: no history read.
    let before = fake.seen("GET", "/sync/history").len();
    core.trakt_sync().await.unwrap();
    assert_eq!(fake.seen("GET", "/sync/history").len(), before);
}

#[tokio::test]
async fn a_failed_page_keeps_the_cursor() {
    let (core, fake) = signed_in("a-page").await;
    fake.page.store(1, Ordering::SeqCst);
    let rows = vec![
        episode_row(1, now() - 3_600_000),
        episode_row(2, now() - 1_800_000),
    ];
    fake.list("/sync/history", rows.clone());
    let stored = state(&core, "a-page", "activities");
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    let down = Reply {
        status: 503,
        body: "{}".into(),
        delay: Duration::ZERO,
        headers: String::new(),
    };
    fake.queue(
        "GET /sync/history",
        vec![reply(200, json!([rows[0]])), down],
    );
    assert!(core.trakt_sync().await.is_err());
    assert!(
        !watched(&core, "tt100:1:1"),
        "nothing applied from a partial read"
    );
    assert_eq!(
        state(&core, "a-page", "activities"),
        stored,
        "the cursor stayed"
    );
    core.trakt_sync().await.unwrap();
    assert!(watched(&core, "tt100:1:1") && watched(&core, "tt100:1:2"));
}

#[tokio::test]
async fn first_connect_reads_every_page_and_resumes() {
    let (core, fake) = setup("a-first").await;
    fake.page.store(2, Ordering::SeqCst);
    let t = now() - 86_400_000;
    fake.list(
        "/sync/watched/movies",
        ["tt200", "tt201", "tt202"]
            .map(|m| movie_row(m, t))
            .to_vec(),
    );
    fake.list(
        "/sync/watched/shows",
        vec![json!({"last_updated_at": "2026-10-01T00:00:00.000Z",
                    "show": {"title": "Show", "ids": {"trakt": 77, "imdb": SHOW}}})],
    );
    fake.list(
        "/sync/history/shows/77",
        [1, 2, 3, 5, 6].map(|e| episode_row(e, t)).to_vec(),
    );
    // The first show history read fails: the import stops there.
    let down = Reply {
        status: 503,
        body: "{}".into(),
        delay: Duration::ZERO,
        headers: String::new(),
    };
    fake.queue("GET /sync/history/shows/77", vec![down]);
    core.set_trakt(Some(record("a-first"))).await.unwrap();
    let _ = core.trakt_sync().await;
    let _ = core.trakt_sync().await;
    assert!(
        ["tt200", "tt201", "tt202"]
            .iter()
            .all(|m| watched(&core, m))
    );
    for e in [1, 2, 3, 5, 6] {
        assert!(watched(&core, &format!("tt100:1:{e}")), "episode {e}");
    }
    assert_eq!(
        state(&core, "a-first", "show:77").as_deref(),
        Some("2026-10-01T00:00:00.000Z")
    );
    // Every page: 5 rows at 2 a page is 3 pages and an empty one.
    let pages = fake
        .seen("GET", "/sync/history/shows/77")
        .iter()
        .filter(|r| r.query.contains("page="))
        .count();
    assert!(pages >= 4, "{pages}");
    // A later reconcile skips the unchanged show.
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    let before = fake.seen("GET", "/sync/history/shows/77").len();
    core.trakt_sync().await.unwrap();
    assert_eq!(fake.seen("GET", "/sync/history/shows/77").len(), before);
}

#[tokio::test]
async fn a_backdated_play_is_reconciled_with_episode_ids() {
    let (core, fake) = signed_in("a-backdated").await;
    // The watched time moved, but no row is newer than the last one seen:
    // a play added in the past.
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    fake.list(
        "/sync/watched/shows",
        vec![json!({"last_updated_at": iso(now()),
                    "show": {"title": "Show", "ids": {"trakt": 77, "imdb": SHOW}}})],
    );
    fake.list(
        "/sync/history/shows/77",
        vec![episode_row(2, 1_000_000_000_000)],
    );
    core.trakt_sync().await.unwrap();
    assert!(watched(&core, "tt100:1:2"));
}

#[tokio::test]
async fn rows_dated_ahead_wait() {
    let (core, fake) = signed_in("a-future").await;
    let today = state(&core, "a-future", "history_day").unwrap();
    fake.list(
        "/sync/history",
        vec![
            episode_row(1, now() - 3_600_000),
            episode_row(3, now() + 86_400_000),
        ],
    );
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    core.trakt_sync().await.unwrap();
    assert!(watched(&core, "tt100:1:1"));
    assert!(!watched(&core, "tt100:1:3"), "a row dated tomorrow waits");
    assert_eq!(
        state(&core, "a-future", "history_day"),
        Some(today),
        "not past it"
    );
    // In a reconcile, the show stays open to be read again.
    fake.list("/sync/history", vec![]);
    *fake.activities.lock().unwrap() = activities(&iso(now() + 1000));
    fake.list(
        "/sync/watched/shows",
        vec![json!({"last_updated_at": iso(now()),
                    "show": {"title": "Show", "ids": {"trakt": 78, "imdb": SHOW}}})],
    );
    fake.list(
        "/sync/history/shows/78",
        vec![episode_row(5, now() + 86_400_000)],
    );
    core.trakt_sync().await.unwrap();
    assert!(!watched(&core, "tt100:1:5"));
    assert_eq!(state(&core, "a-future", "show:78"), None);
}

#[tokio::test]
async fn own_pauses_do_not_come_back_and_others_resume() {
    let (core, fake) = signed_in("a-echo").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(2));
    view.report_progress(at(40), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    view.report_progress(at(40), MINUTES, true);
    fake.wait_for("pause", |f| f.scrobbles().len() == 2).await;
    std::mem::forget(view);
    // Episode 5 has older local progress with its duration (30 minutes).
    let old = mq_library::Hlc {
        wall_ms: (now() - 86_400_000) as u64,
        counter: 0,
        device: 1,
    }
    .encode();
    let row = mq_library::ProgressRow {
        video_id: "tt100:1:5",
        item_id: SHOW,
        position_ms: 60_000,
        duration_ms: 1_800_000,
        updated_ms: now() - 86_400_000,
    };
    core.library().set_progress("p", row, old.as_str()).unwrap();
    let item = |e: u64, progress: f64| {
        let mut row = episode_row(e, now());
        row["progress"] = json!(progress);
        row["paused_at"] = json!(iso(now()));
        row
    };
    fake.list(
        "/sync/playback",
        vec![item(2, 40.3), item(3, 50.0), item(5, 60.0), item(6, 1.5)],
    );
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    core.trakt_sync().await.unwrap();
    let progress = |v: &str| core.library().progress("p", v).unwrap();
    assert_eq!(
        progress("tt100:1:2"),
        Some((at(40) as i64, MINUTES as i64)),
        "own pause"
    );
    // No local duration: the meta's 40 minutes, 97 percent of it.
    assert_eq!(progress("tt100:1:3"), Some((1_164_000, 2_400_000)));
    assert_eq!(progress("tt100:1:5"), Some((1_080_000, 1_800_000)));
    assert_eq!(progress("tt100:1:6"), None, "under 2 percent");
}

#[tokio::test]
async fn without_an_app_every_trakt_op_says_not_configured() {
    // Tests build with no MARQUEE_TRAKT_CLIENT_ID: the build's default.
    let core = Core::temporary("p").unwrap();
    let refused = core.set_trakt(Some(record("x"))).await;
    assert!(matches!(
        refused,
        Err(mq_runtime::CoreError::TraktNotConfigured)
    ));
    core.set_trakt(None).await.unwrap();
    let sync = core.trakt_sync().await;
    assert!(matches!(
        sync,
        Err(mq_runtime::CoreError::TraktNotConfigured)
    ));
    assert_eq!(status(&core).state, TraktState::NotConfigured);
    let login = TraktLoginView::open(&core);
    assert_eq!(login.snapshot(), TraktLoginSnapshot::NotConfigured);
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    assert_eq!(view.snapshot().trakt, TraktPlayback::Off);
}

#[tokio::test]
async fn a_device_code_sign_in_offers_the_record_until_stored() {
    let (core, fake) = setup("a-login").await;
    fake.queue(
        "POST /oauth/device/code",
        vec![reply(
            200,
            json!({"device_code": "dev", "user_code": "ABCD1234",
                   "verification_url": "https://trakt.tv/activate", "expires_in": 600, "interval": 1}),
        )],
    );
    let tokens = json!({"access_token": "at", "refresh_token": "rt", "expires_in": 604_800,
                        "created_at": now() / 1000, "scope": "public", "token_type": "bearer"});
    fake.queue(
        "POST /oauth/device/token",
        vec![
            reply(400, json!({})),
            reply(429, json!({})),
            reply(200, tokens),
        ],
    );
    let login = TraktLoginView::open(&core);
    until("the code", || {
        matches!(login.snapshot(), TraktLoginSnapshot::Code { ref user_code, .. } if user_code == "ABCD1234")
    })
    .await;
    for _ in 0..300 {
        if login.snapshot() != TraktLoginSnapshot::Starting
            && !matches!(login.snapshot(), TraktLoginSnapshot::Code { .. })
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        login.snapshot(),
        TraktLoginSnapshot::Done {
            user: "tester".into()
        }
    );
    // Slowed down once: the third poll came at least 5 s after the second.
    let polls = fake.seen("POST", "/oauth/device/token");
    assert_eq!(polls.len(), 3);
    let gap = polls[2].at - polls[1].at;
    assert!(gap >= Duration::from_secs(5), "{gap:?}");
    let held = status(&core);
    assert_eq!(
        (held.state.clone(), held.user.as_deref()),
        (TraktState::Connected, Some("tester"))
    );
    let record = held.record.expect("offered to the app");
    assert_eq!(record.account, "a-login");
    core.set_trakt(Some(record)).await.unwrap();
    assert_eq!(status(&core).record, None, "stored");
}

#[tokio::test]
async fn signing_out_drops_the_accounts_work_and_keeps_the_library() {
    let (core, fake) = signed_in("a-out").await;
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:1", true).unwrap();
    assert_eq!(pending(&core, "a-out"), 1);
    assert!(state(&core, "a-out", "history_day").is_some());
    // Another device of the profile is still signed in.
    core.library()
        .set_setting("p", "trakt.device.42", "\"a-out\"", core.clock())
        .unwrap();
    core.trakt_sign_out().await.unwrap();
    assert_eq!(fake.seen("POST", "/oauth/revoke").len(), 1);
    assert_eq!(pending(&core, "a-out"), 0);
    assert_eq!(state(&core, "a-out", "history_day"), None);
    assert!(watched(&core, "tt100:1:1"), "the library keeps its marks");
    assert_eq!(status(&core).state, TraktState::ThisDeviceOff);
    // The last device out: the profile stops using Trakt.
    core.library()
        .set_setting("p", "trakt.device.42", "null", core.clock())
        .unwrap();
    core.trakt_sign_out().await.unwrap();
    assert_eq!(status(&core).state, TraktState::Off);
}

#[tokio::test]
async fn a_scrobble_racing_a_pull_lands_once() {
    let (core, fake) = signed_in("a-race").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(95), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    // Trakt also has an older play of it from another app, and another
    // episode; the stop answers only after the round reached its writes.
    fake.list(
        "/sync/history",
        vec![
            episode_row(1, now() - 3_600_000),
            episode_row(2, now() - 1_800_000),
        ],
    );
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    let answer = json!({"action": "scrobble", "progress": 95.0,
                        "episode": {"ids": {"tvdb": 1001}}, "show": {"ids": {"imdb": SHOW}}});
    fake.queue(
        "POST /scrobble/stop",
        vec![Reply {
            status: 201,
            body: answer.to_string(),
            delay: Duration::from_secs(8),
            headers: String::new(),
        }],
    );
    drop(view);
    let report = core.trakt_sync().await.unwrap();
    fake.wait_for("stop", |f| f.scrobbles().len() == 2).await;
    until("the held write to go", || pending(&core, "a-race") == 0).await;
    core.trakt_sync().await.unwrap();
    assert_eq!(actions(&fake), ["start", "stop"]);
    assert!(
        history_writes(&fake).is_empty(),
        "the play went once, as the scrobble"
    );
    assert!(watched(&core, "tt100:1:1") && watched(&core, "tt100:1:2"));
    assert!(report.applied >= 1, "{report:?}");
}

#[tokio::test]
async fn another_devices_sign_in_shows_this_device_off() {
    let (core, _fake) = setup("a-other").await;
    assert_eq!(status(&core).state, TraktState::Off);
    core.library()
        .set_setting("p", "trakt.account", "\"a-other\"", core.clock())
        .unwrap();
    assert_eq!(status(&core).state, TraktState::ThisDeviceOff);
}

fn record_as(account: &str, access: &str, generation: u64) -> TraktRecord {
    TraktRecord {
        account: account.into(),
        access_token: access.into(),
        generation,
        ..record(account)
    }
}

fn down() -> Reply {
    Reply {
        status: 503,
        body: "{}".into(),
        delay: Duration::ZERO,
        headers: String::new(),
    }
}

fn watched_in(core: &Core, video: &str) -> bool {
    core.library()
        .watched_among(core.profile(), &[video])
        .unwrap()
        .contains(video)
}

/// Makes every waiting write due now.
fn due(core: &Core, account: &str) {
    for intent in ready(core, account) {
        core.library()
            .retry_tracker_intent(key(account), &intent.video_id, intent.rev, 0)
            .unwrap();
    }
}

#[tokio::test]
async fn a_session_keeps_its_connection() {
    let (core, fake) = signed_in("a-bind").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(50), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    // Another account signs in on this device while it plays.
    core.set_trakt(Some(record_as("a-bind-other", "other-access", 2)))
        .await
        .unwrap();
    drop(view);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let stops = fake.seen("POST", "/scrobble/stop");
    assert!(
        stops.is_empty(),
        "the old account's stop never goes with the new token: {stops:?}"
    );
}

#[tokio::test]
async fn a_hand_mark_waits_while_the_shows_details_are_down() {
    let (core, fake) = signed_in("a-meta-hand").await;
    fake.meta_down.store(true, Ordering::SeqCst);
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:1", true).unwrap();
    core.trakt_sync().await.unwrap();
    assert!(history_writes(&fake).is_empty());
    assert_eq!(pending(&core, "a-meta-hand"), 1, "kept for a later try");
    assert!(
        status(&core).unmapped.is_empty(),
        "never listed as unmapped"
    );
    fake.meta_down.store(false, Ordering::SeqCst);
    due(&core, "a-meta-hand");
    core.trakt_sync().await.unwrap();
    assert_eq!(
        history_writes(&fake),
        [("/sync/history".to_owned(), vec![1001])]
    );
}

#[tokio::test]
async fn a_first_import_waits_while_the_shows_details_are_down() {
    let (core, fake) = setup("a-meta-first").await;
    fake.meta_down.store(true, Ordering::SeqCst);
    let t = now() - 86_400_000;
    fake.list(
        "/sync/watched/shows",
        vec![json!({"last_updated_at": "2026-10-01T00:00:00.000Z",
                    "show": {"title": "Show", "ids": {"trakt": 77, "imdb": SHOW}}})],
    );
    fake.list(
        "/sync/history/shows/77",
        vec![episode_row(1, t), episode_row(2, t)],
    );
    core.set_trakt(Some(record("a-meta-first"))).await.unwrap();
    assert!(core.trakt_sync().await.is_err());
    assert_eq!(
        state(&core, "a-meta-first", "show:77"),
        None,
        "not checkpointed"
    );
    assert!(!watched(&core, "tt100:1:1"));
    assert!(
        status(&core).unmapped.is_empty(),
        "never listed as unmapped"
    );
    fake.meta_down.store(false, Ordering::SeqCst);
    core.trakt_sync().await.unwrap();
    assert!(watched(&core, "tt100:1:1") && watched(&core, "tt100:1:2"));
    assert_eq!(
        state(&core, "a-meta-first", "show:77").as_deref(),
        Some("2026-10-01T00:00:00.000Z")
    );
}

#[tokio::test]
async fn a_scrobble_waits_while_the_shows_details_are_down() {
    let (core, fake) = signed_in("a-meta-play").await;
    fake.meta_down.store(true, Ordering::SeqCst);
    let view = PlaybackView::open(&core, Vec::new(), episode(2));
    view.report_progress(at(30), MINUTES, false);
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(fake.scrobbles().is_empty());
    assert_eq!(view.snapshot().trakt, TraktPlayback::On, "not unmapped");
    fake.meta_down.store(false, Ordering::SeqCst);
    view.report_progress(at(31), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    assert_eq!(view.snapshot().trakt, TraktPlayback::On);
    assert!(status(&core).unmapped.is_empty());
    std::mem::forget(view);
}

#[tokio::test]
async fn a_failed_stop_holds_the_next_episodes_start() {
    let (core, fake) = signed_in("a-order").await;
    fake.queue("POST /scrobble/stop", vec![down(), down()]);
    let one = PlaybackView::open(&core, Vec::new(), episode(1));
    one.report_progress(at(90), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    drop(one);
    let two = PlaybackView::open(&core, Vec::new(), episode(2));
    two.report_progress(at(5), MINUTES, false);
    fake.wait_within("the stop's third try, then the start", 60, |f| {
        f.scrobbles().len() == 5
    })
    .await;
    let sent: Vec<(String, u64)> = fake
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.path.starts_with("/scrobble/"))
        .map(|r| {
            let tvdb = r.body["episode"]["ids"]["tvdb"].as_u64().unwrap();
            (r.path[10..].to_owned(), tvdb)
        })
        .collect();
    let expected = [
        ("start", 1001),
        ("stop", 1001),
        ("stop", 1001),
        ("stop", 1001),
        ("start", 1002),
    ];
    assert_eq!(sent, expected.map(|(a, t)| (a.to_owned(), t)));
    std::mem::forget(two);
}

/// A data directory of its own, removed at the end.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("mq-trakt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn core(&self) -> Core {
        Core::open(mq_runtime::CoreConfig {
            data_dir: self.0.clone(),
            profile_id: "default".into(),
            device_id: 7,
        })
        .unwrap()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn config(base: &str) -> TraktConfig {
    TraktConfig {
        client_id: "client".into(),
        redirect_uri: "https://marquee.example/trakt".into(),
        hosts: TraktHosts {
            api: base.into(),
            auth: base.into(),
            local: true,
        },
    }
}

#[tokio::test]
async fn the_end_mark_and_its_held_write_commit_together() {
    let fake = Fake::new("a-atomic");
    let base = fake.start().await;
    let dir = Dir::new("atomic");
    let core = dir.core();
    core.set_trakt_config(Some(config(&base)));
    core.install_addon(&format!("{base}/manifest.json"))
        .await
        .unwrap();
    core.set_trakt(Some(record("a-atomic"))).await.unwrap();
    // The held write cannot be stored: the mark must not be either.
    let db = rusqlite::Connection::open(dir.0.join("library.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER no_outbox BEFORE INSERT ON tracker_outbox BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    )
    .unwrap();
    let view = PlaybackView::open(&core, Vec::new(), episode(2));
    view.report_progress(at(95), MINUTES, false);
    assert!(
        !watched_in(&core, "tt100:1:2"),
        "no mark without the write that carries it to Trakt"
    );
    db.execute_batch("DROP TRIGGER no_outbox;").unwrap();
    std::mem::forget(view);
}

#[tokio::test]
async fn a_sign_out_during_a_sign_in_stays_signed_out() {
    let (core, fake) = setup("a-login-out").await;
    fake.queue(
        "POST /oauth/device/code",
        vec![reply(
            200,
            json!({"device_code": "dev", "user_code": "ABCD1234",
                   "verification_url": "https://trakt.tv/activate", "expires_in": 600, "interval": 1}),
        )],
    );
    let tokens = json!({"access_token": "at", "refresh_token": "rt", "expires_in": 604_800,
                        "created_at": now() / 1000, "scope": "public"});
    fake.queue("POST /oauth/device/token", vec![reply(200, tokens)]);
    // The user lookup answers late: the sign-out happens meanwhile.
    let user =
        json!({"user": {"username": "tester", "ids": {"slug": "tester", "uuid": "a-login-out"}}});
    fake.queue(
        "GET /users/settings",
        vec![Reply {
            status: 200,
            body: user.to_string(),
            delay: Duration::from_millis(1500),
            headers: String::new(),
        }],
    );
    let login = TraktLoginView::open(&core);
    fake.wait_for("the lookup", |f| {
        !f.seen("GET", "/users/settings").is_empty()
    })
    .await;
    core.trakt_sign_out().await.unwrap();
    until("the sign-in's end", || {
        matches!(login.snapshot(), TraktLoginSnapshot::Failed { .. })
    })
    .await;
    let after = status(&core);
    assert_eq!(after.state, TraktState::Off, "{after:?}");
    assert_eq!(after.record, None, "nothing adopted");
}

#[tokio::test]
async fn a_newer_sign_in_overtakes_an_older_one() {
    let (core, fake) = setup("a-login-new").await;
    let code = |c: &str| {
        reply(
            200,
            json!({"device_code": c, "user_code": c, "verification_url": "https://trakt.tv/activate",
                   "expires_in": 600, "interval": 1}),
        )
    };
    fake.queue(
        "POST /oauth/device/code",
        vec![code("dev-a"), code("dev-b")],
    );
    let tokens = json!({"access_token": "a-tok", "refresh_token": "rt", "expires_in": 604_800,
                        "created_at": now() / 1000, "scope": "public"});
    // The older code is approved, but only after the newer sign-in began.
    fake.queue(
        "POST /oauth/device/token#dev-a",
        vec![Reply {
            status: 200,
            body: tokens.to_string(),
            delay: Duration::from_millis(1500),
            headers: String::new(),
        }],
    );
    fake.queue(
        "POST /oauth/device/token#dev-b",
        (0..20).map(|_| reply(400, json!({}))).collect(),
    );
    let older = TraktLoginView::open(&core);
    fake.wait_for("the older poll", |f| {
        f.seen("POST", "/oauth/device/token")
            .iter()
            .any(|r| r.body["code"] == "dev-a")
    })
    .await;
    let newer = TraktLoginView::open(&core);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        status(&core).record,
        None,
        "the overtaken sign-in adopted nothing"
    );
    drop((older, newer));
}

#[tokio::test]
async fn an_answer_that_counts_nothing_keeps_the_writes() {
    let (core, fake) = signed_in("a-ack").await;
    fake.queue("POST /sync/history", vec![reply(201, json!({}))]);
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:1", true).unwrap();
    core.trakt_sync().await.unwrap();
    assert_eq!(history_writes(&fake).len(), 1);
    assert_eq!(
        pending(&core, "a-ack"),
        1,
        "an empty answer acknowledges nothing"
    );
    assert_eq!(
        status(&core).last_error.as_deref(),
        Some("Trakt did not answer: Trakt's answer did not acknowledge the writes")
    );
    due(&core, "a-ack");
    core.trakt_sync().await.unwrap();
    assert_eq!(pending(&core, "a-ack"), 0);
}

#[tokio::test]
async fn dropping_every_handle_ends_the_trakt_work() {
    let fake = Fake::new("a-drop");
    let base = fake.start().await;
    let dir = Dir::new("drop");
    {
        let core = dir.core();
        core.set_trakt_config(Some(config(&base)));
        core.set_trakt(Some(record("a-drop"))).await.unwrap();
        core.trakt_sync().await.unwrap();
        fake.wait_for("the scheduler's round", |f| {
            f.seen("GET", "/sync/last_activities").len() >= 2
        })
        .await;
        core.trakt_sync().await.unwrap();
        // A movie: no meta addon is installed here.
        let movie = PlayTarget {
            kind: "movie".into(),
            item_id: MOVIE.into(),
            video_id: MOVIE.into(),
            name: Some("Film".into()),
            poster: None,
            season: None,
            episode: None,
        };
        let view = PlaybackView::open(&core, Vec::new(), movie);
        view.report_progress(at(10), MINUTES, false);
        fake.wait_for("the start", |f| f.scrobbles().len() == 1)
            .await;
        drop(view);
        fake.wait_for("the stop", |f| f.scrobbles().len() == 2)
            .await;
    }
    // The work in flight ends; nothing holds the runtime after it.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    // Opened again, the runtime is a new one: the old tokens left memory.
    let again = dir.core();
    again.set_trakt_config(Some(config(&base)));
    let state = TraktView::open(&again).snapshot().state;
    assert_eq!(
        state,
        TraktState::ThisDeviceOff,
        "no record held: {state:?}"
    );
}

#[tokio::test]
async fn deleting_a_profile_ends_its_trakt_work() {
    let (core, fake) = signed_in("a-owner").await;
    let owner = core.for_profile("default");
    let kid = owner.create_profile("Kid", "fox").unwrap();
    let kid_core = core.for_profile(&kid);
    kid_core
        .set_trakt(Some(record_as("a-kid", "kid-access", 1)))
        .await
        .unwrap();
    fake.wait_for("the kid's round", |f| {
        f.seen("GET", "/sync/last_activities")
            .iter()
            .any(|r| r.token == "kid-access")
    })
    .await;
    kid_core.trakt_sync().await.unwrap();
    owner.delete_profile(&kid).unwrap();
    let deleted = std::time::Instant::now();
    core.trakt_foreground();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let late = fake
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.token == "kid-access" && r.at > deleted)
        .count();
    assert_eq!(late, 0, "no call for a deleted profile");
    let state = TraktView::open(&kid_core).snapshot().state;
    assert_ne!(state, TraktState::Connected, "its tokens left memory");
}

#[tokio::test]
async fn writes_go_out_when_the_pull_fails() {
    let (core, fake) = signed_in("a-drain").await;
    fake.queue("GET /sync/playback", vec![reply(400, json!({}))]);
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:1", true).unwrap();
    assert!(core.trakt_sync().await.is_err(), "the pull failed");
    assert_eq!(
        history_writes(&fake),
        [("/sync/history".to_owned(), vec![1001])]
    );
    assert_eq!(pending(&core, "a-drain"), 0);
}

#[tokio::test]
async fn a_slow_history_write_never_holds_a_pause() {
    let (core, fake) = signed_in("a-lane").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(20), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:3", true).unwrap();
    let slow =
        json!({"added": {"movies": 0, "episodes": 1}, "not_found": {"movies": [], "episodes": []}});
    fake.queue(
        "POST /sync/history",
        vec![Reply {
            status: 201,
            body: slow.to_string(),
            delay: Duration::from_secs(4),
            headers: String::new(),
        }],
    );
    let sync = {
        let core = core.clone();
        tokio::spawn(async move { core.trakt_sync().await })
    };
    fake.wait_for("the write", |f| !f.seen("POST", "/sync/history").is_empty())
        .await;
    view.report_progress(at(21), MINUTES, true);
    fake.wait_for("the pause", |f| f.scrobbles().len() == 2)
        .await;
    let post = fake.seen("POST", "/sync/history")[0].at;
    let pause = fake.seen("POST", "/scrobble/pause")[0].at;
    assert!(
        pause.duration_since(post) < Duration::from_secs(3),
        "the pause went while the write waited: {:?}",
        pause.duration_since(post)
    );
    sync.await.unwrap().unwrap();
    std::mem::forget(view);
}

#[tokio::test]
async fn scrobbles_go_before_outbox_writes() {
    let (core, fake) = signed_in("a-first").await;
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    view.report_progress(at(90), MINUTES, false);
    fake.wait_for("start", |f| f.scrobbles().len() == 1).await;
    let answer = json!({"action": "scrobble", "progress": 90.0,
                        "episode": {"ids": {"tvdb": 1001}}, "show": {"ids": {"imdb": SHOW}}});
    fake.queue(
        "POST /scrobble/stop",
        vec![Reply {
            status: 201,
            body: answer.to_string(),
            delay: Duration::from_secs(3),
            headers: String::new(),
        }],
    );
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:3", true).unwrap();
    drop(view);
    fake.wait_for("the stop", |f| f.scrobbles().len() == 2)
        .await;
    core.trakt_sync().await.unwrap();
    let stop = fake.seen("POST", "/scrobble/stop")[0].at;
    let post = fake.seen("POST", "/sync/history")[0].at;
    assert!(
        post.duration_since(stop) >= Duration::from_millis(2900),
        "the write waited for the stop's answer: {:?}",
        post.duration_since(stop)
    );
}

#[tokio::test]
async fn a_429_waits_its_retry_after() {
    let (core, fake) = signed_in("a-429").await;
    fake.queue(
        "POST /scrobble/start",
        vec![Reply {
            status: 429,
            body: "{}".into(),
            delay: Duration::ZERO,
            headers: "Retry-After: 3\r\n".into(),
        }],
    );
    let view = PlaybackView::open(&core, Vec::new(), episode(1));
    for p in 10..22 {
        view.report_progress(at(p), MINUTES, false);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(actions(&fake), ["start"], "nothing before Retry-After");
    fake.wait_for("the start again", |f| f.scrobbles().len() == 2)
        .await;
    let starts = fake.seen("POST", "/scrobble/start");
    assert!(starts[1].at.duration_since(starts[0].at) >= Duration::from_millis(2900));
    std::mem::forget(view);
}

#[tokio::test]
async fn a_wrong_show_is_left_out_of_hand_writes_too() {
    let (core, fake) = signed_in("a-sticky").await;
    let sticky = json!({"name": "Show", "reason": "Trakt matched it to another title",
                        "at": 1, "sticky": true});
    core.library()
        .set_tracker_state(key("a-sticky"), "unmapped:tt100", &sticky.to_string())
        .unwrap();
    let title = TitleView::open(&core, "series", SHOW);
    title.set_watched("tt100:1:1", true).unwrap();
    core.trakt_sync().await.unwrap();
    assert!(
        history_writes(&fake).is_empty(),
        "nothing to the wrong show"
    );
    assert_eq!(pending(&core, "a-sticky"), 0);
    // A later miss of the show does not replace the reason.
    let view = PlaybackView::open(&core, Vec::new(), episode(4));
    view.report_progress(at(50), MINUTES, false);
    until("unmapped", || {
        view.snapshot().trakt == TraktPlayback::Unmapped
    })
    .await;
    let unmapped = status(&core).unmapped;
    assert_eq!(unmapped.len(), 1);
    assert_eq!(unmapped[0].reason, "Trakt matched it to another title");
    std::mem::forget(view);
}

#[tokio::test]
async fn a_row_at_the_last_mark_does_not_hide_a_backdated_play() {
    let (core, fake) = signed_in("a-edge").await;
    // The row that set the last mark is read again with its day.
    fake.list("/sync/history", vec![episode_row(1, 1_767_225_600_000)]);
    *fake.activities.lock().unwrap() = activities(&iso(now()));
    fake.list(
        "/sync/watched/shows",
        vec![json!({"last_updated_at": iso(now()),
                    "show": {"title": "Show", "ids": {"trakt": 77, "imdb": SHOW}}})],
    );
    fake.list(
        "/sync/history/shows/77",
        vec![episode_row(2, 1_000_000_000_000)],
    );
    core.trakt_sync().await.unwrap();
    assert!(
        watched(&core, "tt100:1:2"),
        "the backdated play was reconciled"
    );
}

#[tokio::test]
async fn own_sends_are_forgotten_after_a_day() {
    let (core, _fake) = signed_in("a-prune").await;
    let lib = core.library();
    let sent = |at: i64| json!({"percent": 50.0, "at": at}).to_string();
    lib.set_tracker_state(
        key("a-prune"),
        "sent:tt100:1:1",
        &sent(now() - 2 * 86_400_000),
    )
    .unwrap();
    lib.set_tracker_state(key("a-prune"), "sent:tt100:1:2", &sent(now()))
        .unwrap();
    core.trakt_sync().await.unwrap();
    assert_eq!(state(&core, "a-prune", "sent:tt100:1:1"), None);
    assert!(state(&core, "a-prune", "sent:tt100:1:2").is_some());
}

#[tokio::test]
async fn a_profile_deleted_elsewhere_stops_its_rounds() {
    let (core, fake) = signed_in("a-owner2").await;
    let owner = core.for_profile("default");
    let kid = owner.create_profile("Kid", "fox").unwrap();
    let kid_core = core.for_profile(&kid);
    kid_core
        .set_trakt(Some(record_as("a-kid2", "kid2-access", 1)))
        .await
        .unwrap();
    fake.wait_for("the kid's round", |f| {
        f.seen("GET", "/sync/last_activities")
            .iter()
            .any(|r| r.token == "kid2-access")
    })
    .await;
    kid_core.trakt_sync().await.unwrap();
    // As sync brings a removal from another device: the library alone.
    core.library().delete_profile(&kid, core.clock()).unwrap();
    let deleted = std::time::Instant::now();
    core.trakt_foreground();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let late = fake
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r.token == "kid2-access" && r.at > deleted)
        .count();
    assert_eq!(late, 0, "no call for a profile removed elsewhere");
}
