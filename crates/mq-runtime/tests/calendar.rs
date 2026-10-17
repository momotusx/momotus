use mq_library::{AddonRow, LibraryItemRow};
use mq_runtime::views::calendar::parse_day;
use mq_runtime::{CalendarSnapshot, CalendarView, Core};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SHOW: &str = r#"{"meta":{"id":"tt1","type":"series","name":"Show","videos":[
  {"id":"tt1:1:1","title":"Seven days ago","season":1,"episode":1,"released":"2026-10-10T05:00:00.000Z"},
  {"id":"tt1:1:2","title":"Eight days ago","season":1,"episode":2,"released":"2026-10-09T00:00:00.000Z"},
  {"id":"tt1:1:4","title":"Today late","season":1,"episode":4,"released":"2026-10-17T05:00:00.000Z"},
  {"id":"tt1:1:3","title":"Today","season":1,"episode":3,"released":"2026-10-17T00:00:00.000Z"},
  {"id":"tt1:1:5","title":"In thirty days","season":1,"episode":5,"released":"2026-11-16T05:00:00.000Z"},
  {"id":"tt1:1:6","title":"In thirty-one days","season":1,"episode":6,"released":"2026-11-17T05:00:00.000Z"},
  {"id":"tt1:0:1","title":"Special","season":0,"episode":1,"released":"2026-10-17T05:00:00.000Z"},
  {"id":"tt1:1:7","title":"Undated","season":1,"episode":7}]}}"#;

const UNDATED: &str = r#"{"meta":{"id":"tt2","type":"series","name":"Undated Show","videos":[
  {"id":"tt2:1:1","title":"Pilot","season":1,"episode":1}]}}"#;

/// A meta addon: tt1 and tt2 answer after `delay`, anything else is a 404.
async fn meta_addon(delay: Duration) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let line = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = if line.contains("/meta/series/tt1.json") {
                    Some(SHOW)
                } else if line.contains("/meta/series/tt2.json") {
                    Some(UNDATED)
                } else {
                    None
                };
                tokio::time::sleep(delay).await;
                let resp = match body {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .into()
                    }
                };
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "meta", "version": "1.0.0", "name": "Meta",
        "types": ["series"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

fn series(id: &str, name: &str) -> LibraryItemRow {
    LibraryItemRow {
        item_id: id.into(),
        kind: "series".into(),
        name: name.into(),
        poster: Some(format!("https://images.example/{id}.jpg")),
        poster_shape: None,
        added_ms: 1,
    }
}

async fn setup(delay: Duration, ids: &[(&str, &str)]) -> Core {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[meta_addon(delay).await], &core.hlc())
        .unwrap();
    for (id, name) in ids {
        core.library()
            .upsert_item("p", &series(id, name), true, &core.hlc())
            .unwrap();
    }
    core
}

async fn settled(
    view: &CalendarView,
    done: impl Fn(&CalendarSnapshot) -> bool,
) -> CalendarSnapshot {
    for _ in 0..300 {
        let s = view.snapshot();
        if s.pending == 0 && done(&s) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("calendar never settled: {:?}", view.snapshot());
}

fn layout(s: &CalendarSnapshot) -> Vec<(String, Vec<String>)> {
    s.days
        .iter()
        .map(|d| {
            (
                d.date.clone(),
                d.items.iter().map(|i| i.video_id.clone()).collect(),
            )
        })
        .collect()
}

#[tokio::test]
async fn release_dates_are_dates_and_the_window_is_inclusive() {
    let core = setup(Duration::ZERO, &[("tt1", "Show")]).await;
    let view = CalendarView::open(&core, parse_day("2026-10-17").unwrap());
    // Loading is visible from the first snapshot on.
    assert_eq!(view.snapshot().pending, 1);
    let s = settled(&view, |s| !s.days.is_empty()).await;
    assert_eq!(
        layout(&s),
        [
            ("2026-10-10".to_string(), vec!["tt1:1:1".to_string()]),
            (
                "2026-10-17".into(),
                vec!["tt1:1:3".into(), "tt1:1:4".into()]
            ),
            ("2026-11-16".into(), vec!["tt1:1:5".into()]),
        ]
    );
    let item = &s.days[1].items[0];
    assert_eq!((item.season, item.episode), (Some(1), Some(3)));
    assert_eq!(item.name, "Show");
    assert_eq!(
        item.poster.as_deref(),
        Some("https://images.example/tt1.jpg")
    );
}

#[tokio::test]
async fn the_calendar_follows_marks_the_library_and_the_day() {
    let core = setup(Duration::ZERO, &[("tt1", "Show")]).await;
    let view = CalendarView::open(&core, parse_day("2026-10-17").unwrap());
    settled(&view, |s| !s.days.is_empty()).await;

    core.library()
        .set_watched("p", "tt1", "tt1:1:3", true, &core.hlc())
        .unwrap();
    let s = settled(&view, |s| s.days.get(1).is_some_and(|d| d.items[0].watched)).await;
    assert!(!s.days[1].items[1].watched);

    view.set_today(parse_day("2026-11-10").unwrap());
    let s = settled(&view, |s| s.today == "2026-11-10").await;
    assert_eq!(
        layout(&s),
        [
            ("2026-11-16".to_string(), vec!["tt1:1:5".to_string()]),
            ("2026-11-17".into(), vec!["tt1:1:6".into()]),
        ]
    );

    // A window with nothing in it still follows the series: "nothing airs".
    view.set_today(parse_day("2027-06-01").unwrap());
    let s = settled(&view, |s| s.today == "2027-06-01").await;
    assert!(s.days.is_empty(), "{s:?}");
    assert_eq!(s.series, 1);

    // With the series gone there is nothing to follow: "no series".
    core.library()
        .upsert_item("p", &series("tt1", "Show"), false, &core.hlc())
        .unwrap();
    let s = settled(&view, |s| s.series == 0).await;
    assert!(s.days.is_empty(), "{s:?}");
}

#[tokio::test]
async fn a_series_removed_while_loading_leaves_nothing_behind() {
    let core = setup(Duration::from_millis(400), &[]).await;
    let view = CalendarView::open(&core, parse_day("2026-10-17").unwrap());
    core.library()
        .upsert_item("p", &series("tt1", "Show"), true, &core.hlc())
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(view.snapshot().pending, 1, "{:?}", view.snapshot());
    core.library()
        .upsert_item("p", &series("tt1", "Show"), false, &core.hlc())
        .unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    let s = view.snapshot();
    assert!(
        s.days.is_empty() && s.pending == 0 && s.failed.is_empty(),
        "{s:?}"
    );
}

#[tokio::test]
async fn failed_and_unscheduled_series_are_told_apart() {
    let core = setup(
        Duration::ZERO,
        &[
            ("tt1", "Show"),
            ("tt2", "Undated Show"),
            ("tt3", "Missing Show"),
        ],
    )
    .await;
    let view = CalendarView::open(&core, parse_day("2026-10-17").unwrap());
    let s = settled(&view, |s| !s.days.is_empty()).await;
    assert_eq!(
        s.no_schedule
            .iter()
            .map(|x| x.item_id.as_str())
            .collect::<Vec<_>>(),
        ["tt2"]
    );
    assert_eq!(
        s.failed
            .iter()
            .map(|x| x.item_id.as_str())
            .collect::<Vec<_>>(),
        ["tt3"]
    );
    assert!(s.failed[0].reason.is_some());
    assert_eq!(s.series, 3, "loaded, undated and failed series all count");
}

/// Live: Cinemeta's popular series in the library, today's calendar.
/// Run with `--ignored --nocapture`.
#[tokio::test]
#[ignore = "network"]
async fn live_calendar_for_popular_series() {
    let core = Core::temporary("p").unwrap();
    core.install_addon("https://v3-cinemeta.strem.io/manifest.json")
        .await
        .unwrap();
    let http = mq_http::HttpClient::new(Default::default()).unwrap();
    let top: serde_json::Value = http
        .get(
            "https://v3-cinemeta.strem.io/catalog/series/top.json",
            &Default::default(),
        )
        .await
        .unwrap()
        .json()
        .unwrap();
    for meta in top["metas"].as_array().unwrap().iter().take(8) {
        core.library()
            .upsert_item(
                "p",
                &series(meta["id"].as_str().unwrap(), meta["name"].as_str().unwrap()),
                true,
                &core.hlc(),
            )
            .unwrap();
    }
    let today = chrono::DateTime::from_timestamp_millis(mq_library::hlc::now_ms() as i64)
        .unwrap()
        .date_naive();
    let started = std::time::Instant::now();
    let view = CalendarView::open(&core, today);
    let s = settled(&view, |_| true).await;
    let items: usize = s.days.iter().map(|d| d.items.len()).sum();
    println!(
        "{}",
        serde_json::json!({"today": s.today, "series": 8, "days": s.days.len(), "items": items,
            "failed": s.failed.len(), "noSchedule": s.no_schedule.len(), "ms": started.elapsed().as_millis()})
    );
    assert!(s.failed.is_empty(), "{:?}", s.failed);
}
