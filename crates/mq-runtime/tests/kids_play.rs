//! A kids profile's play path (plans/m11-kids.md, phase K3b): title,
//! versions and playback refused outside the list, the strict release rule,
//! the next episode checked when it lands, and a playback stopped because
//! its title left the list. Open profiles are checked to be as before.

use mq_library::{AddonRow, AllowedRow, OWNER};
use mq_runtime::views::versions::Verified;
use mq_runtime::{
    Core, EmptyAction, EmptyKind, PlayTarget, PlaybackPhase, PlaybackView, TitleView,
    VersionsParams, VersionsView,
};
use mq_streams::{DeviceCaps, HideReason, StreamPrefs, Target};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A local addon serving metas and streams, logging each request.
#[derive(Clone, Default)]
struct Fixture {
    log: Arc<Mutex<Vec<String>>>,
    /// Answers by request path; a meta defaults to one of the asked kind
    /// and id, streams to none.
    answers: Arc<Mutex<HashMap<String, Value>>>,
    /// How long a meta answer takes.
    meta_delay: Arc<Mutex<Duration>>,
}

impl Fixture {
    async fn addon(&self) -> AddonRow {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fixture = self.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let fixture = fixture.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = head
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .replace("%3A", ":");
                    fixture.log.lock().unwrap().push(path.clone());
                    let body = fixture.answer(&path).await;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });
        let manifest = json!({
            "id": "fixture", "version": "1.0.0", "name": "Fixture",
            "types": ["movie", "series"], "resources": ["meta", "stream", "subtitles"],
            "idPrefixes": ["tt"], "catalogs": []
        });
        AddonRow {
            transport_url: format!("http://{addr}/manifest.json"),
            manifest_json: manifest.to_string(),
            position: 0,
            enabled: true,
            allow_private: true,
        }
    }

    async fn answer(&self, path: &str) -> String {
        let path = path.replace("%3A", ":");
        if path.starts_with("/meta/") {
            let delay = *self.meta_delay.lock().unwrap();
            tokio::time::sleep(delay).await;
        }
        if let Some(answer) = self.answers.lock().unwrap().get(&path) {
            return answer.to_string();
        }
        let rest = path.trim_end_matches(".json");
        if let Some(rest) = rest.strip_prefix("/meta/") {
            let (kind, id) = rest.split_once('/').unwrap_or_default();
            return json!({"meta": {"id": id, "type": kind, "name": format!("Name {id}")}})
                .to_string();
        }
        json!({"streams": []}).to_string()
    }

    fn set(&self, path: &str, answer: Value) {
        self.answers.lock().unwrap().insert(path.into(), answer);
    }

    fn asked(&self, prefix: &str) -> Vec<String> {
        let log = self.log.lock().unwrap();
        log.iter()
            .filter(|p| p.starts_with(prefix))
            .cloned()
            .collect()
    }
}

const PIN: &str = "1357";

/// The owner with a PIN and `fixture`'s addon, and a kids profile.
async fn kid(fixture: &Fixture) -> (Core, Core) {
    let owner = Core::temporary(OWNER).unwrap();
    // Store builds open torrents only through an account (M13 3.4).
    if !owner.has_engine() {
        owner.set_debrid_keys(&[(mq_runtime::StoreId::RealDebrid, "rd".into())]);
    }
    owner.set_pin(OWNER, Some(PIN), None).await.unwrap();
    let addon = fixture.addon().await;
    owner
        .library()
        .set_addons(OWNER, &[addon], &owner.hlc())
        .unwrap();
    let id = owner.create_kids_profile("Kid", "fox").unwrap();
    (owner.clone(), owner.for_profile(&id))
}

fn allow(kid: &Core, id: &str, kind: &str, name: &str, names: &[&str]) {
    let row = AllowedRow {
        item_id: id.into(),
        kind: kind.into(),
        name: name.into(),
        names: names.iter().map(|n| n.to_string()).collect(),
        poster: None,
        allowed: true,
        added_ms: 1,
    };
    kid.library()
        .write_allowed(kid.profile(), &row, &kid.hlc())
        .unwrap();
}

fn remove(kid: &Core, id: &str) {
    let row = AllowedRow {
        item_id: id.into(),
        kind: "movie".into(),
        name: String::new(),
        names: vec![],
        poster: None,
        allowed: false,
        added_ms: 0,
    };
    kid.library()
        .write_allowed(kid.profile(), &row, &kid.hlc())
        .unwrap();
}

fn generation(core: &Core) -> u64 {
    core.library().policy_checked(core.profile()).unwrap().1
}

async fn until(what: &str, check: impl Fn() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("waited 5 s for {what}");
}

fn params(kind: &str, video_id: &str, title: &str) -> VersionsParams {
    VersionsParams {
        kind: kind.into(),
        video_id: video_id.into(),
        target: Target {
            title: title.into(),
            ..Target::default()
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
        facts: Default::default(),
    }
}

fn target(kind: &str, item_id: &str, video_id: &str) -> PlayTarget {
    PlayTarget {
        kind: kind.into(),
        item_id: item_id.into(),
        video_id: video_id.into(),
        name: Some("Name".into()),
        ..PlayTarget::default()
    }
}

#[tokio::test]
async fn a_title_off_the_list_opens_denied_and_trailers_never_show() {
    let fixture = Fixture::default();
    fixture.set(
        "/meta/movie/tt1.json",
        json!({"meta": {"id": "tt1", "type": "movie", "name": "Up",
            "trailerStreams": [{"title": "Trailer", "ytId": "eRsGyueVLvQ"}]}}),
    );
    let (owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", &[]);
    let off = TitleView::open(&kid, "movie", "tt2");
    assert!(off.denied());
    let page = TitleView::open(&kid, "movie", "tt1");
    assert!(!page.denied());
    until("the details", || page.snapshot().header.details.is_some()).await;
    assert!(page.snapshot().header.details.unwrap().trailers.is_empty());
    assert!(fixture.asked("/meta/movie/tt2").is_empty(), "never asked");
    assert!(off.snapshot().header.details.is_none());
    // The owner's page is as before.
    let owners = TitleView::open(&owner, "movie", "tt1");
    until("the owner's details", || {
        owners.snapshot().header.details.is_some()
    })
    .await;
    assert_eq!(owners.snapshot().header.details.unwrap().trailers.len(), 1);
}

#[tokio::test]
async fn versions_need_the_list_and_a_video_of_the_title() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "series", "Bluey", &[]);
    let refused = [
        VersionsView::open(&kid, params("series", "tt1:1:1", "Bluey")),
        VersionsView::open_checked(&kid, params("series", "tt10:1:1", "Bluey"), Some("tt1")),
        VersionsView::open_checked(&kid, params("movie", "tt1", "Bluey"), Some("tt1")),
        VersionsView::open_checked(&kid, params("movie", "tt2", "Up"), Some("tt2")),
    ];
    assert!(refused.iter().all(VersionsView::denied));
    assert!(refused.iter().all(|v| v.candidates("any").is_empty()));
    // Said in a child's words, with nothing to do (play-every-version 2a).
    assert!(refused.iter().all(|v| {
        v.snapshot()
            .empty
            .is_some_and(|e| (e.kind, e.action) == (EmptyKind::KidsNothing, EmptyAction::None))
    }));
    let allowed =
        VersionsView::open_checked(&kid, params("series", "tt1:2:5", "Other"), Some("tt1"));
    assert!(!allowed.denied());
    let verified: &Verified = allowed.verified().unwrap();
    assert_eq!(
        (verified.item_id(), verified.video_id(), verified.name()),
        ("tt1", "tt1:2:5", "Bluey")
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let asked = fixture.asked("/stream/");
    assert_eq!(
        asked,
        ["/stream/series/tt1:2:5.json"],
        "only the allowed one"
    );
}

/// The releases a kid's "Up" meets on one addon.
fn up_streams() -> Value {
    let torrent = |i: u8, title: &str| {
        json!({"name": "Fixture\n1080p", "title": title,
               "infoHash": format!("{i:040x}"), "fileIdx": 0})
    };
    json!({"streams": [
        torrent(1, "Up.2009.1080p.BluRay.x264-GROUP"),
        torrent(2, "Up.in.the.Air.2009.1080p.BluRay.x264-GROUP"),
        torrent(3, "Yukari.Bak.2009.720p.WEB.x264-GROUP"),
        torrent(4, "Up.2009.XXX.1080p.WEB.x264-GROUP"),
        torrent(5, "Up.2009.720p.BluRay.x264-GROUP\nRipped in Essex"),
        {"ytId": "abcdefghijk", "title": "Up"}
    ]})
}

async fn settled(view: &VersionsView) -> mq_runtime::VersionsSnapshot {
    until("every addon answered", || view.snapshot().done).await;
    view.snapshot()
}

fn release_names(s: &mq_runtime::VersionsSnapshot) -> Vec<String> {
    let mut names: Vec<String> = s
        .recommended
        .iter()
        .chain(&s.rows)
        .map(|r| r.release_name.clone())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn a_kid_gets_only_releases_of_the_allowed_title() {
    let fixture = Fixture::default();
    fixture.set("/stream/movie/tt1.json", up_streams());
    let (owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", &["Yukari Bak"]);
    // The app passes another title: the list's names rank, not it.
    let view =
        VersionsView::open_checked(&kid, params("movie", "tt1", "Up in the Air"), Some("tt1"));
    let s = settled(&view).await;
    assert_eq!(
        release_names(&s),
        [
            "Up.2009.1080p.BluRay.x264-GROUP",
            "Up.2009.720p.BluRay.x264-GROUP",
            "Yukari.Bak.2009.720p.WEB.x264-GROUP"
        ]
    );
    let by = |reason| {
        s.hidden
            .by_reason
            .iter()
            .find(|(r, _)| *r == reason)
            .map_or(0, |(_, n)| *n)
    };
    assert_eq!(by(HideReason::WrongTitle), 1);
    assert_eq!(by(HideReason::NotForKids), 1);
    assert_eq!(by(HideReason::LeavesApp), 1);
    // The owner's Versions have none of the kids rules; the link out is
    // hidden for every profile, since playback opens none (M13 code review
    // r2 P1-1).
    let owners = VersionsView::open(&owner, params("movie", "tt1", "Up"));
    let s = settled(&owners).await;
    assert_eq!(
        s.hidden.by_reason,
        [(HideReason::UnsupportedSource, 1)],
        "{:?}",
        s.hidden
    );
}

#[tokio::test]
async fn nothing_playable_leaves_no_pick_and_the_owner_counts_none() {
    let fixture = Fixture::default();
    fixture.set(
        "/stream/movie/tt1.json",
        json!({"streams": [{"name": "Fixture", "title": "Up.in.the.Air.2009.1080p.x264-GROUP",
                            "infoHash": format!("{:040x}", 7), "fileIdx": 0}]}),
    );
    let (owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", &[]);
    let s = settled(&VersionsView::open_checked(
        &kid,
        params("movie", "tt1", "Up"),
        Some("tt1"),
    ))
    .await;
    assert!(s.recommended.is_none() && s.rows.is_empty());
    assert_eq!(s.empty.map(|e| e.kind), Some(EmptyKind::KidsNothing));
    // "View as": the owner runs the same view as the kid.
    let as_kid = owner.for_profile(kid.profile());
    let s = settled(&VersionsView::open_checked(
        &as_kid,
        params("movie", "tt1", "Up"),
        Some("tt1"),
    ))
    .await;
    assert_eq!(s.recommended.iter().count() + s.rows.len(), 0);
}

#[tokio::test]
async fn a_playback_off_the_list_opens_denied() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "series", "Bluey", &[]);
    for t in [
        target("movie", "tt2", "tt2"),
        target("series", "tt1", "tt10:1:1"),
        target("movie", "tt1", "tt1"),
    ] {
        let view = PlaybackView::open(&kid, Vec::new(), t);
        assert!(view.denied());
        assert_eq!(view.snapshot().url, None);
    }
    assert!(!PlaybackView::open(&kid, Vec::new(), target("series", "tt1", "tt1:1:1")).denied());
}

/// A file origin: answers any range of a `len`-byte file, sending it in
/// 16 KiB pieces `gap` apart, after `first` before the first byte.
async fn origin(len: u64, first: Duration, gap: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let start: u64 = head
                    .split("range: bytes=")
                    .nth(1)
                    .and_then(|r| r.split('-').next())
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
                tokio::time::sleep(first).await;
                let resp = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\nContent-Type: video/x-matroska\r\nConnection: close\r\n\r\n",
                    len - 1,
                    len - start
                );
                if socket.write_all(resp.as_bytes()).await.is_err() {
                    return;
                }
                let mut left = len - start;
                while left > 0 {
                    let n = left.min(16 * 1024);
                    if socket.write_all(&vec![b'v'; n as usize]).await.is_err() {
                        return;
                    }
                    left -= n;
                    tokio::time::sleep(gap).await;
                }
            });
        }
    });
    format!("http://{addr}")
}

fn http_streams(base: &str) -> Value {
    json!({"streams": [{"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
                        "url": format!("{base}/up.mkv")}]})
}

/// The kid's candidates for `video_id` of `item_id`, from its own Versions.
async fn candidates(
    kid: &Core,
    kind: &str,
    item_id: &str,
    video_id: &str,
) -> Vec<mq_streams::ParsedStream> {
    let view = VersionsView::open_checked(kid, params(kind, video_id, ""), Some(item_id));
    let s = settled(&view).await;
    let key = s.recommended.expect("a pick").key;
    view.candidates(&key)
}

async fn ready(view: &PlaybackView) -> String {
    until("ready", || {
        view.snapshot().phase != PlaybackPhase::Resolving
    })
    .await;
    let s = view.snapshot();
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    s.url.unwrap()
}

#[tokio::test]
async fn the_next_episode_is_checked_when_it_lands() {
    let fixture = Fixture::default();
    let base = origin(64 * 1024, Duration::ZERO, Duration::ZERO).await;
    fixture.set(
        "/stream/series/tt1:1:1.json",
        json!({"streams": [
        {"name": "Fixture", "title": "Bluey.S01E01.1080p.WEB.x264-GROUP",
         "url": format!("{base}/e1.mkv")}]}),
    );
    fixture.set(
        "/meta/series/tt1.json",
        json!({"meta": {"id": "tt1", "type": "series", "name": "Bluey", "videos": [
            {"id": "tt1:1:1", "title": "One", "season": 1, "episode": 1,
             "released": "2020-01-01T00:00:00Z"},
            {"id": "tt1:1:2", "title": "Two", "season": 1, "episode": 2,
             "released": "2020-01-08T00:00:00Z"}]}}),
    );
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "series", "Bluey", &[]);
    let found = candidates(&kid, "series", "tt1", "tt1:1:1").await;
    *fixture.meta_delay.lock().unwrap() = Duration::from_millis(800);
    let mut play = target("series", "tt1", "tt1:1:1");
    (play.season, play.episode) = (Some(1), Some(1));
    let view = PlaybackView::open(&kid, found, play);
    ready(&view).await;
    // Taken off the list while the next episode's meta is on the way.
    remove(&kid, "tt1");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(view.snapshot().next, None);
}

#[tokio::test]
async fn a_stopped_playback_ends_its_body_and_its_url() {
    let fixture = Fixture::default();
    let base = origin(8 << 20, Duration::ZERO, Duration::from_millis(20)).await;
    fixture.set("/stream/movie/tt1.json", http_streams(&base));
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", &[]);
    let found = candidates(&kid, "movie", "tt1", "tt1").await;
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    let url = ready(&view).await;
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let local = mq_http::RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let mut body = http.stream(&url, None, &local).await.unwrap();
    let first = body.next_chunk().await.unwrap().unwrap();
    assert!(!first.is_empty());
    view.stop_forbidden(generation(&kid)).await;
    let mut read = first.len();
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(chunk)) = body.next_chunk().await {
            read += chunk.len();
        }
    })
    .await;
    assert!(ended.is_ok(), "the body ended");
    assert!(read < 8 << 20, "cut short at {read}");
    assert_eq!(view.snapshot().url, None);
    let again = http.stream(&url, None, &local).await.unwrap();
    assert_eq!(again.status, 404, "the URL is gone");
}

#[tokio::test]
async fn a_resolve_that_ends_after_the_stop_adopts_nothing() {
    let fixture = Fixture::default();
    let base = origin(64 * 1024, Duration::from_millis(800), Duration::ZERO).await;
    fixture.set("/stream/movie/tt1.json", http_streams(&base));
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", &[]);
    let found = candidates(&kid, "movie", "tt1", "tt1").await;
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    view.stop_forbidden(generation(&kid)).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let s = view.snapshot();
    assert_eq!((s.url, s.phase), (None, PlaybackPhase::Resolving));
    assert_eq!(kid.stream_server().await.unwrap().playbacks(), 0);
}

/// A title off the list writes nothing: no watched mark from its page, no
/// progress from its playback (they would sync, and show once allowed).
#[tokio::test]
async fn denied_views_write_no_progress_or_watched_rows() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    let page = TitleView::open(&kid, "movie", "tt2");
    assert!(page.denied());
    page.set_watched("tt2", true).unwrap();
    let play = PlaybackView::open(&kid, Vec::new(), target("movie", "tt2", "tt2"));
    assert!(play.denied());
    play.report_progress(60_000, 600_000, false);
    drop(play);
    let library = kid.library();
    assert!(
        library
            .watched_videos(kid.profile(), "tt2")
            .unwrap()
            .is_empty()
    );
    assert_eq!(library.progress(kid.profile(), "tt2").unwrap(), None);
}

/// An origin that counts the connections it gets and answers none.
async fn counter() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = count.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            drop(socket);
        }
    });
    (format!("http://{addr}"), count)
}

/// After the stop, a failover opens no other source: the same release from
/// a second origin (a debrid one would reach the owner's account) is never
/// asked, and the view stays as the stop left it.
#[tokio::test]
async fn a_stopped_playback_opens_no_other_source() {
    let fixture = Fixture::default();
    let base = origin(64 * 1024, Duration::ZERO, Duration::ZERO).await;
    let (spare, asked) = counter().await;
    fixture.set(
        "/stream/movie/tt1.json",
        json!({"streams": [
            {"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
             "url": format!("{base}/up.mkv")},
            {"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
             "url": format!("{spare}/up.mkv")}]}),
    );
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", &[]);
    let found = candidates(&kid, "movie", "tt1", "tt1").await;
    assert_eq!(found.len(), 2, "one release from two origins");
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    ready(&view).await;
    remove(&kid, "tt1");
    view.stop_forbidden(generation(&kid)).await;
    let stopped = view.snapshot();
    view.fail_over(None, None, None, "player error");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(view.snapshot(), stopped);
}

/// The stop ends the resolve: its subtitle work (the hash read still under
/// way) asks no addon after it.
#[tokio::test]
async fn a_stopped_playback_asks_no_addon_again() {
    let fixture = Fixture::default();
    let base = origin(
        8 << 20,
        Duration::from_millis(600),
        Duration::from_millis(20),
    )
    .await;
    fixture.set("/stream/movie/tt1.json", http_streams(&base));
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", &[]);
    let found = candidates(&kid, "movie", "tt1", "tt1").await;
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    ready(&view).await;
    remove(&kid, "tt1");
    view.stop_forbidden(generation(&kid)).await;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert_eq!(fixture.asked("/subtitles/"), Vec::<String>::new());
}

// Torrents (feature `torrent`): a loopback swarm with discovery off, the
// leecher installed as the device's engine.

#[cfg(feature = "torrent")]
#[path = "../../mq-engine/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

/// The swarm, its torrent known to the leecher (so the playback's own
/// `add_magnet` finds it with its peer), and a kid whose "Up" is it.
#[cfg(feature = "torrent")]
async fn torrent_kid() -> (
    support::Swarm,
    mq_engine::TorrentHandle,
    Core,
    Core,
    Vec<mq_streams::ParsedStream>,
) {
    let s = support::swarm(true).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = s
        .leecher
        .files(t)
        .unwrap()
        .iter()
        .position(|f| f.path.ends_with("movie.mkv"))
        .unwrap();
    let fixture = Fixture::default();
    fixture.set(
        "/stream/movie/tt1.json",
        json!({"streams": [{"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
                            "infoHash": s.info_hash, "fileIdx": movie}]}),
    );
    let (owner, kid) = kid(&fixture).await;
    kid.set_torrent_engine(s.leecher.clone());
    allow(&kid, "tt1", "movie", "Up", &[]);
    let found = candidates(&kid, "movie", "tt1", "tt1").await;
    (s, t, owner, kid, found)
}

#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_playback_pauses_its_torrent_and_an_allow_plays_it_again() {
    let (s, t, _owner, kid, found) = torrent_kid().await;
    let view = PlaybackView::open(&kid, found.clone(), target("movie", "tt1", "tt1"));
    ready(&view).await;
    remove(&kid, "tt1");
    view.stop_forbidden(generation(&kid)).await;
    let stats = s.leecher.stats(t).await.unwrap();
    assert!(stats.paused && stats.held, "{stats:?}");
    drop(view);
    // Allowed again: the new playback's own check is newer than the hold.
    allow(&kid, "tt1", "movie", "Up", &[]);
    let again = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    ready(&again).await;
    assert!(!s.leecher.stats(t).await.unwrap().held);
}

/// A torrent playback sends its rate to the engine: a guess from the
/// release at the start (1080p: 10 Mbit/s), then the file's size over the
/// duration the player reports (plans/streaming-window.md 2.2).
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_playback_sends_its_rate_to_the_engine() {
    let (s, t, _owner, kid, found) = torrent_kid().await;
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    ready(&view).await;
    // The adapter takes a rate from its mailbox: its stats show it a moment
    // after `set_rate` returns.
    let leecher = &s.leecher;
    let rate = |want: u64| async move {
        let mut last = None;
        for _ in 0..200 {
            last = leecher.stats(t).await.unwrap().bytes_per_s;
            if last == Some(want) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        last
    };
    assert_eq!(rate(1_250_000).await, Some(1_250_000));
    let key = view.snapshot().key.unwrap();
    view.report_duration(&key, 120_000);
    let exact = s.movie.len() as u64 * 1000 / 120_000;
    assert_eq!(
        rate(exact).await,
        Some(exact),
        "the player's duration never reached the engine"
    );
}

/// A torrent file larger than the room on a device under the window's
/// floor (plans/streaming-window.md 2.1) fails with "this device is full"
/// before its start is asked for; with room for it, it plays whole.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_on_a_full_device_is_refused_and_one_that_fits_plays_whole() {
    let (s, t, _owner, kid, found) = torrent_kid().await;
    kid.set_punches(true);
    kid.set_free_bytes(mq_engine::RESERVE_BYTES + 1024 * 1024);
    let view = PlaybackView::open(&kid, found.clone(), target("movie", "tt1", "tt1"));
    until("the playback failed", || {
        view.snapshot().phase == PlaybackPhase::Failed
    })
    .await;
    let attempts = view.snapshot().attempts;
    assert!(
        attempts.iter().any(|a| a.error == "this device is full"),
        "{attempts:?}"
    );
    assert!(s.leecher.stats(t).await.is_err(), "the torrent went");
    let (s, t, _owner, kid, found) = torrent_kid().await;
    kid.set_punches(true);
    kid.set_free_bytes(mq_engine::RESERVE_BYTES + 64 * 1024 * 1024);
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    ready(&view).await;
    assert_eq!(s.leecher.stats(t).await.unwrap().window_bytes, None);
}

/// Plans/player-polish.md 2b: a resume asks for its own point first. At
/// Ready the piece the resume reads first is in beside the head's, before
/// the player has read anything; a start that opened the head alone never
/// asked for it.
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resume_has_its_own_point_in_at_ready() {
    // Half way through ten minutes of a film.
    resume_reads_its_share("movie", "tt1", None, 600_000).await;
}

/// Review P1-1: an 80 minute episode resumes at its own share of the file,
/// where the episode rate floor (the file over an hour) would place it a
/// third too far.
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_80_minute_episode_resumes_at_its_share_of_the_file() {
    resume_reads_its_share("series", "tt1:1:1", Some((1, 1)), 4_800_000).await;
}

/// A play of `video_id` saved half way through `duration_ms` resumes 32 MiB
/// into the 64 MiB file: that piece and the head's are in at Ready.
#[cfg(feature = "test-hooks")]
async fn resume_reads_its_share(
    kind: &str,
    video_id: &str,
    episode: Option<(u32, u32)>,
    duration_ms: i64,
) {
    const MIB: u64 = 1 << 20;
    let s = support::swarm_big(64 << 20, 1 << 20).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let files = s.leecher.files(t).unwrap();
    let movie = files
        .iter()
        .position(|f| f.path.ends_with("movie.mkv"))
        .unwrap();
    let fixture = Fixture::default();
    fixture.set(
        &format!("/stream/{kind}/{video_id}.json"),
        json!({"streams": [{"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
                            "infoHash": s.info_hash, "fileIdx": movie}]}),
    );
    let (owner, _kid) = kid(&fixture).await;
    owner.set_torrent_engine(s.leecher.clone());
    let found = candidates(&owner, kind, "tt1", video_id).await;
    let progress = mq_library::ProgressRow {
        video_id,
        item_id: "tt1",
        position_ms: duration_ms / 2,
        duration_ms,
        updated_ms: 1,
    };
    owner
        .library()
        .set_progress(owner.profile(), progress, &owner.hlc())
        .unwrap();
    let play = PlayTarget {
        season: episode.map(|e| e.0),
        episode: episode.map(|e| e.1),
        ..target(kind, "tt1", video_id)
    };
    let view = PlaybackView::open(&owner, found, play);
    ready(&view).await;
    let offset = |file_at: u64| files[..movie].iter().map(|f| f.len).sum::<u64>() + file_at;
    let piece = |at: u64| (offset(at) / MIB) as u32;
    let have = |p: u32| {
        let leecher = s.leecher.clone();
        async move {
            mq_engine::test_hooks::piece_state(&leecher, t, p)
                .await
                .contains("Some((true,")
        }
    };
    assert!(have(piece(0)).await, "the head");
    assert!(have(piece(32 * MIB)).await, "the resume point");
}

/// Plans/player-polish.md 2b: a title whose last torrent this device holds
/// at its resume point says so, in its Versions row and its Continue
/// Watching caption; before it played here, nothing says so.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_title_kept_on_this_device_says_so() {
    let (s, _t, _owner, kid, found) = torrent_kid().await;
    let versions = || VersionsView::open_checked(&kid, params("movie", "tt1", ""), Some("tt1"));
    let before = settled(&versions()).await;
    assert_eq!(before.recommended.unwrap().on_device, None);
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    let url = ready(&view).await;
    let len = s.movie.len() as u64;
    assert!(read_range(&url, 0, len).await == s.movie, "the whole film");
    view.report_progress(60_000, 120_000, false);
    drop(view);
    let after = versions();
    until("the row says it is on this device", || {
        after
            .snapshot()
            .recommended
            .is_some_and(|r| r.on_device.as_deref() == Some("On this device"))
    })
    .await;
    let home = mq_runtime::HomeView::open(&kid);
    let caption = || {
        home.snapshot()
            .rows
            .iter()
            .find(|r| r.row_id == mq_runtime::views::home::CONTINUE_ROW_ID)
            .and_then(|r| r.tiles.iter().find(|t| t.id == "tt1").cloned())
            .and_then(|t| t.release_info)
    };
    until("the caption says it is on this device", || {
        caption().is_some_and(|c| c.ends_with(" \u{b7} On this device"))
    })
    .await;
    assert_eq!(
        caption().as_deref(),
        Some("1 min left \u{b7} On this device")
    );

    // A streaming account would take the title, so neither says it while
    // one is connected; both say it again once it goes, on the open views.
    kid.set_debrid_keys(&[(mq_runtime::StoreId::RealDebrid, "rd".into())]);
    until("the account takes the mark away", || {
        caption().as_deref() == Some("1 min left")
            && after
                .snapshot()
                .recommended
                .is_some_and(|r| r.on_device.is_none())
    })
    .await;
    kid.set_debrid_keys(&[]);
    until("the mark comes back", || {
        caption().is_some_and(|c| c.ends_with(" \u{b7} On this device"))
            && after
                .snapshot()
                .recommended
                .is_some_and(|r| r.on_device.as_deref() == Some("On this device"))
    })
    .await;
}

/// Plans/play-every-version.md 2a: with every source down, the version
/// this device keeps is offered as "Play from this device", and it plays
/// with no addon.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_every_source_down_the_kept_version_plays_from_this_device() {
    let (s, _t, owner, kid, found) = torrent_kid().await;
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    let url = ready(&view).await;
    let len = s.movie.len() as u64;
    assert!(read_range(&url, 0, len).await == s.movie, "the whole film");
    view.report_progress(60_000, 120_000, false);
    drop(view);
    // The one addon's port is closed: every source fails.
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut down = Fixture::default().addon().await;
    down.transport_url = format!("http://127.0.0.1:{port}/manifest.json");
    owner
        .library()
        .set_addons(OWNER, &[down], &owner.hlc())
        .unwrap();
    let versions = VersionsView::open_checked(&kid, params("movie", "tt1", ""), Some("tt1"));
    until("play from this device", || {
        versions
            .snapshot()
            .empty
            .is_some_and(|e| e.action == EmptyAction::PlayFromDevice)
    })
    .await;
    let empty = versions.snapshot().empty.unwrap();
    assert_eq!(empty.kind, EmptyKind::Unreachable);
    let key = empty.key.unwrap();
    let play = PlaybackView::open_picked(
        &kid,
        versions.pick(&key),
        target("movie", "tt1", "tt1"),
        &versions.plays(),
    );
    let url = ready(&play).await;
    assert!(
        read_range(&url, 0, len).await == s.movie,
        "from this device"
    );
}

/// Plans/player-polish.md 2b, after the app was closed: a core opened again
/// on the same data folder, with no engine started, says "On this device"
/// in Continue Watching from the folder alone, and starts no engine for it.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn after_a_relaunch_the_folder_alone_says_on_this_device() {
    let s = support::swarm(true).await;
    let data = tempfile::tempdir().unwrap();
    {
        let (_core, engine, t, _fixture, _) = played_here(&s, data.path()).await;
        // Closed: the torrent leaves the session and its bitfield is on disk.
        until_async("the torrent let go", || async {
            engine.remove(t, true).await.is_ok()
        })
        .await;
    }
    // The first launch's engine is gone (its folder lock with it), as when
    // the app was closed: a new one could start here. Its capacity sample
    // holds it for the 30 s after the start.
    let lock = data.path().join("torrents").join(".lock");
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    while !std::fs::File::open(&lock).is_ok_and(|f| f.try_lock().is_ok()) {
        assert!(
            std::time::Instant::now() < deadline,
            "the first engine lives on"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let again = Core::open(own_config(data.path())).unwrap();
    let home = mq_runtime::HomeView::open(&again);
    let caption = || {
        home.snapshot()
            .rows
            .iter()
            .find(|r| r.row_id == mq_runtime::views::home::CONTINUE_ROW_ID)
            .and_then(|r| r.tiles.iter().find(|t| t.id == "tt1").cloned())
            .and_then(|t| t.release_info)
    };
    until("the caption says it is on this device", || {
        caption().is_some_and(|c| c.ends_with(" \u{b7} On this device"))
    })
    .await;
    assert!(!again.engine_started(), "the check started the engine");
    // Review P2-1: a refresh applies the row once, with the mark: no update
    // shows the caption without it.
    use futures_util::StreamExt;
    let (_, mut updates) = home.subscribe();
    let other = mq_library::ProgressRow {
        video_id: "tt9",
        item_id: "tt9",
        position_ms: 1_000,
        duration_ms: 100_000,
        updated_ms: 2,
    };
    again
        .library()
        .set_progress(again.profile(), other, &again.hlc())
        .unwrap();
    let mut seen = Vec::new();
    let end = tokio::time::Instant::now() + Duration::from_secs(3);
    while let Ok(Some(batch)) = tokio::time::timeout_at(end, updates.next()).await {
        captions_of("tt1", &serde_json::to_value(&batch).unwrap(), &mut seen);
    }
    assert!(!seen.is_empty(), "the refresh never reached the row");
    assert!(
        seen.iter().all(|c| c.ends_with(" \u{b7} On this device")),
        "{seen:?}"
    );
    // Round 2 N5: with a streaming account the service may take it, as a
    // Versions row says: no mark.
    again.set_debrid_keys(&[(mq_runtime::StoreId::RealDebrid, "rd".into())]);
    let another = mq_library::ProgressRow {
        video_id: "tt8",
        item_id: "tt8",
        position_ms: 1_000,
        duration_ms: 100_000,
        updated_ms: 3,
    };
    again
        .library()
        .set_progress(again.profile(), another, &again.hlc())
        .unwrap();
    until("the mark gone with an account", || {
        caption().as_deref() == Some("1 min left")
    })
    .await;
}

/// Round 2 N4: a resume of a torrent kept here starts from the device with
/// the swarm gone: Ready from disk, at the saved share of the file.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_resume_starts_from_the_device_with_the_swarm_gone() {
    let s = support::swarm(true).await;
    let film = s.movie.clone();
    let data = tempfile::tempdir().unwrap();
    let (core, engine, t, _fixture, _) = played_here(&s, data.path()).await;
    until_async("the torrent let go", || async {
        engine.remove(t, true).await.is_ok()
    })
    .await;
    drop(s);
    let found = candidates(&core, "movie", "tt1", "tt1").await;
    let view = PlaybackView::open(&core, found, target("movie", "tt1", "tt1"));
    let url = ready(&view).await;
    assert_eq!(view.snapshot().resume_ms, Some(60_000));
    let at = film.len() as u64 / 2;
    let read = read_range(&url, at, 1 << 20).await;
    assert!(
        read == film[at as usize..at as usize + (1 << 20)],
        "the resume point"
    );
}

/// Round 2 N4: a resume waits for the head's blocking window as well as its
/// own point: kept here with the head lost and the swarm gone, it never
/// comes Ready from its point alone.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resume_waits_for_the_head_as_well() {
    let s = support::swarm(true).await;
    let data = tempfile::tempdir().unwrap();
    let (core, engine, t, _fixture, _) = played_here(&s, data.path()).await;
    until_async("the torrent let go", || async {
        engine.remove(t, true).await.is_ok()
    })
    .await;
    let hash = s.info_hash.clone();
    drop(s);
    // The first 2 MiB (pieces 0 to 7, the head's blocking window among
    // them) gone from the bitfield and the data; the resume point and the
    // tail stay.
    let dir = data.path().join("torrents");
    let have = dir.join(format!("{hash}.have"));
    let mut bits = std::fs::read(&have).unwrap();
    bits[0] = 0;
    std::fs::write(&have, bits).unwrap();
    let film = walk(&dir.join(&hash))
        .into_iter()
        .find(|p| p.ends_with("movie.mkv"))
        .unwrap();
    let mut bytes = std::fs::read(&film).unwrap();
    bytes[..1 << 20].fill(0);
    std::fs::write(&film, bytes).unwrap();
    let found = candidates(&core, "movie", "tt1", "tt1").await;
    let view = PlaybackView::open(&core, found, target("movie", "tt1", "tt1"));
    tokio::time::sleep(Duration::from_secs(5)).await;
    let s = view.snapshot();
    assert_eq!(
        s.phase,
        PlaybackPhase::Resolving,
        "Ready with no head: {s:?}"
    );
}

/// Every file under `dir`.
#[cfg(feature = "torrent")]
fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir).map_or(Vec::new(), |entries| {
        entries
            .flatten()
            .flat_map(|e| {
                let path = e.path();
                if path.is_dir() {
                    walk(&path)
                } else {
                    vec![path]
                }
            })
            .collect()
    })
}

/// Every `releaseInfo` of a tile `id` anywhere in `value`.
#[cfg(feature = "torrent")]
fn captions_of(id: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(o) => {
            if o.get("id").and_then(Value::as_str) == Some(id)
                && let Some(c) = o.get("releaseInfo").and_then(Value::as_str)
            {
                out.push(c.to_owned());
            }
            o.values().for_each(|v| captions_of(id, v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| captions_of(id, v, out)),
        _ => {}
    }
}

/// Review P1-1: a start of a torrent the device kept that fails (here its
/// file is larger than the version lists) leaves the torrent's folder, its
/// details and its bitfield: the title stays on the device.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_start_of_a_kept_torrent_keeps_what_the_device_held() {
    let s = support::swarm(true).await;
    let data = tempfile::tempdir().unwrap();
    let (core, _engine, _t, fixture, movie) = played_here(&s, data.path()).await;
    fixture.set(
        "/stream/movie/tt2.json",
        json!({"streams": [{"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
                            "infoHash": s.info_hash, "fileIdx": movie,
                            "behaviorHints": {"videoSize": 1000}}]}),
    );
    let found = candidates(&core, "movie", "tt2", "tt2").await;
    let view = PlaybackView::open(&core, found, target("movie", "tt2", "tt2"));
    until("the start failed", || {
        view.snapshot().phase == PlaybackPhase::Failed
    })
    .await;
    let attempts = view.snapshot().attempts;
    assert!(
        attempts
            .iter()
            .any(|a| a.error.contains("larger than the version")),
        "{attempts:?}"
    );
    let dir = data.path().join("torrents");
    assert!(dir.join(&s.info_hash).is_dir(), "the folder");
    assert!(
        dir.join(format!("{}.torrent", s.info_hash)).exists(),
        "the details"
    );
    assert_eq!(
        mq_engine::cached_on_disk(&dir, &s.info_hash, movie, 0),
        Some(mq_engine::Cached { ready: true }),
        "the bitfield"
    );
}

/// A core on its own data folder, as an app opens it.
#[cfg(feature = "torrent")]
fn own_config(data: &std::path::Path) -> mq_runtime::CoreConfig {
    mq_runtime::CoreConfig {
        data_dir: data.into(),
        profile_id: OWNER.into(),
        device_id: 0,
    }
}

/// A core on `data` whose engine, on the core's own torrent folder as the
/// app's, played the swarm's film whole as tt1, left half way; the fixture
/// serving its streams, and the film's file index.
#[cfg(feature = "torrent")]
async fn played_here(
    s: &support::Swarm,
    data: &std::path::Path,
) -> (
    Core,
    mq_engine::Engine,
    mq_engine::TorrentHandle,
    Fixture,
    usize,
) {
    let core = Core::open(own_config(data)).unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let ip = std::net::Ipv4Addr::LOCALHOST.into();
    let config = mq_engine::EngineConfig {
        discovery: false,
        upload: false,
        ..mq_engine::EngineConfig::new(data.join("torrents"), http.torrent_scope(&[ip]))
    };
    let engine = mq_engine::Engine::start(config).await.unwrap();
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .files(t)
        .unwrap()
        .iter()
        .position(|f| f.path.ends_with("movie.mkv"))
        .unwrap();
    core.set_torrent_engine(engine.clone());
    let fixture = Fixture::default();
    fixture.set(
        "/stream/movie/tt1.json",
        json!({"streams": [{"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
                            "infoHash": s.info_hash, "fileIdx": movie}]}),
    );
    let addon = fixture.addon().await;
    core.library()
        .set_addons(OWNER, &[addon], &core.hlc())
        .unwrap();
    let found = candidates(&core, "movie", "tt1", "tt1").await;
    let view = PlaybackView::open(&core, found, target("movie", "tt1", "tt1"));
    let url = ready(&view).await;
    let len = s.movie.len() as u64;
    assert!(read_range(&url, 0, len).await == s.movie, "the whole film");
    view.report_progress(60_000, 120_000, false);
    drop(view);
    (core, engine, t, fixture, movie)
}

/// A resume on a small device (review P1-2): the torrent's own folder
/// counts in R, so what the trim left is room the resume plays in. With
/// free space short of the window's floor but for that folder, the second
/// playback opens a window and reads the resume region from disk.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resume_counts_its_own_folder_in_the_room() {
    const MIB: u64 = 1 << 20;
    let s = support::swarm_big(256 << 20, 1 << 20).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = s
        .leecher
        .files(t)
        .unwrap()
        .iter()
        .position(|f| f.path.ends_with("movie.mkv"))
        .unwrap();
    let fixture = Fixture::default();
    fixture.set(
        "/stream/movie/tt1.json",
        json!({"streams": [{"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
                            "infoHash": s.info_hash, "fileIdx": movie}]}),
    );
    let (owner, _kid) = kid(&fixture).await;
    owner.set_torrent_engine(s.leecher.clone());
    owner.set_punches(true);
    let found = candidates(&owner, "movie", "tt1", "tt1").await;
    let len = s.movie.len() as u64;
    // The first play: a window in 200 MiB of room, 16 MiB read 40% in.
    owner.set_free_bytes(mq_engine::RESERVE_BYTES + 200 * MIB);
    let view = PlaybackView::open(&owner, found.clone(), target("movie", "tt1", "tt1"));
    let url = ready(&view).await;
    assert!(s.leecher.stats(t).await.unwrap().window_bytes.is_some());
    let (pos_ms, duration_ms) = (400_000, 1_000_000);
    let at = len * pos_ms / duration_ms;
    let have = |p: u32| {
        let leecher = s.leecher.clone();
        async move {
            mq_engine::test_hooks::piece_state(&leecher, t, p)
                .await
                .contains("Some((true,")
        }
    };
    // The head read through piece 6 (6 MiB in, past the head's pinned
    // window), then 16 MiB 40% in.
    assert!(read_range(&url, 0, 8 * MIB).await == s.movie[..(8 * MIB) as usize]);
    assert!(have(6).await, "piece 6 held before the trim");
    assert!(read_range(&url, at, 16 * MIB).await == s.movie[at as usize..(at + 16 * MIB) as usize]);
    view.report_progress(pos_ms, duration_ms, false);
    drop(view);
    until_async("the trim", || async {
        s.leecher.stats(t).await.unwrap().window_bytes.is_none()
    })
    .await;
    let own = folder_bytes(&s.leech_dir().join(&s.info_hash));
    assert!(!have(6).await, "the trim took piece 6");
    // A resume with room over both floors (the 1080p guess's and the
    // duration's): the window's rate is the file over the last session's
    // duration, not the guess (review P2-2).
    owner.set_free_bytes(mq_engine::RESERVE_BYTES + 200 * MIB - own);
    let view = PlaybackView::open(&owner, found.clone(), target("movie", "tt1", "tt1"));
    ready(&view).await;
    let stats = s.leecher.stats(t).await.unwrap();
    assert!(stats.window_bytes.is_some());
    assert_eq!(stats.bytes_per_s, Some(len * 1000 / duration_ms));
    drop(view);
    until_async("the trim", || async {
        s.leecher.stats(t).await.unwrap().window_bytes.is_none()
    })
    .await;
    let own = folder_bytes(&s.leech_dir().join(&s.info_hash));
    // Free space short of the floor plus 64 MiB by all but 4 MiB of what
    // the torrent holds.
    let floor = mq_engine::window_plan(
        mq_engine::DeviceClass::Standard,
        mq_engine::WindowInputs {
            free: 1 << 40,
            reclaimable: 0,
            committed: 0,
            bitrate: len * 1000 / duration_ms,
            piece_len: MIB,
            fill: 0,
            ..Default::default()
        },
    )
    .unwrap()
    .floor;
    assert!(own > 8 * MIB, "the trim kept {own}");
    owner.set_free_bytes(mq_engine::RESERVE_BYTES + floor + 68 * MIB - own);
    let view = PlaybackView::open(&owner, found, target("movie", "tt1", "tt1"));
    let url = ready(&view).await;
    assert!(s.leecher.stats(t).await.unwrap().window_bytes.is_some());
    // Before the player's read: the head read was a probe, so nothing past
    // the head's pinned window was wanted again at byte 0 (review P2-2).
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!have(6).await, "the head's windows were fetched again");
    assert!(read_range(&url, at, 8 * MIB).await == s.movie[at as usize..(at + 8 * MIB) as usize]);
    assert_eq!(s.leecher.stats(t).await.unwrap().refetched_bytes, 0);
}

/// A season pack (review r2 P1-1): the played episode's bytes are room for
/// the next episode's window, whose keeper evicts them first, never for it
/// whole. Where the volume cannot punch holes, the next episode that fits
/// only by counting them is hidden, and refused when opened.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_packs_played_episode_is_room_for_the_next_only_in_a_window() {
    const MIB: u64 = 1 << 20;
    let s = support::swarm_pack(&[("e01.mkv", 160 << 20), ("e02.mkv", 160 << 20)], 1 << 20).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let files = s.leecher.files(t).unwrap();
    let idx = |name: &str| files.iter().position(|f| f.path.ends_with(name)).unwrap();
    let fixture = Fixture::default();
    for (id, name) in [("tt1", "e01.mkv"), ("tt2", "e02.mkv")] {
        fixture.set(
            &format!("/stream/movie/{id}.json"),
            json!({"streams": [{"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP",
                                "infoHash": s.info_hash, "fileIdx": idx(name),
                                "behaviorHints": {"videoSize": 160u64 << 20}}]}),
        );
    }
    let (owner, _kid) = kid(&fixture).await;
    owner.set_torrent_engine(s.leecher.clone());
    owner.set_punches(true);
    let first = candidates(&owner, "movie", "tt1", "tt1").await;
    let next = candidates(&owner, "movie", "tt2", "tt2").await;
    // Episode 1 plays whole, 140 MiB of it read (not finished, so the
    // seeder stays connected for episode 2).
    let play_first = || async {
        owner.set_free_bytes(mq_engine::RESERVE_BYTES + 1024 * MIB);
        let view = PlaybackView::open(&owner, first.clone(), target("movie", "tt1", "tt1"));
        let url = ready(&view).await;
        let t = s.leecher.add_magnet(&s.info_hash, &[], &[]).await.unwrap();
        assert_eq!(s.leecher.stats(t).await.unwrap().window_bytes, None);
        assert!(read_range(&url, 0, 140 * MIB).await == s.movie[..(140 * MIB) as usize]);
    };
    play_first().await;
    // Free space 40 MiB over the reserve: episode 2 (160 MiB) fits only by
    // counting episode 1's 140 MiB. Maintenance counts the folder idle.
    let free = mq_engine::RESERVE_BYTES + 40 * MIB;
    tokio::time::sleep(Duration::from_secs(6)).await;
    // A volume that cannot punch holes: hidden, and refused when opened.
    let (flat, _) = kid(&fixture).await;
    flat.set_torrent_engine(s.leecher.clone());
    flat.set_punches(false);
    flat.set_free_bytes(free);
    let versions = VersionsView::open_checked(&flat, params("movie", "tt2", ""), Some("tt2"));
    let shown = settled(&versions).await;
    assert!(shown.recommended.is_none(), "{:?}", shown.recommended);
    // Hidden silently, with the one storage line (play-every-version 2c).
    assert_eq!(shown.hidden.count, 0, "{:?}", shown.hidden);
    assert!(shown.blocked.is_none(), "{:?}", shown.blocked);
    assert_eq!(shown.empty.map(|e| e.kind), Some(EmptyKind::NoRoom));
    let refused = PlaybackView::open(&flat, next.clone(), target("movie", "tt2", "tt2"));
    until("the playback failed", || {
        refused.snapshot().phase == PlaybackPhase::Failed
    })
    .await;
    let attempts = refused.snapshot().attempts;
    assert!(
        attempts.iter().any(|a| a.error == "this device is full"),
        "{attempts:?}"
    );
    drop(refused);
    // The refusal let the torrent go but kept its folder: episode 1's bytes
    // stay (review r3 B P3-1).
    tokio::time::sleep(Duration::from_millis(500)).await;
    let kept = folder_bytes(&s.leech_dir().join(&s.info_hash));
    assert!(kept >= 140 * MIB, "the refusal kept the folder: {kept}");
    // Episode 1 again, then where holes punch: a window, which evicts
    // episode 1 first.
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    play_first().await;
    owner.set_free_bytes(free);
    let view = PlaybackView::open(&owner, next, target("movie", "tt2", "tt2"));
    let url = ready(&view).await;
    assert!(s.leecher.stats(t).await.unwrap().window_bytes.is_some());
    let e02 = &s.others[0];
    assert!(read_range(&url, 0, 100 * MIB).await == e02[..(100 * MIB) as usize]);
    // Piece 180 is episode 1's (the torrent lists episode 2 first).
    let state = mq_engine::test_hooks::piece_state(&s.leecher, t, 180).await;
    assert!(
        !state.contains("Some((true,"),
        "episode 1 went first: {state}"
    );
}

/// Phase 3 (plans/streaming-window.md 3.3): where windows play, a version
/// too big for the free space shows with the buffer line, its minutes ahead
/// on Wi-Fi and none on mobile data, where the engine keeps the device
/// preset.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_version_too_big_for_the_free_space_says_its_minutes_ahead() {
    use mq_runtime::speed::Network;
    use mq_streams::ReasonLine;
    let s = support::swarm(true).await;
    let fixture = Fixture::default();
    fixture.set(
        "/stream/movie/tt3.json",
        json!({"streams": [{"name": "Fixture", "title": "Up.2009.1080p.WEB.x264-GROUP\n👤 300 💾 2.5 GB",
                            "infoHash": "a".repeat(40), "fileIdx": 0}]}),
    );
    let (owner, _kid) = kid(&fixture).await;
    owner.set_torrent_engine(s.leecher.clone());
    owner.set_punches(true);
    owner.set_free_bytes(mq_engine::RESERVE_BYTES + (1 << 30));
    owner.set_network(Network::Wifi);
    let versions = VersionsView::open_checked(&owner, params("movie", "tt3", ""), Some("tt3"));
    let minutes = |snap: &mq_runtime::VersionsSnapshot| {
        let row = snap
            .recommended
            .clone()
            .or_else(|| snap.rows.first().cloned());
        match row.map(|r| r.line) {
            Some(ReasonLine::Buffer { ahead_min, .. }) => Ok(ahead_min),
            other => Err(format!("{other:?} ({:?})", snap.hidden)),
        }
    };
    let on_wifi = minutes(&settled(&versions).await).unwrap();
    assert!(
        on_wifi.is_some_and(|m| (2..=10).contains(&m)),
        "{on_wifi:?}"
    );
    // A torrent playing on the engine at 4 MiB/s, readahead by Wi-Fi.
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = s
        .leecher
        .select_file(t, "default", &mq_engine::FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    s.leecher.set_rate(t, movie, 4 << 20, false).await.unwrap();
    let readahead = || async { s.leecher.stats(t).await.unwrap().readahead_bytes };
    until_async("the Wi-Fi readahead", || async {
        readahead().await.is_some()
    })
    .await;
    let on_wifi_ahead = readahead().await.unwrap();
    // The device moves to mobile data with the view open: the same row now
    // promises no minutes (review r1 B P2-1), and the playing torrent keeps
    // only the device preset ahead (B P3-5); back on Wi-Fi both return.
    owner.on_network_changed(Some(Network::Cellular));
    until("the row ranked for mobile data", || {
        minutes(&versions.snapshot()) == Ok(None)
    })
    .await;
    until_async("the mobile preset", || async {
        readahead().await.is_some_and(|r| r < on_wifi_ahead)
    })
    .await;
    owner.on_network_changed(Some(Network::Wifi));
    until("the row ranked for Wi-Fi", || {
        minutes(&versions.snapshot()) == Ok(on_wifi)
    })
    .await;
}

/// `len` bytes of `url` from `at`.
#[cfg(feature = "torrent")]
async fn read_range(url: &str, at: u64, len: u64) -> Vec<u8> {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let local = mq_http::RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let range = Some(format!("bytes={at}-{}", at + len - 1));
    let mut body = http.stream(url, range, &local).await.unwrap();
    let mut got = Vec::new();
    while (got.len() as u64) < len {
        let chunk = tokio::time::timeout(Duration::from_secs(30), body.next_chunk())
            .await
            .expect("a chunk in 30 s")
            .expect("more bytes")
            .unwrap();
        got.extend_from_slice(&chunk);
    }
    got
}

/// What a folder takes on disk.
#[cfg(feature = "torrent")]
fn folder_bytes(dir: &std::path::Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| {
                let m = e.metadata().unwrap();
                if m.is_dir() {
                    folder_bytes(&e.path())
                } else {
                    m.blocks() * 512
                }
            })
            .sum()
    })
}

/// Waits up to 30 s for `f` to answer true.
#[cfg(feature = "torrent")]
async fn until_async<F: std::future::Future<Output = bool>>(what: &str, mut f: impl FnMut() -> F) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !f().await {
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: not within 30 s"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Whether `url` serves its first bytes within 10 s.
#[cfg(feature = "torrent")]
async fn serves(url: &str) -> bool {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let local = mq_http::RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let read = async {
        let mut body = http.stream(url, None, &local).await.ok()?;
        if !(200..300).contains(&body.status) {
            return None;
        }
        body.next_chunk().await?.ok().filter(|c| !c.is_empty())
    };
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// The kid's hold binds the kid only: the owner, and a second kids profile
/// with the title on its list, play the held release at the same access
/// generation.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn others_play_a_release_held_for_the_kid() {
    let (s, t, owner, kid, found) = torrent_kid().await;
    let other = owner.for_profile(&owner.create_kids_profile("Other", "owl").unwrap());
    allow(&other, "tt1", "movie", "Up", &[]);
    let view = PlaybackView::open(&kid, found.clone(), target("movie", "tt1", "tt1"));
    ready(&view).await;
    remove(&kid, "tt1");
    let removed = generation(&kid);
    view.stop_forbidden(removed).await;
    drop(view);
    assert!(s.leecher.stats(t).await.unwrap().held);
    for core in [&owner, &other] {
        assert_eq!(generation(core), removed, "nothing committed since");
        let play = PlaybackView::open(core, found.clone(), target("movie", "tt1", "tt1"));
        let url = ready(&play).await;
        assert!(serves(&url).await, "{}", core.profile());
    }
    assert!(
        s.leecher.stats(t).await.unwrap().held,
        "the kid's hold stays"
    );
}

/// The owner is already playing the release when the kid's playback of it
/// is stopped: the torrent is not paused and the owner's goes on.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_owners_playback_goes_on_when_the_kids_is_stopped() {
    let (s, t, owner, kid, found) = torrent_kid().await;
    let mine = PlaybackView::open(&owner, found.clone(), target("movie", "tt1", "tt1"));
    let url = ready(&mine).await;
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    ready(&view).await;
    remove(&kid, "tt1");
    view.stop_forbidden(generation(&kid)).await;
    let stats = s.leecher.stats(t).await.unwrap();
    assert!(stats.held && !stats.paused, "{stats:?}");
    assert!(serves(&url).await);
    assert_eq!(mine.snapshot().phase, PlaybackPhase::Ready);
}

/// A kid's playback whose check is older than the hold (the removal
/// committed after it read the list) fails with `Held` while the owner
/// plays the same release: the torrent, its hold and its files stay, and
/// the owner's playback goes on.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stale_held_failure_removes_nothing() {
    let (s, t, owner, kid, found) = torrent_kid().await;
    let mine = PlaybackView::open(&owner, found.clone(), target("movie", "tt1", "tt1"));
    let url = ready(&mine).await;
    // The watcher's pause for a removal this playback's check did not see.
    let later = generation(&kid) + 1;
    assert!(s.leecher.pause_now(t, kid.profile(), later).await.unwrap());
    let stale = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    until("the stale playback failed", || {
        stale.snapshot().phase == PlaybackPhase::Failed
    })
    .await;
    let stats = s
        .leecher
        .stats(t)
        .await
        .expect("the torrent is still there");
    assert!(stats.held, "{stats:?}");
    assert!(serves(&url).await);
}

/// Taken off the list while the playback is still setting its torrent up
/// (before the head is ready): the torrent is paused and held all the same.
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_playback_stopped_during_setup_holds_its_torrent() {
    let (s, t, _owner, kid, found) = torrent_kid().await;
    let hold = mq_engine::test_hooks::hold_ranges(&s.leecher);
    let view = PlaybackView::open(&kid, found, target("movie", "tt1", "tt1"));
    tokio::time::timeout(Duration::from_secs(30), hold.reached())
        .await
        .expect("the head range is being set up");
    remove(&kid, "tt1");
    view.stop_forbidden(generation(&kid)).await;
    hold.open();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let stats = s.leecher.stats(t).await.unwrap();
    assert!(stats.paused && stats.held, "{stats:?}");
    assert_eq!(view.snapshot().url, None);
}

/// The corpus bar (plans/m11-kids.md 4.4): the 30 most popular family
/// movies and 20 family series in Cinemeta's catalogs, allowed for a kid
/// with the real meta and stream addons (Cinemeta and Torrentio). Prints
/// one JSON line per title: what the kid can play next to what an open
/// profile can. Run by hand; the numbers go to bench/m11.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "network: Cinemeta and Torrentio"]
async fn corpus_of_fifty_family_titles() {
    let owner = Core::temporary(OWNER).unwrap();
    owner.set_pin(OWNER, Some(PIN), None).await.unwrap();
    owner
        .install_addon("https://v3-cinemeta.strem.io/manifest.json")
        .await
        .unwrap();
    owner
        .install_addon("https://torrentio.strem.fun/manifest.json")
        .await
        .unwrap();
    let kid_id = owner.create_kids_profile("Kid", "fox").unwrap();
    let kid = owner.for_profile(&kid_id);
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let mut titles: Vec<(String, String, String)> = Vec::new();
    for (kind, take) in [("movie", 30), ("series", 20)] {
        let url = format!("https://v3-cinemeta.strem.io/catalog/{kind}/top/genre=Family.json");
        let body = http.get(&url, &Default::default()).await.unwrap().body;
        let list: Value = serde_json::from_slice(&body).unwrap();
        for m in list["metas"].as_array().unwrap().iter().take(take) {
            titles.push((
                kind.into(),
                m["id"].as_str().unwrap().into(),
                m["name"].as_str().unwrap_or_default().into(),
            ));
        }
    }
    let count = |view: &VersionsView| {
        let s = view.snapshot();
        s.recommended.iter().count() + s.rows.len()
    };
    for (kind, id, name) in titles {
        let allowed = owner.allow_title(&kid_id, &kind, &id).await;
        let video = if kind == "series" {
            let meta = owner.fetch_meta("series", &id).await.ok().map(|m| m.0);
            meta.and_then(|m| {
                m.videos
                    .iter()
                    .filter(|v| v.series_info.as_ref().is_some_and(|s| s.season > 0))
                    .min_by_key(|v| {
                        let s = v.series_info.as_ref().unwrap();
                        (s.season, s.episode)
                    })
                    .map(|v| v.id.clone())
            })
        } else {
            Some(id.clone())
        };
        let Some(video) = video else {
            println!(
                "{}",
                json!({"id": id, "kind": kind, "name": name, "error": "no episode"})
            );
            continue;
        };
        // One view at a time, paced: Torrentio limits how often it is asked.
        let open = VersionsView::open(&owner, params(&kind, &video, &name));
        answered(&open).await;
        let kids = VersionsView::open_checked(&kid, params(&kind, &video, ""), Some(&id));
        answered(&kids).await;
        let hidden = kids.snapshot().hidden.by_reason;
        let allowed = allowed
            .as_ref()
            .err()
            .map_or("ok", |e| e.code().unwrap_or("error"));
        println!(
            "{}",
            json!({"id": id, "kind": kind, "name": name, "video": video,
                   "allowed": allowed, "denied": kids.denied(),
                   "open": count(&open), "kids": count(&kids),
                   "openFailed": open.snapshot().failed_addons,
                   "kidsFailed": kids.snapshot().failed_addons,
                   "kidsHidden": serde_json::to_value(&hidden).unwrap()})
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Waits until every addon answered, at most 40 s (real addons).
async fn answered(view: &VersionsView) {
    let started = std::time::Instant::now();
    while !view.snapshot().done && started.elapsed() < Duration::from_secs(40) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
