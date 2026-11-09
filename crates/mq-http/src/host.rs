//! The webOS core's transport (plans/webos.md 3.2): the service's Node host
//! makes each hop (`mqHost.fetch`, apps/webos/service/host.d.ts) to the
//! addresses Rust approved, and Rust follows the redirects. Unscoped and
//! scoped requests run through the one loop here, so the hop cap, the https
//! downgrade refusal, `HopRule`, the address rules and the headers that reach
//! only their owner's origin hold in one place. Budgets, caps, timeouts,
//! redaction and error kinds are the native client's. Native test builds run
//! it against a fake host (`imports` below).

use crate::{
    Body, HttpClient, HttpError, MAX_REDIRECTS, Method, OUT_OF_HOPS, OUT_OF_SCOPE, Request,
    RequestOptions, Response, Url, build_headers, header_limit, headers_for, host_tag,
    is_private_host, is_private_ip, literal, lock, next_hop, redact_str, redirect_allowed,
    same_origin_credentials,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use http::header::{
    AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, LOCATION, RETRY_AFTER,
    USER_AGENT,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
mod imports {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = mqHost)]
        pub fn resolve(id: u32, host: &str);
        #[wasm_bindgen(js_namespace = mqHost)]
        pub fn interfaces() -> String;
        #[wasm_bindgen(js_namespace = mqHost)]
        pub fn fetch(id: u32, request: &str);
        #[wasm_bindgen(js_namespace = mqHost)]
        pub fn abort(id: u32);
        /// Monotonic milliseconds (`perf_hooks` on Node 8, installed by host.js).
        #[wasm_bindgen(js_namespace = performance, js_name = now)]
        pub fn now_ms() -> f64;
    }
}

/// A host the tests drive: names it resolves, an answer per request (JSON
/// null never answers), and what it was asked. Answers come on a later turn
/// (a spawned task), as host.d.ts requires of the real one.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod imports {
    use std::cell::RefCell;
    use std::collections::HashMap;

    type Answer = Box<dyn Fn(&serde_json::Value) -> serde_json::Value>;

    #[derive(Default)]
    pub(crate) struct Fake {
        pub names: HashMap<String, Vec<String>>,
        pub answer: Option<Answer>,
        pub requests: Vec<serde_json::Value>,
        pub aborted: Vec<u32>,
    }

    thread_local! {
        pub(crate) static FAKE: RefCell<Fake> = RefCell::default();
    }

    pub fn resolve(id: u32, host: &str) {
        let list = FAKE.with(|f| f.borrow().names.get(host).cloned().unwrap_or_default());
        let json = serde_json::to_string(&list).unwrap();
        tokio::spawn(async move { super::resolved(id, &json) });
    }

    pub fn fetch(id: u32, request: &str) {
        let request: serde_json::Value = serde_json::from_str(request).unwrap();
        let answer = FAKE.with(|f| {
            let mut f = f.borrow_mut();
            f.requests.push(request.clone());
            f.answer.as_ref().map(|a| a(&request))
        });
        if let Some(answer) = answer.filter(|a| !a.is_null()) {
            tokio::spawn(async move { super::fetched(id, &answer.to_string()) });
        }
    }

    pub fn abort(id: u32) {
        FAKE.with(|f| f.borrow_mut().aborted.push(id));
    }

    pub fn now_ms() -> f64 {
        static START: std::sync::LazyLock<std::time::Instant> =
            std::sync::LazyLock::new(std::time::Instant::now);
        START.elapsed().as_secs_f64() * 1000.0
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn interfaces_json() -> String {
    imports::interfaces()
}

/// A host call's state: waiting (with the task to wake), or answered.
enum Slot {
    Waiting(Option<Waker>),
    Done(String),
}

type Slots = Mutex<BTreeMap<u32, Slot>>;

static FETCHES: Slots = Mutex::new(BTreeMap::new());
static LOOKUPS: Slots = Mutex::new(BTreeMap::new());
static NEXT: AtomicU32 = AtomicU32::new(1);

/// Stores an answer and wakes its task outside the lock (the no-threads
/// `Mutex` panics on a nested lock). An answer nobody waits for (its call
/// was dropped) is forgotten.
fn complete(slots: &Slots, id: u32, answer: &str) {
    let waker = {
        let mut slots = lock(slots);
        if !matches!(slots.get(&id), Some(Slot::Waiting(_))) {
            return;
        }
        match slots.insert(id, Slot::Done(answer.to_owned())) {
            Some(Slot::Waiting(waker)) => waker,
            _ => None,
        }
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}

/// The host's answer to `fetch(id, request)`: host.d.ts `HostResponse`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn fetched(id: u32, response: &str) {
    complete(&FETCHES, id, response);
}

/// The host's answer to `resolve(id, host)`: a JSON array of addresses.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn resolved(id: u32, addresses: &str) {
    complete(&LOOKUPS, id, addresses);
}

/// One call to the host, finished by its export. It holds only an id, so it
/// is `Send`. Dropped unanswered, it forgets its slot and, for a fetch, tells
/// the host to abort.
struct Call {
    id: u32,
    slots: &'static Slots,
    abort: bool,
    done: bool,
}

impl Call {
    fn start(slots: &'static Slots, abort: bool, send: impl FnOnce(u32)) -> Call {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        lock(slots).insert(id, Slot::Waiting(None));
        send(id);
        Call {
            id,
            slots,
            abort,
            done: false,
        }
    }
}

impl Future for Call {
    type Output = String;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<String> {
        let id = self.id;
        let mut slots = lock(self.slots);
        match slots.remove(&id) {
            Some(Slot::Done(answer)) => {
                self.done = true;
                Poll::Ready(answer)
            }
            _ => {
                slots.insert(id, Slot::Waiting(Some(cx.waker().clone())));
                Poll::Pending
            }
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        if !self.done {
            lock(self.slots).remove(&self.id);
            if self.abort {
                imports::abort(self.id);
            }
        }
    }
}

/// `host`'s addresses through Node's `dns.lookup`, in the host's order
/// (IPv4 first); none on failure.
async fn resolve(host: &str) -> Vec<IpAddr> {
    let answer = Call::start(&LOOKUPS, false, |id| imports::resolve(id, host)).await;
    let list: Vec<String> = serde_json::from_str(&answer).unwrap_or_default();
    list.iter()
        .filter_map(|a| a.split('%').next()?.parse().ok())
        .collect()
}

/// What `fetch` sends: host.d.ts `HostRequest`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HostRequest<'a> {
    method: &'a str,
    url: &'a str,
    headers: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    addresses: Vec<String>,
    timeout_ms: u64,
    max_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    prefix_bytes: Option<usize>,
}

/// What `fetched` gets: host.d.ts `HostResponse`.
#[derive(serde::Deserialize)]
struct HostResponse {
    status: Option<u16>,
    #[serde(default)]
    headers: Vec<(String, String)>,
    body: Option<String>,
    #[serde(default)]
    truncated: bool,
    error: Option<String>,
}

/// The first bytes of an answer, whatever its status (plans/webos.md 3.4).
#[derive(Debug, Clone)]
pub struct Prefix {
    pub response: Response,
    /// The body went on past the bytes read.
    pub truncated: bool,
}

/// A host failure as the native client words it: `dns error` keeps
/// `HttpError::unresolved` true, and no text names the address.
fn host_error(kind: &str, opts: &RequestOptions) -> HttpError {
    match kind {
        "timeout" => HttpError::Timeout(opts.timeout),
        "cap" => HttpError::TooLarge {
            limit: opts.max_bytes,
        },
        "dns" => HttpError::Network("dns error: the name did not resolve".into()),
        "refused" => HttpError::Network("connection refused".into()),
        "tls" => HttpError::Network("tls: the server's certificate was refused".into()),
        _ => HttpError::Network("connection reset".into()),
    }
}

impl HttpClient {
    /// `send`'s transport on webOS: the whole answer, a non-2xx status as
    /// `HttpError::Status`, as `read_capped` returns them natively.
    pub(crate) async fn host_send(
        &self,
        req: Request,
        opts: &RequestOptions,
    ) -> Result<Response, HttpError> {
        let (response, _) = self.host_fetch(req, opts, None).await?;
        if !(200..300).contains(&response.status) {
            let retry_after = response
                .headers
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(HttpError::Status {
                status: response.status,
                body: response.body,
                retry_after,
            });
        }
        Ok(response)
    }

    /// webOS only (plans/webos.md 3.4, `detect`): at most `len` bytes of the
    /// answer to a Range request for `len` bytes from `offset`, whatever its
    /// status, with redirects followed as `get_range` follows them. The host
    /// drops the connection after them, so an oversized body is read only
    /// that far and never fails as too large.
    pub async fn get_prefix(
        &self,
        url: &str,
        offset: u64,
        len: u64,
        opts: &RequestOptions,
    ) -> Result<Prefix, HttpError> {
        let parsed = Url::parse(url).map_err(|_| HttpError::InvalidUrl(redact_str(url)))?;
        self.check_hops(&parsed)?;
        let replay = self.replayed(&parsed, opts);
        let (sent, opts) = match &replay {
            Some((to, replay_opts)) => (to.clone(), replay_opts),
            None => (parsed.clone(), opts),
        };
        let end = offset + len.saturating_sub(1);
        let request = Request {
            method: Method::GET,
            url: sent,
            body: None,
            range: Some(format!("bytes={offset}-{end}")),
            timeout: Some(opts.timeout),
        };
        let via = self.via(replay.is_some());
        let prefix = usize::try_from(len).unwrap_or(usize::MAX);
        let (mut response, truncated) = via.host_fetch(request, opts, Some(prefix)).await?;
        if replay.is_some() {
            response.final_url = parsed;
        }
        Ok(Prefix {
            response,
            truncated,
        })
    }

    /// Every hop of one request, timed as `fetch` times them natively.
    async fn host_fetch(
        &self,
        req: Request,
        opts: &RequestOptions,
        prefix: Option<usize>,
    ) -> Result<(Response, bool), HttpError> {
        let host = host_tag(req.url.host_str().unwrap_or_default());
        let started = imports::now_ms();
        let fetched = self.host_follow(req, opts, prefix).await;
        tracing::debug!(
            target: "mq_http::timing",
            host,
            ms = (imports::now_ms() - started) as u64,
            ok = fetched.is_ok(),
            "headers"
        );
        fetched
    }

    /// The scoped `follow` loop for every request: each hop takes its host
    /// slot, passes the hop rule and the address rules, gets the headers its
    /// origin may see, and goes to the host with the time left of the
    /// request's total. Release before acquire, as natively.
    async fn host_follow(
        &self,
        mut req: Request,
        opts: &RequestOptions,
        prefix: Option<usize>,
    ) -> Result<(Response, bool), HttpError> {
        let first = req.url.clone();
        let owner = opts.owner.clone().unwrap_or_else(|| req.url.clone());
        let total = req.timeout.unwrap_or(opts.timeout);
        let deadline = imports::now_ms() + total.as_secs_f64() * 1000.0;
        let mut hops = 0;
        loop {
            self.check_hops(&req.url)?;
            let host = req.url.host_str().unwrap_or_default().to_owned();
            // ponytail: the slot wait has no timer of its own here (the host
            // has the only clock); a request that waited past its deadline
            // fails below before it sends. `mq_rt::timeout_at` bounds it once
            // mq-rt lands.
            let permit = self.budget.acquire(&host, opts.priority).await;
            let addresses = self.approve(&req.url, &first, hops, opts).await?;
            let left = deadline - imports::now_ms();
            if left <= 0.0 {
                return Err(HttpError::Timeout(opts.timeout));
            }
            // A hop's own limit covers its body too here: the host buffers it.
            let hop_limit = header_limit(&req, opts).as_secs_f64() * 1000.0;
            let timeout_ms = left.min(hop_limit).ceil().max(1.0) as u64;
            let (status, headers, body, truncated) = self
                .hop(&req, &owner, addresses, timeout_ms, opts, prefix)
                .await?;
            let mut sent = req.url.clone();
            let _ = sent.set_username("");
            let _ = sent.set_password(None);
            let done = |status, headers, body| {
                let response = Response {
                    status,
                    headers,
                    body,
                    final_url: sent.clone(),
                };
                Ok((response, truncated))
            };
            if self.hops.as_ref().is_some_and(|rule| !rule.follow) {
                return done(status, headers, body);
            }
            // Under a hop rule a downgrade to http is the rule's refusal too.
            let next =
                next_hop(status, headers.get(LOCATION), &req.url).map_err(|e| match self.hops {
                    Some(_) => HttpError::BlockedPrivate(OUT_OF_HOPS.into()),
                    None => e,
                })?;
            let Some(next) = next else {
                return done(status, headers, body);
            };
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

    /// Where a hop may connect, in order, IPv4 first. Scoped: the handle's
    /// `Allowed`, its literal or every answer checked. Unscoped: the first URL
    /// is checked against `allow_private`, no redirect leads from a public
    /// address to a private one, and without `allow_private` a name's private
    /// answers are dropped (`PublicOnly`).
    async fn approve(
        &self,
        url: &Url,
        first: &Url,
        hop: usize,
        opts: &RequestOptions,
    ) -> Result<Vec<IpAddr>, HttpError> {
        let host = url.host_str().unwrap_or_default();
        let scope = self
            .scope
            .as_ref()
            .map(|local| self.scopes.client(local).allowed);
        if scope.is_none() {
            if hop == 0 && !opts.allow_private && is_private_host(host) {
                return Err(HttpError::BlockedPrivate(host.to_owned()));
            }
            if !redirect_allowed(first, url) {
                return Err(HttpError::Network(
                    "redirect from a public to a private address".into(),
                ));
            }
        }
        let found = match literal(url) {
            Some(ip) => vec![ip],
            None => resolve(host).await,
        };
        if found.is_empty() {
            return Err(host_error("dns", opts));
        }
        let mut kept: Vec<IpAddr> = found
            .into_iter()
            .filter(|ip| match &scope {
                Some(allowed) => allowed.permits(*ip),
                None => opts.allow_private || !is_private_ip(*ip),
            })
            .collect();
        if kept.is_empty() {
            return Err(match scope {
                Some(_) => HttpError::BlockedPrivate(OUT_OF_SCOPE.into()),
                None => HttpError::Network(
                    "dns error: the name resolves only to private addresses".into(),
                ),
            });
        }
        kept.sort_by_key(|ip| !ip.is_ipv4());
        Ok(kept)
    }

    /// One hop through the host: status, headers, the decoded body, and
    /// whether a prefix read stopped early.
    async fn hop(
        &self,
        req: &Request,
        owner: &Url,
        addresses: Vec<IpAddr>,
        timeout_ms: u64,
        opts: &RequestOptions,
        prefix: Option<usize>,
    ) -> Result<(u16, HeaderMap, Bytes, bool), HttpError> {
        let request = self.host_request(req, owner, addresses, timeout_ms, opts, prefix);
        let answer = Call::start(&FETCHES, true, |id| imports::fetch(id, &request)).await;
        let bad = || HttpError::Network("unreadable host answer".into());
        let answer: HostResponse = serde_json::from_str(&answer).map_err(|_| bad())?;
        if let Some(kind) = answer.error {
            return Err(host_error(&kind, opts));
        }
        let status = answer.status.ok_or_else(bad)?;
        let mut headers = HeaderMap::new();
        for (name, value) in &answer.headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::try_from(name.as_str()),
                HeaderValue::try_from(value.as_str()),
            ) {
                headers.append(n, v);
            }
        }
        let body = STANDARD
            .decode(answer.body.unwrap_or_default())
            .map_err(|_| bad())?;
        // The host caps the decoded bytes; this holds even if it did not.
        let limit = prefix.unwrap_or(opts.max_bytes);
        if body.len() > limit {
            return Err(HttpError::TooLarge {
                limit: opts.max_bytes,
            });
        }
        Ok((status, headers, body.into(), answer.truncated))
    }

    /// The JSON `fetch` gets. URL credentials go as Basic auth, as reqwest
    /// sends them, unless the caller set `Authorization`; the user agent is
    /// the client's unless the caller set one.
    fn host_request(
        &self,
        req: &Request,
        owner: &Url,
        addresses: Vec<IpAddr>,
        timeout_ms: u64,
        opts: &RequestOptions,
        prefix: Option<usize>,
    ) -> String {
        let mut headers = build_headers(
            &headers_for(owner, &req.url, &opts.headers),
            req.range.clone(),
        );
        let body = req
            .body
            .as_ref()
            .map(Body::encode)
            .map(|(content_type, bytes)| {
                if let Ok(value) = HeaderValue::try_from(content_type) {
                    headers.insert(CONTENT_TYPE, value);
                }
                STANDARD.encode(bytes)
            });
        if !headers.contains_key(USER_AGENT)
            && let Ok(value) = HeaderValue::try_from(self.scopes.config.user_agent.as_str())
        {
            headers.insert(USER_AGENT, value);
        }
        let mut url = req.url.clone();
        if !url.username().is_empty() || url.password().is_some() {
            let decode = |s: &str| {
                percent_encoding::percent_decode_str(s)
                    .decode_utf8_lossy()
                    .into_owned()
            };
            let user = decode(url.username());
            let pass = url.password().map(decode).unwrap_or_default();
            let basic = format!("Basic {}", STANDARD.encode(format!("{user}:{pass}")));
            if !headers.contains_key(AUTHORIZATION)
                && let Ok(value) = HeaderValue::try_from(basic)
            {
                headers.insert(AUTHORIZATION, value);
            }
            let _ = url.set_username("");
            let _ = url.set_password(None);
        }
        let headers = headers
            .iter()
            .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect();
        let request = HostRequest {
            method: req.method.as_str(),
            url: url.as_str(),
            headers,
            body,
            addresses: addresses.iter().map(IpAddr::to_string).collect(),
            timeout_ms,
            max_bytes: opts.max_bytes,
            prefix_bytes: prefix,
        };
        serde_json::to_string(&request).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
