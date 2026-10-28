//! One plugin call (plans/plugins-and-marketplace.md 6.2): its own thread and
//! current-thread runtime, a fresh QuickJS-NG runtime with a memory and a
//! stack limit, an interrupt that reads the thread's CPU time, the wall
//! deadline and the cancel flag, and the host reached only through
//! `HostCall` and `HostReply` messages: the engine thread holds no client and
//! no key. Both are plain data, so the engine can move to another process
//! with the same messages (6.2).

use rquickjs::prelude::Func;
use rquickjs::promise::{MaybePromise, Promised};
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, CaughtError, Ctx, Exception, Function};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::future::Future;
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

/// What a plugin asks of the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostCall {
    Fetch { url: String },
}

/// The host's answer to one `HostCall`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostReply {
    Body(String),
    /// What the plugin's promise rejects with.
    Failed(String),
}

/// One call on the channel and where its reply goes.
#[derive(Debug)]
pub struct HostRequest {
    pub call: HostCall,
    pub reply: oneshot::Sender<HostReply>,
}

/// Requests one call may make (6.4); past it `fetch` throws and the host
/// never hears of it.
pub const MAX_REQUESTS: u32 = 30;

/// How a call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `getStreams`'s answer, as JSON.
    Done(String),
    /// The script threw, or `getStreams` was missing.
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

thread_local! {
    /// The host of this thread's one call.
    static HOST: RefCell<Option<mpsc::UnboundedSender<HostRequest>>> = const { RefCell::new(None) };
    /// Requests this thread's call has made.
    static SENT: Cell<u32> = const { Cell::new(0) };
}

/// Runs `script` and its `getStreams()` on a new thread; the handle ends
/// when the thread does. Fails only when the thread cannot be made.
pub fn call(
    script: String,
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
            rt.block_on(run(script, limits, cancel))
        })
}

async fn run(script: String, limits: Limits, cancel: Arc<AtomicBool>) -> Outcome {
    let why = Arc::new(AtomicU8::new(NONE));
    let Ok(qjs) = AsyncRuntime::new() else {
        return Outcome::Memory;
    };
    qjs.set_memory_limit(limits.memory).await;
    qjs.set_max_stack_size(limits.stack).await;
    let started = Instant::now();
    let cpu_start = thread_cpu();
    let (flag, said) = (cancel.clone(), why.clone());
    qjs.set_interrupt_handler(Some(Box::new(move || {
        let reason = if flag.load(Ordering::Relaxed) {
            CANCEL
        } else if cpu_start
            .zip(thread_cpu())
            .is_none_or(|(start, now)| now.saturating_sub(start) > limits.cpu)
        {
            CPU
        } else if started.elapsed() > limits.wall {
            WALL
        } else {
            return false;
        };
        said.store(reason, Ordering::Relaxed);
        true
    })))
    .await;
    let Ok(ctx) = AsyncContext::full(&qjs).await else {
        return Outcome::Memory;
    };
    let work = ctx.async_with(async |ctx| evaluate(&ctx, &script).await);
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
        (Some(Ok(json)), _) => Outcome::Done(json),
        (Some(Err(words)), _) if words == OUT_OF_MEMORY => Outcome::Memory,
        (Some(Err(words)), _) => Outcome::Threw(cut(words, MAX_WORDS)),
    }
}

const OUT_OF_MEMORY: &str = "out of memory";

/// What `Done` may carry: the output cap of 6.2.
pub const MAX_ANSWER: usize = 256 * 1024;

/// What `Threw` keeps of the script's words.
const MAX_WORDS: usize = 1024;

/// The longest URL `fetch` takes, in UTF-16 units as JS counts them.
pub const MAX_URL: usize = 8 * 1024;

/// An array or array-like longer than this makes every `Array.prototype`
/// method throw. QuickJS-NG walks generic array-likes in C loops that never
/// read the interrupt (`Array.prototype.reverse.call({length: 2 ** 53 - 1})`
/// ran forever, 2026-10-28), and no heap of 24 MiB holds an array this long.
const MAX_ARRAY: u64 = 1 << 24;

/// Runs before the script: the array guard above, and `fetch` over the
/// native one, which the script never sees. The URL's length is checked here
/// so that no long string is copied out of the heap.
const PRELUDE: &str = r#"(native, maxArray, maxUrl) => {
    const proto = Array.prototype, apply = Reflect.apply, define = Object.defineProperty;
    for (const name of Object.getOwnPropertyNames(proto)) {
        const method = proto[name];
        if (typeof method !== "function" || name === "constructor") continue;
        const guarded = { [name](...args) {
            if (this != null && +this.length > maxArray) throw new RangeError("array too long");
            return apply(method, this, args);
        } }[name];
        define(proto, name, { value: guarded, writable: true, configurable: true, enumerable: false });
    }
    globalThis.fetch = function fetch(url) {
        url = String(url);
        if (url.length > maxUrl) throw new TypeError("URL too long");
        return native(url);
    };
}"#;

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

/// The script, then `getStreams()` awaited, as JSON; or the words of what threw.
async fn evaluate<'js>(ctx: &Ctx<'js>, script: &str) -> Result<String, String> {
    let words = |e: CaughtError<'js>| match e {
        CaughtError::Exception(e) => e.message().unwrap_or_default(),
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
    let prelude: Function = ctx.eval(PRELUDE).catch(ctx).map_err(words)?;
    prelude
        .call::<_, ()>((Func::from(fetch), MAX_ARRAY as f64, MAX_URL as f64))
        .catch(ctx)
        .map_err(words)?;
    ctx.eval::<(), _>(script).catch(ctx).map_err(words)?;
    let entry: Function = globals.get("getStreams").catch(ctx).map_err(words)?;
    let answer: MaybePromise = entry.call(()).catch(ctx).map_err(words)?;
    let value: rquickjs::Value = answer.into_future().await.catch(ctx).map_err(words)?;
    let json = ctx.json_stringify(value).catch(ctx).map_err(words)?;
    let json = match json {
        Some(json) => json.to_string().catch(ctx).map_err(words)?,
        None => "null".into(),
    };
    if json.len() > MAX_ANSWER {
        return Err("the answer is over 256 KiB".into());
    }
    Ok(json)
}

/// The native `fetch(url)`: a message to the host, and a promise of the
/// body's text. Past the request cap it throws at once, so a loop of calls
/// leaves nothing behind on the host side.
fn fetch<'js>(
    ctx: Ctx<'js>,
    url: String,
) -> rquickjs::Result<Promised<impl Future<Output = rquickjs::Result<String>> + 'js>> {
    if SENT.get() >= MAX_REQUESTS {
        return Err(Exception::throw_type(&ctx, "rate limited"));
    }
    SENT.set(SENT.get() + 1);
    let (reply, answer) = oneshot::channel();
    let call = HostCall::Fetch { url };
    let sent = HOST.with(|h| {
        h.borrow()
            .as_ref()
            .map(|host| host.send(HostRequest { call, reply }))
    });
    if !matches!(sent, Some(Ok(()))) {
        return Err(Exception::throw_message(&ctx, "the host is gone"));
    }
    Ok(Promised(async move {
        match answer.await {
            Ok(HostReply::Body(body)) => Ok(body),
            Ok(HostReply::Failed(words)) => Err(Exception::throw_type(&ctx, &words)),
            Err(_) => Err(Exception::throw_message(&ctx, "the host is gone")),
        }
    }))
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
