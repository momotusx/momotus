//! Shared HTTP client for the core: strict TLS, mandatory timeouts, response
//! size caps, per-host and global concurrency limits, and request priorities.

mod budget;
mod net;

pub use budget::Priority;
pub use net::{is_private_host, is_private_ip, local_addrs, redact_url};
pub use reqwest::Method;
pub use reqwest::header::HeaderMap;
pub use url::Url;

use budget::Budget;
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use net::Device;
use reqwest::header::{CONTENT_TYPE, HeaderName, HeaderValue, LOCATION, RANGE, RETRY_AFTER};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once, RwLock};
use std::time::Duration;
use tokio::time::Instant;

/// Redirects a request follows at most.
const MAX_REDIRECTS: usize = 5;
/// A scoped handle's refusal. It never names the address, which may be the
/// user's own.
const OUT_OF_SCOPE: &str = "outside the playback's scope";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

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
    /// What went wrong as a fixed word, for logs that must not name an
    /// address (a sync relay's host is the user's own).
    pub fn kind(&self) -> &'static str {
        match self {
            HttpError::InvalidUrl(_) => "invalid url",
            HttpError::BlockedPrivate(_) => "blocked address",
            HttpError::Timeout(_) => "timeout",
            HttpError::TooLarge { .. } => "too large",
            HttpError::Status { .. } => "http status",
            HttpError::Network(_) => "network",
            HttpError::Decode(_) => "decode",
        }
    }

    fn from_reqwest(e: reqwest::Error, timeout: Duration) -> Self {
        if e.is_timeout() {
            HttpError::Timeout(timeout)
        } else if is_blocked(&e) {
            HttpError::BlockedPrivate(OUT_OF_SCOPE.into())
        } else {
            HttpError::Network(redact_error(e))
        }
    }
}

/// Whether a scoped resolver's refusal is among `e`'s causes.
fn is_blocked(e: &(dyn std::error::Error + 'static)) -> bool {
    std::iter::successors(Some(e), |e| e.source()).any(|e| e.is::<Blocked>())
}

fn redact_error(e: reqwest::Error) -> String {
    let url = e.url().map(redact_url);
    let e = e.without_url();
    // The causes say what failed (a refused connection, a certificate the
    // device does not trust); none of them carries the address.
    let mut text = e.to_string();
    let mut cause = std::error::Error::source(&e);
    while let Some(c) = cause {
        text.push_str(": ");
        text.push_str(&c.to_string());
        cause = c.source();
    }
    match url {
        Some(url) => format!("{text} ({url})"),
        None => text,
    }
}

/// Per-request options. Defaults suit addon JSON requests.
#[derive(Debug, Clone)]
pub struct RequestOptions {
    pub priority: Priority,
    /// A buffered request's total limit, over every redirect hop and the
    /// body; a stream's idle limit between body chunks.
    pub timeout: Duration,
    /// How long each hop may wait for its response headers. Unset: a
    /// buffered request's own `timeout`, and `STREAM_HEADERS` for a stream,
    /// which has no total limit. Unscoped requests follow redirects inside
    /// one hop.
    pub hop_headers: Option<Duration>,
    pub max_bytes: usize,
    pub allow_private: bool,
    /// Unscoped requests without `allow_private` only: no proxy at all, not
    /// even the system's. A proxy, an HTTPS one above all, resolves the
    /// destination itself, past the public-only resolver; installs from the
    /// addon lists take this path.
    pub no_proxy: bool,
    pub headers: Vec<(String, String)>,
    /// Scoped handles only: the URL whose origin gets every header in
    /// `headers` (`headers_for`). Unset: the request's own first URL.
    pub owner: Option<Url>,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            priority: Priority::Addon,
            timeout: Duration::from_secs(10),
            hop_headers: None,
            max_bytes: 8 * 1024 * 1024,
            allow_private: false,
            no_proxy: false,
            headers: Vec::new(),
            owner: None,
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
    /// A decode error names its kind and position, never the text it read: a
    /// response may echo a key or token the request sent.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, HttpError> {
        serde_json::from_slice(&self.body).map_err(|e| {
            HttpError::Decode(format!(
                "{:?} at line {} column {}",
                e.classify(),
                e.line(),
                e.column()
            ))
        })
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
    /// The unscoped clients, shared by every clone and replaced as a whole by
    /// `network_changed`.
    pools: Arc<RwLock<Arc<Pools>>>,
    budget: Arc<Budget>,
    scopes: Arc<Scopes>,
    /// Set on a handle from `scoped`: its addresses, canonical and sorted.
    /// Each request takes the current generation's client for them.
    scope: Option<Arc<[IpAddr]>>,
}

/// The unscoped clients and their connection pools.
struct Pools {
    /// For requests allowed to reach private addresses (local addons).
    inner: reqwest::Client,
    /// For everything else: names that resolve only to private addresses
    /// are refused when connecting, so a public name pointing at the LAN,
    /// a redirect to one, or a DNS answer that changes later cannot reach
    /// it.
    public: reqwest::Client,
    /// `public` without any proxy (`RequestOptions::no_proxy`).
    direct: reqwest::Client,
}

impl Pools {
    fn build(config: &ClientConfig) -> Result<Self, HttpError> {
        let follow = || builder(config).redirect(reqwest::redirect::Policy::custom(redirect));
        let build =
            |b: reqwest::ClientBuilder| b.build().map_err(|e| HttpError::Network(e.to_string()));
        let inner = follow();
        // Test builds only: names in the test table resolve here too.
        #[cfg(any(test, feature = "test-dns"))]
        let inner = inner.dns_resolver(Arc::new(TestTable));
        Ok(Self {
            inner: build(inner)?,
            public: build(follow().dns_resolver(Arc::new(PublicOnly)))?,
            direct: build(follow().no_proxy().dns_resolver(Arc::new(PublicOnly)))?,
        })
    }
}

/// The scoped clients of the current interface snapshot, one per address set,
/// and what building one takes.
struct Scopes {
    config: ClientConfig,
    /// Read by every scoped resolver and literal check, re-read by every
    /// `scoped` call.
    device: Arc<RwLock<Device>>,
    /// Emptied when the snapshot changes (a new generation), so no request
    /// reuses a connection pooled under an older one.
    clients: Mutex<HashMap<Vec<IpAddr>, Scope>>,
    /// The first failed interface read was logged.
    unreadable: AtomicBool,
    #[cfg(any(test, feature = "test-dns"))]
    injected: Mutex<Option<Result<Vec<net::Interface>, ()>>>,
    #[cfg(test)]
    built: std::sync::atomic::AtomicUsize,
}

impl Scopes {
    fn new(config: ClientConfig) -> Self {
        Self {
            config,
            device: Default::default(),
            clients: Default::default(),
            unreadable: AtomicBool::new(false),
            #[cfg(any(test, feature = "test-dns"))]
            injected: Default::default(),
            #[cfg(test)]
            built: Default::default(),
        }
    }

    /// Re-reads the interfaces. A failed read keeps the last snapshot (none
    /// before the first success), and only the first failure is logged.
    /// The read happens under the lock, so two calls racing across a network
    /// change cannot install the older list last.
    fn refresh(&self) {
        let mut clients = lock(&self.clients);
        match self.interfaces() {
            Ok(list) => {
                let device = Device::from_interfaces(list);
                let (own, prefixes, tunnels) = device.sizes();
                let mut current = self.device.write().unwrap_or_else(|e| e.into_inner());
                if *current != device {
                    // Counts only, once per change: the device checks of
                    // the app walks read this line.
                    tracing::info!(own, prefixes, tunnels, "interface snapshot");
                    *current = device;
                    clients.clear();
                } else {
                    tracing::debug!(own, prefixes, tunnels, "interface snapshot unchanged");
                }
            }
            Err(e) => {
                if !self.unreadable.swap(true, Ordering::Relaxed) {
                    let kind = e.kind();
                    tracing::warn!("interface list unreadable ({kind}); keeping the last snapshot");
                }
            }
        }
    }

    fn interfaces(&self) -> std::io::Result<Vec<net::Interface>> {
        #[cfg(any(test, feature = "test-dns"))]
        if let Some(injected) = lock(&self.injected).clone() {
            return injected.map_err(|()| std::io::ErrorKind::Other.into());
        }
        net::interfaces()
    }

    /// The current generation's client for `local`, built on first use. A
    /// failed build is not kept, so the next request tries again.
    fn client(&self, local: &[IpAddr]) -> Scope {
        let mut clients = lock(&self.clients);
        if let Some(scope) = clients.get(local) {
            return scope.clone();
        }
        let scope = self.build(local.to_vec());
        if scope.client.is_ok() {
            clients.insert(local.to_vec(), scope.clone());
        }
        scope
    }

    fn build(&self, local: Vec<IpAddr>) -> Scope {
        #[cfg(test)]
        self.built
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let allowed = Allowed {
            local,
            device: self.device.clone(),
        };
        let client = builder(&self.config)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .dns_resolver(allowed.clone())
            .build()
            .map_err(|e| HttpError::Network(e.to_string()));
        Scope { client, allowed }
    }
}

#[derive(Clone)]
struct Scope {
    /// A client that could not be built fails every request: nothing falls
    /// back to a wider one.
    client: Result<reqwest::Client, HttpError>,
    allowed: Allowed,
}

/// The addresses a scoped handle may connect to: `local`, and public ones off
/// the device's own network. As a resolver it keeps only those.
#[derive(Clone)]
struct Allowed {
    /// Canonical and sorted.
    local: Vec<IpAddr>,
    device: Arc<RwLock<Device>>,
}

impl Allowed {
    fn permits(&self, ip: IpAddr) -> bool {
        let device = self.device.read().unwrap_or_else(|e| e.into_inner());
        net::permitted(ip, &self.local, &device)
    }
}

impl reqwest::dns::Resolve for Allowed {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let allowed = self.clone();
        Box::pin(async move {
            let kept: Vec<SocketAddr> = lookup(name.as_str())
                .await?
                .into_iter()
                .filter(|a| allowed.permits(a.ip()))
                .collect();
            if kept.is_empty() {
                return Err(Box::new(Blocked) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(kept.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A scoped resolver's refusal: no address it found is in scope.
#[derive(Debug)]
struct Blocked;

impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(OUT_OF_SCOPE)
    }
}

impl std::error::Error for Blocked {}

/// The system resolver's answer, or the test table's.
async fn lookup(name: &str) -> std::io::Result<Vec<SocketAddr>> {
    #[cfg(any(test, feature = "test-dns"))]
    if let Some(found) = test_dns::get(name) {
        return Ok(found.into_iter().map(|ip| SocketAddr::new(ip, 0)).collect());
    }
    Ok(tokio::net::lookup_host((name, 0)).await?.collect())
}

/// Names the scoped resolver answers before the system resolver, so a test
/// can point a name at a fixture and change its answer, and interface lists
/// a client reads instead of the device's. Only dev-dependencies enable it.
#[cfg(any(test, feature = "test-dns"))]
pub mod test_dns {
    pub use crate::net::Interface;
    use crate::{HttpClient, lock};
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::sync::{LazyLock, Mutex};

    static NAMES: LazyLock<Mutex<HashMap<String, Vec<IpAddr>>>> = LazyLock::new(Default::default);

    /// Answers `name` with `addrs` from now on, for every scoped handle.
    pub fn set(name: &str, addrs: &[IpAddr]) {
        let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
        names.insert(name.to_ascii_lowercase(), addrs.to_vec());
    }

    /// Every later `scoped()` call on `http` (or a handle of it) reads this
    /// list, or fails to read, instead of the device's interfaces. Per client,
    /// so tests running side by side do not see each other's lists.
    pub fn set_interfaces(http: &HttpClient, list: Result<Vec<Interface>, ()>) {
        *lock(&http.scopes.injected) = Some(list);
    }

    /// The own addresses of `http`'s interface snapshot.
    pub fn own_addresses(http: &HttpClient) -> Vec<IpAddr> {
        let device = http.scopes.device.read().unwrap_or_else(|e| e.into_inner());
        device.own().to_vec()
    }

    pub(crate) fn get(name: &str) -> Option<Vec<IpAddr>> {
        let names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
        names.get(&name.to_ascii_lowercase()).cloned()
    }

    static PUBLIC: LazyLock<Mutex<HashMap<String, Vec<IpAddr>>>> = LazyLock::new(Default::default);

    /// Answers `name` with `addrs` for the unscoped public-only clients, as
    /// if they were public: a loopback fixture then stands for a public
    /// origin. The name's `set` answers come first and are filtered as
    /// usual, so a test can mix private answers in.
    pub fn set_public(name: &str, addrs: &[IpAddr]) {
        let mut names = PUBLIC.lock().unwrap_or_else(|e| e.into_inner());
        names.insert(name.to_ascii_lowercase(), addrs.to_vec());
    }

    pub(crate) fn public(name: &str) -> Option<Vec<IpAddr>> {
        let names = PUBLIC.lock().unwrap_or_else(|e| e.into_inner());
        names.get(&name.to_ascii_lowercase()).cloned()
    }

    /// An https origin on loopback, with the self-signed certificate test
    /// builds accept: every request gets `answer(path)` as its whole
    /// response, then the connection closes. Returns its port.
    pub fn https(answer: impl Fn(&str) -> String + Send + Sync + 'static) -> u16 {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        use std::io::{Read, Write};
        let cert = CertificateDer::from(&include_bytes!("../tests/fixtures/tls/cert.der")[..]);
        let key = PrivatePkcs8KeyDer::from(&include_bytes!("../tests/fixtures/tls/key.der")[..]);
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|b| {
                b.with_no_client_auth()
                    .with_single_cert(vec![cert], PrivateKeyDer::Pkcs8(key))
            })
            .expect("the fixture certificate");
        let config = std::sync::Arc::new(config);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("bound").port();
        let answer = std::sync::Arc::new(answer);
        std::thread::spawn(move || {
            for socket in listener.incoming().flatten() {
                let (config, answer) = (config.clone(), answer.clone());
                std::thread::spawn(move || {
                    let Ok(conn) = rustls::ServerConnection::new(config) else {
                        return;
                    };
                    let mut tls = rustls::StreamOwned::new(conn, socket);
                    let (mut head, mut buf) = (Vec::new(), [0u8; 1024]);
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match tls.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => head.extend_from_slice(&buf[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&head).into_owned();
                    let path = head.split(' ').nth(1).unwrap_or_default();
                    let _ = tls.write_all(answer(path).as_bytes());
                    tls.conn.send_close_notify();
                    let _ = tls.flush();
                });
            }
        });
        port
    }
}

/// Resolves with the system resolver (or the test table) and keeps only
/// public addresses.
struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let (found, stand_ins) = public_lookup(name.as_str()).await?;
            let public: Vec<SocketAddr> = found
                .into_iter()
                .filter(|a| stand_ins.contains(&a.ip()) || !is_private_ip(a.ip()))
                .collect();
            if public.is_empty() {
                return Err(format!("{} resolves only to private addresses", name.as_str()).into());
            }
            Ok(Box::new(public.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// The system resolver behind the test table, every address kept (test
/// builds' private-reach client).
#[cfg(any(test, feature = "test-dns"))]
struct TestTable;

#[cfg(any(test, feature = "test-dns"))]
impl reqwest::dns::Resolve for TestTable {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let found = lookup(name.as_str()).await?;
            Ok(Box::new(found.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// `lookup`, and the answers a test lets stand for public addresses
/// (`test_dns::set_public`), after the name's other test answers.
async fn public_lookup(name: &str) -> std::io::Result<(Vec<SocketAddr>, Vec<IpAddr>)> {
    #[cfg(any(test, feature = "test-dns"))]
    if let Some(stand_ins) = test_dns::public(name) {
        let others = test_dns::get(name).unwrap_or_default();
        let found = others.iter().chain(&stand_ins);
        let found = found.map(|ip| SocketAddr::new(*ip, 0)).collect();
        return Ok((found, stand_ins));
    }
    Ok((lookup(name).await?, Vec::new()))
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
    let last = attempt.previous().last();
    if attempt.previous().len() > 5 {
        attempt.error("too many redirects")
    } else if last.is_some_and(|last| downgrades(last, attempt.url())) {
        // A key in the path or query would travel in the clear.
        attempt.error("redirect from https to http")
    } else if first.is_some_and(|first| !redirect_allowed(first, attempt.url())) {
        attempt.error("redirect from a public to a private address")
    } else {
        attempt.follow()
    }
}

fn downgrades(from: &Url, to: &Url) -> bool {
    from.scheme() == "https" && to.scheme() == "http"
}

fn redirect_allowed(first: &Url, next: &Url) -> bool {
    let private = |u: &Url| is_private_host(u.host_str().unwrap_or_default());
    private(first) || !private(next)
}

/// What every client shares, scoped or not. TCP keepalive finds a pooled
/// connection the network dropped on platforms that send no network change
/// (`network_changed` drops the pools at once where they do).
fn builder(config: &ClientConfig) -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder()
        .user_agent(config.user_agent.clone())
        .connect_timeout(config.connect_timeout)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(15));
    // Test builds only: `test_dns::https` serves a self-signed certificate.
    #[cfg(any(test, feature = "test-dns"))]
    let builder = builder.tls_danger_accept_invalid_certs(true);
    builder
}

impl HttpClient {
    pub fn new(config: ClientConfig) -> Result<Self, HttpError> {
        install_crypto_provider();
        Ok(Self {
            pools: Arc::new(RwLock::new(Arc::new(Pools::build(&config)?))),
            budget: Arc::new(Budget::new(config.per_host_limit, config.global_limit)),
            scopes: Arc::new(Scopes::new(config)),
            scope: None,
        })
    }

    /// A handle for one playback's requests. They connect only to public
    /// addresses and to `local` (`local_addrs` of the installing addon's
    /// host): each hop's literal is checked before connecting and each DNS
    /// answer when resolving, redirects are followed here hop by hop, no proxy
    /// is used, and headers go in full only to their owner's origin
    /// (`headers_for`). `allow_private` is ignored. The device's own addresses
    /// and the on-link prefixes of its interfaces count as private, re-read on
    /// every call; a `local` with a loopback address also reaches the own
    /// addresses, since its install runs on this device. Handles with the same addresses share one client and its
    /// connections while the snapshot stays the same, and every handle shares
    /// this client's budget.
    pub fn scoped(&self, local: &[IpAddr]) -> HttpClient {
        let mut key: Vec<IpAddr> = local.iter().map(|ip| ip.to_canonical()).collect();
        key.sort();
        key.dedup();
        self.scopes.refresh();
        // Built here, so a playback's open pays for it rather than its first read.
        let _ = self.scopes.client(&key);
        HttpClient {
            pools: self.pools.clone(),
            budget: self.budget.clone(),
            scopes: self.scopes.clone(),
            scope: Some(key.into()),
        }
    }

    /// The device's network changed (the apps say so): every pooled
    /// connection may lead nowhere now, and each would hold a request until
    /// its timeout. The unscoped pools are built again and the scoped
    /// clients forgotten, for every clone of this client. Requests already
    /// sent keep their connection.
    pub fn network_changed(&self) {
        match Pools::build(&self.scopes.config) {
            Ok(pools) => *self.pools.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(pools),
            Err(e) => tracing::warn!("could not build new connection pools: {e}"),
        }
        lock(&self.scopes.clients).clear();
        self.scopes.refresh();
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
        let request = Request {
            method,
            url,
            body,
            range,
            timeout: Some(opts.timeout),
        };
        let (response, _permit) = self.fetch(request, opts).await?;
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
        let request = Request {
            method: Method::GET,
            url: parsed,
            body: None,
            range,
            timeout: None,
        };
        let (response, permit) = self.fetch(request, opts).await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let idle = opts.timeout;
        let final_url = response.url().clone();
        let body = response
            .bytes_stream()
            .map(move |chunk| chunk.map_err(|e| HttpError::from_reqwest(e, idle)));
        Ok(StreamingResponse {
            status,
            headers,
            final_url,
            body: Box::pin(body),
            idle,
            _permit: permit,
        })
    }

    /// Sends `req` and returns the answer with the host slot it holds.
    /// Unscoped, the first URL is checked against `allow_private` and reqwest
    /// follows redirects under `redirect`; scoped, `follow` does it all.
    async fn fetch(
        &self,
        req: Request,
        opts: &RequestOptions,
    ) -> Result<(reqwest::Response, budget::Permit), HttpError> {
        if let Some(local) = &self.scope {
            return self.follow(local, req, opts).await;
        }
        let host = req.url.host_str().unwrap_or_default().to_owned();
        if !opts.allow_private && is_private_host(&host) {
            return Err(HttpError::BlockedPrivate(host));
        }
        let permit = self.budget.acquire(&host, opts.priority).await;
        // The pools as they are once the slot is held: a network change while
        // the request waited must not leave it on a dropped pool.
        let pools = self.pools.read().unwrap_or_else(|e| e.into_inner()).clone();
        let client = if opts.allow_private {
            &pools.inner
        } else if opts.no_proxy {
            &pools.direct
        } else {
            &pools.public
        };
        let headers = build_headers(&opts.headers, req.range.clone());
        let response = open(client, &req, headers, opts).await?;
        Ok((response, permit))
    }

    /// A scoped request, hop by hop: each hop takes its host slot, then the
    /// current generation's client (a refresh while it waited for the slot
    /// cannot leave it on an older pool) and checks its literal before
    /// connecting (names go through `Allowed`); its headers follow
    /// `headers_for`, and `next_hop` decides where it leads, at most
    /// `MAX_REDIRECTS` times, with `Range` kept and methods as RFC 9110 15.4
    /// says. Each hop's wait for headers is bounded (`header_limit`); a
    /// buffered request's total limit spans the whole chain, slot waits
    /// included. Release before acquire: a hop's answer and host slot are
    /// dropped before the next hop asks for a slot, since holding both
    /// deadlocks same-host redirects at the per-host limit.
    async fn follow(
        &self,
        local: &[IpAddr],
        mut req: Request,
        opts: &RequestOptions,
    ) -> Result<(reqwest::Response, budget::Permit), HttpError> {
        let first = req.url.clone();
        let owner = opts.owner.clone().unwrap_or_else(|| req.url.clone());
        let deadline = req.timeout.map(|total| Instant::now() + total);
        let mut hops = 0;
        loop {
            let host = req.url.host_str().unwrap_or_default();
            let acquire = self.budget.acquire(host, opts.priority);
            let permit = match deadline {
                Some(deadline) => tokio::time::timeout_at(deadline, acquire)
                    .await
                    .map_err(|_| HttpError::Timeout(opts.timeout))?,
                None => acquire.await,
            };
            let scope = self.scopes.client(local);
            let client = scope.client?;
            if literal(&req.url).is_some_and(|ip| !scope.allowed.permits(ip)) {
                return Err(HttpError::BlockedPrivate(OUT_OF_SCOPE.into()));
            }
            if let Some(deadline) = deadline {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(HttpError::Timeout(opts.timeout));
                }
                req.timeout = Some(left);
            }
            let headers = headers_for(&owner, &req.url, &opts.headers);
            let headers = build_headers(&headers, req.range.clone());
            let response = open(&client, &req, headers, opts).await?;
            let status = response.status().as_u16();
            let Some(next) = next_hop(status, response.headers().get(LOCATION), &req.url)? else {
                return Ok((response, permit));
            };
            drop(response);
            drop(permit);
            hops += 1;
            if hops > MAX_REDIRECTS {
                return Err(HttpError::Network("too many redirects".into()));
            }
            let to_get = match status {
                301 | 302 => req.method == Method::POST,
                303 => req.method != Method::HEAD,
                _ => false,
            };
            if to_get {
                req.method = Method::GET;
                req.body = None;
            }
            req.url = same_origin_credentials(&first, next);
        }
    }
}

/// `next` with the first URL's userinfo when it is the same origin (or its
/// https upgrade) and names none itself: reqwest sends URL userinfo as Basic
/// credentials, and an absolute `Location` on the same server carries none
/// (nor does a response's `final_url`). Another origin never gets them.
pub fn same_origin_credentials(first: &Url, mut next: Url) -> Url {
    let same = first.origin() == next.origin() || upgrades(first, &next);
    if same && next.username().is_empty() && next.password().is_none() {
        let _ = next.set_username(first.username());
        let _ = next.set_password(first.password());
    }
    next
}

/// One request as `send` and `stream` hand it to `fetch`.
struct Request {
    method: Method,
    url: Url,
    body: Option<Body>,
    range: Option<String>,
    /// A total limit; streams have only `opts.timeout` between chunks.
    timeout: Option<Duration>,
}

/// A stream's default wait for one hop's response headers: the stream
/// server's `OPEN` deadline (`mq-server/src/ahead.rs`), which bounded every
/// stream before, so its re-opens and passthrough reads keep today's limit.
/// Buffered requests keep their own total.
const STREAM_HEADERS: Duration = Duration::from_secs(20);

/// How long one hop may wait for its response headers.
fn header_limit(req: &Request, opts: &RequestOptions) -> Duration {
    let default = if req.timeout.is_some() {
        opts.timeout
    } else {
        STREAM_HEADERS
    };
    opts.hop_headers.unwrap_or(default)
}

/// Sends one hop. Its wait for response headers is bounded by
/// `header_limit`; when that passes, the hop's future is dropped.
async fn open(
    client: &reqwest::Client,
    req: &Request,
    mut headers: HeaderMap,
    opts: &RequestOptions,
) -> Result<reqwest::Response, HttpError> {
    let mut request = client.request(req.method.clone(), req.url.clone());
    if let Some(timeout) = req.timeout {
        request = request.timeout(timeout);
    }
    if let Some(body) = &req.body {
        let (content_type, bytes) = body.encode();
        if let Ok(value) = HeaderValue::try_from(content_type) {
            headers.insert(CONTENT_TYPE, value);
        }
        request = request.body(bytes);
    }
    let sent = request.headers(headers).send();
    let limit = header_limit(req, opts);
    match tokio::time::timeout(limit, sent).await {
        Ok(answer) => answer.map_err(|e| HttpError::from_reqwest(e, opts.timeout)),
        Err(_) => Err(HttpError::Timeout(limit)),
    }
}

fn literal(url: &Url) -> Option<IpAddr> {
    match url.host()? {
        url::Host::Ipv4(ip) => Some(ip.into()),
        url::Host::Ipv6(ip) => Some(ip.into()),
        url::Host::Domain(_) => None,
    }
}

/// Where a scoped hop's answer leads; `None` returns it as it is. Only 301,
/// 302, 303, 307 and 308 with a `Location` naming an http or https URL lead
/// on, and never from https to http.
fn next_hop(
    status: u16,
    location: Option<&HeaderValue>,
    from: &Url,
) -> Result<Option<Url>, HttpError> {
    if !matches!(status, 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }
    // UTF-8 as reqwest reads it; `join` percent-encodes what needs it.
    let next = location
        .and_then(|l| std::str::from_utf8(l.as_bytes()).ok())
        .and_then(|l| from.join(l).ok());
    match next {
        Some(next) if downgrades(from, &next) => {
            Err(HttpError::Network("redirect from https to http".into()))
        }
        Some(next) if matches!(next.scheme(), "http" | "https") => Ok(Some(next)),
        _ => Ok(None),
    }
}

/// Request headers no hop takes from a caller: the transport's own, plus
/// `Range` and `Accept-Encoding`, which the client sets itself. Every
/// `Proxy-*` header too.
const TRANSPORT: [&str; 11] = [
    "host",
    "connection",
    "keep-alive",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "content-length",
    "range",
    "expect",
    "accept-encoding",
];
/// What every origin gets. Hotlink-protected CDNs check `Referer` and
/// `Origin`, which name a site, not a secret.
const SAFE: [&str; 5] = [
    "user-agent",
    "referer",
    "origin",
    "accept",
    "accept-language",
];

/// The part of a stream's `headers` a scoped hop to `url` sends, when they
/// belong to `owner`'s origin: all of them on that origin, or on its https
/// upgrade (http on the default port to https on the default port, same
/// host); only `SAFE` anywhere else; never `TRANSPORT`.
pub fn headers_for(owner: &Url, url: &Url, headers: &[(String, String)]) -> Vec<(String, String)> {
    let owned = owner.origin() == url.origin() || upgrades(owner, url);
    headers
        .iter()
        .filter(|(name, _)| {
            let name = name.to_ascii_lowercase();
            let transport = TRANSPORT.contains(&name.as_str()) || name.starts_with("proxy-");
            !transport && (owned || SAFE.contains(&name.as_str()))
        })
        .cloned()
        .collect()
}

fn upgrades(owner: &Url, url: &Url) -> bool {
    owner.scheme() == "http"
        && url.scheme() == "https"
        && owner.host() == url.host()
        && owner.port().is_none()
        && url.port().is_none()
}

/// A response whose body arrives as a stream of chunks.
pub struct StreamingResponse {
    pub status: u16,
    pub headers: HeaderMap,
    /// Where the answer came from, after redirects.
    pub final_url: Url,
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

    #[tokio::test]
    async fn names_that_resolve_only_to_private_addresses_are_refused() {
        use reqwest::dns::Resolve;
        let name = "localhost".parse().unwrap();
        let err = PublicOnly.resolve(name).await.err().expect("refused");
        assert!(err.to_string().contains("private"), "{err}");
    }

    #[tokio::test]
    async fn a_network_error_says_why_and_never_the_address() {
        let client = HttpClient::new(ClientConfig::default()).unwrap();
        // A loopback port with nothing listening: the connection is refused.
        let err = client
            .get(
                "http://127.0.0.1:9/secret-path?token=abc",
                &RequestOptions {
                    allow_private: true,
                    ..RequestOptions::default()
                },
            )
            .await
            .expect_err("refused");
        let text = err.to_string();
        assert!(
            !text.contains("secret-path") && !text.contains("token=abc"),
            "{text}"
        );
        // The cause after the top line ("error sending request").
        assert!(text.matches(": ").count() >= 2, "{text}");
    }

    #[test]
    fn https_never_redirects_to_http() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert!(downgrades(
            &url("https://a.example/m"),
            &url("http://a.example/m")
        ));
        assert!(!downgrades(
            &url("http://a.example/m"),
            &url("https://b.example/m")
        ));
        assert!(!downgrades(
            &url("https://a.example/m"),
            &url("https://b.example/m")
        ));
    }

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

    #[test]
    fn a_decode_error_never_carries_what_it_read() {
        let response = Response {
            status: 200,
            headers: Default::default(),
            body: Bytes::from_static(br#"{"expires_in":"SECRET-TOKEN"}"#),
            final_url: Url::parse("https://api.example/token").unwrap(),
        };
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Token {
            expires_in: u64,
        }
        let err = response.json::<Token>().unwrap_err();
        assert!(!format!("{err} {err:?}").contains("SECRET"), "{err:?}");
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[tokio::test]
    async fn allowed_resolver_keeps_public_and_own() {
        use reqwest::dns::Resolve;
        let allowed = Allowed {
            local: vec![ip("127.0.0.1")],
            device: Default::default(),
        };
        let answer = [
            "8.8.8.8",
            "127.0.0.1",
            "127.0.0.3",
            "192.168.1.1",
            "fd00::1",
        ]
        .map(ip);
        test_dns::set("mixed.unit.test", &answer);
        let kept: Vec<IpAddr> = allowed
            .resolve("mixed.unit.test".parse().unwrap())
            .await
            .expect("two answers are allowed")
            .map(|a| a.ip())
            .collect();
        assert_eq!(kept, [ip("8.8.8.8"), ip("127.0.0.1")]);
        test_dns::set("denied.unit.test", &[ip("127.0.0.3"), ip("10.0.0.1")]);
        let err = allowed
            .resolve("denied.unit.test".parse().unwrap())
            .await
            .err()
            .expect("nothing is allowed");
        assert!(err.is::<Blocked>(), "{err}");
    }

    #[test]
    fn scoped_clients_are_shared_by_address_set() {
        let http = HttpClient::new(ClientConfig::default()).unwrap();
        // A fixed interface list: a real one could change between the calls.
        test_dns::set_interfaces(&http, Ok(Vec::new()));
        let (a, b) = (ip("127.0.0.1"), ip("192.168.1.5"));
        let _first = http.scoped(&[a, b]);
        let _reversed = http.scoped(&[b, a]);
        let _mapped = http.scoped(&[ip("::ffff:127.0.0.1"), b, b]);
        let built = &http.scopes.built;
        assert_eq!(built.load(std::sync::atomic::Ordering::Relaxed), 1);
        let _other = http.scoped(&[]);
        assert_eq!(built.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    #[test]
    fn headers_follow_their_owner() {
        let url = |s: &str| Url::parse(s).unwrap();
        let headers: Vec<(String, String)> = [
            ("X-Api-Key", "k"),
            ("Cookie", "c"),
            ("Referer", "https://site.example/"),
            ("User-Agent", "ua"),
            ("Host", "evil.example"),
            ("Range", "bytes=0-"),
            ("Proxy-Authorization", "p"),
            ("Accept-Encoding", "gzip"),
        ]
        .map(|(n, v)| (n.to_owned(), v.to_owned()))
        .into();
        let names = |owner: &str, to: &str| -> Vec<String> {
            headers_for(&url(owner), &url(to), &headers)
                .into_iter()
                .map(|(n, _)| n)
                .collect()
        };
        let all = ["X-Api-Key", "Cookie", "Referer", "User-Agent"];
        let safe = ["Referer", "User-Agent"];
        assert_eq!(names("http://a.example/v", "http://a.example/w"), all);
        // An upgrade to https on the same host keeps the owner.
        assert_eq!(names("http://a.example/v", "https://a.example/w"), all);
        assert_eq!(
            names("http://a.example/v", "https://a.example:8443/w"),
            safe
        );
        assert_eq!(names("https://a.example/v", "http://a.example/w"), safe);
        assert_eq!(names("http://a.example/v", "http://a.example:81/w"), safe);
        assert_eq!(names("http://a.example/v", "http://b.example/v"), safe);
    }

    #[test]
    fn a_redirect_never_downgrades_or_leaves_http() {
        let from = Url::parse("https://cdn.example/a/v.mkv").unwrap();
        let to = |status: u16, location: &str| {
            next_hop(
                status,
                Some(&HeaderValue::from_str(location).unwrap()),
                &from,
            )
        };
        let next = to(302, "../b/v.mkv").unwrap().expect("followed");
        assert_eq!(next.as_str(), "https://cdn.example/b/v.mkv");
        let err = to(301, "http://cdn.example/v.mkv").expect_err("downgrade");
        assert!(err.to_string().contains("https to http"), "{err}");
        for location in ["file:///etc/passwd", "ftp://cdn.example/v.mkv", "data:,x"] {
            assert!(to(302, location).unwrap().is_none(), "{location}");
        }
        assert!(next_hop(302, None, &from).unwrap().is_none());
        assert!(to(300, "/x").unwrap().is_none());
        // A UTF-8 file name, as some servers send it, is followed encoded.
        let utf8 = HeaderValue::from_bytes("../b/Amélie.mkv".as_bytes()).unwrap();
        let next = next_hop(302, Some(&utf8), &from)
            .unwrap()
            .expect("followed");
        assert_eq!(next.as_str(), "https://cdn.example/b/Am%C3%A9lie.mkv");
    }

    #[test]
    fn https_to_http_redirect_is_refused() {
        let url = |s: &str| Url::parse(s).unwrap();
        let secure = url("https://addon.example/manifest.json");
        assert!(downgrades(
            &secure,
            &url("http://addon.example/manifest.json")
        ));
        assert!(!downgrades(&secure, &url("https://cdn.example/m.json")));
        assert!(!downgrades(&url("http://addon.example/"), &secure));
        assert!(!downgrades(
            &url("http://addon.example/a"),
            &url("http://addon.example/b")
        ));
    }
}
