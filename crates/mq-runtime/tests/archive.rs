//! The Internet Archive source (plans/plugins-and-marketplace.md 5 and the
//! archive pass of 4.4), against an https fixture standing for archive.org.

use mq_library::{AddonRow, AllowedRow, ArchiveFilm, OWNER};
use mq_runtime::archive::{index, norm};
use mq_runtime::speed::Network;
use mq_runtime::{Core, RowOrigin, TitleFacts, VersionsParams, VersionsView};
use mq_streams::{DeviceCaps, StreamPrefs, Target};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

const SCRAPE: &str = include_str!("fixtures/archive-scrape.json");
const NOSFERATU: &str = "nosferatu-1922_202504";
const DAY_MS: u64 = 86_400_000;

/// What the fixture answers: the list, and items by id with a delay.
#[derive(Default)]
struct Answers {
    scrape: Option<(u16, Vec<u8>)>,
    items: HashMap<String, (u16, Vec<u8>, Duration)>,
}

/// archive.org on loopback over https, with every request it got (host
/// and path).
#[derive(Clone)]
struct Fixture {
    port: u16,
    asked: Arc<Mutex<Vec<String>>>,
    answers: Arc<Mutex<Answers>>,
}

fn reply(status: u16, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

impl Fixture {
    fn start() -> Fixture {
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        for name in ["archive.org", "elsewhere.c-archive.test"] {
            mq_http::test_dns::set_public(name, &[loopback]);
        }
        let asked: Arc<Mutex<Vec<String>>> = Arc::default();
        let answers: Arc<Mutex<Answers>> = Arc::new(Mutex::new(Answers {
            scrape: Some((200, SCRAPE.as_bytes().to_vec())),
            items: HashMap::from([(NOSFERATU.to_owned(), (200, nosferatu(), Duration::ZERO))]),
        }));
        let (log, said) = (asked.clone(), answers.clone());
        let port = mq_http::test_dns::https_requests(move |r| {
            let host = r.header("host").unwrap_or_default().to_owned();
            log.lock().unwrap().push(format!("{host}{}", r.path));
            if r.path.starts_with("/services/search/v1/scrape") {
                let scrape = said.lock().unwrap().scrape.clone();
                return match scrape {
                    Some((status, body)) => reply(status, &body),
                    None => {
                        let port = host.rsplit(':').next().unwrap_or_default();
                        format!("HTTP/1.1 302 Found\r\nLocation: https://elsewhere.c-archive.test:{port}/list\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
                    }
                };
            }
            let id = r.path.strip_prefix("/metadata/").unwrap_or_default();
            let item = said.lock().unwrap().items.get(id).cloned();
            match item {
                Some((status, body, delay)) => {
                    std::thread::sleep(delay);
                    reply(status, &body)
                }
                None => reply(404, b"{}"),
            }
        });
        Fixture {
            port,
            asked,
            answers,
        }
    }

    fn origin(&self) -> Url {
        Url::parse(&format!("https://archive.org:{}", self.port)).unwrap()
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    fn lists(&self) -> usize {
        self.asked()
            .iter()
            .filter(|a| a.contains("/scrape"))
            .count()
    }

    fn items(&self) -> usize {
        self.asked()
            .iter()
            .filter(|a| a.contains("/metadata/"))
            .count()
    }

    fn answer_list(&self, status: u16, body: Vec<u8>) {
        self.answers.lock().unwrap().scrape = Some((status, body));
    }

    fn answer_item(&self, id: &str, status: u16, body: Vec<u8>, delay: Duration) {
        (self.answers.lock().unwrap().items).insert(id.to_owned(), (status, body, delay));
    }
}

/// Nosferatu's files: three MP4s of different heights, a fourth shorter
/// one, and files that are not MP4.
fn nosferatu() -> Vec<u8> {
    json!({"files": [
        {"name": "Nosferatu (1922).mp4", "format": "MPEG4", "size": "493091534", "height": "288", "width": "368"},
        {"name": "__ia_thumb.jpg", "format": "Item Tile", "size": "9918"},
        {"name": "Nosferatu (1922)_512kb.mp4", "format": "512Kb MPEG4", "size": "300000000", "height": "240"},
        {"name": "Nosferatu.ogv", "format": "Ogg Video", "height": "480"},
        {"name": "nosferatu.HD.mp4", "format": "h.264", "size": "900000000", "height": "480"},
        {"name": "extra/clip.mp4", "format": "h.264", "size": "1000", "height": "144"}
    ]})
    .to_string()
    .into_bytes()
}

/// A core whose Archive is `fixture`, its profile past the notice.
fn core_at(fixture: &Fixture) -> Core {
    let core = Core::temporary("p").unwrap();
    core.use_archive_at(fixture.origin());
    core.set_prefs(&json!({"archiveNoticeSeen": true})).unwrap();
    core
}

fn film(title: &str, year: Option<u16>, id: &str) -> VersionsParams {
    let year_text = year.map(|y| y.to_string());
    VersionsParams {
        kind: "movie".into(),
        video_id: id.into(),
        target: Target {
            title: title.into(),
            year,
            season: None,
            episode: None,
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
        facts: TitleFacts::read(Some("90 min"), year_text.as_deref(), None, None),
    }
}

fn nosferatu_film() -> VersionsParams {
    film("Nosferatu", Some(1922), "tt0013442")
}

async fn until(what: &str, check: impl Fn() -> bool) {
    for _ in 0..400 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("waited 10 s for {what}");
}

/// Opens Versions and waits until the addons and the Archive pass are in.
async fn settled(core: &Core, params: VersionsParams) -> VersionsView {
    let view = VersionsView::open(core, params);
    until("settled", || {
        let s = view.snapshot();
        s.done && !s.archive.pending
    })
    .await;
    view
}

fn archive_keys(view: &VersionsView) -> Vec<String> {
    let s = view.snapshot();
    s.archive.rows.iter().map(|r| r.key.clone()).collect()
}

fn fetched_days_ago(core: &Core, days: u64) {
    let at = mq_library::hlc::now_ms() - days * DAY_MS;
    let state = json!({"fetchedMs": at}).to_string();
    core.library()
        .set_device_value("archive_index", &state)
        .unwrap();
}

fn state(core: &Core) -> Value {
    let text = core.library().device_value("archive_index").unwrap();
    serde_json::from_str(&text.unwrap_or_default()).unwrap_or_default()
}

/// The list keeps 5.2's rules: no year, a bare number, the upload year, a
/// 1968 film and a trailer go; "(YYYY)" in the title wins over the field;
/// one item per normalized title and year, a plain title before a dub or
/// an edition, then downloads, then the first; an identifier that could
/// leave a URL path goes. The same answer through the rules script
/// (bench/plugins/archive-index-rules.py) gives these films and one more,
/// the bad identifier.
#[test]
fn the_list_keeps_the_rules() {
    let items: Value = serde_json::from_str(SCRAPE).unwrap();
    let films = index(items["items"].as_array().unwrap());
    let mut got: Vec<String> = films
        .iter()
        .map(|f| format!("{}|{}|{}", f.title, f.year, f.identifier))
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            "1984 nigel kneale|1954|1984-nigel-kneale",
            "his girl friday|1940|his-girl-friday-1940",
            "last year|1963|the-1963-film",
            "metropolis|1927|metropolis-1927",
            "nosferatu dvd quality|1922|Nosferatu_DVD_quality",
            "nosferatu|1922|nosferatu-1922_202504",
            "plan 9 from outer space|1957|plan-9-1957",
            "white zombie|1932|white-zombie-3",
        ]
    );
}

#[test]
fn titles_normalize_as_the_rules_script_does() {
    for (title, normalized) in [
        ("Nosferatu (1922)", "nosferatu"),
        ("The White Zombie", "white zombie"),
        ("White Zombie Full HD", "white zombie"),
        ("Metropolis [Restored] (1927) - English", "metropolis"),
        ("The", "the"),
        ("HD-Hunter", "hunter"),
        ("Shadow (unfinished", "shadow unfinished"),
        ("His Girl Friday!", "his girl friday"),
        ("Café Society", "caf society"),
        ("Plan 9 from Outer Space", "plan 9 from outer space"),
        ("Fullhd Days", "fullhd days"),
    ] {
        assert_eq!(norm(title), normalized, "{title}");
    }
}

/// `MQ_ARCHIVE_SCRAPE=<answer.json> MQ_ARCHIVE_OUT=<out.txt>`: the live
/// answer's list as title|year|identifier lines, for the parity check with
/// the rules script.
#[test]
#[ignore = "reads a live answer saved by hand"]
fn writes_the_list_of_a_live_answer() {
    let path = std::env::var("MQ_ARCHIVE_SCRAPE").unwrap();
    let items: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let films: Vec<ArchiveFilm> = index(items["items"].as_array().unwrap());
    let mut lines: Vec<String> = films
        .iter()
        .map(|f| format!("{}|{}|{}", f.title, f.year, f.identifier))
        .collect();
    lines.sort();
    std::fs::write(
        std::env::var("MQ_ARCHIVE_OUT").unwrap(),
        lines.join("\n") + "\n",
    )
    .unwrap();
}

/// A matched film lists its tallest three MP4s as Archive rows: URLs built
/// here, keys of their own, the resolution from the height, "Internet
/// Archive" and archive.org named; never the pick, never among the addons'
/// rows, and with nothing else to play no pick, nothing empty, the rows
/// startable from their own pass.
#[tokio::test]
async fn a_matched_film_lists_its_files_in_a_section_of_their_own() {
    let fixture = Fixture::start();
    let core = core_at(&fixture);
    let view = settled(&core, nosferatu_film()).await;
    let s = view.snapshot();
    let base = format!("https://archive.org:{}/download/{NOSFERATU}", fixture.port);
    let mut rows: Vec<(&str, &str)> = (s.archive.rows.iter())
        .map(|r| (r.key.as_str(), r.quality.as_str()))
        .collect();
    // The 480p file leads; the two 240p-class ones follow in the pass's order.
    assert_eq!(
        rows[0],
        ("archive:nosferatu-1922_202504:nosferatu.HD.mp4", "SD")
    );
    rows.sort();
    assert_eq!(
        rows,
        [
            ("archive:nosferatu-1922_202504:Nosferatu (1922).mp4", "SD"),
            (
                "archive:nosferatu-1922_202504:Nosferatu (1922)_512kb.mp4",
                "SD"
            ),
            ("archive:nosferatu-1922_202504:nosferatu.HD.mp4", "SD"),
        ]
    );
    let first = &s.archive.rows[0];
    assert_eq!(first.origin, RowOrigin::Archive);
    assert_eq!(first.host.as_deref(), Some("archive.org"));
    assert_eq!(first.source_name.as_deref(), Some("Internet Archive"));
    assert_eq!(first.via, "Internet Archive");
    assert_eq!(first.release_name, "nosferatu.HD.mp4");
    // Only Archive rows: no pick, nothing said to be empty.
    assert!(s.recommended.is_none() && s.rows.is_empty(), "{s:?}");
    assert!(s.empty.is_none(), "{:?}", s.empty);
    let played = view.candidates(&first.key);
    assert_eq!(played.len(), 1);
    let mq_protocol::Source::Http { url } = &played[0].source else {
        panic!("{:?}", played[0].source);
    };
    assert_eq!(url.as_str(), format!("{base}/nosferatu.HD.mp4"));
    let spaced = view.candidates("archive:nosferatu-1922_202504:Nosferatu (1922).mp4");
    let mq_protocol::Source::Http { url } = &spaced[0].source else {
        panic!("{spaced:?}");
    };
    assert_eq!(url.as_str(), format!("{base}/Nosferatu%20(1922).mp4"));
    assert_eq!(
        played[0].addon_url.as_ref().map(Url::as_str),
        Some(mq_server::ARCHIVE)
    );
    // A tap is no pick: it teaches nothing.
    let pick = view.pick(&first.key);
    assert!(pick.note.is_none() && !pick.handle.recommended());
    assert_eq!((fixture.lists(), fixture.items()), (1, 1));
    // The item is kept a day, in memory: a second open asks nothing.
    let again = settled(&core, nosferatu_film()).await;
    assert_eq!(archive_keys(&again).len(), 3);
    assert_eq!((fixture.lists(), fixture.items()), (1, 1));
}

/// A film the list does not hold asks archive.org nothing and adds no
/// wait: no item request, and with no addon the view says so at once.
/// Neither does a film with no year, a series, nor a title one letter off
/// or three years off.
#[tokio::test]
async fn no_match_asks_nothing() {
    let fixture = Fixture::start();
    let core = core_at(&fixture);
    // The list arrives with the first open.
    settled(&core, film("Inception", Some(2010), "tt1375666")).await;
    assert_eq!((fixture.lists(), fixture.items()), (1, 0));
    let before = fixture.asked().len();
    for params in [
        film("Inception", Some(2010), "tt1375666"),
        film("Nosferatu", None, "tt0013442"),
        film("Nosferatu", Some(1925), "tt0013442"),
        film("Nosferat", Some(1922), "tt0013442"),
    ] {
        let view = settled(&core, params).await;
        let s = view.snapshot();
        assert!(s.archive.rows.is_empty());
        assert_eq!(
            s.empty.as_ref().map(|e| e.kind),
            Some(mq_runtime::EmptyKind::NoSource)
        );
    }
    let mut series = nosferatu_film();
    series.kind = "series".into();
    let view = VersionsView::open(&core, series);
    assert!(!view.snapshot().archive.pending, "a series is never asked");
    settled(&core, film("Nosferatu", Some(1923), "tt0013442")).await;
    // Only the year one off asked an item.
    let asked = fixture.asked();
    assert_eq!(asked.len(), before + 1, "{:?}", &asked[before..]);
    assert!(asked[before].ends_with(&format!("/metadata/{NOSFERATU}")));
}

/// Nothing is asked of archive.org before the notice, for a profile made
/// in Manage profiles too; the source off, or a kids profile, asks
/// nothing either; contact follows the active profile, not another one
/// that wants it.
#[tokio::test]
async fn the_notice_the_switch_and_kids_gate_every_contact() {
    let fixture = Fixture::start();
    let owner = Core::temporary(OWNER).unwrap();
    owner.use_archive_at(fixture.origin());
    owner
        .set_prefs(&json!({"archiveNoticeSeen": true}))
        .unwrap();
    let guest = owner.for_profile(&owner.create_profile("Guest", "fox").unwrap());
    assert!(guest.prefs().archive && !guest.prefs().archive_notice_seen);
    let view = settled(&guest, nosferatu_film()).await;
    assert!(view.snapshot().archive.rows.is_empty());
    assert!(fixture.asked().is_empty(), "{:?}", fixture.asked());
    // Off, though the notice was seen.
    guest
        .set_prefs(&json!({"archiveNoticeSeen": true, "archive": false}))
        .unwrap();
    settled(&guest, nosferatu_film()).await;
    assert!(fixture.asked().is_empty(), "{:?}", fixture.asked());
    // A kids profile, the film on its list, the source on and seen.
    owner.set_pin(OWNER, Some("1357"), None).await.unwrap();
    let kid = owner.for_profile(&owner.create_kids_profile("Kid", "owl").unwrap());
    kid.set_prefs(&json!({"archiveNoticeSeen": true})).unwrap();
    let row = AllowedRow {
        item_id: "tt0013442".into(),
        kind: "movie".into(),
        name: "Nosferatu".into(),
        names: vec![],
        poster: None,
        allowed: true,
        added_ms: 1,
    };
    kid.library()
        .write_allowed(kid.profile(), &row, &kid.hlc())
        .unwrap();
    let view = VersionsView::open_checked(&kid, nosferatu_film(), Some("tt0013442"));
    assert!(!view.denied());
    assert!(!view.snapshot().archive.pending);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(fixture.asked().is_empty(), "{:?}", fixture.asked());
    // The guest sees the notice: from then on it is asked.
    guest.set_prefs(&json!({"archive": true})).unwrap();
    let view = settled(&guest, nosferatu_film()).await;
    assert_eq!(archive_keys(&view).len(), 3);
    assert_eq!((fixture.lists(), fixture.items()), (1, 1));
}

/// The list goes once no profile that may use it has the source on.
#[tokio::test]
async fn the_list_goes_when_no_profile_wants_it() {
    let fixture = Fixture::start();
    let core = core_at(&fixture);
    settled(&core, nosferatu_film()).await;
    let films = || core.library().archive_films().unwrap();
    assert_eq!(films(), 8);
    core.set_prefs(&json!({"archive": false})).unwrap();
    assert_eq!(films(), 8, "the owner still has it on");
    core.for_profile(OWNER)
        .set_prefs(&json!({"archive": false}))
        .unwrap();
    assert_eq!(films(), 0);
}

/// The list is asked once a week, never on mobile data; a refusal (429,
/// 5xx) waits 1, 3, then 7 days and keeps the old list, which still
/// matches meanwhile.
#[tokio::test]
async fn the_list_is_weekly_never_on_mobile_data_and_backs_off() {
    let fixture = Fixture::start();
    let core = core_at(&fixture);
    // Mobile data: no first list either.
    core.set_network(Network::Cellular);
    settled(&core, nosferatu_film()).await;
    assert_eq!(fixture.lists(), 0);
    core.set_network(Network::Wifi);
    settled(&core, nosferatu_film()).await;
    assert_eq!(fixture.lists(), 1);
    // Six days on, the list stands; eight days on, it is asked again,
    // and the old one matches meanwhile.
    fetched_days_ago(&core, 6);
    settled(&core, nosferatu_film()).await;
    assert_eq!(fixture.lists(), 1);
    fetched_days_ago(&core, 8);
    fixture.answer_list(503, b"{}".to_vec());
    let view = settled(&core, nosferatu_film()).await;
    assert_eq!(archive_keys(&view).len(), 3, "the old list matches");
    until("the refresh", || fixture.lists() == 2).await;
    until("the backoff", || state(&core)["refusals"] == 1).await;
    let wait = |s: &Value| {
        let now = mq_library::hlc::now_ms();
        (s["retryMs"].as_u64().unwrap() - now + DAY_MS / 2) / DAY_MS
    };
    assert_eq!(wait(&state(&core)), 1);
    assert_eq!(core.library().archive_films().unwrap(), 8, "kept");
    settled(&core, nosferatu_film()).await;
    assert_eq!(fixture.lists(), 2, "held for a day");
    // The day passes: 429 now, three days; then seven, and seven again.
    fixture.answer_list(429, b"{}".to_vec());
    for (refusals, days) in [(2, 3), (3, 7), (4, 7)] {
        let mut s = state(&core);
        s["retryMs"] = json!(0);
        core.library()
            .set_device_value("archive_index", &s.to_string())
            .unwrap();
        settled(&core, nosferatu_film()).await;
        until("the backoff", || state(&core)["refusals"] == refusals).await;
        assert_eq!(wait(&state(&core)), days, "after {refusals}");
    }
    // An answer again: the list is new and the backoff gone.
    fixture.answer_list(200, SCRAPE.as_bytes().to_vec());
    let mut s = state(&core);
    s["retryMs"] = json!(0);
    core.library()
        .set_device_value("archive_index", &s.to_string())
        .unwrap();
    settled(&core, nosferatu_film()).await;
    until("a new list", || state(&core)["refusals"] == 0).await;
    assert!(state(&core)["fetchedMs"].as_u64().is_some());
}

/// Past 8 MiB or 50,000 items the list is refused and the old one kept;
/// an item past 2 MiB, or whose video is past its first 2,000 files,
/// lists nothing; a list that moves off archive.org is never followed.
#[tokio::test]
async fn caps_and_hops_hold() {
    let fixture = Fixture::start();
    let core = core_at(&fixture);
    let item = |i: usize| format!(r#"{{"identifier":"f{i}","title":"F (1930)","downloads":1}}"#);
    let many = (0..50_001).map(item).collect::<Vec<_>>().join(",");
    fixture.answer_list(200, format!(r#"{{"items":[{many}]}}"#).into_bytes());
    settled(&core, nosferatu_film()).await;
    until("refused", || state(&core)["refusals"] == 1).await;
    assert_eq!(core.library().archive_films().unwrap(), 0);
    let pad = "x".repeat(9 << 20);
    fixture.answer_list(200, format!(r#"{{"items":[],"pad":"{pad}"}}"#).into_bytes());
    let mut s = state(&core);
    s["retryMs"] = json!(0);
    core.library()
        .set_device_value("archive_index", &s.to_string())
        .unwrap();
    settled(&core, nosferatu_film()).await;
    until("refused", || state(&core)["refusals"] == 2).await;
    assert_eq!(core.library().archive_films().unwrap(), 0);
    // A list that moves off archive.org: not followed, not a refusal.
    let fresh = core_at(&fixture);
    fixture.answers.lock().unwrap().scrape = None;
    settled(&fresh, nosferatu_film()).await;
    assert!(
        !fixture.asked().iter().any(|a| a.starts_with("elsewhere.")),
        "{:?}",
        fixture.asked()
    );
    assert_eq!(fresh.library().archive_films().unwrap(), 0);
    // The real list, then items too big or too long.
    fixture.answer_list(200, SCRAPE.as_bytes().to_vec());
    let core = core_at(&fixture);
    let pad = "x".repeat(2 << 20);
    let big = json!({"pad": pad, "files": [{"name": "a.mp4", "height": "480"}]});
    fixture.answer_item(NOSFERATU, 200, big.to_string().into_bytes(), Duration::ZERO);
    let view = settled(&core, nosferatu_film()).await;
    assert!(archive_keys(&view).is_empty());
    let mut files: Vec<Value> = (0..2_000)
        .map(|i| json!({"name": format!("t{i}.jpg")}))
        .collect();
    files.push(json!({"name": "late.mp4", "height": "480"}));
    let long = json!({ "files": files });
    fixture.answer_item(
        NOSFERATU,
        200,
        long.to_string().into_bytes(),
        Duration::ZERO,
    );
    let view = settled(&core, nosferatu_film()).await;
    assert!(archive_keys(&view).is_empty());
}

/// A stream addon on loopback answering `body` for any request.
async fn stream_addon(body: String) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = json!({
        "id": "films", "version": "1.0.0", "name": "Films",
        "types": ["movie"], "resources": ["stream"], "catalogs": [], "idPrefixes": ["tt"]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

/// An addon's 1080p torrent (ready on the account) stays the pick over Archive rows, which never
/// join the addons' rows; an addon row with an Archive row's URL is a
/// second row, each playing its own source through its own install.
#[tokio::test]
async fn addon_rows_keep_the_pick_and_their_own_sources() {
    let fixture = Fixture::start();
    let core = core_at(&fixture);
    core.set_debrid_keys(&[(mq_runtime::StoreId::RealDebrid, "rd".into())]);
    let same = format!(
        "https://archive.org:{}/download/{NOSFERATU}/nosferatu.HD.mp4",
        fixture.port
    );
    let body = json!({"streams": [
        {"name": "Films\n1080p", "title": "Nosferatu.1922.1080p.BluRay.x264-GRP",
         "infoHash": "a".repeat(40), "fileIdx": 0,
         "behaviorHints": {"cached": true, "service": "realdebrid", "videoSize": 4_000_000_000u64}},
        {"name": "Films", "title": "Nosferatu.1922.480p.WEB.x264", "url": same}
    ]});
    let addon = stream_addon(body.to_string()).await;
    let installed = addon.transport_url.clone();
    core.library()
        .set_addons("p", &[addon], &core.hlc())
        .unwrap();
    let view = settled(&core, nosferatu_film()).await;
    let s = view.snapshot();
    let pick = s.recommended.as_ref().expect("a pick");
    assert!(pick.key.starts_with("bt:"), "{}", pick.key);
    assert_eq!(pick.origin, RowOrigin::Addon);
    let addon_keys: Vec<&str> = std::iter::once(pick)
        .chain(&s.rows)
        .map(|r| r.key.as_str())
        .collect();
    assert!(
        !addon_keys.iter().any(|k| k.starts_with("archive:")),
        "{addon_keys:?}"
    );
    let twin = format!("url:{same}");
    assert!(addon_keys.contains(&twin.as_str()), "{addon_keys:?}");
    let archive_twin = format!("archive:{NOSFERATU}:nosferatu.HD.mp4");
    assert!(archive_keys(&view).contains(&archive_twin));
    let by = |key: &str| {
        let played = view.candidates(key);
        assert_eq!(played.len(), 1, "{key}: {played:?}");
        played[0].addon_url.as_ref().map(Url::to_string)
    };
    assert_eq!(by(&twin), Some(installed));
    assert_eq!(by(&archive_twin).as_deref(), Some(mq_server::ARCHIVE));
}

/// `done` and the pick never wait for the Archive pass; `empty` waits while
/// it reads an item; an addon change keeps its rows and asks nothing again.
#[tokio::test]
async fn the_pass_is_pending_apart_and_survives_an_addon_change() {
    let fixture = Fixture::start();
    let core = core_at(&fixture);
    // The list first, then a slow item.
    settled(&core, film("Inception", Some(2010), "tt1375666")).await;
    fixture.answer_item(NOSFERATU, 200, nosferatu(), Duration::from_millis(1500));
    let view = VersionsView::open(&core, nosferatu_film());
    until("done", || view.snapshot().done).await;
    let s = view.snapshot();
    assert!(s.archive.pending, "the item is still being read");
    assert!(s.empty.is_none(), "{:?}", s.empty);
    until("the pass", || !view.snapshot().archive.pending).await;
    assert_eq!(archive_keys(&view).len(), 3);
    let items = fixture.items();
    let addon = stream_addon(json!({"streams": []}).to_string()).await;
    core.library()
        .set_addons("p", &[addon], &core.hlc())
        .unwrap();
    until("the addon's answer", || {
        let s = view.snapshot();
        s.done && s.empty.is_none() && s.pending_addons.is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(archive_keys(&view).len(), 3, "the pass survives");
    assert_eq!(fixture.items(), items, "and is not run again");
}
