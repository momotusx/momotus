use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_library::AddonRow;
use mq_protocol::decode_streams;
use mq_runtime::{Core, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView};
use mq_streams::{ParsedStream, normalize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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

/// Installs a local addon (so its LAN URLs are allowed) and returns the
/// parsed candidates in the order given.
fn setup(core: &Core, streams: &str) -> Vec<ParsedStream> {
    let manifest = serde_json::json!({
        "id": "local", "version": "1.0.0", "name": "Local",
        "types": ["movie"], "resources": ["stream"], "catalogs": []
    });
    core.library()
        .set_addons(
            core.profile(),
            &[AddonRow {
                transport_url: "http://127.0.0.1:9/manifest.json".into(),
                manifest_json: manifest.to_string(),
                position: 0,
                enabled: true,
                allow_private: true,
            }],
            &core.hlc(),
        )
        .unwrap();
    decode_streams(streams.as_bytes())
        .unwrap()
        .streams
        .iter()
        .enumerate()
        .map(|(i, w)| normalize(w, "local", "Local", 0, i as u32))
        .collect()
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

#[tokio::test]
async fn falls_over_to_the_first_source_that_answers() {
    let base = origin().await;
    let core = Core::temporary("p").unwrap();
    let streams = format!(
        r#"{{"streams":[
          {{"name":"Torrent","title":"Sintel.2010.1080p","infoHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}},
          {{"name":"Dead","url":"{base}/dead.mkv"}},
          {{"name":"Live","url":"{base}/ok.mkv","behaviorHints":{{"filename":"Sintel.2010.mkv","proxyHeaders":{{"request":{{"x-key":"1"}}}}}}}}
        ]}}"#
    );
    let view = PlaybackView::open(&core, setup(&core, &streams), target());
    let s = settled(&view).await;
    assert_eq!(s.phase, PlaybackPhase::Ready, "{s:?}");
    let errors: Vec<_> = s.attempts.iter().map(|a| a.error.as_str()).collect();
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("debrid account"), "{errors:?}");
    assert!(errors[1].contains("404"), "{errors:?}");

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
    view.fail_over("decoder error");
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

/// A 200 KiB file with Range support plus a subtitles addon that records the
/// path it was asked for.
async fn file_and_subtitles() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(Vec::new()));
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
                    log.lock().unwrap().push(path);
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

#[tokio::test]
async fn finds_subtitles_by_hash_size_and_filename() {
    let (base, asked) = file_and_subtitles().await;
    let core = Core::temporary("p").unwrap();
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
    let candidates: Vec<ParsedStream> = decode_streams(streams.as_bytes())
        .unwrap()
        .streams
        .iter()
        .map(|w| normalize(w, "subs", "Subs", 0, 0))
        .collect();
    let view = PlaybackView::open(&core, candidates, target());
    let mut done = None;
    for _ in 0..500 {
        let s = view.snapshot();
        if s.phase == PlaybackPhase::Ready && !s.subtitles_pending {
            done = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let s = done.expect("subtitles settled");
    let urls: Vec<_> = s.subtitles.iter().map(|t| t.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://own.example/en.srt",
            "https://subs.example/1.srt",
            "https://subs.example/2.srt"
        ]
    );
    let path = asked.lock().unwrap()[0].clone();
    let hash = expected_hash(&video());
    assert_eq!(
        path,
        format!(
            "/subtitles/movie/tt1727587/videoHash={hash}&videoSize=204800&filename=Sintel.2010.mkv.json"
        )
    );
}
