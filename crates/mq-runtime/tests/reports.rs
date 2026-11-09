//! Error and crash reports on the device (plans/error-reports.md 5, 6.1, 13).
//! The hook and the list are process-wide, so the tests here run one at a
//! time (`SERIAL`) in their own binary.

use mq_runtime::reports::{self, App, Channel, Context, CoreBuild, Device, Form, Kind, Report};
use mq_runtime::{Core, CoreConfig};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// An async lock: each test holds it across its awaits, and it never poisons when a test panics.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// The hook that was there before ours: it must still run.
static PREVIOUS_RAN: AtomicBool = AtomicBool::new(false);
const KEY: &str = "ABCDEFGH1234abcdefgh5678";

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mq-reports-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A core over `dir` with the hook pointed at it and the switch on.
fn open(dir: &Path) -> Core {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        // The hook before ours notes that it ran, then prints as the default one does.
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PREVIOUS_RAN.store(true, Ordering::SeqCst);
            default(info);
        }));
    });
    reports::install(dir);
    let core = Core::open(CoreConfig {
        data_dir: dir.to_path_buf(),
        profile_id: "default".into(),
        device_id: 0,
    })
    .unwrap();
    // The list is process-wide: off then on empties what an earlier test left in it.
    core.set_reports(false).unwrap();
    core.set_reports(true).unwrap();
    core.library()
        .set_device_value("reports.next", "0")
        .unwrap();
    core
}

fn device() -> Device {
    Device {
        app: App::Android,
        channel: Channel::Release,
        version: "0.14.2".into(),
        build: "d8b973d8".into(),
        core: "d8b973d8".into(),
        os: "Android 15".into(),
        model: "Google Pixel 9 Pro".into(),
        abi: "arm64-v8a".into(),
        form: Form::Phone,
        core_build: CoreBuild::Own,
    }
}

fn queue(core: &Core) -> Vec<Report> {
    core.library()
        .device_value("reports.queue")
        .unwrap()
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap_or_default()
}

fn pending(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir.join("reports-pending"))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn panic_in_a_thread(text: &'static str) {
    let _ = std::thread::Builder::new()
        .name("test-panic".into())
        .spawn(move || panic!("{text}"))
        .unwrap()
        .join();
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..200 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}

fn report(kind: Kind, message: &str) -> Report {
    let mut r = Report::new(kind, mq_library::hlc::now_ms());
    r.message = Some(message.to_owned());
    r
}

/// A relay stand-in: answers every request with `status` (and
/// `Retry-After`, a body) and keeps each request's body.
async fn relay(
    status: u16,
    retry_after: Option<u32>,
    body: &'static str,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let kept = kept.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let Ok(n) = socket.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| {
                                let (name, value) = l.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + length {
                            kept.lock().unwrap().push(text[end + 4..].to_owned());
                            break;
                        }
                    }
                }
                let retry = retry_after
                    .map(|s| format!("retry-after: {s}\r\n"))
                    .unwrap_or_default();
                let answer = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{retry}connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(answer.as_bytes()).await;
            });
        }
    });
    (base, seen)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panic_is_one_file_until_the_drain_takes_it() {
    let _serial = SERIAL.lock().await;
    let dir = scratch();
    let core = open(&dir);
    PREVIOUS_RAN.store(false, Ordering::SeqCst);
    panic_in_a_thread("boom");
    assert!(
        PREVIOUS_RAN.load(Ordering::SeqCst),
        "the hook before ours still runs"
    );
    assert_eq!(pending(&dir).len(), 1, "the hook wrote one file");
    assert!(queue(&core).is_empty(), "the hook never writes the queue");
    core.report_context(device(), String::new());
    until("the drain", || !queue(&core).is_empty()).await;
    let q = queue(&core);
    assert_eq!(q.len(), 1);
    assert_eq!(
        (q[0].kind, q[0].message.as_deref()),
        (Kind::Panic, Some("boom"))
    );
    assert_eq!(q[0].thread.as_deref(), Some("test-panic"));
    assert!(
        q[0].location
            .as_deref()
            .unwrap()
            .contains("tests/reports.rs")
    );
    assert!(pending(&dir).is_empty(), "imported files go");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_pending_folder_holds_at_most_thirty() {
    let _serial = SERIAL.lock().await;
    let dir = scratch();
    let _core = open(&dir);
    for i in 0..35 {
        // Distinct texts: no two merge into one file.
        panic_in_a_thread(Box::leak(
            format!("boom {}", "x".repeat(i)).into_boxed_str(),
        ));
    }
    assert_eq!(pending(&dir).len(), 30);
}

#[tokio::test(flavor = "multi_thread")]
async fn off_deletes_every_waiting_report_and_stops_capture() {
    let _serial = SERIAL.lock().await;
    let dir = scratch();
    let core = open(&dir);
    core.report_context(device(), String::new());
    reports::push(report(Kind::Core, "one"));
    until("the queue", || queue(&core).len() == 1).await;
    panic_in_a_thread("waiting");
    assert_eq!(pending(&dir).len(), 1);
    core.set_reports(false).unwrap();
    assert!(!core.reports_on());
    assert!(queue(&core).is_empty(), "the queue is emptied");
    assert!(pending(&dir).is_empty(), "pending files are deleted");
    panic_in_a_thread("while off");
    // Looked at before any drain, which would delete a file unread.
    assert!(
        pending(&dir).is_empty(),
        "the hook writes nothing while off"
    );
    reports::push(report(Kind::Core, "while off"));
    core.reports_flush();
    assert!(
        pending(&dir).is_empty() && queue(&core).is_empty(),
        "nothing is captured"
    );
    // A relaunch: a panic during open left a file before the switch was read.
    std::fs::create_dir_all(dir.join("reports-pending")).unwrap();
    std::fs::write(
        dir.join("reports-pending/1-aaaa.json"),
        serde_json::to_vec(&report(Kind::Panic, "during open")).unwrap(),
    )
    .unwrap();
    let (base, seen) = relay(200, None, r#"{"accepted":1}"#).await;
    let again = Core::open(CoreConfig {
        data_dir: dir.clone(),
        profile_id: "default".into(),
        device_id: 0,
    })
    .unwrap();
    again.report_context(device(), base);
    until("the file deleted unread", || pending(&dir).is_empty()).await;
    again.reports_upload_now().await;
    assert!(queue(&again).is_empty());
    assert!(seen.lock().unwrap().is_empty(), "no request while off");
    core.set_reports(true).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn incognito_entries_are_stripped_where_they_are_made() {
    let _serial = SERIAL.lock().await;
    let dir = scratch();
    let core = open(&dir);
    core.report_context(device(), String::new());
    // Incognito copies the profile's addon list, so the profile needs one; the row is only copied, never fetched.
    let manifest = serde_json::json!({
        "id": "org.test", "version": "1.0.0", "name": "Test", "types": ["movie"],
        "resources": ["meta"], "catalogs": []
    });
    let row = mq_library::AddonRow {
        transport_url: "http://127.0.0.1:9/manifest.json".into(),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    core.library()
        .set_addons(mq_library::OWNER, &[row], core.clock())
        .unwrap();
    let scope = core.open_incognito().unwrap();
    reports::push(report(Kind::Playback, "a play failed"));
    let mut r = report(Kind::Core, "no row for The Matrix");
    r.context = Some(Context {
        view: Some("playback".into()),
        ..Context::default()
    });
    r.location = Some("crates/a.rs:1".into());
    reports::push(r);
    panic_in_a_thread("panic while in Incognito");
    let file = std::fs::read_to_string(&pending(&dir)[0]).unwrap();
    assert!(!file.contains("panic while in Incognito"), "{file}");
    until("the drain", || queue(&core).len() == 2).await;
    let q = queue(&core);
    assert!(
        q.iter().all(|e| e.kind != Kind::Playback),
        "no playback entry in Incognito"
    );
    for e in &q {
        assert_eq!((&e.message, &e.context), (&None, &None), "{e:?}");
        assert!(e.log.is_empty() && e.attempts.is_empty());
    }
    let text = core
        .library()
        .device_value("reports.queue")
        .unwrap()
        .unwrap();
    assert!(
        !text.contains("Matrix") && !text.contains("Incognito"),
        "{text}"
    );
    // An app's crash read after the fact, flagged Incognito by the app.
    assert!(scope.end_incognito());
    reports::push_from_app(report(Kind::Crash, "jvm text"), true);
    core.reports_flush();
    assert!(queue(&core).iter().all(|e| e.message.is_none()));
}

#[tokio::test(flavor = "multi_thread")]
async fn repeats_merge_and_crashes_push_out_playback() {
    let _serial = SERIAL.lock().await;
    let dir = scratch();
    let core = open(&dir);
    core.report_context(device(), String::new());
    reports::push(report(Kind::Core, "read stalled 4000 ms"));
    reports::push(report(Kind::Core, "read stalled 4012 ms"));
    core.reports_flush();
    let q = queue(&core);
    assert_eq!((q.len(), q[0].count), (1, 2), "digits fold into one entry");
    for i in 0..30 {
        let mut r = Report::new(Kind::Playback, mq_library::hlc::now_ms());
        r.attempts = vec![mq_runtime::reports::Attempt {
            status: Some(400 + i),
            ..Default::default()
        }];
        reports::push(r);
        if i % 10 == 9 {
            core.reports_flush();
        }
    }
    assert_eq!(queue(&core).len(), 30, "the cap");
    reports::push_from_app(report(Kind::Crash, "a crash"), false);
    core.reports_flush();
    let q = queue(&core);
    assert_eq!(q.len(), 30);
    assert!(
        q.iter().any(|e| e.kind == Kind::Crash),
        "a crash pushes out the oldest"
    );
    assert!(q[0].kind != Kind::Core, "the oldest non-crash entry went");
}

#[tokio::test(flavor = "multi_thread")]
async fn every_entry_path_reaches_the_relay_scrubbed() {
    use tracing_subscriber::prelude::*;
    let _serial = SERIAL.lock().await;
    let dir = scratch();
    let core = open(&dir);
    let (base, seen) = relay(200, None, r#"{"accepted":4}"#).await;
    core.report_context(device(), base);
    let secret =
        format!("https://torrentio.strem.fun/realdebrid={KEY}/stream/movie/tt0111161.json");
    reports::push(report(Kind::Core, &format!("core {secret}")));
    reports::push_from_app(report(Kind::Crash, &format!("app {secret}")), false);
    panic_in_a_thread(Box::leak(format!("panic {secret}").into_boxed_str()));
    let subscriber = tracing_subscriber::registry().with(reports::layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!(target: "mq_runtime::test", "layer {secret}");
        // A third party's site changing is not ours to report.
        tracing::error!(target: "mq_turkcealtyazi::site", "site changed");
    });
    core.reports_upload_now().await;
    let bodies = seen.lock().unwrap().join("\n");
    assert!(!bodies.is_empty(), "one request");
    assert!(!bodies.contains(KEY), "{bodies}");
    assert!(!bodies.contains("tt0111161"), "{bodies}");
    for word in ["core <url>", "app <url>", "panic <url>", "layer <url>"] {
        assert!(bodies.contains(word), "{word} in {bodies}");
    }
    assert!(!bodies.contains("site changed"), "{bodies}");
    assert!(queue(&core).is_empty(), "a 200 empties the queue");
}

#[tokio::test(flavor = "multi_thread")]
async fn each_answer_keeps_or_drops_the_queue() {
    let _serial = SERIAL.lock().await;
    let next = |core: &Core| -> u64 {
        core.library()
            .device_value("reports.next")
            .unwrap()
            .unwrap()
            .parse()
            .unwrap()
    };
    let now = mq_library::hlc::now_ms;
    for (status, retry, body, kept, wait_ms) in [
        (400, None, r#"{"code":"badReport"}"#, false, None),
        (413, None, "", false, None),
        (
            400,
            None,
            r#"{"code":"badVersion"}"#,
            true,
            Some(24 * 3_600_000u64),
        ),
        (429, Some(120), r#"{"code":"busy"}"#, true, Some(120_000)),
        (
            503,
            Some(86_400),
            r#"{"code":"reportsFull"}"#,
            true,
            Some(24 * 3_600_000),
        ),
        (404, None, "", true, Some(24 * 3_600_000)),
        (500, None, "", true, Some(60_000)),
    ] {
        let dir = scratch();
        let core = open(&dir);
        let (base, seen) = relay(status, retry, body).await;
        core.report_context(device(), base);
        reports::push(report(Kind::Core, "x"));
        let before = now();
        core.reports_upload_now().await;
        assert_eq!(seen.lock().unwrap().len(), 1, "{status}");
        assert_eq!(!queue(&core).is_empty(), kept, "{status} {body}");
        if let Some(wait) = wait_ms {
            let at = next(&core);
            assert!(
                at >= before + wait && at <= now() + wait,
                "{status}: next {at}"
            );
            // The wait holds: no second request before it.
            core.reports_upload_now().await;
            assert_eq!(seen.lock().unwrap().len(), 1, "{status} waited");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_open_without_a_context_sends_nothing() {
    let _serial = SERIAL.lock().await;
    let dir = scratch();
    let core = open(&dir);
    reports::push(report(Kind::Core, "x"));
    core.reports_upload_now().await;
    core.reports_flush();
    assert!(
        queue(&core).is_empty(),
        "no drain and no upload without reportContext"
    );
}

/// The layer never touches the library: `error!` events from many threads
/// while drains write the queue finish, and none is lost to a lock.
#[tokio::test(flavor = "multi_thread")]
async fn error_events_during_queue_writes_never_block() {
    use tracing_subscriber::prelude::*;
    let _serial = SERIAL.lock().await;
    let dir = scratch();
    let core = open(&dir);
    core.report_context(device(), String::new());
    let writer = {
        let core = core.clone();
        std::thread::spawn(move || {
            for _ in 0..200 {
                core.reports_flush();
            }
        })
    };
    let subscriber = tracing_subscriber::registry().with(reports::layer());
    tracing::subscriber::with_default(subscriber, || {
        for i in 0..5 {
            tracing::error!(target: "mq_runtime::test", "event kind {}", ["a", "b", "c", "d", "e"][i]);
        }
    });
    writer.join().unwrap();
    core.reports_flush();
    assert_eq!(queue(&core).len(), 5, "{:?}", queue(&core));
}
