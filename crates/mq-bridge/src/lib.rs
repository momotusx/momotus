//! JSON protocol between the core and the web UI. The same session runs over
//! a WebSocket (dev server, browser, Playwright) and over the desktop shells'
//! IPC channel.
//!
//! Requests: `{"id", "op": "subscribe" | "unsubscribe" | "command" | "installAddon" |
//! "removeAddon" | "moveAddon" | "setAddonEnabled" | "setPrefs" | "setDebridKeys" |
//! "createSyncAccount" | "syncNow" | "approvePairing" | "createRecoveryKit" | "recover" |
//! "syncDevices" | "lostPreview" | "removeDevice" | "reportLost" | "signOut" |
//! "useServerRoster",
//! ...}`.
//! Views: home, search, discover, library, title, versions, playback, addons, import,
//! settings, pairing.
//! Replies carry the request `id`; view pushes carry `sub` and `updates`.

use futures_util::StreamExt;
use mq_runtime::{
    AddonsView, Core, DiscoverView, HomeView, ImportView, LibrarySort, LibraryView, PairingView,
    PlayTarget, PlaybackView, SearchView, SettingsView, StoreId, StremioEndpoints, TitleView,
    VersionsParams, VersionsView,
};
use mq_streams::{DeviceCaps, StreamPrefs, Target};
use mq_sync::SyncConfig;
use mq_sync::roster::Id;
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
    RemoveAddon {
        id: u64,
        url: String,
    },
    MoveAddon {
        id: u64,
        url: String,
        to: usize,
    },
    SetAddonEnabled {
        id: u64,
        url: String,
        enabled: bool,
    },
    /// Debrid accounts from the platform keystore, by service name
    /// ("realdebrid", "torbox", "alldebrid", "premiumize"). Replaces the set.
    SetDebridKeys {
        id: u64,
        keys: Vec<(String, String)>,
    },
    /// Creates a relay account with this device as its first member. The
    /// reply is the keystore record (`SyncConfig`).
    CreateSyncAccount {
        id: u64,
        server: String,
        #[serde(default)]
        invite: Option<String>,
        name: String,
    },
    /// One sync round. `record` is the keystore record; a reply carrying a
    /// new `record` must be stored, then sync again (`again`).
    SyncNow {
        id: u64,
        record: Box<SyncConfig>,
    },
    /// On a device that syncs: vouches for the device showing `qr`. Replies
    /// `{code, record}`.
    ApprovePairing {
        id: u64,
        record: Box<SyncConfig>,
        qr: String,
    },
    /// Makes a recovery kit and adds it to the roster. Replies `{kit,
    /// record}`; show the kit once.
    CreateRecoveryKit {
        id: u64,
        record: Box<SyncConfig>,
    },
    /// Joins the account a recovery kit belongs to as a new device; the
    /// reply is its keystore record.
    Recover {
        id: u64,
        server: String,
        kit: String,
        name: String,
    },
    /// The account's members, fingerprint and epoch.
    SyncDevices {
        id: u64,
        record: Box<SyncConfig>,
    },
    /// What reporting `lost` would drop: devices to untick, kits that go.
    LostPreview {
        id: u64,
        record: Box<SyncConfig>,
        lost: Id,
    },
    /// Removes one device (no cascade); replies the record to store.
    RemoveDevice {
        id: u64,
        record: Box<SyncConfig>,
        device: Id,
    },
    /// Drops `lost` and what it vouched for, except `keep`; replies the
    /// record to store.
    ReportLost {
        id: u64,
        record: Box<SyncConfig>,
        lost: Id,
        #[serde(default)]
        keep: Vec<Id>,
    },
    /// Sends what is queued, leaves the account and forgets sync state; the
    /// platform then deletes the keystore record.
    SignOut {
        id: u64,
        record: Box<SyncConfig>,
        #[serde(default)]
        force: bool,
    },
    /// After a fork: takes the relay's roster, dropping again what this
    /// device's roster had dropped plus `drop`; replies the record to store.
    UseServerRoster {
        id: u64,
        record: Box<SyncConfig>,
        #[serde(default)]
        drop: Vec<Id>,
    },
    /// Merges `patch` into the profile preferences (see `Core::set_prefs`).
    SetPrefs {
        id: u64,
        patch: Value,
    },
    /// Answers at once from the request loop: the core is alive.
    Ping {
        id: u64,
    },
    /// Every Home catalog in Home's order, hidden ones included, for the
    /// "Customize Home" screen.
    HomeCatalogs {
        id: u64,
    },
    /// Hides or shows one Home row; replies with the new list.
    SetHomeRow {
        id: u64,
        #[serde(rename = "rowId")]
        row_id: String,
        hidden: bool,
    },
    /// Moves one Home row `by` places (negative is up); replies with the new list.
    MoveHomeRow {
        id: u64,
        #[serde(rename = "rowId")]
        row_id: String,
        by: i64,
    },
    /// The app's life as the OS reports it: `foreground`, `background`,
    /// `memoryPressure` (frees caches) or `networkChanged`.
    Lifecycle {
        id: u64,
        event: String,
    },
}

/// Holding a view keeps its background work alive; dropping it cancels.
enum View {
    Home { _view: HomeView },
    Title(TitleView),
    Versions(VersionsView),
    Playback(PlaybackView),
    Search(SearchView),
    Library(LibraryView),
    Discover(DiscoverView),
    Pairing(PairingView),
    Settings { _view: SettingsView },
    Addons { _view: AddonsView },
    Import { _view: ImportView },
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
            Request::RemoveAddon { id, url } => {
                let result = self.core.remove_addon(&url);
                self.reply(json!(id), done(result));
            }
            Request::MoveAddon { id, url, to } => {
                let result = self.core.move_addon(&url, to);
                self.reply(json!(id), done(result));
            }
            Request::SetAddonEnabled { id, url, enabled } => {
                let result = self.core.set_addon_enabled(&url, enabled);
                self.reply(json!(id), done(result));
            }
            Request::CreateSyncAccount {
                id,
                server,
                invite,
                name,
            } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let http = core.http();
                    answer(mq_sync::create_account(http, &server, invite.as_deref(), &name).await)
                });
            }
            Request::SyncNow { id, record } => {
                let core = self.core.clone();
                self.spawn(id, async move { answer(core.sync(&record).await) });
            }
            Request::ApprovePairing { id, record, qr } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::pair::approve_pairing(library, http, &record, clock, &qr)
                        .await
                        .map(|(code, record)| json!({ "code": code, "record": record }))
                });
            }
            Request::CreateRecoveryKit { id, record } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::pair::make_kit(library, http, &record, clock)
                        .await
                        .map(|(kit, record)| json!({ "kit": kit, "record": record }))
                });
            }
            Request::Recover {
                id,
                server,
                kit,
                name,
            } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    answer(mq_sync::pair::recover(core.http(), &server, &kit, &name).await)
                });
            }
            Request::SyncDevices { id, record } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::devices(library, http, &record, clock).await)
                });
            }
            Request::LostPreview { id, record, lost } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::lost_preview(library, http, &record, clock, lost).await)
                });
            }
            Request::RemoveDevice { id, record, device } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    let devices = vec![device];
                    answer(mq_sync::remove_devices(library, http, &record, clock, devices).await)
                });
            }
            Request::ReportLost {
                id,
                record,
                lost,
                keep,
            } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::report_lost(library, http, &record, clock, lost, keep).await)
                });
            }
            Request::SignOut { id, record, force } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::sign_out(library, http, &record, clock, force)
                        .await
                        .map(|()| json!({}))
                });
            }
            Request::UseServerRoster { id, record, drop } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::use_server_roster(library, http, &record, clock, drop).await)
                });
            }
            Request::Ping { id } => self.reply(json!(id), Ok(json!({}))),
            Request::HomeCatalogs { id } => {
                let catalogs = self.core.home_catalogs();
                self.reply(json!(id), Ok(json!(catalogs)))
            }
            Request::SetHomeRow { id, row_id, hidden } => {
                let result = self
                    .core
                    .set_home_row_hidden(&row_id, hidden)
                    .map(|list| json!(list))
                    .map_err(|e| e.to_string());
                self.reply(json!(id), result)
            }
            Request::MoveHomeRow { id, row_id, by } => {
                let result = self
                    .core
                    .move_home_row(&row_id, by)
                    .map(|list| json!(list))
                    .map_err(|e| e.to_string());
                self.reply(json!(id), result)
            }
            Request::Lifecycle { id, event } => {
                let result = match event.as_str() {
                    "memoryPressure" => {
                        self.core.on_memory_pressure();
                        Ok(json!({}))
                    }
                    // ponytail: accepted and ignored until the core has work
                    // for them (iOS listener reopen, network re-probes).
                    "foreground" | "background" | "networkChanged" => Ok(json!({})),
                    other => Err(format!("unknown lifecycle event: {other}")),
                };
                self.reply(json!(id), result);
            }
            Request::SetPrefs { id, patch } => {
                let result = self
                    .core
                    .set_prefs(&patch)
                    .map_err(|e| e.to_string())
                    .and_then(|p| to_value(&p));
                self.reply(json!(id), result);
            }
            Request::SetDebridKeys { id, keys } => {
                let result = keys
                    .into_iter()
                    .map(|(service, key)| {
                        StoreId::from_service(&service)
                            .map(|id| (id, key))
                            .ok_or(format!("unknown debrid service: {service}"))
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(|keys| {
                        self.core.set_debrid_keys(&keys);
                        json!({ "stores": keys.len() })
                    });
                self.reply(json!(id), result);
            }
            Request::InstallAddon { id, url } => {
                let core = self.core.clone();
                self.spawn(id, async move {
                    core.install_addon(&url)
                        .await
                        .map(|a| json!({ "id": a.id(), "name": a.manifest.name }))
                        .map_err(|e| e.to_string())
                });
            }
        }
    }

    fn reply(&self, id: Value, result: Result<Value, String>) {
        let _ = self.out.send(message(id, result.map_err(Failure::from)));
    }

    /// Runs a request that waits on the network off the request loop, so a
    /// sync or an addon fetch never holds up playback commands. Replies
    /// carry the request id, so they may arrive in any order.
    fn spawn<E: Into<Failure> + Send + 'static>(
        &self,
        id: u64,
        work: impl Future<Output = Result<Value, E>> + Send + 'static,
    ) {
        let out = self.out.clone();
        tokio::spawn(async move {
            // A request that panics still gets an answer, so the app never
            // waits for it forever.
            let result = match tokio::spawn(work).await {
                Ok(result) => result.map_err(Into::into),
                Err(e) => Err(Failure::from(format!("internal error: {e}"))),
            };
            let _ = out.send(message(json!(id), result));
        });
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
                let versions = VersionsView::open(
                    &self.core,
                    versions_params(params, self.core.prefs().streams)?,
                );
                let (snapshot, updates) = versions.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Versions(versions), to_value(&snapshot)?, forward)
            }
            "addons" => {
                let addons = AddonsView::open(&self.core);
                let (snapshot, updates) = addons.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (
                    View::Addons { _view: addons },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "import" => {
                let import = ImportView::open(&self.core, StremioEndpoints::default());
                let (snapshot, updates) = import.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (
                    View::Import { _view: import },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "settings" => {
                let settings = SettingsView::open(&self.core);
                let (snapshot, updates) = settings.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (
                    View::Settings { _view: settings },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "pairing" => {
                let name = param(params, "name").unwrap_or_else(|_| "New device".into());
                let pairing = PairingView::open(&self.core, &param(params, "server")?, &name);
                let (snapshot, updates) = pairing.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Pairing(pairing), to_value(&snapshot)?, forward)
            }
            "discover" => {
                let discover = DiscoverView::open(&self.core);
                let (snapshot, updates) = discover.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Discover(discover), to_value(&snapshot)?, forward)
            }
            "library" => {
                let library = LibraryView::open(&self.core);
                let (snapshot, updates) = library.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Library(library), to_value(&snapshot)?, forward)
            }
            "search" => {
                let search = SearchView::open(&self.core);
                let (snapshot, updates) = search.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Search(search), to_value(&snapshot)?, forward)
            }
            "playback" => {
                let playback = self.playback(params)?;
                let (snapshot, updates) = playback.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out));
                (View::Playback(playback), to_value(&snapshot)?, forward)
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
            (View::Discover(discover), "select") => {
                let text = |key: &str| command.get(key).and_then(Value::as_str);
                // An absent genre keeps the current one; null clears it.
                let genre = command.get("genre").map(Value::as_str);
                discover.select(text("kind"), text("catalog"), genre);
            }
            (View::Discover(discover), "loadMore") => discover.load_more(),
            (View::Pairing(pairing), "confirm") => pairing.confirm(),
            (View::Pairing(pairing), "reject") => pairing.reject(),
            (View::Library(library), "setFilter") => {
                library.set_filter(
                    command
                        .get("kind")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                );
            }
            (View::Library(library), "setSort") => {
                let sort: LibrarySort =
                    serde_json::from_value(command.get("sort").cloned().unwrap_or_default())
                        .map_err(|e| format!("bad sort: {e}"))?;
                library.set_sort(sort);
            }
            (View::Search(search), "setQuery") => search.set_query(&param(command, "query")?),
            (View::Search(search), "remember") => search.remember().map_err(|e| e.to_string())?,
            (View::Playback(playback), "progress") => {
                let ms = |key: &str| {
                    command
                        .get(key)
                        .and_then(Value::as_u64)
                        .ok_or(format!("{key} missing"))
                };
                playback.report_progress(ms("positionMs")?, ms("durationMs")?);
            }
            (View::Playback(playback), "duration") => {
                let duration_ms = command
                    .get("durationMs")
                    .and_then(Value::as_u64)
                    .ok_or("durationMs missing")?;
                playback.report_duration(&param(command, "key")?, duration_ms);
            }
            (View::Playback(playback), "failOver") => {
                playback
                    .fail_over(&param(command, "error").unwrap_or_else(|_| "player error".into()));
            }
            (_, other) => return Err(format!("unknown command: {other}")),
        }
        Ok(json!({}))
    }
}

impl Session {
    /// Playback params: `versionsSub` (the open Versions view), `key` (its
    /// row), `kind`, `itemId`, `videoId`, optional `name`, `poster`, `season`,
    /// `episode`.
    fn playback(&self, params: &Value) -> Result<PlaybackView, String> {
        let versions_sub = params
            .get("versionsSub")
            .and_then(Value::as_u64)
            .ok_or("missing parameter: versionsSub")?;
        let Some(Subscription {
            view: View::Versions(versions),
            ..
        }) = self.subs.get(&versions_sub)
        else {
            return Err(format!("no versions subscription {versions_sub}"));
        };
        let candidates = versions.candidates(&param(params, "key")?);
        if candidates.is_empty() {
            return Err("that version is no longer listed".into());
        }
        let num = |key: &str| params.get(key).and_then(Value::as_u64).map(|n| n as u32);
        let target = PlayTarget {
            kind: param(params, "kind")?,
            item_id: param(params, "itemId")?,
            video_id: param(params, "videoId")?,
            name: param(params, "name").ok(),
            poster: param(params, "poster").ok(),
            season: num("season"),
            episode: num("episode"),
        };
        Ok(PlaybackView::open(&self.core, candidates, target))
    }
}

/// An optional typed param: absent is `None`, a wrong shape is an error.
fn field<T: serde::de::DeserializeOwned>(params: &Value, key: &str) -> Result<Option<T>, String> {
    params
        .get(key)
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad {key}: {e}")))
        .transpose()
}

/// A failed request: the message to show and, for the sync errors an app
/// acts on (a dropped device, a fork), a stable `code`.
struct Failure {
    error: String,
    code: Option<&'static str>,
}

impl From<String> for Failure {
    fn from(error: String) -> Self {
        Self { error, code: None }
    }
}

impl From<mq_sync::SyncError> for Failure {
    fn from(e: mq_sync::SyncError) -> Self {
        Self {
            code: e.code(),
            error: e.to_string(),
        }
    }
}

fn message(id: Value, result: Result<Value, Failure>) -> Value {
    match result {
        Ok(value) => json!({ "id": id, "ok": true, "result": value }),
        Err(Failure { error, code: None }) => json!({ "id": id, "ok": false, "error": error }),
        Err(Failure {
            error,
            code: Some(code),
        }) => json!({ "id": id, "ok": false, "error": error, "code": code }),
    }
}

fn done(result: Result<(), mq_runtime::CoreError>) -> Result<Value, String> {
    result.map(|()| json!({})).map_err(|e| e.to_string())
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

fn answer<T: serde::Serialize>(result: Result<T, mq_sync::SyncError>) -> Result<Value, Failure> {
    to_value(&result?).map_err(Failure::from)
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
/// optional `season`, `episode`, `bingeGroup`, and what the device can do:
/// `displayHeight`, `hdr` ("dolby_vision", "hdr10", "hdr10_plus", "hlg"),
/// `decoders` ("h264", "h265", "av1", "vp9", ...), `tenBitHevc`,
/// `passthrough` and `audioDecoders` ("aac", "ac3", "eac3", "dts",
/// "true_hd", "atmos", ...), `containers` ("mkv", "mp4", ...) and
/// `hasFallbackEngine`. What is left out keeps a 1080p SDR stick's default.
/// `language` (ISO 639-1, the device's) stands in for audio preferences the
/// user has not set: that language first, then English.
fn versions_params(params: &Value, mut prefs: StreamPrefs) -> Result<VersionsParams, String> {
    let num = |key: &str| params.get(key).and_then(Value::as_u64).map(|n| n as u32);
    if prefs.languages.is_empty() {
        let device: Option<String> = field(params, "language")?;
        prefs.languages = device.into_iter().chain(["en".to_owned()]).collect();
        prefs.languages.dedup();
    }
    let mut caps = DeviceCaps::basic_1080p();
    if let Some(height) = num("displayHeight") {
        caps.display_height = height;
    }
    if let Some(hdr) = field(params, "hdr")? {
        caps.display_hdr = hdr;
    }
    if let Some(decoders) = field(params, "decoders")? {
        caps.decoders = decoders;
    }
    if let Some(ten_bit) = field(params, "tenBitHevc")? {
        caps.ten_bit_hevc = ten_bit;
    }
    if let Some(passthrough) = field(params, "passthrough")? {
        caps.passthrough = passthrough;
    }
    if let Some(fallback) = field(params, "hasFallbackEngine")? {
        caps.has_fallback_engine = fallback;
    }
    if let Some(audio) = field(params, "audioDecoders")? {
        caps.audio_decoders = audio;
    }
    if let Some(containers) = field::<Vec<String>>(params, "containers")? {
        caps.containers = containers.iter().map(|c| c.to_ascii_lowercase()).collect();
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
        prefs,
        last_binge_group: params
            .get("bingeGroup")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_params_carry_what_the_device_can_do() {
        let params = json!({
            "kind": "movie", "videoId": "tt1", "displayHeight": 2160,
            "decoders": ["h264", "h265", "av1"], "tenBitHevc": false,
            "passthrough": ["eac3", "atmos"], "hasFallbackEngine": false,
            "audioDecoders": ["aac", "opus"], "containers": ["MP4", "webm"],
            "language": "tr"
        });
        let parsed = versions_params(&params, StreamPrefs::default()).unwrap();
        let caps = parsed.caps;
        assert_eq!(caps.display_height, 2160);
        assert_eq!(json!(caps.decoders), json!(["h264", "h265", "av1"]));
        assert!(!caps.ten_bit_hevc && !caps.has_fallback_engine);
        assert_eq!(json!(caps.passthrough), json!(["eac3", "atmos"]));
        assert_eq!(json!(caps.audio_decoders), json!(["aac", "opus"]));
        assert_eq!(caps.containers, ["mp4", "webm"]);
        assert_eq!(
            parsed.prefs.languages,
            ["tr", "en"],
            "device language, then English"
        );

        // Languages the user chose win over the device's.
        let chosen = StreamPrefs {
            languages: vec!["de".into()],
            ..StreamPrefs::default()
        };
        let parsed = versions_params(&params, chosen).unwrap();
        assert_eq!(parsed.prefs.languages, ["de"]);

        let defaults = json!({"kind": "movie", "videoId": "tt1"});
        let caps = versions_params(&defaults, StreamPrefs::default())
            .unwrap()
            .caps;
        assert_eq!(caps, DeviceCaps::basic_1080p());

        let wrong = json!({"kind": "movie", "videoId": "tt1", "decoders": ["betamax"]});
        let err = versions_params(&wrong, StreamPrefs::default())
            .err()
            .expect("refused");
        assert!(err.starts_with("bad decoders"), "{err}");
    }
}

#[cfg(test)]
mod liveness {
    use super::*;

    #[tokio::test]
    async fn ping_answers_and_a_panicking_request_still_gets_a_reply() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut session = Session::new(Core::temporary("p").unwrap(), tx);
        session
            .handle_text(&json!({"id": 1, "op": "ping"}).to_string())
            .await;
        assert_eq!(
            rx.recv().await.unwrap(),
            json!({"id": 1, "ok": true, "result": {}})
        );

        session.spawn(2, async {
            panic!("boom");
            #[allow(unreachable_code)]
            Ok::<Value, String>(json!({}))
        });
        let reply = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("a reply")
            .unwrap();
        assert_eq!(reply["id"], 2);
        assert_eq!(reply["ok"], false);
        assert!(
            reply["error"]
                .as_str()
                .unwrap()
                .starts_with("internal error"),
            "{reply}"
        );
    }
}

#[cfg(test)]
mod lifecycle {
    use super::*;

    #[tokio::test]
    async fn lifecycle_events_are_accepted_and_unknown_ones_refused() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut session = Session::new(Core::temporary("p").unwrap(), tx);
        for event in [
            "foreground",
            "background",
            "memoryPressure",
            "networkChanged",
        ] {
            session
                .handle_text(&json!({"id": 1, "op": "lifecycle", "event": event}).to_string())
                .await;
            assert_eq!(rx.recv().await.unwrap()["ok"], true, "{event}");
        }
        session
            .handle_text(&json!({"id": 2, "op": "lifecycle", "event": "sleep"}).to_string())
            .await;
        assert_eq!(rx.recv().await.unwrap()["ok"], false);
    }
}
