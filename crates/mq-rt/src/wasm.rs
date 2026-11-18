//! Wasm: one thread, tasks on `wasm_bindgen_futures::spawn_local`, timers
//! from the host. The signatures (and their `Send` bounds) are tokio's, so
//! code that builds natively builds here. A panic ends the process (the hook
//! calls `mqHost.died`), so a `JoinError` here is always a cancellation.

use futures_util::FutureExt;
use futures_util::StreamExt;
use futures_util::future::{AbortHandle as Abort, Abortable};
use futures_util::stream::FuturesUnordered;
use std::collections::BTreeMap;
use std::future::{Future, IntoFuture, poll_fn};
use std::pin::{Pin, pin};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::sync::oneshot;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = mqHost, js_name = setTimer)]
    fn set_timer(ms: u32, id: u32);
    #[wasm_bindgen(js_namespace = mqHost, js_name = clearTimer)]
    fn clear_timer(id: u32);
    #[wasm_bindgen(js_namespace = mqHost, js_name = log)]
    fn host_log(level: u32, line: &str);
    #[wasm_bindgen(js_namespace = mqHost)]
    fn died(message: &str);
}

/// A log line to the host (`ares-inspect --service` shows it). Levels are
/// `tracing`'s: 1 error, 2 warn, 3 info, 4 debug, 5 trace.
pub fn log(level: u32, line: &str) {
    host_log(level, line);
}

/// Every panic goes to `mqHost.died`, which replaces the core's process.
pub fn set_panic_hook() {
    std::panic::set_hook(Box::new(|info| died(&info.to_string())));
}

// ---- clocks ----

pub use web_time::Instant as StdInstant;

/// tokio's `Instant` shape over the host's monotonic clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(StdInstant);

impl Instant {
    pub fn now() -> Self {
        Self(StdInstant::now())
    }
    pub fn from_std(std: StdInstant) -> Self {
        Self(std)
    }
    pub fn into_std(self) -> StdInstant {
        self.0
    }
    /// Zero when `earlier` is later, as tokio's.
    pub fn duration_since(&self, earlier: Self) -> Duration {
        self.0.saturating_duration_since(earlier.0)
    }
    pub fn checked_duration_since(&self, earlier: Self) -> Option<Duration> {
        self.0.checked_duration_since(earlier.0)
    }
    pub fn saturating_duration_since(&self, earlier: Self) -> Duration {
        self.0.saturating_duration_since(earlier.0)
    }
    pub fn elapsed(&self) -> Duration {
        Self::now().duration_since(*self)
    }
    pub fn checked_add(&self, duration: Duration) -> Option<Self> {
        self.0.checked_add(duration).map(Self)
    }
    pub fn checked_sub(&self, duration: Duration) -> Option<Self> {
        self.0.checked_sub(duration).map(Self)
    }
}

impl From<StdInstant> for Instant {
    fn from(std: StdInstant) -> Self {
        Self(std)
    }
}

impl From<Instant> for StdInstant {
    fn from(instant: Instant) -> Self {
        instant.0
    }
}

impl std::ops::Add<Duration> for Instant {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self {
        Self(self.0 + rhs)
    }
}
impl std::ops::AddAssign<Duration> for Instant {
    fn add_assign(&mut self, rhs: Duration) {
        self.0 += rhs;
    }
}
impl std::ops::Sub<Duration> for Instant {
    type Output = Self;
    fn sub(self, rhs: Duration) -> Self {
        Self(self.0 - rhs)
    }
}
impl std::ops::SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, rhs: Duration) {
        self.0 -= rhs;
    }
}
impl std::ops::Sub for Instant {
    type Output = Duration;
    fn sub(self, rhs: Self) -> Duration {
        self.duration_since(rhs)
    }
}

/// Milliseconds since the Unix epoch by the wall clock (0 before it).
pub fn now_ms() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Always true: `spawn` works anywhere on wasm.
pub fn in_runtime() -> bool {
    true
}

/// Nothing on wasm: no threads, and the callers' work (the release-name
/// table) builds lazily on first use.
pub fn background(f: impl FnOnce() + Send + 'static) {
    drop(f);
}

// ---- timers ----

enum Slot {
    Waiting(Option<Waker>),
    Fired,
}

static TIMERS: Mutex<BTreeMap<u32, Slot>> = Mutex::new(BTreeMap::new());
static NEXT_TIMER: AtomicU32 = AtomicU32::new(1);
/// `setTimeout`'s longest delay; a longer sleep re-arms when this fires.
const MAX_TIMER_MS: u128 = (1 << 31) - 1;

fn timers() -> MutexGuard<'static, BTreeMap<u32, Slot>> {
    TIMERS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The host's answer to `setTimer(ms, id)`. The waker runs after the lock
/// is released: the no-threads `Mutex` panics on a nested lock.
#[wasm_bindgen]
pub fn fire(id: u32) {
    let waker = {
        let mut map = timers();
        match map.get_mut(&id) {
            Some(slot) => match std::mem::replace(slot, Slot::Fired) {
                Slot::Waiting(waker) => waker,
                Slot::Fired => None,
            },
            // Cleared or dropped before the host fired: nothing waits.
            None => None,
        }
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// tokio's far future for a deadline that overflows (about 30 years).
fn far_future() -> Instant {
    Instant::now() + Duration::from_secs(86_400 * 365 * 30)
}

fn deadline_after(duration: Duration) -> Instant {
    Instant::now()
        .checked_add(duration)
        .unwrap_or_else(far_future)
}

/// Completes at its deadline. The host timer is armed on the first poll and
/// re-armed whenever it fires early (rounding, or a delay past
/// `MAX_TIMER_MS`), so `Instant::now() >= deadline` holds once it is ready.
#[derive(Debug)]
pub struct Sleep {
    deadline: Instant,
    armed: Option<u32>,
}

impl Sleep {
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn is_elapsed(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn disarm(&mut self) {
        if let Some(id) = self.armed.take() {
            let pending = matches!(timers().remove(&id), Some(Slot::Waiting(_)));
            if pending {
                clear_timer(id);
            }
        }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let left = this.deadline.duration_since(Instant::now());
        if left.is_zero() {
            this.disarm();
            return Poll::Ready(());
        }
        let mut map = timers();
        if let Some(Slot::Waiting(waker)) = this.armed.and_then(|id| map.get_mut(&id)) {
            match waker {
                Some(w) if w.will_wake(cx.waker()) => {}
                _ => *waker = Some(cx.waker().clone()),
            }
            return Poll::Pending;
        }
        // Not armed yet, or fired before the deadline: arm a new timer.
        if let Some(old) = this.armed.take() {
            map.remove(&old);
        }
        let id = loop {
            let id = NEXT_TIMER.fetch_add(1, Ordering::Relaxed);
            if !map.contains_key(&id) {
                break id;
            }
        };
        map.insert(id, Slot::Waiting(Some(cx.waker().clone())));
        drop(map);
        this.armed = Some(id);
        // Rounded up, so an early fire can only come from the host.
        let ms = left.as_micros().div_ceil(1000).min(MAX_TIMER_MS);
        set_timer(ms as u32, id);
        Poll::Pending
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        self.disarm();
    }
}

pub fn sleep(duration: Duration) -> Sleep {
    sleep_until(deadline_after(duration))
}

pub fn sleep_until(deadline: Instant) -> Sleep {
    Sleep {
        deadline,
        armed: None,
    }
}

/// tokio's `Elapsed`: the deadline passed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elapsed(());

impl std::fmt::Display for Elapsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("deadline has elapsed")
    }
}

impl std::error::Error for Elapsed {}

/// The deadline is taken now, as tokio's is.
pub fn timeout<F: IntoFuture>(
    duration: Duration,
    future: F,
) -> impl Future<Output = Result<F::Output, Elapsed>> {
    timeout_at(deadline_after(duration), future)
}

/// The future is polled before the deadline, so a ready value wins, as in
/// tokio.
pub fn timeout_at<F: IntoFuture>(
    deadline: Instant,
    future: F,
) -> impl Future<Output = Result<F::Output, Elapsed>> {
    let future = future.into_future();
    let mut sleep = sleep_until(deadline);
    async move {
        let mut future = pin!(future);
        poll_fn(|cx| {
            if let Poll::Ready(value) = future.as_mut().poll(cx) {
                return Poll::Ready(Ok(value));
            }
            Pin::new(&mut sleep).poll(cx).map(|()| Err(Elapsed(())))
        })
        .await
    }
}

/// tokio's `Interval` with its defaults: the first tick at once, and
/// missed ticks in a burst (`MissedTickBehavior::Burst`).
#[derive(Debug)]
pub struct Interval {
    next: Instant,
    period: Duration,
}

/// Panics on a zero period, as tokio's does.
pub fn interval(period: Duration) -> Interval {
    assert!(period > Duration::ZERO, "`period` must be non-zero.");
    Interval {
        next: Instant::now(),
        period,
    }
}

impl Interval {
    /// Cancel safe: a dropped `tick` leaves the schedule as it was.
    pub async fn tick(&mut self) -> Instant {
        sleep_until(self.next).await;
        let at = self.next;
        self.next = at.checked_add(self.period).unwrap_or_else(far_future);
        at
    }

    pub fn period(&self) -> Duration {
        self.period
    }
}

// ---- tasks ----

pub mod task {
    use std::sync::atomic::Ordering;

    /// A task's id, unique for the process.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Id(pub(super) u64);

    impl Id {
        pub(super) fn next() -> Self {
            Self(super::NEXT_TASK.fetch_add(1, Ordering::Relaxed))
        }
    }

    impl std::fmt::Display for Id {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.fmt(f)
        }
    }
}

use task::Id;

static NEXT_TASK: AtomicU64 = AtomicU64::new(1);

/// A task that ended without its value: always a cancellation on wasm.
#[derive(Debug)]
pub struct JoinError {
    id: Id,
}

impl JoinError {
    pub fn is_cancelled(&self) -> bool {
        true
    }
    pub fn is_panic(&self) -> bool {
        false
    }
    pub fn id(&self) -> Id {
        self.id
    }
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "task {} was cancelled", self.id)
    }
}

impl std::error::Error for JoinError {}

/// Aborts a task without owning its result.
#[derive(Debug)]
pub struct AbortHandle {
    id: Id,
    abort: Abort,
    done: Arc<AtomicBool>,
}

impl AbortHandle {
    pub fn abort(&self) {
        self.abort.abort();
    }
    pub fn is_finished(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
    pub fn id(&self) -> Id {
        self.id
    }
}

/// A spawned task's result. Dropping it detaches the task.
#[derive(Debug)]
pub struct JoinHandle<T> {
    result: oneshot::Receiver<Result<T, JoinError>>,
    id: Id,
    abort: Abort,
    done: Arc<AtomicBool>,
}

impl<T> JoinHandle<T> {
    pub fn abort(&self) {
        self.abort.abort();
    }
    pub fn abort_handle(&self) -> AbortHandle {
        AbortHandle {
            id: self.id,
            abort: self.abort.clone(),
            done: self.done.clone(),
        }
    }
    pub fn is_finished(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
    pub fn id(&self) -> Id {
        self.id
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = Result<T, JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let id = self.id;
        Pin::new(&mut self.result)
            .poll(cx)
            .map(|answer| answer.unwrap_or(Err(JoinError { id })))
    }
}

pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let id = Id::next();
    let (abort, registration) = Abort::new_pair();
    let done = Arc::new(AtomicBool::new(false));
    let (tx, result) = oneshot::channel();
    let finished = done.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let out = Abortable::new(future, registration)
            .await
            .map_err(|_| JoinError { id });
        finished.store(true, Ordering::Release);
        let _ = tx.send(out);
    });
    JoinHandle {
        result,
        id,
        abort,
        done,
    }
}

/// Runs `f` at once, on the caller's turn (no threads on wasm), and hands
/// back a finished handle.
pub fn spawn_blocking<F, R>(f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let (tx, result) = oneshot::channel();
    let _ = tx.send(Ok(f()));
    JoinHandle {
        result,
        id: Id::next(),
        abort: Abort::new_pair().0,
        done: Arc::new(AtomicBool::new(true)),
    }
}

/// Lets every other ready task run once before this one goes on.
pub async fn yield_now() {
    let mut yielded = false;
    poll_fn(|cx| {
        if yielded {
            return Poll::Ready(());
        }
        yielded = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    })
    .await;
}

/// A handle that answers with its task's id.
struct Tagged<T>(JoinHandle<T>);

impl<T> Future for Tagged<T> {
    type Output = Result<(Id, T), JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let id = self.0.id;
        Pin::new(&mut self.0).poll(cx).map(|r| r.map(|v| (id, v)))
    }
}

/// tokio's `JoinSet`: results in completion order, every task aborted when
/// the set is dropped.
pub struct JoinSet<T> {
    tasks: FuturesUnordered<Tagged<T>>,
}

impl<T> Default for JoinSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::fmt::Debug for JoinSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinSet").field("len", &self.len()).finish()
    }
}

impl<T> JoinSet<T> {
    pub fn new() -> Self {
        Self {
            tasks: FuturesUnordered::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn abort_all(&mut self) {
        for task in self.tasks.iter() {
            task.0.abort();
        }
    }

    pub async fn join_next(&mut self) -> Option<Result<T, JoinError>> {
        self.join_next_with_id()
            .await
            .map(|r| r.map(|(_, value)| value))
    }

    pub async fn join_next_with_id(&mut self) -> Option<Result<(Id, T), JoinError>> {
        self.tasks.next().await
    }

    /// A finished task's result, or `None` when none has finished yet.
    pub fn try_join_next(&mut self) -> Option<Result<T, JoinError>> {
        self.tasks
            .next()
            .now_or_never()
            .flatten()
            .map(|r| r.map(|(_, value)| value))
    }
}

impl<T: Send + 'static> JoinSet<T> {
    pub fn spawn<F>(&mut self, future: F) -> AbortHandle
    where
        F: Future<Output = T> + Send + 'static,
    {
        let handle = spawn(future);
        let abort = handle.abort_handle();
        self.tasks.push(Tagged(handle));
        abort
    }
}

impl<T> Drop for JoinSet<T> {
    fn drop(&mut self) {
        self.abort_all();
    }
}
