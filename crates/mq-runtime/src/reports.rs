//! Error and crash reports on the device (plans/error-reports.md 5, 6.1).
//!
//! Entry points never write SQLite: the library's writer mutex is not
//! reentrant and events are logged while it is held. The `tracing` layer,
//! the bridge's failure path, `fail_over` and the apps' `report` op push
//! raw entries onto a small list in memory (`push`); the native panic hook
//! writes one file into `reports-pending/` (the process may be about to
//! end). One task per open (`run`), started by the app's `reportContext`,
//! drains both into the `reports.queue` key of the device's own library and
//! uploads. The uploader is the one place on the device that scrubs
//! (`Batch::build` cleans every report); the relay scrubs again.
//!
//! Incognito entries are stripped where they are made, before they are
//! kept anywhere, and Incognito makes no `playback` entry at all.

use crate::Core;
use mq_http::{HttpError, RequestOptions};
use mq_library::Library;
pub use mq_reports::{App, Attempt, Channel, Context, CoreBuild, Device, Form, Kind, Report};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context as LayerContext, Layer};

const OFF_KEY: &str = "reports.off";
const QUEUE_KEY: &str = "reports.queue";
const NEXT_KEY: &str = "reports.next";
/// Entries held in memory, in the queue, and as pending files.
const MAX_ENTRIES: usize = 30;
const MAX_QUEUE_BYTES: usize = 256 * 1024;
const MAX_AGE_MS: u64 = 14 * 24 * 60 * 60 * 1000;
const RING: usize = 30;
const DRAIN_AFTER: Duration = Duration::from_secs(5);
const FIRST_UPLOAD: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const AFTER_CRASH: Duration = Duration::from_secs(60);
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Back-off after a failed upload: 1 min, 5 min, 30 min, then 6 h.
const BACKOFF_MS: [u64; 4] = [60_000, 300_000, 1_800_000, 21_600_000];

/// Entries pushed since the last drain.
static LIST: Mutex<Vec<Report>> = Mutex::new(Vec::new());
/// The last warn and error callsites: `level target file:line`.
static RING_LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// The switch as the drain last read it. It starts false (capture on) until
/// the drain reads `reports.off`; the drain deletes what was captured
/// before when the switch is off (plans/error-reports.md 5.1).
static REPORTS_OFF: AtomicBool = AtomicBool::new(false);
/// The device's Incognito scope count, handed over by the drain's start.
static SCOPES: Mutex<Option<Arc<AtomicUsize>>> = Mutex::new(None);
/// `<data dir>/reports-pending`, set by `install`.
static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);
/// The latest `reportContext`: the device and the relay's URL.
static CONTEXT: Mutex<Option<(Device, String)>> = Mutex::new(None);
static NOTIFY: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);
/// Bumped when the switch empties the queue, so an upload in flight does
/// not remove entries that came after.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static FAILURES: AtomicU32 = AtomicU32::new(0);
/// Held by whatever reads and writes `reports.queue` (drains, an upload's
/// removal, the switch emptying it), so none of them undoes another's write.
/// Never taken by an entry point.
static QUEUE_WRITE: Mutex<()> = Mutex::new(());

fn now_ms() -> u64 {
    mq_library::hlc::now_ms()
}

fn guard<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Installs the panic hook once per process and points it at `data_dir`.
/// Hosts call it at their logging seam, before the core opens; it reads
/// nothing and imports nothing (the drain does, once the core is open).
pub fn install(data_dir: &Path) {
    *guard(&PENDING) = Some(data_dir.join("reports-pending"));
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            capture_panic(info);
            previous(info);
        }));
    });
}

/// The native hook writes only the pending file: never the list (one panic
/// counted once), never SQLite (the panicking thread may hold its lock).
fn capture_panic(info: &std::panic::PanicHookInfo<'_>) {
    if REPORTS_OFF.load(Ordering::SeqCst) {
        return;
    }
    let mut report = Report::new(Kind::Panic, now_ms());
    report.message = Some(match info.payload().downcast_ref::<&str>() {
        Some(s) => (*s).to_owned(),
        None => info
            .payload()
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "a panic with no text".to_owned()),
    });
    report.location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    report.thread = std::thread::current().name().map(str::to_owned);
    if let Ok(ring) = RING_LINES.try_lock() {
        report.log = ring.iter().cloned().collect();
    }
    if in_incognito() {
        report.strip_incognito();
    }
    write_pending(&report);
    NOTIFY.notify_one();
}

fn pending_dir() -> Option<PathBuf> {
    PENDING.try_lock().ok().and_then(|p| p.clone())
}

fn write_pending(report: &Report) {
    let Some(dir) = pending_dir() else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    // A core that never opens cannot pile files up for someone who turned
    // reports off: the folder holds at most `MAX_ENTRIES`.
    let held = std::fs::read_dir(&dir).map_or(0, |d| d.count());
    if held >= MAX_ENTRIES {
        return;
    }
    let Ok(text) = serde_json::to_vec(report) else {
        return;
    };
    let name = format!("{}-{}.json", report.at, &report.merge_key()[..8]);
    let tmp = dir.join(format!(".{name}.tmp"));
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join(name));
    }
}

/// Pending files read and deleted (the switch off: deleted unread).
fn take_pending(read: bool) -> Vec<Report> {
    let Some(dir) = pending_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if read
            && path.extension().is_some_and(|e| e == "json")
            && let Some(report) = std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Report>(&b).ok())
        {
            out.push(report);
        }
        let _ = std::fs::remove_file(&path);
    }
    out
}

/// Any Incognito scope alive on this device. Unknown (the count is held
/// elsewhere this instant) counts as Incognito: the safe side.
fn in_incognito() -> bool {
    match SCOPES.try_lock() {
        Ok(scopes) => scopes
            .as_ref()
            .is_some_and(|s| s.load(Ordering::SeqCst) > 0),
        Err(_) => true,
    }
}

/// An entry from inside the core (the bridge's failure path, `fail_over`,
/// an `error!` event): stripped in Incognito, never a `playback` entry
/// there, then onto the list. Never blocks.
pub fn push(report: Report) {
    push_entry(report, false);
}

/// An entry from an app's `report` op; `incognito` is the app's own word
/// (a JVM crash file written in Incognito), besides the core's.
pub fn push_from_app(report: Report, incognito: bool) {
    push_entry(report, incognito);
}

fn push_entry(mut report: Report, incognito: bool) {
    if REPORTS_OFF.load(Ordering::SeqCst) {
        return;
    }
    if incognito || in_incognito() {
        if report.kind == Kind::Playback {
            return;
        }
        report.strip_incognito();
    }
    // Waits for the list's lock: it is held only for a merge or a drain's
    // `take`, never while anything logs or panics, so this cannot deadlock,
    // and a push never loses its entry to a drain (the panic hook, which
    // may run anywhere, never takes it).
    let mut list = guard(&LIST);
    add(&mut list, report, usize::MAX);
    drop(list);
    NOTIFY.notify_one();
}

/// `report` into `entries`: merged into a repeat, else added within the
/// caps; past a cap a crash kind pushes out the oldest `playback` or
/// `core` entry, anything else is dropped.
fn add(entries: &mut Vec<Report>, report: Report, max_bytes: usize) {
    let key = report.merge_key();
    if let Some(same) = entries.iter_mut().find(|e| e.merge_key() == key) {
        same.count = same.count.saturating_add(report.count);
        same.at = same.at.max(report.at);
        return;
    }
    let crash = report.kind.is_crash();
    entries.push(report);
    while entries.len() > MAX_ENTRIES || bytes(entries) > max_bytes {
        let evict = crash
            .then(|| {
                entries
                    .iter()
                    .position(|e| matches!(e.kind, Kind::Playback | Kind::Core))
            })
            .flatten();
        match evict {
            Some(at) => {
                entries.remove(at);
            }
            None => {
                entries.pop();
            }
        }
    }
}

fn bytes(entries: &[Report]) -> usize {
    serde_json::to_vec(entries).map_or(0, |b| b.len())
}

/// The `tracing` layer the hosts add beside their log layer: it keeps the
/// ring of warn and error callsites (no formatted message) and turns
/// `error!` events from our core crates into `core` entries.
pub fn layer() -> ReportsLayer {
    ReportsLayer
}

pub struct ReportsLayer;

impl<S: Subscriber> Layer<S> for ReportsLayer {
    fn on_event(&self, event: &Event<'_>, _: LayerContext<'_, S>) {
        let meta = event.metadata();
        let level = *meta.level();
        if (level != Level::WARN && level != Level::ERROR)
            || std::thread::panicking()
            || REPORTS_OFF.load(Ordering::SeqCst)
        {
            return;
        }
        let place = format!(
            "{}:{}",
            meta.file().unwrap_or("?"),
            meta.line().unwrap_or(0)
        );
        if let Ok(mut ring) = RING_LINES.try_lock() {
            let word = if level == Level::ERROR {
                "error"
            } else {
                "warn"
            };
            ring.push_back(format!("{word} {} {place}", meta.target()));
            while ring.len() > RING {
                ring.pop_front();
            }
        }
        if level == Level::ERROR && reported_target(meta.target()) {
            let mut message = Message(String::new());
            event.record(&mut message);
            // A download job's panic is already a panic report.
            if meta.target().starts_with("mq_runtime::downloads") && message.0.contains("panicked")
            {
                return;
            }
            let mut report = Report::new(Kind::Core, now_ms());
            report.message = Some(message.0);
            report.location = Some(place);
            push(report);
        }
    }
}

/// `error!` events that are ours to fix: the core crates, not the relay,
/// the command line, or a third-party site changing (`mq_turkcealtyazi`).
fn reported_target(target: &str) -> bool {
    target.starts_with("mq_")
        && !["mq_turkcealtyazi", "mq_sync_server", "mq_cli"]
            .iter()
            .any(|t| target.starts_with(t))
}

struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_owned();
        }
    }
}

impl Core {
    /// The app's `reportContext`: who it is and where reports go. Starts
    /// this open's drain and uploader once. No context, no upload: tests,
    /// the command line and tvOS never send it.
    pub fn report_context(&self, device: Device, server: String) {
        *guard(&CONTEXT) = Some((device, server));
        *guard(&SCOPES) = Some(self.live_scopes().clone());
        if self.start_reports() {
            // The first drain here, before the answer: the switch is read and
            // the pending files are in the queue (or gone, when off) by then.
            let crashed = drain(self.device_library());
            if tokio::runtime::Handle::try_current().is_ok() {
                tokio::spawn(run(self.clone(), crashed));
            }
        }
    }

    /// Settings, This device, Error reports (on by default).
    pub fn reports_on(&self) -> bool {
        !switched_off(self.device_library())
    }

    /// The switch. Off empties the list, the queue and the pending files at
    /// once and stops capture; on lets capture go on.
    pub fn set_reports(&self, on: bool) -> Result<(), crate::CoreError> {
        let library = self.device_library();
        library.set_device_value(OFF_KEY, if on { "0" } else { "1" })?;
        REPORTS_OFF.store(!on, Ordering::SeqCst);
        if !on {
            let _one = guard(&QUEUE_WRITE);
            GENERATION.fetch_add(1, Ordering::SeqCst);
            guard(&LIST).clear();
            take_pending(false);
            library.set_device_value(QUEUE_KEY, "[]")?;
        }
        Ok(())
    }

    /// `lifecycle background`: the list into the queue now, since the app
    /// may not come back.
    pub fn reports_flush(&self) {
        if self.reports_running() {
            drain(self.device_library());
        }
    }

    /// Drain and one upload now, under the same rules as the timer's
    /// (`reports.next` included). Tests; an open without `reportContext`
    /// sends nothing.
    #[doc(hidden)]
    pub async fn reports_upload_now(&self) {
        if self.reports_running() {
            drain(self.device_library());
            upload(self).await;
        }
    }
}

fn switched_off(library: &Library) -> bool {
    library.device_value(OFF_KEY).ok().flatten().as_deref() == Some("1")
}

/// The drain and the uploader for one open (plans/error-reports.md 5.2,
/// 5.3): the list 5 s after a push, at each upload and on `background`;
/// uploads 30 s after start, every 6 hours, and 60 s after a crash kind,
/// never before `reports.next`.
async fn run(core: Core, mut crashed: bool) {
    let mut next = std::time::Instant::now() + FIRST_UPLOAD;
    loop {
        if crashed {
            next = next.min(std::time::Instant::now() + AFTER_CRASH);
        }
        tokio::select! {
            () = NOTIFY.notified() => {
                tokio::time::sleep(DRAIN_AFTER).await;
                crashed = drain(core.device_library());
            }
            () = tokio::time::sleep_until(next.into()) => {
                drain(core.device_library());
                upload(&core).await;
                crashed = false;
                next = std::time::Instant::now() + EVERY;
            }
        }
    }
}

/// The list and the pending files into `reports.queue`. Returns whether a
/// crash kind came in. With the switch off, both are deleted unread.
fn drain(library: &Library) -> bool {
    // One drain at a time (the task's, `background`'s, an upload's removal):
    // each reads and writes the whole queue.
    let _one = guard(&QUEUE_WRITE);
    let off = switched_off(library);
    REPORTS_OFF.store(off, Ordering::SeqCst);
    let mut entries = std::mem::take(&mut *guard(&LIST));
    entries.extend(take_pending(!off));
    if off || entries.is_empty() {
        return false;
    }
    let crashed = entries.iter().any(|e| e.kind.is_crash());
    let mut queue = read_queue(library);
    for entry in entries {
        add(&mut queue, entry, MAX_QUEUE_BYTES);
    }
    let oldest = now_ms().saturating_sub(MAX_AGE_MS);
    queue.retain(|e| e.at >= oldest);
    write_queue(library, &queue);
    crashed
}

fn read_queue(library: &Library) -> Vec<Report> {
    library
        .device_value(QUEUE_KEY)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_queue(library: &Library, queue: &[Report]) {
    let text = serde_json::to_string(queue).unwrap_or_else(|_| "[]".to_owned());
    if let Err(e) = library.set_device_value(QUEUE_KEY, &text) {
        tracing::debug!("reports queue: {e}");
    }
}

fn wait_until(library: &Library, ms: u64) {
    let _ = library.set_device_value(NEXT_KEY, &ms.to_string());
}

/// One batch to `<server>/v1/reports`, cleaned on the way (the one
/// client-side scrub), with the answer rules of plans/error-reports.md 5.3.
async fn upload(core: &Core) {
    let library = core.device_library();
    let Some((device, server)) = guard(&CONTEXT).clone() else {
        return;
    };
    if server.is_empty() || switched_off(library) {
        return;
    }
    let now = now_ms();
    let next: u64 = library
        .device_value(NEXT_KEY)
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if now < next {
        return;
    }
    let queue = read_queue(library);
    if queue.is_empty() {
        return;
    }
    let generation = GENERATION.load(Ordering::SeqCst);
    let (batch, taken) = mq_reports::Batch::build(device, &queue);
    let remove = |count: usize| {
        let _one = guard(&QUEUE_WRITE);
        if GENERATION.load(Ordering::SeqCst) == generation {
            let mut queue = read_queue(library);
            queue.drain(..count.min(queue.len()));
            write_queue(library, &queue);
        }
    };
    if taken == 0 {
        // One entry that cannot fit a batch alone never will.
        remove(1);
        return;
    }
    let url = format!("{}/v1/reports", server.trim_end_matches('/'));
    match core.http().post_json(&url, &batch, &options(&server)).await {
        Ok(_) => {
            FAILURES.store(0, Ordering::SeqCst);
            remove(taken);
        }
        Err(HttpError::Status {
            status: 400, body, ..
        }) if serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["code"].as_str().map(str::to_owned))
            .as_deref()
            == Some("badVersion") =>
        {
            wait_until(library, now + DAY_MS);
        }
        Err(HttpError::Status {
            status: 400 | 413, ..
        }) => remove(taken),
        Err(HttpError::Status {
            status: 429 | 503,
            retry_after,
            ..
        }) => {
            let wait = retry_after.map_or(60 * 60 * 1000, |d| d.as_millis() as u64);
            wait_until(library, now + wait.min(DAY_MS));
        }
        Err(HttpError::Status {
            status: 404 | 405, ..
        }) => wait_until(library, now + DAY_MS),
        Err(_) => {
            let failures = FAILURES.fetch_add(1, Ordering::SeqCst) as usize;
            wait_until(
                library,
                now + BACKOFF_MS[failures.min(BACKOFF_MS.len() - 1)],
            );
        }
    }
}

fn options(server: &str) -> RequestOptions {
    let allow_private = url::Url::parse(server)
        .ok()
        .and_then(|u| u.host_str().map(mq_http::is_private_host))
        .unwrap_or(false);
    RequestOptions {
        timeout: Duration::from_secs(30),
        max_bytes: 64 * 1024,
        allow_private,
        ..Default::default()
    }
}

/// Debug builds' dev hooks only: a real panic in a core task, so a walk
/// can check the whole path (plans/error-reports.md 13).
#[cfg(debug_assertions)]
pub fn debug_panic() {
    std::thread::Builder::new()
        .name("debug-panic".into())
        .spawn(|| panic!("debugPanic: a test panic from the dev hooks"))
        .ok()
        .map(std::thread::JoinHandle::join);
}
