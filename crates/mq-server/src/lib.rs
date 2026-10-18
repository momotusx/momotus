//! Loopback stream server. Every playback gets one URL,
//! `http://127.0.0.1:<port>/s/<token>/<filename>`, where the token is 32
//! random bytes. Players never learn where bytes come from: an upstream
//! HTTP(S) URL served through a read-ahead block cache (`ahead`), or
//! (feature `torrent`, full builds only) a file of the embedded torrent
//! engine served from verified pieces.
//!
//! Its subtitles get `http://127.0.0.1:<port>/sub/<token>/<id>/<lang>.vtt`
//! under the same token: the addon's file fetched on request and served as
//! UTF-8 WebVTT. A token exists only while its `Lease` does.

mod ahead;
mod subtitles;
#[cfg(all(test, feature = "torrent"))]
#[allow(dead_code)]
#[path = "../../mq-engine/tests/support/mod.rs"]
mod torrent_support;

pub use subtitles::to_vtt;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{Stream, StreamExt, stream};
use mq_http::{HttpClient, Priority, RequestOptions, Url};
use mq_media::CancellationToken;
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Where the bytes of one playback come from.
#[derive(Debug, Clone)]
pub enum Upstream {
    Http {
        url: String,
        /// Request headers from the stream's `proxyHeaders`.
        headers: Vec<(String, String)>,
        /// The transport URL of the install that listed the stream: its own
        /// host is the only private address the playback may reach. `None`
        /// (a debrid link) reaches public addresses only.
        addon_url: Option<Url>,
    },
    #[cfg(feature = "torrent")]
    Torrent {
        engine: mq_engine::Engine,
        torrent: mq_engine::TorrentHandle,
        file: usize,
    },
}

/// An addon's subtitle file, served converted.
#[derive(Debug, Clone, PartialEq)]
struct Subtitle {
    url: String,
    lang: String,
    /// The install whose answer named the file: its scope, no headers.
    addon_url: Option<Url>,
}

/// The addresses an install's own host adds to its playbacks' scope
/// (`mq_http::local_addrs`); none without an install.
fn scope(addon_url: Option<&Url>) -> Vec<IpAddr> {
    addon_url
        .and_then(Url::host_str)
        .map(mq_http::local_addrs)
        .unwrap_or_default()
}

/// Where one playback's bytes come from.
enum Content {
    Http(Arc<ahead::Source>),
    #[cfg(feature = "torrent")]
    Torrent {
        engine: mq_engine::Engine,
        torrent: mq_engine::TorrentHandle,
        file: usize,
    },
}

/// One playback: its body and its subtitles, under one token.
struct Playback {
    body: Content,
    /// Cancelled at release: bodies being served and fetches in flight end.
    released: CancellationToken,
    /// By the id in `/sub/{token}/{id}/...`; deduplicated within the playback.
    subtitles: Mutex<Vec<Subtitle>>,
}

impl Playback {
    #[allow(irrefutable_let_patterns)] // one variant without `torrent`
    fn release(&self) {
        self.released.cancel();
        if let Content::Http(source) = &self.body {
            source.shutdown();
        }
    }
}

/// What the routes and every lease share.
struct Shared {
    http: HttpClient,
    playbacks: Mutex<HashMap<String, Arc<Playback>>>,
    addr: SocketAddr,
}

pub struct StreamServer {
    shared: Arc<Shared>,
    /// Stops accepting and closes every connection, keep-alive ones too.
    shutdown: CancellationToken,
}

impl StreamServer {
    /// Binds an ephemeral port on 127.0.0.1 only.
    pub async fn start(http: HttpClient) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let shared = Arc::new(Shared {
            http,
            playbacks: Mutex::default(),
            addr: listener.local_addr()?,
        });
        let app = Router::new()
            .route("/s/{token}/{name}", get(serve).head(serve))
            .route("/sub/{token}/{id}/{name}", get(serve_subtitle))
            .with_state(shared.clone());
        let shutdown = CancellationToken::new();
        let stop = shutdown.clone().cancelled_owned();
        let listener = Loopback {
            tcp: listener,
            shutdown: shutdown.clone(),
        };
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(stop)
                .await;
        });
        Ok(Self { shared, shutdown })
    }

    /// A playback that serves at once, with no check of its origin: for the
    /// CLI and examples. `filename` only helps players guess the container;
    /// it carries no secrets. An HTTP upstream starts reading at once.
    pub fn register(&self, upstream: Upstream, filename: &str) -> Lease {
        self.lease(self.playback(upstream), filename)
    }

    /// A playback whose origin answered within `deadline`, so a dead link
    /// fails here instead of in the player. For HTTP that answer is the read
    /// that goes on to serve the player; no extra request is made. The token
    /// exists only once the answer came: dropping this future first leaves
    /// none, and stops the read.
    #[allow(irrefutable_let_patterns)] // one variant without `torrent`
    pub async fn open(
        &self,
        upstream: Upstream,
        filename: &str,
        deadline: Duration,
    ) -> Result<Lease, String> {
        let playback = self.playback(upstream);
        // Torrent playbacks are checked before they get here.
        if let Content::Http(source) = &playback.body {
            let error = match tokio::time::timeout(deadline, source.meta(0)).await {
                Ok(Ok(_) | Err(ahead::Refusal::Passthrough)) => None,
                Ok(Err(ahead::Refusal::Status(status, _))) => Some(refused(status)),
                Ok(Err(ahead::Refusal::Blocked)) => {
                    Some("The source points to a private network address".into())
                }
                Ok(Err(ahead::Refusal::Failed(reason))) => {
                    tracing::info!("stream source unreachable: {reason}");
                    Some("The source could not be reached".into())
                }
                Err(_) => Some("The source did not answer in time".into()),
            };
            if let Some(error) = error {
                playback.release();
                return Err(error);
            }
        }
        Ok(self.lease(playback, filename))
    }

    fn playback(&self, upstream: Upstream) -> Arc<Playback> {
        let body = match upstream {
            Upstream::Http {
                url,
                headers,
                addon_url,
            } => {
                let http = self.shared.http.scoped(&scope(addon_url.as_ref()));
                let source = ahead::Source::new(http, url, headers);
                source.prefetch();
                Content::Http(source)
            }
            #[cfg(feature = "torrent")]
            Upstream::Torrent {
                engine,
                torrent,
                file,
            } => Content::Torrent {
                engine,
                torrent,
                file,
            },
        };
        Arc::new(Playback {
            body,
            released: CancellationToken::new(),
            subtitles: Mutex::default(),
        })
    }

    fn lease(&self, playback: Arc<Playback>, filename: &str) -> Lease {
        let token = new_token();
        lock(&self.shared.playbacks).insert(token.clone(), playback.clone());
        let url = format!(
            "http://{}/s/{token}/{}",
            self.shared.addr,
            sanitize(filename)
        );
        Lease {
            url,
            token,
            playback,
            shared: self.shared.clone(),
        }
    }

    /// Playbacks with a live token.
    pub fn playbacks(&self) -> usize {
        lock(&self.shared.playbacks).len()
    }

    pub fn addr(&self) -> SocketAddr {
        self.shared.addr
    }
}

impl Drop for StreamServer {
    /// Every token goes, every body and fetch ends, and axum closes the
    /// connections, keep-alive ones too.
    fn drop(&mut self) {
        self.shutdown.cancel();
        let playbacks = std::mem::take(&mut *lock(&self.shared.playbacks));
        for playback in playbacks.values() {
            playback.release();
        }
    }
}

/// The server's listener. Its connections fail every read and write once
/// the server is dropped, so a player that stopped reading cannot keep a
/// connection task, or graceful shutdown, waiting.
struct Loopback {
    tcp: tokio::net::TcpListener,
    shutdown: CancellationToken,
}

impl axum::serve::Listener for Loopback {
    type Io = Conn;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Conn, SocketAddr) {
        let (io, addr) = axum::serve::Listener::accept(&mut self.tcp).await;
        let closed = Box::pin(self.shutdown.clone().cancelled_owned());
        (Conn { io, closed }, addr)
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.tcp.local_addr()
    }
}

struct Conn {
    io: tokio::net::TcpStream,
    closed: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl Conn {
    /// An error once the server is gone. Polling also wakes a task that
    /// waits on the socket when that happens.
    fn open(&mut self, cx: &mut Context<'_>) -> std::io::Result<()> {
        match self.closed.as_mut().poll(cx) {
            Poll::Ready(()) => Err(std::io::ErrorKind::ConnectionAborted.into()),
            Poll::Pending => Ok(()),
        }
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.open(cx)?;
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.open(cx)?;
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.open(cx)?;
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

/// A response body fed from a task of its own, which ends at release
/// whether or not the player reads, and when the player goes. So a player
/// that stopped reading keeps neither an upstream response and its request
/// slot nor a torrent range's demand.
fn pumped<S>(chunks: S, released: CancellationToken) -> Body
where
    S: Stream<Item = std::io::Result<bytes::Bytes>> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        let pump = async {
            let mut chunks = std::pin::pin!(chunks);
            while let Some(chunk) = chunks.next().await {
                if tx.send(chunk).await.is_err() {
                    return;
                }
            }
        };
        tokio::select! {
            () = pump => {}
            () = tx.closed() => {}
            () = released.cancelled() => {}
        }
    });
    Body::from_stream(stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|chunk| (chunk, rx))
    }))
}

/// One playback's token: its URL and its subtitles' work while the lease
/// lives. Dropping it removes the token, ends bodies being served and stops
/// every upstream read.
pub struct Lease {
    pub url: String,
    token: String,
    playback: Arc<Playback>,
    shared: Arc<Shared>,
}

impl Lease {
    /// Where this playback's subtitle tracks are added, for as long as it
    /// lives.
    pub fn subtitles(&self) -> SubtitleSlot {
        SubtitleSlot {
            playback: Arc::downgrade(&self.playback),
            base: format!("http://{}/sub/{}", self.shared.addr, self.token),
        }
    }

    /// The playback's bytes in process (through its read-ahead cache), for
    /// the subtitle hash; `None` for an origin that ignores ranges and for a
    /// torrent, which has its own reader.
    pub fn reader(&self) -> Option<Box<dyn mq_media::RangeReader>> {
        match &self.playback.body {
            Content::Http(source) if !source.passthrough() => {
                Some(Box::new(ahead::Reader(source.clone())))
            }
            _ => None,
        }
    }
}

impl std::fmt::Debug for Lease {
    /// Never the URL: its token is the playback's only secret.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lease").finish_non_exhaustive()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut playbacks = lock(&self.shared.playbacks);
        if playbacks
            .get(&self.token)
            .is_some_and(|p| Arc::ptr_eq(p, &self.playback))
        {
            playbacks.remove(&self.token);
        }
        drop(playbacks);
        self.playback.release();
    }
}

/// Adds subtitle tracks to one playback. It never keeps the playback alive:
/// once the lease is gone, `add` gives `None`.
#[derive(Clone)]
pub struct SubtitleSlot {
    playback: Weak<Playback>,
    base: String,
}

impl SubtitleSlot {
    /// The URL that serves the addon's file `url` as UTF-8 WebVTT. The same
    /// file gets the same URL, so tracks compare by it. `addon_url` is the
    /// install whose answer named the file: the fetch reaches only its own
    /// host among private addresses. `None` once the playback was released.
    pub fn add(&self, url: &str, lang: &str, addon_url: Option<&Url>) -> Option<String> {
        let playback = self.playback.upgrade()?;
        if playback.released.is_cancelled() {
            return None;
        }
        let subtitle = Subtitle {
            url: url.to_owned(),
            lang: lang.to_owned(),
            addon_url: addon_url.cloned(),
        };
        let mut subtitles = lock(&playback.subtitles);
        let id = match subtitles.iter().position(|s| *s == subtitle) {
            Some(id) => id,
            None => {
                subtitles.push(subtitle);
                subtitles.len() - 1
            }
        };
        Some(format!("{}/{id}/{}.vtt", self.base, sanitize(lang)))
    }
}

/// A route id: decimal, no sign, no leading zero.
fn parse_id(id: &str) -> Option<usize> {
    let digits = !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit());
    (digits && (id == "0" || !id.starts_with('0')))
        .then(|| id.parse::<u32>().ok())
        .flatten()
        .map(|id| id as usize)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("os randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sanitize(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    if clean.is_empty() {
        "stream".into()
    } else {
        clean
    }
}

/// Response headers forwarded from the origin to the player.
const PASSTHROUGH: [header::HeaderName; 6] = [
    header::CONTENT_TYPE,
    header::CONTENT_LENGTH,
    header::CONTENT_RANGE,
    header::ACCEPT_RANGES,
    header::LAST_MODIFIED,
    header::ETAG,
];

/// DNS rebinding guard: only answer requests addressed to our loopback.
fn host_ok(shared: &Shared, headers: &HeaderMap) -> bool {
    let port = shared.addr.port();
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"))
}

/// The playback a token names, while its lease lives.
fn playback(shared: &Shared, token: &str) -> Option<Arc<Playback>> {
    lock(&shared.playbacks).get(token).cloned()
}

/// Fetches the addon's subtitle file and answers with UTF-8 WebVTT. Any
/// origin may read it (a browser's `<track crossorigin>`); the token is the
/// capability.
async fn serve_subtitle(
    State(shared): State<Arc<Shared>>,
    Path((token, id, _name)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    if !host_ok(&shared, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(playback) = playback(&shared, &token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let subtitle = parse_id(&id).and_then(|id| lock(&playback.subtitles).get(id).cloned());
    let Some(subtitle) = subtitle else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let opts = RequestOptions {
        priority: Priority::Playback,
        timeout: Duration::from_secs(20),
        ..Default::default()
    };
    let http = shared.http.scoped(&scope(subtitle.addon_url.as_ref()));
    let fetch = http.get(&subtitle.url, &opts);
    // Release ends the fetch too.
    let body = match playback.released.run_until_cancelled(fetch).await {
        None => return StatusCode::NOT_FOUND.into_response(),
        Some(Ok(response)) => response.body,
        Some(Err(e)) => {
            tracing::warn!("subtitle upstream failed: {}", e.kind());
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let Some(vtt) = to_vtt(&body, &subtitle.lang) else {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, "text/vtt; charset=utf-8"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
        ],
        vtt,
    )
        .into_response()
}

async fn serve(
    State(shared): State<Arc<Shared>>,
    Path((token, _name)): Path<(String, String)>,
    request: Request<Body>,
) -> Response {
    if !host_ok(&shared, request.headers()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(playback) = playback(&shared, &token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let head_only = request.method() == Method::HEAD;
    match &playback.body {
        Content::Http(source) => serve_http(source.clone(), range, head_only).await,
        #[cfg(feature = "torrent")]
        Content::Torrent {
            engine,
            torrent,
            file,
        } => {
            let released = playback.released.clone();
            torrent::serve(
                engine,
                *torrent,
                *file,
                range.as_deref(),
                head_only,
                released,
            )
            .await
        }
    }
}

/// A source's error answer in plain words for the attempt list, with the
/// status kept for support.
fn refused(status: StatusCode) -> String {
    let code = status.as_u16();
    let words = match code {
        404 | 410 => "The link no longer exists",
        401 | 403 => "The source refused access",
        429 => "The source is busy, try again soon",
        500..=599 => "The source had an error",
        _ => "The source answered with an error",
    };
    format!("{words} ({code})")
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n` against `len`, as `[start, end)`.
/// `None` when unsatisfiable or not a single byte range.
fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => (len.saturating_sub(n.parse().ok()?), len),
        (a, "") => (a.parse().ok()?, len),
        (a, b) => (
            a.parse().ok()?,
            b.parse::<u64>().ok()?.saturating_add(1).min(len),
        ),
    };
    (start < end && start < len).then_some((start, end))
}

/// Status and span for a request against `len`, with `Content-Range` and
/// `Content-Length` set. `None` is unsatisfiable: answer 416 with `out`.
fn range_headers(
    range: Option<&str>,
    len: u64,
    out: &mut HeaderMap,
) -> Option<(StatusCode, u64, u64)> {
    let (status, start, end) = match range {
        None => (StatusCode::OK, 0, len),
        Some(h) => match parse_range(h, len) {
            Some((start, end)) => {
                let value = format!("bytes {start}-{}/{len}", end - 1);
                if let Ok(v) = HeaderValue::from_str(&value) {
                    out.insert(header::CONTENT_RANGE, v);
                }
                (StatusCode::PARTIAL_CONTENT, start, end)
            }
            None => {
                if let Ok(v) = HeaderValue::from_str(&format!("bytes */{len}")) {
                    out.insert(header::CONTENT_RANGE, v);
                }
                return None;
            }
        },
    };
    out.insert(header::CONTENT_LENGTH, HeaderValue::from(end - start));
    Some((status, start, end))
}

#[cfg(feature = "torrent")]
mod torrent {
    use super::*;

    fn content_type(name: &str) -> &'static str {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("mkv") => "video/x-matroska",
            Some("mp4" | "m4v") => "video/mp4",
            Some("webm") => "video/webm",
            Some("avi") => "video/x-msvideo",
            Some("mov") => "video/quicktime",
            Some("ts" | "m2ts") => "video/mp2t",
            Some("mpg" | "mpeg") => "video/mpeg",
            Some("wmv") => "video/x-ms-wmv",
            Some("flv") => "video/x-flv",
            _ => "application/octet-stream",
        }
    }

    /// Serves one file of the engine with Range support. Headers go out at
    /// once; the body follows as pieces are verified. HEAD opens no demand.
    /// A release before the answer goes out stops the setup too, so a
    /// released torrent is never selected, woken or given demand again.
    pub(crate) async fn serve(
        engine: &mq_engine::Engine,
        torrent: mq_engine::TorrentHandle,
        file: usize,
        range: Option<&str>,
        head_only: bool,
        released: CancellationToken,
    ) -> Response {
        let answer = open(engine, torrent, file, range, head_only, released.clone());
        released
            .run_until_cancelled(answer)
            .await
            .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
    }

    async fn open(
        engine: &mq_engine::Engine,
        torrent: mq_engine::TorrentHandle,
        file: usize,
        range: Option<&str>,
        head_only: bool,
        released: CancellationToken,
    ) -> Response {
        let reader = match engine.open_reader(torrent, file).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("torrent stream unavailable: {e}");
                return StatusCode::NOT_FOUND.into_response();
            }
        };
        let len = reader.len();
        let mut out = HeaderMap::new();
        out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        out.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(content_type(&reader.file().path)),
        );
        let (status, start, end) = match range_headers(range, len, &mut out) {
            Some(span) => span,
            None => return (StatusCode::RANGE_NOT_SATISFIABLE, out).into_response(),
        };
        if head_only {
            return (status, out).into_response();
        }
        let body = match reader.range(start, Some(end)).await {
            Ok(body) => body,
            Err(e) => {
                tracing::warn!("torrent range failed: {e}");
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        };
        // Release ends the body: a read waiting for pieces stops at once,
        // and the range's demand goes.
        let chunks = stream::unfold(Some(body), |body| async move {
            use tokio::io::AsyncReadExt;
            let mut body = body?;
            let mut buf = vec![0u8; 64 * 1024];
            match body.read(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    Some((Ok(bytes::Bytes::from(buf)), Some(body)))
                }
                Err(e) => Some((Err(e), None)),
            }
        });
        (status, out, pumped(chunks, released)).into_response()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::torrent_support as support;

        #[test]
        fn names_content_types() {
            assert_eq!(content_type("A/B.MKV"), "video/x-matroska");
            assert_eq!(content_type("x.iso"), "application/octet-stream");
        }

        /// A request that found its playback just before the lease went:
        /// its setup never runs, so the torrent is neither started nor
        /// given demand.
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_released_setup_never_starts_the_torrent() {
            let s = support::swarm(true).await;
            let t = s
                .leecher
                .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
                .await
                .unwrap();
            let file = s.leecher.files(t).unwrap().len() - 1;
            let released = CancellationToken::new();
            released.cancel();
            let response = serve(&s.leecher, t, file, Some("bytes=0-"), false, released).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let stats = s.leecher.stats(t).await.unwrap();
            assert_eq!((stats.readers, stats.pieces_total), (0, 0));
        }
    }
}

/// Serves a range from the playback's read-ahead cache.
async fn serve_http(
    source: Arc<ahead::Source>,
    range: Option<String>,
    head_only: bool,
) -> Response {
    if source.passthrough() {
        return proxy_http(&source, range, head_only).await;
    }
    let first = range
        .as_deref()
        .and_then(|h| {
            h.trim()
                .strip_prefix("bytes=")?
                .split_once('-')?
                .0
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or(0);
    let meta = match source.meta(first).await {
        Ok(meta) => meta,
        Err(ahead::Refusal::Passthrough) => {
            return proxy_http(&source, range, head_only).await;
        }
        Err(ahead::Refusal::Status(status, headers)) => return (status, headers).into_response(),
        Err(ahead::Refusal::Blocked | ahead::Refusal::Failed(_)) => {
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let mut out = meta.headers.clone();
    out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    // A page may read it as `<video crossorigin>`; the token is the capability.
    out.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    let (status, start, end) = match range_headers(range.as_deref(), meta.len, &mut out) {
        Some(span) => span,
        None => return (StatusCode::RANGE_NOT_SATISFIABLE, out).into_response(),
    };
    if head_only {
        return (status, out).into_response();
    }
    let body = stream::unfold(Some(start), move |pos| {
        let source = source.clone();
        async move {
            let pos = pos.filter(|&p| p < end)?;
            match source.read(pos, end).await {
                Ok(bytes) => {
                    let next = pos + bytes.len() as u64;
                    Some((Ok(bytes), Some(next)))
                }
                Err(e) => Some((Err(e), None)),
            }
        }
    });
    (status, out, Body::from_stream(body)).into_response()
}

/// Plain proxy with Range passthrough, for origins that ignore ranges or
/// give no length. A redirect the client did not follow answers 502, and an
/// error status goes on with its `Content-Range` but without its body.
async fn proxy_http(source: &ahead::Source, range: Option<String>, head_only: bool) -> Response {
    let opts = source.options();
    let released = source.released().clone();
    let open = tokio::time::timeout(
        ahead::OPEN,
        source.http().stream(source.url(), range, &opts),
    );
    let upstream = match released.run_until_cancelled(open).await {
        None => return StatusCode::NOT_FOUND.into_response(),
        Some(Ok(Ok(response))) => response,
        Some(Ok(Err(e))) => {
            tracing::warn!("plain proxy upstream failed: {}", e.kind());
            return StatusCode::BAD_GATEWAY.into_response();
        }
        Some(Err(_)) => {
            tracing::warn!("plain proxy upstream sent no answer in {:?}", ahead::OPEN);
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let status = StatusCode::from_u16(upstream.status).unwrap_or(StatusCode::BAD_GATEWAY);
    if status.is_redirection() {
        tracing::warn!("plain proxy upstream answered a redirect it could not follow");
        return StatusCode::BAD_GATEWAY.into_response();
    }
    if !status.is_success() {
        let mut out = HeaderMap::new();
        if let Some(value) = upstream.headers.get(header::CONTENT_RANGE) {
            out.insert(header::CONTENT_RANGE, value.clone());
        }
        return (status, out).into_response();
    }
    let mut out = HeaderMap::new();
    for name in PASSTHROUGH {
        if let Some(value) = upstream.headers.get(&name) {
            out.insert(name, value.clone());
        }
    }
    out.entry(header::ACCEPT_RANGES)
        .or_insert(HeaderValue::from_static("bytes"));
    // A page may read it as `<video crossorigin>`; the token is the capability.
    out.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    if head_only {
        return (status, out).into_response();
    }
    let chunks = stream::unfold(upstream, |mut upstream| async move {
        let chunk = upstream.next_chunk().await?;
        Some((chunk.map_err(std::io::Error::other), upstream))
    });
    (status, out, pumped(chunks, released)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_byte_ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 100)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 20)));
        assert_eq!(parse_range("bytes=90-500", 100), Some((90, 100)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 100)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=20-10", 100), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }

    #[tokio::test]
    async fn names_keep_their_extension() {
        let http = HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let server = StreamServer::start(http).await.unwrap();
        let upstream = Upstream::Http {
            url: "http://127.0.0.1:9/v.mkv".into(),
            headers: Vec::new(),
            addon_url: None,
        };
        let lease = server.register(upstream, "v.mkv");
        let slot = lease.subtitles();
        for lang in ["eng", "", "a/../b.srt", &"l".repeat(300)] {
            let url = slot.add("http://127.0.0.1:9/a.srt", lang, None).unwrap();
            assert!(url.ends_with(".vtt"), "{url}");
        }
    }
}
