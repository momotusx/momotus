//! JSON protocol between the core and the web UI. The same session runs over
//! a WebSocket (dev server, browser, Playwright) and over the desktop shells'
//! IPC channel.
//!
//! Requests: `{"id", "op": "subscribe" | "unsubscribe" | "command" | "installAddon" |
//! "removeAddon" | "moveAddon" | "setAddonEnabled" | "setPrefs" | "setDebridKeys" |
//! "createSyncAccount" | "syncNow" | "approvePairing" | "createRecoveryKit" | "recover" |
//! "syncDevices" | "lostPreview" | "removeDevice" | "reportLost" | "signOut" |
//! "useServerRoster" | "createProfile" | "updateProfile" | "deleteProfile" | "setPin" |
//! "switchProfile" | "unlockProfile" | "lockSession",
//! ...}`.
//! Views: home, search, discover, library, title, versions, playback, addons, import,
//! settings, pairing, profiles.
//! Replies carry the request `id`; view pushes carry `sub` and `updates`.
//!
//! A session runs as one profile (plans/m11-profiles.md section 5): it opens
//! as the device's active profile, locked when that profile has a PIN. A
//! switch or a lock moves the session to a new generation: replies to
//! profile work started before it carry code `switched`, and pushes of the
//! old generation are dropped.

use futures_util::StreamExt;
use mq_library::Table;
use mq_runtime::{
    AddonsView, CalendarView, Core, DiscoverView, HomeView, ImportView, LibrarySort, LibraryView,
    PairingView, PlayTarget, PlaybackView, ProfileError, ProfilesView, SearchView, SessionState,
    SettingsView, StoreId, StremioEndpoints, TitleView, VersionsParams, VersionsView,
};
use mq_streams::{DeviceCaps, StreamPrefs, Target};
use mq_sync::SyncConfig;
use mq_sync::roster::Id;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, mpsc, watch};
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
    /// The owner adds a profile (no PIN, the owner's addons copied once).
    /// Profile ops reply `ProfilesSnapshot`.
    CreateProfile {
        id: u64,
        name: String,
        #[serde(default)]
        avatar: String,
    },
    /// Renames, re-avatars or moves `profile` (itself, or any as the owner).
    UpdateProfile {
        id: u64,
        profile: String,
        name: Option<String>,
        avatar: Option<String>,
        position: Option<i64>,
    },
    /// The owner removes `profile` for good, on every device.
    DeleteProfile {
        id: u64,
        profile: String,
    },
    /// Sets `pin` (or clears it with null) on `profile`.
    SetPin {
        id: u64,
        profile: String,
        pin: Option<String>,
        #[serde(rename = "currentPin")]
        current_pin: Option<String>,
    },
    /// Moves the session to `profile`; only the picker may be open.
    SwitchProfile {
        id: u64,
        profile: String,
        pin: Option<String>,
    },
    /// Unlocks a locked session with its profile's PIN.
    UnlockProfile {
        id: u64,
        pin: Option<String>,
    },
    /// Locks the session: every view but the picker closes.
    LockSession {
        id: u64,
    },
}

impl Request {
    /// What a locked session still answers (section 9, N5): the picker,
    /// unlocking and switching, sync (it applies records and shows nothing),
    /// liveness and closing views.
    fn allowed_while_locked(&self) -> bool {
        match self {
            Request::Subscribe { view, .. } => view == "profiles",
            Request::Unsubscribe { .. }
            | Request::UnlockProfile { .. }
            | Request::SwitchProfile { .. }
            | Request::SyncNow { .. }
            | Request::Ping { .. }
            | Request::Lifecycle { .. } => true,
            _ => false,
        }
    }
}

/// Holding a view keeps its background work alive; dropping it cancels.
enum View {
    Home { _view: HomeView },
    Title(TitleView),
    Versions(VersionsView),
    Playback(PlaybackView),
    Search(SearchView),
    Library(LibraryView),
    Calendar(CalendarView),
    Discover(DiscoverView),
    Pairing(PairingView),
    Settings { _view: SettingsView },
    Addons { _view: AddonsView },
    Import { _view: ImportView },
    Profiles { _view: ProfilesView },
}

impl View {
    fn name(&self) -> &'static str {
        match self {
            View::Home { .. } => "home",
            View::Title(_) => "title",
            View::Versions(_) => "versions",
            View::Playback(_) => "playback",
            View::Search(_) => "search",
            View::Library(_) => "library",
            View::Calendar(_) => "calendar",
            View::Discover(_) => "discover",
            View::Pairing(_) => "pairing",
            View::Settings { .. } => "settings",
            View::Addons { .. } => "addons",
            View::Import { .. } => "import",
            View::Profiles { .. } => "profiles",
        }
    }
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

/// What the request loop shares with the session's background work, under
/// one lock: pushes and replies check `generation` under it, so nothing
/// started before a switch or lock reaches the app after it.
struct Shared {
    core: Core,
    locked: bool,
    generation: u64,
    subs: HashMap<u64, Subscription>,
    /// (active profile, locked) for the picker view.
    state: watch::Sender<SessionState>,
}

impl Shared {
    /// A switch or a lock: every view but the picker closes (playback saves
    /// its position as it closes) and older work is fenced off.
    fn transition(&mut self, core: Core, locked: bool) {
        self.generation += 1;
        self.core = core;
        self.subs
            .retain(|_, s| matches!(s.view, View::Profiles { .. }));
        self.set_locked(locked);
    }

    fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
        self.state
            .send_replace((self.core.profile().to_owned(), locked));
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

/// One client connection. Dropping it cancels every view it opened.
pub struct Session {
    shared: Arc<Mutex<Shared>>,
    out: mpsc::UnboundedSender<Value>,
    next_sub: u64,
    watcher: JoinHandle<()>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}

impl Session {
    /// Opens as the device's active profile (a stale one falls back to the
    /// owner), locked when that profile has a PIN: a cold start or a
    /// reconnect never resumes a protected profile.
    pub fn new(core: Core, out: mpsc::UnboundedSender<Value>) -> Self {
        let core = core.active();
        let locked = core.has_pin();
        // Subscribe before reading what is gone, so no removal slips by.
        let changes = core.library().subscribe();
        let gone = core.library().gone_profiles().unwrap_or_default();
        let (state, _) = watch::channel((core.profile().to_owned(), locked));
        let shared = Arc::new(Mutex::new(Shared {
            core: core.clone(),
            locked,
            generation: 0,
            subs: HashMap::new(),
            state,
        }));
        let watcher = tokio::spawn(watch_gone(
            Arc::downgrade(&shared),
            core,
            changes,
            gone.into_iter().collect(),
            out.clone(),
        ));
        Self {
            shared,
            out,
            next_sub: 1,
            watcher,
        }
    }

    fn core(&self) -> Core {
        lock(&self.shared).core.clone()
    }

    pub async fn handle_text(&mut self, text: &str) {
        let raw: Value = serde_json::from_str(text).unwrap_or_default();
        let id = raw.get("id").cloned().unwrap_or(Value::Null);
        let request: Request = match serde_json::from_str(text) {
            Ok(r) => r,
            Err(e) => {
                self.reply(id, Err(format!("bad request: {e}")));
                return;
            }
        };
        if lock(&self.shared).locked && !request.allowed_while_locked() {
            let refused = Failure::coded("locked", "the profile is locked: unlock it first");
            let _ = self.out.send(message(id, Err(refused)));
            return;
        }
        let core = self.core();
        match request {
            Request::Subscribe { id, view, params } => {
                let result = self.subscribe(&view, &params);
                self.reply(json!(id), result);
            }
            Request::Unsubscribe { id, sub } => {
                let removed = lock(&self.shared).subs.remove(&sub);
                let found = removed.is_some();
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
                let result = core.remove_addon(&url);
                self.reply(json!(id), done(result));
            }
            Request::MoveAddon { id, url, to } => {
                let result = core.move_addon(&url, to);
                self.reply(json!(id), done(result));
            }
            Request::SetAddonEnabled { id, url, enabled } => {
                let result = core.set_addon_enabled(&url, enabled);
                self.reply(json!(id), done(result));
            }
            Request::CreateSyncAccount {
                id,
                server,
                invite,
                name,
            } => {
                self.spawn(id, async move {
                    let http = core.http();
                    answer(mq_sync::create_account(http, &server, invite.as_deref(), &name).await)
                });
            }
            Request::SyncNow { id, record } => {
                self.spawn(id, async move { answer(core.sync(&record).await) });
            }
            Request::ApprovePairing { id, record, qr } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::pair::approve_pairing(library, http, &record, clock, &qr)
                        .await
                        .map(|(code, record)| json!({ "code": code, "record": record }))
                });
            }
            Request::CreateRecoveryKit { id, record } => {
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
                self.spawn(id, async move {
                    answer(mq_sync::pair::recover(core.http(), &server, &kit, &name).await)
                });
            }
            Request::SyncDevices { id, record } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::devices(library, http, &record, clock).await)
                });
            }
            Request::LostPreview { id, record, lost } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::lost_preview(library, http, &record, clock, lost).await)
                });
            }
            Request::RemoveDevice { id, record, device } => {
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
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::report_lost(library, http, &record, clock, lost, keep).await)
                });
            }
            Request::SignOut { id, record, force } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::sign_out(library, http, &record, clock, force)
                        .await
                        .map(|()| json!({}))
                });
            }
            Request::UseServerRoster { id, record, drop } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::use_server_roster(library, http, &record, clock, drop).await)
                });
            }
            Request::Ping { id } => self.reply(json!(id), Ok(json!({}))),
            Request::HomeCatalogs { id } => {
                let catalogs = core.home_catalogs();
                self.reply(json!(id), Ok(json!(catalogs)))
            }
            Request::SetHomeRow { id, row_id, hidden } => {
                let result = core
                    .set_home_row_hidden(&row_id, hidden)
                    .map(|list| json!(list))
                    .map_err(|e| e.to_string());
                self.reply(json!(id), result)
            }
            Request::MoveHomeRow { id, row_id, by } => {
                let result = core
                    .move_home_row(&row_id, by)
                    .map(|list| json!(list))
                    .map_err(|e| e.to_string());
                self.reply(json!(id), result)
            }
            Request::Lifecycle { id, event } => {
                let result = match event.as_str() {
                    "memoryPressure" => {
                        core.on_memory_pressure();
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
                let result = core
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
                        core.set_debrid_keys(&keys);
                        json!({ "stores": keys.len() })
                    });
                self.reply(json!(id), result);
            }
            Request::InstallAddon { id, url } => {
                // Bound to the profile: a switch first makes it `switched`.
                self.spawn_fenced(id, async move {
                    core.install_addon(&url)
                        .await
                        .map(|a| json!({ "id": a.id(), "name": a.manifest.name }))
                        .map_err(|e| e.to_string())
                });
            }
            Request::CreateProfile { id, name, avatar } => {
                let result = core.create_profile(&name, &avatar).map(drop);
                self.reply_profiles(id, result);
            }
            Request::UpdateProfile {
                id,
                profile,
                name,
                avatar,
                position,
            } => {
                let result =
                    core.update_profile(&profile, name.as_deref(), avatar.as_deref(), position);
                self.reply_profiles(id, result);
            }
            Request::DeleteProfile { id, profile } => {
                let result = core.delete_profile(&profile);
                self.reply_profiles(id, result);
            }
            Request::SetPin {
                id,
                profile,
                pin,
                current_pin,
            } => {
                let result = core
                    .set_pin(&profile, pin.as_deref(), current_pin.as_deref())
                    .await;
                self.reply_profiles(id, result);
            }
            Request::SwitchProfile { id, profile, pin } => {
                let result = self.switch(&profile, pin.as_deref()).await;
                let _ = self.out.send(message(json!(id), result));
            }
            Request::UnlockProfile { id, pin } => {
                let result = self.unlock(pin.as_deref()).await;
                self.reply_profiles(id, result);
            }
            Request::LockSession { id } => {
                let mut shared = lock(&self.shared);
                shared.transition(core, true);
                drop(shared);
                self.reply_profiles(id, Ok(()));
            }
        }
    }

    fn reply(&self, id: Value, result: Result<Value, String>) {
        let _ = self.out.send(message(id, result.map_err(Failure::from)));
    }

    /// The picker's snapshot after a profile op, or the op's error.
    fn reply_profiles(&self, id: u64, result: Result<(), ProfileError>) {
        let result = result.map_err(Failure::from).map(|()| {
            let shared = lock(&self.shared);
            json!(shared.core.profiles_snapshot(shared.locked))
        });
        let _ = self.out.send(message(json!(id), result));
    }

    /// Moves the session to `profile` (its PIN when it has one). Refused
    /// while any view but the picker is open: the app closes them first, so
    /// playback has saved its position.
    async fn switch(&mut self, profile: &str, pin: Option<&str>) -> Result<Value, Failure> {
        let open: Vec<&str> = lock(&self.shared)
            .subs
            .values()
            .map(|s| s.view.name())
            .filter(|name| *name != "profiles")
            .collect();
        if !open.is_empty() {
            return Err(Failure::coded("viewsOpen", "close the open views first")
                .with(json!({ "views": open })));
        }
        let target = self.core().for_profile(profile);
        target.check_pin(profile, pin).await?;
        target.make_active()?;
        let mut shared = lock(&self.shared);
        shared.transition(target, false);
        Ok(json!(shared.core.profiles_snapshot(false)))
    }

    /// Unlocks with the session profile's PIN (none needed without one). A
    /// profile removed meanwhile stays locked: switch to another.
    async fn unlock(&mut self, pin: Option<&str>) -> Result<(), ProfileError> {
        let core = self.core();
        if !lock(&self.shared).locked {
            return Ok(());
        }
        core.check_pin(core.profile(), pin).await?;
        let mut shared = lock(&self.shared);
        if core.library().is_gone(core.profile())? {
            return Err(ProfileError::Unknown);
        }
        if shared.core.profile() == core.profile() {
            shared.set_locked(false);
        }
        Ok(())
    }

    /// Runs a request that waits on the network off the request loop, so a
    /// sync or an addon fetch never holds up playback commands. Replies
    /// carry the request id, so they may arrive in any order. For work of
    /// the device (sync, pairing), whose answer holds for any profile.
    fn spawn<E: Into<Failure> + Send + 'static>(
        &self,
        id: u64,
        work: impl Future<Output = Result<Value, E>> + Send + 'static,
    ) {
        let out = self.out.clone();
        tokio::spawn(async move {
            let result = run(work).await;
            let _ = out.send(message(json!(id), result));
        });
    }

    /// `spawn` for work of the session's profile: when the session switched
    /// or locked before it finished, the reply is code `switched` instead,
    /// so the app never applies one profile's answer to another.
    fn spawn_fenced<E: Into<Failure> + Send + 'static>(
        &self,
        id: u64,
        work: impl Future<Output = Result<Value, E>> + Send + 'static,
    ) {
        let out = self.out.clone();
        let shared = Arc::downgrade(&self.shared);
        let generation = lock(&self.shared).generation;
        tokio::spawn(async move {
            let result = run(work).await;
            let Some(shared) = shared.upgrade() else {
                return;
            };
            let shared = lock(&shared);
            let result = if shared.generation == generation {
                result
            } else {
                Err(Failure::coded(
                    "switched",
                    "the profile changed before this finished",
                ))
            };
            let _ = out.send(message(json!(id), result));
        });
    }

    fn subscribe(&mut self, view: &str, params: &Value) -> Result<Value, String> {
        let sub = self.next_sub;
        self.next_sub += 1;
        let core = self.core();
        let out = self.out.clone();
        // Pushes of profile views stop at the next switch or lock; the
        // picker's carry on.
        let fence = Some((Arc::downgrade(&self.shared), lock(&self.shared).generation));
        let (view, snapshot, forward) = match view {
            "profiles" => {
                let state = lock(&self.shared).state.subscribe();
                let profiles = ProfilesView::open(&core, state);
                let (snapshot, updates) = profiles.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, None));
                (
                    View::Profiles { _view: profiles },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "home" => {
                let home = HomeView::open(&core);
                let (snapshot, updates) = home.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Home { _view: home }, to_value(&snapshot)?, forward)
            }
            "title" => {
                let kind = param(params, "kind")?;
                let id = param(params, "id")?;
                let title = TitleView::open(&core, &kind, &id);
                let (snapshot, updates) = title.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Title(title), to_value(&snapshot)?, forward)
            }
            "versions" => {
                let versions =
                    VersionsView::open(&core, versions_params(params, core.prefs().streams)?);
                let (snapshot, updates) = versions.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Versions(versions), to_value(&snapshot)?, forward)
            }
            "addons" => {
                let addons = AddonsView::open(&core);
                let (snapshot, updates) = addons.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (
                    View::Addons { _view: addons },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "import" => {
                let import = ImportView::open(&core, StremioEndpoints::default());
                let (snapshot, updates) = import.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (
                    View::Import { _view: import },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "settings" => {
                let settings = SettingsView::open(&core);
                let (snapshot, updates) = settings.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (
                    View::Settings { _view: settings },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "pairing" => {
                let name = param(params, "name").unwrap_or_else(|_| "New device".into());
                let pairing = PairingView::open(&core, &param(params, "server")?, &name);
                let (snapshot, updates) = pairing.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Pairing(pairing), to_value(&snapshot)?, forward)
            }
            "discover" => {
                let discover = DiscoverView::open(&core);
                let (snapshot, updates) = discover.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Discover(discover), to_value(&snapshot)?, forward)
            }
            "calendar" => {
                let today = params
                    .get("today")
                    .and_then(Value::as_str)
                    .and_then(mq_runtime::views::calendar::parse_day)
                    .ok_or("today must be YYYY-MM-DD")?;
                let calendar = CalendarView::open(&core, today);
                let (snapshot, updates) = calendar.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Calendar(calendar), to_value(&snapshot)?, forward)
            }
            "library" => {
                let library = LibraryView::open(&core);
                let (snapshot, updates) = library.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Library(library), to_value(&snapshot)?, forward)
            }
            "search" => {
                let search = SearchView::open(&core);
                let (snapshot, updates) = search.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Search(search), to_value(&snapshot)?, forward)
            }
            "playback" => {
                let playback = self.playback(&core, params)?;
                let (snapshot, updates) = playback.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Playback(playback), to_value(&snapshot)?, forward)
            }
            other => return Err(format!("unknown view: {other}")),
        };
        lock(&self.shared)
            .subs
            .insert(sub, Subscription { view, forward });
        Ok(json!({ "sub": sub, "snapshot": snapshot }))
    }

    fn command(&mut self, sub: u64, command: &Value) -> Result<Value, String> {
        let shared = lock(&self.shared);
        let subscription = shared
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
            (View::Calendar(calendar), "setToday") => {
                let today = command
                    .get("today")
                    .and_then(Value::as_str)
                    .and_then(mq_runtime::views::calendar::parse_day)
                    .ok_or("today must be YYYY-MM-DD")?;
                calendar.set_today(today);
            }
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
    fn playback(&self, core: &Core, params: &Value) -> Result<PlaybackView, String> {
        let versions_sub = params
            .get("versionsSub")
            .and_then(Value::as_u64)
            .ok_or("missing parameter: versionsSub")?;
        let shared = lock(&self.shared);
        let Some(Subscription {
            view: View::Versions(versions),
            ..
        }) = shared.subs.get(&versions_sub)
        else {
            return Err(format!("no versions subscription {versions_sub}"));
        };
        let candidates = versions.candidates(&param(params, "key")?);
        drop(shared);
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
        Ok(PlaybackView::open(core, candidates, target))
    }
}

/// An optional typed param: absent is `None`, a wrong shape is an error.
fn field<T: serde::de::DeserializeOwned>(params: &Value, key: &str) -> Result<Option<T>, String> {
    params
        .get(key)
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("bad {key}: {e}")))
        .transpose()
}

/// A failed request: the message to show and, for the errors an app acts
/// on (a dropped device, a fork, a wrong PIN), a stable `code`, with any
/// fields that go with it (`retryAfterMs`, `views`).
struct Failure {
    error: String,
    code: Option<&'static str>,
    extra: Value,
}

impl Failure {
    fn coded(code: &'static str, error: &str) -> Self {
        Self {
            error: error.to_owned(),
            code: Some(code),
            extra: Value::Null,
        }
    }

    fn with(self, extra: Value) -> Self {
        Self { extra, ..self }
    }
}

impl From<String> for Failure {
    fn from(error: String) -> Self {
        Self {
            error,
            code: None,
            extra: Value::Null,
        }
    }
}

impl From<mq_sync::SyncError> for Failure {
    fn from(e: mq_sync::SyncError) -> Self {
        Self {
            code: e.code(),
            error: e.to_string(),
            extra: Value::Null,
        }
    }
}

impl From<ProfileError> for Failure {
    fn from(e: ProfileError) -> Self {
        let extra = match e {
            ProfileError::Throttled { retry_after_ms } => json!({ "retryAfterMs": retry_after_ms }),
            _ => Value::Null,
        };
        Self {
            code: e.code(),
            error: e.to_string(),
            extra,
        }
    }
}

fn message(id: Value, result: Result<Value, Failure>) -> Value {
    match result {
        Ok(value) => json!({ "id": id, "ok": true, "result": value }),
        Err(Failure { error, code, extra }) => {
            let mut reply = json!({ "id": id, "ok": false, "error": error });
            if let Some(code) = code {
                reply["code"] = json!(code);
            }
            if let Value::Object(fields) = extra {
                reply.as_object_mut().expect("an object").extend(fields);
            }
            reply
        }
    }
}

/// Runs request work as its own task: one that panics still gets an
/// answer, so the app never waits for it forever.
async fn run<E: Into<Failure> + Send + 'static>(
    work: impl Future<Output = Result<Value, E>> + Send + 'static,
) -> Result<Value, Failure> {
    match tokio::spawn(work).await {
        Ok(result) => result.map_err(Into::into),
        Err(e) => Err(Failure::from(format!("internal error: {e}"))),
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

/// The session generation a view was opened in; `None` for the picker,
/// which outlives switches and locks.
type Fence = Option<(Weak<Mutex<Shared>>, u64)>;

async fn forward<S, U>(sub: u64, mut updates: S, out: mpsc::UnboundedSender<Value>, fence: Fence)
where
    S: futures_util::Stream<Item = Vec<U>> + Unpin,
    U: serde::Serialize,
{
    while let Some(batch) = updates.next().await {
        let Ok(updates) = serde_json::to_value(&batch) else {
            continue;
        };
        let push = json!({ "sub": sub, "updates": updates });
        let sent = match &fence {
            None => out.send(push).is_ok(),
            // Checked and sent under the session lock: once a switch or a
            // lock has bumped the generation, nothing older goes out.
            Some((shared, generation)) => shared.upgrade().is_some_and(|shared| {
                let shared = lock(&shared);
                shared.generation == *generation && out.send(push).is_ok()
            }),
        };
        if !sent {
            return;
        }
    }
}

/// Profiles removed while the session runs, here or by sync: the app is
/// told with `{"profile": "gone", "id"}` (to drop that profile's keystore
/// entries), and a session running as one is locked, its views closed.
async fn watch_gone(
    shared: Weak<Mutex<Shared>>,
    core: Core,
    mut changes: broadcast::Receiver<Vec<Table>>,
    mut known: HashSet<String>,
    out: mpsc::UnboundedSender<Value>,
) {
    loop {
        match changes.recv().await {
            Ok(tables) if !tables.contains(&Table::Profiles) => continue,
            Ok(_) | Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return,
        }
        let Ok(gone) = core.library().gone_profiles() else {
            continue;
        };
        let Some(shared) = shared.upgrade() else {
            return;
        };
        let mut shared = lock(&shared);
        for id in gone.iter().filter(|id| !known.contains(*id)) {
            if shared.core.profile() == id {
                let core = shared.core.clone();
                shared.transition(core, true);
            }
            let _ = out.send(json!({ "profile": "gone", "id": id }));
        }
        known = gone.into_iter().collect();
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

#[cfg(test)]
mod fence {
    use super::*;

    /// A view task still running when a lock or switch moves the generation
    /// (the race the session lock closes) sends nothing older.
    #[tokio::test]
    async fn pushes_of_an_older_generation_never_go_out() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let session = Session::new(Core::temporary("p").unwrap(), tx);
        let generation = lock(&session.shared).generation;
        let (feed, mut updates) = mpsc::unbounded_channel::<Vec<u32>>();
        let stream = futures_util::stream::poll_fn(move |cx| updates.poll_recv(cx));
        let fence = Some((Arc::downgrade(&session.shared), generation));
        let task = tokio::spawn(forward(7, stream, session.out.clone(), fence));
        feed.send(vec![1]).unwrap();
        assert_eq!(rx.recv().await.unwrap(), json!({"sub": 7, "updates": [1]}));

        lock(&session.shared).generation += 1;
        feed.send(vec![2]).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("the forwarder stops")
            .unwrap();
        assert!(rx.try_recv().is_err(), "an older push went out");
    }
}
