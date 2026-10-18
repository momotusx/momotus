//! Read-ahead for HTTP playbacks. Players ask for many ranges in a row (two
//! bytes to probe, the start, the index at the end, the start again), and
//! each new upstream request pays the origin's time to first byte. Here
//! every range is served from one block cache per playback, filled by a few
//! long upstream reads ("cursors"); a request that starts at or just past a
//! cursor's position waits for it instead of opening another. The reuse rule
//! is rclone's VFS downloaders'.

use crate::lock;
use axum::http::{HeaderMap, StatusCode, header};
use bytes::{Bytes, BytesMut};
use mq_http::{HttpClient, HttpError, Priority, RequestOptions, StreamingResponse, Url};
use mq_media::{BLOCK_SIZE, ByteCache, CancellationToken};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::{Notify, watch};

/// Block cache per playback.
const CACHE_BYTES: u64 = 24 << 20;
/// Upstream reads open at once per playback; a new one closes the least
/// recently used.
const MAX_CURSORS: usize = 3;
/// How far a cursor reads past the furthest byte asked of it.
const READ_AHEAD: u64 = 4 << 20;
/// A request starting this far past a cursor's position waits for it.
const ATTACH: u64 = 1 << 20;
/// A cursor that nobody asked anything of for this long closes.
const IDLE: Duration = Duration::from_secs(10);
/// Longest wait for the origin's answer, its host's request slot included.
pub(crate) const OPEN: Duration = Duration::from_secs(20);
/// A running cursor that a reader waits on and that writes nothing for this
/// long is stuck (a connection the origin stopped feeding): it is replaced
/// by a fresh read from the reader's position.
const STALL: Duration = Duration::from_secs(6);
/// Cursor failures in a row that end a response.
const RETRIES: u32 = 3;
/// Request headers that never follow a redirect to another origin, as
/// reqwest drops them on the hop itself.
const CREDENTIALS: [&str; 5] = [
    "authorization",
    "cookie",
    "cookie2",
    "proxy-authorization",
    "www-authenticate",
];

/// Why a playback cannot be served from the cache.
#[derive(Debug, Clone)]
pub(crate) enum Refusal {
    /// The origin ignores ranges or gives no length: proxy it plainly.
    Passthrough,
    /// The origin's own error answer, passed on to the player with its
    /// `Content-Range` (a 416 names the length).
    Status(StatusCode, HeaderMap),
    /// No answer; the reason, for the playback's attempt list.
    Failed(String),
}

/// What the first upstream answer told about the file.
pub(crate) struct Meta {
    pub len: u64,
    /// `Content-Type`, `ETag` and `Last-Modified` from the origin.
    pub headers: HeaderMap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Opening,
    Running,
    Done,
    Closed,
}

#[derive(Debug, Clone, Copy)]
struct Progress {
    /// Every byte from the cursor's start to here went into the cache.
    frontier: u64,
    phase: Phase,
}

impl Progress {
    fn live(self) -> bool {
        matches!(self.phase, Phase::Opening | Phase::Running)
    }
}

struct Cursor {
    start: u64,
    progress: watch::Sender<Progress>,
    /// The furthest byte a reader asked of this cursor.
    demand: AtomicU64,
    wake: Notify,
    used: AtomicU64,
    refusal: Mutex<Option<Refusal>>,
    task: Mutex<Option<tokio::task::AbortHandle>>,
}

impl Cursor {
    fn progress(&self) -> Progress {
        *self.progress.borrow()
    }

    fn want(&self, pos: u64) {
        self.demand.fetch_max(pos, Ordering::AcqRel);
        self.wake.notify_one();
    }

    fn close(&self) {
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        self.finish(Phase::Closed);
    }

    /// Ends the cursor; an ended cursor never runs again.
    fn finish(&self, phase: Phase) {
        self.progress.send_if_modified(|p| {
            let live = p.live();
            if live {
                p.phase = phase;
            }
            live
        });
    }
}

/// Ends its cursor however the task stops, an abort included.
struct Exit(Arc<Cursor>);

impl Drop for Exit {
    fn drop(&mut self) {
        self.0.finish(Phase::Closed);
    }
}

/// Where the origin's redirect led, with the request headers that go there.
struct Redirect {
    url: String,
    headers: Vec<(String, String)>,
}

/// What cursor tasks share with the source.
struct Shared {
    http: HttpClient,
    url: String,
    headers: Vec<(String, String)>,
    allow_private: bool,
    /// The redirect target an earlier read reached, so later reads skip the
    /// hop. It may carry a short-lived token, so it lives only as long as
    /// this playback.
    redirect: Mutex<Option<Arc<Redirect>>>,
    cache: ByteCache,
    meta: OnceLock<Arc<Meta>>,
    passthrough: AtomicBool,
    /// Cancelled when the playback is released.
    released: CancellationToken,
    /// Held around the release check and each cache write, so nothing
    /// lands in the cache after release clears it.
    writing: Mutex<()>,
}

/// One HTTP playback.
pub(crate) struct Source {
    shared: Arc<Shared>,
    cursors: Mutex<Vec<Arc<Cursor>>>,
    tick: AtomicU64,
}

impl Drop for Source {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Source {
    pub(crate) fn new(
        http: HttpClient,
        url: String,
        headers: Vec<(String, String)>,
        allow_private: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(Shared {
                http,
                url,
                headers,
                allow_private,
                redirect: Mutex::default(),
                cache: ByteCache::new(CACHE_BYTES),
                meta: OnceLock::new(),
                passthrough: AtomicBool::new(false),
                released: CancellationToken::new(),
                writing: Mutex::default(),
            }),
            cursors: Mutex::default(),
            tick: AtomicU64::new(0),
        })
    }

    /// Stops every upstream read and frees the cache; responses still open
    /// end with an error.
    pub(crate) fn shutdown(&self) {
        let mut cursors = lock(&self.cursors);
        let _writing = lock(&self.shared.writing);
        self.shared.released.cancel();
        for cursor in cursors.drain(..) {
            cursor.close();
        }
        self.shared.cache.clear();
        *lock(&self.shared.redirect) = None;
    }

    /// Cancelled once the playback is released.
    pub(crate) fn released(&self) -> &CancellationToken {
        &self.shared.released
    }

    pub(crate) fn url(&self) -> &str {
        &self.shared.url
    }

    pub(crate) fn headers(&self) -> &[(String, String)] {
        &self.shared.headers
    }

    pub(crate) fn allow_private(&self) -> bool {
        self.shared.allow_private
    }

    pub(crate) fn passthrough(&self) -> bool {
        self.shared.passthrough.load(Ordering::Acquire)
    }

    /// Starts reading the file's beginning, so the player's first request
    /// finds it under way. Outside a runtime it does nothing.
    pub(crate) fn prefetch(&self) {
        if tokio::runtime::Handle::try_current().is_ok() {
            let _ = self.cursor_at(0);
        }
    }

    /// The file's length and headers, opening a cursor at `pos` when no
    /// upstream answer came yet.
    pub(crate) async fn meta(&self, pos: u64) -> Result<Arc<Meta>, Refusal> {
        if let Some(meta) = self.shared.meta.get() {
            return Ok(meta.clone());
        }
        let released = || Refusal::Failed("playback released".into());
        let cursor = self.cursor_at(pos).ok_or_else(released)?;
        let mut rx = cursor.progress.subscribe();
        let _ = rx.wait_for(|p| p.phase != Phase::Opening).await;
        if let Some(meta) = self.shared.meta.get() {
            return Ok(meta.clone());
        }
        Err(lock(&cursor.refusal).clone().unwrap_or_else(released))
    }

    /// The bytes at `pos`, up to the end of its block or `end`.
    pub(crate) async fn read(&self, pos: u64, end: u64) -> std::io::Result<Bytes> {
        let mut failures = 0;
        loop {
            let gone = || std::io::Error::other("playback released");
            if self.shared.released.is_cancelled() {
                return Err(gone());
            }
            let want = (BLOCK_SIZE - pos % BLOCK_SIZE).min(end - pos);
            if let Some(bytes) = self.shared.cache.get(&self.shared.url, pos, want)
                && !bytes.is_empty()
            {
                self.touch(pos + bytes.len() as u64);
                return Ok(bytes);
            }
            let cursor = self.cursor_at(pos).ok_or_else(gone)?;
            cursor.want(pos + want);
            let reached = wait_past(&cursor, pos).await;
            if !reached {
                tracing::warn!("stream read at {pos} ended or stalled; reading again from there");
                cursor.close();
                failures += 1;
                // A passthrough origin cannot continue a cached answer.
                if failures >= RETRIES || self.passthrough() {
                    return Err(std::io::Error::other("upstream failed"));
                }
            }
        }
    }

    /// Keeps the cursor a reader is following reading ahead of it.
    fn touch(&self, pos: u64) {
        for cursor in lock(&self.cursors).iter() {
            let p = cursor.progress();
            if p.live() && cursor.start <= pos && pos <= p.frontier + ATTACH {
                cursor.want(pos);
            }
        }
    }

    /// A live cursor that reaches `pos` soon, else a new one at `pos`;
    /// `None` once the playback is released.
    fn cursor_at(&self, pos: u64) -> Option<Arc<Cursor>> {
        let mut cursors = lock(&self.cursors);
        if self.shared.released.is_cancelled() {
            return None;
        }
        cursors.retain(|c| c.progress().live());
        let tick = self.tick.fetch_add(1, Ordering::Relaxed);
        let near = cursors.iter().find(|c| {
            let p = c.progress();
            p.frontier <= pos && pos < p.frontier + ATTACH
        });
        if let Some(cursor) = near {
            cursor.used.store(tick, Ordering::Relaxed);
            return Some(cursor.clone());
        }
        if cursors.len() >= MAX_CURSORS
            && let Some(oldest) =
                (0..cursors.len()).min_by_key(|&i| cursors[i].used.load(Ordering::Relaxed))
        {
            cursors.remove(oldest).close();
        }
        let start = pos - pos % BLOCK_SIZE;
        let (progress, _) = watch::channel(Progress {
            frontier: start,
            phase: Phase::Opening,
        });
        let cursor = Arc::new(Cursor {
            start,
            progress,
            demand: AtomicU64::new(start),
            wake: Notify::new(),
            used: AtomicU64::new(tick),
            refusal: Mutex::default(),
            task: Mutex::default(),
        });
        let task = tokio::spawn(run(self.shared.clone(), cursor.clone()));
        *lock(&cursor.task) = Some(task.abort_handle());
        cursors.push(cursor.clone());
        Some(cursor)
    }
}

/// Waits until `cursor` has written past `pos`. False when it ended first,
/// or made no progress for `STALL` while running (`OPEN` and a margin while
/// opening, which has its own deadline).
async fn wait_past(cursor: &Cursor, pos: u64) -> bool {
    let mut rx = cursor.progress.subscribe();
    loop {
        let p = *rx.borrow_and_update();
        if p.frontier > pos {
            return true;
        }
        if !p.live() {
            return false;
        }
        let limit = match p.phase {
            Phase::Opening => OPEN + Duration::from_secs(2),
            _ => STALL,
        };
        match tokio::time::timeout(limit, rx.changed()).await {
            Ok(Ok(())) => {}
            _ => return false,
        }
    }
}

async fn run(shared: Arc<Shared>, cursor: Arc<Cursor>) {
    let _exit = Exit(cursor.clone());
    let phase = match shared.open(cursor.start).await {
        Ok(upstream) => {
            cursor.progress.send_if_modified(|p| {
                let opening = p.phase == Phase::Opening;
                if opening {
                    p.phase = Phase::Running;
                }
                opening
            });
            pump(&shared, &cursor, upstream).await
        }
        Err(refusal) => {
            if matches!(refusal, Refusal::Passthrough) {
                shared.passthrough.store(true, Ordering::Release);
            }
            *lock(&cursor.refusal) = Some(refusal);
            Phase::Closed
        }
    };
    cursor.finish(phase);
}

impl Shared {
    /// A read from `start`: at the redirect target an earlier read reached,
    /// else at the original URL, which is also asked when the target answers
    /// anything but the range (an expired link, a refusal, no connection).
    async fn request(&self, start: u64) -> Result<StreamingResponse, HttpError> {
        let range = Some(format!("bytes={start}-"));
        let redirect = lock(&self.redirect).clone();
        if let Some(redirect) = redirect {
            let answer = self
                .http
                .stream(
                    &redirect.url,
                    range.clone(),
                    &self.options(&redirect.headers),
                )
                .await;
            match answer {
                Ok(upstream) if upstream.status == 206 => return Ok(upstream),
                Ok(upstream) => tracing::info!(
                    "stream redirect target answered {}; asking the original URL",
                    upstream.status
                ),
                Err(e) => {
                    tracing::info!("stream redirect target failed: {e}; asking the original URL")
                }
            }
            *lock(&self.redirect) = None;
        }
        self.http
            .stream(&self.url, range, &self.options(&self.headers))
            .await
    }

    fn options(&self, headers: &[(String, String)]) -> RequestOptions {
        RequestOptions {
            priority: Priority::Playback,
            timeout: Duration::from_secs(30),
            allow_private: self.allow_private,
            headers: headers.to_vec(),
            ..Default::default()
        }
    }

    /// Keeps where the original URL's redirect led for this playback's later
    /// reads.
    fn remember(&self, target: &Url) {
        let Ok(first) = Url::parse(&self.url) else {
            return;
        };
        if *target == first {
            return;
        }
        let cross = target.scheme() != first.scheme()
            || target.host_str() != first.host_str()
            || target.port_or_known_default() != first.port_or_known_default();
        let headers = self
            .headers
            .iter()
            .filter(|(name, _)| !cross || !CREDENTIALS.iter().any(|c| name.eq_ignore_ascii_case(c)))
            .cloned()
            .collect();
        *lock(&self.redirect) = Some(Arc::new(Redirect {
            url: target.to_string(),
            headers,
        }));
    }

    async fn open(&self, start: u64) -> Result<StreamingResponse, Refusal> {
        let upstream = match tokio::time::timeout(OPEN, self.request(start)).await {
            Ok(Ok(upstream)) => upstream,
            Ok(Err(e)) => {
                tracing::warn!("stream upstream failed: {e}");
                return Err(Refusal::Failed(e.to_string()));
            }
            Err(_) => {
                tracing::warn!("stream upstream sent no answer in {OPEN:?}");
                // A target that never answers is not asked again.
                *lock(&self.redirect) = None;
                return Err(Refusal::Failed("no answer in time".into()));
            }
        };
        let len = match upstream.status {
            // An origin that caps its answers (sends less than asked) is
            // proxied plainly, as players handle short answers themselves.
            206 => content_range(&upstream.headers)
                .filter(|&(first, last, len)| first == start && last.checked_add(1) == Some(len))
                .map(|(_, _, len)| len),
            // The whole file for a range: the origin ignores ranges.
            200 => None,
            status => {
                let mut headers = HeaderMap::new();
                if let Some(value) = upstream.headers.get(header::CONTENT_RANGE) {
                    headers.insert(header::CONTENT_RANGE, value.clone());
                }
                let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
                return Err(Refusal::Status(status, headers));
            }
        };
        let Some(len) = len else {
            return Err(Refusal::Passthrough);
        };
        let mut headers = HeaderMap::new();
        for name in [header::CONTENT_TYPE, header::ETAG, header::LAST_MODIFIED] {
            if let Some(value) = upstream.headers.get(&name) {
                headers.insert(name, value.clone());
            }
        }
        let meta = self.meta.get_or_init(|| {
            Arc::new(Meta {
                len,
                headers: headers.clone(),
            })
        });
        // Bytes of another version of the file must never join the cache: a
        // validator the first answer had must come back unchanged.
        // ponytail: an origin with no validators, or only a weak ETag, is
        // trusted on its length; checking bytes would need a hash per block.
        let changed = meta.len != len
            || [header::ETAG, header::LAST_MODIFIED].iter().any(|name| {
                meta.headers
                    .get(name)
                    .is_some_and(|first| headers.get(name) != Some(first))
            });
        if changed {
            tracing::warn!("stream upstream changed during playback");
            return Err(Refusal::Failed("the file changed during playback".into()));
        }
        self.remember(&upstream.final_url);
        Ok(upstream)
    }
}

/// Copies the upstream body into whole cache blocks until the end of the
/// file, a failure, or `IDLE` without demand.
async fn pump(shared: &Shared, cursor: &Cursor, mut upstream: StreamingResponse) -> Phase {
    let len = shared.meta.get().map_or(u64::MAX, |m| m.len);
    let mut offset = cursor.start;
    let mut pending = BytesMut::new();
    loop {
        while offset
            >= cursor
                .demand
                .load(Ordering::Acquire)
                .saturating_add(READ_AHEAD)
        {
            if tokio::time::timeout(IDLE, cursor.wake.notified())
                .await
                .is_err()
            {
                return Phase::Closed;
            }
        }
        match upstream.next_chunk().await {
            Some(Ok(chunk)) => {
                pending.extend_from_slice(&chunk);
                // A released playback's cache is cleared; never refill it.
                let _writing = lock(&shared.writing);
                if shared.released.is_cancelled() {
                    return Phase::Closed;
                }
                while pending.len() as u64 >= BLOCK_SIZE {
                    let block = pending.split_to(BLOCK_SIZE as usize).freeze();
                    shared.cache.insert(&shared.url, offset, &block);
                    offset += BLOCK_SIZE;
                    cursor.progress.send_modify(|p| p.frontier = offset);
                }
            }
            None if offset + pending.len() as u64 == len => {
                let _writing = lock(&shared.writing);
                if !pending.is_empty() && !shared.released.is_cancelled() {
                    let end = len;
                    shared.cache.insert(&shared.url, offset, &pending.freeze());
                    cursor.progress.send_modify(|p| p.frontier = end);
                }
                return Phase::Done;
            }
            // Ended early or failed: a partial block would read as the end
            // of the file, so it is dropped.
            None | Some(Err(_)) => return Phase::Closed,
        }
    }
}

/// `bytes first-last/len` as `(first, last, len)`.
fn content_range(headers: &HeaderMap) -> Option<(u64, u64, u64)> {
    let value = headers.get(header::CONTENT_RANGE)?.to_str().ok()?;
    let (span, len) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (first, last) = span.split_once('-')?;
    Some((
        first.trim().parse().ok()?,
        last.trim().parse().ok()?,
        len.trim().parse().ok()?,
    ))
}
