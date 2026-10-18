use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::{AddonRow, CachedResponse};
use mq_protocol::decode_streams;
use mq_runtime::{Core, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView};
use mq_streams::{ParsedStream, normalize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

/// Origin with a live file that needs `x-key: 1` and a dead one.
async fn origin() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let resp: Vec<u8> = if head.starts_with("get /ok.mkv") && head.contains("x-key: 1")
                {
                    let mut r = b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/10\r\nContent-Length: 1\r\nConnection: close\r\n\r\n".to_vec();
                    r.push(b'M');
                    r
                } else if head.starts_with("get /full.mkv") {
                    // Ignores Range and starts sending a 1.1 GB file.
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1180090590\r\nConnection: close\r\n\r\nxxxx"
                        .to_vec()
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec()
                };
                let _ = socket.write_all(&resp).await;
            });
        }
    });
    format!("http://{addr}")
}

const LOCAL_ADDON: &str = "http://127.0.0.1:9/manifest.json";

/// Installs a local addon (so its LAN URLs are allowed) and returns the
/// parsed candidates in the order given.
fn setup(core: &Core, streams: &str) -> Vec<ParsedStream> {
    install(core, &[row(LOCAL_ADDON, "local", "stream", true)]);
    listed(LOCAL_ADDON, "local", "Local", streams)
}

/// An installed addon serving `resource`, named after its id.
fn row(transport_url: &str, id: &str, resource: &str, allow_private: bool) -> AddonRow {
    let manifest = serde_json::json!({
        "id": id, "version": "1.0.0", "name": id,
        "types": ["movie"], "resources": [resource], "catalogs": []
    });
    AddonRow {
        transport_url: transport_url.into(),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private,
    }
}

/// Installs `rows` in this order.
fn install(core: &Core, rows: &[AddonRow]) {
    let rows: Vec<AddonRow> = (0..)
        .zip(rows)
        .map(|(position, r)| AddonRow {
            position,
            ..r.clone()
        })
        .collect();
    core.library()
        .set_addons(core.profile(), &rows, &core.hlc())
        .unwrap();
}

/// Candidates as Versions builds them: each carries the transport URL of the
/// install that listed it.
fn listed(addon_url: &str, id: &str, name: &str, streams: &str) -> Vec<ParsedStream> {
    let addon_url = Url::parse(addon_url).unwrap();
    decode_streams(streams.as_bytes())
        .unwrap()
        .streams
        .iter()
        .enumerate()
        .map(|(i, w)| ParsedStream {
            addon_url: Some(addon_url.clone()),
            ..normalize(w, id, name, 0, i as u32)
        })
        .collect()
}

/// An origin that serves a one-byte file to anyone and counts the requests.
async fn counted_origin() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(AtomicUsize::new(0));
    let count = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/10\r\nContent-Length: 1\r\nConnection: close\r\n\r\nM")
                    .await;
            });
        }
    });
    (base, asked)
}

/// What the view says when the server refuses a private address.
const PRIVATE: &str = "The source points to a private network address";

#[tokio::test]
async fn a_public_install_never_borrows_a_local_ones_reach() {
    let (base, asked) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    install(&core, &[row(LOCAL_ADDON, "local", "stream", true)]);
    // Another install declares the local addon's manifest id.
    let streams = format!(r#"{{"streams":[{{"name":"Dup","url":"{base}/ok.mkv"}}]}}"#);
    let candidates = listed(
        "https://dup.example/manifest.json",
        "local",
        "Local",
        &streams,
    );
    let view = PlaybackView::open(&core, candidates, target());
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Failed, "{s:?}");
    assert_eq!(s.attempts[0].error, PRIVATE);
    assert_eq!(asked.load(Ordering::SeqCst), 0, "the origin was asked");
}

#[tokio::test]
async fn a_synced_bit_grants_nothing() {
    let (base, asked) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    // A row with a public transport URL that arrived with the bit set.
    let synced = "https://synced.example/manifest.json";
    install(&core, &[row(synced, "synced", "stream", true)]);
    let streams = format!(r#"{{"streams":[{{"name":"Synced","url":"{base}/ok.mkv"}}]}}"#);
    let view = PlaybackView::open(
        &core,
        listed(synced, "synced", "Synced", &streams),
        target(),
    );
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Failed, "{s:?}");
    assert_eq!(s.attempts[0].error, PRIVATE);
    assert_eq!(asked.load(Ordering::SeqCst), 0, "the origin was asked");
}

fn target() -> PlayTarget {
    PlayTarget {
        kind: "movie".into(),
        item_id: "tt1727587".into(),
        video_id: "tt1727587".into(),
        name: Some("Sintel".into()),
        ..PlayTarget::default()
    }
}

async fn settled(view: &PlaybackView) -> PlaybackSnapshot {
    for _ in 0..200 {
        let s = view.snapshot();
        if s.phase != PlaybackPhase::Resolving {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("playback never settled: {:?}", view.snapshot());
}

/// A loopback origin with a one-byte video, a subtitles answer naming
/// `/addon.srt` on itself, and two SubRip files.
async fn subtitle_origin() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let own = base.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let base = own.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let (status, body) = if head.starts_with("GET /subtitles/") {
                    let url = format!("{base}/addon.srt");
                    let list =
                        serde_json::json!({"subtitles": [{"id": "1", "lang": "eng", "url": url}]});
                    ("200 OK", list.to_string())
                } else if head.starts_with("GET /video.mkv") {
                    (
                        "206 Partial Content\r\nContent-Range: bytes 0-0/10",
                        "M".into(),
                    )
                } else {
                    ("200 OK", "1\n00:00:01,000 --> 00:00:02,000\nHello\n".into())
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    base
}

#[tokio::test]
async fn subtitle_tracks_take_their_reach_from_their_install() {
    let base = subtitle_origin().await;
    let core = Core::temporary("p").unwrap();
    let stream_addon = format!("{base}/own/manifest.json");
    install(
        &core,
        &[
            // First: a public install with the subtitles addon's id.
            row(
                "https://pub.invalid/manifest.json",
                "subs",
                "subtitles",
                false,
            ),
            row(&format!("{base}/manifest.json"), "subs", "subtitles", true),
            // The stream's install arrived without the bit; its URL decides.
            row(&stream_addon, "own", "stream", false),
        ],
    );
    let streams = format!(
        r#"{{"streams":[{{"url":"{base}/video.mkv","subtitles":[{{"id":"own","lang":"eng","url":"{base}/own.srt"}}]}}]}}"#
    );
    let view = PlaybackView::open(
        &core,
        listed(&stream_addon, "own", "Own", &streams),
        target(),
    );
    let mut done = None;
    for _ in 0..1500 {
        let s = view.snapshot();
        if s.phase != PlaybackPhase::Resolving && !s.subtitles_pending {
            done = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let s = done.expect("subtitles settled");
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    let sources: Vec<_> = s.subtitles.iter().map(|t| t.source.as_str()).collect();
    assert_eq!(sources, ["Own", "subs"]);
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    for track in &s.subtitles {
        let served = http.get(&track.url, &local).await;
        let body = served.unwrap_or_else(|e| panic!("{}'s track: {e}", track.source));
        assert!(String::from_utf8_lossy(&body.body).contains("Hello"));
    }
}

/// A loopback origin whose video ignores `Range` (so no subtitle hash is
/// sent and the subtitles request has no extras) and which serves
/// `/pub.srt`, counting requests for it.
async fn passthrough_origin() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let srt = Arc::new(AtomicUsize::new(0));
    let count = srt.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let count = count.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let body = if head.starts_with("GET /pub.srt") {
                    count.fetch_add(1, Ordering::SeqCst);
                    "1\n00:00:01,000 --> 00:00:02,000\nHello\n"
                } else {
                    "M"
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (base, srt)
}

#[tokio::test]
async fn a_public_subtitles_answer_gets_no_reach() {
    let (base, srt) = passthrough_origin().await;
    let core = Core::temporary("p").unwrap();
    let public = "https://pubsubs.example/manifest.json";
    let stream_addon = format!("{base}/manifest.json");
    install(
        &core,
        &[
            row(public, "pubsubs", "subtitles", false),
            row(&stream_addon, "own", "stream", true),
        ],
    );
    // The public install really answers, from the response cache, with a
    // track on the loopback origin.
    let answer = serde_json::json!({"subtitles": [{"id": "1", "lang": "eng", "url": format!("{base}/pub.srt")}]});
    let cached = CachedResponse {
        body: answer.to_string().into_bytes(),
        stored_ms: mq_library::hlc::now_ms() as i64,
        max_age_s: 3600,
        stale_revalidate_s: 0,
        stale_error_s: 0,
    };
    let key = "https://pubsubs.example/subtitles/movie/tt1727587.json";
    let removals = core.library().removals();
    assert!(core.library().cache_put(key, &cached, removals).unwrap());
    let streams = format!(r#"{{"streams":[{{"url":"{base}/video.mkv"}}]}}"#);
    let view = PlaybackView::open(
        &core,
        listed(&stream_addon, "own", "Own", &streams),
        target(),
    );
    let mut done = None;
    for _ in 0..1500 {
        let s = view.snapshot();
        if s.phase != PlaybackPhase::Resolving && !s.subtitles_pending {
            done = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let s = done.expect("subtitles settled");
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    let track = s
        .subtitles
        .iter()
        .find(|t| t.source == "pubsubs")
        .unwrap_or_else(|| panic!("the public install answered: {s:?}"));
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let served = http.get(&track.url, &local).await;
    assert!(
        matches!(served, Err(HttpError::Status { status: 502, .. })),
        "{served:?}"
    );
    assert_eq!(
        srt.load(Ordering::SeqCst),
        0,
        "the loopback file was fetched"
    );
}

#[tokio::test]
async fn falls_over_to_the_first_source_that_answers() {
    let base = origin().await;
    let core = Core::temporary("p").unwrap();
    // Without debrid, a store build fails a torrent at once. A full build
    // (feature `torrent`) would ask the real DHT for it, so it is left out.
    let torrent = if cfg!(feature = "torrent") {
        ""
    } else {
        r#"{"name":"Torrent","title":"Sintel.2010.1080p","infoHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"#
    };
    let streams = format!(
        r#"{{"streams":[{torrent}
          {{"name":"Dead","url":"{base}/dead.mkv"}},
          {{"name":"Live","url":"{base}/ok.mkv","behaviorHints":{{"filename":"Sintel.2010.mkv","proxyHeaders":{{"request":{{"x-key":"1"}}}}}}}}
        ]}}"#
    );
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    let mut errors: Vec<_> = s.attempts.iter().map(|a| a.error.as_str()).collect();
    if !torrent.is_empty() {
        assert!(errors.remove(0).contains("debrid account"), "{errors:?}");
    }
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("404"), "{errors:?}");

    // The player gets a loopback URL; the origin's header is injected for it.
    let url = s.url.unwrap();
    assert!(url.starts_with("http://127.0.0.1:") && url.ends_with("/Sintel.2010.mkv"));
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let resp = http.get_range(&url, 0, 1, &local).await.unwrap();
    assert_eq!(&resp.body[..], b"M");

    // A player error moves on; with nothing left the view says so.
    view.fail_over(None, None, None, "decoder error");
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Failed);
    assert_eq!(s.attempts.last().unwrap().error, "decoder error");
    assert!(http.get_range(&url, 0, 1, &local).await.is_err());
}

#[tokio::test]
async fn saves_progress_resumes_and_marks_finished() {
    let base = origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = format!(
        r#"{{"streams":[{{"name":"Live","url":"{base}/ok.mkv","behaviorHints":{{"proxyHeaders":{{"request":{{"x-key":"1"}}}}}}}}]}}"#
    );
    let candidates = setup(&core, &streams);

    let view = PlaybackView::open(&core, candidates.clone(), target());
    assert_eq!(view.snapshot().resume_ms, None);
    view.report_progress(60_000, 900_000);
    // Throttled: this one is held until the view closes.
    view.report_progress(61_000, 900_000);
    assert_eq!(
        core.library().progress("p", "tt1727587").unwrap(),
        Some((60_000, 900_000))
    );
    drop(view);
    assert_eq!(
        core.library().progress("p", "tt1727587").unwrap(),
        Some((61_000, 900_000))
    );
    let cw = core.library().continue_watching("p", 10).unwrap();
    assert_eq!(cw[0].name, "Sintel");

    let view = PlaybackView::open(&core, candidates.clone(), target());
    assert_eq!(view.snapshot().resume_ms, Some(61_000));
    view.report_progress(890_000, 900_000);
    drop(view);
    assert!(
        core.library()
            .continue_watching("p", 10)
            .unwrap()
            .is_empty()
    );
    let view = PlaybackView::open(&core, candidates, target());
    assert_eq!(view.snapshot().resume_ms, None);
}

#[tokio::test]
async fn origins_that_ignore_range_still_count_as_alive() {
    let base = origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = format!(r#"{{"streams":[{{"name":"Full","url":"{base}/full.mkv"}}]}}"#);
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
}

#[tokio::test]
async fn a_late_fail_over_never_drops_the_next_source() {
    let base = origin().await;
    let core = Core::temporary("p").unwrap();
    // One link listed twice, as two installs do: both sources have one key.
    let live = format!(
        r#"{{"name":"Live","url":"{base}/ok.mkv","behaviorHints":{{"proxyHeaders":{{"request":{{"x-key":"1"}}}}}}}}"#
    );
    let streams = format!(r#"{{"streams":[{live},{live}]}}"#);
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let first = settled(&view).await;
    assert_eq!(first.phase, PlaybackPhase::Ready, "{first:?}");
    // The player reports facts; the attempt list gets the core's words.
    let http = (Some("http"), Some(404), "The source answered with HTTP 404");
    view.fail_over(Some(first.attempt), http.0, http.1, http.2);
    let second = settled(&view).await;
    assert_eq!(second.phase, PlaybackPhase::Ready, "{second:?}");
    assert_eq!(second.key, first.key);
    assert!(second.attempt > first.attempt, "{second:?}");
    let failed: Vec<_> = second.attempts.iter().map(|a| a.error.as_str()).collect();
    assert_eq!(failed, ["The link no longer exists (404)"]);
    // The first source's player reports a second error after the move.
    view.fail_over(Some(first.attempt), Some("decode"), None, "late");
    let now = view.snapshot();
    assert_eq!(
        (now.phase, now.attempt),
        (PlaybackPhase::Ready, second.attempt)
    );
    // The source playing now can still fail over.
    view.fail_over(Some(second.attempt), Some("decode"), None, "decoder error");
    let last = settled(&view).await;
    assert_eq!(last.phase, PlaybackPhase::Failed);
    assert_eq!(
        last.attempts.last().map(|a| a.error.as_str()),
        Some("This device cannot decode the video")
    );
}

/// A 200 KiB file with Range support plus a subtitles addon that records the
/// path it was asked for.
/// Logs the subtitles paths asked for, and the video's `Range` values.
#[derive(Clone, Default)]
struct Asked {
    subtitles: Arc<Mutex<Vec<String>>>,
    ranges: Arc<Mutex<Vec<String>>>,
}

async fn file_and_subtitles() -> (String, Asked) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Asked::default();
    let log = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let reply = if path.starts_with("/subtitles/") {
                    log.subtitles.lock().unwrap().push(path);
                    let body = r#"{"subtitles":[{"id":"1","lang":"eng","url":"https://subs.example/1.srt"},{"id":"2","lang":"tur","url":"https://subs.example/2.srt"}]}"#;
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .into_bytes()
                } else {
                    let file = video();
                    let range = head.lines().find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("range: bytes=")
                            .map(str::to_owned)
                    });
                    log.ranges
                        .lock()
                        .unwrap()
                        .extend(range.as_ref().map(|r| r.trim().to_owned()));
                    let (start, end) = range
                        .and_then(|r| {
                            let (a, b) = r.trim().split_once('-')?;
                            let a: usize = a.parse().ok()?;
                            let b: usize = b.parse().unwrap_or(file.len() - 1).min(file.len() - 1);
                            Some((a, b))
                        })
                        .unwrap_or((0, file.len() - 1));
                    let mut r = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        file.len(),
                        end - start + 1
                    )
                    .into_bytes();
                    r.extend_from_slice(&file[start..=end]);
                    r
                };
                let _ = socket.write_all(&reply).await;
            });
        }
    });
    (base, asked)
}

fn video() -> Vec<u8> {
    (0..200 * 1024u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect()
}

/// The OpenSubtitles hash, written out independently of the core's version.
fn expected_hash(file: &[u8]) -> String {
    let words = |part: &[u8]| -> u64 {
        part.chunks(8)
            .map(|w| u64::from_le_bytes(w.try_into().unwrap()))
            .fold(0u64, u64::wrapping_add)
    };
    let n = file.len();
    let sum = (n as u64)
        .wrapping_add(words(&file[..65536]))
        .wrapping_add(words(&file[n - 65536..]));
    format!("{sum:016x}")
}

/// Plays the video at `base` with its addon installed for streams and
/// subtitles, until the subtitles settle.
async fn played_with_subtitles(base: &str) -> (PlaybackView, PlaybackSnapshot) {
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({"subtitleLanguages": ["tur"]}))
        .unwrap();
    let manifest = serde_json::json!({
        "id": "subs", "version": "1.0.0", "name": "Subs",
        "types": ["movie"], "resources": ["stream", "subtitles"], "catalogs": []
    });
    core.library()
        .set_addons(
            core.profile(),
            &[AddonRow {
                transport_url: format!("{base}/manifest.json"),
                manifest_json: manifest.to_string(),
                position: 0,
                enabled: true,
                allow_private: true,
            }],
            &core.hlc(),
        )
        .unwrap();
    let streams = format!(
        r#"{{"streams":[{{"url":"{base}/video.mkv","behaviorHints":{{"filename":"Sintel.2010.mkv"}},
             "subtitles":[{{"id":"own","lang":"eng","url":"https://own.example/en.srt"}}]}}]}}"#
    );
    let candidates = listed(&format!("{base}/manifest.json"), "subs", "Subs", &streams);
    let view = PlaybackView::open(&core, candidates, target());
    for _ in 0..500 {
        let s = view.snapshot();
        if s.phase == PlaybackPhase::Ready && !s.subtitles_pending {
            return (view, s);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("subtitles never settled: {:?}", view.snapshot());
}

#[tokio::test]
async fn finds_subtitles_by_hash_size_and_filename() {
    let (base, asked) = file_and_subtitles().await;
    let (_view, s) = played_with_subtitles(&base).await;
    // The preferred language comes first; the rest keep arrival order (the
    // stream's own track, then the addon's).
    let order: Vec<_> = s
        .subtitles
        .iter()
        .map(|t| (t.lang.as_str(), t.source.as_str()))
        .collect();
    assert_eq!(order[0].0, "tur");
    assert_eq!(order[1], ("eng", "Subs"));
    assert_eq!(order[2].0, "eng");
    // Every track is served by the loopback server as WebVTT, one address
    // per file.
    let urls: std::collections::HashSet<_> = s.subtitles.iter().map(|t| &t.url).collect();
    assert_eq!(urls.len(), 3);
    assert!(
        urls.iter().all(|u| u.starts_with("http://127.0.0.1:")
            && u.contains("/sub/")
            && u.ends_with(".vtt")),
        "{urls:?}"
    );
    let path = asked.subtitles.lock().unwrap()[0].clone();
    let hash = expected_hash(&video());
    assert_eq!(
        path,
        format!(
            "/subtitles/movie/tt1727587/videoHash={hash}&videoSize=204800&filename=Sintel.2010.mkv.json"
        )
    );
}

#[tokio::test]
async fn every_ready_url_is_a_loopback_url() {
    let (base, _) = file_and_subtitles().await;
    let (_view, s) = played_with_subtitles(&base).await;
    let url = s.url.unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:") && url.contains("/s/"),
        "{url}"
    );
    assert!(!url.contains(&base), "{url}");
    for track in &s.subtitles {
        assert!(track.url.starts_with("http://127.0.0.1:"), "{}", track.url);
    }
}

#[tokio::test]
async fn the_hash_is_read_in_process() {
    let (base, asked) = file_and_subtitles().await;
    let (_view, _) = played_with_subtitles(&base).await;
    let path = asked.subtitles.lock().unwrap()[0].clone();
    let hash = expected_hash(&video());
    assert!(path.contains(&format!("videoHash={hash}")), "{path}");
    // The playback's cursors ask open ranges; a closed one is a second
    // reader on the origin.
    let ranges = asked.ranges.lock().unwrap().clone();
    assert!(!ranges.is_empty());
    assert!(ranges.iter().all(|r| r.ends_with('-')), "{ranges:?}");
}

/// Meta addon for a three-episode season plus a special; episode 3 is not
/// out yet. Also serves a one-byte video.
async fn series_addon() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let body = if head.starts_with("GET /meta/") {
                    let ep = |s: u32, e: u32, released: &str| {
                        serde_json::json!({"id": format!("tt9:{s}:{e}"), "title": format!("Episode {e}"),
                                           "season": s, "episode": e, "released": released})
                    };
                    serde_json::json!({"meta": {"id": "tt9", "type": "series", "name": "Show", "videos": [
                        ep(1, 2, "2020-01-08T00:00:00Z"), ep(0, 1, "2019-12-01T00:00:00Z"),
                        ep(1, 1, "2020-01-01T00:00:00Z"), ep(1, 3, "2099-01-01T00:00:00Z")
                    ]}})
                    .to_string()
                } else {
                    "M".into()
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    base
}

#[tokio::test]
async fn offers_the_next_released_episode_and_the_binge_group() {
    let base = series_addon().await;
    let core = Core::temporary("p").unwrap();
    let manifest = serde_json::json!({
        "id": "show", "version": "1.0.0", "name": "Show",
        "types": ["series"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    core.library()
        .set_addons(
            core.profile(),
            &[AddonRow {
                transport_url: format!("{base}/manifest.json"),
                manifest_json: manifest.to_string(),
                position: 0,
                enabled: true,
                allow_private: true,
            }],
            &core.hlc(),
        )
        .unwrap();
    let streams = format!(
        r#"{{"streams":[{{"url":"{base}/video.mkv","behaviorHints":{{"bingeGroup":"show|1080p|NTb"}}}}]}}"#
    );
    let candidates = listed(&format!("{base}/manifest.json"), "show", "Show", &streams);
    let play = |episode: u32| PlayTarget {
        kind: "series".into(),
        item_id: "tt9".into(),
        video_id: format!("tt9:1:{episode}"),
        season: Some(1),
        episode: Some(episode),
        ..PlayTarget::default()
    };

    let view = PlaybackView::open(&core, candidates.clone(), play(1));
    let mut next = None;
    for _ in 0..300 {
        let s = view.snapshot();
        if s.next.is_some() {
            next = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let s = next.expect("next episode found");
    assert_eq!(s.next.unwrap().video_id, "tt9:1:2");
    assert_eq!(s.binge_group.as_deref(), Some("show|1080p|NTb"));
    assert_eq!(
        core.library()
            .last_binge_group("p", "tt9")
            .unwrap()
            .as_deref(),
        Some("show|1080p|NTb"),
        "the played release line is remembered for the title"
    );

    // Episode 3 is not out yet, so the last aired episode offers nothing.
    let view = PlaybackView::open(&core, candidates, play(2));
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Ready);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(view.snapshot().next, None);
}
