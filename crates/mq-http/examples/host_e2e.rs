//! mq-http's webOS transport in Node, over the service's real host
//! (apps/webos/service/host.js) and a local fixture server:
//! `tests/host-e2e/run.sh` builds it by the plans/webos.md section 2 recipe
//! and `run.js` drives it under each TV Node. Empty on other targets.
#![cfg(target_arch = "wasm32")]

use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use std::time::Duration;
use wasm_bindgen::prelude::*;

/// What `get` is asked to do, JSON from run.js.
#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Ask {
    url: String,
    allow_private: bool,
    scoped: bool,
    max_bytes: Option<usize>,
    timeout_ms: Option<u64>,
    headers: Vec<(String, String)>,
    /// `get_prefix` for this many bytes instead of `get`.
    prefix: Option<u64>,
    /// Races the request against a `get` of this URL and drops the loser.
    race: Option<String>,
}

fn answer(result: Result<(u16, String, Vec<u8>, bool), HttpError>) -> String {
    match result {
        Ok((status, url, body, truncated)) => serde_json::json!({
            "status": status,
            "url": url,
            "body": String::from_utf8_lossy(&body),
            "len": body.len(),
            "truncated": truncated,
        }),
        Err(e) => serde_json::json!({
            "error": e.kind(),
            "unresolved": e.unresolved(),
            "text": e.to_string(),
        }),
    }
    .to_string()
}

async fn run(ask: Ask) -> String {
    let http = HttpClient::new(ClientConfig::default()).expect("a client");
    let http = if ask.scoped { http.scoped(&[]) } else { http };
    let mut opts = RequestOptions {
        allow_private: ask.allow_private,
        headers: ask.headers,
        ..Default::default()
    };
    if let Some(max) = ask.max_bytes {
        opts.max_bytes = max;
    }
    if let Some(ms) = ask.timeout_ms {
        opts.timeout = Duration::from_millis(ms);
    }
    if let Some(len) = ask.prefix {
        let got = http.get_prefix(&ask.url, 0, len, &opts).await;
        return answer(got.map(|p| {
            let r = p.response;
            (
                r.status,
                r.final_url.to_string(),
                r.body.to_vec(),
                p.truncated,
            )
        }));
    }
    let get = async {
        let r = http.get(&ask.url, &opts).await?;
        Ok((r.status, r.final_url.to_string(), r.body.to_vec(), false))
    };
    let Some(other) = ask.race else {
        return answer(get.await);
    };
    let other = async {
        let r = http.get(&other, &opts).await?;
        Ok((r.status, r.final_url.to_string(), r.body.to_vec(), false))
    };
    futures_util::pin_mut!(get, other);
    match futures_util::future::select(get, other).await {
        futures_util::future::Either::Left((won, _)) => answer(won),
        futures_util::future::Either::Right((won, _)) => answer(won),
    }
}

/// Runs one ask; resolves with `answer`'s JSON.
#[wasm_bindgen]
pub fn get(ask: &str) -> js_sys::Promise {
    let ask: Ask = serde_json::from_str(ask).unwrap_or_default();
    wasm_bindgen_futures::future_to_promise(async move { Ok(JsValue::from_str(&run(ask).await)) })
}
