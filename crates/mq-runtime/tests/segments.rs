use mq_library::AddonRow;
use mq_protocol::decode_streams;
use mq_runtime::segments::{Hosts, SegmentKind};
use mq_runtime::{Core, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView};
use mq_streams::{ParsedStream, normalize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DURATION: u64 = 3_500_000;

/// One local server for the stream files and the skip databases. Every
/// request line is recorded; TheIntroDB answers after `delay`.
async fn server(delay: Duration) -> (String, Arc<Mutex<Vec<String>>>) {
    server_failing(delay, 0).await
}

/// As [server], with TheIntroDB refusing (503) its first `failures` lookups.
async fn server_failing(delay: Duration, failures: usize) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let refused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            let refused = refused.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = head.lines().next().unwrap_or_default().to_owned();
                log.lock().unwrap().push(line.clone());
                let json = |body: &str| {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .into_bytes()
                };
                let resp = if line.starts_with("GET /a.mkv") || line.starts_with("GET /b.mkv") {
                    b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/10\r\nContent-Length: 1\r\nConnection: close\r\n\r\nM".to_vec()
                } else if line.starts_with("GET /v3/media")
                    && refused.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < failures
                {
                    b"HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
                } else if line.starts_with("GET /v3/media") {
                    tokio::time::sleep(delay).await;
                    json(
                        r#"{"intro":[{"start_ms":null,"end_ms":90000}],"credits":[{"start_ms":3400000,"end_ms":null}]}"#,
                    )
                } else if line.starts_with("GET /segments") {
                    json(
                        r#"{"intro":{"start_ms":1000,"end_ms":80000},"recap":{"start_ms":95000,"end_ms":120000},"outro":null,"post_credits":null}"#,
                    )
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec()
                };
                let _ = socket.write_all(&resp).await;
            });
        }
    });
    (format!("http://{addr}"), seen)
}

fn setup(core: &Core, base: &str) -> Vec<ParsedStream> {
    core.segments().set_hosts(Hosts {
        theintrodb: base.into(),
        introdb: base.into(),
        aniskip: base.into(),
        kitsu: base.into(),
        local: true,
    });
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
    let streams = format!(
        r#"{{"streams":[{{"name":"A","url":"{base}/a.mkv"}},{{"name":"B","url":"{base}/b.mkv"}}]}}"#
    );
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
        ..PlayTarget::default()
    }
}

async fn wait_for(
    view: &PlaybackView,
    done: impl Fn(&PlaybackSnapshot) -> bool,
) -> PlaybackSnapshot {
    for _ in 0..400 {
        let s = view.snapshot();
        if done(&s) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never reached: {:?}", view.snapshot());
}

fn lookups(seen: &Mutex<Vec<String>>) -> usize {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|l| l.starts_with("GET /v3/media"))
        .count()
}

#[tokio::test]
async fn segments_arrive_after_the_duration_and_fill_the_gaps() {
    let (base, seen) = server(Duration::ZERO).await;
    let core = Core::temporary("p").unwrap();
    let view = PlaybackView::open(&core, setup(&core, &base), target());
    let ready = wait_for(&view, |s| s.phase == PlaybackPhase::Ready).await;
    assert!(ready.segments.is_empty());

    view.report_duration(ready.key.as_deref().unwrap(), DURATION);
    let s = wait_for(&view, |s| s.segments.len() == 3).await;
    let got: Vec<_> = s
        .segments
        .iter()
        .map(|g| (g.kind, g.start_ms, g.end_ms, g.source.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (SegmentKind::Intro, 0, 90_000, "TheIntroDB"),
            (SegmentKind::Recap, 95_000, 120_000, "IntroDB"),
            (SegmentKind::Credits, 3_400_000, DURATION, "TheIntroDB"),
        ]
    );
    assert_eq!(s.next_at_ms, Some(3_400_000));
    let line = seen
        .lock()
        .unwrap()
        .iter()
        .find(|l| l.starts_with("GET /v3/media"))
        .cloned()
        .unwrap();
    assert!(
        line.contains("imdb_id=tt1727587") && line.contains("duration_ms=3500000"),
        "{line}"
    );
}

#[tokio::test]
async fn a_refused_lookup_is_asked_again_while_the_version_plays() {
    let (base, seen) = server_failing(Duration::ZERO, 1).await;
    let core = Core::temporary("p").unwrap();
    core.segments()
        .set_retries(vec![Duration::from_millis(300)]);
    let view = PlaybackView::open(&core, setup(&core, &base), target());
    let ready = wait_for(&view, |s| s.phase == PlaybackPhase::Ready).await;
    view.report_duration(ready.key.as_deref().unwrap(), DURATION);

    // TheIntroDB refused: the filler goes ahead at once.
    let s = wait_for(&view, |s| !s.segments.is_empty()).await;
    assert!(
        s.segments.iter().all(|g| g.source == "IntroDB"),
        "{:?}",
        s.segments
    );
    assert_eq!(s.next_at_ms, None);

    // Asked again: its segments take precedence, and the credits set Up Next.
    let s = wait_for(&view, |s| s.next_at_ms.is_some()).await;
    let got: Vec<_> = s
        .segments
        .iter()
        .map(|g| (g.kind, g.source.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (SegmentKind::Intro, "TheIntroDB"),
            (SegmentKind::Recap, "IntroDB"),
            (SegmentKind::Credits, "TheIntroDB"),
        ]
    );
    assert_eq!(s.next_at_ms, Some(3_400_000));
    assert_eq!(lookups(&seen), 2);
}

#[tokio::test]
async fn unusable_reports_start_no_lookup_and_repeats_start_one() {
    let (base, seen) = server(Duration::ZERO).await;
    let core = Core::temporary("p").unwrap();
    let view = PlaybackView::open(&core, setup(&core, &base), target());
    let key = wait_for(&view, |s| s.phase == PlaybackPhase::Ready)
        .await
        .key
        .unwrap();
    view.report_duration("another-version", DURATION);
    view.report_duration(&key, 30_000);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(lookups(&seen), 0);

    view.report_duration(&key, DURATION);
    view.report_duration(&key, DURATION + 1_000);
    wait_for(&view, |s| !s.segments.is_empty()).await;
    assert_eq!(lookups(&seen), 1);
}

#[tokio::test]
async fn a_late_answer_never_lands_on_the_next_source() {
    let (base, seen) = server(Duration::from_millis(800)).await;
    let core = Core::temporary("p").unwrap();
    let view = PlaybackView::open(&core, setup(&core, &base), target());
    let first = wait_for(&view, |s| s.phase == PlaybackPhase::Ready).await;
    view.report_duration(first.key.as_deref().unwrap(), DURATION);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(lookups(&seen), 1, "lookup in flight");

    view.fail_over("decoder error");
    let second = wait_for(&view, |s| {
        s.phase == PlaybackPhase::Ready && s.key.is_some() && s.key != first.key
    })
    .await;
    // Well past the delayed answer: nothing from the first source shows.
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(view.snapshot().segments.is_empty(), "{:?}", view.snapshot());

    view.report_duration(second.key.as_deref().unwrap(), DURATION - 60_000);
    let s = wait_for(&view, |s| !s.segments.is_empty()).await;
    assert_eq!(s.segments[0].source, "TheIntroDB");
}

/// Live: the real databases. Prints one JSON line per lookup (kinds and
/// latency only, never the times). Run with `--ignored --nocapture`.
#[tokio::test]
#[ignore = "network"]
async fn live_skip_sources() {
    use mq_runtime::segments::{Target, merge};
    let core = Core::temporary("p").unwrap();
    let cases = [
        (
            "Breaking Bad S01E01",
            Target::from_ids("series", "tt0903747", "tt0903747:1:1"),
            3_480_000,
        ),
        (
            "Breaking Bad S01E02",
            Target::from_ids("series", "tt0903747", "tt0903747:1:2"),
            2_900_000,
        ),
        (
            "Inception",
            Target::from_ids("movie", "tt1375666", "tt1375666"),
            8_880_000,
        ),
        (
            "Frieren E1",
            Target::from_ids("series", "kitsu:46474", "kitsu:46474:1"),
            1_510_000,
        ),
        (
            "One Piece E1",
            Target::from_ids("series", "kitsu:12", "kitsu:12:1"),
            1_500_000,
        ),
    ];
    for (name, target, duration) in cases {
        let target = target.unwrap();
        let started = std::time::Instant::now();
        let found = core
            .segments()
            .primary(&target, duration)
            .await
            .unwrap_or_default();
        let primary_ms = started.elapsed().as_millis();
        let extra = core
            .segments()
            .fill(&target, duration, &found.segments)
            .await;
        let all = merge(&found.segments, extra);
        let kinds: Vec<String> = all
            .iter()
            .map(|s| format!("{:?}:{}", s.kind, s.source))
            .collect();
        println!(
            "{}",
            serde_json::json!({"title": name, "primaryMs": primary_ms, "totalMs": started.elapsed().as_millis(),
                "kinds": kinds, "nextAt": found.next_at_ms.is_some()})
        );
    }
}

/// A source that fails once is asked again next time, not remembered as a
/// title without segments.
#[tokio::test]
async fn a_failed_lookup_is_not_remembered() {
    use mq_runtime::segments::Target;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let n = seen.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let body = r#"{"intro":[{"start_ms":null,"end_ms":90000}]}"#;
                let resp = if n == 0 {
                    "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let core = Core::temporary("p").unwrap();
    let base = format!("http://{addr}");
    core.segments().set_hosts(Hosts {
        theintrodb: base.clone(),
        introdb: base.clone(),
        aniskip: base.clone(),
        kitsu: base,
        local: true,
    });
    let target = Target::from_ids("movie", "tt1727587", "tt1727587").unwrap();
    assert_eq!(
        core.segments().primary(&target, DURATION).await,
        None,
        "a failure is not a miss"
    );
    let second = core.segments().primary(&target, DURATION).await.unwrap();
    assert_eq!(second.segments.len(), 1, "asked again after the failure");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn imdb_ids_are_tt_and_digits_only() {
    use mq_runtime::segments::Target;
    assert!(Target::from_ids("movie", "tt1&x=1", "tt1&x=1").is_none());
    assert!(Target::from_ids("series", "tt1", "tt1#a:1:2").is_none());
    assert!(Target::from_ids("movie", "tt12345678901", "tt12345678901").is_none());
    assert!(Target::from_ids("movie", "tt0111161", "tt0111161").is_some());
}
