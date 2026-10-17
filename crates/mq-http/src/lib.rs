//! Shared HTTP client for the core: strict TLS, mandatory timeouts, response
//! size caps, per-host and global concurrency limits, and request priorities.

mod budget;
mod net;

pub use budget::Priority;
pub use net::{is_private_host, redact_url};
pub use reqwest::header::HeaderMap;

use budget::Budget;
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use reqwest::header::{HeaderName, HeaderValue, RANGE};
use std::sync::{Arc, Once};
use std::time::Duration;
use url::Url;

#[derive(Debug, thiserror::Error)]
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
    Status { status: u16, body: Bytes },
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

impl HttpClient {
    pub fn new(config: ClientConfig) -> Result<Self, HttpError> {
        install_crypto_provider();
        let inner = reqwest::Client::builder()
            .user_agent(config.user_agent)
            .connect_timeout(config.connect_timeout)
            .redirect(reqwest::redirect::Policy::limited(5))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| HttpError::Network(e.to_string()))?;
        Ok(Self {
            inner,
            budget: Arc::new(Budget::new(config.per_host_limit, config.global_limit)),
        })
    }

    pub async fn get(&self, url: &str, opts: &RequestOptions) -> Result<Response, HttpError> {
        self.send(url, opts, None).await
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
        self.send(url, opts, Some(format!("bytes={offset}-{end}")))
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
        raw: &str,
        opts: &RequestOptions,
        range: Option<String>,
    ) -> Result<Response, HttpError> {
        let url = Url::parse(raw).map_err(|_| HttpError::InvalidUrl(redact_str(raw)))?;
        let host = url.host_str().unwrap_or_default().to_owned();
        if !opts.allow_private && is_private_host(&host) {
            return Err(HttpError::BlockedPrivate(host));
        }
        let _permit = self.budget.acquire(&host, opts.priority).await;
        let request = self
            .inner
            .get(url)
            .timeout(opts.timeout)
            .headers(build_headers(&opts.headers, range));
        let response = request
            .send()
            .await
            .map_err(|e| HttpError::from_reqwest(e, opts.timeout))?;
        read_capped(response, opts).await
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
        return Err(HttpError::Status { status, body });
    }
    Ok(Response {
        status,
        headers,
        body,
        final_url,
    })
}
