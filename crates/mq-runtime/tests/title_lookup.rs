//! The title page's TMDB and Trakt lookups (plans/title-page-ratings-genres-similar.md):
//! asked only once the page has been visible a moment, TMDB and Trakt
//! together for a movie or a series, an anime by the kind TMDB gives its
//! IMDb id; each source's slot pending while an answer can come and none
//! when no call will be made; "More like this" from Trakt's related titles;
//! answers kept, a 429 quieting every Trakt call; a kids profile asks
//! nothing.

use mq_library::{AddonRow, AllowedRow};
use mq_runtime::views::title::{CastMember, Slot, TitleSnapshot, TitleState};
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

const MOVIE: &str = r#"{"meta":{"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","links":[{"name":"9.3","category":"imdb","url":"https://imdb.com/title/tt0111161"},{"name":"Drama","category":"Genres","url":"stremio:///discover"},{"name":"Frank Darabont","category":"Writers","url":"stremio:///search?search=Frank%20Darabont"},{"name":"Tim Robbins","category":"Cast","url":"stremio:///search?search=Tim%20Robbins"},{"name":"Morgan Freeman","category":"Cast","url":"stremio:///search?search=Morgan%20Freeman"},{"name":"Bob Gunton","category":"Cast","url":"stremio:///search?search=Bob%20Gunton"}]}}"#;
/// TMDB's credits of 2026-10-30, trimmed: the addon's names in another
/// case, one with no headshot, one the addon does not name.
const CREDITS: &str = r#"{"id":278,"cast":[{"name":"tim robbins","character":"Andy Dufresne","profile_path":"/robbins.jpg"},{"name":"Morgan Freeman","character":"Ellis Boyd 'Red' Redding","profile_path":null},{"name":"William Sadler","character":"Heywood","profile_path":"/sadler.jpg"}]}"#;
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
        ("/3/movie/278/credits".into(), 200, CREDITS.into()),
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
            "idPrefixes": ["tt", "kitsu", "tmdb"], "catalogs": []
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
        scores(s) == (shown("87%"), shown("92%"))
            && s.similar.len() == 2
            && cast(s).first().is_some_and(|m| m.photo.is_some())
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
    assert_eq!(lookups(&fake), 4, "answers are kept, the credits too");
}

fn cast(snap: &TitleSnapshot) -> Vec<CastMember> {
    snap.header
        .details
        .as_ref()
        .map(|d| d.cast.clone())
        .unwrap_or_default()
}

fn member(name: &str, character: Option<&str>, photo: Option<&str>) -> CastMember {
    CastMember {
        name: name.into(),
        character: character.map(str::to_owned),
        photo: photo.map(|p| format!("https://image.tmdb.org/t/p/w185{p}")),
    }
}

/// Starring's headshots (plans/batch-2.md R5): TMDB's credits put a
/// headshot and a role on each name the addon gives, matched by name in any
/// case, in the addon's order; a name TMDB does not list stays bare (the
/// apps draw its initials). Asked once with the scores and kept.
#[tokio::test]
async fn the_cast_gets_tmdbs_headshots_and_roles() {
    let fake = fake().await;
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "movie", "tt0111161");
    let snap = until(&view, "the meta", ready).await;
    assert_eq!(
        cast(&snap),
        [
            member("Tim Robbins", None, None),
            member("Morgan Freeman", None, None),
            member("Bob Gunton", None, None),
        ],
        "names only before the page is visible"
    );
    view.set_visible(true);
    let snap = until(&view, "the headshots", |s| {
        cast(s).iter().any(|m| m.photo.is_some())
    })
    .await;
    assert_eq!(
        cast(&snap),
        [
            member("Tim Robbins", Some("Andy Dufresne"), Some("/robbins.jpg")),
            member("Morgan Freeman", Some("Ellis Boyd 'Red' Redding"), None),
            member("Bob Gunton", None, None),
        ]
    );
    let again = TitleView::open(&core, "movie", "tt0111161");
    again.set_visible(true);
    until(&again, "the kept headshots", |s| {
        cast(s).first().is_some_and(|m| m.photo.is_some())
    })
    .await;
    assert_eq!(fake.asked("/3/movie/278/credits"), 1);
}

/// A show whose addon names no cast shows TMDB's first ten, from its
/// credits across the seasons.
#[tokio::test]
async fn a_title_with_no_cast_from_its_addon_shows_tmdbs() {
    let fake = fake().await;
    let people: Vec<String> = (1..=12)
        .map(|i| format!(r#"{{"name":"Actor {i}","roles":[{{"character":"Role {i}"}}],"profile_path":"/a{i}.jpg"}}"#))
        .collect();
    fake.route(
        "/3/tv/105248/aggregate_credits",
        200,
        &format!(r#"{{"cast":[{}]}}"#, people.join(",")),
    );
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "series", "tt0903747");
    until(&view, "the meta", ready).await;
    view.set_visible(true);
    let snap = until(&view, "TMDB's cast", |s| !cast(s).is_empty()).await;
    let shown = cast(&snap);
    assert_eq!(shown.len(), 10);
    assert_eq!(shown[0], member("Actor 1", Some("Role 1"), Some("/a1.jpg")));
    assert_eq!(shown[9].name, "Actor 10");
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

/// Hidden then visible in the same tick (the player pushed and popped, two
/// commands in one read): the lookup starts again and the scores land.
#[tokio::test]
async fn hidden_then_visible_at_once_starts_again() {
    let fake = fake().await;
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "movie", "tt0111161");
    until(&view, "the meta", ready).await;
    view.set_visible(true);
    view.set_visible(false);
    view.set_visible(true);
    until(&view, "the scores", |s| {
        scores(s) == (shown("87%"), shown("92%"))
    })
    .await;
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

/// A token TMDB refuses (revoked) is refused for every title: after one
/// 401 the next title page asks TMDB nothing, and Trakt still answers.
#[tokio::test]
async fn a_refused_tmdb_token_quiets_tmdb_for_every_title() {
    let fake = fake().await;
    fake.route("/3/find/", 401, r#"{"status_code":7}"#);
    let core = core(&fake, Some("revoked"));
    let movie = TitleView::open(&core, "movie", "tt0111161");
    until(&movie, "the meta", ready).await;
    movie.set_visible(true);
    until(&movie, "the scores", |s| {
        scores(s) == (Slot::Empty, shown("92%"))
    })
    .await;
    let series = TitleView::open(&core, "series", "tt0903747");
    until(&series, "the meta", ready).await;
    series.set_visible(true);
    until(&series, "TMDB none", |s| scores(s).0 == Slot::Empty).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.asked("/3/find/"), 1, "one call for the revoked token");
}

/// TMDB's badge follows the page's kind: a series whose IMDb id TMDB also
/// lists as a movie takes the show's score and page.
#[tokio::test]
async fn tmdb_answers_for_the_pages_kind() {
    let fake = fake().await;
    fake.route(
        "/3/find/tt0903747",
        200,
        r#"{"movie_results":[{"id":1,"vote_average":5.0,"vote_count":900}],"tv_results":[{"id":1396,"vote_average":8.9,"vote_count":18925}]}"#,
    );
    let core = core(&fake, Some("test-token"));
    let view = TitleView::open(&core, "series", "tt0903747");
    until(&view, "the meta", ready).await;
    view.set_visible(true);
    let snap = until(&view, "TMDB", |s| scores(s).0 == shown("89%")).await;
    assert_eq!(
        links(&snap)[1].as_deref(),
        Some("https://www.themoviedb.org/tv/1396")
    );
}

/// A title opened by another id whose meta answers with an IMDb id: every
/// badge and lookup use that one id, so IMDb, TMDB and Trakt all show.
#[tokio::test]
async fn a_title_opened_by_another_id_uses_its_metas_imdb_id() {
    let fake = fake().await;
    fake.route("/meta/movie/tmdb:278", 200, MOVIE);
    let core = core(&fake, Some("test-token"));
    // An addon configured with RPDB's key, so the page has a rated cover.
    let mut keyed = addon(&fake.base);
    keyed.transport_url = format!(
        "{}/%7B%22rpdb_key%22%3A%22t0-free-rpdb%22%7D/manifest.json",
        fake.base
    );
    keyed.position = 1;
    core.library()
        .set_addons("p", &[addon(&fake.base), keyed], &core.hlc())
        .unwrap();
    let view = TitleView::open(&core, "movie", "tmdb:278");
    let snap = until(&view, "the meta", ready).await;
    assert_eq!(scores(&snap), (Slot::Pending, Slot::Pending));
    // The cover is the meta's IMDb id's, as the badges are.
    assert_eq!(
        snap.header
            .details
            .as_ref()
            .unwrap()
            .rated_poster
            .as_deref(),
        Some(
            "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0111161.jpg?fallback=true"
        )
    );
    view.set_visible(true);
    let snap = until(&view, "the scores", |s| {
        scores(s) == (shown("87%"), shown("92%"))
    })
    .await;
    assert_eq!(
        links(&snap),
        [
            Some("https://www.imdb.com/title/tt0111161/".into()),
            Some("https://www.themoviedb.org/movie/278".into()),
            Some("https://trakt.tv/movies/tt0111161".into()),
        ]
    );
}

/// Collects what the log writes, for `the_tmdb_token_never_reaches_a_log_line`.
#[derive(Clone)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The TMDB token never reaches a log line, whatever TMDB answers (a
/// failure, a refusal), with every level logged.
#[tokio::test]
async fn the_tmdb_token_never_reaches_a_log_line() {
    let lines = Sink(Arc::new(Mutex::new(Vec::new())));
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    // The process's subscriber, not this thread's: other tests' threads run
    // at the same time, and a call site one of them registers with no
    // subscriber would stay silent here. Only this test sets one; their
    // lines carry other tokens.
    tracing::subscriber::set_global_default(subscriber).expect("one subscriber");
    let token = "secret-tmdb-token-4f2a";
    let fake = fake().await;
    fake.route("/3/find/tt0111161", 500, "{}");
    fake.route("/3/find/tt0903747", 401, r#"{"status_code":7}"#);
    let core = core(&fake, Some(token));
    for (kind, id) in [("movie", "tt0111161"), ("series", "tt0903747")] {
        let view = TitleView::open(&core, kind, id);
        until(&view, "the meta", ready).await;
        view.set_visible(true);
        until(&view, "TMDB none", |s| scores(s).0 == Slot::Empty).await;
    }
    assert_eq!(fake.asked("/3/find/"), 2);
    let log = String::from_utf8(lines.0.lock().unwrap().clone()).unwrap();
    assert!(!log.is_empty(), "nothing was logged");
    assert!(!log.contains(token), "the token is in the log");
}
