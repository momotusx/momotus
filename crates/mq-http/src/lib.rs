//! Shared HTTP client for the core: strict TLS, mandatory timeouts, response
//! size caps, per-host and global concurrency limits, and request priorities.

mod budget;
mod net;

pub use budget::Priority;
pub use net::{is_private_host, redact_url};
pub use reqwest::Method;
pub use reqwest::header::HeaderMap;

use budget::Budget;
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use reqwest::header::{CONTENT_TYPE, HeaderName, HeaderValue, RANGE, RETRY_AFTER};
use std::sync::{Arc, Once};
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone, thiserror::Error)]
pub enum HttpError {
    #[error("invalid url: {0}")]
    InvalidUrl(String),
    #[error("blocked private address: {0}")]
    BlockedPrivate(String),
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("response larger than {limit} bytes")]
    TooLarge { limit: usize },
    #[error("http status {status}")]
    Status {
        status: u16,
        body: Bytes,
        /// Parsed `Retry-After` seconds, when the server sent one.
        retry_after: Option<Duration>,
    },
    #[error("network: {0}")]
    Network(String),
    #[error("decode: {0}")]
    Decode(String),
}

impl HttpError {
    fn from_reqwest(e: reqwest::Error, timeout: Duration) -> Self {
        if e.is_timeout() {
            HttpError::Timeout(timeout)
        } else {
            HttpError::Network(redact_error(e))
        }
    }
}

fn redact_error(e: reqwest::Error) -> String {
    let url = e.url().map(redact_url);
    let text = e.without_url().to_string();
    match url {
        Some(url) => format!("{text} ({url})"),
        None => text,
    }
}

/// Per-request options. Defaults suit addon JSON requests.
#[derive(Debug, Clone)]
pub struct RequestOptions {
    pub priority: Priority,
    pub timeout: Duration,
    pub max_bytes: usize,
    pub allow_private: bool,
    pub headers: Vec<(String, String)>,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            priority: Priority::Addon,
            timeout: Duration::from_secs(10),
            max_bytes: 8 * 1024 * 1024,
            allow_private: false,
            headers: Vec::new(),
        }
    }
}

/// Request body for non-GET calls.
#[derive(Debug, Clone)]
pub enum Body {
    Form(Vec<(String, String)>),
    /// Text fields only, sent as `multipart/form-data`.
    Multipart(Vec<(String, String)>),
    Json(serde_json::Value),
}

// ponytail: fixed boundary, fine for the short text fields callers send;
// switch to a random boundary if arbitrary user text is ever carried.
const MULTIPART_BOUNDARY: &str = "mq7d3f9a2c51e84b06a9d1f3c5e7b2a4d8";

impl Body {
    fn encode(&self) -> (String, Vec<u8>) {
        match self {
            Body::Form(pairs) => (
                "application/x-www-form-urlencoded".into(),
                url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(pairs)
                    .finish()
                    .into_bytes(),
            ),
            Body::Json(value) => ("application/json".into(), value.to_string().into_bytes()),
            Body::Multipart(fields) => {
                let mut out = String::new();
                for (name, value) in fields {
                    out.push_str(&format!(
                        "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{value}\r\n",
                        name.replace('"', "%22")
                    ));
                }
                out.push_str(&format!("--{MULTIPART_BOUNDARY}--\r\n"));
                (
                    format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
                    out.into_bytes(),
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub final_url: Url,
}

impl Response {
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, HttpError> {
        serde_json::from_slice(&self.body).map_err(|e| HttpError::Decode(e.to_string()))
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub user_agent: String,
    pub connect_timeout: Duration,
    pub per_host_limit: usize,
    pub global_limit: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            user_agent: format!("Marquee/{}", env!("CARGO_PKG_VERSION")),
            connect_timeout: Duration::from_secs(5),
            per_host_limit: 6,
            global_limit: 32,
        }
    }
}

#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    budget: Arc<Budget>,
}

static CRYPTO: Once = Once::new();

fn install_crypto_provider() {
    CRYPTO.call_once(|| {
        // Ignore the error: another component may have installed one already.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Follows at most 5 redirects. Callers check only the first URL against
/// `allow_private`, so a chain that starts at a public address may not
/// lead to a private one.
fn redirect(attempt: reqwest::redirect::Attempt) -> reqwest::redirect::Action {
    let first = attempt.previous().first();
    if attempt.previous().len() > 5 {
        attempt.error("too many redirects")
    } else if first.is_some_and(|first| !redirect_allowed(first, attempt.url())) {
        attempt.error("redirect from a public to a private address")
    } else {
        attempt.follow()
    }
}

fn redirect_allowed(first: &Url, next: &Url) -> bool {
    let private = |u: &Url| is_private_host(u.host_str().unwrap_or_default());
    private(first) || !private(next)
}

impl HttpClient {
    pub fn new(config: ClientConfig) -> Result<Self, HttpError> {
        install_crypto_provider();
        let inner = reqwest::Client::builder()
            .user_agent(config.user_agent)
            .connect_timeout(config.connect_timeout)
            .redirect(reqwest::redirect::Policy::custom(redirect))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| HttpError::Network(e.to_string()))?;
        Ok(Self {
            inner,
            budget: Arc::new(Budget::new(config.per_host_limit, config.global_limit)),
        })
    }

    pub async fn get(&self, url: &str, opts: &RequestOptions) -> Result<Response, HttpError> {
        self.send(Method::GET, url, None, opts, None).await
    }

    /// POSTs `body` as JSON. Same budgets, caps and redaction as `get`.
    pub async fn post_json<B: serde::Serialize>(
        &self,
        url: &str,
        body: &B,
        opts: &RequestOptions,
    ) -> Result<Response, HttpError> {
        let value = serde_json::to_value(body).map_err(|e| HttpError::Decode(e.to_string()))?;
        self.request(Method::POST, url, Some(Body::Json(value)), opts)
            .await
    }

    /// Any method with an optional body (POST, PUT, DELETE for provider APIs).
    pub async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Body>,
        opts: &RequestOptions,
    ) -> Result<Response, HttpError> {
        self.send(method, url, body, opts, None).await
    }

    /// Reads `len` bytes starting at `offset` with a Range request.
    pub async fn get_range(
        &self,
        url: &str,
        offset: u64,
        len: u64,
        opts: &RequestOptions,
    ) -> Result<Response, HttpError> {
        let end = offset + len.saturating_sub(1);
        self.send(
            Method::GET,
            url,
            None,
            opts,
            Some(format!("bytes={offset}-{end}")),
        )
        .await
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        opts: &RequestOptions,
    ) -> Result<T, HttpError> {
        self.get(url, opts).await?.json()
    }

    async fn send(
        &self,
        method: Method,
        raw: &str,
        body: Option<Body>,
        opts: &RequestOptions,
        range: Option<String>,
    ) -> Result<Response, HttpError> {
        let url = Url::parse(raw).map_err(|_| HttpError::InvalidUrl(redact_str(raw)))?;
        let host = url.host_str().unwrap_or_default().to_owned();
        if !opts.allow_private && is_private_host(&host) {
            return Err(HttpError::BlockedPrivate(host));
        }
        let _permit = self.budget.acquire(&host, opts.priority).await;
        let response = self
            .open(method, url, body, opts, range, Some(opts.timeout))
            .await?;
        read_capped(response, opts).await
    }

    /// Streams a response body without buffering it, for playback. The
    /// per-host permit is held until the body is dropped; `opts.timeout` is an
    /// idle timeout between chunks, not a total limit. Non-2xx statuses are
    /// returned as responses so a proxy can pass them through.
    pub async fn stream(
        &self,
        url: &str,
        range: Option<String>,
        opts: &RequestOptions,
    ) -> Result<StreamingResponse, HttpError> {
        let parsed = Url::parse(url).map_err(|_| HttpError::InvalidUrl(redact_str(url)))?;
        let host = parsed.host_str().unwrap_or_default().to_owned();
        if !opts.allow_private && is_private_host(&host) {
            return Err(HttpError::BlockedPrivate(host));
        }
        let permit = self.budget.acquire(&host, opts.priority).await;
        let response = self
            .open(Method::GET, parsed, None, opts, range, None)
            .await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let idle = opts.timeout;
        let body = response
            .bytes_stream()
            .map(move |chunk| chunk.map_err(|e| HttpError::from_reqwest(e, idle)));
        Ok(StreamingResponse {
            status,
            headers,
            body: Box::pin(body),
            idle,
            _permit: permit,
        })
    }

    async fn open(
        &self,
        method: Method,
        url: Url,
        body: Option<Body>,
        opts: &RequestOptions,
        range: Option<String>,
        total_timeout: Option<Duration>,
    ) -> Result<reqwest::Response, HttpError> {
        let mut headers = build_headers(&opts.headers, range);
        let mut request = self.inner.request(method, url);
        if let Some(timeout) = total_timeout {
            request = request.timeout(timeout);
        }
        if let Some(body) = body {
            let (content_type, bytes) = body.encode();
            if let Ok(value) = HeaderValue::try_from(content_type) {
                headers.insert(CONTENT_TYPE, value);
            }
            request = request.body(bytes);
        }
        let request = request.headers(headers);
        request
            .send()
            .await
            .map_err(|e| HttpError::from_reqwest(e, opts.timeout))
    }
}

/// A response whose body arrives as a stream of chunks.
pub struct StreamingResponse {
    pub status: u16,
    pub headers: HeaderMap,
    body: std::pin::Pin<Box<dyn futures_util::Stream<Item = Result<Bytes, HttpError>> + Send>>,
    idle: Duration,
    _permit: budget::Permit,
}

impl StreamingResponse {
    /// Next chunk, or `Timeout` when the origin stalls past the idle limit.
    pub async fn next_chunk(&mut self) -> Option<Result<Bytes, HttpError>> {
        match tokio::time::timeout(self.idle, self.body.next()).await {
            Ok(chunk) => chunk,
            Err(_) => Some(Err(HttpError::Timeout(self.idle))),
        }
    }
}

fn redact_str(raw: &str) -> String {
    Url::parse(raw)
        .map(|u| redact_url(&u))
        .unwrap_or_else(|_| "<invalid>".into())
}

fn build_headers(pairs: &[(String, String)], range: Option<String>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        if let (Ok(n), Ok(v)) = (
            HeaderName::try_from(name.as_str()),
            HeaderValue::try_from(value.as_str()),
        ) {
            headers.insert(n, v);
        }
    }
    if let Some(range) = range.and_then(|r| HeaderValue::try_from(r).ok()) {
        headers.insert(RANGE, range);
    }
    headers
}

async fn read_capped(
    response: reqwest::Response,
    opts: &RequestOptions,
) -> Result<Response, HttpError> {
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let final_url = response.url().clone();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs);
    if let Some(len) = response.content_length()
        && len as usize > opts.max_bytes
    {
        return Err(HttpError::TooLarge {
            limit: opts.max_bytes,
        });
    }
    let mut body = BytesMut::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|e| HttpError::from_reqwest(e, opts.timeout))?;
        if body.len() + chunk.len() > opts.max_bytes {
            return Err(HttpError::TooLarge {
                limit: opts.max_bytes,
            });
        }
        body.extend_from_slice(&chunk);
    }
    let body = body.freeze();
    if !(200..300).contains(&status) {
        return Err(HttpError::Status {
            status,
            body,
            retry_after,
        });
    }
    Ok(Response {
        status,
        headers,
        body,
        final_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_never_lead_from_public_to_private() {
        let url = |s: &str| Url::parse(s).unwrap();
        let public = url("https://cdn.example/video.mkv");
        let lan = url("http://192.168.1.1/admin");
        assert!(!redirect_allowed(&public, &lan));
        assert!(!redirect_allowed(&public, &url("http://localhost:8080/")));
        assert!(redirect_allowed(
            &public,
            &url("https://ia800.example/v.mp4")
        ));
        // A local addon (already allowed to reach private addresses).
        assert!(redirect_allowed(&url("http://127.0.0.1:7000/s"), &lan));
        assert!(redirect_allowed(&url("http://127.0.0.1:7000/s"), &public));
    }
}
