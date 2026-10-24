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
#[derive(Clone, Copy, Default)]
struct How {
    /// The first answer stops after this many body bytes.
    cut_first_after: Option<usize>,
    /// Every answer is 200 with the whole file.
    ignore_ranges: bool,
    /// An HTML page instead of the file.
    html: bool,
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
            let (data, seen) = (data.clone(), out.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let nth = seen.answers.fetch_add(1, Ordering::SeqCst);
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

/// A season's add queues its aired episodes in order and never one not out
/// yet; a kids profile adds nothing and sees only its list.
#[tokio::test]
async fn a_season_queues_its_aired_episodes_and_kids_add_nothing() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
    let core = core_with(addon(&url, Duration::ZERO).await).await;
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
    for (id, video) in [("e1", "tt9:2:1"), ("e2", "tt9:2:2")] {
        let mut row = record(
            id,
            "tt9",
            video,
            "done",
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
        public_name: None,
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

/// Smart downloads: a downloaded episode watched to its end brings the
/// next one, and goes once the next is done; a kid half way through it
/// keeps it (4.14).
#[tokio::test]
async fn a_finished_episode_brings_the_next_and_goes() {
    let (url, _) = origin(video(1 << 20), How::default()).await;
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
    let e1 = || PlayTarget {
        kind: "series".into(),
        item_id: "tt9".into(),
        video_id: "tt9:2:1".into(),
        name: None,
        poster: None,
        season: Some(2),
        episode: Some(1),
    };
    let play_to_the_end = |core: Core| async move {
        let view = VersionsView::open(
            &core,
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
        let playback = PlaybackView::open_picked(&core, view.pick(&pick.key), e1(), &view.plays());
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while playback.snapshot().phase != PlaybackPhase::Ready {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        playback.report_progress(3_000_000, 3_100_000, false);
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(playback);
    };
    play_to_the_end(core.clone()).await;
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
