use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_library::AddonRow;
use mq_protocol::decode_streams;
use mq_runtime::{Core, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView};
use mq_streams::{ParsedStream, normalize};
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
