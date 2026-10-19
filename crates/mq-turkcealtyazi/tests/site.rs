//! The site client against a fixture origin that serves the recorded pages
//! and archives: requests, caches, and every kind of failure.

mod common;

use common::corpus;
use mq_http::test_dns::{self, Request};
use mq_http::{ClientConfig, HttpClient, Url};
use mq_turkcealtyazi::{
    Fault, Fetched, Kind, Limits, Listing, Playing, Problem, Row, Site, Target, Track,
};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HOST: &str = "turkcealtyazi.test";
const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const S01E01: Target = Target::Episode {
    season: 1,
    episode: 1,
};
type Route = Box<dyn Fn(&Request) -> Option<Vec<u8>> + Send + Sync>;

fn reply(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");
    let mut out = out.into_bytes();
    out.extend_from_slice(body);
    out
}

fn ok(body: &[u8]) -> Vec<u8> {
    reply(
        "200 OK",
        &[("Content-Type", "text/html; charset=UTF-8")],
        body,
    )
}

fn moved(to: &str) -> Vec<u8> {
    reply("301 Moved Permanently", &[("Location", to)], b"")
}

/// A fixture site: routes tried in order (the last added first), every
/// request recorded.
struct Fake {
    port: u16,
    seen: Arc<Mutex<Vec<Request>>>,
    routes: Arc<Mutex<Vec<Route>>>,
}

impl Fake {
    fn new() -> Self {
        test_dns::set_public(HOST, &[LOOPBACK]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let routes: Arc<Mutex<Vec<Route>>> = Arc::new(Mutex::new(Vec::new()));
        let (s, r) = (seen.clone(), routes.clone());
        let port = test_dns::https_requests(move |request| {
            s.lock().unwrap().push(request.clone());
            let routes = r.lock().unwrap();
            routes
                .iter()
                .rev()
                .find_map(|route| route(request))
                .unwrap_or_else(|| reply("404 Not Found", &[], b"no route"))
        });
        let fake = Self { port, seen, routes };
        fake.title(
            "0903747",
            "breaking-badd",
            corpus_text("title-breaking-bad.html"),
        );
        fake.title("0133093", "the-matrix1", corpus_text("title-matrix.html"));
        fake.title("9999999", "404-error", corpus_text("title-404.html"));
        fake.subtitle(108735, "breaking-badd", "episode.zip");
        fake.subtitle(725317, "breaking-badd", "pack.zip");
        fake
    }

    fn route(&self, route: impl Fn(&Request) -> Option<Vec<u8>> + Send + Sync + 'static) {
        self.routes.lock().unwrap().push(Box::new(route));
    }

    /// `/mov/<digits>/x.html` redirecting to its page, as the site does.
    fn title(&self, digits: &str, slug: &str, html: String) {
        let (x, page) = (
            format!("/mov/{digits}/x.html"),
            format!("/mov/{digits}/{slug}.html"),
        );
        let to = page.clone();
        self.route(move |r| (r.path == x).then(|| moved(&to)));
        self.route(move |r| (r.path == page).then(|| ok(html.as_bytes())));
    }

    /// A subtitle page with the recorded form (for `altid`) and its archive.
    fn subtitle(&self, altid: u32, slug: &str, archive: &str) {
        let page = format!("/sub/{altid}/{slug}.html");
        let form = corpus_text("sub-form.html").replace("108735", &altid.to_string());
        self.route(move |r| (r.path == page).then(|| ok(form.as_bytes())));
        let zip = corpus(archive);
        let field = format!("altid={altid}");
        self.route(move |r| {
            (r.method == "POST"
                && r.path == "/ind"
                && String::from_utf8_lossy(&r.body).contains(&field))
            .then(|| reply("200 OK", &[("Content-Type", "application/zip")], &zip))
        });
    }

    fn origin(&self) -> String {
        format!("https://{HOST}:{}", self.port)
    }

    fn site(&self) -> Site {
        self.site_with(Limits {
            request: Duration::from_secs(2),
            fetch: Duration::from_secs(3),
            spacing: Duration::from_millis(5),
        })
    }

    fn site_with(&self, limits: Limits) -> Site {
        let http = HttpClient::new(ClientConfig::default()).unwrap();
        Site::with(http, Url::parse(&self.origin()).unwrap(), limits)
    }

    fn paths(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| format!("{} {}", r.method, r.path))
            .collect()
    }

    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

fn corpus_text(name: &str) -> String {
    String::from_utf8(corpus(name)).unwrap()
}

fn tracks(listing: Listing) -> Vec<Track> {
    match listing {
        Listing::Tracks(tracks) => tracks,
        other => panic!("{other:?}"),
    }
}

fn file(fetched: Fetched) -> Vec<u8> {
    match fetched {
        Fetched::File(bytes) => bytes,
        other => panic!("{other:?}"),
    }
}

/// Moves the clock on between requests (never while one is in flight).
async fn later(by: Duration) {
    tokio::time::pause();
    tokio::time::advance(by).await;
    tokio::time::resume();
}

fn row(rows: &[Track], altid: u32) -> Row {
    rows.iter()
        .find(|t| t.row.altid == altid)
        .unwrap()
        .row
        .clone()
}

#[tokio::test]
async fn a_title_costs_one_visit_then_comes_from_the_cache() {
    let fake = Fake::new();
    let site = fake.site();
    let got = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    assert_eq!(got.len(), 7);
    assert_eq!(
        fake.paths(),
        [
            "GET /mov/0903747/x.html",
            "GET /mov/0903747/breaking-badd.html"
        ]
    );
    let e02 = Target::Episode {
        season: 1,
        episode: 2,
    };
    assert!(!tracks(site.list("0903747", e02, &Playing::default()).await).is_empty());
    assert_eq!(fake.count(), 2, "the second episode from the cache");
    assert_eq!(
        site.list("9999999", S01E01, &Playing::default()).await,
        Listing::Nothing
    );
    let movie = tracks(
        site.list("0133093", Target::Movie, &Playing::default())
            .await,
    );
    assert_eq!(movie[0].row.altid, 152408);
}

#[tokio::test]
async fn a_played_file_is_one_form_and_one_post_with_its_origin_and_referer() {
    let fake = Fake::new();
    let site = fake.site();
    let listed = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    let bytes = file(site.fetch(&row(&listed, 108735), S01E01, None).await);
    assert!(bytes.starts_with(b"1\r\n00:02:06,573 --> "));
    let seen = fake.seen.lock().unwrap().clone();
    let post = seen.iter().find(|r| r.method == "POST").unwrap();
    assert_eq!(post.path, "/ind");
    let body = String::from_utf8_lossy(&post.body);
    assert_eq!(
        body,
        "idid=174736&altid=108735&sidid=0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f"
    );
    assert_eq!(post.header("origin"), Some(fake.origin().as_str()));
    let referer = format!("{}/sub/108735/breaking-badd.html", fake.origin());
    assert_eq!(post.header("referer"), Some(referer.as_str()));
    assert_eq!(
        post.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
}

#[tokio::test]
async fn a_season_pack_serves_its_other_episodes_from_the_cache() {
    let fake = Fake::new();
    let site = fake.site();
    let listed = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    let pack = row(&listed, 725317);
    assert_eq!(pack.kind, Kind::Pack { season: 1 });
    let e01 = file(site.fetch(&pack, S01E01, None).await);
    let before = fake.count();
    let e07 = file(
        site.fetch(
            &pack,
            Target::Episode {
                season: 1,
                episode: 7,
            },
            None,
        )
        .await,
    );
    assert_eq!(fake.count(), before, "no request for the second episode");
    assert!(String::from_utf8_lossy(&e01).contains("00:01:08,135 -->"));
    assert!(String::from_utf8_lossy(&e07).contains("00:00:01,201 -->"));
    // An episode the pack lacks: that pack is off the list for it.
    let e09 = Target::Episode {
        season: 1,
        episode: 9,
    };
    assert!(matches!(
        site.fetch(&pack, e09, None).await,
        Fetched::Fault {
            downloads_changed: false,
            ..
        }
    ));
}

#[tokio::test]
async fn a_site_that_does_not_answer_is_left_alone_then_tried_again() {
    let fake = Fake::new();
    fake.route(|r| {
        r.path
            .starts_with("/mov/")
            .then(|| reply("503 Service Unavailable", &[], b"down"))
    });
    let site = fake.site();
    let problem = Listing::Problem(Problem::Unreachable);
    assert_eq!(
        site.list("0903747", S01E01, &Playing::default()).await,
        problem
    );
    let tried = fake.count();
    assert_eq!(
        site.list("0133093", Target::Movie, &Playing::default())
            .await,
        problem
    );
    assert_eq!(fake.count(), tried, "nothing sent while backing off");
    later(Duration::from_secs(121)).await;
    assert_eq!(
        site.list("0133093", Target::Movie, &Playing::default())
            .await,
        problem
    );
    assert!(fake.count() > tried, "one probe after the wait");
}

#[tokio::test]
async fn a_challenge_or_a_429_turns_the_device_away_for_a_while() {
    let fake = Fake::new();
    fake.route(|r| {
        (r.path == "/mov/0903747/x.html")
            .then(|| reply("429 Too Many Requests", &[("Retry-After", "30")], b""))
    });
    let site = fake.site();
    let blocked = Listing::Problem(Problem::Blocked);
    assert_eq!(
        site.list("0903747", S01E01, &Playing::default()).await,
        blocked
    );
    let tried = fake.count();
    later(Duration::from_secs(20)).await;
    assert_eq!(
        site.list("0133093", Target::Movie, &Playing::default())
            .await,
        blocked
    );
    assert_eq!(fake.count(), tried, "Retry-After honoured");
    later(Duration::from_secs(11)).await;
    assert!(matches!(
        site.list("0133093", Target::Movie, &Playing::default())
            .await,
        Listing::Tracks(_)
    ));

    // A challenge page as a 503, known by its body (a plain 503 is
    // unreachable), and as a 2xx with Cloudflare's header.
    let fake = Fake::new();
    fake.route(|r| {
        (r.path == "/mov/0903747/x.html").then(|| {
            reply(
                "503 Service Unavailable",
                &[],
                b"<div id=\"challenge-platform\"></div>",
            )
        })
    });
    assert_eq!(
        fake.site()
            .list("0903747", S01E01, &Playing::default())
            .await,
        blocked
    );
    let fake = Fake::new();
    fake.route(|r| {
        (r.path == "/mov/0903747/x.html")
            .then(|| reply("403 Forbidden", &[], b"<title>Just a moment...</title>"))
    });
    assert_eq!(
        fake.site()
            .list("0903747", S01E01, &Playing::default())
            .await,
        blocked
    );
    let fake = Fake::new();
    fake.route(|r| {
        (r.path == "/mov/0903747/x.html")
            .then(|| reply("200 OK", &[("cf-mitigated", "challenge")], b"<html></html>"))
    });
    assert_eq!(
        fake.site()
            .list("0903747", S01E01, &Playing::default())
            .await,
        blocked
    );
}

#[tokio::test]
async fn a_changed_page_is_reported_for_that_title_and_the_site_goes_on() {
    let fake = Fake::new();
    let renamed = corpus_text("title-breaking-bad.html").replace(
        r#"<div class="altyazi-list-wrapper">"#,
        r#"<div class="list">"#,
    );
    fake.title("0903747", "breaking-badd", renamed);
    let site = fake.site();
    let changed = Listing::Problem(Problem::Changed);
    assert_eq!(
        site.list("0903747", S01E01, &Playing::default()).await,
        changed
    );
    assert!(matches!(
        site.list("0133093", Target::Movie, &Playing::default())
            .await,
        Listing::Tracks(_)
    ));
    let tried = fake.count();
    assert_eq!(
        site.list("0903747", S01E01, &Playing::default()).await,
        changed
    );
    assert_eq!(fake.count(), tried, "the title remembers it for the hour");
}

#[tokio::test]
async fn an_answer_from_another_origin_or_over_plain_http_is_a_change() {
    let elsewhere = Fake::new();
    let fake = Fake::new();
    let away = format!("{}/mov/0903747/breaking-badd.html", elsewhere.origin());
    fake.route(move |r| (r.path == "/mov/0903747/x.html").then(|| moved(&away)));
    let plain = format!("http://{HOST}:{}/mov/0133093/the-matrix1.html", fake.port);
    fake.route(move |r| (r.path == "/mov/0133093/x.html").then(|| moved(&plain)));
    let site = fake.site();
    let changed = Listing::Problem(Problem::Changed);
    assert_eq!(
        site.list("0903747", S01E01, &Playing::default()).await,
        changed,
        "another origin"
    );
    assert_eq!(
        site.list("0133093", Target::Movie, &Playing::default())
            .await,
        changed,
        "plain http"
    );
}

#[tokio::test]
async fn a_missing_file_is_off_the_list_for_hours_and_never_pauses_the_site() {
    let fake = Fake::new();
    fake.route(|r| {
        (r.path == "/sub/108735/breaking-badd.html").then(|| reply("404 Not Found", &[], b"gone"))
    });
    let site = fake.site();
    let listed = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    let gone = row(&listed, 108735);
    assert_eq!(
        site.fetch(&gone, S01E01, None).await,
        Fetched::Fault {
            fault: Fault::Gone,
            downloads_changed: false
        }
    );
    let tried = fake.count();
    assert_eq!(
        site.fetch(&gone, S01E01, None).await,
        Fetched::Fault {
            fault: Fault::KnownBad,
            downloads_changed: false
        }
    );
    assert_eq!(fake.count(), tried);
    let again = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    assert!(again.iter().all(|t| t.row.altid != 108735), "off the list");
    // The site goes on: another file downloads.
    assert!(matches!(
        site.fetch(&row(&listed, 725317), S01E01, None).await,
        Fetched::File(_)
    ));
}

#[tokio::test]
async fn three_files_failing_one_step_alike_mean_the_downloads_changed() {
    let fake = Fake::new();
    fake.route(|r| {
        r.path
            .starts_with("/sub/")
            .then(|| ok(b"<html>no form here</html>"))
    });
    let site = fake.site();
    let listed = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    let ids: Vec<u32> = listed.iter().map(|t| t.row.altid).take(3).collect();
    let mut changed = Vec::new();
    for id in ids {
        match site.fetch(&row(&listed, id), S01E01, None).await {
            Fetched::Fault {
                fault: Fault::NoForm,
                downloads_changed,
            } => changed.push(downloads_changed),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(changed, [false, false, true]);
}

#[tokio::test]
async fn a_fetch_that_runs_out_of_time_keeps_its_track() {
    let fake = Fake::new();
    fake.route(|r| {
        (r.method == "POST").then(|| {
            std::thread::sleep(Duration::from_secs(4));
            reply("200 OK", &[], b"late")
        })
    });
    // Each request may take 10 s: only the whole fetch's 1 s cap ends it.
    let site = fake.site_with(Limits {
        request: Duration::from_secs(10),
        fetch: Duration::from_secs(1),
        spacing: Duration::from_millis(5),
    });
    let listed = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    let track = row(&listed, 108735);
    let started = std::time::Instant::now();
    assert_eq!(
        site.fetch(&track, S01E01, None).await,
        Fetched::Problem(Problem::Unreachable)
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    later(Duration::from_secs(121)).await;
    let again = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    assert!(again.iter().any(|t| t.row.altid == 108735), "still offered");
}

#[tokio::test]
async fn a_forbidden_download_is_that_files_fault() {
    let fake = Fake::new();
    fake.route(|r| (r.method == "POST").then(|| reply("403 Forbidden", &[], b"no")));
    let site = fake.site();
    let listed = tracks(site.list("0903747", S01E01, &Playing::default()).await);
    assert_eq!(
        site.fetch(&row(&listed, 108735), S01E01, None).await,
        Fetched::Fault {
            fault: Fault::Gone,
            downloads_changed: false
        }
    );
    assert!(
        matches!(
            site.list("0133093", Target::Movie, &Playing::default())
                .await,
            Listing::Tracks(_)
        ),
        "no pause"
    );
}
