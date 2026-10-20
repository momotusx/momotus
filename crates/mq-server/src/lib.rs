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
//!
//! An HLS or DASH stream is found at open and served as a manifest
//! rewritten on every request (`manifest`): each child it names is
//! `http://127.0.0.1:<port>/c/<token>/<id>/<name>`, served by the role that
//! named it, and each DASH base or template directory
//! `http://127.0.0.1:<port>/t/<token>/<id>/<tail>`, so a player never
//! fetches from an origin.

mod ahead;
mod manifest;
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
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::{Bytes, BytesMut};
use futures_util::{FutureExt, Stream, StreamExt, stream};
use manifest::{Children, Declared, Role, Sniffed};
use mq_http::{HttpClient, HttpError, Priority, RequestOptions, Url};
use mq_media::CancellationToken;
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
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
    /// A file of a torrent, read as the claim's profile: the claim lives as
    /// long as the playback.
    #[cfg(feature = "torrent")]
    Torrent {
        claim: mq_engine::Claim,
        file: usize,
    },
}

/// A subtitle file, served converted.
#[derive(Clone)]
struct Subtitle {
    lang: String,
    origin: Origin,
}

#[derive(Clone)]
enum Origin {
    /// An addon's file. `addon_url` is the install whose answer named it:
    /// its scope, no headers.
    Url { url: String, addon_url: Option<Url> },
    /// A file the core fetches itself, compared by `key`.
    Fetch { key: String, fetch: Fetch },
}

/// How the core fetches a file of a built-in source: its bytes, or `None`
/// when there are none, which the source has already reported.
pub type Fetch =
    Arc<dyn Fn() -> futures_util::future::BoxFuture<'static, Option<Vec<u8>>> + Send + Sync>;

impl PartialEq for Subtitle {
    fn eq(&self, other: &Self) -> bool {
        self.lang == other.lang
            && match (&self.origin, &other.origin) {
                (
                    Origin::Url { url, addon_url },
                    Origin::Url {
                        url: u,
                        addon_url: a,
                    },
                ) => (url, addon_url) == (u, a),
                (Origin::Fetch { key, .. }, Origin::Fetch { key: k, .. }) => key == k,
                _ => false,
            }
    }
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
    /// An HLS playlist or a DASH manifest, fetched fresh and rewritten for
    /// every request.
    Hls(Arc<Hls>),
    #[cfg(feature = "torrent")]
    Torrent {
        claim: mq_engine::Claim,
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
    /// Told when a subtitle could not be served (`SubtitleSlot::on_trouble`).
    trouble: Mutex<Option<Reporter>>,
    /// File offsets of `hev1` and `dvhe` types and the types they are
    /// served as, found by the first body of a file (`relabels`).
    relabel: tokio::sync::OnceCell<Scan>,
}

/// Why a subtitle was answered with an empty WebVTT instead of its cues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleTrouble {
    /// The file did not come this time: a timeout, the network, a 5xx, 408
    /// or 429.
    Unreachable,
    /// Asking again gives the same: gone (4xx), refused, too large, or no
    /// format `to_vtt` reads.
    Unreadable,
}

type Reporter = Arc<dyn Fn(&str, SubtitleTrouble) + Send + Sync>;

impl Playback {
    #[allow(irrefutable_let_patterns)] // one variant without `torrent`
    fn release(&self) {
        self.released.cancel();
        if let Content::Http(source) = &self.body {
            source.shutdown();
        }
    }
}

/// A manifest playback (HLS or DASH, `kind`): the stream URL's manifest
/// and the children its rewrites named, all fetched through the playback's
/// scoped handle.
struct Hls {
    url: Url,
    kind: Format,
    http: HttpClient,
    /// The stream's `proxyHeaders`: in full only to the stream URL's origin.
    headers: Vec<(String, String)>,
    /// `http://127.0.0.1:{port}/`, `{core}c/{token}/` where children go and
    /// `{core}t/{token}/` where DASH directories go.
    core: String,
    prefix: String,
    templates: String,
    /// Held by a rewrite for its whole run on a blocking thread; a child's
    /// lookup waits for it without holding a runtime worker, and no longer
    /// than a playlist request may take.
    table: Arc<tokio::sync::Mutex<manifest::Table>>,
    /// Tests only: runs as a rewrite starts on its blocking thread, holding
    /// the table, so a test can hold it there.
    #[cfg(test)]
    hook: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Hls {
    fn options(&self) -> RequestOptions {
        RequestOptions {
            priority: Priority::Playback,
            headers: self.headers.clone(),
            owner: Some(self.url.clone()),
            ..Default::default()
        }
    }
}

/// How a playback's main URL is served, when it is not a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// An HLS playlist: the main URL ends in `.m3u8`.
    Hls,
    /// A DASH manifest: the main URL ends in `.mpd`.
    Dash,
}

/// The attempt list's words for a manifest that cannot be played.
const UNREADABLE_PLAYLIST: &str = "The stream's playlist could not be read";
/// DASH on a player whose caps do not list `mpd`.
const NO_DASH: &str = "This player cannot play DASH streams";
/// HLS on a player whose caps do not list `m3u8`.
const NO_HLS: &str = "This player cannot play HLS streams";
/// A playlist's fetch, unless a blocking reload may wait longer.
const MANIFEST_DEADLINE: Duration = Duration::from_secs(10);

/// What the routes and every lease share.
struct Shared {
    http: HttpClient,
    playbacks: Mutex<HashMap<String, Arc<Playback>>>,
    /// Moves only when `revive` cannot get the old port back.
    addr: Mutex<SocketAddr>,
    logos: std::sync::OnceLock<&'static [Logo]>,
    /// One per server: as on every other route, a page cannot probe for
    /// the server without it.
    logo_token: String,
}

impl Shared {
    fn addr(&self) -> SocketAddr {
        *lock(&self.addr)
    }
}

/// A bundled image the apps show, such as a default addon's logo.
pub struct Logo {
    pub key: &'static str,
    pub mime: &'static str,
    pub bytes: &'static [u8],
}

pub struct StreamServer {
    shared: Arc<Shared>,
    /// Stops accepting and closes every connection, keep-alive ones too.
    shutdown: CancellationToken,
    app: Router,
    /// The accept loop on the current listener; none once it was closed
    /// and could not be bound again.
    accepting: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl StreamServer {
    /// Binds an ephemeral port on 127.0.0.1 only.
    pub async fn start(http: HttpClient) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let shared = Arc::new(Shared {
            http,
            playbacks: Mutex::default(),
            addr: Mutex::new(listener.local_addr()?),
            logos: std::sync::OnceLock::new(),
            logo_token: new_token(),
        });
        let app = Router::new()
            .route("/s/{token}/{name}", get(serve).head(serve))
            .route("/c/{token}/{id}/{name}", get(serve_child).head(serve_child))
            .route(
                "/t/{token}/{id}/{*tail}",
                get(serve_template).head(serve_template),
            )
            .route("/sub/{token}/{id}/{name}", get(serve_subtitle))
            .route("/logo/{token}/{key}", get(serve_logo))
            // On every path, the fallback's and wrong methods' included.
            .layer(axum::middleware::from_fn_with_state(
                shared.clone(),
                only_loopback,
            ))
            .with_state(shared.clone());
        let shutdown = CancellationToken::new();
        let accepting = accept(app.clone(), listener, shutdown.clone());
        Ok(Self {
            shared,
            shutdown,
            app,
            accepting: tokio::sync::Mutex::new(Some(accepting)),
        })
    }

    /// Binds again when the system took the listener: iOS and tvOS reclaim
    /// the listening sockets of a suspended app (Apple TN2277;
    /// plans/architecture.md 4.3). The same port first, so the URLs handed
    /// out keep working; another one when it is taken. True when the
    /// address moved: a player must open its URL again.
    pub async fn revive(&self) -> std::io::Result<bool> {
        let mut accepting = self.accepting.lock().await;
        let addr = self.shared.addr();
        let reached =
            tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(addr))
                .await;
        if matches!(reached, Ok(Ok(_))) {
            return Ok(false);
        }
        close(&mut accepting).await;
        let tcp = match tokio::net::TcpListener::bind(addr).await {
            Ok(tcp) => tcp,
            Err(_) => tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?,
        };
        let now = tcp.local_addr()?;
        *lock(&self.shared.addr) = now;
        *accepting = Some(accept(self.app.clone(), tcp, self.shutdown.clone()));
        tracing::info!(moved = now != addr, "stream server bound again");
        Ok(now != addr)
    }

    /// Tests: the listener goes, as when the system takes it.
    #[doc(hidden)]
    pub async fn close_listener(&self) {
        close(&mut *self.accepting.lock().await).await;
    }

    /// A playback that serves at once, with no check of its origin: for the
    /// CLI and examples. `filename` only helps players guess the container;
    /// it carries no secrets. An HTTP upstream starts reading at once.
    pub fn register(&self, upstream: Upstream, filename: &str) -> Lease {
        self.lease(
            new_token(),
            self.playback(upstream),
            &file_name(filename),
            None,
        )
    }

    /// A playback whose origin answered within `deadline`, so a dead link
    /// fails here instead of in the player. For a file that answer is the
    /// read that goes on to serve the player; no extra request is made. A
    /// body declared or sniffed as a playlist is fetched whole and rewritten
    /// once within the same deadline, and then served as one (`format`); a
    /// declared one that is not a playlist, and DASH, fail. The token exists
    /// only once all that is done: dropping this future first leaves none,
    /// and stops the reads. A player that plays HLS only: see `open_for`.
    pub async fn open(
        &self,
        upstream: Upstream,
        filename: &str,
        deadline: Duration,
    ) -> Result<Lease, String> {
        self.open_for(upstream, filename, deadline, &[Format::Hls])
            .await
    }

    /// `open` for a player that plays the formats `plays` names (its device
    /// caps): a playlist of another format fails with its words, whether
    /// its type or path declared it or its first bytes did.
    pub async fn open_for(
        &self,
        upstream: Upstream,
        filename: &str,
        deadline: Duration,
        plays: &[Format],
    ) -> Result<Lease, String> {
        let playback = self.playback(upstream);
        // The token goes in the map only at the end, but a playlist's dry run
        // writes it into the children's URLs, so the size limit sees them.
        let token = new_token();
        // Torrent playbacks are checked before they get here.
        #[allow(irrefutable_let_patterns)] // one variant without `torrent`
        let Content::Http(source) = &playback.body else {
            return Ok(self.lease(token, playback, &file_name(filename), None));
        };
        let source = source.clone();
        let end = Instant::now() + deadline;
        // Set once the source answered with a playlist (declared or sniffed):
        // from then on, too slow is the playlist's fault, not the source's.
        let playlist = AtomicBool::new(false);
        let detect = self.detect(&source, &token, &playback.released, end, &playlist, plays);
        match tokio::time::timeout_at(end.into(), detect).await {
            Ok(Ok(None)) => Ok(self.lease(token, playback, &file_name(filename), None)),
            Ok(Ok(Some(hls))) => {
                // The cursors stop: every request fetches the playlist fresh.
                playback.release();
                let kind = hls.kind;
                let playback = Arc::new(Playback {
                    body: Content::Hls(hls),
                    released: CancellationToken::new(),
                    subtitles: Mutex::default(),
                    trouble: Mutex::default(),
                    relabel: Default::default(),
                });
                let name = manifest_name(filename, kind);
                Ok(self.lease(token, playback, &name, Some(kind)))
            }
            Ok(Err(error)) => {
                playback.release();
                Err(error)
            }
            Err(_) => {
                playback.release();
                if playlist.load(Ordering::Relaxed) {
                    Err(UNREADABLE_PLAYLIST.into())
                } else {
                    Err(SILENT.into())
                }
            }
        }
    }

    /// A file (`None`) or a manifest, by the first answer's type, the
    /// URLs' paths and the body's first bytes (plans/c21-c23-loopback.md
    /// 3.3). A ranged origin's first bytes come from the read-ahead cache, a
    /// passthrough origin's from one fresh request; a video or audio type of
    /// a size no playlist has is a file at once, so files keep their start.
    async fn detect(
        &self,
        source: &ahead::Source,
        token: &str,
        released: &CancellationToken,
        end: Instant,
        playlist: &AtomicBool,
        plays: &[Format],
    ) -> Result<Option<Arc<Hls>>, String> {
        let listed = source.url().to_owned();
        let (declared, prefix) = match source.meta(0).await {
            Ok(meta) => {
                let content_type = header_text(&meta.headers);
                let urls = [listed.as_str(), &source.final_url()];
                let declared = manifest::declared(content_type, &urls);
                if declared.is_none() && !manifest::candidate(content_type, Some(meta.len)) {
                    return Ok(None);
                }
                playlist.store(declared.is_some(), Ordering::Relaxed);
                let want = meta.len.min(manifest::SNIFF as u64);
                let mut prefix = BytesMut::new();
                while (prefix.len() as u64) < want {
                    let more = source.read(prefix.len() as u64, want).await;
                    let more = more.map_err(|_| UNREACHABLE.to_owned())?;
                    if more.is_empty() {
                        break;
                    }
                    prefix.extend_from_slice(&more);
                }
                (declared, prefix.freeze())
            }
            Err(ahead::Refusal::Passthrough) => {
                let opts = source.options();
                let upstream = source.http().stream(&listed, None, &opts).await;
                let mut upstream = upstream.map_err(|e| error_words(&e))?;
                if !(200..300).contains(&upstream.status) {
                    let status = StatusCode::from_u16(upstream.status);
                    return Err(status.map_or(UNREACHABLE.to_owned(), refused));
                }
                let content_type = header_text(&upstream.headers);
                let urls = [listed.as_str(), upstream.final_url.as_str()];
                let declared = manifest::declared(content_type, &urls);
                if declared.is_none() && !manifest::candidate(content_type, None) {
                    return Ok(None);
                }
                playlist.store(declared.is_some(), Ordering::Relaxed);
                let mut prefix = BytesMut::new();
                while prefix.len() < manifest::SNIFF {
                    match upstream.next_chunk().await {
                        Some(Ok(chunk)) => prefix.extend_from_slice(&chunk),
                        // A body cut short is judged by what arrived.
                        Some(Err(_)) | None => break,
                    }
                }
                prefix.truncate(manifest::SNIFF);
                (declared, prefix.freeze())
            }
            Err(ahead::Refusal::Status(status, _)) => return Err(refused(status)),
            Err(ahead::Refusal::Blocked) => return Err(PRIVATE.into()),
            Err(ahead::Refusal::Failed(reason)) => {
                tracing::info!("stream source unreachable: {reason}");
                return Err(UNREACHABLE.into());
            }
        };
        let kind = match (manifest::sniff(&prefix), declared) {
            (Sniffed::Hls, None | Some(Declared::Hls)) => Format::Hls,
            (Sniffed::Dash, None | Some(Declared::Dash)) => Format::Dash,
            (Sniffed::File, None) => return Ok(None),
            // Declared but not that format, a DASH patch, a DOCTYPE: a
            // declared body is never served raw.
            _ => return Err(UNREADABLE_PLAYLIST.into()),
        };
        playlist.store(true, Ordering::Relaxed);
        if !plays.contains(&kind) {
            return Err(match kind {
                Format::Hls => NO_HLS.into(),
                Format::Dash => NO_DASH.into(),
            });
        }
        let url = Url::parse(&listed).map_err(|_| UNREADABLE_PLAYLIST.to_owned())?;
        let core = format!("http://{}/", self.shared.addr());
        let hls = Arc::new(Hls {
            url,
            kind,
            http: source.http().clone(),
            headers: source.headers().to_vec(),
            prefix: format!("{core}c/{token}/"),
            templates: format!("{core}t/{token}/"),
            core,
            table: Arc::default(),
            #[cfg(test)]
            hook: None,
        });
        let (body, base) = fetch_playlist(&hls, &hls.url, &[], MANIFEST_DEADLINE)
            .await
            .map_err(|e| match e {
                // The source answered with a playlist: too slow is the
                // playlist's.
                HttpError::Timeout(_) => UNREADABLE_PLAYLIST.to_owned(),
                e => error_words(&e),
            })?;
        // A dry run: the ids it registers stay valid for the real requests.
        let rewritten = rewrite(&hls, body, base, None, released, end).await;
        if let Err(reason) = rewritten {
            tracing::info!("stream playlist refused: {reason}");
            return Err(UNREADABLE_PLAYLIST.to_owned());
        }
        Ok(Some(hls))
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
            Upstream::Torrent { claim, file } => Content::Torrent { claim, file },
        };
        Arc::new(Playback {
            body,
            released: CancellationToken::new(),
            subtitles: Mutex::default(),
            trouble: Mutex::default(),
            relabel: Default::default(),
        })
    }

    /// `name` is already the main URL's name (`file_name`, `manifest_name`).
    fn lease(
        &self,
        token: String,
        playback: Arc<Playback>,
        name: &str,
        format: Option<Format>,
    ) -> Lease {
        lock(&self.shared.playbacks).insert(token.clone(), playback.clone());
        let url = format!("http://{}/s/{token}/{name}", self.shared.addr());
        Lease {
            url,
            format,
            token,
            playback,
            shared: self.shared.clone(),
        }
    }

    /// Playbacks with a live token.
    pub fn playbacks(&self) -> usize {
        lock(&self.shared.playbacks).len()
    }

    /// Serves `logos` from now on; set once, later calls change nothing.
    pub fn set_logos(&self, logos: &'static [Logo]) {
        let _ = self.shared.logos.set(logos);
    }

    /// The loopback URL of logo `key`, when it is one of `set_logos`'.
    pub fn logo_url(&self, key: &str) -> Option<String> {
        let known = self.shared.logos.get()?.iter().any(|l| l.key == key);
        known.then(|| {
            let port = self.shared.addr().port();
            format!(
                "http://127.0.0.1:{port}/logo/{}/{key}",
                self.shared.logo_token
            )
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.shared.addr()
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
/// Ends the accept loop; its listener is closed once this returns.
async fn close(accepting: &mut Option<tokio::task::JoinHandle<()>>) {
    if let Some(task) = accepting.take() {
        task.abort();
        let _ = task.await;
    }
}

/// Serves `app` on `tcp` until the server stops or `revive` replaces the
/// listener (dropping this task closes only the listener: connections run
/// on their own tasks).
fn accept(
    app: Router,
    tcp: tokio::net::TcpListener,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let stop = shutdown.clone().cancelled_owned();
    let listener = Loopback { tcp, shutdown };
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(stop)
            .await;
    })
}

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
    /// `None` for a file.
    pub format: Option<Format>,
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
            base: format!("http://{}/sub/{}", self.shared.addr(), self.token),
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

    /// The connection's capacity as this playback measured it
    /// (plans/m13-stream-choice.md 5): bytes received while the origin's
    /// answer was awaited, and that time, in its first 30 s. `None` for a
    /// torrent (the engine counts its own), a manifest, or an origin that
    /// ignores ranges.
    pub fn sample(&self) -> Option<(u64, std::time::Duration)> {
        match &self.playback.body {
            Content::Http(source) if !source.passthrough() => Some(source.sample()),
            _ => None,
        }
    }

    /// Whether the file has an `hev1` or `dvhe` entry with no parameter
    /// sets: Apple shows no picture from it, even relabelled. The same scan
    /// as the relabel (one per playback, an MP4 or MOV file only).
    pub fn picture_missing_on_apple(&self) -> futures_util::future::BoxFuture<'static, bool> {
        let playback = self.playback.clone();
        Box::pin(async move { scan(&playback).await.bare })
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
        let origin = Origin::Url {
            url: url.to_owned(),
            addon_url: addon_url.cloned(),
        };
        self.push(lang, origin)
    }

    /// The URL that serves what `fetch` gives as UTF-8 WebVTT, the same for
    /// the same `key`. `fetch` runs on every request of the URL, ends with
    /// the playback, and reports its own failures: the route answers them
    /// with an empty WebVTT and reports only a file it cannot read.
    pub fn add_fetched(&self, key: &str, lang: &str, fetch: Fetch) -> Option<String> {
        let origin = Origin::Fetch {
            key: key.to_owned(),
            fetch,
        };
        self.push(lang, origin)
    }

    fn push(&self, lang: &str, origin: Origin) -> Option<String> {
        let playback = self.playback.upgrade()?;
        if playback.released.is_cancelled() {
            return None;
        }
        let subtitle = Subtitle {
            lang: lang.to_owned(),
            origin,
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

    /// `report` hears of every subtitle of this playback that could not be
    /// served, with the URL `add` gave for it. The route answers those with
    /// an empty WebVTT, so no player ever sees a subtitle error: the
    /// playback says what happened instead.
    pub fn on_trouble(&self, report: impl Fn(&str, SubtitleTrouble) + Send + Sync + 'static) {
        if let Some(playback) = self.playback.upgrade() {
            *lock(&playback.trouble) = Some(Arc::new(report));
        }
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

/// A URL's last part as a name: safe characters only, at most 120 of them,
/// cut in the stem so the extension players read stays.
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
        .collect();
    // Every character is ASCII now, so byte offsets are character offsets.
    let clean = match clean.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 16 => {
            let keep = 120 - ext.len() - 1;
            format!("{}.{ext}", &stem[..stem.len().min(keep)])
        }
        _ => clean[..clean.len().min(120)].to_owned(),
    };
    if clean.is_empty() {
        "stream".into()
    } else {
        clean
    }
}

/// Extensions players take for a playlist: Media3 by its source factory
/// (`.ism`, `.isml` for SmoothStreaming, which the app does not ship), mpv by
/// its headerless M3U probe.
const PLAYLIST_EXTENSIONS: [&str; 7] = ["m3u", "m3u8", "mpd", "strm", "pls", "ism", "isml"];

/// A file's main URL name: a playlist extension becomes `.bin`, so no
/// player takes the file for a playlist.
fn file_name(filename: &str) -> String {
    let name = sanitize(filename);
    match name.rsplit_once('.') {
        Some((stem, ext)) if PLAYLIST_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) => {
            format!("{stem}.bin")
        }
        _ => name,
    }
}

/// A manifest playback's main URL name: Media3 picks the source by `.m3u8`
/// or `.mpd`.
fn manifest_name(filename: &str, kind: Format) -> String {
    let name = file_name(filename);
    let stem = name
        .rsplit_once('.')
        .map_or(name.as_str(), |(stem, _)| stem);
    match kind {
        Format::Hls => format!("{stem}.m3u8"),
        Format::Dash => format!("{stem}.mpd"),
    }
}

/// A response's `Content-Type`.
fn header_text(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::CONTENT_TYPE)?.to_str().ok()
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

/// DNS rebinding and proxy guard, before anything else on every path: only
/// a request addressed to this server is answered, by its one `Host` and by
/// the authority of an absolute-form or `CONNECT` target (a player sending
/// everything through the core as its proxy). Any other gets 403, and one
/// debug line that names no address (a hostile page can send many).
async fn only_loopback(
    State(shared): State<Arc<Shared>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let port = shared.addr().port();
    let ours = |h: &str| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}");
    // Exactly one: with two, a front end and this check could read
    // different ones.
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let host = hosts.next().filter(|_| hosts.next().is_none());
    let host = host.and_then(|h| h.to_str().ok());
    let authority = request.uri().authority().map(|a| a.as_str());
    if host.is_some_and(ours) && authority.is_none_or(ours) {
        return next.run(request).await;
    }
    tracing::debug!("refused a request for another host");
    StatusCode::FORBIDDEN.into_response()
}

/// A logo of `set_logos`, under this server's logo token; anything else
/// is not found.
async fn serve_logo(
    State(shared): State<Arc<Shared>>,
    Path((token, key)): Path<(String, String)>,
) -> Response {
    let logo = shared
        .logos
        .get()
        .and_then(|l| l.iter().find(|l| l.key == key));
    match logo {
        Some(logo) if token == shared.logo_token => (
            [
                (header::CONTENT_TYPE, logo.mime),
                (header::CACHE_CONTROL, "private, max-age=86400"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            logo.bytes,
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
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
) -> Response {
    let Some(playback) = playback(&shared, &token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let subtitle = parse_id(&id).and_then(|id| lock(&playback.subtitles).get(id).cloned());
    let Some(subtitle) = subtitle else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let track = format!(
        "http://{}/sub/{token}/{id}/{}.vtt",
        shared.addr(),
        sanitize(&subtitle.lang)
    );
    let read =
        |bytes: &[u8]| to_vtt(bytes, &subtitle.lang).ok_or(Some(SubtitleTrouble::Unreadable));
    // Release ends the fetch too. `Err(None)`: its source said it already.
    let vtt = match subtitle.origin {
        Origin::Url { url, addon_url } => {
            let opts = RequestOptions {
                priority: Priority::Playback,
                timeout: Duration::from_secs(20),
                ..Default::default()
            };
            let http = shared.http.scoped(&scope(addon_url.as_ref()));
            match playback
                .released
                .run_until_cancelled(http.get(&url, &opts))
                .await
            {
                None => return StatusCode::NOT_FOUND.into_response(),
                Some(Ok(response)) => read(&response.body),
                Some(Err(e)) => {
                    tracing::warn!("subtitle upstream failed: {}", e.kind());
                    Err(Some(trouble(&e)))
                }
            }
        }
        Origin::Fetch { fetch, .. } => match playback.released.run_until_cancelled(fetch()).await {
            None => return StatusCode::NOT_FOUND.into_response(),
            Some(Some(bytes)) => read(&bytes),
            Some(None) => Err(None),
        },
    };
    let vtt = vtt.unwrap_or_else(|trouble| {
        let report = lock(&playback.trouble).clone();
        if let Some((report, trouble)) = report.zip(trouble) {
            report(&track, trouble);
        }
        // An empty track, never an error: a player that meets an error on a
        // subtitle may stop or fail over the video.
        EMPTY_VTT.to_owned()
    });
    cors(([(header::CONTENT_TYPE, "text/vtt; charset=utf-8")], vtt).into_response())
}

/// What a subtitle that could not be served answers.
const EMPTY_VTT: &str = "WEBVTT\n\n";

fn trouble(e: &HttpError) -> SubtitleTrouble {
    match e {
        HttpError::Timeout(_) | HttpError::Network(_) => SubtitleTrouble::Unreachable,
        HttpError::Status { status, .. } if *status >= 500 || matches!(status, 408 | 429) => {
            SubtitleTrouble::Unreachable
        }
        // Gone, refused, too large, or an address it may not reach: asking
        // again gives the same.
        _ => SubtitleTrouble::Unreadable,
    }
}

async fn serve(
    State(shared): State<Arc<Shared>>,
    Path((token, _name)): Path<(String, String)>,
    request: Request<Body>,
) -> Response {
    let Some(playback) = playback(&shared, &token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let range = range_of(&request);
    let head_only = request.method() == Method::HEAD;
    match &playback.body {
        Content::Http(source) => {
            let relabel = relabels(playback.clone());
            serve_http(source.clone(), range, head_only, relabel).await
        }
        Content::Hls(hls) => {
            let query = request.uri().query();
            let main = hls.url.clone();
            cors(serve_playlist(&playback, hls, None, main, query).await)
        }
        // A web page plays it in a `<video crossorigin>`, as it does the others.
        #[cfg(feature = "torrent")]
        Content::Torrent { claim, file } => {
            let released = playback.released.clone();
            let relabel = relabels(playback.clone());
            cors(torrent::serve(claim, *file, range.as_deref(), head_only, released, relabel).await)
        }
    }
}

/// Reads for a file's `hev1` entries. A torrent's MP4 is Ready only once its
/// whole `moov` is verified (mq-engine-sched README 12), so the scan reads it
/// in one go from disk: the budget fits a large `moov` (Tenzing's is
/// 5.07 MiB; a long 4K remux's tens of MiB).
const RELABEL_BUDGET: mq_media::ProbeBudget = mq_media::ProbeBudget {
    max_bytes: 64 << 20,
    max_time: Duration::from_secs(30),
};

/// A four-byte sample entry type at a file offset, and the type served.
type Relabelled = (u64, [u8; 4]);

/// What the scan of a file's sample entries found.
#[derive(Clone, Default)]
struct Scan {
    /// The entries to serve under the type Apple decodes.
    relabel: Arc<[Relabelled]>,
    /// An `hev1` or `dvhe` entry whose `hvcC` has no parameter sets:
    /// Apple shows no picture from it in any form.
    bare: bool,
}

/// A body's relabels, awaited before its first bytes: the headers never
/// wait for the scan, and a HEAD request never starts it.
type Relabel = futures_util::future::BoxFuture<'static, Arc<[Relabelled]>>;

/// The offsets of the file's `hev1` types to serve as `hvc1`: Apple's
/// AVFoundation decodes HEVC only from an `hvc1` sample entry and plays an
/// `hev1` file's sound with no picture. Found once per playback, by the
/// first body sent, wherever `moov` sits; only entries whose `hvcC` holds
/// the parameter sets, since the label does not add them.
fn relabels(playback: Arc<Playback>) -> Relabel {
    Box::pin(async move { scan(&playback).await.relabel })
}

async fn scan(playback: &Playback) -> Scan {
    let found = playback.relabel.get_or_init(|| async {
        let reader: Box<dyn mq_media::RangeReader> = match &playback.body {
            Content::Http(source) if !source.passthrough() => {
                Box::new(ahead::Reader(source.clone()))
            }
            #[cfg(feature = "torrent")]
            // A torrent's file names are its own: only an MP4 or MOV one is
            // read, so other files never wait on the scan's pieces.
            Content::Torrent { claim, file } => match claim.open_reader(*file).await {
                Ok(reader) if torrent::iso_bmff(&reader.file().path) => {
                    Box::new(torrent::TorrentBytes(reader))
                }
                _ => return Scan::default(),
            },
            _ => return Scan::default(),
        };
        let cancel = playback.released.clone();
        let entries = mq_media::hev1_entries(reader.as_ref(), RELABEL_BUDGET, &cancel).await;
        let bare = entries.iter().any(|e| !e.parameter_sets);
        if bare {
            tracing::warn!("an hev1 video entry without parameter sets: no picture on Apple");
        }
        Scan {
            relabel: entries
                .iter()
                .filter(|e| e.parameter_sets)
                .map(|e| (e.fourcc_at, e.serve_as))
                .collect(),
            bare,
        }
    });
    found.await.clone()
}

/// Writes each relabel's type over the one at its offset, for the part
/// that falls in `chunk`, which starts at file offset `pos`: a range may
/// hold part of one.
fn relabel_chunk(chunk: &mut [u8], pos: u64, relabel: &[Relabelled]) {
    for &(at, kind) in relabel {
        for (i, &byte) in kind.iter().enumerate() {
            let slot = (at + i as u64)
                .checked_sub(pos)
                .and_then(|r| usize::try_from(r).ok())
                .and_then(|r| chunk.get_mut(r));
            if let Some(slot) = slot {
                *slot = byte;
            }
        }
    }
}

/// `bytes` from file offset `pos` as served: the same buffer when no
/// relabelled type falls in it.
fn relabelled(bytes: Bytes, pos: u64, relabel: &[Relabelled]) -> Bytes {
    let end = pos + bytes.len() as u64;
    if !relabel.iter().any(|&(at, _)| at < end && at + 4 > pos) {
        return bytes;
    }
    let mut owned = bytes.to_vec();
    relabel_chunk(&mut owned, pos, relabel);
    Bytes::from(owned)
}

/// A child of a manifest playback, by the role that named it: a media
/// playlist or DASH manifest rewritten, media bytes with `Range`, or a small
/// key or time read whole. Every answer may be read by any page; the token
/// is the capability.
async fn serve_child(
    State(shared): State<Arc<Shared>>,
    Path((token, id, _name)): Path<(String, String, String)>,
    request: Request<Body>,
) -> Response {
    let Some(playback) = playback(&shared, &token) else {
        return cors(StatusCode::NOT_FOUND.into_response());
    };
    let Content::Hls(hls) = &playback.body else {
        return cors(StatusCode::NOT_FOUND.into_response());
    };
    let Some(id) = parse_id(&id).and_then(|id| u32::try_from(id).ok()) else {
        return cors(StatusCode::NOT_FOUND.into_response());
    };
    let found = match table_within(hls, &playback.released, MANIFEST_DEADLINE).await {
        Ok(mut table) => table.get(id),
        Err(status) => return cors(status.into_response()),
    };
    let Some((url, role)) = found else {
        return cors(StatusCode::NOT_FOUND.into_response());
    };
    let released = playback.released.clone();
    let out = match role {
        Role::Playlist | Role::Dash => {
            let query = request.uri().query();
            serve_playlist(&playback, hls, Some(id), url, query).await
        }
        Role::Media => {
            let range = range_of(&request);
            let head_only = request.method() == Method::HEAD;
            proxy(
                &hls.http,
                url.as_str(),
                hls.options(),
                released,
                range,
                head_only,
            )
            .await
        }
        Role::Small => serve_small(hls, &url, &released).await,
    };
    cors(out)
}

/// A file under a DASH base or template directory: `tail` (the raw path,
/// never decoded) on the entry's origin and prefix, with `Range`.
async fn serve_template(
    State(shared): State<Arc<Shared>>,
    Path((token, id, _tail)): Path<(String, String, String)>,
    request: Request<Body>,
) -> Response {
    let Some(playback) = playback(&shared, &token) else {
        return cors(StatusCode::NOT_FOUND.into_response());
    };
    let Content::Hls(hls) = &playback.body else {
        return cors(StatusCode::NOT_FOUND.into_response());
    };
    let Some(id) = parse_id(&id).and_then(|id| u32::try_from(id).ok()) else {
        return cors(StatusCode::NOT_FOUND.into_response());
    };
    // `/t/{token}/{id}/` and then the tail, as the player sent it.
    let tail = request.uri().path().splitn(5, '/').nth(4).unwrap_or("");
    let found = match table_within(hls, &playback.released, MANIFEST_DEADLINE).await {
        Ok(mut table) => table.template(id),
        Err(status) => return cors(status.into_response()),
    };
    let url = found.and_then(|(origin, prefix, query)| {
        manifest::template_url(
            &origin,
            &prefix,
            query.as_deref(),
            tail,
            request.uri().query(),
        )
    });
    let Some(url) = url else {
        return cors(StatusCode::NOT_FOUND.into_response());
    };
    let range = range_of(&request);
    let head_only = request.method() == Method::HEAD;
    let released = playback.released.clone();
    let opts = hls.options();
    cors(proxy(&hls.http, url.as_str(), opts, released, range, head_only).await)
}

/// Lets any page read the answer (`<video crossorigin>` and Safari's HLS).
fn cors(mut response: Response) -> Response {
    allow_any_origin(response.headers_mut());
    response
}

/// `Access-Control-Allow-Origin: *`: the token in the path is the capability.
fn allow_any_origin(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
}

/// The request's `Range` header.
fn range_of(request: &Request<Body>) -> Option<String> {
    let range = request.headers().get(header::RANGE)?;
    range.to_str().ok().map(str::to_owned)
}

/// The request's LL-HLS directives (`_HLS_msn`, `_HLS_part`, `_HLS_skip`),
/// the only part of a request's query passed on to a playlist's origin.
fn hls_directives(query: Option<&str>) -> Vec<(String, String)> {
    let Some(query) = query else {
        return Vec::new();
    };
    let mut parsed = Url::parse("http://q/").expect("a valid URL");
    parsed.set_query(Some(query));
    parsed
        .query_pairs()
        .filter(|(name, _)| name.starts_with("_HLS_"))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// A playlist fetched whole (at most `MANIFEST_MAX`), with `directives`
/// added to its query; and the final URL it came from, its base.
async fn fetch_playlist(
    hls: &Hls,
    url: &Url,
    directives: &[(String, String)],
    deadline: Duration,
) -> Result<(Bytes, Url), HttpError> {
    let mut url = url.clone();
    if !directives.is_empty() {
        let mut query = url.query_pairs_mut();
        for (name, value) in directives {
            query.append_pair(name, value);
        }
    }
    let opts = RequestOptions {
        timeout: deadline,
        max_bytes: manifest::MANIFEST_MAX,
        ..hls.options()
    };
    let response = hls.http.get(url.as_str(), &opts).await?;
    Ok((response.body, response.final_url))
}

/// The largest target duration a blocking reload's wait counts: three of
/// them, 90 s, is longer than an origin should hold a reload.
const TARGET_MAX: u64 = 30;

/// How long a playlist request may take: `MANIFEST_DEADLINE`, or for a
/// blocking reload three of its target durations, each at most
/// `TARGET_MAX`; an origin's huge value would overflow the deadline.
fn reload_deadline(target: Option<u64>) -> Duration {
    let wait = target.map_or(0, |target| target.min(TARGET_MAX) * 3);
    MANIFEST_DEADLINE.max(Duration::from_secs(wait))
}

/// A playlist (`id`: a child's, `None`: the main one) fetched fresh and
/// rewritten: never cached, as a live one changes with every reload. A
/// blocking reload (`_HLS_msn`) may wait three of its target durations.
async fn serve_playlist(
    playback: &Playback,
    hls: &Arc<Hls>,
    id: Option<u32>,
    url: Url,
    query: Option<&str>,
) -> Response {
    // The request's clock starts here: a wait for the table counts against
    // its deadline (a lookup never waits past `MANIFEST_DEADLINE`).
    let started = Instant::now();
    let directives = match hls.kind {
        Format::Hls => hls_directives(query),
        Format::Dash => Vec::new(),
    };
    let blocking = directives.iter().any(|(name, _)| name == "_HLS_msn");
    let target = if blocking {
        match table_within(hls, &playback.released, MANIFEST_DEADLINE).await {
            Ok(table) => table.target(id),
            Err(status) => return status.into_response(),
        }
    } else {
        None
    };
    let end = started + reload_deadline(target);
    let left = end.saturating_duration_since(Instant::now());
    let fetch = fetch_playlist(hls, &url, &directives, left);
    let (body, base) = match playback.released.run_until_cancelled(fetch).await {
        None => return StatusCode::NOT_FOUND.into_response(),
        Some(Ok(fetched)) => fetched,
        Some(Err(e)) => {
            tracing::warn!("playlist upstream failed: {}", e.kind());
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let released = &playback.released;
    let rewritten = rewrite(hls, body, base, id, released, end).await;
    let rewritten = match rewritten {
        Ok(rewritten) => rewritten,
        Err(reason) => {
            tracing::warn!("playlist refused: {reason}");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let content_type = match hls.kind {
        Format::Hls => "application/vnd.apple.mpegurl",
        Format::Dash => "application/dash+xml",
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store"),
        ],
        rewritten.text,
    )
        .into_response()
}

/// The playback's table, for a request that reads it: the wait for a
/// rewrite that holds it ends at `bound` (503) or at release (404).
async fn table_within<'a>(
    hls: &'a Hls,
    released: &CancellationToken,
    bound: Duration,
) -> Result<tokio::sync::MutexGuard<'a, manifest::Table>, StatusCode> {
    let table = tokio::time::timeout(bound, hls.table.lock());
    match released.run_until_cancelled(table).await {
        None => Err(StatusCode::NOT_FOUND),
        Some(Err(_)) => Err(StatusCode::SERVICE_UNAVAILABLE),
        Some(Ok(table)) => Ok(table),
    }
}

/// The table guard on its way to a rewrite's blocking work. The work takes
/// it when it starts; until then it stays here, and when the request gives
/// up (its deadline, release, or a dropped request) this takes it back, so
/// work still queued for a blocking thread never holds the table.
struct Handoff(Arc<Mutex<Option<tokio::sync::OwnedMutexGuard<manifest::Table>>>>);

impl Drop for Handoff {
    fn drop(&mut self) {
        lock(&self.0).take();
    }
}

/// Rewrites a fetched playlist (`id`: a child's, `None`: the main one) on a
/// blocking thread, holding the playback's table, and gives up at `end`, at
/// release, or when the request is dropped: the rewrite is CPU work no
/// `await` interrupts, so it checks for those itself and runs where it holds
/// no runtime worker.
async fn rewrite(
    hls: &Arc<Hls>,
    body: Bytes,
    base: Url,
    id: Option<u32>,
    released: &CancellationToken,
    end: Instant,
) -> Result<manifest::Rewritten, &'static str> {
    let deadline = tokio::time::Instant::from_std(end);
    let table = tokio::time::timeout_at(deadline, hls.table.clone().lock_owned());
    let table = match released.run_until_cancelled(table).await {
        None => return Err("released"),
        Some(Err(_)) => return Err("out of time"),
        Some(Ok(table)) => table,
    };
    let slot = Arc::new(Mutex::new(Some(table)));
    let _handoff = Handoff(slot.clone());
    // Cancelled when this future ends, however it ends: at the deadline
    // (`timeout_at` below returns), at release, or with the request dropped.
    let gone = CancellationToken::new();
    let _gone = gone.clone().drop_guard();
    let hls = hls.clone();
    let release = released.clone();
    let work = tokio::task::spawn_blocking(move || {
        let Some(mut table) = lock(&slot).take() else {
            return Err("out of time");
        };
        #[cfg(test)]
        if let Some(hook) = &hls.hook {
            hook();
        }
        let alive = || !release.is_cancelled() && !gone.is_cancelled();
        let mut children = Children {
            core: &hls.core,
            prefix: &hls.prefix,
            templates: &hls.templates,
            table: &mut table,
            alive: &alive,
        };
        let rewritten = match hls.kind {
            Format::Hls => manifest::rewrite_hls(&body, &base, id.is_some(), &mut children)?,
            Format::Dash => manifest::rewrite_dash(&body, &base, &mut children)?,
        };
        if let Some(target) = rewritten.target_duration {
            table.set_target(id, target);
        }
        Ok(rewritten)
    });
    // Work still queued for a blocking thread cannot look at `released`:
    // release ends the wait here, and `_handoff` takes the table back.
    let work = tokio::time::timeout_at(deadline, work);
    match released.run_until_cancelled(work).await {
        None => Err("released"),
        Some(Ok(Ok(rewritten))) => rewritten,
        Some(Ok(Err(_))) => Err("the rewrite failed"),
        Some(Err(_)) => Err("out of time"),
    }
}

/// A key, read whole: at most `SMALL_MAX`, else 502.
async fn serve_small(hls: &Hls, url: &Url, released: &CancellationToken) -> Response {
    let opts = RequestOptions {
        timeout: MANIFEST_DEADLINE,
        max_bytes: manifest::SMALL_MAX,
        ..hls.options()
    };
    let fetch = hls.http.get(url.as_str(), &opts);
    match released.run_until_cancelled(fetch).await {
        None => StatusCode::NOT_FOUND.into_response(),
        // Key bytes for a player, never a page: any page may read the
        // answer (`cors`), so it gets no type of the origin's choosing.
        Some(Ok(response)) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            response.body,
        )
            .into_response(),
        Some(Err(e)) => {
            tracing::warn!("key upstream failed: {}", e.kind());
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

/// The attempt list's words for a failed fetch at open.
fn error_words(e: &HttpError) -> String {
    match e {
        HttpError::Status { status, .. } => {
            StatusCode::from_u16(*status).map_or(UNREACHABLE.to_owned(), refused)
        }
        HttpError::BlockedPrivate(_) => PRIVATE.into(),
        HttpError::Timeout(_) => SILENT.into(),
        HttpError::TooLarge { .. } => UNREADABLE_PLAYLIST.into(),
        _ => {
            tracing::info!("stream source unreachable: {}", e.kind());
            UNREACHABLE.into()
        }
    }
}

const UNREACHABLE: &str = "The source could not be reached";
const SILENT: &str = "The source did not answer in time";
const PRIVATE: &str = "The source points to a private network address";

/// A source's error answer in plain words for the attempt list, with the
/// status kept for support.
fn refused(status: StatusCode) -> String {
    status_words(status.as_u16())
}

/// An HTTP status in the attempt list's words, with the status kept for
/// support: what the stream server says at open, and what a player's
/// `failOver` with that status says.
pub fn status_words(status: u16) -> String {
    let words = match status {
        404 | 410 => "The link no longer exists",
        401 | 403 => "The source refused access",
        429 => "The source is busy, try again soon",
        500..=599 => "The source had an error",
        _ => "The source answered with an error",
    };
    format!("{words} ({status})")
}

/// Why a player gave a source up, in the attempt list's words, from what its
/// `failOver` reports: `kind` (`http` with `status`, `network`, `timeout`,
/// `unreadable`, `decode`, `audio`, `other`) and the player's own `error`.
/// The stream server's sentences where it has one for the same cause. With
/// no kind (an older client), `other` or one not known here, `error` stays
/// as the player wrote it.
pub fn failure_words(kind: Option<&str>, status: Option<u16>, error: &str) -> String {
    match (kind, status) {
        (Some("http"), Some(status)) => status_words(status),
        (Some("http"), None) => "The source answered with an error".into(),
        (Some("network"), _) => UNREACHABLE.into(),
        (Some("timeout"), _) => SILENT.into(),
        (Some("unreadable"), _) => "The file could not be read".into(),
        (Some("decode"), _) => "This device cannot decode the video".into(),
        (Some("audio"), _) => "The audio output failed".into(),
        _ => error.to_owned(),
    }
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

    /// An MP4 or MOV file by its name in the torrent.
    pub(crate) fn iso_bmff(name: &str) -> bool {
        matches!(content_type(name), "video/mp4" | "video/quicktime")
    }

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
        claim: &mq_engine::Claim,
        file: usize,
        range: Option<&str>,
        head_only: bool,
        released: CancellationToken,
        relabel: Relabel,
    ) -> Response {
        let answer = open(claim, file, range, head_only, released.clone(), relabel);
        released
            .run_until_cancelled(answer)
            .await
            .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
    }

    async fn open(
        claim: &mq_engine::Claim,
        file: usize,
        range: Option<&str>,
        head_only: bool,
        released: CancellationToken,
        relabel: Relabel,
    ) -> Response {
        let reader = match claim.open_reader(file).await {
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
        let chunks = async move { chunks(body, start, relabel.await) }.flatten_stream();
        (status, out, pumped(chunks, released)).into_response()
    }

    /// `body`'s reads from file offset `start`, `relabel` applied.
    pub(crate) fn chunks<R>(
        body: R,
        start: u64,
        relabel: Arc<[Relabelled]>,
    ) -> impl Stream<Item = std::io::Result<bytes::Bytes>> + Send + 'static
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        stream::unfold(Some((body, start)), move |state| {
            let relabel = relabel.clone();
            async move {
                use tokio::io::AsyncReadExt;
                let (mut body, pos) = state?;
                let mut buf = vec![0u8; 64 * 1024];
                match body.read(&mut buf).await {
                    Ok(0) => None,
                    Ok(n) => {
                        buf.truncate(n);
                        relabel_chunk(&mut buf, pos, &relabel);
                        Some((Ok(bytes::Bytes::from(buf)), Some((body, pos + n as u64))))
                    }
                    Err(e) => Some((Err(e), None)),
                }
            }
        })
    }

    /// A file of the engine for the container scan: reads wait for pieces.
    pub(crate) struct TorrentBytes(pub(crate) mq_engine::StreamReader);

    impl mq_media::RangeReader for TorrentBytes {
        fn len(&self) -> futures_util::future::BoxFuture<'_, Result<u64, mq_media::ProbeError>> {
            let len = self.0.len();
            Box::pin(async move { Ok(len) })
        }

        fn read_at(
            &self,
            offset: u64,
            len: u64,
        ) -> futures_util::future::BoxFuture<'_, Result<bytes::Bytes, mq_media::ProbeError>>
        {
            Box::pin(async move {
                use tokio::io::AsyncReadExt;
                let failed = |e: String| mq_media::ProbeError::Http(e);
                let mut range = self
                    .0
                    .range(offset, Some(offset.saturating_add(len)))
                    .await
                    .map_err(|e| failed(e.to_string()))?;
                let mut out = Vec::new();
                range
                    .read_to_end(&mut out)
                    .await
                    .map_err(|e| failed(e.to_string()))?;
                Ok(bytes::Bytes::from(out))
            })
        }
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
            let claim = s.leecher.claim(t, "default").unwrap();
            let response = serve(
                &claim,
                file,
                Some("bytes=0-"),
                false,
                released,
                Box::pin(async { Arc::from([]) }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let stats = s.leecher.stats(t).await.unwrap();
            assert_eq!((stats.readers, stats.pieces_total), (0, 0));
        }

        /// A release while a range's setup waits for the torrent's
        /// initialization ends the request at once and drops the range;
        /// initialization going on later brings no demand back.
        #[cfg(feature = "test-hooks")]
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_release_during_setup_ends_it() {
            let s = support::swarm(true).await;
            let t = s
                .leecher
                .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
                .await
                .unwrap();
            let hint = mq_engine::FileHint::Name("movie.mkv".into());
            let file = s.leecher.select_file(t, "default", &hint).await.unwrap();
            let hold = mq_engine::test_hooks::hold_ranges(&s.leecher);
            let released = CancellationToken::new();
            let request = tokio::spawn({
                let claim = s.leecher.claim(t, "default").unwrap();
                let released = released.clone();
                async move {
                    let range = Some("bytes=0-");
                    let relabel: Relabel = Box::pin(async { Arc::from([]) });
                    serve(&claim, file, range, false, released, relabel)
                        .await
                        .status()
                }
            });
            let reached = tokio::time::timeout(Duration::from_secs(5), hold.reached()).await;
            assert!(reached.is_ok(), "the setup never reached the barrier");
            let readers = || async { s.leecher.stats(t).await.unwrap().readers };
            assert_eq!(readers().await, 1, "the range's demand is open");
            released.cancel();
            let ended = tokio::time::timeout(Duration::from_secs(1), request).await;
            let status = ended.expect("the setup outlived its release").unwrap();
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(readers().await, 0);
            hold.open();
            tokio::time::sleep(Duration::from_millis(300)).await;
            assert_eq!(readers().await, 0, "demand came back");
        }
    }
}

/// Serves a range from the playback's read-ahead cache, `relabel` applied.
async fn serve_http(
    source: Arc<ahead::Source>,
    range: Option<String>,
    head_only: bool,
    relabel: Relabel,
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
    allow_any_origin(&mut out);
    let (status, start, end) = match range_headers(range.as_deref(), meta.len, &mut out) {
        Some(span) => span,
        None => return (StatusCode::RANGE_NOT_SATISFIABLE, out).into_response(),
    };
    if head_only {
        return (status, out).into_response();
    }
    let body = async move {
        let relabel = relabel.await;
        stream::unfold(Some(start), move |pos| {
            let (source, relabel) = (source.clone(), relabel.clone());
            async move {
                let pos = pos.filter(|&p| p < end)?;
                match source.read(pos, end).await {
                    Ok(bytes) => {
                        let next = pos + bytes.len() as u64;
                        Some((Ok(relabelled(bytes, pos, &relabel)), Some(next)))
                    }
                    Err(e) => Some((Err(e), None)),
                }
            }
        })
    }
    .flatten_stream();
    (status, out, Body::from_stream(body)).into_response()
}

/// Plain proxy with Range passthrough, for origins that ignore ranges or
/// give no length.
async fn proxy_http(source: &ahead::Source, range: Option<String>, head_only: bool) -> Response {
    let released = source.released().clone();
    let (http, url) = (source.http(), source.url());
    proxy(http, url, source.options(), released, range, head_only).await
}

/// `url` through `http` with `Range` passed through, its body ending at
/// release. A redirect the client did not follow answers 502, and an error
/// status goes on with its `Content-Range` but without its body. HEAD makes
/// the same request and drops it after the headers.
async fn proxy(
    http: &HttpClient,
    url: &str,
    opts: RequestOptions,
    released: CancellationToken,
    range: Option<String>,
    head_only: bool,
) -> Response {
    let open = tokio::time::timeout(ahead::OPEN, http.stream(url, range, &opts));
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
    allow_any_origin(&mut out);
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

    /// A file whose `hev1` type sits at offset 10, served in every split:
    /// each piece gets the part of `hvc1` that falls in it.
    #[test]
    fn a_relabel_lands_in_whatever_part_of_it_a_chunk_holds() {
        let file: Vec<u8> = [&[7u8; 10][..], b"hev1", &[9u8; 10]].concat();
        let mut want = file.clone();
        want[10..14].copy_from_slice(b"hvc1");
        let relabel = [(10u64, *b"hvc1")];
        for cut in 0..=file.len() {
            for second in cut..=file.len() {
                let mut served = Vec::new();
                for (from, to) in [(0, cut), (cut, second), (second, file.len())] {
                    let mut chunk = file[from..to].to_vec();
                    relabel_chunk(&mut chunk, from as u64, &relabel);
                    served.extend(chunk);
                }
                assert_eq!(served, want, "cuts at {cut} and {second}");
            }
        }
        let untouched = Bytes::from_static(b"abcdef");
        let same = relabelled(untouched.clone(), 100, &relabel);
        assert_eq!(
            same.as_ptr(),
            untouched.as_ptr(),
            "no copy without a relabel"
        );
        assert_eq!(
            &relabelled(Bytes::from_static(b"hev1"), 10, &relabel)[..],
            b"hvc1"
        );
        assert_eq!(
            &relabelled(Bytes::from_static(b"v1xx"), 12, &relabel)[..],
            b"c1xx"
        );
    }

    /// A torrent body read in pieces of any size: each read's file offset
    /// is where the last one ended.
    #[cfg(feature = "torrent")]
    #[tokio::test]
    async fn torrent_reads_relabel_at_their_own_offset() {
        use tokio::io::AsyncReadExt;
        let file: Vec<u8> = [&[1u8; 20][..], b"hev1", &[2u8; 20]].concat();
        let parts = [&file[..5], &file[5..22], &file[22..]];
        let body = std::io::Cursor::new(parts[0].to_vec())
            .chain(std::io::Cursor::new(parts[1].to_vec()))
            .chain(std::io::Cursor::new(parts[2].to_vec()));
        let start = 100;
        let relabel: Arc<[Relabelled]> = Arc::from([(start + 20, *b"hvc1")]);
        let chunks: Vec<_> = torrent::chunks(body, start, relabel).collect().await;
        assert_eq!(chunks.len(), 3, "one chunk per read");
        let served: Vec<u8> = chunks
            .into_iter()
            .flat_map(|c| c.unwrap().to_vec())
            .collect();
        assert_eq!(&served[20..24], b"hvc1");
        assert_eq!(served.len(), file.len());
    }

    #[test]
    fn a_blocking_reload_waits_at_most_90_s() {
        let s = Duration::from_secs;
        assert_eq!(reload_deadline(None), s(10));
        assert_eq!(reload_deadline(Some(2)), s(10));
        assert_eq!(reload_deadline(Some(6)), s(18));
        assert_eq!(reload_deadline(Some(u64::MAX)), s(90));
    }

    #[test]
    fn a_players_failure_is_said_as_the_server_says_it() {
        let word = |kind, status, error| failure_words(Some(kind), status, error);
        let raw = "MEDIA_ELEMENT_ERROR: Format error";
        let rows = [
            (
                word("http", Some(404), raw),
                "The link no longer exists (404)",
            ),
            (
                word("http", Some(403), raw),
                "The source refused access (403)",
            ),
            (
                word("http", Some(503), raw),
                "The source had an error (503)",
            ),
            (word("http", None, raw), "The source answered with an error"),
            (
                word("network", None, raw),
                "The source could not be reached",
            ),
            (
                word("timeout", None, raw),
                "The source did not answer in time",
            ),
            (word("unreadable", None, raw), "The file could not be read"),
            (
                word("decode", None, raw),
                "This device cannot decode the video",
            ),
            (word("audio", None, raw), "The audio output failed"),
            (word("other", None, raw), raw),
            (word("someday", None, raw), raw),
            (failure_words(None, Some(404), raw), raw),
        ];
        for (got, want) in rows {
            assert_eq!(got, want);
        }
        // The same sentences the server gives at open.
        assert_eq!(refused(StatusCode::NOT_FOUND), word("http", Some(404), raw));
        assert_eq!(UNREACHABLE, word("network", None, raw));
        assert_eq!(SILENT, word("timeout", None, raw));
    }

    #[test]
    fn parses_single_byte_ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 100)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 20)));
        assert_eq!(parse_range("bytes=90-500", 100), Some((90, 100)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 100)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=20-10", 100), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 100), None);
        // Past 4 GiB (a 4K remux), as on a 32-bit TV stick: offsets stay u64 end to end.
        let big = 9 * (1u64 << 30);
        assert_eq!(
            parse_range("bytes=5000000000-", big),
            Some((5_000_000_000, big))
        );
        assert_eq!(
            parse_range("bytes=5000000000-5000000099", big),
            Some((5_000_000_000, 5_000_000_100))
        );
        assert_eq!(
            parse_range("bytes=-4294967396", big),
            Some((big - 4_294_967_396, big))
        );
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
        // A long segment name is cut in its stem.
        let segment = sanitize(&format!("{}.m4s", "s".repeat(300)));
        assert!(
            segment.ends_with(".m4s") && segment.len() == 120,
            "{segment}"
        );
        // A file is never named like a playlist.
        assert_eq!(file_name("x.m3u8"), "x.bin");
        assert_eq!(file_name("x.ism"), "x.bin");
        assert_eq!(file_name("Movie.2010.mkv"), "Movie.2010.mkv");
        assert_eq!(
            manifest_name("tt1727587.mkv", Format::Hls),
            "tt1727587.m3u8"
        );
        assert_eq!(
            manifest_name("tt1727587.mkv", Format::Dash),
            "tt1727587.mpd"
        );
        assert!(lease.url.ends_with("/v.mkv"));
    }
}

/// The rewrite's lifetime, proved with a hook that holds it on its blocking
/// thread rather than with timings.
#[cfg(test)]
mod rewrites {
    use super::*;
    use std::sync::mpsc;

    type Hook = Arc<dyn Fn() + Send + Sync>;

    fn hls(hook: Option<Hook>) -> Arc<Hls> {
        let core = "http://127.0.0.1:7777/".to_owned();
        Arc::new(Hls {
            url: base(),
            kind: Format::Hls,
            http: HttpClient::new(mq_http::ClientConfig::default()).unwrap(),
            headers: Vec::new(),
            prefix: format!("{core}c/token/"),
            templates: format!("{core}t/token/"),
            core,
            table: Arc::default(),
            hook,
        })
    }

    fn base() -> Url {
        Url::parse("https://cdn.example/v/live.m3u8").unwrap()
    }

    fn media(segments: usize) -> Bytes {
        let mut body = String::from("#EXTM3U\n#EXT-X-TARGETDURATION:4\n");
        for i in 0..segments {
            body.push_str(&format!("#EXTINF:4,\ns{i}.ts\n"));
        }
        Bytes::from(body)
    }

    /// A hook that says when the rewrite reached it, then holds it until the
    /// test lets go (or 5 s pass), and records which of the two it was.
    struct Held {
        entered: tokio::sync::oneshot::Receiver<()>,
        go: mpsc::Sender<()>,
        /// `Some(true)`: the test let go; `Some(false)`: 5 s passed.
        let_go: Arc<Mutex<Option<bool>>>,
    }

    fn held() -> (Hook, Held) {
        let (entered_tx, entered) = tokio::sync::oneshot::channel();
        let entered_tx = Mutex::new(Some(entered_tx));
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let let_go = Arc::new(Mutex::new(None));
        let record = let_go.clone();
        let hook = move || {
            if let Some(entered) = lock(&entered_tx).take() {
                let _ = entered.send(());
            }
            let ok = lock(&wait).recv_timeout(Duration::from_secs(5)).is_ok();
            *lock(&record) = Some(ok);
        };
        (
            Arc::new(hook),
            Held {
                entered,
                go,
                let_go,
            },
        )
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    #[tokio::test]
    async fn a_rewrite_holds_no_runtime_worker() {
        let (hook, held) = held();
        let first = hls(Some(hook));
        let token = CancellationToken::new();
        let task = {
            let (first, token) = (first.clone(), token.clone());
            tokio::spawn(
                async move { rewrite(&first, media(10), base(), None, &token, far()).await },
            )
        };
        held.entered.await.unwrap();
        // The rewrite is held on its blocking thread, and this runtime's one
        // thread still runs work: another playback's rewrite completes.
        let other = hls(None);
        rewrite(&other, media(10), base(), None, &token, far())
            .await
            .unwrap();
        held.go.send(()).unwrap();
        task.await.unwrap().unwrap();
        assert_eq!(
            *lock(&held.let_go),
            Some(true),
            "the rewrite held the runtime thread"
        );
    }

    #[tokio::test]
    async fn a_rewrite_past_its_deadline_gives_up_and_stops() {
        let (hook, held) = held();
        let hls = hls(Some(hook));
        let token = CancellationToken::new();
        let end = Instant::now() + Duration::from_millis(100);
        let result = rewrite(&hls, media(10), base(), None, &token, end).await;
        // The request gave up at its deadline while the rewrite was held.
        assert_eq!(result.unwrap_err(), "out of time");
        assert_eq!(
            *lock(&held.let_go),
            None,
            "the request waited for the held rewrite"
        );
        held.go.send(()).unwrap();
        // Let go, the rewrite looks at its time before any work: it names
        // nothing and lets the table go.
        let table = tokio::time::timeout(Duration::from_secs(5), hls.table.lock())
            .await
            .expect("the table is free");
        assert_eq!(table.len(), 0, "the rewrite went on past its deadline");
    }

    #[tokio::test]
    async fn a_dropped_request_stops_its_rewrite() {
        let (hook, held) = held();
        let hls = hls(Some(hook));
        let token = CancellationToken::new();
        let task = {
            let (hls, token) = (hls.clone(), token.clone());
            tokio::spawn(async move { rewrite(&hls, media(10), base(), None, &token, far()).await })
        };
        held.entered.await.unwrap();
        task.abort();
        let _ = task.await;
        held.go.send(()).unwrap();
        let table = tokio::time::timeout(Duration::from_secs(5), hls.table.lock())
            .await
            .expect("the table is free");
        assert_eq!(table.len(), 0, "the rewrite went on after its request");
    }

    /// The only blocking thread is busy: a rewrite's work waits in the queue
    /// past its deadline, and the table it was given goes back.
    #[test]
    fn queued_rewrite_work_never_keeps_the_table() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let (busy, wait) = mpsc::channel::<()>();
            let occupied = tokio::task::spawn_blocking(move || {
                let _ = wait.recv_timeout(Duration::from_secs(10));
            });
            let hls = hls(None);
            let token = CancellationToken::new();
            let end = Instant::now() + Duration::from_millis(100);
            let result = rewrite(&hls, media(10), base(), None, &token, end).await;
            assert_eq!(result.unwrap_err(), "out of time");
            assert!(hls.table.try_lock().is_ok(), "queued work kept the table");
            busy.send(()).unwrap();
            occupied.await.unwrap();
        });
    }

    /// Release ends a rewrite's waits with the request still there: its wait
    /// for a table another rewrite holds, and its work queued behind the
    /// only blocking thread, both well before their 60 s deadline.
    #[test]
    fn release_ends_a_queued_rewrite() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let hls = hls(None);
            // Waiting for the table.
            let rewriting = hls.table.clone().lock_owned().await;
            let token = CancellationToken::new();
            let waiting = rewrite(&hls, media(10), base(), None, &token, far());
            let release = async {
                tokio::task::yield_now().await;
                token.cancel();
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(waiting, release)
            })
            .await
            .expect("release ended the wait for the table");
            assert_eq!(result.unwrap_err(), "released");
            drop(rewriting);
            // Queued behind a busy blocking thread, holding the table.
            let (busy, wait) = mpsc::channel::<()>();
            let occupied = tokio::task::spawn_blocking(move || {
                let _ = wait.recv_timeout(Duration::from_secs(10));
            });
            let token = CancellationToken::new();
            let queued = rewrite(&hls, media(10), base(), None, &token, far());
            let release = async {
                while hls.table.try_lock().is_ok() {
                    tokio::task::yield_now().await;
                }
                token.cancel();
            };
            let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(queued, release)
            })
            .await
            .expect("release ended the queued work's wait");
            assert_eq!(result.unwrap_err(), "released");
            assert!(hls.table.try_lock().is_ok(), "queued work kept the table");
            busy.send(()).unwrap();
            occupied.await.unwrap();
            // Its work, run at last, finds the table taken back.
            let table = tokio::time::timeout(Duration::from_secs(5), hls.table.lock())
                .await
                .expect("the table is free");
            assert_eq!(table.len(), 0, "released work went on");
        });
    }

    #[tokio::test]
    async fn a_reader_waits_for_the_table_no_longer_than_its_bound() {
        let hls = hls(None);
        let _rewriting = hls.table.clone().lock_owned().await;
        let token = CancellationToken::new();
        let bounded = table_within(&hls, &token, Duration::from_millis(50));
        let gave_up = tokio::time::timeout(Duration::from_secs(5), bounded)
            .await
            .expect("the wait ended at its bound");
        assert_eq!(gave_up.err(), Some(StatusCode::SERVICE_UNAVAILABLE));
        token.cancel();
        let released = table_within(&hls, &token, Duration::from_secs(60));
        let gone = tokio::time::timeout(Duration::from_secs(5), released)
            .await
            .expect("the wait ended at release");
        assert_eq!(gone.err(), Some(StatusCode::NOT_FOUND));
    }
}
