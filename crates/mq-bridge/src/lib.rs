//! JSON protocol between the core and the web UI. The same session runs over
//! a WebSocket (dev server, browser, Playwright) and over the desktop shells'
//! IPC channel.
//!
//! Requests: `{"id", "op": "subscribe" | "unsubscribe" | "command" | "installAddon", ...}`.
//! Replies carry the request `id`; view pushes carry `sub` and `updates`.

use futures_util::StreamExt;
use mq_runtime::{Core, HomeView, TitleView, VersionsParams, VersionsView};
use mq_streams::{DeviceCaps, StreamPrefs, Target};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Request {
    Subscribe {
        id: u64,
        view: String,
        #[serde(default)]
        params: Value,
    },
    Unsubscribe {
        id: u64,
        sub: u64,
    },
    Command {
        id: u64,
        sub: u64,
        command: Value,
    },
    InstallAddon {
        id: u64,
        url: String,
    },
}

/// Holding a view keeps its background work alive; dropping it cancels.
enum View {
    Home { _view: HomeView },
    Title(TitleView),
    Versions(VersionsView),
}

struct Subscription {
    view: View,
    forward: JoinHandle<()>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.forward.abort();
    }
}

/// One client connection. Dropping it cancels every view it opened.
pub struct Session {
    core: Core,
    out: mpsc::UnboundedSender<Value>,
    subs: HashMap<u64, Subscription>,
    next_sub: u64,
}

impl Session {
    pub fn new(core: Core, out: mpsc::UnboundedSender<Value>) -> Self {
        Self {
            core,
            out,
            subs: HashMap::new(),
            next_sub: 1,
        }
    }

    pub async fn handle_text(&mut self, text: &str) {
        let request: Request = match serde_json::from_str(text) {
            Ok(r) => r,
            Err(e) => {
                let id = serde_json::from_str::<Value>(text)
                    .ok()
                    .and_then(|v| v.get("id").cloned());
                self.reply(id.unwrap_or(Value::Null), Err(format!("bad request: {e}")));
                return;
            }
        };
        match request {
            Request::Subscribe { id, view, params } => {
                let result = self.subscribe(&view, &params);
                self.reply(json!(id), result);
            }
            Request::Unsubscribe { id, sub } => {
                let found = self.subs.remove(&sub).is_some();
                self.reply(
                    json!(id),
                    if found {
                        Ok(json!({}))
                    } else {
                        Err(format!("no subscription {sub}"))
                    },
                );
            }
            Request::Command { id, sub, command } => {
                let result = self.command(sub, &command);
                self.reply(json!(id), result);
            }
            Request::InstallAddon { id, url } => {
                let result = self
                    .core
                    .install_addon(&url)
                    .await
                    .map(|a| json!({ "id": a.id(), "name": a.manifest.name }))
                    .map_err(|e| e.to_string());
                self.reply(json!(id), result);
            }
        }
    }

    fn reply(&self, id: Value, result: Result<Value, String>) {
        let message = match result {
            Ok(value) => json!({ "id": id, "ok": true, "result": value }),
            Err(error) => json!({ "id": id, "ok": false, "error": error }),
        };
        let _ = self.out.send(message);
    }

    fn subscribe(&mut self, view: &str, params: &Value) -> Result<Value, String> {
        let sub = self.next_sub;
        self.next_sub += 1;
        let out = self.out.clone();
        let (view, snapshot, forward) = match view {
            "home" => {
                let home = HomeView::open(&self.core);
                let (snapshot, updates) = home.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Home { _view: home }, to_value(&snapshot)?, forward)
            }
            "title" => {
                let kind = param(params, "kind")?;
                let id = param(params, "id")?;
                let title = TitleView::open(&self.core, &kind, &id);
                let (snapshot, updates) = title.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Title(title), to_value(&snapshot)?, forward)
            }
            "versions" => {
                let versions = VersionsView::open(&self.core, versions_params(params)?);
                let (snapshot, updates) = versions.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Versions(versions), to_value(&snapshot)?, forward)
            }
            other => return Err(format!("unknown view: {other}")),
        };
        self.subs.insert(sub, Subscription { view, forward });
        Ok(json!({ "sub": sub, "snapshot": snapshot }))
    }

    fn command(&mut self, sub: u64, command: &Value) -> Result<Value, String> {
        let subscription = self
            .subs
            .get(&sub)
            .ok_or_else(|| format!("no subscription {sub}"))?;
        let kind = command
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match (&subscription.view, kind) {
            (View::Title(title), "selectSeason") => {
                let season = command
                    .get("season")
                    .and_then(Value::as_u64)
                    .ok_or("season missing")?;
                title.select_season(season as u32);
            }
            (View::Title(title), "setInLibrary") => {
                let value = command
                    .get("value")
                    .and_then(Value::as_bool)
                    .ok_or("value missing")?;
                title.set_in_library(value).map_err(|e| e.to_string())?;
            }
            (View::Title(title), "setWatched") => {
                let video = param(command, "videoId")?;
                let value = command
                    .get("value")
                    .and_then(Value::as_bool)
                    .ok_or("value missing")?;
                title
                    .set_watched(&video, value)
                    .map_err(|e| e.to_string())?;
            }
            (View::Versions(versions), "freeze") => versions.freeze(),
            (_, other) => return Err(format!("unknown command: {other}")),
        }
        Ok(json!({}))
    }
}

fn param(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("missing parameter: {key}"))
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

async fn forward<S, U>(sub: u64, mut updates: S, out: mpsc::UnboundedSender<Value>)
where
    S: futures_util::Stream<Item = Vec<U>> + Unpin,
    U: serde::Serialize,
{
    while let Some(batch) = updates.next().await {
        let Ok(updates) = serde_json::to_value(&batch) else {
            continue;
        };
        if out.send(json!({ "sub": sub, "updates": updates })).is_err() {
            return;
        }
    }
}

/// Versions params: `kind`, `videoId`, `title` (for wrong-content checks),
/// optional `season`, `episode`, `displayHeight`, `hdr` (array of
/// "dolby_vision", "hdr10", "hdr10_plus", "hlg"), `bingeGroup`.
fn versions_params(params: &Value) -> Result<VersionsParams, String> {
    let num = |key: &str| params.get(key).and_then(Value::as_u64).map(|n| n as u32);
    let mut caps = DeviceCaps::basic_1080p();
    if let Some(height) = num("displayHeight") {
        caps.display_height = height;
    }
    if let Some(hdr) = params.get("hdr") {
        caps.display_hdr =
            serde_json::from_value(hdr.clone()).map_err(|e| format!("bad hdr: {e}"))?;
    }
    Ok(VersionsParams {
        kind: param(params, "kind")?,
        video_id: param(params, "videoId")?,
        target: Target {
            title: param(params, "title").unwrap_or_default(),
            year: None,
            season: num("season"),
            episode: num("episode"),
        },
        caps,
        prefs: StreamPrefs::default(),
        last_binge_group: params
            .get("bingeGroup")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}
