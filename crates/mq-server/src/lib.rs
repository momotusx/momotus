//! Loopback stream server. Every playback gets one URL,
//! `http://127.0.0.1:<port>/s/<token>/<filename>`, where the token is 32
//! random bytes. Players never learn where bytes come from: an upstream
//! HTTP(S) URL proxied with Range passthrough, or (feature `torrent`, full
//! builds only) a file of the embedded torrent engine served from verified
//! pieces.
//!
//! Subtitles get `http://127.0.0.1:<port>/sub/<token>/<lang>.vtt`: the
//! addon's file fetched on request and served as UTF-8 WebVTT.

mod subtitles;

pub use subtitles::to_vtt;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::stream;
use mq_http::{HttpClient, Priority, RequestOptions};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Where the bytes of one playback come from.
#[derive(Debug, Clone)]
pub enum Upstream {
    Http {
        url: String,
        /// Request headers from the stream's `proxyHeaders`.
        headers: Vec<(String, String)>,
        allow_private: bool,
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
    allow_private: bool,
}

#[derive(Clone)]
struct AppState {
    http: HttpClient,
    sessions: Arc<Mutex<HashMap<String, Upstream>>>,
    subtitles: Arc<Mutex<HashMap<String, Subtitle>>>,
    port: u16,
}

pub struct StreamServer {
    state: AppState,
    addr: SocketAddr,
    _task: tokio::task::JoinHandle<()>,
}

impl StreamServer {
    /// Binds an ephemeral port on 127.0.0.1 only.
    pub async fn start(http: HttpClient) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let state = AppState {
            http,
            sessions: Arc::default(),
            subtitles: Arc::default(),
            port: addr.port(),
        };
        let app = Router::new()
            .route("/s/{token}/{name}", get(serve).head(serve))
            .route("/sub/{token}/{name}", get(serve_subtitle))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self {
            state,
            addr,
            _task: task,
        })
    }

    /// Registers a playback and returns its URL. `filename` only helps
    /// players guess the container; it carries no secrets.
    pub fn register(&self, upstream: Upstream, filename: &str) -> String {
        let token = new_token();
        lock(&self.state.sessions).insert(token.clone(), upstream);
        format!("http://{}/s/{token}/{}", self.addr, sanitize(filename))
    }

    /// Registers an addon's subtitle file and returns a URL that serves it as
    /// UTF-8 WebVTT. The same file gets the same URL, so tracks compare by it.
    pub fn register_subtitle(&self, url: &str, lang: &str, allow_private: bool) -> String {
        let subtitle = Subtitle {
            url: url.to_owned(),
            lang: lang.to_owned(),
            allow_private,
        };
        let mut subtitles = lock(&self.state.subtitles);
        let token = match subtitles.iter().find(|(_, s)| **s == subtitle) {
            Some((token, _)) => token.clone(),
            None => {
                let token = new_token();
                subtitles.insert(token.clone(), subtitle);
                token
            }
        };
        format!("http://{}/sub/{token}/{}.vtt", self.addr, sanitize(lang))
    }

    /// Ends a playback or a subtitle; its URL stops working immediately.
    pub fn release(&self, url: &str) {
        let token = |prefix| {
            url.split(prefix)
                .nth(1)
                .and_then(|rest: &str| rest.split('/').next())
                .map(str::to_owned)
        };
        if let Some(token) = token("/s/") {
            lock(&self.state.sessions).remove(&token);
        }
        if let Some(token) = token("/sub/") {
            lock(&self.state.subtitles).remove(&token);
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
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
fn host_ok(state: &AppState, headers: &HeaderMap) -> bool {
    headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| {
            h == format!("127.0.0.1:{}", state.port) || h == format!("localhost:{}", state.port)
        })
}

/// Fetches the addon's subtitle file and answers with UTF-8 WebVTT. Any
/// origin may read it (a browser's `<track crossorigin>`); the token is the
/// capability.
async fn serve_subtitle(
    State(state): State<AppState>,
    Path((token, _name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !host_ok(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(subtitle) = lock(&state.subtitles).get(&token).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let opts = RequestOptions {
        priority: Priority::Playback,
        timeout: Duration::from_secs(20),
        allow_private: subtitle.allow_private,
        ..Default::default()
    };
    let body = match state.http.get(&subtitle.url, &opts).await {
        Ok(response) => response.body,
        Err(e) => {
            tracing::warn!("subtitle upstream failed: {e}");
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
    State(state): State<AppState>,
    Path((token, _name)): Path<(String, String)>,
    request: Request<Body>,
) -> Response {
    if !host_ok(&state, request.headers()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(upstream) = lock(&state.sessions).get(&token).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    match upstream {
        Upstream::Http {
            url,
            headers,
            allow_private,
        } => {
            proxy_http(
                &state.http,
                &url,
                headers,
                allow_private,
                range,
                request.method() == Method::HEAD,
            )
            .await
        }
        #[cfg(feature = "torrent")]
        Upstream::Torrent {
            engine,
            torrent,
            file,
        } => {
            torrent::serve(
                &engine,
                torrent,
                file,
                range.as_deref(),
                request.method() == Method::HEAD,
            )
            .await
        }
    }
}

#[cfg(feature = "torrent")]
mod torrent {
    use super::*;

    /// `bytes=a-b`, `bytes=a-` or `bytes=-n` against `len`, as `[start, end)`.
    /// `None` when unsatisfiable or not a single byte range.
    pub(crate) fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
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
    pub(crate) async fn serve(
        engine: &mq_engine::Engine,
        torrent: mq_engine::TorrentHandle,
        file: usize,
        range: Option<&str>,
        head_only: bool,
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
                    return (StatusCode::RANGE_NOT_SATISFIABLE, out).into_response();
                }
            },
        };
        out.insert(header::CONTENT_LENGTH, HeaderValue::from(end - start));
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
        (status, out, Body::from_stream(chunks)).into_response()
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
            assert_eq!(content_type("A/B.MKV"), "video/x-matroska");
            assert_eq!(content_type("x.iso"), "application/octet-stream");
        }
    }
}

async fn proxy_http(
    http: &HttpClient,
    url: &str,
    headers: Vec<(String, String)>,
    allow_private: bool,
    range: Option<String>,
    head_only: bool,
) -> Response {
    let opts = RequestOptions {
        priority: Priority::Playback,
        timeout: Duration::from_secs(30),
        allow_private,
        headers,
        ..Default::default()
    };
    let upstream = match http.stream(url, range, &opts).await {
        Ok(response) => response,
        Err(e) => {
            tracing::warn!("stream upstream failed: {e}");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let status = StatusCode::from_u16(upstream.status).unwrap_or(StatusCode::BAD_GATEWAY);
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
    let body = stream::unfold(upstream, |mut upstream| async move {
        let chunk = upstream.next_chunk().await?;
        Some((chunk.map_err(std::io::Error::other), upstream))
    });
    (status, out, Body::from_stream(body)).into_response()
}
