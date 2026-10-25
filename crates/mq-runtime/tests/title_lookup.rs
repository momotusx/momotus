//! The title page's TMDB and Trakt lookups (plans/title-page-ratings-genres-similar.md):
//! asked only once the page has been visible a moment, TMDB and Trakt
//! together for a movie or a series, an anime by the kind TMDB gives its
//! IMDb id; each source's slot pending while an answer can come and none
//! when no call will be made; "More like this" from Trakt's related titles;
//! answers kept, a 429 quieting every Trakt call; a kids profile asks
//! nothing.

use mq_library::{AddonRow, AllowedRow};
use mq_runtime::views::title::{Slot, TitleSnapshot, TitleState};
use mq_runtime::{Core, SearchView, TitleView, TmdbHost, TraktConfig, TraktHosts};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A route: a path prefix and its answer.
type Routes = Arc<Mutex<Vec<(String, u16, String)>>>;

struct Fake {
    base: String,
    routes: Routes,
    asked: Arc<Mutex<Vec<String>>>,
    /// How long TMDB and Trakt hold each answer, in ms.
    hold: Arc<AtomicU64>,
}

impl Fake {
    fn route(&self, prefix: &str, status: u16, body: &str) {
        let mut routes = self.routes.lock().unwrap();
        routes.retain(|(p, _, _)| p != prefix);
        routes.insert(0, (prefix.into(), status, body.into()));
    }

    fn hold(&self, ms: u64) {
        self.hold.store(ms, Ordering::SeqCst);
    }

    fn asked(&self, prefix: &str) -> usize {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.starts_with(prefix))
            .count()
    }
}

const MOVIE: &str = r#"{"meta":{"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","links":[{"name":"9.3","category":"imdb","url":"https://imdb.com/title/tt0111161"},{"name":"Drama","category":"Genres","url":"stremio:///discover"},{"name":"Frank Darabont","category":"Writers","url":"stremio:///search?search=Frank%20Darabont"}]}}"#;
const SERIES: &str =
    r#"{"meta":{"id":"tt0903747","type":"series","name":"Breaking Bad","videos":[]}}"#;
const ANIME: &str = r#"{"meta":{"id":"kitsu:50016","type":"anime","name":"Cyberpunk: Edgerunners II","imdb_id":"tt12590266","videos":[]}}"#;
const TMDB_MOVIE: &str =
    r#"{"movie_results":[{"id":278,"vote_average":8.733,"vote_count":31655}],"tv_results":[]}"#;
const TMDB_SHOW: &str =
    r#"{"movie_results":[],"tv_results":[{"id":105248,"vote_average":8.5,"vote_count":2079}]}"#;
const TRAKT_SCORE: &str = r#"{"rating":9.16432,"votes":48623,"distribution":{}}"#;
const RELATED: &str = r#"[
  {"title":"Cool Hand Luke","year":1967,"ids":{"imdb":"tt0061512"}},
  {"title":"The Shawshank Redemption","ids":{"imdb":"tt0111161"}},
  {"title":"No Id","ids":{"imdb":null}},
  {"title":"Bad Id","ids":{"imdb":"tt1/../x"}},
  {"title":"Cool Hand Luke","ids":{"imdb":"tt0061512"}},
  {"title":"Papillon","ids":{"imdb":"tt0070511"}}
]"#;

/// One local host for the meta addon, TMDB and Trakt, recording every path.
async fn fake() -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes: Routes = Arc::new(Mutex::new(vec![
        ("/meta/movie/tt0111161".into(), 200, MOVIE.into()),
        ("/meta/series/tt0903747".into(), 200, SERIES.into()),
        ("/meta/anime/kitsu:50016".into(), 200, ANIME.into()),
        ("/3/find/tt0111161".into(), 200, TMDB_MOVIE.into()),
        ("/3/find/tt0903747".into(), 200, TMDB_SHOW.into()),
        ("/3/find/tt12590266".into(), 200, TMDB_SHOW.into()),
        ("/movies/tt0111161/ratings".into(), 200, TRAKT_SCORE.into()),
        ("/movies/tt0111161/related".into(), 200, RELATED.into()),
        ("/shows/".into(), 200, "[]".into()),
        ("/search/".into(), 200, "[]".into()),
    ]));
    let asked = Arc::new(Mutex::new(Vec::new()));
    let hold = Arc::new(AtomicU64::new(0));
    let (r, a, h) = (routes.clone(), asked.clone(), hold.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let (routes, asked, hold) = (r.clone(), a.clone(), h.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
                let path = percent_encoding::percent_decode_str(&path)
                    .decode_utf8_lossy()
                    .into_owned();
                asked.lock().unwrap().push(path.clone());
                let (status, body) = routes
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(p, _, _)| path.starts_with(p.as_str()))
                    .map(|(_, s, b)| (*s, b.clone()))
                    .unwrap_or((404, "{}".into()));
                if !path.starts_with("/meta/") {
                    let ms = hold.load(Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                }
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    Fake {
        base,
        routes,
        asked,
        hold,
    }
}

fn addon(base: &str) -> AddonRow {
    AddonRow {
        transport_url: format!("{base}/manifest.json"),
        manifest_json: serde_json::json!({
            "id": "meta", "version": "1.0.0", "name": "Meta",
            "types": ["movie", "series", "anime"], "resources": ["meta"],
            "idPrefixes": ["tt", "kitsu"], "catalogs": []
        })
        .to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

/// A core whose meta addon, TMDB and Trakt are the fake.
fn core(fake: &Fake, tmdb_token: Option<&str>) -> Core {
    core_as(fake, tmdb_token, "p")
}

fn core_as(fake: &Fake, tmdb_token: Option<&str>, profile: &str) -> Core {
    let core = Core::temporary(profile).unwrap();
    core.library()
        .set_addons(profile, &[addon(&fake.base)], &core.hlc())
        .unwrap();
    core.set_trakt_config(Some(TraktConfig {
        client_id: "client".into(),
        redirect_uri: "https://marquee.example/trakt".into(),
        hosts: TraktHosts {
            api: fake.base.clone(),
            auth: fake.base.clone(),
            local: true,
        },
    }));
    core.set_tmdb_host(TmdbHost {
        url: fake.base.clone(),
        token: tmdb_token.map(str::to_owned),
        local: true,
    });
    core
}

async fn until(
    view: &TitleView,
    what: &str,
    done: impl Fn(&TitleSnapshot) -> bool,
) -> TitleSnapshot {
    for _ in 0..400 {
        let snap = view.snapshot();
        if done(&snap) {
            return snap;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "{what}: {:?}",
        view.snapshot().header.details.map(|d| d.scores)
    );
}

fn ready(snap: &TitleSnapshot) -> bool {
    snap.header.state == TitleState::Ready && snap.header.details.is_some()
}

fn shown(text: &str) -> Slot {
    Slot::Shown {
        text: text.into(),
        url: None,
    }
}

/// The two slots, links left out (`links` reads them); both pending
/// before the meta is in.
fn scores(snap: &TitleSnapshot) -> (Slot, Slot) {
    let plain = |slot: &Slot| match slot {
        Slot::Shown { text, .. } => shown(text),
        other => other.clone(),
    };
    match &snap.header.details {
        Some(d) => (plain(&d.scores.tmdb), plain(&d.scores.trakt)),
        None => (Slot::Pending, Slot::Pending),
    }
}

/// The IMDb, TMDB and Trakt links the page carries.
fn links(snap: &TitleSnapshot) -> [Option<String>; 3] {
    let url = |slot: &Slot| match slot {
        Slot::Shown { url, .. } => url.clone(),
        _ => None,
    };
    let d = snap.header.details.as_ref().unwrap();
    [
        d.imdb_url.clone(),
        url(&d.scores.tmdb),
        url(&d.scores.trakt),
    ]
}

fn lookups(fake: &Fake) -> usize {
    fake.asked("/3/") + fake.asked("/movies/") + fake.asked("/shows/")
}

/// Nothing is asked before the page is visible; once it has been visible a
/// moment, TMDB's and Trakt's numbers land as percentages and "More like
/// this" lists Trakt's related titles once each, never the title itself and
/// never one with no IMDb id, with Stremio's public poster when there is no
/// RPDB key.
#[tokio::test]
async fn scores_and_similar_land_once_the_page_is_visible() {
    let fake = fake().await;
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "movie", "tt0111161");
    let snap = until(&view, "the meta", ready).await;
    assert_eq!(scores(&snap), (Slot::Pending, Slot::Pending));
    assert!(snap.header.genres_linked);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(lookups(&fake), 0, "nothing before the page is visible");

    view.set_visible(true);
    let snap = until(&view, "the scores", |s| {
        scores(s) == (shown("87%"), shown("92%")) && s.similar.len() == 2
    })
    .await;
    // Each badge links to the title's own page there, built from checked
    // ids; the names the meta links carry, writers too.
    assert_eq!(
        links(&snap),
        [
            Some("https://www.imdb.com/title/tt0111161/".into()),
            Some("https://www.themoviedb.org/movie/278".into()),
            Some("https://trakt.tv/movies/tt0111161".into()),
        ]
    );
    let details = snap.header.details.as_ref().unwrap();
    assert_eq!(details.writers, ["Frank Darabont"]);
    let similar: Vec<(&str, &str, Option<&str>)> = snap
        .similar
        .iter()
        .map(|t| (t.id.as_str(), t.name.as_str(), t.poster.as_deref()))
        .collect();
    assert_eq!(
        similar,
        [
            (
                "tt0061512",
                "Cool Hand Luke",
                Some("https://images.metahub.space/poster/medium/tt0061512/img")
            ),
            (
                "tt0070511",
                "Papillon",
                Some("https://images.metahub.space/poster/medium/tt0070511/img")
            ),
        ]
    );
    assert_eq!(fake.asked("/3/find/tt0111161"), 1);
    assert_eq!(fake.asked("/movies/tt0111161/ratings"), 1);
    assert_eq!(fake.asked("/movies/tt0111161/related?limit=20"), 1);

    // Back to the page, or the page again within the day: no second call.
    view.set_visible(false);
    view.set_visible(true);
    let again = TitleView::open(&core, "movie", "tt0111161");
    again.set_visible(true);
    let opened = std::time::Instant::now();
    until(&again, "the kept answers", |s| {
        scores(s) == (shown("87%"), shown("92%"))
    })
    .await;
    assert!(
        opened.elapsed() < Duration::from_millis(700),
        "kept answers show at once, with no wait: {:?}",
        opened.elapsed()
    );
    assert_eq!(lookups(&fake), 3, "answers are kept");
}

/// TMDB down or with no token never holds Trakt back: a movie still gets
/// Trakt's score and its similar row, and TMDB's slot is none (with no
/// token from the first render: no gap kept for it).
#[tokio::test]
async fn trakt_answers_without_tmdb() {
    let fake = fake().await;
    let no_token = core(&fake, None);
    let view = TitleView::open(&no_token, "movie", "tt0111161");
    let snap = until(&view, "the meta", ready).await;
    assert_eq!(scores(&snap).0, Slot::Empty, "no token: no slot kept");
    view.set_visible(true);
    let snap = until(&view, "Trakt", |s| {
        scores(s).1 == shown("92%") && !s.similar.is_empty()
    })
    .await;
    assert_eq!(scores(&snap).0, Slot::Empty);
    assert_eq!(fake.asked("/3/"), 0);

    let fake = self::fake().await;
    fake.route("/3/find/", 500, "{}");
    let down = core(&fake, Some("test-token"));
    let view = TitleView::open(&down, "movie", "tt0111161");
    view.set_visible(true);
    let snap = until(&view, "Trakt with TMDB down", |s| {
        ready(s) && scores(s) == (Slot::Empty, shown("92%")) && !s.similar.is_empty()
    })
    .await;
    assert_eq!(snap.similar.len(), 2);
}

/// An anime is asked of TMDB by the IMDb id its addon gave; TMDB's kind
/// (a show here) picks Trakt's shows path. With no TMDB token, its Trakt
/// slot is none from the first render and Trakt is never asked.
#[tokio::test]
async fn an_anime_asks_trakt_by_the_kind_tmdb_gives() {
    let fake = fake().await;
    fake.route(
        "/shows/tt12590266/ratings",
        200,
        r#"{"rating":8.7,"votes":900}"#,
    );
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "anime", "kitsu:50016");
    view.set_visible(true);
    until(&view, "the anime's scores", |s| {
        ready(s) && scores(s) == (shown("85%"), shown("87%"))
    })
    .await;
    assert_eq!(fake.asked("/3/find/tt12590266"), 1);
    assert_eq!(fake.asked("/shows/tt12590266/ratings"), 1);
    assert_eq!(fake.asked("/movies/tt12590266"), 0);

    let fake = self::fake().await;
    let no_token = self::core(&fake, None);
    let view = TitleView::open(&no_token, "anime", "kitsu:50016");
    let snap = until(&view, "the meta", ready).await;
    assert_eq!(scores(&snap), (Slot::Empty, Slot::Empty));
    view.set_visible(true);
    tokio::time::sleep(Duration::from_millis(1600)).await;
    assert_eq!(lookups(&fake), 0);
    assert!(view.snapshot().similar.is_empty());
}

/// Under 10 votes a number is noise: its slot is none.
#[tokio::test]
async fn a_number_with_few_votes_shows_nothing() {
    let fake = fake().await;
    fake.route(
        "/3/find/tt0111161",
        200,
        r#"{"movie_results":[{"vote_average":10.0,"vote_count":1}],"tv_results":[]}"#,
    );
    fake.route(
        "/movies/tt0111161/ratings",
        200,
        r#"{"rating":10.0,"votes":9}"#,
    );
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "movie", "tt0111161");
    view.set_visible(true);
    until(&view, "the empty slots", |s| {
        ready(s) && scores(s) == (Slot::Empty, Slot::Empty)
    })
    .await;
}

/// A page hidden before it was visible a moment asks nothing; the next
/// visible starts the lookup again.
#[tokio::test]
async fn a_glance_asks_nothing() {
    let fake = fake().await;
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "movie", "tt0111161");
    until(&view, "the meta", ready).await;
    view.set_visible(true);
    tokio::time::sleep(Duration::from_millis(400)).await;
    view.set_visible(false);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(lookups(&fake), 0);
    view.set_visible(true);
    until(&view, "the scores", |s| scores(s).1 == shown("92%")).await;
}

/// Hidden while TMDB and Trakt are still answering cancels the lookup and
/// keeps nothing against the title (no rest, no empty answer): back on the
/// page they are asked again and land.
#[tokio::test]
async fn hidden_mid_answer_keeps_nothing_and_visible_asks_again() {
    let fake = fake().await;
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "movie", "tt0111161");
    until(&view, "the meta", ready).await;
    fake.hold(800);
    view.set_visible(true);
    for _ in 0..300 {
        if lookups(&fake) == 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(lookups(&fake), 3, "TMDB and Trakt asked");
    view.set_visible(false);
    fake.hold(0);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        scores(&view.snapshot()),
        (Slot::Pending, Slot::Pending),
        "a cancelled answer is not shown"
    );
    view.set_visible(true);
    until(&view, "the scores", |s| {
        scores(s) == (shown("87%"), shown("92%"))
    })
    .await;
    assert_eq!(fake.asked("/3/find/tt0111161"), 2);
}

/// A repeated visible keeps the running wait: the lookup starts a second
/// after the first one, not after the last.
#[tokio::test]
async fn a_repeated_visible_keeps_the_wait() {
    let fake = fake().await;
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "movie", "tt0111161");
    until(&view, "the meta", ready).await;
    view.set_visible(true);
    tokio::time::sleep(Duration::from_millis(700)).await;
    view.set_visible(true);
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(lookups(&fake) > 0, "the second visible restarted the wait");
}

/// A 429 from Trakt's related titles quiets every Trakt call for at least
/// a minute: the next title's score and Search's Top results ask nothing.
/// A TMDB failure rests that title: opened again, TMDB is not asked.
#[tokio::test]
async fn a_429_quiets_trakt_and_a_failure_rests_the_title() {
    let fake = fake().await;
    fake.route("/movies/tt0111161/related", 429, "{}");
    fake.route("/3/find/tt0111161", 503, "{}");
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "movie", "tt0111161");
    view.set_visible(true);
    until(&view, "the 429", |_| {
        fake.asked("/movies/tt0111161/related") == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let series = TitleView::open(&core, "series", "tt0903747");
    series.set_visible(true);
    until(&series, "TMDB for the series", |s| {
        ready(s) && scores(s).0 == shown("85%")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.asked("/shows/"), 0, "Trakt is quiet");
    assert_eq!(scores(&series.snapshot()).1, Slot::Empty);

    let search = SearchView::open(&core);
    search.set_query("breaking bad");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        fake.asked("/search/"),
        0,
        "Search's Top results are quiet too"
    );

    let again = TitleView::open(&core, "movie", "tt0111161");
    again.set_visible(true);
    until(&again, "the meta", ready).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        fake.asked("/3/find/tt0111161"),
        1,
        "a failure rests the title"
    );
}

/// A kids profile has no Discover and asks TMDB and Trakt nothing: genres
/// are not links, both slots are none, and there is no similar row.
#[tokio::test]
async fn a_kids_profile_asks_nothing() {
    let fake = fake().await;
    let owner = core_as(&fake, Some("test-token"), mq_library::OWNER);
    owner
        .set_pin(mq_library::OWNER, Some("1357"), None)
        .await
        .unwrap();
    let id = owner.create_kids_profile("Kid", "fox").unwrap();
    let kid = owner.for_profile(&id);
    let row = AllowedRow {
        item_id: "tt0111161".into(),
        kind: "movie".into(),
        name: "The Shawshank Redemption".into(),
        names: vec![],
        poster: None,
        allowed: true,
        added_ms: 1,
    };
    kid.library()
        .write_allowed(kid.profile(), &row, &kid.hlc())
        .unwrap();
    let view = TitleView::open(&kid, "movie", "tt0111161");
    let snap = until(&view, "the meta", ready).await;
    assert!(!snap.header.genres_linked);
    assert_eq!(scores(&snap), (Slot::Empty, Slot::Empty));
    // No badge leads out of the app: IMDb's page is the open web.
    assert_eq!(links(&snap), [None, None, None]);
    view.set_visible(true);
    tokio::time::sleep(Duration::from_millis(1600)).await;
    assert_eq!(lookups(&fake), 0);
    assert!(view.snapshot().similar.is_empty());
}
