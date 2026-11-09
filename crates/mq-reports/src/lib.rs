//! Error and crash reports (plans/error-reports.md): the shape a device
//! sends to the relay's `POST /v1/reports`, the scrubber, and the checks.
//! The core's uploader runs `Report::clean` on every report it sends (the
//! one client-side choke point) and the relay runs `parse_batch`, which
//! cleans again, before it stores anything.
//!
//! A report holds no identifier of any kind: no install id, account,
//! device id or address (plans/error-reports.md 3.2).

mod scrub;

pub use scrub::{TOKEN_FLOOR, scrub};

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

/// The shape's version. A new field comes with a new version, and the relay
/// that takes it deploys before any app that sends it (5.3).
pub const VERSION: u32 = 1;
pub const MAX_REPORTS: usize = 20;
pub const MAX_BATCH_BYTES: usize = 64 * 1024;
pub const MAX_REPORT_BYTES: usize = 16 * 1024;
const MAX_MESSAGE: usize = 512;
const MAX_WHERE: usize = 200;
const MAX_LINE: usize = 200;
const MAX_STACK: usize = 48;
const MAX_LOG: usize = 30;
const MAX_ATTEMPTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Panic,
    Crash,
    Anr,
    Oom,
    Js,
    Core,
    Playback,
}

impl Kind {
    /// The kinds that push out a queued `playback` or `core` report when the
    /// queue is full (5.2), and that bring the next upload forward.
    pub fn is_crash(self) -> bool {
        matches!(self, Kind::Panic | Kind::Crash | Kind::Anr | Kind::Oom)
    }

    fn word(self) -> &'static str {
        match self {
            Kind::Panic => "panic",
            Kind::Crash => "crash",
            Kind::Anr => "anr",
            Kind::Oom => "oom",
            Kind::Js => "js",
            Kind::Core => "core",
            Kind::Playback => "playback",
        }
    }
}

/// Where in the protocol a `core` report came from: names from the
/// protocol's own lists, never free text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
    /// The `CoreError` variant (`Library`, `Io`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

/// One source a failed play tried: classes and numbers, never a name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attempt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub kind: Kind,
    /// The device's time; data only (a TV box may boot with a wrong clock).
    pub at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// A panic's `Location` or an event's `file:line`, fixed at compile time.
    #[serde(rename = "where", default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<Attempt>,
    /// The last warn and error callsites: `level target file:line`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub log: Vec<String>,
}

fn one() -> u32 {
    1
}

impl Report {
    pub fn new(kind: Kind, at: u64) -> Self {
        Self {
            kind,
            at,
            group: String::new(),
            count: 1,
            message: None,
            location: None,
            thread: None,
            stack: Vec::new(),
            context: None,
            attempts: Vec::new(),
            log: Vec::new(),
        }
    }

    /// What an Incognito report keeps (3.1): kind, where, stack, thread and
    /// count. Run at the entry point, before the report is kept anywhere.
    pub fn strip_incognito(&mut self) {
        self.message = None;
        self.context = None;
        self.attempts.clear();
        self.log.clear();
    }

    /// The key the device's queue merges repeats by: kind, where (or the
    /// top three frames) and the raw message with digits folded, so
    /// `stalled 4000 ms` and `stalled 4012 ms` are one entry.
    pub fn merge_key(&self) -> String {
        digest(&[
            self.kind.word(),
            &self.place(),
            &fold_digits(self.message.as_deref().unwrap_or("")),
        ])
    }

    /// Where it failed: the location, else the top three frames, else (a
    /// `playback` or bridge report) its context and attempt classes.
    fn place(&self) -> String {
        match &self.location {
            Some(at) => at.clone(),
            None if !self.stack.is_empty() => self
                .stack
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
            None => serde_json::to_string(&(&self.context, &self.attempts)).unwrap_or_default(),
        }
    }

    /// Scrubs and checks every field, and sets `group` (computed over the
    /// scrubbed message). What fails a check is dropped, never sent raw.
    pub fn clean(&mut self, app: App) {
        self.message = self
            .message
            .take()
            .map(|m| cap(&scrub(&m), MAX_MESSAGE))
            .filter(|m| !m.is_empty());
        self.location = self.location.take().filter(|w| where_ok(w));
        self.thread = self.thread.take().filter(|t| thread_ok(t));
        self.stack.truncate(MAX_STACK);
        for line in &mut self.stack {
            *line = cap(&scrub(line), MAX_LINE);
        }
        self.log.retain(|l| log_ok(l));
        self.log.truncate(MAX_LOG);
        if let Some(c) = &mut self.context {
            for field in [&mut c.op, &mut c.view, &mut c.failure_kind, &mut c.class] {
                *field = field.take().filter(|v| name_ok(v));
            }
        }
        self.attempts.truncate(MAX_ATTEMPTS);
        for a in &mut self.attempts {
            for field in [
                &mut a.source,
                &mut a.store,
                &mut a.failure,
                &mut a.resolution,
                &mut a.codec,
                &mut a.hdr,
                &mut a.audio,
            ] {
                *field = field.take().filter(|v| value_ok(v));
            }
        }
        self.count = self.count.max(1);
        // Too large: frames go first, from the bottom.
        while serde_json::to_vec(self).map_or(0, |b| b.len()) > MAX_REPORT_BYTES
            && self.stack.pop().is_some()
        {}
        self.group = group(
            self.kind,
            app,
            &self.place(),
            self.message.as_deref().unwrap_or(""),
        );
    }
}

/// 16 hex of SHA-256 over kind, app, where (or top frames) and the scrubbed
/// message with digits folded.
pub fn group(kind: Kind, app: App, place: &str, scrubbed: &str) -> String {
    digest(&[kind.word(), app.word(), place, &fold_digits(scrubbed)])
}

fn digest(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hash.finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn fold_digits(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

fn cap(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => text[..at].to_owned(),
        None => text.to_owned(),
    }
}

static WHERE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\w./~@+-]+:\d+(:\d+)?$").expect("a valid pattern"));

/// A compile-time location: `crates/...`, `~/.cargo/...` or `/rustc/...`.
/// Anything with a leading `/` else (a build without the path remap carries
/// the builder's home folder) is refused, and `where` is never scrubbed.
pub fn where_ok(at: &str) -> bool {
    at.len() <= MAX_WHERE
        && WHERE.is_match(at)
        && (!at.starts_with('/') || at.starts_with("/rustc/"))
}

/// `warn mq_server::ahead crates/mq-server/src/ahead.rs:212`.
pub fn log_ok(line: &str) -> bool {
    let mut parts = line.split(' ');
    let (Some(level), Some(target), Some(at), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    matches!(level, "warn" | "error")
        && !target.is_empty()
        && target.len() <= 80
        && target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
        && where_ok(at)
}

/// A protocol name: `subscribe`, `playback`, `Library`.
fn name_ok(v: &str) -> bool {
    (1..=40).contains(&v.len())
        && v.starts_with(|c: char| c.is_ascii_alphabetic())
        && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// An attempt's class: `debrid`, `realdebrid`, `2160p`, `hevc`, `eac3`.
fn value_ok(v: &str) -> bool {
    (1..=24).contains(&v.len())
        && v.starts_with(|c: char| c.is_ascii_alphanumeric())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
}

fn thread_ok(v: &str) -> bool {
    (1..=40).contains(&v.len())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || " ._:-#".contains(c))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum App {
    Android,
    Ios,
    Macos,
    Web,
    Webos,
}

impl App {
    fn word(self) -> &'static str {
        match self {
            App::Android => "android",
            App::Ios => "ios",
            App::Macos => "macos",
            App::Web => "web",
            App::Webos => "webos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    Release,
    Debug,
    Walk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Form {
    Phone,
    Tablet,
    Tv,
    Desktop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoreBuild {
    Own,
    Store,
}

/// What is the same for every report from one device: versions and the
/// device model, never its user-given name and never an identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Device {
    pub app: App,
    pub channel: Channel,
    pub version: String,
    #[serde(default)]
    pub build: String,
    #[serde(default)]
    pub core: String,
    pub os: String,
    pub model: String,
    #[serde(default)]
    pub abi: String,
    pub form: Form,
    pub core_build: CoreBuild,
}

impl Device {
    pub fn check(&self) -> Result<(), &'static str> {
        let fits = |v: &str, min: usize, max: usize, extra: &str| {
            (min..=max).contains(&v.len())
                && v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
        };
        let checks = [
            (fits(&self.version, 1, 32, "._+-"), "version"),
            (fits(&self.build, 0, 40, "._-"), "build"),
            (fits(&self.core, 0, 40, "._-"), "core"),
            (fits(&self.os, 1, 40, " ._-"), "os"),
            (fits(&self.model, 1, 60, " ,._()+-"), "model"),
            (fits(&self.abi, 0, 20, "_-"), "abi"),
        ];
        match checks.iter().find(|(ok, _)| !ok) {
            Some((_, field)) => Err(*field),
            None => Ok(()),
        }
    }
}

/// The body of `POST /v1/reports`: `v`, the `Device` fields, `reports`.
/// (serde's `flatten` cannot be combined with `deny_unknown_fields`, so
/// the device travels as its own object inside the body: `device`.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Batch {
    pub v: u32,
    pub device: Device,
    pub reports: Vec<Report>,
}

impl Batch {
    /// Up to `MAX_REPORTS` of `reports`, each cleaned, and as many as fit
    /// in `MAX_BATCH_BYTES`; returns the batch and how many it took.
    pub fn build(device: Device, reports: &[Report]) -> (Batch, usize) {
        let mut batch = Batch {
            v: VERSION,
            device,
            reports: Vec::new(),
        };
        for report in reports.iter().take(MAX_REPORTS) {
            let mut report = report.clone();
            report.clean(batch.device.app);
            batch.reports.push(report);
            if serde_json::to_vec(&batch).map_or(0, |b| b.len()) > MAX_BATCH_BYTES {
                batch.reports.pop();
                break;
            }
        }
        let taken = batch.reports.len();
        (batch, taken)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Another shape version: the device keeps its queue and waits.
    BadVersion,
    /// The batch will never pass: the device drops it.
    BadReport(&'static str),
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Refusal::BadVersion => "badVersion",
            Refusal::BadReport(_) => "badReport",
        }
    }
}

/// The relay's reading of a body: version, shape, limits, then every report
/// cleaned again (the scrub is idempotent, so a clean batch is unchanged).
pub fn parse_batch(body: &[u8]) -> Result<Batch, Refusal> {
    if body.len() > MAX_BATCH_BYTES {
        return Err(Refusal::BadReport("too large"));
    }
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| Refusal::BadReport("not json"))?;
    if value.get("v").and_then(|v| v.as_u64()) != Some(u64::from(VERSION)) {
        return Err(Refusal::BadVersion);
    }
    let mut batch: Batch =
        serde_json::from_value(value).map_err(|_| Refusal::BadReport("shape"))?;
    if batch.reports.len() > MAX_REPORTS {
        return Err(Refusal::BadReport("too many reports"));
    }
    batch.device.check().map_err(Refusal::BadReport)?;
    let app = batch.device.app;
    for report in &mut batch.reports {
        report.clean(app);
    }
    Ok(batch)
}

#[cfg(test)]
mod tests;
