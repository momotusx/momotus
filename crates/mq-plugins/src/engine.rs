//! One plugin call (plans/plugins-and-marketplace.md 6.2): its own thread and
//! current-thread runtime, a fresh QuickJS-NG runtime with a memory and a
//! stack limit, an interrupt that reads the thread's CPU time, the wall
//! deadline and the cancel flag, and the host reached only through
//! `HostCall` and `HostReply` messages: the engine thread holds no client and
//! no key. Both are plain data, so the engine can move to another process
//! with the same messages (6.2).
//!
//! The script runs as Nuvio runs it: wrapped in a function after
//! `js/prelude.js` has made its world, then `getStreams(tmdbId, mediaType,
//! season, episode, info)` from `module.exports` or the global object, its
//! answer read by `streams`.

use crate::natives;
use crate::streams::Reader;
use rquickjs::promise::MaybePromise;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, CaughtError, Ctx, Function, Object, Value,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The JS heap (`set_memory_limit`).
    pub memory: usize,
    /// The JS stack (`set_max_stack_size`); the thread's own stack is `THREAD_STACK`.
    pub stack: usize,
    /// The thread's CPU time.
    pub cpu: Duration,
    pub wall: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory: 24 * 1024 * 1024,
            stack: 512 * 1024,
            cpu: Duration::from_secs(3),
            wall: Duration::from_secs(12),
        }
    }
}

/// The engine thread's stack: host functions run on it too.
const THREAD_STACK: usize = 4 * 1024 * 1024;

/// What one call runs: the plugin's code and Nuvio's `getStreams` arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    /// The script as downloaded.
    pub script: String,
    /// `SCRAPER_ID`: the manifest entry's id.
    pub plugin_id: String,
    pub tmdb_id: String,
    pub media: Media,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// Our additive fifth argument.
    pub info: Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Media {
    Movie,
    Tv,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Info {
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<u32>,
}

/// What a plugin asks of the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostCall {
    /// Answered by `HostReply::Response` or `HostReply::Failed`.
    Fetch(FetchRequest),
    /// One `console` line, within the call's 4 KiB; needs no answer (the
    /// reply's receiver is dropped).
    Log { level: LogLevel, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRequest {
    pub method: Method,
    /// As the plugin gave it, at most 8 KiB; the host parses it and applies
    /// 6.4's host rule.
    pub url: String,
    /// The plugin's headers, in order and unfiltered (the host's filter runs
    /// on its final map), lowercase names; at most 32, each pair at most 8 KiB.
    pub headers: Vec<(String, String)>,
    /// At most 256 KiB (the host refuses over 16 KiB under any-site). Its
    /// `Content-Type`, if any, is in `headers`.
    pub body: Option<Vec<u8>>,
    /// False for `redirect: "manual"`: a 3xx is answered as itself.
    pub follow: bool,
    /// The decoded body's cap: `min(4 MiB, the call's remaining host bytes)`.
    pub max_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    Get,
    Post,
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// The host's answer to one `HostCall::Fetch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostReply {
    /// Any status, non-2xx included (`ok: false`).
    Response(FetchResponse),
    /// The promise rejects with a `TypeError` of these words ("rate
    /// limited", "network not ready", "could not reach {host}", "not
    /// allowed: {host}").
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    /// The final URL after redirects.
    pub url: String,
    /// Lowercase names; at most 64, each at most 8 KiB.
    pub headers: Vec<(String, String)>,
    /// Decoded, at most the request's `max_bytes`.
    pub body: Vec<u8>,
}

/// One call on the channel and where its reply goes.
#[derive(Debug)]
pub struct HostRequest {
    pub call: HostCall,
    pub reply: oneshot::Sender<HostReply>,
}

/// One stream as the plugin returned it (`streams`). Nothing is sanitized or
/// checked as a URL yet: the host's adapter does that (6.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginStream {
    /// `url`, or `url.url`; at most 8 KiB, else the stream is dropped.
    pub url: String,
    /// Strings or numbers, each at most 1 KiB.
    pub name: Option<String>,
    pub title: Option<String>,
    pub quality: Option<String>,
    pub size: Option<String>,
    pub language: Option<String>,
    /// String values only; at most 16 pairs of 1 KiB.
    pub headers: Vec<(String, String)>,
    /// At most 16.
    pub subtitles: Vec<PluginSubtitle>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSubtitle {
    pub url: String,
    /// `language`, as Nuvio reads it.
    pub lang: Option<String>,
}

/// Requests one call may make (6.4); past it `fetch` rejects at once and the
/// host never hears of it.
pub const MAX_REQUESTS: u32 = 30;

/// How a call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// At most 100 streams, 256 KiB in all.
    Done(Vec<PluginStream>),
    /// The script threw, or no `getStreams` was found: at most 1 KiB of words.
    Threw(String),
    Memory,
    Cpu,
    Wall,
    Cancelled,
}

const NONE: u8 = 0;
const CPU: u8 = 1;
const WALL: u8 = 2;
const CANCEL: u8 = 3;

/// The call's budget, read by the interrupt and by host work (the HTML
/// parse) between its steps.
#[derive(Clone)]
struct Budget {
    cancel: Arc<AtomicBool>,
    limits: Limits,
    started: Instant,
    cpu_start: Option<Duration>,
}

impl Budget {
    /// Why the call must stop now, or NONE.
    fn reason(&self) -> u8 {
        if self.cancel.load(Ordering::Relaxed) {
            CANCEL
        } else if self
            .cpu_start
            .zip(thread_cpu())
            .is_none_or(|(start, now)| now.saturating_sub(start) > self.limits.cpu)
        {
            CPU
        } else if self.started.elapsed() > self.limits.wall {
            WALL
        } else {
            NONE
        }
    }
}

thread_local! {
    /// The host of this thread's one call.
    static HOST: RefCell<Option<mpsc::UnboundedSender<HostRequest>>> = const { RefCell::new(None) };
    static BUDGET: RefCell<Option<Budget>> = const { RefCell::new(None) };
}

/// Sends one request to this thread's host.
pub(crate) fn host(request: HostRequest) -> Result<(), ()> {
    HOST.with_borrow(|h| match h {
        Some(host) => host.send(request).map_err(drop),
        None => Err(()),
    })
}

/// Whether this thread's call is out of budget (CPU, wall or cancel).
pub(crate) fn spent() -> bool {
    BUDGET.with_borrow(|b| b.as_ref().is_some_and(|b| b.reason() != NONE))
}

/// Runs the call on a new thread; the handle ends when the thread does.
/// Fails only when the thread cannot be made.
pub fn call(
    call: Call,
    limits: Limits,
    cancel: Arc<AtomicBool>,
    host: mpsc::UnboundedSender<HostRequest>,
) -> std::io::Result<std::thread::JoinHandle<Outcome>> {
    std::thread::Builder::new()
        .name("mq-plugin".into())
        .stack_size(THREAD_STACK)
        .spawn(move || {
            HOST.with(|h| *h.borrow_mut() = Some(host));
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            else {
                return Outcome::Memory;
            };
            rt.block_on(run(call, limits, cancel))
        })
}

async fn run(call: Call, limits: Limits, cancel: Arc<AtomicBool>) -> Outcome {
    let why = Arc::new(AtomicU8::new(NONE));
    let Ok(qjs) = AsyncRuntime::new() else {
        return Outcome::Memory;
    };
    qjs.set_memory_limit(limits.memory).await;
    qjs.set_max_stack_size(limits.stack).await;
    let budget = Budget {
        cancel: cancel.clone(),
        limits,
        started: Instant::now(),
        cpu_start: thread_cpu(),
    };
    BUDGET.with(|b| *b.borrow_mut() = Some(budget.clone()));
    let said = why.clone();
    qjs.set_interrupt_handler(Some(Box::new(move || {
        let reason = budget.reason();
        if reason == NONE {
            return false;
        }
        said.store(reason, Ordering::Relaxed);
        true
    })))
    .await;
    let Ok(ctx) = AsyncContext::full(&qjs).await else {
        return Outcome::Memory;
    };
    let work = ctx.async_with(async |ctx| evaluate(&ctx, &call).await);
    let cancelled = async {
        while !cancel.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    // The work first: an answer that is ready wins over a timer that is too.
    let ended = tokio::select! {
        biased;
        result = work => Some(result),
        _ = tokio::time::sleep(limits.wall) => None,
        _ = cancelled => None,
    };
    // What the interrupt said comes before anything the script made of it.
    match (ended, why.load(Ordering::Relaxed)) {
        (_, CANCEL) => Outcome::Cancelled,
        (_, CPU) => Outcome::Cpu,
        (_, WALL) => Outcome::Wall,
        (None, _) if cancel.load(Ordering::Relaxed) => Outcome::Cancelled,
        (None, _) => Outcome::Wall,
        (Some(Ok(streams)), _) => Outcome::Done(streams),
        (Some(Err(words)), _) if words == OUT_OF_MEMORY => Outcome::Memory,
        (Some(Err(words)), _) => Outcome::Threw(cut(words, MAX_WORDS)),
    }
}

const OUT_OF_MEMORY: &str = "out of memory";

/// What `Threw` keeps of the script's words.
const MAX_WORDS: usize = 1024;

/// The longest URL `fetch` takes, in UTF-16 units as JS counts them.
pub const MAX_URL: usize = 8 * 1024;

/// An array or array-like longer than this makes every `Array.prototype`
/// method throw (`js/prelude.js`); no heap of 24 MiB holds one this long.
const MAX_ARRAY: u64 = 1 << 24;

const PRELUDE: &str = include_str!("../js/prelude.js");

/// `words` cut to at most `max` bytes, at a char boundary.
fn cut(mut words: String, max: usize) -> String {
    if words.len() > max {
        let mut at = max;
        while !words.is_char_boundary(at) {
            at -= 1;
        }
        words.truncate(at);
    }
    words
}

/// The prelude, the script, then `getStreams` awaited and its streams read;
/// or the words of what threw.
async fn evaluate<'js>(ctx: &Ctx<'js>, call: &Call) -> Result<Vec<PluginStream>, String> {
    let words = |e: CaughtError<'js>| match e {
        CaughtError::Exception(e) => cut(e.message().unwrap_or_default(), MAX_WORDS),
        // At its heap limit QuickJS-NG cannot build its own "out of memory"
        // error either, and the call throws `null` (seen 2026-10-27: an
        // object bomb under a 24 MiB limit ended as a thrown null). A
        // plugin's own `throw null` reads as memory too, which is harmless.
        CaughtError::Value(v) if v.is_null() => OUT_OF_MEMORY.into(),
        CaughtError::Value(v) => cut(format!("{v:?}"), MAX_WORDS),
        CaughtError::Error(rquickjs::Error::Allocation) => OUT_OF_MEMORY.into(),
        CaughtError::Error(e) => e.to_string(),
    };
    let globals = ctx.globals();
    // What a plugin never has (6.2): shared memory, which could park the
    // thread inside one native call, and WebAssembly.
    for name in ["SharedArrayBuffer", "Atomics", "WebAssembly"] {
        globals.remove(name).catch(ctx).map_err(words)?;
    }
    // Made before the plugin runs, so nothing it changes reaches the reads.
    let mut reader = Reader::new(ctx.clone()).catch(ctx).map_err(words)?;
    let config = Object::new(ctx.clone()).catch(ctx).map_err(words)?;
    config
        .set("pluginId", call.plugin_id.as_str())
        .catch(ctx)
        .map_err(words)?;
    config
        .set("maxArray", MAX_ARRAY as f64)
        .catch(ctx)
        .map_err(words)?;
    config
        .set("maxUrl", MAX_URL as f64)
        .catch(ctx)
        .map_err(words)?;
    let natives = natives::natives(ctx).catch(ctx).map_err(words)?;
    let prelude: Function = ctx.eval(PRELUDE).catch(ctx).map_err(words)?;
    prelude
        .call::<_, ()>((natives, config))
        .catch(ctx)
        .map_err(words)?;
    // Nuvio's wrapper: the script's own declarations stay in its function.
    let wrapped = format!("(function () {{\n{}\n}})();", call.script);
    ctx.eval::<(), _>(wrapped).catch(ctx).map_err(words)?;
    let entry = entry(ctx).catch(ctx).map_err(words)?;
    let Some(entry) = entry else {
        return Err("no getStreams".into());
    };
    let info = Object::new(ctx.clone()).catch(ctx).map_err(words)?;
    info.set("imdbId", call.info.imdb_id.as_deref())
        .catch(ctx)
        .map_err(words)?;
    info.set("title", call.info.title.as_str())
        .catch(ctx)
        .map_err(words)?;
    info.set("year", call.info.year).catch(ctx).map_err(words)?;
    let media = match call.media {
        Media::Movie => "movie",
        Media::Tv => "tv",
    };
    let answer: MaybePromise = entry
        .call((
            call.tmdb_id.as_str(),
            media,
            call.season,
            call.episode,
            info,
        ))
        .catch(ctx)
        .map_err(words)?;
    let value: Value = answer.into_future().await.catch(ctx).map_err(words)?;
    Ok(reader.streams(value))
}

/// `module.exports.getStreams`, else the global one.
fn entry<'js>(ctx: &Ctx<'js>) -> rquickjs::Result<Option<Function<'js>>> {
    let globals = ctx.globals();
    let exported = match globals.get::<_, Value>("module")?.into_object() {
        Some(module) => match module.get::<_, Value>("exports")?.into_object() {
            Some(exports) => exports.get::<_, Value>("getStreams")?.into_function(),
            None => None,
        },
        None => None,
    };
    match exported {
        Some(f) => Ok(Some(f)),
        None => Ok(globals.get::<_, Value>("getStreams")?.into_function()),
    }
}

/// This thread's CPU time; None if the clock cannot be read, which ends the
/// call as over its CPU limit.
#[cfg(unix)]
fn thread_cpu() -> Option<Duration> {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid pointer to a timespec the call fills.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    (rc == 0).then(|| Duration::new(t.tv_sec as u64, t.tv_nsec as u32))
}
