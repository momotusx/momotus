use futures_util::StreamExt;
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
/// answering 502, `/good.srt` one cue, and anything else 200 with no cues.
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
                } else if head.starts_with("GET /good") {
                    ("200 OK", "1\n00:00:01,000 --> 00:00:02,000\nHello\n")
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

/// Opens Sintel on `failing_subtitles` with the stream's own `subtitles`
/// (`[lang, path]`), the profile's subtitle languages `languages`, and
/// waits for the list to settle.
async fn start_with_files(
    languages: &[&str],
    files: &[[&str; 2]],
) -> (PlaybackView, PlaybackSnapshot) {
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": languages }))
        .unwrap();
    open_files(&core, files).await
}

/// Opens Sintel on `failing_subtitles` with the stream's own `subtitles`
/// on `core`, and waits for the list to settle.
async fn open_files(core: &Core, files: &[[&str; 2]]) -> (PlaybackView, PlaybackSnapshot) {
    let base = failing_subtitles().await;
    let subs: Vec<_> = files
        .iter()
        .enumerate()
        .map(|(i, [lang, path])| serde_json::json!({"id": i.to_string(), "lang": lang, "url": format!("{base}/{path}")}))
        .collect();
    let streams =
        serde_json::json!({"streams": [{"url": format!("{base}/video.mkv"), "subtitles": subs}]})
            .to_string();
    let view = PlaybackView::open(core, setup(core, &streams), target());
    let s = settled_list(&view).await;
    (view, s)
}

/// The player loads the start subtitle's URL, as the apps do, and gets
/// its body; then the snapshot once the core has heard how it went.
async fn load_start(view: &PlaybackView, s: &PlaybackSnapshot) -> (String, PlaybackSnapshot) {
    let url = s.auto_subtitle.clone().expect("a start subtitle");
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let body = http.get(&url, &local).await.unwrap().body;
    let body = String::from_utf8_lossy(&body).into_owned();
    if body.contains("Hello") {
        return (body, view.snapshot());
    }
    let next = until(view, "the start subtitle moved on", |n| {
        n.auto_subtitle.as_deref() != Some(url.as_str())
    })
    .await;
    (body, next)
}

/// The Mac walk: a start subtitle whose file cannot be read hands over to
/// its language's next best track with no note and no source named, so
/// two unreadable files then a good one end with the good one on.
#[tokio::test]
async fn an_unreadable_start_subtitle_hands_over_to_the_next_best() {
    let (view, s) = start_with_files(
        &["eng"],
        &[
            ["eng", "bad1.srt"],
            ["eng", "bad2.srt"],
            ["eng", "good.srt"],
        ],
    )
    .await;
    let (body, s) = load_start(&view, &s).await;
    assert_eq!(body, "WEBVTT\n\n");
    let (body, s) = load_start(&view, &s).await;
    assert_eq!(body, "WEBVTT\n\n");
    let (body, s) = load_start(&view, &s).await;
    assert!(body.contains("Hello"), "{body}");
    assert_eq!(s.subtitles.len(), 1, "the two unreadable left: {s:?}");
    assert_eq!(s.auto_subtitle.as_ref(), Some(&s.subtitles[0].url));
    assert!(
        s.subtitle_notes.is_empty(),
        "no note: {:?}",
        s.subtitle_notes
    );
    assert!(!s.start_subtitle_failed);
}

/// A start subtitle whose file does not answer (a 502 here, as a timeout)
/// hands over as an unreadable one does, with no note, and stays listed:
/// it may load when picked again (review P2). With every one unreachable
/// the start ends neutral and the list stays whole.
#[tokio::test]
async fn an_unreachable_start_subtitle_hands_over_too() {
    let (view, s) = start_with_files(&["eng"], &[["eng", "down.srt"], ["eng", "good.srt"]]).await;
    let down = s.auto_subtitle.clone().unwrap();
    let (body, s) = load_start(&view, &s).await;
    assert_eq!(body, "WEBVTT\n\n");
    let (body, s) = load_start(&view, &s).await;
    assert!(body.contains("Hello"), "{body}");
    assert_eq!(s.subtitles.len(), 2, "the unreachable one stays: {s:?}");
    assert!(s.subtitles.iter().any(|t| t.url == down));
    assert!(
        s.subtitle_notes.is_empty(),
        "no note: {:?}",
        s.subtitle_notes
    );

    let (view, mut s) =
        start_with_files(&["eng"], &[["eng", "down.srt"], ["eng", "down2.srt"]]).await;
    // The network down: every start track unreachable, each tried once.
    let mut picked = Vec::new();
    while let Some(url) = s.auto_subtitle.clone() {
        picked.push(url);
        let (_, next) = load_start(&view, &s).await;
        s = next;
    }
    assert_eq!(picked.len(), 2, "none picked twice: {picked:?}");
    assert_ne!(picked[0], picked[1]);
    assert!(s.start_subtitle_failed, "{s:?}");
    assert_eq!(s.subtitles.len(), 2, "both stay listed, still pickable");
}

/// The viewer's own pick stays theirs: when its file cannot be read it
/// leaves the list with its note, and nothing else turns on.
#[tokio::test]
async fn a_hand_picked_unreadable_track_never_hands_over() {
    let (view, s) = start_with_files(&["eng"], &[["eng", "bad1.srt"], ["eng", "good.srt"]]).await;
    let picked = s.auto_subtitle.clone().expect("a start subtitle");
    view.subtitle_chosen(Some(&picked), None);
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let body = http.get(&picked, &local).await.unwrap().body;
    assert_eq!(&body[..], b"WEBVTT\n\n");
    let after = until(&view, "the note", |n| !n.subtitle_notes.is_empty()).await;
    assert_eq!(
        after.subtitle_notes[0].problem,
        SubtitleProblem::TrackFailed
    );
    assert!(after.subtitles.iter().all(|t| t.url != picked));
    assert_eq!(after.auto_subtitle, None, "no move on a hand pick");
    assert!(!after.start_subtitle_failed);
}

/// Review P3-3: the title's remembered language (French, picked by hand
/// in an earlier play) with every track unreadable falls back to the
/// profile's other languages in order before giving up.
#[tokio::test]
async fn a_remembered_language_spent_falls_back_to_the_profile() {
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["eng"] }))
        .unwrap();
    let (view, s) = open_files(&core, &[["fre", "good.srt"], ["eng", "good.srt"]]).await;
    let french = s
        .subtitles
        .iter()
        .find(|t| t.lang == "fre")
        .unwrap()
        .url
        .clone();
    view.subtitle_chosen(Some(&french), None);
    drop(view);
    let (view, s) = open_files(&core, &[["fre", "bad1.srt"], ["eng", "good.srt"]]).await;
    let start = |s: &PlaybackSnapshot| {
        let url = s.auto_subtitle.as_ref()?;
        s.subtitles
            .iter()
            .find(|t| &t.url == url)
            .map(|t| t.lang.clone())
    };
    assert_eq!(start(&s).as_deref(), Some("fre"), "the title's own first");
    let (_, s) = load_start(&view, &s).await;
    assert_eq!(start(&s).as_deref(), Some("eng"), "{s:?}");
    let (body, s) = load_start(&view, &s).await;
    assert!(body.contains("Hello"), "{body}");
    assert!(!s.start_subtitle_failed);
}

/// Review P3-1: a pick after every start subtitle failed clears
/// `startSubtitleFailed`.
#[tokio::test]
async fn a_pick_clears_the_failed_start() {
    let (view, mut s) = start_with_files(&["eng"], &[["eng", "bad1.srt"]]).await;
    let (_, next) = load_start(&view, &s).await;
    s = next;
    assert!(s.start_subtitle_failed, "{s:?}");
    view.subtitle_chosen(None, None);
    assert!(!view.snapshot().start_subtitle_failed);
}

/// Every candidate unreadable, the language's then the next language's:
/// the start subtitle ends null with the neutral `startSubtitleFailed`,
/// and still no note names a source.
#[tokio::test]
async fn every_start_subtitle_unreadable_ends_neutral() {
    let (view, mut s) = start_with_files(
        &["eng", "fre"],
        &[
            ["eng", "bad1.srt"],
            ["eng", "bad2.srt"],
            ["fre", "bad3.srt"],
        ],
    )
    .await;
    let mut tried = Vec::new();
    while s.auto_subtitle.is_some() {
        let lang = |s: &PlaybackSnapshot| {
            let url = s.auto_subtitle.as_ref().unwrap();
            s.subtitles
                .iter()
                .find(|t| &t.url == url)
                .unwrap()
                .lang
                .clone()
        };
        tried.push(lang(&s));
        let (body, next) = load_start(&view, &s).await;
        assert_eq!(body, "WEBVTT\n\n");
        s = next;
    }
    assert_eq!(tried, ["eng", "eng", "fre"]);
    assert!(s.start_subtitle_failed, "{s:?}");
    assert!(s.subtitles.is_empty());
    assert!(
        s.subtitle_notes.is_empty(),
        "no note: {:?}",
        s.subtitle_notes
    );
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
            // A track that names nothing reads as its language, never as
            // its source.
            ("en", "English", "English 1"),
            ("en", "English", "English 2"),
            // The rest by English name; one language however it is spelled.
            ("yue", "Chinese (Cantonese)", "Chinese (Cantonese)"),
            ("zh-Hant", "Chinese (Traditional)", "Chinese (Traditional)"),
            ("fr", "French", "NTb, WEB"),
            ("de", "German", "German 1"),
            ("de", "German", "German 2"),
            ("cnr", "Montenegrin", "Montenegrin"),
            ("pt-BR", "Portuguese (Brazil)", "Portuguese (Brazil)"),
            (
                "es-419",
                "Spanish (Latin America)",
                "Spanish (Latin America)"
            ),
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
            ("English", false, "English"),
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
            device_language: None,
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

/// A profile with no subtitle languages starts subtitles in the device's
/// language, whatever its audio languages say: a Turkish device whose
/// profile lists English audio turns Turkish on, not English (the lead,
/// 2026-10-26); with no device language nothing is turned on by itself, and
/// a profile that chose No subtitles (an empty list it wrote) keeps none.
#[tokio::test]
async fn without_subtitle_languages_the_device_language_turns_on() {
    use mq_runtime::{VersionsParams, VersionsView};
    let (video, _) = counted_origin().await;
    let subs: Vec<_> = ["eng", "tur"]
        .iter()
        .map(|lang| serde_json::json!({"id": lang, "lang": lang, "url": format!("https://subs.test/{lang}.srt")}))
        .collect();
    let streams = serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": "Sintel.2010.1080p.WEB-DL", "subtitles": subs}]});
    for (device, chose_none, expected) in [
        (Some("tr"), false, Some("tr")),
        (None, false, None),
        (Some("tr"), true, None),
    ] {
        let core = Core::temporary("p").unwrap();
        if chose_none {
            core.set_prefs(&serde_json::json!({ "subtitleLanguages": [] }))
                .unwrap();
        }
        install(&core, &[answering_addon(streams.to_string()).await]);
        let params = VersionsParams {
            device_language: device.map(str::to_owned),
            kind: "movie".into(),
            video_id: "tt1727587".into(),
            target: mq_streams::Target {
                title: "Sintel".into(),
                year: None,
                season: None,
                episode: None,
            },
            caps: mq_streams::DeviceCaps::basic_1080p(),
            // The profile's own audio language: English.
            prefs: mq_streams::StreamPrefs {
                languages: vec!["en".into()],
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
        // The core serves the track from its own address: read it back by its language.
        let on = s.auto_subtitle.as_ref().map(|url| {
            let track = s.subtitles.iter().find(|t| &t.url == url).unwrap();
            track.language.clone()
        });
        assert_eq!(
            on.as_deref(),
            expected,
            "device {device:?}, chose none {chose_none}"
        );
    }
}

/// A subtitles addon that lists one track in `lang`, `delay` after it is asked.
async fn subtitles_addon(id: &str, lang: &str, delay: Duration) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = serde_json::json!({"subtitles": [
        {"id": id, "lang": lang, "url": format!("https://subs.test/{id}.srt")}
    ]})
    .to_string();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    row(
        &format!("http://{addr}/manifest.json"),
        id,
        "subtitles",
        true,
    )
}

/// One slow subtitle addon (SubSense took 12 to 20 s on the iphone-a walk)
/// never holds the start subtitle: after 1 s the tracks listed decide it, and the slow addon's track, listed later and
/// ranked first, never changes it.
#[tokio::test]
async fn a_slow_subtitle_addon_does_not_hold_the_start_subtitle() {
    let (video, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["eng"] }))
        .unwrap();
    let slow = subtitles_addon("slow", "eng", Duration::from_secs(8)).await;
    let quick = subtitles_addon("quick", "eng", Duration::ZERO).await;
    install(
        &core,
        &[row(LOCAL_ADDON, "local", "stream", true), slow, quick],
    );
    let streams = serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": "Sintel.2010.1080p.WEB"}]})
        .to_string();
    let view = PlaybackView::open(
        &core,
        listed(LOCAL_ADDON, "local", "Local", &streams),
        target(),
    );
    let opened = std::time::Instant::now();
    let s = until(&view, "the start subtitle", |s| s.auto_subtitle.is_some()).await;
    assert!(
        opened.elapsed() < Duration::from_secs(2),
        "the start subtitle took {:?}",
        opened.elapsed()
    );
    assert!(s.subtitles_pending, "the slow addon is still out");
    let source = |s: &PlaybackSnapshot| {
        s.subtitles
            .iter()
            .find(|t| Some(&t.url) == s.auto_subtitle.as_ref())
            .map(|t| t.source.clone())
    };
    assert_eq!(source(&s).as_deref(), Some("quick"));
    let done = settled_list(&view).await;
    assert!(done.subtitles.iter().any(|t| t.source == "slow"));
    assert_eq!(
        done.auto_subtitle, s.auto_subtitle,
        "a slower source never changes it"
    );
}

/// A subtitles addon answering every request with the recorded `body`.
async fn recorded_subtitles(id: &str, body: &'static str) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
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
    row(
        &format!("http://{addr}/manifest.json"),
        id,
        "subtitles",
        true,
    )
}

/// The owner's Mac walk: OpenSubtitles v3's real answer for Sintel
/// (tests/fixtures/opensubtitles-sintel.json, recorded 2026-10-23) listed
/// "sintel_en 1", "sintel_en 2" and "Sintel (2010).eng", and "sintel.he"
/// under English. Names now say only what tells tracks apart, the
/// language numbered when nothing does, and sintel.he is Hebrew. A second
/// source's track with no label and no file name reads as its language
/// too, never as the source ("OpenSubtitles v3 1" before).
#[tokio::test]
async fn real_subtitle_names_read_clean() {
    let addon = recorded_subtitles(
        "org.stremio.opensubtitlesv3",
        include_str!("fixtures/opensubtitles-sintel.json"),
    )
    .await;
    let bare = recorded_subtitles(
        "Bare Subs",
        r#"{"subtitles":[{"id":"b","lang":"eng","url":"https://subs.test/b.srt"}]}"#,
    )
    .await;
    let view = open_with_subtitles(&["eng", "heb"], vec![addon, bare]).await;
    let s = settled_list(&view).await;
    let names = |language: &str| -> Vec<String> {
        s.subtitles
            .iter()
            .filter(|t| t.language_name == language)
            .map(|t| t.name.clone())
            .collect()
    };
    for t in &s.subtitles {
        let lower = t.name.to_lowercase();
        assert!(
            !["sintel", "2010", ".srt", "opensubtitles", "bare"]
                .iter()
                .any(|w| lower.contains(w)),
            "{} reads {:?}",
            t.release.as_deref().unwrap_or_default(),
            t.name
        );
    }
    assert_eq!(
        names("English"),
        ["English 1", "English 2", "English 3", "English 4"],
        "in the core's order, the bare one last"
    );
    let bare = s
        .subtitles
        .iter()
        .find(|t| t.source == "Bare Subs")
        .unwrap();
    assert_eq!(bare.name, "English 4");
    assert!(
        s.subtitles
            .iter()
            .any(|t| t.language_name == "Hebrew" && t.release.as_deref() == Some("sintel.he")),
        "sintel.he is Hebrew: {:?}",
        names("Hebrew")
    );
    let named = |release: &str| {
        s.subtitles
            .iter()
            .find(|t| t.release.as_deref() == Some(release))
            .map(|t| t.name.clone())
    };
    assert_eq!(
        named("Sintel.2010.1080p.WEB.H264-CLASSiCALHD").as_deref(),
        Some("CLASSiCALHD, WEB 1080p")
    );
    assert_eq!(
        named("Sintel.2010.1080p-cze(1)").as_deref(),
        Some("1080p"),
        "a copy number is no group"
    );
    assert_eq!(named("sintel_fi_ps").as_deref(), Some("Finnish"));

    // Review P3-3: a series' names too, from OpenSubtitles v3's answer for
    // Breaking Bad S1E1 (tests/fixtures/opensubtitles-breaking-bad-s01e01.json,
    // recorded 2026-10-23).
    let addon = recorded_subtitles(
        "org.stremio.opensubtitlesv3",
        include_str!("fixtures/opensubtitles-breaking-bad-s01e01.json"),
    )
    .await;
    let view = open_with_subtitles(&["eng"], vec![addon]).await;
    let s = settled_list(&view).await;
    for t in &s.subtitles {
        let lower = t.name.to_lowercase();
        assert!(
            ![
                "breaking",
                "bad",
                "s01e01",
                "pilot",
                ".srt",
                "opensubtitles"
            ]
            .iter()
            .any(|w| lower.contains(w)),
            "{} reads {:?}",
            t.release.as_deref().unwrap_or_default(),
            t.name
        );
    }
    let named = |release: &str| {
        s.subtitles
            .iter()
            .find(|t| t.release.as_deref() == Some(release))
            .map(|t| t.name.clone())
    };
    assert_eq!(
        named("Breaking.Bad.S01E01.720p.BluRay.X264-REWARD").as_deref(),
        Some("REWARD, BluRay 720p")
    );
    assert!(
        named("Breaking Bad Season 1 Episode 01 - Pilot")
            .is_some_and(|n| n.starts_with("English ")),
        "a name with nothing to tell reads as its language"
    );
}

/// Opens Sintel with the stream addon and `subtitles` installed in this
/// order, the profile's subtitle languages set to `languages`.
async fn open_with_subtitles(languages: &[&str], subtitles: Vec<AddonRow>) -> PlaybackView {
    let (video, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": languages }))
        .unwrap();
    let mut rows = vec![row(LOCAL_ADDON, "local", "stream", true)];
    rows.extend(subtitles);
    install(&core, &rows);
    let streams = serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": "Sintel.2010.1080p.WEB"}]})
        .to_string();
    PlaybackView::open(
        &core,
        listed(LOCAL_ADDON, "local", "Local", &streams),
        target(),
    )
}

fn start_source(s: &PlaybackSnapshot) -> Option<String> {
    s.subtitles
        .iter()
        .find(|t| Some(&t.url) == s.auto_subtitle.as_ref())
        .map(|t| t.source.clone())
}

/// Review P2-1 and P3-1: a profile with Turkish then English. English
/// listed at once never settles the start early, as a slower source may
/// bring Turkish; Turkish listed at 2.5 s settles it then, with a source
/// still out until 8 s, and that one changes nothing. The margins (0.7 s
/// before Turkish, 5.5 s after it) hold on a loaded machine.
#[tokio::test]
async fn the_start_subtitle_settles_early_only_in_the_first_language() {
    let view = open_with_subtitles(
        &["tur", "eng"],
        vec![
            subtitles_addon("late", "eng", Duration::from_secs(8)).await,
            subtitles_addon("turkish", "tur", Duration::from_millis(2500)).await,
            subtitles_addon("english", "eng", Duration::ZERO).await,
        ],
    )
    .await;
    until(&view, "the English track", |s| {
        s.subtitles.iter().any(|t| t.source == "english")
    })
    .await;
    // Past the 1 s wait, Turkish not listed yet.
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let before = view.snapshot();
    assert!(!before.subtitles.iter().any(|t| t.source == "turkish"));
    assert_eq!(
        before.auto_subtitle, None,
        "English settled before Turkish came"
    );
    let s = until(&view, "the start subtitle", |s| s.auto_subtitle.is_some()).await;
    assert!(
        s.subtitles_pending,
        "settled only when the late addon answered"
    );
    assert_eq!(start_source(&s).as_deref(), Some("turkish"));
    let done = settled_list(&view).await;
    assert_eq!(done.auto_subtitle, s.auto_subtitle);
}

/// Review P2-1: when the first language never comes, the second settles
/// only once every source has answered.
#[tokio::test]
async fn a_second_language_waits_for_every_source() {
    let view = open_with_subtitles(
        &["tur", "eng"],
        vec![
            subtitles_addon("late", "eng", Duration::from_secs(3)).await,
            subtitles_addon("english", "eng", Duration::ZERO).await,
        ],
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    let s = view.snapshot();
    assert!(s.subtitles_pending && s.auto_subtitle.is_none(), "{s:?}");
    let done = settled_list(&view).await;
    assert!(
        done.auto_subtitle.is_some(),
        "English turns on once every source answered"
    );
}

/// Review P3: the viewer's pick stays the pick: the start subtitle never
/// comes over it, early or last. The profile wants English, which only the
/// 3 s addon lists, so nothing can settle before the viewer picks the
/// French track listed at once, however slow the machine.
#[tokio::test]
async fn a_pick_before_the_wait_stays_the_pick() {
    let view = open_with_subtitles(
        &["eng"],
        vec![
            subtitles_addon("late", "eng", Duration::from_secs(3)).await,
            subtitles_addon("french", "fre", Duration::ZERO).await,
        ],
    )
    .await;
    let s = until(&view, "the French track", |s| {
        s.subtitles.iter().any(|t| t.source == "french")
    })
    .await;
    assert_eq!(s.auto_subtitle, None);
    let picked = s
        .subtitles
        .iter()
        .find(|t| t.source == "french")
        .unwrap()
        .url
        .clone();
    view.subtitle_chosen(Some(&picked), None);
    let done = settled_list(&view).await;
    assert!(done.subtitles.iter().any(|t| t.source == "late"));
    assert_eq!(
        done.auto_subtitle, None,
        "the English track came over the pick"
    );
}

/// What Apple picks its player engine by (plans/apple-second-engine.md
/// 2.1): the opened file's container and the release's audio, once
/// `ready`; and `playedOn`, which the next play of the same version reports
/// back as `engine`, on this device only.
#[tokio::test]
async fn the_snapshot_names_the_container_audio_and_the_engine_that_played() {
    let (video, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = serde_json::json!({"streams": [{
        "url": format!("{video}/v"),
        "title": "Sintel.2010.1080p.BluRay.DTS-HD.MA.5.1.x264-GRP",
        "behaviorHints": { "filename": "Sintel.2010.1080p.BluRay.DTS-HD.MA.5.1.x264-GRP.mkv" },
    }]})
    .to_string();
    let open = || PlaybackView::open(&core, setup(&core, &streams), target());
    let view = open();
    let s = until(&view, "ready", |s| s.phase == PlaybackPhase::Ready).await;
    assert_eq!(s.container.as_deref(), Some("mkv"));
    assert_eq!(s.audio, ["dts_hd_ma"]);
    assert_eq!(s.engine, None);
    let key = s.key.clone().unwrap();
    assert!(view.played_on(&key, "vlc").is_err(), "only a known engine");
    view.played_on(&key, "aether").unwrap();
    assert_eq!(view.snapshot().engine.as_deref(), Some("aether"));
    drop(view);
    let again = until(&open(), "ready again", |s| s.phase == PlaybackPhase::Ready).await;
    assert_eq!(
        again.engine.as_deref(),
        Some("aether"),
        "remembered for the version"
    );
}

/// `playedOn` counts only once `ready` and for the version playing, as
/// `duration` does; a failover clears what the snapshot said of the source.
#[tokio::test]
async fn played_on_waits_for_ready_and_its_version_and_a_failover_clears_the_source() {
    let base = origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = format!(
        r#"{{"streams":[
          {{"name":"Live","url":"{base}/ok.mkv","title":"Sintel.2010.1080p.BluRay.DTS-HD.MA.5.1.x264-GRP","behaviorHints":{{"filename":"Sintel.2010.1080p.BluRay.DTS-HD.MA.5.1.x264-GRP.mkv","proxyHeaders":{{"request":{{"x-key":"1"}}}}}}}},
          {{"name":"Dead","url":"{base}/dead.mkv"}}
        ]}}"#
    );
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    // No await yet: the resolve has not run, so the view is not ready.
    let early = view.snapshot();
    assert_ne!(early.phase, PlaybackPhase::Ready);
    view.played_on(early.key.as_deref().unwrap_or("any"), "aether")
        .unwrap();
    assert_eq!(view.snapshot().engine, None, "before ready");
    let s = until(&view, "ready", |s| s.phase == PlaybackPhase::Ready).await;
    assert_eq!(s.engine, None, "the early mark was not kept");
    view.played_on("another version", "aether").unwrap();
    assert_eq!(view.snapshot().engine, None, "another version");
    view.played_on(s.key.as_deref().unwrap(), "aether").unwrap();
    let marked = view.snapshot();
    assert_eq!(marked.container.as_deref(), Some("mkv"));
    assert_eq!(marked.audio, ["dts_hd_ma"]);
    assert_eq!(marked.engine.as_deref(), Some("aether"));
    view.fail_over(Some(s.attempt), None, None, "decoder error");
    let after = settled(&view).await;
    assert_eq!(after.phase, PlaybackPhase::Failed, "{after:?}");
    assert_eq!(
        (after.container, after.audio.len(), after.engine),
        (None, 0, None)
    );
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
    view.subtitle_chosen(None, None);
    assert_eq!(view.snapshot().auto_subtitle, None);
    drop(view);
    let again = settled_list(&open(&core, "Sintel.2010.1080p.WEB")).await;
    assert_eq!(again.auto_subtitle, None, "Off is remembered for the title");

    // The user picks the English track: next time English turns on, even
    // though Turkish is the profile's language.
    let view = open(&core, "Sintel.2010.1080p.WEB");
    let s = settled_list(&view).await;
    view.subtitle_chosen(Some(&url(&s, "eng")), None);
    drop(view);
    let next = settled_list(&open(&core, "Sintel.2010.1080p.WEB")).await;
    assert_eq!(next.auto_subtitle, Some(url(&next, "eng")));
}

/// The owner's flow (plans/m13-subtitle-languages.md 7): a subtitle picked
/// in one title puts its language first in the profile's languages, so the
/// next title starts with it on; Off stays that title's own. A kids
/// profile's preferences stay as the owner left them (plans/m11-kids.md):
/// its pick is that title's own.
#[tokio::test]
async fn a_picked_subtitle_language_starts_the_next_titles() {
    let (video, _) = counted_origin().await;
    let subs: Vec<_> = ["eng", "tur", "ger"]
        .iter()
        .map(|lang| serde_json::json!({"id": lang, "lang": lang, "url": format!("https://subs.test/{lang}.srt")}))
        .collect();
    let stream = serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": "Film.2020.1080p.WEB", "subtitles": subs}]})
        .to_string();
    let open = |core: &Core, id: &str| {
        let title = PlayTarget {
            item_id: id.into(),
            video_id: id.into(),
            ..target()
        };
        PlaybackView::open(core, setup(core, &stream), title)
    };
    // `lang`: the language of a track by its url; `url`: the track of a language.
    let lang = |s: &PlaybackSnapshot, url: Option<&String>| {
        let on = s.subtitles.iter().find(|t| Some(&t.url) == url);
        on.map(|t| t.lang.clone())
    };
    let url = |s: &PlaybackSnapshot, lang: &str| {
        s.subtitles
            .iter()
            .find(|t| t.lang == lang)
            .unwrap()
            .url
            .clone()
    };
    let started = async |core: &Core, id: &str| {
        let s = settled_list(&open(core, id)).await;
        lang(&s, s.auto_subtitle.as_ref())
    };
    let pick = async |core: &Core, id: &str, choice: Option<&str>| {
        let view = open(core, id);
        let s = settled_list(&view).await;
        view.subtitle_chosen(choice.map(|l| url(&s, l)).as_deref(), None);
    };
    let on = |l: &str| Some(l.to_owned());

    let owner = Core::temporary(mq_library::OWNER).unwrap();
    owner
        .set_pin(mq_library::OWNER, Some("1357"), None)
        .await
        .unwrap();
    let kid = owner.for_profile(&owner.create_kids_profile("Kid", "fox").unwrap());
    for id in ["tt1", "tt2"] {
        let row = mq_library::AllowedRow {
            item_id: id.into(),
            kind: "movie".into(),
            name: id.into(),
            names: vec![],
            poster: None,
            allowed: true,
            added_ms: 1,
        };
        kid.library()
            .write_allowed(kid.profile(), &row, &kid.hlc())
            .unwrap();
    }

    // Title A: nothing set, nothing on; the user picks Turkish.
    assert_eq!(started(&owner, "tt1").await, None);
    pick(&owner, "tt1", Some("tur")).await;
    assert_eq!(owner.prefs().subtitle_languages, ["tr"]);
    // Title B starts with Turkish on; the user turns it off.
    assert_eq!(started(&owner, "tt2").await, on("tur"));
    pick(&owner, "tt2", None).await;
    assert_eq!(owner.prefs().subtitle_languages, ["tr"], "Off is B's own");
    // Title C starts with Turkish on; B stays off.
    assert_eq!(started(&owner, "tt3").await, on("tur"));
    assert_eq!(started(&owner, "tt2").await, None);
    // A German pick goes first, Turkish after it.
    pick(&owner, "tt3", Some("ger")).await;
    assert_eq!(owner.prefs().subtitle_languages, ["de", "tr"]);
    assert_eq!(started(&owner, "tt4").await, on("ger"));

    // The kids profile set nothing: the owner's picks start nothing there.
    assert!(kid.prefs().subtitle_languages.is_empty());
    assert_eq!(started(&kid, "tt1").await, None);
    // Its English pick is that title's own: its preferences and its next
    // title stay as they were, and the owner's too.
    pick(&kid, "tt1", Some("eng")).await;
    assert!(kid.prefs().subtitle_languages.is_empty());
    assert_eq!(started(&kid, "tt1").await, on("eng"));
    assert_eq!(started(&kid, "tt2").await, None);
    assert_eq!(owner.prefs().subtitle_languages, ["de", "tr"]);
}

/// A pick of the playing audio's own language (subtitles for the hard of
/// hearing, a learner) is kept like any other: the title starts with it
/// again, the next title in another language too, while a title whose
/// audio is in it starts nothing.
#[tokio::test]
async fn a_pick_in_the_audios_own_language_is_kept_and_skipped_for_that_audio() {
    let (video, _) = counted_origin().await;
    let stream = |title: &str| {
        let subs: Vec<_> = ["eng", "tur"]
            .iter()
            .map(|lang| serde_json::json!({"id": lang, "lang": lang, "url": format!("https://subs.test/{lang}.srt")}))
            .collect();
        serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": title, "subtitles": subs}]})
            .to_string()
    };
    let open = |core: &Core, id: &str, title: &str| {
        let target = PlayTarget {
            item_id: id.into(),
            video_id: id.into(),
            ..target()
        };
        PlaybackView::open(core, setup(core, &stream(title)), target)
    };
    let started = async |core: &Core, id: &str, title: &str| {
        let s = settled_list(&open(core, id, title)).await;
        let on = s
            .subtitles
            .iter()
            .find(|t| Some(&t.url) == s.auto_subtitle.as_ref());
        on.map(|t| t.lang.clone())
    };
    let core = Core::temporary("p").unwrap();
    // Turkish audio (the release says so): the viewer turns Turkish on.
    let dubbed = "Film.2020.TURKISH.1080p.WEB";
    let view = open(&core, "tt1", dubbed);
    let s = settled_list(&view).await;
    assert_eq!(s.auto_subtitle, None);
    let turkish = s
        .subtitles
        .iter()
        .find(|t| t.lang == "tur")
        .unwrap()
        .url
        .clone();
    view.subtitle_chosen(Some(&turkish), None);
    drop(view);
    assert_eq!(core.prefs().subtitle_languages, ["tr"]);
    // The title remembers it; another title in another language starts with it;
    // another one dubbed in Turkish starts nothing.
    assert_eq!(started(&core, "tt1", dubbed).await.as_deref(), Some("tur"));
    assert_eq!(
        started(&core, "tt2", "Film.2020.1080p.WEB")
            .await
            .as_deref(),
        Some("tur")
    );
    assert_eq!(started(&core, "tt3", dubbed).await, None);
}

/// A track inside the video file comes as its language code, with no url:
/// it counts as a core track of that language, so the title remembers it
/// and it goes first in the profile's languages; a code the core does not
/// know only stops the start pick.
#[tokio::test]
async fn an_in_file_pick_puts_its_language_first() {
    let (video, _) = counted_origin().await;
    let subs: Vec<_> = ["eng", "ger"]
        .iter()
        .map(|lang| serde_json::json!({"id": lang, "lang": lang, "url": format!("https://subs.test/{lang}.srt")}))
        .collect();
    let stream = serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": "Film.2020.1080p.WEB", "subtitles": subs}]})
        .to_string();
    let open = |core: &Core, id: &str| {
        let title = PlayTarget {
            item_id: id.into(),
            video_id: id.into(),
            ..target()
        };
        PlaybackView::open(core, setup(core, &stream), title)
    };
    let started = async |core: &Core, id: &str| {
        let s = settled_list(&open(core, id)).await;
        let on = s
            .subtitles
            .iter()
            .find(|t| Some(&t.url) == s.auto_subtitle.as_ref());
        on.map(|t| t.lang.clone())
    };
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["en"] }))
        .unwrap();

    // The file's German track, by its ISO 639-2 code: German first, the
    // title and the next one start with it.
    let view = open(&core, "tt1");
    let s = settled_list(&view).await;
    assert!(s.auto_subtitle.is_some());
    view.subtitle_chosen(None, Some("ger"));
    assert_eq!(view.snapshot().auto_subtitle, None);
    drop(view);
    assert_eq!(core.prefs().subtitle_languages, ["de", "en"]);
    assert_eq!(started(&core, "tt1").await.as_deref(), Some("ger"));
    assert_eq!(started(&core, "tt2").await.as_deref(), Some("ger"));

    // A file track of no known language: the start pick stops, and nothing
    // is written, so the title still starts with German.
    let view = open(&core, "tt2");
    settled_list(&view).await;
    view.subtitle_chosen(None, Some("und"));
    assert_eq!(view.snapshot().auto_subtitle, None);
    drop(view);
    assert_eq!(core.prefs().subtitle_languages, ["de", "en"]);
    assert_eq!(started(&core, "tt2").await.as_deref(), Some("ger"));
}

/// A link slow to send its first byte says "Opening the video" once its
/// start has taken 3 s (plans/core-protocol.md, Playback `start`), as a
/// torrent's stage does, and still plays when the byte comes.
#[tokio::test]
async fn a_link_slow_to_send_its_first_byte_says_it_opens_the_video() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(AtomicUsize::new(0));
    let count = asked.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let first = count.fetch_add(1, Ordering::SeqCst) == 0;
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                // The first read waits 5 s for its first byte.
                if first {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                let _ = socket
                    .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/10\r\nContent-Length: 1\r\nConnection: close\r\n\r\nM")
                    .await;
            });
        }
    });
    let core = Core::temporary("p").unwrap();
    let streams = serde_json::json!({"streams": [{"url": format!("{base}/v.mkv"), "title": "Film.2020.1080p.WEB"}]})
        .to_string();
    let began = std::time::Instant::now();
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let caption = |s: &PlaybackSnapshot| s.start.as_ref().and_then(|p| p.caption.clone());
    let s = until(&view, "the opening words or the end of the start", |s| {
        caption(s).is_some() || s.phase != PlaybackPhase::Resolving
    })
    .await;
    assert_eq!(caption(&s).as_deref(), Some("Opening the video"), "{s:?}");
    assert_eq!(s.start.as_ref().map(|p| p.stage), Some(StartStage::Opening));
    assert!(
        began.elapsed() >= Duration::from_secs(3),
        "{:?}",
        began.elapsed()
    );
    let s = until(&view, "the play", |s| s.phase != PlaybackPhase::Resolving).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
}

/// The 3 s words never come to a link that opens at once.
#[tokio::test]
async fn a_link_that_opens_at_once_never_says_it_opens_the_video() {
    let (video, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = serde_json::json!({"streams": [{"url": format!("{video}/v.mkv"), "title": "Film.2020.1080p.WEB"}]})
        .to_string();
    let began = std::time::Instant::now();
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let (_, mut updates) = view.subscribe();
    let mut said = Vec::new();
    while let Ok(Some(batch)) = tokio::time::timeout(Duration::from_secs(10), updates.next()).await
    {
        said.extend(
            batch
                .iter()
                .filter_map(|s| s.start.as_ref()?.caption.clone()),
        );
        if batch.iter().any(|s| s.phase != PlaybackPhase::Resolving) {
            break;
        }
    }
    assert_eq!(view.snapshot().phase, PlaybackPhase::Ready);
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "{:?}",
        began.elapsed()
    );
    assert!(said.is_empty(), "{said:?}");
}

/// After the 3 s words, a source that fails hands over to the next, whose
/// start says it is source 2 again, not "Opening the video".
#[tokio::test]
async fn a_slow_link_that_fails_hands_over_with_the_next_sources_words() {
    // Source 1 holds its first byte 5 s, then is gone; source 2 answers in 1 s.
    let origin = async |wait: u64, reply: &'static [u8]| {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    let _ = socket.write_all(reply).await;
                });
            }
        });
        base
    };
    let gone = origin(
        5,
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    let slow = origin(1, b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/10\r\nContent-Length: 1\r\nConnection: close\r\n\r\nM").await;
    let streams = serde_json::json!({"streams": [
        {"url": format!("{gone}/v.mkv"), "title": "Film.2020.1080p.WEB"},
        {"url": format!("{slow}/v.mkv"), "title": "Film.2020.720p.WEB"}
    ]})
    .to_string();
    let core = Core::temporary("p").unwrap();
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let caption = |s: &PlaybackSnapshot| s.start.as_ref().and_then(|p| p.caption.clone());
    until(&view, "source 1's words", |s| {
        caption(s).as_deref() == Some("Opening the video")
    })
    .await;
    let s = until(&view, "source 2 or the end", |s| {
        s.start.as_ref().is_none_or(|p| p.source == 2)
    })
    .await;
    assert_eq!(
        caption(&s).as_deref(),
        Some("Trying source 2 of 2"),
        "{s:?}"
    );
    let s = until(&view, "the play", |s| s.phase != PlaybackPhase::Resolving).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    assert_eq!(s.attempts.len(), 1, "{s:?}");
}

/// A playback closed while its link waits for its first byte publishes
/// nothing more: the 3 s words never come.
#[tokio::test]
async fn a_playback_closed_while_its_link_waits_says_nothing_more() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(Duration::from_secs(30)).await;
            });
        }
    });
    let core = Core::temporary("p").unwrap();
    let streams = serde_json::json!({"streams": [{"url": format!("{base}/v.mkv"), "title": "Film.2020.1080p.WEB"}]})
        .to_string();
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let (_, mut updates) = view.subscribe();
    until(&view, "the opening stage", |s| {
        s.start
            .as_ref()
            .is_some_and(|p| p.stage == StartStage::Opening)
    })
    .await;
    drop(view);
    let mut said = Vec::new();
    while let Ok(Some(batch)) = tokio::time::timeout(Duration::from_secs(4), updates.next()).await {
        said.extend(
            batch
                .iter()
                .filter_map(|s| s.start.as_ref()?.caption.clone()),
        );
    }
    assert!(said.is_empty(), "{said:?}");
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
    view.subtitle_chosen(Some(&picked), None);
    let s = settled_list(&view).await;
    assert_eq!(releases(&s), [PAL, FILM], "the rate landed after the pick");
    assert_eq!(s.subtitles[1].url, picked);
    assert_eq!(s.auto_subtitle, None);
}

/// Review P2-2: the start subtitle ranks with the probed frame rate in.
/// The rate lands at 1.2 s, after the 1 s wait (the probe gives up at 1.5 s): the pick is the 25 fps
/// track the rate puts first, never the film-rate one listed first.
#[tokio::test]
async fn the_start_subtitle_waits_for_the_frame_rate() {
    let video = late_tracks_origin(mkv_at_25_fps(), Duration::from_millis(1200)).await;
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["eng"] }))
        .unwrap();
    let view = PlaybackView::open(&core, setup(&core, &rated_rows(&video)), target());
    let s = until(&view, "the start subtitle", |s| s.auto_subtitle.is_some()).await;
    let pick = s
        .subtitles
        .iter()
        .find(|t| Some(&t.url) == s.auto_subtitle.as_ref())
        .and_then(|t| t.release.clone());
    assert_eq!(pick.as_deref(), Some(PAL));
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
    assert_eq!(delays(&view.snapshot()), at([("a", -60_000), ("b", 0)]));
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

    // 0 for another track (sent on close for the active one) forgets
    // nothing; 0 for the track itself forgets its delay.
    view.subtitle_delay(&s.subtitles[0].url, 700);
    view.subtitle_delay(&s.subtitles[1].url, 0);
    assert_eq!(delays(&view.snapshot()), at([("a", 700), ("b", 0)]));
    drop(view);
    let view = play("v.mkv");
    let s = settled_list(&view).await;
    assert_eq!(delays(&s), at([("a", 700), ("b", 0)]));
    view.subtitle_delay(&s.subtitles[0].url, 0);
    drop(view);
    let s = settled_list(&play("v.mkv")).await;
    assert_eq!(delays(&s), at([("a", 0), ("b", 0)]));
}

/// Review of feat/subtitle-best P2-4: a pick is of one source's tracks.
/// After a failover the next source's settle turns the title's remembered
/// choice on again.
#[tokio::test]
async fn after_a_failover_the_remembered_choice_turns_on_again() {
    let (video, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    let source = |file: &str| {
        serde_json::json!({"url": format!("{video}/{file}"), "title": "Sintel.2010.1080p.WEB",
            "subtitles": [{"id": file, "lang": "eng", "url": format!("https://subs.test/{file}.srt")}]})
    };
    let streams = serde_json::json!({"streams": [source("v.mkv"), source("w.mkv")]});
    let view = PlaybackView::open(&core, setup(&core, &streams.to_string()), target());
    let first = settled_list(&view).await;
    assert_eq!(first.auto_subtitle, None, "no subtitle languages set");
    view.subtitle_chosen(Some(&first.subtitles[0].url), None);
    view.fail_over(Some(first.attempt), Some("network"), None, "gone");
    let next = until(&view, "the next source", |s| {
        s.attempt > first.attempt && s.phase == PlaybackPhase::Ready && !s.subtitles_pending
    })
    .await;
    assert_eq!(next.auto_subtitle, Some(next.subtitles[0].url.clone()));
}

/// A film split over discs, its first one playing: that disc's subtitle
/// fits, the whole film's too (its start is that disc's), the other disc's
/// sorts last as before.
#[tokio::test]
async fn the_disc_playing_keeps_its_own_subtitle_first() {
    let (video, _) = counted_origin().await;
    let core = Core::temporary("p").unwrap();
    let sub = |id: &str, file: &str| {
        serde_json::json!({"id": id, "lang": "eng", "url": format!("https://subs.test/{id}.srt"),
            "subtitleFileName": file})
    };
    let streams = serde_json::json!({"streams": [{
        "url": format!("{video}/v.avi"),
        "behaviorHints": {"filename": "Shawshank.1994.DVDRip.CD1-aFLDp.avi"},
        "subtitles": [
            sub("2", "Shawshank.1994.DVDRip.CD2-aFLDp.srt"),
            sub("1", "Shawshank.1994.DVDRip.CD1-aFLDp.srt"),
            sub("all", "Shawshank.1994.DVDRip-aFLDp.srt"),
        ],
    }]});
    let view = PlaybackView::open(&core, setup(&core, &streams.to_string()), target());
    let s = settled_list(&view).await;
    assert_eq!(
        releases(&s),
        [
            "Shawshank.1994.DVDRip.CD1-aFLDp",
            "Shawshank.1994.DVDRip-aFLDp",
            "Shawshank.1994.DVDRip.CD2-aFLDp",
        ]
    );
}

/// A film whose title says "forced" ("Forced Vengeance"): the word is in
/// every file's name, the stream's own tracks and an addon's alike, so it
/// marks none of them forced and the start pick still turns one on.
#[tokio::test]
async fn a_title_with_the_word_forced_still_gets_a_subtitle() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let (status, body) = if head.starts_with("GET /subtitles/") {
                    let list = serde_json::json!({"subtitles": [{"id": "1", "lang": "eng",
                        "url": "https://subs.test/addon.srt",
                        "subtitleFileName": "Forced.Vengeance.1982.720p.WEB.x264-ADD.srt"}]});
                    ("200 OK", list.to_string())
                } else {
                    (
                        "206 Partial Content\r\nContent-Range: bytes 0-0/10",
                        "M".into(),
                    )
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&serde_json::json!({ "subtitleLanguages": ["eng"] }))
        .unwrap();
    let stream_addon = format!("{base}/own/manifest.json");
    install(
        &core,
        &[
            row(&format!("{base}/manifest.json"), "subs", "subtitles", true),
            row(&stream_addon, "own", "stream", true),
        ],
    );
    let streams = serde_json::json!({"streams": [{
        "url": format!("{base}/v.mkv"),
        "behaviorHints": {"filename": "Forced.Vengeance.1982.1080p.BluRay.x264-GRP.mkv"},
        "subtitles": [{"id": "own", "lang": "eng", "url": "https://subs.test/own.srt",
            "subtitleFileName": "Forced.Vengeance.1982.1080p.BluRay.x264-GRP.srt"}],
    }]});
    let view = PlaybackView::open(
        &core,
        listed(&stream_addon, "own", "Own", &streams.to_string()),
        target(),
    );
    let s = settled_list(&view).await;
    let sources: Vec<(&str, bool)> = s
        .subtitles
        .iter()
        .map(|t| (t.source.as_str(), t.forced))
        .collect();
    assert_eq!(sources, [("Own", false), ("subs", false)]);
    assert_eq!(
        s.auto_subtitle.as_deref(),
        Some(s.subtitles[0].url.as_str())
    );
}
