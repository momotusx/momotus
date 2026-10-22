use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_library::{AddonRow, CachedResponse};
use mq_protocol::decode_streams;
use mq_runtime::{
    Core, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView, StartStage, SubtitleProblem,
};
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
                } else if head.starts_with("get /slow.mkv") {
                    // Gone, after a second and a half.
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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

/// The first snapshot that `holds`: waits on the state a test asserts,
/// for as long as a loaded machine needs, and names it if it never comes.
async fn until(
    view: &PlaybackView,
    what: &str,
    holds: impl Fn(&PlaybackSnapshot) -> bool,
) -> PlaybackSnapshot {
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let s = view.snapshot();
        if holds(&s) {
            return s;
        }
        assert!(std::time::Instant::now() < deadline, "{what}: {s:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
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

/// A loopback origin with a one-byte video, `/down.srt` and `/down2.srt`
/// answering 502, and anything else 200 with no cues.
async fn failing_subtitles() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let (status, body) = if head.starts_with("GET /video.mkv") {
                    ("206 Partial Content\r\nContent-Range: bytes 0-0/10", "M")
                } else if head.starts_with("GET /down") {
                    ("502 Bad Gateway", "")
                } else {
                    ("200 OK", "no cues in here")
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

/// A subtitle the route cannot serve leaves one note per source and
/// problem: a file that did not answer keeps its track, one that cannot be
/// read leaves the list. The notes belong to the playing source.
#[tokio::test]
async fn a_failing_subtitle_leaves_a_note() {
    let base = failing_subtitles().await;
    let core = Core::temporary("p").unwrap();
    let stream = format!(
        r#"{{"url":"{base}/video.mkv","subtitles":[
            {{"id":"a","lang":"eng","url":"{base}/down.srt"}},
            {{"id":"b","lang":"fre","url":"{base}/down2.srt"}},
            {{"id":"c","lang":"tur","url":"{base}/garbage.srt"}}]}}"#
    );
    let streams = format!(r#"{{"streams":[{stream},{stream}]}}"#);
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let s = until(&view, "a source", |s| s.phase != PlaybackPhase::Resolving).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    assert_eq!(s.subtitles.len(), 3, "{s:?}");
    assert!(s.subtitle_notes.is_empty());
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    for track in &s.subtitles {
        let served = http.get(&track.url, &local).await.unwrap();
        assert_eq!(&served.body[..], b"WEBVTT\n\n", "{}", track.lang);
    }
    let after = until(&view, "both notes", |s| s.subtitle_notes.len() == 2).await;
    let langs: Vec<_> = after.subtitles.iter().map(|t| t.lang.as_str()).collect();
    assert_eq!(langs, ["eng", "fre"], "the unreadable file left");
    let notes: Vec<_> = after
        .subtitle_notes
        .iter()
        .map(|n| (n.source.as_str(), n.problem))
        .collect();
    assert_eq!(
        notes,
        [
            ("Local", SubtitleProblem::Unreachable),
            ("Local", SubtitleProblem::TrackFailed)
        ]
    );

    view.fail_over(None, None, None, "decoder error");
    let next = until(&view, "the next source", |n| {
        n.phase == PlaybackPhase::Ready && n.attempt == s.attempt + 1
    })
    .await;
    assert!(next.subtitle_notes.is_empty(), "{next:?}");
    assert_eq!(next.subtitles.len(), 3);
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
    let s = until(&view, "the public install's track", |s| {
        s.subtitles.iter().any(|t| t.source == "pubsubs")
    })
    .await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    let track = s.subtitles.iter().find(|t| t.source == "pubsubs").unwrap();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    // Refused: an empty track, and a note that the file could not be read.
    let served = http.get(&track.url, &local).await.unwrap();
    assert_eq!(&served.body[..], b"WEBVTT\n\n");
    let after = until(&view, "the note", |s| !s.subtitle_notes.is_empty()).await;
    assert!(
        after.subtitles.iter().all(|t| t.url != track.url),
        "{after:?}"
    );
    let notes = after.subtitle_notes;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(
        (notes[0].source.as_str(), notes[0].problem),
        ("pubsubs", SubtitleProblem::TrackFailed)
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

/// Plans/player-polish.md 1.3: while it resolves, the snapshot says which
/// source the start is on, from the second on in the core's words, and the
/// start is gone at `ready`.
#[tokio::test]
async fn the_start_names_its_source_and_clears_at_ready() {
    let base = origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = format!(
        r#"{{"streams":[
          {{"name":"Slow","url":"{base}/slow.mkv"}},
          {{"name":"Slow too","url":"{base}/slow.mkv?again"}},
          {{"name":"Live","url":"{base}/ok.mkv","behaviorHints":{{"proxyHeaders":{{"request":{{"x-key":"1"}}}}}}}}
        ]}}"#
    );
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    // A link opens at once; its first byte is read at the `opening` stage.
    let on = |n: u32| {
        move |s: &PlaybackSnapshot| {
            s.start
                .as_ref()
                .is_some_and(|p| p.source == n && p.stage == StartStage::Opening)
        }
    };
    let first = until(&view, "the first source", on(1)).await.start.unwrap();
    assert_eq!((first.sources, first.caption.as_deref()), (3, None));
    assert!(first.progress < 0.1, "{first:?}");
    let second = until(&view, "the second source", on(2))
        .await
        .start
        .unwrap();
    assert!(second.progress >= first.progress, "{second:?}");
    assert_eq!(second.caption.as_deref(), Some("Trying source 2 of 3"));
    let s = until(&view, "ready", |s| s.phase != PlaybackPhase::Resolving).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    assert_eq!(s.start, None);
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
    view.report_progress(60_000, 900_000, false);
    // Throttled: this one is held until the view closes.
    view.report_progress(61_000, 900_000, false);
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
    view.report_progress(890_000, 900_000, false);
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
    assert!(!s.manifest);
    for track in &s.subtitles {
        assert!(track.url.starts_with("http://127.0.0.1:"), "{}", track.url);
    }
    // An HLS stream: its playlist and every child are the core's too.
    let hls = hls_origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = format!(r#"{{"streams":[{{"name":"HLS","url":"{hls}/hls/index.m3u8"}}]}}"#);
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    assert!(s.manifest);
    let url = s.url.unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:") && url.contains("/s/") && url.ends_with(".m3u8"),
        "{url}"
    );
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let playlist = http.get(&url, &local).await.unwrap();
    let playlist = String::from_utf8(playlist.body.to_vec()).unwrap();
    let children: Vec<_> = playlist.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(children.len(), 1, "{playlist}");
    assert!(children[0].starts_with("http://127.0.0.1:") && !children[0].contains(&hls));
    assert_eq!(
        http.get(children[0], &local).await.unwrap().body[..],
        b"segment"[..]
    );
}

#[tokio::test]
async fn a_player_without_hls_never_gets_a_playlist() {
    // Its caps list no m3u8: the playlist fails over with the words, whether
    // its path declared it or not.
    let hls = hls_origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = format!(r#"{{"streams":[{{"name":"HLS","url":"{hls}/hls/index.m3u8"}}]}}"#);
    let view = PlaybackView::open_for(&core, setup(&core, &streams), target(), &[]);
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Failed, "{s:?}");
    assert_eq!(s.url, None);
    let words: Vec<&str> = s.attempts.iter().map(|a| a.error.as_str()).collect();
    assert_eq!(words, ["This player cannot play HLS streams"]);
}

#[tokio::test]
async fn apple_refuses_dash() {
    // Apple's caps list m3u8 and no mpd: the DASH candidate fails with the
    // words and the file after it plays.
    let dash = dash_origin().await;
    let (file, _) = file_and_subtitles().await;
    let streams = format!(
        r#"{{"streams":[{{"name":"DASH","url":"{dash}/dash/main.mpd"}},{{"name":"File","url":"{file}/video.mkv"}}]}}"#
    );
    let core = Core::temporary("p").unwrap();
    let hls = [mq_server::Format::Hls];
    let view = PlaybackView::open_for(&core, setup(&core, &streams), target(), &hls);
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    let words: Vec<&str> = s.attempts.iter().map(|a| a.error.as_str()).collect();
    assert_eq!(words, ["This player cannot play DASH streams"]);
    assert!(!s.manifest);
    assert!(!s.url.unwrap().ends_with(".mpd"));
    // A player whose caps list mpd plays the DASH candidate.
    let core = Core::temporary("p").unwrap();
    let dash_too = [mq_server::Format::Hls, mq_server::Format::Dash];
    let view = PlaybackView::open_for(&core, setup(&core, &streams), target(), &dash_too);
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    assert!(s.attempts.is_empty(), "{s:?}");
    assert!(s.manifest);
    let url = s.url.unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:") && url.ends_with(".mpd"),
        "{url}"
    );
}

/// A DASH origin: an MPD whose segments sit beside it.
async fn dash_origin() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let mpd = "<?xml version=\"1.0\"?><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" \
                           type=\"static\" mediaPresentationDuration=\"PT4S\"><Period>\
                           <AdaptationSet mimeType=\"video/mp4\"><SegmentTemplate media=\"s-$Number$.m4s\" \
                           duration=\"4\"/><Representation id=\"v\" bandwidth=\"1\"/></AdaptationSet>\
                           </Period></MPD>";
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/dash+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{mpd}",
                    mpd.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

/// An HLS origin: a media playlist naming one segment.
async fn hls_origin() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let (kind, body) = if head.starts_with("GET /hls/index.m3u8") {
                    let playlist =
                        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nseg.ts\n#EXT-X-ENDLIST\n";
                    ("application/vnd.apple.mpegurl", playlist)
                } else {
                    ("video/mp2t", "segment")
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
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

/// The owner's list: many languages in many spellings, OpenSubtitles'
/// specials, and several files of one language from one source.
#[tokio::test]
async fn subtitles_come_grouped_by_language_named_and_told_apart() {
    let (base, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({
        "subtitleLanguages": ["tur"],
        "streams": { "languages": ["en"] }
    }))
    .unwrap();
    let sub = |id: &str, lang: &str, more: serde_json::Value| {
        let mut s = serde_json::json!({"id": id, "lang": lang, "url": format!("https://subs.test/{id}.srt")});
        s.as_object_mut()
            .unwrap()
            .extend(more.as_object().unwrap().clone());
        s
    };
    let none = serde_json::json!({});
    let subtitles = serde_json::json!([
        sub("1", "pob", none.clone()),
        sub(
            "2",
            "eng",
            serde_json::json!({"subtitleFileName": "Show.S01E01.DVDRip.XviD-ORPHEUS.srt"})
        ),
        sub("3", "spl", none.clone()),
        sub("4", "tur", serde_json::json!({"label": "TOPAZ"})),
        sub("5", "eng", serde_json::json!({"label": "English"})),
        sub("6", "zht", none.clone()),
        sub("7", "ger", none.clone()),
        sub("8", "tur", serde_json::json!({"label": "TOPAZ"})),
        sub("9", "deu", none.clone()),
        sub("10", "eng", none.clone()),
        sub("11", "zhc", none.clone()),
        sub("12", "mne", none.clone()),
        sub(
            "13",
            "fre",
            serde_json::json!({"movieReleaseName": "Show.S01.WEB-NTb"})
        ),
    ]);
    let streams =
        serde_json::json!({"streams": [{"url": format!("{base}/v.mkv"), "subtitles": subtitles}]});
    let view = PlaybackView::open(&core, setup(&core, &streams.to_string()), target());
    let s = until(&view, "the tracks", |s| {
        s.phase == PlaybackPhase::Ready && !s.subtitles_pending
    })
    .await;
    let rows: Vec<(&str, &str, &str)> = s
        .subtitles
        .iter()
        .map(|t| {
            (
                t.language.as_str(),
                t.language_name.as_str(),
                t.name.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            // The profile's subtitle language, then its audio language.
            ("tr", "Turkish", "TOPAZ 1"),
            ("tr", "Turkish", "TOPAZ 2"),
            ("en", "English", "ORPHEUS, DVD"),
            ("en", "English", "Local 1"),
            ("en", "English", "Local 2"),
            // The rest by English name; one language however it is spelled.
            ("yue", "Chinese (Cantonese)", "Local"),
            ("zh-Hant", "Chinese (Traditional)", "Local"),
            ("fr", "French", "NTb, WEB"),
            ("de", "German", "Local 1"),
            ("de", "German", "Local 2"),
            ("cnr", "Montenegrin", "Local"),
            ("pt-BR", "Portuguese (Brazil)", "Local"),
            ("es-419", "Spanish (Latin America)", "Local"),
        ]
    );
    // The addon's own code is kept for the player.
    assert_eq!(s.subtitles[11].lang, "pob");
}

/// Plans/player-polish.md 2.2: within each language, the tracks naming the
/// playing release's group come first and `fit`, a plain one before one
/// for the hard of hearing; then the same source family; then the addon's
/// order. The start pick is the best match. Each track names its language
/// in that language too.
#[tokio::test]
async fn subtitles_rank_by_the_playing_release_within_each_language() {
    let (base, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["eng"] }))
        .unwrap();
    let sub = |id: &str, lang: &str, file: &str, hi: bool| {
        serde_json::json!({"id": id, "lang": lang, "url": format!("https://subs.test/{id}.srt"),
            "subtitleFileName": file, "hearingImpaired": hi})
    };
    let subtitles = serde_json::json!([
        sub(
            "disc",
            "eng",
            "Sintel.2010.1080p.BluRay.x264-OTHER.srt",
            false
        ),
        sub(
            "tr-disc",
            "tur",
            "Sintel.2010.720p.BluRay.x264-OTHER.srt",
            false
        ),
        sub(
            "web",
            "eng",
            "Sintel.2010.1080p.WEBRip.x264-RARBG.srt",
            false
        ),
        sub(
            "own-hi",
            "eng",
            "Sintel.2010.1080p.WEB-DL.DDP5.1.H.264-FLUX-hi.srt",
            false
        ),
        sub("plain", "eng", "", false),
        sub(
            "own",
            "eng",
            "Sintel.2010.1080p.WEB-DL.DDP5.1.H.264-FLUX.srt",
            false
        ),
        sub(
            "tr-own",
            "tur",
            "Sintel.2010.1080p.WEB-DL.H.264-FLUX.srt",
            false
        ),
    ]);
    let streams = serde_json::json!({"streams": [{
        "url": format!("{base}/v.mkv"),
        "title": "Sintel.2010.1080p.WEB-DL.DDP5.1.H.264-FLUX",
        "subtitles": subtitles,
    }]});
    let view = PlaybackView::open(&core, setup(&core, &streams.to_string()), target());
    let s = settled_list(&view).await;
    let rows: Vec<(&str, bool, &str)> = s
        .subtitles
        .iter()
        .map(|t| (t.name.as_str(), t.fits, t.native_name.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("FLUX, WEB 1080p 1", true, "English"),
            ("FLUX, WEB 1080p 2", true, "English"),
            ("RARBG, WEBRip 1080p", false, "English"),
            ("OTHER, BluRay 1080p", false, "English"),
            ("Local", false, "English"),
            ("FLUX, WEB 1080p", true, "Türkçe"),
            ("OTHER, BluRay 720p", false, "Türkçe"),
        ]
    );
    // The "-hi" in its file name marks it, with no flag from the addon;
    // the detail line keeps the release text.
    assert!(s.subtitles[1].hearing_impaired);
    assert_eq!(
        s.subtitles[1].release.as_deref(),
        Some("Sintel.2010.1080p.WEB-DL.DDP5.1.H.264-FLUX-hi")
    );
    assert_eq!(
        s.auto_subtitle.as_deref(),
        Some(s.subtitles[0].url.as_str())
    );
}

/// An addon that answers `body` to every request: its manifest is the row.
async fn answering_addon(body: String) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = Arc::new(body);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "subs-langs", "version": "1.0.0", "name": "Langs",
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

/// A profile that set no subtitle languages: the languages the pick was
/// ranked with lead, as for audio: the device's, then English. A profile's
/// own subtitle languages still come first.
#[tokio::test]
async fn without_subtitle_languages_the_device_language_then_english_lead() {
    use mq_runtime::{VersionsParams, VersionsView};
    let (video, _) = counted_origin().await;
    let subs: Vec<_> = ["ger", "eng", "tur", "fre"]
        .iter()
        .map(|lang| serde_json::json!({"id": lang, "lang": lang, "url": format!("https://subs.test/{lang}.srt")}))
        .collect();
    let streams = serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": "Sintel.2010.1080p.WEB-DL", "subtitles": subs}]});
    for (device, own, order) in [
        (Some("tr"), None, ["Turkish", "English", "French", "German"]),
        (Some("en"), None, ["English", "French", "German", "Turkish"]),
        (None, None, ["English", "French", "German", "Turkish"]),
        (
            Some("tr"),
            Some("ger"),
            ["German", "Turkish", "English", "French"],
        ),
    ] {
        let core = Core::temporary("p").unwrap();
        if let Some(own) = own {
            core.set_prefs(&serde_json::json!({ "subtitleLanguages": [own] }))
                .unwrap();
        }
        install(&core, &[answering_addon(streams.to_string()).await]);
        // As the bridge reads the device: its language, then English.
        let mut languages: Vec<String> = device.into_iter().map(str::to_owned).collect();
        languages.push("en".into());
        languages.dedup();
        let params = VersionsParams {
            kind: "movie".into(),
            video_id: "tt1727587".into(),
            target: mq_streams::Target {
                title: "Sintel".into(),
                year: None,
                season: None,
                episode: None,
            },
            caps: mq_streams::DeviceCaps::basic_1080p(),
            prefs: mq_streams::StreamPrefs {
                languages,
                ..Default::default()
            },
            last_binge_group: None,
            facts: Default::default(),
        };
        let versions = VersionsView::open(&core, params);
        let mut key = None;
        for _ in 0..3000 {
            let s = versions.snapshot();
            if let Some(r) = s.recommended.as_ref().filter(|_| s.done) {
                key = Some(r.key.clone());
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let key = key.unwrap_or_else(|| panic!("no pick: {:?}", versions.snapshot()));
        let plays = versions.plays();
        let view = PlaybackView::open_picked(&core, versions.pick(&key), target(), &plays);
        let s = until(&view, "the tracks", |s| {
            s.phase == PlaybackPhase::Ready && !s.subtitles_pending
        })
        .await;
        let names: Vec<_> = s
            .subtitles
            .iter()
            .map(|t| t.language_name.as_str())
            .collect();
        assert_eq!(names, order, "device {device:?}, profile {own:?}");
    }
}

/// The track every app turns on at start: only when the profile set
/// subtitle languages and the audio is in none of them; the user's own
/// choice (a track, or Off) wins and is remembered for the title.
#[tokio::test]
async fn a_subtitle_turns_on_at_start_only_as_the_profile_and_the_user_say() {
    let (video, _) = counted_origin().await;
    let stream = |title: &str| {
        let subs: Vec<_> = ["eng", "tur", "tur"]
            .iter()
            .enumerate()
            .map(|(i, lang)| serde_json::json!({"id": i.to_string(), "lang": lang, "url": format!("https://subs.test/{i}.srt")}))
            .collect();
        serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": title, "subtitles": subs}]})
            .to_string()
    };
    let open =
        |core: &Core, title: &str| PlaybackView::open(core, setup(core, &stream(title)), target());
    let url = |s: &PlaybackSnapshot, lang: &str| {
        s.subtitles
            .iter()
            .find(|t| t.lang == lang)
            .unwrap()
            .url
            .clone()
    };

    // Not set: nothing turns on.
    let core = Core::temporary("p").unwrap();
    let s = settled_list(&open(&core, "Sintel.2010.1080p.WEB")).await;
    assert_eq!(s.auto_subtitle, None);

    // Set to Turkish, the audio's language unknown: the first Turkish track.
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["tur"] }))
        .unwrap();
    let view = open(&core, "Sintel.2010.1080p.WEB");
    let s = settled_list(&view).await;
    assert_eq!(s.auto_subtitle, Some(url(&s, "tur")));

    // The audio is Turkish (the release says so): nothing turns on.
    let dubbed = settled_list(&open(&core, "Sintel.2010.TURKISH.1080p.WEB")).await;
    assert_eq!(dubbed.auto_subtitle, None);

    // The user turns subtitles off: gone at once, and the title remembers.
    view.subtitle_chosen(None);
    assert_eq!(view.snapshot().auto_subtitle, None);
    drop(view);
    let again = settled_list(&open(&core, "Sintel.2010.1080p.WEB")).await;
    assert_eq!(again.auto_subtitle, None, "Off is remembered for the title");

    // The user picks the English track: next time English turns on, even
    // though Turkish is the profile's language.
    let view = open(&core, "Sintel.2010.1080p.WEB");
    let s = settled_list(&view).await;
    view.subtitle_chosen(Some(&url(&s, "eng")));
    drop(view);
    let next = settled_list(&open(&core, "Sintel.2010.1080p.WEB")).await;
    assert_eq!(next.auto_subtitle, Some(url(&next, "eng")));
}

async fn settled_list(view: &PlaybackView) -> PlaybackSnapshot {
    until(view, "the list", |s| {
        s.phase == PlaybackPhase::Ready && !s.subtitles_pending
    })
    .await
}

/// Pixel journey walk PX8 (bench/m6/journey-walk.jsonl), in its order: a
/// 51 s file played to its end, then three partial plays of the same
/// video (two left by a force stop), then a 261 s file paused at 57.5 s.
/// Continue Watching shows the film again, as the title page's Resume does
/// (both read the same rule: progress newer than the watched mark).
#[tokio::test]
async fn a_partial_play_after_a_finish_is_in_continue_watching() {
    let core = Core::temporary("p").unwrap();
    let sintel = PlayTarget {
        kind: "movie".into(),
        item_id: "tt1727587".into(),
        video_id: "tt1727587".into(),
        name: Some("Sintel".into()),
        poster: None,
        season: None,
        episode: None,
    };
    let play = |reports: &[(u64, u64, bool)]| {
        let view = PlaybackView::open(&core, Vec::new(), sintel.clone());
        for &(position, duration, paused) in reports {
            view.report_progress(position, duration, paused);
        }
        view
    };
    let shown = || {
        core.library()
            .continue_watching("p", 20)
            .unwrap()
            .into_iter()
            .map(|c| (c.video_id, c.position_ms))
            .collect::<Vec<_>>()
    };
    drop(play(&[(10_000, 51_000, false), (51_000, 51_000, false)]));
    assert_eq!(
        core.library().watched_videos("p", "tt1727587").unwrap(),
        ["tt1727587"]
    );
    assert!(shown().is_empty(), "finished");
    // Force stops: the views are never closed.
    std::mem::forget(play(&[(1_000, 51_000, false), (5_000, 51_000, true)]));
    std::mem::forget(play(&[(8_000, 51_000, true)]));
    drop(play(&[(15_000, 51_000, false)]));
    drop(play(&[(30_000, 261_000, false), (57_500, 261_000, true)]));
    // What the title page's Resume reads.
    assert_eq!(
        core.library().latest_progress("p", "tt1727587").unwrap(),
        Some(("tt1727587".to_string(), 57_500, 261_000))
    );
    assert_eq!(shown(), [("tt1727587".to_string(), 57_500)]);
    let home = mq_runtime::HomeView::open(&core);
    let mut row = None;
    for _ in 0..300 {
        row = home
            .snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id == mq_runtime::views::home::CONTINUE_ROW_ID);
        if row.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let row = row.expect("Home shows Continue Watching");
    assert_eq!(row.tiles[0].video_id.as_deref(), Some("tt1727587"));
}

/// A Matroska head whose video track runs at 25 fps (DefaultDuration 40
/// ms), its Tracks 128 KiB in, past a Void; padded to 256 KiB.
fn mkv_at_25_fps() -> Vec<u8> {
    fn el(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut v = id.to_vec();
        v.push(0x01);
        v.extend_from_slice(&(body.len() as u64).to_be_bytes()[1..]);
        v.extend_from_slice(body);
        v
    }
    let uint = |id: &[u8], x: u64| el(id, &x.to_be_bytes());
    let header = el(
        &[0x1A, 0x45, 0xDF, 0xA3],
        &[uint(&[0x42, 0x86], 1), el(&[0x42, 0x82], b"matroska")].concat(),
    );
    let info = el(
        &[0x15, 0x49, 0xA9, 0x66],
        &uint(&[0x2A, 0xD7, 0xB1], 1_000_000),
    );
    let void = el(&[0xEC], &vec![0; 128 * 1024]);
    let video = el(
        &[0xAE],
        &[
            uint(&[0xD7], 1),
            uint(&[0x83], 1),
            el(&[0x86], b"V_MPEG4/ISO/AVC"),
            uint(&[0x23, 0xE3, 0x83], 40_000_000),
        ]
        .concat(),
    );
    let tracks = el(&[0x16, 0x54, 0xAE, 0x6B], &video);
    let cluster = el(
        &[0x1F, 0x43, 0xB6, 0x75],
        &[uint(&[0xE7], 0), el(&[0xA3], &[0x55; 64])].concat(),
    );
    let mut file = [
        header,
        el(
            &[0x18, 0x53, 0x80, 0x67],
            &[info, void, tracks, cluster].concat(),
        ),
    ]
    .concat();
    file.resize(256 * 1024, 0);
    file
}

/// Serves `file` by ranges. Its first 64 KiB come at once; the rest of a
/// range that starts before the last 64 KiB comes `late`, so the probe,
/// which needs the Tracks 128 KiB in, ends that long after Ready.
async fn late_tracks_origin(file: Vec<u8>, late: Duration) -> String {
    const NEAR: usize = 64 * 1024;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let file = Arc::new(file);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let file = file.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let (start, end) = head
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| {
                        let (a, b) = r.trim().split_once('-')?;
                        let a: usize = a.parse().ok()?;
                        let b = b.parse().unwrap_or(file.len() - 1).min(file.len() - 1);
                        Some((a, b))
                    })
                    .unwrap_or((0, file.len() - 1));
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    file.len(),
                    end - start + 1
                );
                let cut = NEAR.clamp(start, end + 1);
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&file[start..cut]).await;
                let _ = socket.flush().await;
                if cut <= end && start < file.len() - NEAR {
                    tokio::time::sleep(late).await;
                }
                let _ = socket.write_all(&file[cut..=end]).await;
            });
        }
    });
    base
}

/// Two English files for one WEB release, a film-rate one listed first and
/// a 25 fps one, as OpenSubtitles v3 rows carry them, on a 25 fps video
/// whose rate the probe reads only after Ready.
fn rated_rows(video: &str) -> String {
    let sub = |id: &str, file: &str, fps: u32| {
        serde_json::json!({"id": id, "lang": "eng", "url": format!("https://subs.test/{id}.srt"),
            "subtitleFileName": file, "fpsMilli": fps})
    };
    serde_json::json!({"streams": [{
        "url": format!("{video}/v.mkv"),
        "title": "Sintel.2010.1080p.WEB-DL.H.264-GRP",
        "subtitles": [
            sub("film", "Sintel.2010.1080p.WEBRip.x264-AAA.srt", 23976),
            sub("pal", "Sintel.2010.1080p.WEBRip.x264-BBB.srt", 25000),
        ],
    }]})
    .to_string()
}

fn releases(s: &PlaybackSnapshot) -> Vec<&str> {
    s.subtitles
        .iter()
        .filter_map(|t| t.release.as_deref())
        .collect()
}

const FILM: &str = "Sintel.2010.1080p.WEBRip.x264-AAA";
const PAL: &str = "Sintel.2010.1080p.WEBRip.x264-BBB";

/// Plans/player-polish.md 2g, review P1-1: the probe runs on every play,
/// so its frame rate orders the tracks with no built-in TurkceAltyazi.org
/// source installed and no subtitle languages set.
#[tokio::test]
async fn the_probed_frame_rate_orders_subtitles_without_the_built_in_source() {
    let video = late_tracks_origin(mkv_at_25_fps(), Duration::from_millis(300)).await;
    let core = Core::temporary("p").unwrap();
    let view = PlaybackView::open(&core, setup(&core, &rated_rows(&video)), target());
    let s = settled_list(&view).await;
    assert_eq!(releases(&s), [PAL, FILM]);
    assert_eq!(s.subtitles[1].made_for_fps, Some(23.976));
    assert_eq!(s.subtitles[0].made_for_fps, None);
}

/// Review P3-5: the viewer picks a track before the probe ends. The rate
/// still reorders the list when it lands, the picked track stays, and the
/// core turns nothing else on over the pick.
#[tokio::test]
async fn a_pick_before_the_probe_ends_stays_the_pick() {
    let video = late_tracks_origin(mkv_at_25_fps(), Duration::from_millis(1000)).await;
    let core = Core::temporary("p").unwrap();
    let view = PlaybackView::open(&core, setup(&core, &rated_rows(&video)), target());
    let early = until(&view, "ready", |s| s.phase == PlaybackPhase::Ready).await;
    assert!(early.subtitles_pending);
    assert_eq!(releases(&early), [FILM, PAL], "the rate is not read yet");
    let picked = early.subtitles[0].url.clone();
    view.subtitle_chosen(Some(&picked));
    let s = settled_list(&view).await;
    assert_eq!(releases(&s), [PAL, FILM], "the rate landed after the pick");
    assert_eq!(s.subtitles[1].url, picked);
    assert_eq!(s.auto_subtitle, None);
}

/// Plans/player-polish.md 2g: the stepper's delay is remembered for the
/// title's version, video and track (its origin, not its per-play URL),
/// and the next play of the same version starts its track there.
#[tokio::test]
async fn a_subtitle_delay_is_remembered_for_the_version_and_the_track() {
    let (video, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = |file: &str| {
        let subs: Vec<_> = ["a", "b"]
            .iter()
            .map(|id| serde_json::json!({"id": id, "lang": "eng", "url": format!("https://subs.test/{id}.srt"), "subtitleFileName": format!("{id}.srt")}))
            .collect();
        serde_json::json!({"streams": [{"url": format!("{video}/{file}"), "title": "Sintel.2010.1080p.WEB", "subtitles": subs}]})
            .to_string()
    };
    let play = |file: &str| PlaybackView::open(&core, setup(&core, &streams(file)), target());
    let delays = |s: &PlaybackSnapshot| -> Vec<(String, i32)> {
        s.subtitles
            .iter()
            .map(|t| (t.release.clone().unwrap_or_default(), t.delay_ms))
            .collect()
    };
    let at = |pairs: [(&str, i32); 2]| -> Vec<(String, i32)> {
        pairs.iter().map(|(r, ms)| (r.to_string(), *ms)).collect()
    };

    let view = play("v.mkv");
    let s = settled_list(&view).await;
    assert_eq!(delays(&s), at([("a", 0), ("b", 0)]));
    let b = s.subtitles[1].url.clone();
    view.subtitle_delay(&b, 1_300);
    assert_eq!(delays(&view.snapshot()), at([("a", 0), ("b", 1_300)]));
    // Not a core track (an in-file one): nothing changes.
    view.subtitle_delay("http://127.0.0.1:1/sub/x/in-file.vtt", 500);
    assert_eq!(delays(&view.snapshot()), at([("a", 0), ("b", 1_300)]));
    drop(view);

    // The next play of the version: a new URL, the same delay.
    let view = play("v.mkv");
    let s = settled_list(&view).await;
    assert_ne!(s.subtitles[1].url, b);
    assert_eq!(delays(&s), at([("a", 0), ("b", 1_300)]));
    drop(view);
    // Each start rewrites the memory and keeps it.
    let view = play("v.mkv");
    let s = settled_list(&view).await;
    assert_eq!(delays(&s), at([("a", 0), ("b", 1_300)]));
    // The stepper stops at a minute either way.
    view.subtitle_delay(&s.subtitles[0].url, -90_000);
    drop(view);
    let view = play("v.mkv");
    let s = settled_list(&view).await;
    assert_eq!(delays(&s), at([("a", -60_000), ("b", 0)]));
    drop(view);

    // Another version of the title starts at 0 and forgets it.
    let s = settled_list(&play("w.mkv")).await;
    assert_eq!(delays(&s), at([("a", 0), ("b", 0)]));
    let view = play("v.mkv");
    let s = settled_list(&view).await;
    assert_eq!(delays(&s), at([("a", 0), ("b", 0)]));

    // Reset to 0 is remembered as no delay.
    view.subtitle_delay(&s.subtitles[0].url, 700);
    view.subtitle_delay(&s.subtitles[0].url, 0);
    drop(view);
    let s = settled_list(&play("v.mkv")).await;
    assert_eq!(delays(&s), at([("a", 0), ("b", 0)]));
}
