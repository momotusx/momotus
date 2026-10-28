//! The natives behind `js/prelude.js` (plans/plugins-and-marketplace.md 6.4):
//! `fetch` as a message to the host, timers, `console`, bytes, base64,
//! random values, URLs and crypto-js. The prelude gets them as one object
//! the plugin never sees. Every count here is per call, and a call has its
//! own thread, so the counts are thread-locals.

use crate::engine::{
    FetchRequest, FetchResponse, HostCall, HostReply, HostRequest, LogLevel, MAX_REQUESTS, Method,
    host,
};
use base64::Engine;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD};
use rquickjs::prelude::{Func, Opt};
use rquickjs::promise::Promised;
use rquickjs::{Array, Ctx, Exception, Object, Result, TypedArray, Value};
use std::cell::Cell;
use std::future::Future;
use std::time::Duration;
use tokio::sync::oneshot;
use url::Url;

/// What the decoded bodies and the parsed documents of one call may hold in
/// all: the host bytes of 6.2.
pub const HOST_BYTES: usize = 16 * 1024 * 1024;
/// One response's decoded body at most (6.4).
pub const MAX_BODY: usize = 4 * 1024 * 1024;
/// One request's body at most (6.4).
pub const MAX_REQUEST_BODY: usize = 256 * 1024;
pub const MAX_HEADERS: usize = 32;
pub const MAX_HEADER: usize = 8 * 1024;
/// Timers per call, and the longest one (6.4).
pub const MAX_TIMERS: u32 = 1000;
pub const MAX_SLEEP: Duration = Duration::from_secs(10);
/// What `console` may say per call (6.4).
pub const MAX_LOG: usize = 4 * 1024;

thread_local! {
    static SENT: Cell<u32> = const { Cell::new(0) };
    static TIMERS: Cell<u32> = const { Cell::new(0) };
    static LOGGED: Cell<usize> = const { Cell::new(0) };
    static SPENT_BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Takes `bytes` of the call's host bytes, or says how many were left.
pub fn charge(bytes: usize) -> std::result::Result<(), usize> {
    let spent = SPENT_BYTES.get();
    let left = HOST_BYTES.saturating_sub(spent);
    if bytes > left {
        return Err(left);
    }
    SPENT_BYTES.set(spent + bytes);
    Ok(())
}

fn type_error<T>(ctx: &Ctx<'_>, words: &str) -> Result<T> {
    Err(Exception::throw_type(ctx, words))
}

/// The natives, as one object for the prelude.
pub fn natives<'js>(ctx: &Ctx<'js>) -> Result<Object<'js>> {
    let n = Object::new(ctx.clone())?;
    n.set("fetch", Func::from(fetch))?;
    n.set("sleep", Func::from(sleep))?;
    n.set("log", Func::from(log))?;
    n.set("random", Func::from(random))?;
    n.set("encode", Func::from(encode))?;
    n.set("decode", Func::from(decode))?;
    n.set("atob", Func::from(atob))?;
    n.set("btoa", Func::from(btoa))?;
    n.set("urlParse", Func::from(url_parse))?;
    n.set("urlGet", Func::from(url_get))?;
    n.set("urlSet", Func::from(url_set))?;
    n.set("formParse", Func::from(form_parse))?;
    n.set("formSerialize", Func::from(form_serialize))?;
    n.set("cryptoJs", Func::from(crypto_js))?;
    crate::cheerio::install(&n)?;
    Ok(n)
}

/// A typed array's bytes, copied out before any JS runs again.
fn bytes(array: &TypedArray<'_, u8>) -> Vec<u8> {
    // SAFETY: the slice is copied at once, with no JS run while it lives.
    unsafe { array.as_bytes() }
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

/// `fetch`: checked and counted before anything is sent, so a loop of calls
/// past the cap leaves nothing on the host side; the promise resolves to
/// `[status, statusText, url, [[name, value]], Uint8Array]`.
fn fetch<'js>(
    ctx: Ctx<'js>,
    method: String,
    url: String,
    headers: Array<'js>,
    body: Option<TypedArray<'js, u8>>,
    follow: bool,
) -> Result<Promised<impl Future<Output = Result<Array<'js>>> + 'js>> {
    if SENT.get() >= MAX_REQUESTS {
        return type_error(&ctx, "rate limited");
    }
    let method = match method.as_str() {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "HEAD" => Method::Head,
        _ => return type_error(&ctx, "method not allowed"),
    };
    if headers.len() > MAX_HEADERS {
        return type_error(&ctx, "too many headers");
    }
    let mut pairs = Vec::with_capacity(headers.len());
    for pair in headers.iter::<Array>() {
        let pair = pair?;
        let (name, value): (String, String) = (pair.get(0)?, pair.get(1)?);
        if name.len() + value.len() > MAX_HEADER {
            return type_error(&ctx, "header too long");
        }
        pairs.push((name, value));
    }
    let body = body.map(|b| bytes(&b));
    if body.as_ref().is_some_and(|b| b.len() > MAX_REQUEST_BODY) {
        return type_error(&ctx, "request body over 256 KiB");
    }
    let left = HOST_BYTES.saturating_sub(SPENT_BYTES.get());
    if left == 0 {
        return type_error(&ctx, "the call's 16 MiB are spent");
    }
    SENT.set(SENT.get() + 1);
    let (reply, answer) = oneshot::channel();
    let request = FetchRequest {
        method,
        url,
        headers: pairs,
        body,
        follow,
        max_bytes: left.min(MAX_BODY),
    };
    let max_bytes = request.max_bytes;
    if host(HostRequest {
        call: HostCall::Fetch(request),
        reply,
    })
    .is_err()
    {
        return Err(Exception::throw_message(&ctx, "the host is gone"));
    }
    Ok(Promised(async move {
        let response = match answer.await {
            Ok(HostReply::Response(response)) => response,
            Ok(HostReply::Failed(words)) => return type_error(&ctx, &words),
            Err(_) => return Err(Exception::throw_message(&ctx, "the host is gone")),
        };
        let FetchResponse {
            status,
            status_text,
            url,
            headers,
            body,
        } = response;
        if body.len() > max_bytes || charge(body.len()).is_err() {
            return type_error(&ctx, "response over the call's bytes");
        }
        let list = Array::new(ctx.clone())?;
        for (i, (name, value)) in headers.into_iter().enumerate() {
            let pair = Array::new(ctx.clone())?;
            pair.set(0, name)?;
            pair.set(1, value)?;
            list.set(i, pair)?;
        }
        let out = Array::new(ctx.clone())?;
        out.set(0, status)?;
        out.set(1, status_text)?;
        out.set(2, url)?;
        out.set(3, list)?;
        out.set(4, TypedArray::<u8>::new(ctx.clone(), body)?)?;
        Ok(out)
    }))
}

fn sleep<'js>(ctx: Ctx<'js>, ms: f64) -> Result<Promised<impl Future<Output = ()> + 'js>> {
    if TIMERS.get() >= MAX_TIMERS {
        return type_error(&ctx, "too many timers");
    }
    TIMERS.set(TIMERS.get() + 1);
    let wait = Duration::from_secs_f64((ms / 1000.0).clamp(0.0, MAX_SLEEP.as_secs_f64()));
    Ok(Promised(tokio::time::sleep(wait)))
}

/// One `console` line to the host, within the call's budget; no answer.
fn log(level: i32, text: String) {
    let left = MAX_LOG.saturating_sub(LOGGED.get());
    if left == 0 {
        return;
    }
    let mut at = left.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    LOGGED.set(LOGGED.get() + at.max(1));
    let level = match level {
        0 => LogLevel::Debug,
        1 => LogLevel::Info,
        2 => LogLevel::Warn,
        _ => LogLevel::Error,
    };
    let (reply, _) = oneshot::channel();
    let _ = host(HostRequest {
        call: HostCall::Log {
            level,
            text: text[..at].to_owned(),
        },
        reply,
    });
}

fn random<'js>(ctx: Ctx<'js>, n: usize) -> Result<TypedArray<'js, u8>> {
    if n > 65536 {
        return type_error(&ctx, "at most 65536 bytes");
    }
    let mut buf = vec![0; n];
    if getrandom::fill(&mut buf).is_err() {
        return Err(Exception::throw_message(&ctx, "no randomness"));
    }
    TypedArray::new(ctx, buf)
}

fn encode<'js>(ctx: Ctx<'js>, text: String) -> Result<TypedArray<'js, u8>> {
    TypedArray::new(ctx, text.into_bytes())
}

/// UTF-8 only, a leading BOM dropped, as `TextDecoder` does.
fn decode(ctx: Ctx<'_>, data: TypedArray<'_, u8>, fatal: bool) -> Result<String> {
    let data = bytes(&data);
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data);
    if fatal {
        return match std::str::from_utf8(data) {
            Ok(text) => Ok(text.to_owned()),
            Err(_) => type_error(&ctx, "invalid UTF-8"),
        };
    }
    Ok(String::from_utf8_lossy(data).into_owned())
}

/// The forgiving base64 of the HTML standard.
const FORGIVING: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

fn atob(ctx: Ctx<'_>, data: String) -> Result<String> {
    let clean: String = data
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' '))
        .collect();
    let trimmed = if clean.len() % 4 == 0 {
        clean
            .strip_suffix("==")
            .or(clean.strip_suffix('='))
            .unwrap_or(&clean)
    } else {
        &clean
    };
    if trimmed.len() % 4 == 1 || trimmed.contains('=') {
        return type_error(&ctx, "invalid base64");
    }
    match FORGIVING.decode(trimmed) {
        Ok(bytes) => Ok(bytes.into_iter().map(char::from).collect()),
        Err(_) => type_error(&ctx, "invalid base64"),
    }
}

fn btoa(ctx: Ctx<'_>, data: String) -> Result<String> {
    let mut bytes = Vec::with_capacity(data.len());
    for c in data.chars() {
        match u8::try_from(u32::from(c)) {
            Ok(b) => bytes.push(b),
            Err(_) => return type_error(&ctx, "a character outside Latin-1"),
        }
    }
    Ok(STANDARD.encode(bytes))
}

fn url_parse(ctx: Ctx<'_>, input: String, base: Opt<String>) -> Result<String> {
    let base = match base.0 {
        Some(base) => match Url::parse(&base) {
            Ok(base) => Some(base),
            Err(_) => return type_error(&ctx, "Invalid base URL"),
        },
        None => None,
    };
    match Url::options().base_url(base.as_ref()).parse(&input) {
        Ok(url) => Ok(url.into()),
        Err(_) => type_error(&ctx, "Invalid URL"),
    }
}

fn url_get(ctx: Ctx<'_>, href: String, part: String) -> Result<String> {
    let Ok(url) = Url::parse(&href) else {
        return type_error(&ctx, "Invalid URL");
    };
    use url::quirks as q;
    Ok(match part.as_str() {
        "href" => q::href(&url).to_owned(),
        "origin" => q::origin(&url),
        "protocol" => q::protocol(&url).to_owned(),
        "username" => q::username(&url).to_owned(),
        "password" => q::password(&url).to_owned(),
        "host" => q::host(&url).to_owned(),
        "hostname" => q::hostname(&url).to_owned(),
        "port" => q::port(&url).to_owned(),
        "pathname" => q::pathname(&url).to_owned(),
        "search" => q::search(&url).to_owned(),
        "hash" => q::hash(&url).to_owned(),
        _ => return type_error(&ctx, "unknown URL part"),
    })
}

/// A URL setter: an invalid value leaves the URL as it was, as the standard
/// has it, except for `href`, which throws.
fn url_set(ctx: Ctx<'_>, href: String, part: String, value: String) -> Result<String> {
    let Ok(mut url) = Url::parse(&href) else {
        return type_error(&ctx, "Invalid URL");
    };
    use url::quirks as q;
    match part.as_str() {
        "href" => {
            if q::set_href(&mut url, &value).is_err() {
                return type_error(&ctx, "Invalid URL");
            }
        }
        "protocol" => drop(q::set_protocol(&mut url, &value)),
        "username" => drop(q::set_username(&mut url, &value)),
        "password" => drop(q::set_password(&mut url, &value)),
        "host" => drop(q::set_host(&mut url, &value)),
        "hostname" => drop(q::set_hostname(&mut url, &value)),
        "port" => drop(q::set_port(&mut url, &value)),
        "pathname" => q::set_pathname(&mut url, &value),
        "search" => q::set_search(&mut url, &value),
        "hash" => q::set_hash(&mut url, &value),
        _ => return type_error(&ctx, "unknown URL part"),
    }
    Ok(url.into())
}

fn form_parse<'js>(ctx: Ctx<'js>, query: String) -> Result<Array<'js>> {
    let out = Array::new(ctx.clone())?;
    for (i, (name, value)) in url::form_urlencoded::parse(query.as_bytes()).enumerate() {
        let pair = Array::new(ctx.clone())?;
        pair.set(0, name.as_ref())?;
        pair.set(1, value.as_ref())?;
        out.set(i, pair)?;
    }
    Ok(out)
}

fn form_serialize(list: Array<'_>) -> Result<String> {
    let mut out = url::form_urlencoded::Serializer::new(String::new());
    for pair in list.iter::<Array>() {
        let pair = pair?;
        out.append_pair(&pair.get::<String>(0)?, &pair.get::<String>(1)?);
    }
    Ok(out.finish())
}

/// crypto-js 4.2.0 (`js/crypto-js-4.2.0.js`, its SHA-256 pinned by a test),
/// evaluated on first use: most plugins never ask for it.
pub const CRYPTO_JS: &str = include_str!("../js/crypto-js-4.2.0.js");

fn crypto_js<'js>(ctx: Ctx<'js>) -> Result<Value<'js>> {
    let module: Object = ctx.eval("({ exports: {} })")?;
    let load: rquickjs::Function =
        ctx.eval(format!("(function (module, exports) {{\n{CRYPTO_JS}\n}})"))?;
    load.call::<_, ()>((module.clone(), module.get::<_, Value>("exports")?))?;
    module.get("exports")
}
