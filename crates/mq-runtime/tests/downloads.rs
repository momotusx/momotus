//! Offline downloads (plans/offline-downloads.md 6.1): HTTP downloads,
//! their queue, and playing them with no network.

use mq_library::AddonRow;
use mq_runtime::downloads::{AddRequest, DownloadReason, DownloadRow, DownloadState, Link, Scope};
use mq_runtime::{Core, PlayTarget, PlaybackPhase, PlaybackView, VersionsParams, VersionsView};
use mq_streams::{DeviceCaps, StreamPrefs, Target};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A playable MP4 grown with a `free` box to `len` bytes, so a download
/// passes several checkpoints.
fn video(len: usize) -> Arc<Vec<u8>> {
    let mut bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mq-media/tests/fixtures/h264.mp4"
    ))
    .unwrap();
    let pad = len - bytes.len();
    bytes.extend_from_slice(&(pad as u32).to_be_bytes());
    bytes.extend_from_slice(b"free");
    bytes.extend((0..pad - 8).map(|i| (i % 251) as u8));
    Arc::new(bytes)
}

/// How the test origin answers.
#[derive(Clone, Default)]
struct How {
    /// The first answer stops after this many body bytes.
    cut_first_after: Option<usize>,
    /// Every answer is 200 with the whole file.
    ignore_ranges: bool,
    /// An HTML page instead of the file.
    html: bool,
    /// The body in 64 KiB pieces 20 ms apart.
    slow: bool,
    /// `slow`'s pieces this far apart instead (perf).
    pace: Option<Duration>,
    /// Every answer after the first serves this file instead: a link
    /// resolved again that names another file.
    then: Option<Arc<Vec<u8>>>,
    /// Every answer is this status with no body.
    status: Option<u16>,
    /// A request without `Authorization` set to this is answered 401.
    auth: Option<&'static str>,
}

#[derive(Clone, Default)]
struct Seen {
    ranges: Arc<Mutex<Vec<String>>>,
    answers: Arc<AtomicUsize>,
}

/// An origin serving `data` at `/video.mp4` with single ranges.
async fn origin(data: Arc<Vec<u8>>, how: How) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Seen::default();
    let out = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let (data, seen, how) = (data.clone(), out.clone(), how.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let nth = seen.answers.fetch_add(1, Ordering::SeqCst);
                let data = how.then.clone().filter(|_| nth > 0).unwrap_or(data);
                if how.html {
                    // 20 MB of page: the head check stops it at its start.
                    let page = "<!doctype html><html><body>expired</body></html>".repeat(400_000);
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                        page.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    return;
                }
                let range = head
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .map(str::to_owned);
                seen.ranges
                    .lock()
                    .unwrap()
                    .push(range.clone().unwrap_or_default());
                if let Some(auth) = how.auth
                    && !head.contains(&format!("authorization: {}", auth.to_ascii_lowercase()))
                {
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 401 No\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                    return;
                }
                if let Some(code) = how.status {
                    let resp = format!(
                        "HTTP/1.1 {code} No\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    return;
                }
                let start: usize = range
                    .as_deref()
                    .and_then(|r| r.split('-').next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let (status, from) = if how.ignore_ranges || range.is_none() {
                    ("200 OK".to_owned(), 0)
                } else {
                    ("206 Partial Content".to_owned(), start)
                };
                let body = &data[from..];
                let cut = match how.cut_first_after {
                    Some(n) if nth == 0 => n.min(body.len()),
                    _ => body.len(),
                };
                // A cut answer names no length: its body just ends early,
                // as a connection the origin closed does.
                let length = if cut < body.len() && status.starts_with("206") {
                    String::new()
                } else {
                    format!("Content-Length: {}\r\n", body.len())
                };
                let mut head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: video/mp4\r\n{length}Connection: close\r\n"
                );
                if status.starts_with("206") {
                    head.push_str(&format!(
                        "Content-Range: bytes {from}-{}/{}\r\n",
                        data.len() - 1,
                        data.len()
                    ));
                }
                head.push_str("\r\n");
                let _ = socket.write_all(head.as_bytes()).await;
                if how.slow {
                    for chunk in body[..cut].chunks(64 << 10) {
                        if socket.write_all(chunk).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(how.pace.unwrap_or(Duration::from_millis(20))).await;
                    }
                    return;
                }
                let _ = socket.write_all(&body[..cut]).await;
            });
        }
    });
    (format!("http://{addr}/video.mp4"), seen)
}

const ITEM: &str = "tt1727587";

/// Subtitle requests the test addons got.
/// Subtitle requests each test addon got, by its port: tests run at once.
static SUBTITLE_ASKS: Mutex<std::collections::BTreeMap<u16, usize>> =
    Mutex::new(std::collections::BTreeMap::new());

/// The subtitle requests `row`'s addon got.
fn asks(row: &AddonRow) -> usize {
    let port = url::Url::parse(&row.transport_url).unwrap().port().unwrap();
    SUBTITLE_ASKS
        .lock()
        .unwrap()
        .get(&port)
        .copied()
        .unwrap_or(0)
}

/// One addon answering meta and streams: Sintel with one direct stream, or
/// a series with two episodes. `delay` holds every stream answer.
async fn addon(stream_url: &str, delay: Duration) -> AddonRow {
    let stream = serde_json::json!({
        "name": "Direct\n1080p",
        "title": "Sintel.2010.1080p.WEB-DL.mp4",
        "url": stream_url,
        "behaviorHints": { "filename": "Sintel.2010.1080p.WEB-DL.mp4" }
    });
    addon_listing(stream, delay).await
}

/// [`addon`] listing `stream` as Sintel's one stream.
async fn addon_listing(stream: serde_json::Value, delay: Duration) -> AddonRow {
    addon_serving(stream, delay, Extras::default()).await
}

/// What a test addon also serves.
#[derive(Default, Clone)]
struct Extras {
    /// Subtitles it lists, each `(id, lang, forced)`: its file at
    /// `/subfile/<id>.srt` says the id.
    subtitles: Vec<(&'static str, &'static str, bool)>,
    /// A name that resolves public to this server (`test_dns`): the meta's
    /// poster is `http://<name>:<port>/poster.jpg`.
    public_name: Option<&'static str>,
    /// Its subtitles answer 503: no source answers the subtitle step.
    subtitles_fail: bool,
}

/// The poster a test addon serves.
const POSTER: &[u8] = b"\xFF\xD8\xFF\xE0 a poster";

async fn addon_serving(stream: serde_json::Value, delay: Duration, extras: Extras) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let streams = serde_json::json!({ "streams": [stream] }).to_string();
    let poster = match extras.public_name {
        Some(name) => {
            mq_http::test_dns::set_public(name, &[std::net::Ipv4Addr::LOCALHOST.into()]);
            format!("http://{name}:{}/poster.jpg", addr.port())
        }
        None => "https://images.example/sintel.jpg".to_owned(),
    };
    let subtitles =
        serde_json::json!({ "subtitles": extras.subtitles.iter().map(|(id, lang, forced)| {
        serde_json::json!({ "id": id, "lang": lang, "forced": forced,
                            "url": format!("http://{addr}/subfile/{id}.srt") })
    }).collect::<Vec<_>>() })
        .to_string();
    let meta = serde_json::json!({ "meta": {
        "id": ITEM, "type": "movie", "name": "Sintel", "runtime": "15 min",
        "releaseInfo": "2010", "poster": poster
    }})
    .to_string();
    let series = serde_json::json!({ "meta": {
        "id": "tt9", "type": "series", "name": "Show", "runtime": "50 min",
        "videos": [
            { "id": "tt9:2:1", "title": "One", "season": 2, "episode": 1,
              "released": "2020-01-01T00:00:00.000Z" },
            { "id": "tt9:2:2", "title": "Two", "season": 2, "episode": 2,
              "released": "2020-01-08T00:00:00.000Z" },
            { "id": "tt9:3:1", "title": "Later", "season": 3, "episode": 1,
              "released": "2999-01-01T00:00:00.000Z" }
        ]
    }})
    .to_string();
    let subtitles_fail = extras.subtitles_fail;
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let (streams, meta, series, subtitles) = (
                streams.clone(),
                meta.clone(),
                series.clone(),
                subtitles.clone(),
            );
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                if head.contains("/subtitles/") {
                    *SUBTITLE_ASKS
                        .lock()
                        .unwrap()
                        .entry(addr.port())
                        .or_default() += 1;
                    if subtitles_fail {
                        let _ = socket
                            .write_all(
                                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
                            )
                            .await;
                        return;
                    }
                }
                if head.starts_with("GET /poster.jpg") {
                    let mut resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                        POSTER.len()
                    )
                    .into_bytes();
                    resp.extend_from_slice(POSTER);
                    let _ = socket.write_all(&resp).await;
                    return;
                }
                if let Some(rest) = head.strip_prefix("GET /subfile/") {
                    let id = rest.split(".srt").next().unwrap_or_default();
                    let srt = format!("1\n00:00:01,000 --> 00:00:02,000\nline of {id}\n");
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{srt}",
                        srt.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    return;
                }
                let body = if head.contains("/subtitles/") {
                    subtitles
                } else if head.contains("/meta/series/") {
                    series
                } else if head.contains("/meta/") {
                    meta
                } else {
                    tokio::time::sleep(delay).await;
                    streams
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "local", "version": "1.0.0", "name": "Local",
        "types": ["movie", "series"], "resources": ["meta", "stream", "subtitles"], "catalogs": [],
        "idPrefixes": ["tt"]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

async fn core_with(row: AddonRow) -> Core {
    let core = Core::temporary("default").unwrap();
    core.set_free_bytes(100 << 30);
    core.library()
        .set_addons(core.profile(), &[row], &core.hlc())
        .unwrap();
    // Wi-Fi, unmetered: nothing holds the queue.
    core.downloads().set_link(Link::default());
    core
}

fn movie() -> AddRequest {
    AddRequest {
        kind: "movie".into(),
        item_id: ITEM.into(),
        scope: Scope::Video,
        video_id: Some(ITEM.into()),
        season: None,
        params: serde_json::json!({ "displayHeight": 1080 }),
        pick: None,
    }
}

fn row_of(core: &Core, id: &str) -> Option<DownloadRow> {
    core.downloads()
        .board()
        .get()
        .rows
        .into_iter()
        .find(|r| r.id == id)
}

async fn until(core: &Core, id: &str, what: &str, check: impl Fn(&DownloadRow) -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        if row_of(core, id).as_ref().is_some_and(&check) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never: {what}; last {:?}",
            row_of(core, id)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn params() -> VersionsParams {
    VersionsParams {
        kind: "movie".into(),
        video_id: ITEM.into(),
        target: Target {
            title: "Sintel".into(),
            year: Some(2010),
            season: None,
            episode: None,
        },
        caps: DeviceCaps::basic_1080p(),
        prefs: StreamPrefs::default(),
        last_binge_group: None,
        facts: Default::default(),
        device_language: None,
    }
}

fn target() -> PlayTarget {
    PlayTarget {
        kind: "movie".into(),
        item_id: ITEM.into(),
        video_id: ITEM.into(),
        name: None,
        poster: None,
        season: None,
        episode: None,
    }
}

fn file_of(core: &Core, id: &str) -> Vec<u8> {
    let path = core.downloads().file_path(id).expect("a downloaded file");
    std::fs::read(path).unwrap()
}

/// A whole download, then a play with no network: the Versions view
/// recommends it before any addon answers, and the player reads the file.
#[tokio::test]
async fn a_download_plays_offline_before_any_addon_answers() {
    let data = video(20 << 20);
    let (url, _) = origin(data.clone(), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, &ids[0]), *data);
    let row = row_of(&core, &ids[0]).unwrap();
    assert_eq!(row.bytes_total, Some(data.len() as u64));
    assert_eq!(row.quality.as_deref(), Some("1080p"));

    // Offline, and the addon now holds its answer for 30 s: the download
    // is the pick at once.
    let slow = addon(&url, Duration::from_secs(30)).await;
    core.library()
        .set_addons(core.profile(), std::slice::from_ref(&slow), &core.hlc())
        .unwrap();
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let view = VersionsView::open(&core, params());
    let snapshot = view.snapshot();
    let pick = snapshot
        .recommended
        .expect("the download is recommended at once");
    assert!(snapshot.frozen && !snapshot.done);
    assert_eq!(pick.on_device.as_deref(), Some("Downloaded"));
    assert!(snapshot.empty.is_none());

    let asked = asks(&slow);
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while playback.snapshot().phase != PlaybackPhase::Ready {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Offline no subtitle source is asked.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(asks(&slow), asked);
    assert!(!playback.snapshot().subtitles_pending);
    let url = playback.snapshot().url.unwrap();
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let opts = mq_http::RequestOptions {
        allow_private: true,
        max_bytes: 64 << 20,
        ..Default::default()
    };
    let got = http.get_range(&url, 1000, 4096, &opts).await.unwrap();
    assert_eq!(got.status, 206);
    assert_eq!(got.body[..], data[1000..5096]);
}

/// A connection cut halfway: the download goes on from its last checkpoint
/// with a range, and the file is whole.
#[tokio::test]
async fn a_cut_download_resumes_from_its_checkpoint() {
    let data = video(30 << 20);
    let (url, seen) = origin(
        data.clone(),
        How {
            cut_first_after: Some(20 << 20),
            ..How::default()
        },
    )
    .await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, &ids[0]), *data);
    let ranges = seen.ranges.lock().unwrap().clone();
    assert_eq!(ranges[0], "0-");
    // The second request starts where the first stopped (flushed at the
    // cut).
    let second: u64 = ranges[1].trim_end_matches('-').parse().unwrap();
    assert!(second > 0 && second <= 20 << 20, "{ranges:?}");
}

/// A subtitle step no source answers asks again only after a wait, never
/// at every look of the queue; back online it asks at once.
#[tokio::test]
async fn a_subtitle_step_no_source_answers_waits_to_ask_again() {
    let (url, _) = origin(video(4 << 20), How::default()).await;
    let stream = serde_json::json!({
        "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url,
        "behaviorHints": { "filename": "Sintel.2010.1080p.WEB-DL.mp4" }
    });
    let extras = Extras {
        subtitles_fail: true,
        ..Extras::default()
    };
    let row = addon_serving(stream, Duration::ZERO, extras).await;
    let core = core_with(row.clone()).await;
    let film = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &film[0], "done", |r| r.state == DownloadState::Done).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while asks(&row) == 0 {
        assert!(std::time::Instant::now() < deadline, "the step never asked");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Every link report is a look of the queue: none asks again.
    for _ in 0..10 {
        core.downloads().set_link(Link::default());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(asks(&row), 1);
    // Back online after a spell offline: it asks at once.
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    core.downloads().set_link(Link::default());
    while asks(&row) < 2 {
        assert!(std::time::Instant::now() < deadline, "no ask back online");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A resume whose answer is another file (a link resolved again names
/// another file of a pack, of another size): the file starts again from 0
/// instead of growing the old one's start with the new one's end.
#[tokio::test]
async fn a_resume_onto_another_file_starts_again() {
    let first = video(30 << 20);
    let other = video(24 << 20);
    let (url, seen) = origin(
        first,
        How {
            cut_first_after: Some(20 << 20),
            then: Some(other.clone()),
            ..How::default()
        },
    )
    .await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, &ids[0]), *other);
    let ranges = seen.ranges.lock().unwrap().clone();
    assert_eq!(ranges.last().map(String::as_str), Some("0-"), "{ranges:?}");
}

/// An origin that ignores ranges on a resume sends it all again: the file
/// starts over instead of growing a second copy.
#[tokio::test]
async fn an_origin_ignoring_ranges_restarts_the_file() {
    let data = video(20 << 20);
    let (url, _) = origin(
        data.clone(),
        How {
            cut_first_after: Some(12 << 20),
            ignore_ranges: true,
            ..How::default()
        },
    )
    .await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, &ids[0]), *data);
}

/// A page instead of a video (a dead debrid link, a captive portal) is no
/// download.
#[tokio::test]
async fn a_page_instead_of_a_video_fails() {
    let (url, _) = origin(
        video(1 << 20),
        How {
            html: true,
            ..How::default()
        },
    )
    .await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "failed", |r| {
        r.state == DownloadState::Failed
    })
    .await;
    let row = row_of(&core, &ids[0]).unwrap();
    assert_eq!(row.reason, Some(DownloadReason::NotVideo));
    // Refused at its first bytes, not after downloading the page.
    assert_eq!(row.bytes_done, 0, "{row:?}");
}

/// The queue's holds: no network, a metered network with Wi-Fi only (and
/// the user's release onto mobile data), a stream playing, too little space.
#[tokio::test]
async fn the_queue_waits_for_what_holds_it() {
    let data = video(4 << 20);
    let (url, _) = origin(data, How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let downloads = core.downloads();
    downloads.set_link(Link {
        offline: true,
        ..Link::default()
    });
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    let id = &ids[0];
    until(&core, id, "waits for a connection", |r| {
        r.reason == Some(DownloadReason::Network)
    })
    .await;
    downloads.set_link(Link {
        metered: true,
        ..Link::default()
    });
    until(&core, id, "waits for Wi-Fi", |r| {
        r.reason == Some(DownloadReason::Wifi)
    })
    .await;
    downloads.set_link(Link {
        low_data: true,
        ..Link::default()
    });
    until(&core, id, "waits for Low Data Mode", |r| {
        r.reason == Some(DownloadReason::LowData)
    })
    .await;
    downloads.set_link(Link::default());
    let playing = downloads.streaming(None);
    until(&core, id, "waits while a stream plays", |r| {
        r.reason == Some(DownloadReason::Playing)
    })
    .await;
    drop(playing);
    core.set_free_bytes(512 << 20);
    until(&core, id, "waits for space", |r| {
        r.reason == Some(DownloadReason::Space)
    })
    .await;
    core.set_free_bytes(100 << 30);
    downloads.set_link(Link {
        metered: true,
        ..Link::default()
    });
    downloads
        .act(
            std::slice::from_ref(id),
            mq_runtime::downloads::Action::Retry { mobile_data: true },
        )
        .unwrap();
    until(&core, id, "done on mobile data", |r| {
        r.state == DownloadState::Done
    })
    .await;
}

/// A network play holds downloads only while it plays: paused, they go on
/// (the film itself, kept for later); played on, they wait again.
#[tokio::test]
async fn a_paused_network_play_holds_no_download() {
    let (url, _) = origin(
        video(32 << 20),
        How {
            slow: true,
            ..How::default()
        },
    )
    .await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let view = VersionsView::open(&core, params());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while view.snapshot().recommended.is_none() {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let key = view.snapshot().recommended.unwrap().key;
    let playback = PlaybackView::open_picked(&core, view.pick(&key), target(), &view.plays());
    while playback.snapshot().phase != PlaybackPhase::Ready {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The film itself, kept for later while it streams.
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    let id = &ids[0];
    until(&core, id, "waits while the film plays", |r| {
        r.reason == Some(DownloadReason::Playing)
    })
    .await;
    playback.report_progress(10_000, 900_000, true);
    until(&core, id, "runs while the film is paused", |r| {
        r.state == DownloadState::Running
    })
    .await;
    playback.report_progress(10_000, 900_000, false);
    until(&core, id, "waits again once it plays on", |r| {
        r.reason == Some(DownloadReason::Playing)
    })
    .await;
}

/// A season's add queues its aired episodes in order and never one not out
/// yet; a kids profile adds nothing and sees only its list, while `active`
/// still counts the owner's downloads for the keeper, unnamed.
#[tokio::test]
async fn a_season_queues_its_aired_episodes_and_kids_add_nothing() {
    let (url, _) = origin(
        video(1 << 20),
        How {
            slow: true,
            ..How::default()
        },
    )
    .await;
    // A release named for the show: Sintel's would be hidden as another
    // title, and the season would only look running while it picks.
    let stream = serde_json::json!({ "name": "Direct\n1080p", "title": "Show.S02.1080p.WEB-DL.mp4", "url": url });
    let core = core_with(addon_listing(stream, Duration::ZERO).await).await;
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let season = AddRequest {
        kind: "series".into(),
        item_id: "tt9".into(),
        scope: Scope::Series,
        video_id: None,
        season: None,
        params: serde_json::json!({}),
        pick: None,
    };
    let ids = mq_runtime::downloads::add(&core, season).await.unwrap();
    let videos: Vec<String> = core
        .downloads()
        .board()
        .get()
        .rows
        .iter()
        .filter(|r| ids.contains(&r.id))
        .map(|r| r.video_id.clone())
        .collect();
    assert_eq!(videos, ["tt9:2:1", "tt9:2:2"]);
    // A kids profile changes nothing.
    core.set_pin("default", Some("1357"), None).await.unwrap();
    let kid = core.for_profile(&core.create_kids_profile("Kid", "fox").unwrap());
    assert!(mq_runtime::downloads::add(&kid, movie()).await.is_err());
    assert!(mq_runtime::downloads::owner(&kid).is_err());
    // Nothing of its list is downloaded: its view lists nothing.
    let view = mq_runtime::DownloadsView::open(&kid);
    assert!(view.snapshot().downloading.is_empty() && view.snapshot().titles.is_empty());
    // Online the owner's season runs: the kid's keeper keeps the app running
    // for it, and its notification names no title outside the kid's list.
    core.downloads().set_link(Link::default());
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while view.snapshot().active.running == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the owner's season never ran: {:?} {:?}",
            ids.iter()
                .map(|id| row_of(&core, id).map(|r| (r.state, r.reason, r.bytes_done)))
                .collect::<Vec<_>>(),
            view.snapshot().active
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let s = view.snapshot();
    assert!(s.downloading.is_empty());
    assert_eq!(s.active.title, None);
    // Running counted a real download, not a job still picking its version.
    until(&core, &ids[0], "the first episode downloads", |r| {
        r.state == DownloadState::Done
    })
    .await;
}

/// A stored download row as a launch finds it.
fn record(
    id: &str,
    item: &str,
    video: &str,
    state: &str,
    version: serde_json::Value,
) -> mq_library::DownloadRecord {
    mq_library::DownloadRecord {
        id: id.into(),
        added_by: "default".into(),
        kind: if item == ITEM { "movie" } else { "series" }.into(),
        item_id: item.into(),
        video_id: video.into(),
        season: (item != ITEM).then_some(2),
        episode: (item != ITEM).then(|| video.rsplit(':').next().unwrap().parse().unwrap()),
        facts: serde_json::json!({
            "name": "Show", "runtime": "50 min", "episodeTitle": format!("Ep {video}"),
            "released": "2020-01-01"
        })
        .to_string(),
        request: "{}".into(),
        version: Some(version.to_string()),
        state: state.into(),
        reason: None,
        mobile_data: false,
        hand_picked: false,
        bytes_done: 0,
        bytes_total: None,
        moves: 0,
        subtitles: "[]".into(),
        subtitles_state: "pending".into(),
        position: 0,
        created_ms: 1,
        done_ms: None,
        smart: None,
    }
}

/// The version JSON a job keeps for the direct stream at `url`.
fn kept_version(url: &str, addon_url: &str) -> serde_json::Value {
    let wire = serde_json::json!({ "streams": [{
        "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url
    }]});
    let decoded = mq_protocol::decode_streams(wire.to_string().as_bytes()).unwrap();
    let stream = mq_streams::normalize(&decoded.streams[0], "local", "Local", 0, 0);
    serde_json::json!({
        "key": stream.key,
        "chain": [{ "stream": stream, "addonUrl": addon_url }],
        "ext": "mp4"
    })
}

/// A core with no addon, files kept in its own downloads folder.
fn bare() -> Core {
    let core = Core::temporary("default").unwrap();
    core.set_free_bytes(100 << 30);
    core
}

/// Waits until `folder` is gone: a removed download's files go off the
/// caller's thread.
async fn gone(folder: &std::path::Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while folder.exists() {
        assert!(std::time::Instant::now() < deadline, "the file stayed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Writes `bytes` as job `id`'s file `name`.
fn put(core: &Core, id: &str, name: &str, bytes: &[u8]) {
    let job = core.downloads().job_folder(id);
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(job.join(name), bytes).unwrap();
}

/// A launch after a kill: the job left running goes on from its
/// checkpoint, and bytes written after it (which may not have reached the
/// disk) are cut first, so the file comes out whole.
#[tokio::test]
async fn a_launch_resumes_a_killed_download_from_its_checkpoint() {
    let data = video(20 << 20);
    let (url, seen) = origin(data.clone(), How::default()).await;
    let core = bare();
    let addon_url = "http://127.0.0.1:9/manifest.json";
    let mut row = record("k1", ITEM, ITEM, "running", kept_version(&url, addon_url));
    row.bytes_done = 8 << 20;
    row.bytes_total = Some(data.len() as i64);
    core.library().add_download(&row).unwrap();
    // 24 MiB on disk: 8 checkpointed, then 16 of junk, past the file's end.
    let mut part = data[..8 << 20].to_vec();
    part.extend(std::iter::repeat_n(0xEE, 16 << 20));
    put(&core, "k1", "video.mp4.part", &part);
    core.downloads().relaunch();
    core.downloads().set_link(Link::default());
    until(&core, "k1", "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, "k1"), *data);
    assert_eq!(seen.ranges.lock().unwrap()[0], format!("{}-", 8 << 20));
}

/// Killed between the last checkpoint and the rename: the whole file is in
/// its `.part`, and the launch finishes it with no request (a request for
/// nothing would answer 416 and throw the file away).
#[tokio::test]
async fn a_launch_finishes_a_whole_part_without_asking() {
    let data = video(4 << 20);
    let (url, seen) = origin(data.clone(), How::default()).await;
    let core = bare();
    let mut row = record(
        "w1",
        ITEM,
        ITEM,
        "running",
        kept_version(&url, "http://127.0.0.1:9/manifest.json"),
    );
    row.bytes_done = data.len() as i64;
    row.bytes_total = Some(data.len() as i64);
    core.library().add_download(&row).unwrap();
    put(&core, "w1", "video.mp4.part", &data);
    core.downloads().relaunch();
    core.downloads().set_link(Link::default());
    until(&core, "w1", "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, "w1"), *data);
    assert!(seen.ranges.lock().unwrap().is_empty());
}

/// A launch finds a done download's file cut short: it reads "Download
/// again", never a broken play.
#[tokio::test]
async fn a_launch_marks_a_damaged_download_missing() {
    let core = bare();
    let mut row = record(
        "d1",
        ITEM,
        ITEM,
        "done",
        kept_version(
            "http://127.0.0.1:1/v.mp4",
            "http://127.0.0.1:9/manifest.json",
        ),
    );
    row.bytes_done = 100;
    row.bytes_total = Some(100);
    core.library().add_download(&row).unwrap();
    put(&core, "d1", "video.mp4", &[0u8; 50]);
    core.downloads().relaunch();
    until(&core, "d1", "missing", |r| {
        r.state == DownloadState::Missing
    })
    .await;
}

/// No network and no meta: the title page shows what its downloads kept,
/// and plays from there.
#[tokio::test]
async fn the_title_page_opens_offline_from_its_downloads() {
    let core = bare();
    for (id, video) in [("e1", "tt9:2:1"), ("e2", "tt9:2:2"), ("e3", "tt9:2:3")] {
        // E3 failed half-way: the offline page lists what plays (code review
        // r2 N5).
        let state = if id == "e3" { "failed" } else { "done" };
        let mut row = record(
            id,
            "tt9",
            video,
            state,
            kept_version(
                "http://127.0.0.1:1/v.mp4",
                "http://127.0.0.1:9/manifest.json",
            ),
        );
        row.bytes_done = 100;
        row.bytes_total = Some(100);
        core.library().add_download(&row).unwrap();
        put(&core, id, "video.mp4", &[0u8; 100]);
    }
    core.downloads().relaunch();
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    // No addon: the meta cannot be read.
    let view = mq_runtime::TitleView::open(&core, "series", "tt9");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let snapshot = view.snapshot();
        if snapshot.header.offline {
            assert_eq!(snapshot.header.seasons, [2]);
            let videos: Vec<&str> = snapshot
                .episodes
                .iter()
                .map(|e| e.video_id.as_str())
                .collect();
            assert_eq!(videos, ["tt9:2:1", "tt9:2:2"]);
            assert!(snapshot.episodes.iter().all(|e| e.download.is_some()));
            assert_eq!(
                snapshot.header.play_target.map(|t| t.video_id).as_deref(),
                Some("tt9:2:1")
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            snapshot.header
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A title page opened on a launch whose downloads were all done drops
/// their badges when they are deleted: no board change came before the
/// delete for it to compare with (the 2026-10-28 iPhone walk).
#[tokio::test]
async fn a_delete_clears_the_badges_of_a_page_opened_after_launch() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    for (id, video) in [("e1", "tt9:2:1"), ("e2", "tt9:2:2")] {
        let mut row = record(
            id,
            "tt9",
            video,
            "done",
            kept_version(&url, "http://127.0.0.1:9/manifest.json"),
        );
        row.bytes_done = 100;
        row.bytes_total = Some(100);
        core.library().add_download(&row).unwrap();
        put(&core, id, "video.mp4", &[0u8; 100]);
    }
    core.downloads().relaunch();
    core.downloads().set_link(Link::default());
    let view = mq_runtime::TitleView::open(&core, "series", "tt9");
    let badges = |view: &mq_runtime::TitleView| {
        view.snapshot()
            .episodes
            .iter()
            .filter(|e| e.download.is_some())
            .count()
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while badges(&view) != 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            view.snapshot().episodes
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    core.downloads()
        .act(
            &["e1".to_owned(), "e2".to_owned()],
            mq_runtime::downloads::Action::Remove,
        )
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while badges(&view) != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the page kept its badges"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Offline, a download still being written plays as far as it holds; a
/// read past that ends at once.
#[tokio::test]
async fn a_partial_download_plays_offline_as_far_as_it_holds() {
    let data = video(20 << 20);
    let core = bare();
    let mut row = record(
        "p1",
        ITEM,
        ITEM,
        "paused",
        kept_version(
            "http://127.0.0.1:1/v.mp4",
            "http://127.0.0.1:9/manifest.json",
        ),
    );
    row.bytes_done = 10 << 20;
    row.bytes_total = Some(data.len() as i64);
    core.library().add_download(&row).unwrap();
    put(&core, "p1", "video.mp4.part", &data[..10 << 20]);
    core.downloads().relaunch();
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let view = VersionsView::open(&core, params());
    let pick = view
        .snapshot()
        .recommended
        .expect("the partial download, offline");
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while playback.snapshot().phase != PlaybackPhase::Ready {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let url = playback.snapshot().url.unwrap();
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let opts = mq_http::RequestOptions {
        allow_private: true,
        max_bytes: 64 << 20,
        ..Default::default()
    };
    let got = http.get_range(&url, 4096, 4096, &opts).await.unwrap();
    assert_eq!(got.body[..], data[4096..8192]);
    let started = std::time::Instant::now();
    let past = http.get_range(&url, 15 << 20, 4096, &opts).await;
    assert!(past.is_err() || past.unwrap().body.is_empty());
    assert!(started.elapsed() < Duration::from_secs(5));
    // Online again, not running: Versions does not take it.
    core.downloads().set_link(Link::default());
    let online = VersionsView::open(&core, params());
    assert!(online.snapshot().recommended.is_none());
}

/// A free-space figure the platform reported more than 2 minutes ago is
/// unknown: the disk's own decides.
#[tokio::test]
async fn a_stale_free_space_report_counts_for_nothing() {
    let (url, _) = origin(video(4 << 20), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    core.set_free_bytes(512 << 20);
    let downloads = core.downloads();
    downloads.report_free(100 << 30, Duration::from_secs(180));
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "waits for space", |r| {
        r.reason == Some(DownloadReason::Space)
    })
    .await;
    downloads.set_allowed(true, Some(100 << 30));
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
}

/// The disk filling up under a running download (another app wrote) makes
/// it wait, and it is not let go again on the platform's figure alone: it
/// goes on once the disk itself has room.
#[tokio::test]
async fn a_download_that_ran_out_of_space_waits_for_the_disk() {
    let (url, _) = origin(video(12 << 20), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    core.set_free_bytes(300 << 20);
    core.downloads().set_allowed(true, Some(100 << 30));
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "waits for space", |r| {
        r.reason == Some(DownloadReason::Space)
    })
    .await;
    // The platform's figure again: it stays put.
    core.downloads().set_allowed(true, Some(100 << 30));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let row = row_of(&core, &ids[0]).unwrap();
    assert_eq!(row.reason, Some(DownloadReason::Space), "{row:?}");
    core.set_free_bytes(100 << 30);
    core.downloads().set_allowed(true, None);
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
}

/// A folder that cannot be written is a wait for space, never a failure.
#[tokio::test]
async fn an_unwritable_folder_waits_and_never_fails() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    let folder = core.downloads().job_folder(&ids[0]);
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::fs::write(&folder, b"in the way").unwrap();
    core.downloads().set_link(Link::default());
    until(&core, &ids[0], "waits for space", |r| {
        r.state == DownloadState::Waiting && r.reason == Some(DownloadReason::Space)
    })
    .await;
}

// Torrents (feature `torrent`): a loopback swarm with discovery off, the
// leecher installed as the device's engine.

#[cfg(feature = "torrent")]
#[path = "../../mq-engine/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

/// A torrent download, then a play with no network: the engine's folder
/// holds the whole file, the Versions view recommends it before any addon
/// answers, and the player reads it from disk.
#[cfg(feature = "torrent")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_download_plays_offline() {
    let s = support::swarm(true).await;
    // Known to the engine with its peer: the job's own add finds it.
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let file = s
        .leecher
        .files(t)
        .unwrap()
        .iter()
        .position(|f| f.path.ends_with("movie.mkv"))
        .unwrap();
    let stream = serde_json::json!({
        "name": "Fixture\n1080p", "title": "Sintel.2010.1080p.WEB.x264-GROUP",
        "infoHash": s.info_hash, "fileIdx": file
    });
    let core = core_with(addon_listing(stream.clone(), Duration::ZERO).await).await;
    core.set_torrent_engine(s.leecher.clone());
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    let row = row_of(&core, &ids[0]).unwrap();
    assert_eq!(row.bytes_total, Some(s.movie.len() as u64));
    assert_eq!(
        s.leecher.stats(t).await.unwrap().readers,
        0,
        "no reader fetched it"
    );

    // Offline, the addon slow: the download is the pick at once.
    let slow = addon_listing(stream, Duration::from_secs(30)).await;
    core.library()
        .set_addons(core.profile(), &[slow], &core.hlc())
        .unwrap();
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let view = VersionsView::open(&core, params());
    let pick = view.snapshot().recommended.expect("the download, at once");
    assert_eq!(pick.on_device.as_deref(), Some("Downloaded"));
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while playback.snapshot().phase != PlaybackPhase::Ready {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let url = playback.snapshot().url.unwrap();
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let opts = mq_http::RequestOptions {
        allow_private: true,
        max_bytes: 64 << 20,
        ..Default::default()
    };
    let got = http.get_range(&url, 100_000, 4096, &opts).await.unwrap();
    assert_eq!(got.body[..], s.movie[100_000..104_096]);
    // A whole download played takes nothing from the network: downloads
    // go on beside it.
    let others = core.downloads();
    assert!(
        row_of(&core, &ids[0]).is_some_and(|r| r.state == DownloadState::Done),
        "{:?}",
        others.board().get()
    );
    drop(playback);
    drop(view);

    // Online, a paused partial whose head is on disk but whose swarm is not
    // answering now is no pick: it would stall at its first missing piece
    // ahead of a ready version (code review r1 P2-5).
    core.library()
        .update_download(&ids[0], |r| {
            r.state = "paused".into();
            r.bytes_done = (s.movie.len() / 2) as i64;
            true
        })
        .unwrap();
    // The device's torrent folder is the leecher's, so its start reads as
    // on disk.
    let _ = std::fs::remove_dir(core.torrent_dir());
    std::os::unix::fs::symlink(s.leech_dir(), core.torrent_dir()).unwrap();
    core.downloads().relaunch();
    core.downloads().set_link(Link::default());
    let view = VersionsView::open(&core, params());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !view.snapshot().done {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            view.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let s = view.snapshot();
    assert!(!s.frozen);
    assert_ne!(
        s.recommended.and_then(|r| r.on_device).as_deref(),
        Some("Downloaded")
    );
}

/// A loopback swarm whose seeder an HTTP tracker names, and a stream of
/// its film listing that tracker: every add finds the seeder, even after
/// the torrent was let go. The tracker lives as long as its handle.
#[cfg(feature = "test-hooks")]
async fn tracked_swarm() -> (support::Swarm, serde_json::Value, support::HttpTracker) {
    let (v4, v6) = (std::net::Ipv4Addr::LOCALHOST, std::net::Ipv6Addr::LOCALHOST);
    let s = support::swarm_scoped(v4.into(), &[v4.into()], &[v4.into(), v6.into()], true).await;
    // A tracker that names the seeder: every add, the job's own included,
    // finds it after its torrent was let go.
    let mut body = b"d8:intervali60e5:peers6:".to_vec();
    body.extend(v4.octets());
    body.extend(s.seeder_addr.port().to_be_bytes());
    body.extend(b"e");
    let mut answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend(body);
    let tracker = support::HttpTracker::start(answer, false);
    let stream = serde_json::json!({
        "name": "Fixture\n1080p", "title": "Sintel.2010.1080p.WEB.x264-GROUP",
        "infoHash": s.info_hash, "sources": [format!("tracker:{}", tracker.url)]
    });
    (s, stream, tracker)
}

/// A torrent download removed and added again at once downloads whole:
/// the new run waits for the old one's release of the same file, which
/// would otherwise stop its fetch (code review r3 N2).
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_removed_and_added_again_downloads_whole() {
    let (s, stream, _tracker) = tracked_swarm().await;
    let core = core_with(addon_listing(stream, Duration::ZERO).await).await;
    core.set_torrent_engine(s.leecher.clone());
    mq_runtime::downloads::slow_release_for_tests(&s.info_hash, Some(Duration::from_secs(5)));
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    core.downloads()
        .act(&ids, mq_runtime::downloads::Action::Remove)
        .unwrap();
    let again = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let row = row_of(&core, &again[0]);
        if row.as_ref().is_some_and(|r| r.state == DownloadState::Done) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "stalled: {row:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Past the old release: the file is still kept, and plays offline.
    tokio::time::sleep(Duration::from_millis(5500)).await;
    assert_eq!(
        s.leecher.kept_for_tests(&s.info_hash).len(),
        1,
        "the old release let the new download's file go"
    );
    mq_runtime::downloads::slow_release_for_tests(&s.info_hash, None);
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let view = VersionsView::open(&core, params());
    let pick = view.snapshot().recommended.expect("the download, at once");
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while playback.snapshot().phase != PlaybackPhase::Ready {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let opts = mq_http::RequestOptions {
        allow_private: true,
        max_bytes: 64 << 20,
        ..Default::default()
    };
    let got = http
        .get_range(&playback.snapshot().url.unwrap(), 100_000, 4096, &opts)
        .await
        .unwrap();
    assert_eq!(got.body[..], s.movie[100_000..104_096]);
}

/// A release that never ends holds a new run of its torrent only so long
/// (10 s in test builds, a minute in the app): the run goes on and
/// downloads, never Running with nothing coming (code review r5 R5-1).
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stuck_release_holds_the_next_run_only_so_long() {
    let (s, stream, _tracker) = tracked_swarm().await;
    let core = core_with(addon_listing(stream, Duration::ZERO).await).await;
    core.set_torrent_engine(s.leecher.clone());
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    mq_runtime::downloads::slow_release_for_tests(&s.info_hash, Some(Duration::from_secs(600)));
    core.downloads()
        .act(&ids, mq_runtime::downloads::Action::Remove)
        .unwrap();
    let again = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let row = row_of(&core, &again[0]);
        if row.as_ref().is_some_and(|r| r.state == DownloadState::Done) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "held: {row:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    mq_runtime::downloads::slow_release_for_tests(&s.info_hash, None);
}

/// Online, a partial torrent download whose start is not on disk (its
/// swarm may be dead) is no pick: the ranking places the versions the
/// addons list (apps r2 P2-2).
#[cfg(feature = "torrent")]
#[tokio::test]
async fn a_partial_torrent_with_nothing_on_disk_is_no_pick() {
    let core = bare();
    let wire = serde_json::json!({ "streams": [{
        "name": "Fixture\n1080p", "title": "Sintel.2010.1080p.WEB.x264-GROUP",
        "infoHash": "c".repeat(40), "fileIdx": 1
    }]});
    let decoded = mq_protocol::decode_streams(wire.to_string().as_bytes()).unwrap();
    let stream = mq_streams::normalize(&decoded.streams[0], "local", "Local", 0, 0);
    let version = serde_json::json!({
        "key": stream.key,
        "chain": [{ "stream": stream }],
        "torrent": { "hash": "c".repeat(40), "file": 1, "name": "Sintel.mkv" }
    });
    let mut row = record("t1", ITEM, ITEM, "paused", version);
    row.bytes_done = 4 << 20;
    row.bytes_total = Some(100 << 20);
    core.library().add_download(&row).unwrap();
    core.downloads().set_link(Link::default());
    let view = VersionsView::open(&core, params());
    assert!(view.snapshot().recommended.is_none());
}

/// Subtitles download with the video, for the languages of every profile on
/// the device and the device's own (5 each, forced ones beside), and play
/// with no network from their files; a delay set while streaming applies
/// to the kept track (plans/offline-downloads.md 4.10).
#[tokio::test]
async fn subtitles_download_with_the_video_and_play_offline() {
    let data = video(4 << 20);
    let (url, _) = origin(data, How::default()).await;
    let stream = serde_json::json!({
        "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url,
        "behaviorHints": { "filename": "Sintel.2010.1080p.WEB-DL.mp4" }
    });
    let extras = Extras {
        subtitles: vec![
            ("en1", "eng", false),
            ("en2", "eng", false),
            ("en3", "eng", false),
            ("en4", "eng", false),
            ("en5", "eng", false),
            ("en6", "eng", false),
            ("enf", "eng", true),
            ("tr1", "tur", false),
            ("fr1", "fre", false),
        ],
        ..Extras::default()
    };
    let source = addon_serving(stream, Duration::ZERO, extras).await;
    let core = core_with(source.clone()).await;
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["en"] }))
        .unwrap();
    // Streamed first: a delay set on en1 is remembered for this version.
    let view = VersionsView::open(&core, params());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !(view.snapshot().done && view.snapshot().recommended.is_some()) {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let key = view.snapshot().recommended.unwrap().key;
    let online = PlaybackView::open_picked(&core, view.pick(&key), target(), &view.plays());
    let first_english = |s: &mq_runtime::PlaybackSnapshot| {
        s.subtitles
            .iter()
            .find(|t| t.language == "en" && !t.forced)
            .map(|t| t.url.clone())
    };
    loop {
        let s = online.snapshot();
        if s.subtitles.len() == 9 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{:?}", s.subtitles);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let url = first_english(&online.snapshot()).unwrap();
    online.subtitle_delay(&url, 1500);
    drop(online);
    drop(view);

    let mut request = movie();
    request.params = serde_json::json!({ "language": "tr" });
    let ids = mq_runtime::downloads::add(&core, request).await.unwrap();
    until(&core, &ids[0], "done with its subtitles", |r| {
        r.state == DownloadState::Done && !r.subtitles.is_empty()
    })
    .await;
    let row = row_of(&core, &ids[0]).unwrap();
    assert_eq!(
        row.subtitles,
        ["en", "tr"],
        "French is no one's language here"
    );

    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let asked = asks(&source);
    let view = VersionsView::open(&core, params());
    let pick = view.snapshot().recommended.unwrap();
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let s = playback.snapshot();
        if s.phase == PlaybackPhase::Ready && s.subtitles.len() == 7 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{:?}", s.subtitles);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let s = playback.snapshot();
    let english = s.subtitles.iter().filter(|t| t.language == "en").count();
    assert_eq!(english, 6, "five and the forced one");
    assert!(s.subtitles.iter().any(|t| t.language == "tr"));
    // The delay set while streaming holds for the kept track: one
    // identity, the source's and the file's.
    let kept = s
        .subtitles
        .iter()
        .find(|t| t.language == "en" && !t.forced)
        .unwrap();
    assert_eq!(kept.delay_ms, 1500);
    // Its file, served on loopback with no network.
    let first = &s.subtitles[0];
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let opts = mq_http::RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let vtt = http.get(&first.url, &opts).await.unwrap();
    let text = String::from_utf8_lossy(&vtt.body);
    assert!(
        text.starts_with("WEBVTT") && text.contains("line of"),
        "{text}"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(asks(&source), asked, "a source asked offline");
}

/// A download's poster is kept and served on loopback, so the Downloads
/// screen shows it with no network (4.11).
#[tokio::test]
async fn a_downloads_poster_is_kept_and_served() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    let stream = serde_json::json!({ "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url });
    let extras = Extras {
        public_name: Some("posters.downloads.test"),
        ..Extras::default()
    };
    let core = core_with(addon_serving(stream, Duration::ZERO, extras).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "a loopback poster", |r| {
        r.poster
            .as_deref()
            .is_some_and(|p| p.starts_with("http://127.0.0.1"))
    })
    .await;
    let poster = row_of(&core, &ids[0]).unwrap().poster.unwrap();
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let opts = mq_http::RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    assert_eq!(&http.get(&poster, &opts).await.unwrap().body[..], POSTER);
}

/// "Try again" downloads the video again and keeps the rest: the poster
/// stays, and the subtitles come back with the new pick and play offline
/// (code review r3 N1).
#[tokio::test]
async fn a_retry_keeps_the_poster_and_the_subtitles() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    let stream = serde_json::json!({ "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url });
    let extras = Extras {
        subtitles: vec![("en1", "eng", false)],
        public_name: Some("retry.downloads.test"),
        ..Extras::default()
    };
    let core = core_with(addon_serving(stream, Duration::ZERO, extras).await).await;
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["en"] }))
        .unwrap();
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    let kept = |r: &DownloadRow| {
        r.state == DownloadState::Done
            && r.subtitles == ["en"]
            && r.poster
                .as_deref()
                .is_some_and(|p| p.starts_with("http://127.0.0.1"))
    };
    until(&core, &ids[0], "done with its subtitle and poster", &kept).await;
    // The kept file does not play: "Try again".
    let playback = play_when(&core, ITEM, ITEM, true).await;
    playback.fail_over(None, Some("decode"), None, "no frame");
    until(&core, &ids[0], "failed", |r| {
        r.state == DownloadState::Failed
    })
    .await;
    drop(playback);
    // Offline, the file that did not play is not offered (code review r3
    // N4).
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let view = VersionsView::open(&core, params());
    tokio::time::sleep(Duration::from_millis(500)).await;
    let offered = view.snapshot().recommended.and_then(|r| r.on_device);
    assert_ne!(offered.as_deref(), Some("Downloaded"));
    drop(view);
    core.downloads().set_link(Link::default());
    core.downloads()
        .act(
            std::slice::from_ref(&ids[0]),
            mq_runtime::downloads::Action::Retry { mobile_data: false },
        )
        .unwrap();
    until(&core, &ids[0], "done again with both", &kept).await;
    let poster = row_of(&core, &ids[0]).unwrap().poster.unwrap();
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let opts = mq_http::RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    assert_eq!(&http.get(&poster, &opts).await.unwrap().body[..], POSTER);
    // Offline, the subtitle plays from its file.
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let view = VersionsView::open(&core, params());
    let pick = view.snapshot().recommended.unwrap();
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let s = playback.snapshot();
        if s.phase == PlaybackPhase::Ready && s.subtitles.len() == 1 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{:?}", s.subtitles);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// "Try again" on a version the user picked keeps it, and its subtitles in
/// place: no source is asked for them again (code review r4).
#[tokio::test]
async fn a_hand_picked_retry_keeps_its_subtitles_in_place() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    let stream = serde_json::json!({ "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url });
    let extras = Extras {
        subtitles: vec![("en1", "eng", false)],
        ..Extras::default()
    };
    let source = addon_serving(stream, Duration::ZERO, extras).await;
    let core = core_with(source.clone()).await;
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["en"] }))
        .unwrap();
    let view = VersionsView::open(&core, params());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !(view.snapshot().done && view.snapshot().recommended.is_some()) {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let key = view.snapshot().recommended.unwrap().key;
    let mut request = movie();
    request.pick = Some(view.pick(&key).candidates);
    drop(view);
    let ids = mq_runtime::downloads::add(&core, request).await.unwrap();
    let kept = |r: &DownloadRow| r.state == DownloadState::Done && r.subtitles == ["en"];
    until(&core, &ids[0], "done with its subtitle", &kept).await;
    let playback = play_when(&core, ITEM, ITEM, true).await;
    playback.fail_over(None, Some("decode"), None, "no frame");
    until(&core, &ids[0], "failed", |r| {
        r.state == DownloadState::Failed
    })
    .await;
    drop(playback);
    // The online play's own asks are over.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let asked = asks(&source);
    core.downloads()
        .act(
            std::slice::from_ref(&ids[0]),
            mq_runtime::downloads::Action::Retry { mobile_data: false },
        )
        .unwrap();
    until(&core, &ids[0], "done again", &kept).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(asks(&source), asked, "its subtitles were asked for again");
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let view = VersionsView::open(&core, params());
    let pick = view.snapshot().recommended.unwrap();
    assert_eq!(pick.key, key);
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let s = playback.snapshot();
        if s.phase == PlaybackPhase::Ready && s.subtitles.len() == 1 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{:?}", s.subtitles);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A job that panics fails its row and frees its slot: two panics leave
/// the queue running, and the next download finishes (code review r3 N6).
#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn a_job_that_panics_frees_its_slot() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    // Sintel's release: the episodes panic before they pick, the film
    // after them downloads it.
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let season = AddRequest {
        kind: "series".into(),
        item_id: "tt9".into(),
        scope: Scope::Season,
        video_id: None,
        season: Some(2),
        params: serde_json::json!({ "testPanic": true }),
        pick: None,
    };
    let ids = mq_runtime::downloads::add(&core, season).await.unwrap();
    assert_eq!(ids.len(), 2, "as many as the queue runs at once");
    for id in &ids {
        until(&core, id, "failed", |r| {
            r.state == DownloadState::Failed && r.reason == Some(DownloadReason::Source)
        })
        .await;
    }
    let film = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &film[0], "done", |r| r.state == DownloadState::Done).await;
}

/// A kept file gone from the disk reads "Download again", and adding it
/// again brings the subtitles back too (code review r3 N3, r4 R4-1).
#[tokio::test]
async fn a_gone_file_reads_download_again_and_comes_back_whole() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    let stream = serde_json::json!({ "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url });
    let extras = Extras {
        subtitles: vec![("en1", "eng", false)],
        ..Extras::default()
    };
    let core = core_with(addon_serving(stream, Duration::ZERO, extras).await).await;
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["en"] }))
        .unwrap();
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    let kept = |r: &DownloadRow| r.state == DownloadState::Done && r.subtitles == ["en"];
    until(&core, &ids[0], "done with its subtitle", &kept).await;
    let playback = play_when(&core, ITEM, ITEM, true).await;
    // The system purged the folder while it played.
    std::fs::remove_dir_all(core.downloads().job_folder(&ids[0])).unwrap();
    playback.fail_over(None, Some("unreadable"), None, "no such file");
    until(&core, &ids[0], "missing", |r| {
        r.state == DownloadState::Missing
    })
    .await;
    drop(playback);
    let again = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    assert_eq!(again, ids);
    until(&core, &ids[0], "done again with its subtitle", &kept).await;
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let view = VersionsView::open(&core, params());
    let pick = view.snapshot().recommended.unwrap();
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let s = playback.snapshot();
        if s.phase == PlaybackPhase::Ready && s.subtitles.len() == 1 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{:?}", s.subtitles);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A wipe takes the downloaded files with their rows, not at the next
/// launch (code review r3 N9).
#[tokio::test]
async fn a_wipe_takes_the_downloaded_files() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    let folder = core.downloads().job_folder(&ids[0]);
    assert!(folder.exists());
    core.wipe().unwrap();
    gone(&folder).await;
}

/// A series with one stream at `url` (slow, so a deletion before e2 is
/// whole would show), e1 downloaded.
async fn series_with_e1(slow_bytes: usize) -> (Core, String) {
    let (url, _) = origin(
        video(slow_bytes),
        How {
            slow: true,
            ..How::default()
        },
    )
    .await;
    let stream = serde_json::json!({ "name": "Direct\n1080p", "title": "Show.S02.1080p.WEB-DL.mp4", "url": url });
    let core = core_with(addon_listing(stream, Duration::ZERO).await).await;
    let one = AddRequest {
        kind: "series".into(),
        item_id: "tt9".into(),
        scope: Scope::Video,
        video_id: Some("tt9:2:1".into()),
        season: None,
        params: serde_json::json!({}),
        pick: None,
    };
    let ids = mq_runtime::downloads::add(&core, one).await.unwrap();
    until(&core, &ids[0], "e1 done", |r| {
        r.state == DownloadState::Done
    })
    .await;
    (core, ids[0].clone())
}

/// Plays downloaded e1 to its end.
async fn play_e1_to_the_end(core: &Core) {
    let e1 = PlayTarget {
        kind: "series".into(),
        item_id: "tt9".into(),
        video_id: "tt9:2:1".into(),
        name: None,
        poster: None,
        season: Some(2),
        episode: Some(1),
    };
    let view = VersionsView::open(
        core,
        VersionsParams {
            kind: "series".into(),
            video_id: "tt9:2:1".into(),
            target: Target {
                title: "Show".into(),
                year: None,
                season: Some(2),
                episode: Some(1),
            },
            ..params()
        },
    );
    let pick = view.snapshot().recommended.expect("e1 downloaded");
    let playback = PlaybackView::open_picked(core, view.pick(&pick.key), e1, &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while playback.snapshot().phase != PlaybackPhase::Ready {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    playback.report_progress(3_000_000, 3_100_000, false);
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(playback);
}

/// Waits until e2 is done and e1 gone, and never sees e1 go first.
async fn e1_goes_after_e2(core: &Core) {
    let board = || core.downloads().board().get().rows;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let rows = board();
        let e2_done = rows
            .iter()
            .any(|r| r.video_id == "tt9:2:2" && r.state == DownloadState::Done);
        let e1_gone = !rows.iter().any(|r| r.video_id == "tt9:2:1");
        assert!(!e1_gone || e2_done, "e1 went before e2 was whole: {rows:?}");
        if e2_done && e1_gone {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{rows:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Smart downloads: a finished episode brings the next one and goes once
/// it is whole.
#[tokio::test]
async fn a_finished_episode_brings_the_next_and_goes() {
    let (core, _) = series_with_e1(2 << 20).await;
    play_e1_to_the_end(&core).await;
    e1_goes_after_e2(&core).await;
}

/// Finished offline (the meta cannot be read): nothing is added and
/// nothing deleted; the step outlives a relaunch, and online the next one
/// comes and the finished one goes after it.
#[tokio::test]
async fn a_smart_step_waits_offline_and_outlives_a_relaunch() {
    let (core, e1) = series_with_e1(2 << 20).await;
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    play_e1_to_the_end(&core).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    let rows = core.downloads().board().get().rows;
    assert!(rows.iter().any(|r| r.id == e1), "{rows:?}");
    assert!(!rows.iter().any(|r| r.video_id == "tt9:2:2"), "{rows:?}");
    core.downloads().relaunch();
    core.downloads().set_link(Link::default());
    e1_goes_after_e2(&core).await;
}

/// Opens a play of `video_id` of `item_id` through Versions and waits until
/// it is ready.
async fn play(core: &Core, item_id: &str, video_id: &str) -> PlaybackView {
    play_when(core, item_id, video_id, false).await
}

/// [`play`], with every addon answered first when `settled`: the pick then
/// carries the version's online sources after the kept file.
async fn play_when(core: &Core, item_id: &str, video_id: &str, settled: bool) -> PlaybackView {
    let episode: Option<u32> = video_id.rsplit(':').next().and_then(|e| e.parse().ok());
    let series = item_id != ITEM;
    let target = PlayTarget {
        kind: if series { "series" } else { "movie" }.into(),
        item_id: item_id.into(),
        video_id: video_id.into(),
        name: None,
        poster: None,
        season: series.then_some(2),
        episode: episode.filter(|_| series),
    };
    let view = VersionsView::open(
        core,
        VersionsParams {
            kind: target.kind.clone(),
            video_id: video_id.into(),
            target: Target {
                title: if series { "Show" } else { "Sintel" }.into(),
                year: (!series).then_some(2010),
                season: target.season,
                episode: target.episode,
            },
            ..params()
        },
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let pick = loop {
        let snapshot = view.snapshot();
        if let Some(pick) = snapshot.recommended.filter(|_| !settled || snapshot.done) {
            break pick;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            view.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let playback = PlaybackView::open_picked(core, view.pick(&pick.key), target, &view.plays());
    while playback.snapshot().phase != PlaybackPhase::Ready {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    playback
}

fn loopback() -> (mq_http::HttpClient, mq_http::RequestOptions) {
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let opts = mq_http::RequestOptions {
        allow_private: true,
        max_bytes: 64 << 20,
        ..Default::default()
    };
    (http, opts)
}

/// A download deleted while it plays leaves the list at once, but its file
/// stays until the play ends: the next range still answers (code review r1
/// M1).
#[tokio::test]
async fn a_download_deleted_while_it_plays_keeps_its_file_until_the_play_ends() {
    let data = video(8 << 20);
    let (url, _) = origin(data.clone(), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let playback = play(&core, ITEM, ITEM).await;
    core.downloads()
        .act(&ids, mq_runtime::downloads::Action::Remove)
        .unwrap();
    assert!(row_of(&core, &ids[0]).is_none(), "the row goes at once");
    let folder = core.downloads().job_folder(&ids[0]);
    let (http, opts) = loopback();
    let got = http
        .get_range(&playback.snapshot().url.unwrap(), 4 << 20, 4096, &opts)
        .await
        .unwrap();
    assert_eq!(got.status, 206);
    assert_eq!(got.body[..], data[4 << 20..(4 << 20) + 4096]);
    assert!(folder.exists());
    drop(playback);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while folder.exists() {
        assert!(std::time::Instant::now() < deadline, "the file stayed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A version too large for the free space now is still the pick: the job
/// waits for room, never fails, and goes on when room comes (code review
/// r1 M2).
#[tokio::test]
async fn a_download_too_large_for_now_waits_for_room() {
    let data = video(4 << 20);
    let (url, _) = origin(data.clone(), How::default()).await;
    let stream = serde_json::json!({
        "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url,
        "behaviorHints": { "filename": "Sintel.2010.1080p.WEB-DL.mp4", "videoSize": data.len() }
    });
    let core = core_with(addon_listing(stream, Duration::ZERO).await).await;
    core.set_free_bytes(512 << 20);
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "waits for space", |r| {
        r.reason == Some(DownloadReason::Space)
    })
    .await;
    assert_ne!(row_of(&core, &ids[0]).unwrap().state, DownloadState::Failed);
    core.set_free_bytes(100 << 30);
    core.downloads().set_allowed(true, None);
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
}

/// Offline with no meta, the player's next episode is the next downloaded
/// one, and none after the last downloaded (plans/offline-downloads.md
/// 4.12.5, code review r1 M3).
#[tokio::test]
async fn offline_the_next_episode_is_the_next_downloaded_one() {
    let core = bare();
    let data = video(1 << 20);
    for (id, video) in [("e1", "tt9:2:1"), ("e2", "tt9:2:2"), ("e3", "tt9:2:3")] {
        // E3 failed half-way: never offered as next (code review r2 N5).
        let state = if id == "e3" { "failed" } else { "done" };
        let mut row = record(
            id,
            "tt9",
            video,
            state,
            kept_version(
                "http://127.0.0.1:1/v.mp4",
                "http://127.0.0.1:9/manifest.json",
            ),
        );
        row.bytes_done = data.len() as i64;
        row.bytes_total = Some(data.len() as i64);
        core.library().add_download(&row).unwrap();
        put(&core, id, "video.mp4", &data);
    }
    core.downloads().relaunch();
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let first = play(&core, "tt9", "tt9:2:1").await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while first.snapshot().next.is_none() {
        assert!(std::time::Instant::now() < deadline, "no next episode");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(first.snapshot().next.unwrap().video_id, "tt9:2:2");
    drop(first);
    let last = play(&core, "tt9", "tt9:2:2").await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(last.snapshot().next.is_none());
}

/// Offline with the title's meta cached, the next episode is the next
/// released one whether or not it is downloaded, as the Up Next card has it
/// (plans/player-bar.md section 4, D14): only e1 is downloaded here, the
/// addon is gone, and e2 is still offered.
#[tokio::test]
async fn offline_with_cached_meta_the_next_episode_is_the_next_released_one() {
    // An addon that answers the series meta once, cached for an hour, and
    // then stops listening.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let meta = serde_json::json!({ "cacheMaxAge": 3600, "meta": {
        "id": "tt9", "type": "series", "name": "Show",
        "videos": [
            { "id": "tt9:2:1", "title": "One", "season": 2, "episode": 1,
              "released": "2020-01-01T00:00:00.000Z" },
            { "id": "tt9:2:2", "title": "Two", "season": 2, "episode": 2,
              "released": "2020-01-08T00:00:00.000Z" }
        ]
    }})
    .to_string();
    let served = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 2048];
        let _ = socket.read(&mut buf).await;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{meta}",
            meta.len()
        );
        socket.write_all(resp.as_bytes()).await.unwrap();
    });
    let manifest = serde_json::json!({
        "id": "local", "version": "1.0.0", "name": "Local",
        "types": ["series"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    let core = core_with(row).await;
    core.fetch_meta("series", "tt9").await.unwrap();
    served.await.unwrap();

    let data = video(1 << 20);
    let mut e1 = record(
        "e1",
        "tt9",
        "tt9:2:1",
        "done",
        kept_version(
            "http://127.0.0.1:1/v.mp4",
            "http://127.0.0.1:9/manifest.json",
        ),
    );
    e1.bytes_done = data.len() as i64;
    e1.bytes_total = Some(data.len() as i64);
    core.library().add_download(&e1).unwrap();
    put(&core, "e1", "video.mp4", &data);
    core.downloads().relaunch();
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let first = play(&core, "tt9", "tt9:2:1").await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while first.snapshot().next.is_none() {
        assert!(std::time::Instant::now() < deadline, "no next episode");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(first.snapshot().next.unwrap().video_id, "tt9:2:2");
}

/// Two jobs starting together never take the same room: with space for
/// one, the second waits until the first is done (code review r1 M7).
#[tokio::test]
async fn two_jobs_never_take_the_same_room() {
    let (url, _) = origin(
        video(8 << 20),
        How {
            slow: true,
            ..How::default()
        },
    )
    .await;
    let stream = serde_json::json!({ "name": "Direct\n1080p", "title": "Show.S02.1080p.WEB-DL.mp4", "url": url });
    let core = core_with(addon_listing(stream, Duration::ZERO).await).await;
    // Room for one 8 MiB file over the 1 GiB a download leaves.
    core.set_free_bytes((1 << 30) + (12 << 20));
    let season = AddRequest {
        kind: "series".into(),
        item_id: "tt9".into(),
        scope: Scope::Season,
        video_id: None,
        season: Some(2),
        params: serde_json::json!({}),
        pick: None,
    };
    let ids = mq_runtime::downloads::add(&core, season).await.unwrap();
    assert_eq!(ids.len(), 2);
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let mut waited = false;
    loop {
        let rows: Vec<_> = ids.iter().filter_map(|id| row_of(&core, id)).collect();
        // Writing: a job that only checks its room writes nothing.
        let writing = rows
            .iter()
            .filter(|r| {
                r.state == DownloadState::Running
                    && r.bytes_done > 0
                    && r.bytes_total.is_some_and(|t| r.bytes_done < t)
            })
            .count();
        assert!(writing <= 1, "both write on one room: {rows:?}");
        waited |= rows.iter().any(|r| r.reason == Some(DownloadReason::Space));
        if rows.iter().all(|r| r.state == DownloadState::Done) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{rows:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(waited);
}

/// A `.part` the folder lost bytes of (restored from a backup) starts
/// again from 0 instead of growing after a hole, and a file renamed whole
/// before a kill is done with no request (code review r1 m2).
#[tokio::test]
async fn a_short_part_starts_again_and_a_renamed_file_is_done() {
    let data = video(4 << 20);
    let (url, seen) = origin(data.clone(), How::default()).await;
    let core = bare();
    let mut short = record(
        "s1",
        ITEM,
        ITEM,
        "running",
        kept_version(&url, "http://127.0.0.1:9/manifest.json"),
    );
    short.bytes_done = 2 << 20;
    short.bytes_total = Some(data.len() as i64);
    core.library().add_download(&short).unwrap();
    put(&core, "s1", "video.mp4.part", &data[..1 << 20]);
    core.downloads().relaunch();
    core.downloads().set_link(Link::default());
    until(&core, "s1", "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, "s1"), *data);
    assert_eq!(
        seen.ranges.lock().unwrap().last().map(String::as_str),
        Some("0-")
    );

    let asked = seen.answers.load(Ordering::SeqCst);
    let mut renamed = record(
        "r1",
        "tt9",
        "tt9:2:1",
        "running",
        kept_version(&url, "http://127.0.0.1:9/manifest.json"),
    );
    renamed.bytes_done = data.len() as i64;
    renamed.bytes_total = Some(data.len() as i64);
    core.library().add_download(&renamed).unwrap();
    put(&core, "r1", "video.mp4", &data);
    core.downloads().relaunch();
    until(&core, "r1", "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(seen.answers.load(Ordering::SeqCst), asked);
}

/// A folder whose file cannot be opened waits for space a minute at least,
/// never asking its source again and again (code review r1 M6).
#[tokio::test]
async fn an_unopenable_file_waits_without_a_loop() {
    let (url, seen) = origin(video(1 << 20), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    // A folder where the file goes: it cannot be opened for writing.
    std::fs::create_dir_all(core.downloads().job_folder(&ids[0]).join("video.mp4.part")).unwrap();
    core.downloads().set_link(Link::default());
    until(&core, &ids[0], "waits for space", |r| {
        r.reason == Some(DownloadReason::Space)
    })
    .await;
    let asked = seen.answers.load(Ordering::SeqCst);
    // The platform's minute report of room does not end an I/O wait
    // (code review r2 R4).
    for _ in 0..4 {
        core.downloads().set_allowed(true, Some(64 << 30));
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert_eq!(seen.answers.load(Ordering::SeqCst), asked, "asked again");
    assert_eq!(
        row_of(&core, &ids[0]).unwrap().reason,
        Some(DownloadReason::Space)
    );
}

/// A row left `running` with nothing running (a stale write) goes back in
/// the queue and finishes; a pause written then stays (code review r1 M5).
#[tokio::test]
async fn a_running_row_with_no_run_is_repaired_and_a_pause_stays() {
    let data = video(4 << 20);
    let (url, _) = origin(data.clone(), How::default()).await;
    let core = bare();
    let row = record(
        "x1",
        ITEM,
        ITEM,
        "queued",
        kept_version(&url, "http://127.0.0.1:9/manifest.json"),
    );
    core.library().add_download(&row).unwrap();
    core.downloads()
        .act(&["x1".to_owned()], mq_runtime::downloads::Action::Pause)
        .unwrap();
    core.downloads().relaunch();
    core.downloads().set_link(Link::default());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(row_of(&core, "x1").unwrap().state, DownloadState::Paused);
    // Running in the table, with no run behind it.
    core.library()
        .update_download("x1", |r| {
            r.state = "running".into();
            true
        })
        .unwrap();
    core.downloads().set_allowed(true, None);
    until(&core, "x1", "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, "x1"), *data);
}

/// The ops: pause holds a job, resume and retry queue it again, remove
/// takes it and its files; a kids profile may do none of them (code review
/// r1 m10).
#[tokio::test]
async fn every_op_does_its_part_and_none_is_a_kids_one() {
    use mq_runtime::downloads::Action;
    let data = video(4 << 20);
    let (url, _) = origin(data.clone(), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    let d = core.downloads();
    d.act(&ids, Action::Pause).unwrap();
    d.set_link(Link::default());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(row_of(&core, &ids[0]).unwrap().state, DownloadState::Paused);
    d.act(&ids, Action::Resume).unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    // Missing (its file gone), then Try again.
    std::fs::remove_dir_all(d.job_folder(&ids[0])).unwrap();
    d.relaunch();
    until(&core, &ids[0], "missing", |r| {
        r.state == DownloadState::Missing
    })
    .await;
    d.act(&ids, Action::Retry { mobile_data: false }).unwrap();
    until(&core, &ids[0], "done again", |r| {
        r.state == DownloadState::Done
    })
    .await;
    // A kids profile changes nothing.
    core.set_pin("default", Some("1357"), None).await.unwrap();
    let kid = core.for_profile(&core.create_kids_profile("Kid", "fox:amber").unwrap());
    assert!(mq_runtime::downloads::owner(&kid).is_err());
    assert!(mq_runtime::downloads::add(&kid, movie()).await.is_err());
    d.act(&ids, Action::Remove).unwrap();
    assert!(row_of(&core, &ids[0]).is_none());
    gone(&d.job_folder(&ids[0])).await;
}

/// Until the app has told the network once, no job runs: a launch on
/// mobile data with Wi-Fi only on never starts one first (code review r1
/// m8).
#[tokio::test]
async fn nothing_runs_before_the_app_tells_the_network() {
    let (url, seen) = origin(video(1 << 20), How::default()).await;
    let core = bare();
    let row = record(
        "n1",
        ITEM,
        ITEM,
        "queued",
        kept_version(&url, "http://127.0.0.1:9/manifest.json"),
    );
    core.library().add_download(&row).unwrap();
    core.downloads().relaunch();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(seen.answers.load(Ordering::SeqCst), 0);
    assert_ne!(row_of(&core, "n1").unwrap().state, DownloadState::Running);
    core.downloads().set_link(Link::default());
    until(&core, "n1", "done", |r| r.state == DownloadState::Done).await;
}

/// A downloaded file the player cannot read turns the download to "Try
/// again", and the play goes on with the same version online (plans/
/// offline-downloads.md 4.8, code review r1 m4).
#[tokio::test]
async fn a_download_that_does_not_play_fails_and_the_play_goes_online() {
    let data = video(4 << 20);
    let (url, seen) = origin(data.clone(), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    let playback = play_when(&core, ITEM, ITEM, true).await;
    let asked = seen.answers.load(Ordering::SeqCst);
    let first = playback.snapshot().attempt;
    playback.fail_over(None, Some("unreadable"), None, "no moov");
    until(&core, &ids[0], "failed", |r| {
        r.state == DownloadState::Failed && r.reason == Some(DownloadReason::Source)
    })
    .await;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while playback.snapshot().phase != PlaybackPhase::Ready || playback.snapshot().attempt == first
    {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The next source reads from the origin, not the kept file.
    let (http, opts) = loopback();
    let got = http
        .get_range(&playback.snapshot().url.unwrap(), 0, 4096, &opts)
        .await
        .unwrap();
    assert_eq!(got.body[..], data[..4096]);
    assert!(seen.answers.load(Ordering::SeqCst) > asked);
}

/// A download that played well and then hiccups stays downloaded, and a
/// download's own file failing teaches the device nothing about its
/// version, before playback or after (code review r2 N2, N4).
#[tokio::test]
async fn a_download_that_played_keeps_and_its_file_teaches_nothing() {
    let data = video(4 << 20);
    let (url, _) = origin(data.clone(), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    let playback = play_when(&core, ITEM, ITEM, true).await;
    for ms in (0..=32_000).step_by(1000) {
        playback.report_progress(ms, 600_000, false);
    }
    playback.fail_over(None, Some("decode"), None, "a bad frame at 32 s");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(row_of(&core, &ids[0]).unwrap().state, DownloadState::Done);
    assert!(mq_runtime::speed::did_not_play(core.library(), now()).is_empty());
    drop(playback);
    let playback = play_when(&core, ITEM, ITEM, true).await;
    playback.fail_over(None, Some("decode"), None, "no frame");
    until(&core, &ids[0], "failed", |r| {
        r.state == DownloadState::Failed
    })
    .await;
    assert!(mq_runtime::speed::did_not_play(core.library(), now()).is_empty());
}

/// A source that keeps failing (a 503) is tried three more times, the
/// waits shortened by the test hook; with no other source in the pick the
/// job then fails with the source's reason. A dead link (404) is not tried
/// again (plans/offline-downloads.md 4.7.4, code review r1 m10).
#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn a_failing_source_is_tried_three_more_times_and_a_dead_link_once() {
    mq_runtime::downloads::short_backoff_for_tests();
    for (status, tries) in [(503, 4), (404, 1)] {
        let (url, seen) = origin(
            video(1 << 20),
            How {
                status: Some(status),
                ..How::default()
            },
        )
        .await;
        let core = core_with(addon(&url, Duration::ZERO).await).await;
        let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
        until(&core, &ids[0], "failed", |r| {
            r.state == DownloadState::Failed && r.reason == Some(DownloadReason::Source)
        })
        .await;
        assert_eq!(seen.answers.load(Ordering::SeqCst), tries, "{status}");
    }
}

/// Picked before the addons answered, a download's chain is its own file
/// alone; when they answer later and the file fails, the play still goes
/// on to the same version online (plans/offline-downloads.md 4.12.3).
#[tokio::test]
async fn a_download_picked_before_the_addons_still_fails_over_online() {
    let data = video(4 << 20);
    let (url, seen) = origin(data.clone(), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    // The addon now answers a second late: the download is picked first.
    let slow = addon(&url, Duration::from_secs(1)).await;
    core.library()
        .set_addons(core.profile(), std::slice::from_ref(&slow), &core.hlc())
        .unwrap();
    let view = VersionsView::open(&core, params());
    let pick = view.snapshot().recommended.expect("the download at once");
    assert_eq!(pick.on_device.as_deref(), Some("Downloaded"));
    assert!(!view.snapshot().done);
    let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), target(), &view.plays());
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while playback.snapshot().phase != PlaybackPhase::Ready || !view.snapshot().done {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let asked = seen.answers.load(Ordering::SeqCst);
    let first = playback.snapshot().attempt;
    playback.fail_over(None, Some("unreadable"), None, "no moov");
    while playback.snapshot().phase != PlaybackPhase::Ready || playback.snapshot().attempt == first
    {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            playback.snapshot()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (http, opts) = loopback();
    let got = http
        .get_range(&playback.snapshot().url.unwrap(), 0, 4096, &opts)
        .await
        .unwrap();
    assert_eq!(got.body[..], data[..4096]);
    assert!(seen.answers.load(Ordering::SeqCst) > asked);
}

/// A source that needs a header (an addon's proxy `Authorization`) still
/// downloads: the header is never kept on the device, and the job asks the
/// addon for it again as it starts (code review r2 N1).
#[tokio::test]
async fn a_source_that_needs_a_header_downloads_and_keeps_no_secret() {
    let data = video(2 << 20);
    let (url, seen) = origin(
        data.clone(),
        How {
            auth: Some("Bearer t0ken"),
            ..How::default()
        },
    )
    .await;
    let stream = serde_json::json!({
        "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url,
        "behaviorHints": {
            "filename": "Sintel.2010.1080p.WEB-DL.mp4",
            "proxyHeaders": { "request": { "Authorization": "Bearer t0ken" } }
        }
    });
    let core = core_with(addon_listing(stream, Duration::ZERO).await).await;
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    until(&core, &ids[0], "done", |r| r.state == DownloadState::Done).await;
    assert_eq!(file_of(&core, &ids[0]), *data);
    assert!(seen.answers.load(Ordering::SeqCst) >= 1);
    let stored = core.library().download_of(ITEM, ITEM).unwrap().unwrap();
    let version = stored.version.unwrap();
    assert!(!version.contains("t0ken"), "{version}");
    assert!(
        !version.to_ascii_lowercase().contains("authorization"),
        "{version}"
    );
}

/// The header lives only with its addon: once the addon is removed, a job
/// that has not run has nothing to send and never downloads (code review
/// r2 N1).
#[tokio::test]
async fn a_removed_addon_takes_its_header_with_it() {
    let (url, _) = origin(
        video(2 << 20),
        How {
            auth: Some("Bearer t0ken"),
            ..How::default()
        },
    )
    .await;
    let stream = serde_json::json!({
        "name": "Direct\n1080p", "title": "Sintel.2010.1080p.WEB-DL.mp4", "url": url,
        "behaviorHints": {
            "filename": "Sintel.2010.1080p.WEB-DL.mp4",
            "proxyHeaders": { "request": { "Authorization": "Bearer t0ken" } }
        }
    });
    let core = core_with(addon_listing(stream, Duration::ZERO).await).await;
    core.downloads().set_link(Link {
        offline: true,
        ..Link::default()
    });
    let ids = mq_runtime::downloads::add(&core, movie()).await.unwrap();
    core.library()
        .set_addons(core.profile(), &[], &core.hlc())
        .unwrap();
    core.downloads().set_link(Link::default());
    until(&core, &ids[0], "fails", |r| {
        r.state == DownloadState::Failed
    })
    .await;
    assert!(core.downloads().file_path(&ids[0]).is_none());
}

/// Perf (plans/batch-13-perf.md, core-perf C5): what one download sends
/// while its bytes move, a 64 MiB film at 320 KiB/s (a percent every 2 s)
/// over `MQ_PERF_S` seconds (60 by default): the board's changes and the
/// Downloads view's pushes. Prints one JSON line. Run alone:
/// `cargo test -p mq-runtime --test downloads perf_ -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "perf measurement, a minute long"]
async fn perf_a_download_moving_bytes() {
    use futures_util::StreamExt;
    let secs: u64 = std::env::var("MQ_PERF_S")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let (url, _) = origin(
        video(64 << 20),
        How {
            slow: true,
            pace: Some(Duration::from_millis(200)),
            ..How::default()
        },
    )
    .await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
    let view = mq_runtime::DownloadsView::open(&core);
    let id = mq_runtime::downloads::add(&core, movie()).await.unwrap()[0].clone();
    until(&core, &id, "bytes move", |r| r.bytes_done > 0).await;
    let (_, mut pushes) = view.subscribe();
    let mut boards = core.downloads().board().subscribe();
    let (mut pushed, mut changed) = (0u32, 0u32);
    let end = tokio::time::sleep(Duration::from_secs(secs));
    tokio::pin!(end);
    loop {
        tokio::select! {
            () = &mut end => break,
            Some(_) = pushes.next() => pushed += 1,
            Some(_) = boards.next() => changed += 1,
        }
    }
    let row = row_of(&core, &id).unwrap();
    assert_eq!(
        row.state,
        DownloadState::Running,
        "still downloading: {row:?}"
    );
    println!(
        "{}",
        serde_json::json!({
            "secs": secs, "board_changes": changed, "downloads_pushes": pushed,
            "bytes_done": row.bytes_done, "bytes_total": row.bytes_total,
        })
    );
}
