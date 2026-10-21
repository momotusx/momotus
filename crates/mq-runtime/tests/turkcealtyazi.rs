//! TurkceAltyazi.org built into the core (plans/m13-turkcealtyazi.md 3.6
//! to 3.8): the rows that stand for it, the playback that lists it beside
//! the other subtitle addons, and its fetch through the subtitle route,
//! against a fixture site serving the recorded pages and archives.

use mq_http::test_dns::{self, Request};
use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_library::{AddonRow, CachedResponse};
use mq_protocol::decode_streams;
use mq_runtime::{
    AddonsView, Core, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView, SubtitleProblem,
    defaults,
};
use mq_streams::{ParsedStream, normalize};
use mq_turkcealtyazi::Limits;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

const SITE: &str = "turkcealtyazi.test";
/// A copy of the built-in addon on another link, served by the fixture so a
/// request to it would be seen.
const COPY: &str = "turkcealtyazi-copy.test";
const NAME: &str = "TurkceAltyazi.org";

fn corpus(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../mq-turkcealtyazi/tests/corpus/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap()
}

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

/// The fixture site: Breaking Bad's title page, every subtitle page's
/// form, and `episode.zip` for every download, unless `alter` answers
/// first. Returns its port and every request it saw.
fn site(
    alter: impl Fn(&Request) -> Option<Vec<u8>> + Send + Sync + 'static,
) -> (u16, Arc<Mutex<Vec<String>>>) {
    let loopback = [IpAddr::V4(Ipv4Addr::LOCALHOST)];
    test_dns::set_public(SITE, &loopback);
    test_dns::set_public(COPY, &loopback);
    // The default's own addon host: were it asked, nothing leaves the machine.
    let default = Url::parse(&built_in_row().transport_url).unwrap();
    test_dns::set_public(default.host_str().unwrap(), &loopback);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let (title, form, zip) = (
        corpus("title-breaking-bad.html"),
        String::from_utf8(corpus("sub-form.html")).unwrap(),
        corpus("episode.zip"),
    );
    let port = test_dns::https_requests(move |r| {
        log.lock().unwrap().push(format!("{} {}", r.method, r.path));
        if let Some(answer) = alter(r) {
            return answer;
        }
        let html = [("Content-Type", "text/html; charset=UTF-8")];
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/mov/0903747/x.html") => reply(
                "301 Moved Permanently",
                &[("Location", "/mov/0903747/breaking-badd.html")],
                b"",
            ),
            ("GET", "/mov/0903747/breaking-badd.html") => reply("200 OK", &html, &title),
            ("GET", path) if path.starts_with("/sub/") => {
                let altid = path.split('/').nth(2).unwrap_or_default();
                reply("200 OK", &html, form.replace("108735", altid).as_bytes())
            }
            ("POST", "/ind") => reply("200 OK", &[("Content-Type", "application/zip")], &zip),
            _ => reply("404 Not Found", &[], b"no route"),
        }
    });
    (port, seen)
}

fn fast() -> Limits {
    Limits {
        request: Duration::from_secs(3),
        fetch: Duration::from_secs(5),
        spacing: Duration::from_millis(5),
    }
}

fn core_at(port: u16) -> Core {
    let core = Core::temporary("p").unwrap();
    let origin = Url::parse(&format!("https://{SITE}:{port}")).unwrap();
    core.use_turkcealtyazi_at(origin, fast());
    core
}

/// The built-in default as onboarding installs it.
fn built_in_row() -> AddonRow {
    let d = defaults::list()
        .iter()
        .find(|d| d.key == "turkcealtyazi")
        .unwrap();
    AddonRow {
        transport_url: d.url.clone(),
        manifest_json: serde_json::to_string(&d.manifest).unwrap(),
        position: 0,
        enabled: true,
        allow_private: false,
    }
}

/// The same addon on another link, as an import may bring it.
fn copy_row(port: u16) -> AddonRow {
    AddonRow {
        transport_url: format!("https://{COPY}:{port}/manifest.json"),
        ..built_in_row()
    }
}

fn row(transport_url: &str, id: &str, resource: &str, allow_private: bool) -> AddonRow {
    let manifest = serde_json::json!({
        "id": id, "version": "1.0.0", "name": id,
        "types": ["movie", "series"], "resources": [resource], "catalogs": []
    });
    AddonRow {
        transport_url: transport_url.into(),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private,
    }
}

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

fn listed(addon_url: &str, streams: &str) -> Vec<ParsedStream> {
    let addon_url = Url::parse(addon_url).unwrap();
    decode_streams(streams.as_bytes())
        .unwrap()
        .streams
        .iter()
        .enumerate()
        .map(|(i, w)| ParsedStream {
            addon_url: Some(addon_url.clone()),
            ..normalize(w, "own", "Own", 0, i as u32)
        })
        .collect()
}

fn s01e01() -> PlayTarget {
    PlayTarget {
        kind: "series".into(),
        item_id: "tt0903747".into(),
        video_id: "tt0903747:1:1".into(),
        name: Some("Breaking Bad".into()),
        season: Some(1),
        episode: Some(1),
        ..PlayTarget::default()
    }
}

/// A loopback origin serving `video` with ranges, or ignoring them (no
/// hash, no frame rate) when `video` is empty.
async fn video_origin(video: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let video = Arc::new(video);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let video = video.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                let range = head
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim().split_once('-'))
                    .and_then(|(a, b)| Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok())));
                let out = match range {
                    Some((first, last)) if !video.is_empty() => {
                        let end = last.map_or(video.len(), |l| (l + 1).min(video.len()));
                        let mut out = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {first}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            end - 1,
                            video.len(),
                            end - first
                        )
                        .into_bytes();
                        out.extend_from_slice(&video[first..end]);
                        out
                    }
                    _ => b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\nM"
                        .to_vec(),
                };
                let _ = socket.write_all(&out).await;
            });
        }
    });
    base
}

/// A Matroska header whose video track says 25 fps (DefaultDuration 40 ms),
/// padded to 256 KiB.
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
        el(&[0x18, 0x53, 0x80, 0x67], &[info, tracks, cluster].concat()),
    ]
    .concat();
    file.resize(256 * 1024, 0);
    file
}

/// The first snapshot that `holds`, within a minute.
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

fn settled(s: &PlaybackSnapshot) -> bool {
    s.phase == PlaybackPhase::Failed || (s.phase == PlaybackPhase::Ready && !s.subtitles_pending)
}

/// A subtitles addon that answers, from the response cache, one Turkish
/// track named `label`.
fn cached_addon(core: &Core, host: &str, label: &str) -> AddonRow {
    let answer = serde_json::json!({"subtitles": [
        {"id": label, "lang": "tur", "label": label, "url": format!("https://{host}/{label}.srt")}
    ]});
    let cached = CachedResponse {
        body: answer.to_string().into_bytes(),
        stored_ms: mq_library::hlc::now_ms() as i64,
        max_age_s: 3600,
        stale_revalidate_s: 0,
        stale_error_s: 0,
    };
    let key = format!("https://{host}/subtitles/series/tt0903747%3A1%3A1.json");
    let removals = core.library().removals();
    assert!(core.library().cache_put(&key, &cached, removals).unwrap());
    row(
        &format!("https://{host}/manifest.json"),
        host,
        "subtitles",
        false,
    )
}

const STREAM: &str =
    r#"{"streams":[{"url":"VIDEO","title":"Breaking.Bad.S01E01.DVDRip.XviD-ORPHEUS"}]}"#;

#[tokio::test]
async fn the_built_in_source_lists_once_in_its_place_and_plays() {
    let (port, seen) = site(|_| None);
    let core = core_at(port);
    let video = video_origin(Vec::new()).await;
    let own = format!("{video}/manifest.json");
    let before = cached_addon(&core, "before.test", "before");
    let after = cached_addon(&core, "after.test", "after");
    install(
        &core,
        &[
            before,
            built_in_row(),
            after,
            copy_row(port),
            row(&own, "own", "stream", true),
        ],
    );
    let streams = STREAM.replace("VIDEO", &format!("{video}/video.mkv"));
    let view = PlaybackView::open(&core, listed(&own, &streams), s01e01());
    let s = until(&view, "subtitles", settled).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    let sources: Vec<&str> = s.subtitles.iter().map(|t| t.source.as_str()).collect();
    // The file naming the playing group leads the language (plans/
    // player-polish.md 2.2); its other 6 Turkish files, which match no
    // better than the addons', sit where its first row sits, between the two
    // addons around it, whichever answered first.
    let mut expected = vec![NAME, "before.test"];
    expected.extend([NAME; 6]);
    expected.push("after.test");
    assert_eq!(sources, expected, "{s:?}");
    let first = &s.subtitles[0];
    assert!(first.fits);
    assert_eq!(first.lang, "tur");
    assert_eq!(
        first.label.as_deref(),
        Some("ORPHEUS DVDRip, 23.976 fps, season 1 pack"),
        "the playing group first"
    );
    // One title visit for two rows, and no request to either addon host.
    let asked = seen.lock().unwrap().clone();
    assert_eq!(
        asked,
        [
            "GET /mov/0903747/x.html",
            "GET /mov/0903747/breaking-badd.html"
        ]
    );

    // Played: the subtitle page, the download, and the episode's cues.
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let vtt = http.get(&first.url, &local).await.unwrap();
    let text = String::from_utf8_lossy(&vtt.body);
    assert!(
        text.contains("00:02:06.573 --> 00:02:08.487\nBenim adım Walter Hartwell White."),
        "{text}"
    );
    assert_eq!(seen.lock().unwrap().len(), 4, "a form and a download");
}

#[tokio::test]
async fn the_frame_rate_read_from_the_video_ranks_the_files() {
    let (port, _) = site(|_| None);
    let core = core_at(port);
    let video = video_origin(mkv_at_25_fps()).await;
    let own = format!("{video}/manifest.json");
    install(&core, &[built_in_row(), row(&own, "own", "stream", true)]);
    // No group or source in the name: only the frame rate tells them apart.
    let streams =
        format!(r#"{{"streams":[{{"url":"{video}/video.mkv","title":"Breaking Bad S01E01"}}]}}"#);
    let view = PlaybackView::open(&core, listed(&own, &streams), s01e01());
    let s = until(&view, "subtitles", settled).await;
    let labels: Vec<&str> = s
        .subtitles
        .iter()
        .filter_map(|t| t.label.as_deref())
        .collect();
    // At 25 fps the BiA file (25 fps) leads; unread, the 23.976 ones would.
    assert_eq!(labels.first(), Some(&"BiA, 25 fps, by F471H"), "{labels:?}");
}

#[tokio::test]
async fn pending_ends_only_after_the_built_in_listing_too() {
    let (port, _) = site(|r| {
        (r.path == "/mov/0903747/x.html").then(|| {
            std::thread::sleep(Duration::from_millis(800));
            reply(
                "301 Moved Permanently",
                &[("Location", "/mov/0903747/breaking-badd.html")],
                b"",
            )
        })
    });
    let core = core_at(port);
    let video = video_origin(Vec::new()).await;
    let own = format!("{video}/manifest.json");
    let before = cached_addon(&core, "before.test", "before");
    install(
        &core,
        &[before, built_in_row(), row(&own, "own", "stream", true)],
    );
    let streams = STREAM.replace("VIDEO", &format!("{video}/video.mkv"));
    let view = PlaybackView::open(&core, listed(&own, &streams), s01e01());
    let s = until(&view, "subtitles", settled).await;
    // The addon answered at once; pending held until the listing came.
    assert_eq!(
        s.subtitles.iter().filter(|t| t.source == NAME).count(),
        7,
        "{s:?}"
    );
}

#[tokio::test]
async fn a_site_problem_or_a_bad_file_leaves_a_note() {
    // The site answers 503: a note, no tracks.
    let (port, _) = site(|r| {
        r.path
            .starts_with("/mov/")
            .then(|| reply("503 Service Unavailable", &[], b"down"))
    });
    let core = core_at(port);
    let video = video_origin(Vec::new()).await;
    let own = format!("{video}/manifest.json");
    install(&core, &[built_in_row(), row(&own, "own", "stream", true)]);
    let streams = STREAM.replace("VIDEO", &format!("{video}/video.mkv"));
    let view = PlaybackView::open(&core, listed(&own, &streams), s01e01());
    let s = until(&view, "subtitles", settled).await;
    let notes: Vec<_> = s
        .subtitle_notes
        .iter()
        .map(|n| (n.source.as_str(), n.problem))
        .collect();
    assert_eq!(notes, [(NAME, SubtitleProblem::Unreachable)]);
    assert!(s.subtitles.is_empty());

    // A file whose download is gone: its track leaves, with a note.
    let (port, _) = site(|r| (r.method == "POST").then(|| reply("404 Not Found", &[], b"gone")));
    let core = core_at(port);
    install(&core, &[built_in_row(), row(&own, "own", "stream", true)]);
    let view = PlaybackView::open(&core, listed(&own, &streams), s01e01());
    let s = until(&view, "subtitles", settled).await;
    let first = s.subtitles[0].clone();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let local = RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let vtt = http.get(&first.url, &local).await.unwrap();
    assert_eq!(&vtt.body[..], b"WEBVTT\n\n");
    let after = until(&view, "the note", |s| !s.subtitle_notes.is_empty()).await;
    assert!(
        after.subtitles.iter().all(|t| t.url != first.url),
        "{after:?}"
    );
    let notes: Vec<_> = after
        .subtitle_notes
        .iter()
        .map(|n| (n.source.as_str(), n.problem))
        .collect();
    assert_eq!(notes, [(NAME, SubtitleProblem::TrackFailed)]);
}

#[tokio::test]
async fn its_rows_switch_and_go_together_and_are_never_refreshed() {
    let (port, seen) = site(|_| None);
    let core = core_at(port);
    let other = row(
        "https://other.test/manifest.json",
        "other",
        "subtitles",
        false,
    );
    install(&core, &[built_in_row(), other.clone(), copy_row(port)]);
    let copy = copy_row(port).transport_url;
    let states = |core: &Core| -> Vec<(String, bool)> {
        core.library()
            .addons(core.profile())
            .unwrap()
            .into_iter()
            .map(|r| (r.transport_url, r.enabled))
            .collect()
    };
    core.set_addon_enabled(&copy, false).unwrap();
    assert_eq!(
        states(&core),
        [
            (built_in_row().transport_url, false),
            (other.transport_url.clone(), true),
            (copy.clone(), false)
        ]
    );
    // An ordinary addon switches alone.
    core.set_addon_enabled(&other.transport_url, false).unwrap();
    core.set_addon_enabled(&other.transport_url, true).unwrap();
    assert!(!states(&core)[0].1);

    // The daily refresh never asks its hosts (the copy's is the fixture).
    core.refresh_addons(mq_library::hlc::now_ms())
        .await
        .unwrap();
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|r| !r.ends_with("/manifest.json")),
        "{:?}",
        seen.lock().unwrap()
    );

    core.remove_addon(&built_in_row().transport_url).unwrap();
    assert_eq!(states(&core), [(other.transport_url, true)]);
}

#[tokio::test]
async fn the_addon_list_says_built_in_and_offers_no_configure_page() {
    let (port, _) = site(|_| None);
    let core = core_at(port);
    let other = row(
        "https://other.test/manifest.json",
        "other",
        "subtitles",
        false,
    );
    install(&core, &[built_in_row(), copy_row(port), other]);
    let addons = AddonsView::open(&core).snapshot().addons;
    let facts: Vec<_> = addons
        .iter()
        .map(|a| (a.built_in, a.configurable, a.configure_url.is_some()))
        .collect();
    // The bundled manifest says configurable; the built-in never opens it.
    assert_eq!(
        facts,
        [
            (true, false, false),
            (true, false, false),
            (false, false, false)
        ]
    );
    // Its own name and line, not the old addon's Turkish description.
    let words: Vec<_> = addons
        .iter()
        .map(|a| (a.name.as_str(), a.description.as_deref()))
        .collect();
    let line = Some("Turkish subtitles from TurkceAltyazi.org.");
    assert_eq!(words, [(NAME, line), (NAME, line), ("other", None)]);
}
