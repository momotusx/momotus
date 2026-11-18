//! The webOS core (plans/webos.md 3.5): today's `Session` over the core, as a
//! wasm module the service's core child loads on the TV's Node (8 to 20).
//! The contract is apps/webos/service/host.d.ts: `open`, `send`,
//! `onMessage`, `serve` and `ping` are here; `fire`, `fetched` and
//! `resolved` are mq-rt's and mq-http's exports. Every host import lives in
//! the `mqHost` the child installs before it loads this module.

#![cfg(target_arch = "wasm32")]

mod log;

use mq_bridge::{Outgoing, Output, Session};
use mq_runtime::{Core, CoreConfig};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::mpsc;
use wasm_bindgen::prelude::*;

thread_local! {
    static INBOX: RefCell<Option<mpsc::UnboundedSender<String>>> = const { RefCell::new(None) };
    static LISTENER: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static CORE: RefCell<Option<Core>> = const { RefCell::new(None) };
}

/// host.d.ts `CoreConfig`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    data_dir: String,
    /// 64 hex characters, kept by the service in the data dir.
    device_seed: String,
    node: String,
    /// `http://127.0.0.1:<port>`: what subtitle and logo URLs name.
    origin: String,
    /// The service's write probe failed: the library lives in memory.
    #[serde(default)]
    memory_only: bool,
    #[serde(default)]
    #[cfg_attr(not(all(feature = "dev-hooks", debug_assertions)), allow(dead_code))]
    dev: HashMap<String, String>,
}

/// The device id the seed names: its first 64 bits, never 0 (0 asks the
/// core for a random one).
fn device_id(seed: &str) -> Result<u64, JsError> {
    let id = seed
        .get(..16)
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .ok_or_else(|| JsError::new("deviceSeed is not hex"))?;
    Ok(id.max(1))
}

/// `http://127.0.0.1:47801` as an address: an IP literal and a port.
fn origin(text: &str) -> Result<SocketAddr, JsError> {
    let url = url::Url::parse(text).map_err(|_| JsError::new("origin is not a URL"))?;
    let ip = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.into(),
        Some(url::Host::Ipv6(ip)) => ip.into(),
        _ => return Err(JsError::new("origin is not an IP address")),
    };
    let port = url
        .port_or_known_default()
        .ok_or_else(|| JsError::new("origin has no port"))?;
    Ok(SocketAddr::new(ip, port))
}

/// Opens the core over `config.dataDir` and starts the session; replies and
/// pushes go to `onMessage`'s callback. Answers what opened, as JSON.
#[wasm_bindgen]
pub fn open(config: &str) -> Result<String, JsError> {
    mq_rt::set_panic_hook();
    log::init();
    let config: Config = serde_json::from_str(config)?;
    mq_server::set_origin(origin(&config.origin)?);
    #[cfg(all(feature = "dev-hooks", debug_assertions))]
    mq_bridge::set_dev_env(config.dev);
    let core = Core::open(CoreConfig {
        data_dir: config.data_dir.into(),
        profile_id: "default".into(),
        device_id: device_id(&config.device_seed)?,
        memory_only: config.memory_only,
    })
    .map_err(|e| JsError::new(&e.to_string()))?;
    mq_bridge::dev_trakt(&core);
    mq_bridge::dev_tmdb(&core);
    mq_bridge::dev_segments(&core);
    mq_bridge::dev_replay();

    let (out, mut outgoing) = Output::channel();
    let (inbox, mut incoming) = mpsc::unbounded_channel::<String>();
    mq_rt::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            match message {
                Outgoing::Message { text, held } => {
                    deliver(&text);
                    // Handed over: its share of the budget goes back.
                    drop(held);
                }
                // The page stopped taking messages past the budget: the
                // service replaces this core, as after a death.
                Outgoing::Overflow => {
                    deliver(r#"{"core":"overflow"}"#);
                    return;
                }
            }
        }
    });
    let session_core = core.clone();
    mq_rt::spawn(async move {
        let mut session = Session::new(session_core, out);
        while let Some(text) = incoming.recv().await {
            session.handle_text(&text).await;
        }
    });
    INBOX.with(|i| *i.borrow_mut() = Some(inbox));
    CORE.with(|c| *c.borrow_mut() = Some(core));
    Ok(serde_json::json!({
        "ok": true,
        "core": env!("CARGO_PKG_VERSION"),
        "node": config.node,
        "memoryOnly": config.memory_only,
    })
    .to_string())
}

/// Where replies and pushes go, one JSON text each.
#[wasm_bindgen(js_name = onMessage)]
pub fn on_message(callback: js_sys::Function) {
    LISTENER.with(|l| *l.borrow_mut() = Some(callback));
}

fn deliver(text: &str) {
    LISTENER.with(|l| {
        if let Some(callback) = l.borrow().as_ref() {
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(text));
        }
    });
}

/// One protocol request (JSON text), handled on a later turn.
#[wasm_bindgen]
pub fn send(json: String) {
    INBOX.with(|i| {
        if let Some(inbox) = i.borrow().as_ref() {
            let _ = inbox.send(json);
        }
    });
}

/// The service's `/sub/` and `/logo/` routes: `{ status, headers, body }`,
/// or `null` for 404.
#[wasm_bindgen]
pub async fn serve(path: String) -> JsValue {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return JsValue::NULL;
    };
    let Ok(server) = core.stream_server().await else {
        return JsValue::NULL;
    };
    let Some(answer) = server.answer(&path).await else {
        return JsValue::NULL;
    };
    let headers = js_sys::Array::new();
    for (name, value) in &answer.headers {
        headers.push(&js_sys::Array::of2(&name.into(), &value.into()));
    }
    let out = js_sys::Object::new();
    let set = |key: &str, value: &JsValue| js_sys::Reflect::set(&out, &key.into(), value);
    let _ = set("status", &answer.status.into());
    let _ = set("headers", &headers);
    // A copy in JS memory: the host keeps it past this call.
    let _ = set("body", &js_sys::Uint8Array::from(&answer.body[..]));
    out.into()
}

/// The module loaded and runs: the service's start check.
#[wasm_bindgen]
pub fn ping() -> String {
    serde_json::json!({ "pong": true, "core": env!("CARGO_PKG_VERSION") }).to_string()
}
