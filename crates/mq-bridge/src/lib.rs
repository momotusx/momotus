//! JSON protocol between the core and the web UI. The same session runs over
//! a WebSocket (dev server, browser, Playwright) and over the desktop shells'
//! IPC channel.
//!
//! Requests: `{"id", "op": "subscribe" | "unsubscribe" | "command" | "previewAddon" |
//! "installAddon" | "installListed" | "removeAddon" | "moveAddon" | "setAddonEnabled" | "defaultAddons" |
//! "installDefaults" | "setPrefs" | "streamConditions" | "resetSpeed" | "forgetLearned" |
//! "undoLearned" | "setDebridKeys" |
//! "restoreDebrid" | "ackDebrid" | "setDebridKey" | "disconnectDebrid" |
//! "createSyncAccount" | "syncNow" | "approvePairing" | "createRecoveryKit" | "recover" |
//! "syncDevices" | "lostPreview" | "removeDevice" | "reportLost" | "signOut" | "deleteAccount" |
//! "cancelPairing" |
//! "forkPreview" | "useServerRoster" | "createProfile" | "updateProfile" | "deleteProfile" |
//! "setPin" | "switchProfile" | "unlockProfile" | "lockSession" | "removeFromContinue" |
//! "languages",
//! ...}`.
//! Views: home, search, discover, library, title, versions, playback, addons, addonCatalog,
//! import, settings, pairing, profiles, debrid, debridLogin.
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
    AddonCatalogView, AddonsView, CalendarView, Core, DebridLoginView, DebridRecord,
    DebridTombstone, DebridView, DiscoverView, GenresView, HomeView, ImportView, LibrarySort,
    LibraryView, ListedError, ListedFailure, PairingView, PlayTarget, PlaybackView, ProfileError,
    ProfilesView, SearchView, SessionState, SettingsView, SignInSnapshot, SignInView, StoreId,
    TitleFacts, TitleView, TraktLoginView, TraktRecord, TraktView, VersionsParams, VersionsView,
};
use mq_streams::{DeviceCaps, StreamPrefs, Target};
use mq_sync::SyncConfig;
use mq_sync::roster::Digest32;
use mq_sync::roster::Id;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, watch};

mod dev;
pub mod output;
pub use dev::{dev_replay, dev_segments, dev_tmdb, dev_trakt};
pub use output::{Outgoing, Output, SendError};
use tokio::task::JoinHandle;

/// `signOut`'s removal (D13): whether the debrid connections go too.
#[derive(Deserialize)]
struct RemoveAfter {
    #[serde(default)]
    debrid: bool,
}

// No Debug: requests carry keys, PINs, recovery kits and sync records.
#[derive(Deserialize)]
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
    /// What a pasted link would install, for the install sheet; writes nothing.
    PreviewAddon {
        id: u64,
        url: String,
        /// The user saw the `tokenInLink` warning and goes on anyway.
        #[serde(default, rename = "acknowledgeToken")]
        acknowledge_token: bool,
    },
    InstallAddon {
        id: u64,
        url: String,
        /// The user saw the `tokenInLink` warning and installs it anyway.
        #[serde(default, rename = "acknowledgeToken")]
        acknowledge_token: bool,
    },
    /// The default addons and whether each is in the profile's list.
    DefaultAddons {
        id: u64,
    },
    /// Installs the defaults with the user's switches (a key left out is
    /// installed on when absent and left as it is when present).
    InstallDefaults {
        id: u64,
        #[serde(default)]
        choices: Vec<mq_runtime::defaults::DefaultChoice>,
    },
    /// An entry an open `addonCatalog` view shows, by its `key`.
    InstallListed {
        id: u64,
        key: String,
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
    /// At start, before any view: the keystore's debrid records and
    /// tombstones. Replies `{refused, tombstones}`.
    RestoreDebrid {
        id: u64,
        #[serde(default)]
        records: Vec<DebridRecord>,
        #[serde(default)]
        tombstones: Vec<DebridTombstone>,
    },
    /// The app stored this pending record.
    AckDebrid {
        id: u64,
        service: String,
        generation: String,
        revision: u64,
    },
    /// A typed or handed-off key: a new connection. Replies `{generation}`;
    /// the record arrives as pending in the `debrid` view.
    SetDebridKey {
        id: u64,
        service: String,
        key: String,
    },
    /// Sent after the app wrote the tombstone for this generation.
    DisconnectDebrid {
        id: u64,
        service: String,
        generation: String,
    },
    /// Creates a relay account with this device as its first member. The
    /// reply is the keystore record (`SyncConfig`).
    CreateSyncAccount {
        id: u64,
        server: String,
        #[serde(default)]
        invite: Option<String>,
        name: String,
        /// A sign-in's ticket: its identity is linked to the new account.
        #[serde(default)]
        ticket: Option<String>,
        /// Start over: the identity moves here, its old account is scheduled
        /// for deletion.
        #[serde(default, rename = "move")]
        move_link: bool,
        /// This device's form: `phone`, `tablet`, `tv`, `computer` or `web`.
        #[serde(default)]
        form: Option<String>,
    },
    /// The sign-in providers a relay offers: `{providers}` (empty: no
    /// sign-in there, only the pairing and recovery doors). Reads nothing
    /// of the account.
    RelayInfo {
        id: u64,
        server: String,
    },
    /// Opens a native sheet's flow: `{flow, nonce, expiresMs}`, the nonce
    /// for the sheet; the relay takes the flow until `expiresMs` (its time).
    NativeSignInStart {
        id: u64,
        server: String,
        provider: String,
        /// Apple's revoke (the answer then carries a receipt).
        #[serde(default)]
        revoke: bool,
    },
    /// The native sheet's ID token for `flow`: `found`, `new` or `receipt`.
    NativeSignIn {
        id: u64,
        flow: String,
        #[serde(rename = "idToken")]
        id_token: String,
        #[serde(default, rename = "authorizationCode")]
        authorization_code: Option<String>,
    },
    /// The account's identity links: `{links, record}`.
    Identities {
        id: u64,
        record: Box<SyncConfig>,
    },
    /// Links a sign-in's identity to this account: `{record}`.
    LinkIdentity {
        id: u64,
        record: Box<SyncConfig>,
        ticket: String,
        #[serde(default, rename = "move")]
        move_link: bool,
    },
    /// Unlinks by link id: `{record}`.
    UnlinkIdentity {
        id: u64,
        record: Box<SyncConfig>,
        link: i64,
        #[serde(default)]
        receipt: Option<String>,
    },
    /// Cancels a scheduled deletion; `unlink` also removes the identity that
    /// asked for it: `{record}`.
    CancelDeletion {
        id: u64,
        record: Box<SyncConfig>,
        #[serde(default)]
        unlink: bool,
    },
    /// One sync round. `record` is the keystore record; a reply carrying a
    /// new `record` must be stored, then sync again (`again`).
    SyncNow {
        id: u64,
        record: Box<SyncConfig>,
    },
    /// The user saw these announcements (their `id`s from `SyncReport`):
    /// no round reports them again. Replies `{}`.
    AckAnnouncements {
        id: u64,
        ids: Vec<String>,
    },
    /// On a device that syncs: vouches for the device showing `qr`. Replies
    /// `{code, record}`.
    ApprovePairing {
        id: u64,
        record: Box<SyncConfig>,
        qr: String,
    },
    /// Makes a recovery kit and adds it to the roster, on the owner profile
    /// only (D17). Replies `{kit, id, record}`; show the kit once.
    CreateRecoveryKit {
        id: u64,
        record: Box<SyncConfig>,
    },
    /// Saves a passkey as a way into the account: `prf` is its 32-byte PRF
    /// output (base64url), `label` the row's words; the owner only (D17).
    /// Replies `{id, record}`.
    SavePasskey {
        id: u64,
        record: Box<SyncConfig>,
        prf: String,
        label: String,
    },
    /// Joins the account a recovery kit or a passkey (`prf`, base64url)
    /// opens as a new device: `{record, via}` (`kit` or `passkey`).
    Recover {
        id: u64,
        server: String,
        #[serde(default)]
        kit: Option<String>,
        #[serde(default)]
        prf: Option<String>,
        name: String,
        #[serde(default)]
        form: Option<String>,
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
    /// platform then deletes the keystore record. Without a `record` (a
    /// device that left, or whose account was deleted) only `wipe` runs:
    /// the third step of a sign-out that removes the library (D13).
    SignOut {
        id: u64,
        #[serde(default)]
        record: Option<Box<SyncConfig>>,
        #[serde(default)]
        force: bool,
        /// On this device only: no round, nothing sent; for a device the relay
        /// will not let leave.
        #[serde(default)]
        local: bool,
        /// Without a record, on a device that holds no account: removes its
        /// profiles and library (D13), the owner profile only. The device is
        /// as new, and a profile-gone event names each profile it had.
        #[serde(default)]
        wipe: bool,
        /// With `wipe`: the debrid connections go too. Refused (code
        /// `debridConnected`) while one is left: the app disconnects each
        /// first, tombstone first.
        #[serde(default, rename = "disconnectDebrid")]
        disconnect_debrid: bool,
        /// Sign out with removal (D13), the owner profile only: recorded on
        /// this device before the leave goes out, so a restart, a crash or a
        /// closed app finds it in the profiles view (`wipePending`) and
        /// finishes steps two and three. A leave that fails clears it. Without
        /// a record, on a device that holds no account (its account was
        /// deleted), it only records the removal.
        #[serde(default, rename = "removeAfter")]
        remove_after: Option<RemoveAfter>,
    },
    /// Deletes the account on the sync server for every device; replies `{}`
    /// once the relay confirmed. This device forgets the account as after a
    /// sign-out (the library stays); the platform deletes the keystore record.
    /// The owner profile only (`notOwner` otherwise). `receipt`: an Apple
    /// revoke's, when the account has an Apple link.
    DeleteAccount {
        id: u64,
        record: Box<SyncConfig>,
        #[serde(default)]
        receipt: Option<String>,
    },
    /// The digits on the new device differ: ends the approval of `qr` now
    /// with a key rotation (dropping the device if it joined). Replies the
    /// record to store.
    CancelPairing {
        id: u64,
        record: Box<SyncConfig>,
        qr: String,
    },
    /// Both sides of a fork, read only: this device's members, the relay's,
    /// and what taking the relay's drops on its own.
    ForkPreview {
        id: u64,
        record: Box<SyncConfig>,
    },
    /// After a fork: takes the relay's roster, dropping again what this
    /// device's roster had dropped plus `drop`; replies the record to store.
    UseServerRoster {
        id: u64,
        record: Box<SyncConfig>,
        #[serde(default)]
        drop: Vec<Id>,
        /// The relay's head from `forkPreview`, required: only the roster the
        /// user saw is taken, and one that moved since is refused.
        head: Digest32,
    },
    /// Merges `patch` into the profile preferences (see `Core::set_prefs`).
    SetPrefs {
        id: u64,
        patch: Value,
    },
    /// This device's network, measured speeds and free space, for Settings
    /// ("This device"). Replies `StreamConditions`.
    StreamConditions {
        id: u64,
    },
    /// "Measure again": the current network's speeds go back to their
    /// estimates. Replies `StreamConditions`.
    ResetSpeed {
        id: u64,
    },
    /// "Forget what was learned" (plans/m13-stream-choice.md 3.9). Replies
    /// `Prefs`.
    ForgetLearned {
        id: u64,
    },
    /// Undo of a learning note: that choice on that screen class goes back
    /// to zero and no longer learns. Replies `Prefs`.
    UndoLearned {
        id: u64,
        #[serde(rename = "screenClass")]
        screen_class: u32,
        choice: mq_runtime::learn::LearnedChoice,
    },
    /// Answers at once from the request loop: the core is alive.
    Ping {
        id: u64,
    },
    /// "Remove from Continue Watching": the title leaves Continue Watching
    /// and Next Up until it is played again. Replies `{}`.
    RemoveFromContinue {
        id: u64,
        #[serde(rename = "itemId")]
        item_id: String,
    },
    /// Every language the core knows, for the language pickers. Replies
    /// `[{code, name, device}]`, `device` on the one the device's code
    /// (`device`, "nb") reads as.
    Languages {
        id: u64,
        #[serde(default)]
        device: Option<String>,
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
    /// `memoryPressure` (frees caches) or `networkChanged` (drops pooled
    /// connections; `network` is the new type when the app knows it:
    /// `wifi`, `ethernet`, `cellular` or `other`).
    Lifecycle {
        id: u64,
        event: String,
        #[serde(default)]
        network: Option<mq_runtime::speed::Network>,
    },
    /// The profile's Trakt record from the keystore (right after `open` and
    /// after a switch or unlock), or the acknowledgement of one the `trakt`
    /// view offered; null when this device holds none.
    SetTrakt {
        id: u64,
        #[serde(default)]
        record: Option<TraktRecord>,
    },
    /// One Trakt round now ("Sync now"). Replies `TraktReport`.
    TraktSync {
        id: u64,
    },
    /// Signs this device out of Trakt for the profile, or for `profile` (the
    /// owner only, for another: a sign-out that removes the library signs
    /// every profile out first, D13); the app then deletes its record.
    TraktSignOut {
        id: u64,
        #[serde(default)]
        profile: Option<String>,
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
    /// Clears the owner's PIN without it. Apps send it only after the
    /// device's own lock prompt (section 4 of plans/m11-profiles.md).
    ResetOwnerPin {
        id: u64,
    },
}

impl Request {
    /// What a locked session still answers (section 9, N5): the picker,
    /// unlocking and switching, sync (it applies records and shows nothing),
    /// liveness and closing views.
    fn allowed_while_locked(&self) -> bool {
        match self {
            // The debrid view carries rotated records the keystore must get
            // even while locked.
            Request::Subscribe { view, .. } => view == "profiles" || view == "debrid",
            Request::Unsubscribe { .. }
            | Request::UnlockProfile { .. }
            | Request::SwitchProfile { .. }
            | Request::ResetOwnerPin { .. }
            | Request::SyncNow { .. }
            // Debrid accounts are the device's: an app hands them over at
            // start, locked or not.
            | Request::SetDebridKeys { .. }
            | Request::RestoreDebrid { .. }
            | Request::AckDebrid { .. }
            | Request::SetDebridKey { .. }
            | Request::DisconnectDebrid { .. }
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
    SignIn(SignInView),
    Settings { _view: SettingsView },
    Addons { _view: AddonsView },
    Genres { _view: GenresView },
    AddonCatalog(AddonCatalogView),
    Import { _view: ImportView },
    Profiles { _view: ProfilesView },
    Debrid { _view: DebridView },
    DebridLogin { _view: DebridLoginView },
    Trakt { _view: TraktView },
    TraktLogin { _view: TraktLoginView },
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
            View::SignIn(_) => "signIn",
            View::Settings { .. } => "settings",
            View::Addons { .. } => "addons",
            View::Genres { .. } => "genres",
            View::AddonCatalog(_) => "addonCatalog",
            View::Import { .. } => "import",
            View::Profiles { .. } => "profiles",
            View::Debrid { .. } => "debrid",
            View::DebridLogin { .. } => "debridLogin",
            View::Trakt { .. } => "trakt",
            View::TraktLogin { .. } => "traktLogin",
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
    /// Native sign-in flows started and not yet finished, by flow id, with
    /// when they started: their verifiers stay here, in memory only, for as
    /// long as the relay takes the flow.
    native: HashMap<String, (std::time::Instant, mq_sync::identity::NativeFlow)>,
}

/// A native flow lives this long at the relay; then the core drops it too.
const NATIVE_FLOW_LIFE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

impl Shared {
    /// A switch or a lock: every view but the picker and the device's debrid
    /// accounts leaves the session and older work is fenced off. The views it returns close (playback saves
    /// its position) when the caller drops them, after the session lock.
    #[must_use = "drop the closed views after releasing the session lock"]
    fn transition(&mut self, core: Core, locked: bool) -> Vec<Subscription> {
        self.generation += 1;
        self.core = core;
        let (picker, closed) = std::mem::take(&mut self.subs)
            .into_iter()
            .partition(|(_, s)| matches!(s.view, View::Profiles { .. } | View::Debrid { .. }));
        self.subs = picker;
        self.set_locked(locked);
        closed.into_values().collect()
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

/// Fetches installed manifests again in the background, at most once a day
/// (`Core::refresh_addons`): when a session starts and when the app comes to
/// the foreground. Views see a changed manifest through the addon list.
fn refresh_addons(core: &Core) {
    let core = core.clone();
    tokio::spawn(async move {
        if core
            .refresh_addons(mq_library::hlc::now_ms())
            .await
            .is_err()
        {
            tracing::debug!("installed addon manifests not refreshed");
        }
    });
}

/// One client connection. Dropping it cancels every view it opened.
pub struct Session {
    shared: Arc<Mutex<Shared>>,
    out: Output,
    next_sub: u64,
    watcher: JoinHandle<()>,
    /// When the app last went to the background (wall clock, ms: a
    /// monotonic clock stops while the device sleeps).
    background_ms: Option<u64>,
    /// The link to the app stays on this machine or is encrypted (FFI, a
    /// loopback socket). A passkey's PRF output is a key: it never comes
    /// over a plain link from another machine.
    private_link: bool,
    /// Tests only: where `addonCatalog` reads its lists instead of Cinemeta.
    /// No request can set it, so an app cannot point the lists elsewhere.
    #[cfg(test)]
    catalog_source: Option<String>,
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
    pub fn new(core: Core, out: Output) -> Self {
        Self::over(core, out, true)
    }

    /// A session over a link that is private (it stays on this machine, or
    /// is encrypted) or not; on a plain link from another machine
    /// `savePasskey` and `recover {prf}` answer `plainLink`.
    pub fn over(core: Core, out: Output, private_link: bool) -> Self {
        // Subscribed before the profile is chosen: a removal after this
        // reaches the watcher, one before it is in the list `start` reads.
        let changes = core.library().subscribe();
        let mut session = Self::start(core.active(), changes, out);
        session.private_link = private_link;
        session
    }

    fn start(core: Core, changes: broadcast::Receiver<Vec<Table>>, out: Output) -> Self {
        let gone = core.library().gone_profiles().unwrap_or_default();
        // A profile removed after it was chosen starts locked.
        let locked = gone.iter().any(|id| id == core.profile()) || core.has_pin();
        // Removals made while the app was closed: it may still hold their
        // keystore entries.
        for id in &gone {
            let _ = out.send(&json!({ "profile": "gone", "id": id }));
        }
        let (state, _) = watch::channel((core.profile().to_owned(), locked));
        let shared = Arc::new(Mutex::new(Shared {
            core: core.clone(),
            locked,
            generation: 0,
            subs: HashMap::new(),
            state,
            native: HashMap::new(),
        }));
        refresh_addons(&core);
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
            background_ms: None,
            private_link: true,
            #[cfg(test)]
            catalog_source: None,
        }
    }

    /// Cinemeta, where `addonCatalog` reads its lists (a debug build's local
    /// fake when `MQ_DEV_CINEMETA` names one).
    fn catalog_source(&self) -> String {
        #[cfg(test)]
        if let Some(source) = &self.catalog_source {
            return source.clone();
        }
        dev::catalog_source().unwrap_or_else(|| mq_runtime::CINEMETA.to_owned())
    }

    fn core(&self) -> Core {
        lock(&self.shared).core.clone()
    }

    /// A passkey's PRF output as sent: refused over a plain link from
    /// another machine (it is a key), and unless it is 32 bytes.
    fn prf(&self, text: &str) -> Result<[u8; 32], Failure> {
        if !self.private_link {
            return Err(Failure::coded(
                "plainLink",
                "a passkey does not travel an unencrypted link; use the app on this device",
            ));
        }
        mq_sync::pair::read_prf(text)
            .ok_or_else(|| Failure::from("prf is 32 bytes as base64url".to_owned()))
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
            send_answer(&self.out, id, Err(refused));
            return;
        }
        let core = self.core();
        match request {
            Request::Subscribe { id, view, params } => {
                let result = self.subscribe(&view, &params);
                let sub = result.as_ref().ok().and_then(|r| r["sub"].as_u64());
                let reply = message(json!(id), result.map_err(Failure::from));
                if self.out.send(&reply) == Err(SendError::TooLarge) {
                    // An oversized snapshot opens no view: the client gets a
                    // failed subscribe it can show, never a partial view.
                    if let Some(sub) = sub {
                        lock(&self.shared).subs.remove(&sub);
                    }
                    let _ = self.out.send(&message(json!(id), Err(too_large())));
                }
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
            Request::DefaultAddons { id } => {
                self.spawn_fenced(id, async move {
                    let defaults = core.default_addons().await.map_err(|e| e.to_string())?;
                    to_value(&defaults)
                });
            }
            Request::InstallDefaults { id, choices } => {
                let result = core.install_defaults(&choices);
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
                ticket,
                move_link,
                form,
            } => {
                self.spawn(id, async move {
                    let made = mq_sync::create_account_with(
                        core.http(),
                        &server,
                        invite.as_deref(),
                        &name,
                        ticket.as_deref(),
                        move_link,
                        form.as_deref(),
                    )
                    .await;
                    // Not joined: a new account still gets the recommended
                    // addons (plans/accounts.md 3.3).
                    if made.is_ok() {
                        core.created_sync_account();
                    }
                    answer(made)
                });
            }
            Request::RelayInfo { id, server } => {
                self.spawn(id, async move {
                    let providers = mq_sync::identity::providers(core.http(), &server).await?;
                    Ok::<_, mq_sync::SyncError>(json!({ "providers": providers }))
                });
            }
            Request::NativeSignInStart {
                id,
                server,
                provider,
                revoke,
            } => {
                let shared = self.shared.clone();
                self.spawn(id, async move {
                    let flow =
                        mq_sync::identity::start_native(core.http(), &server, &provider, revoke)
                            .await?;
                    let answer = json!({
                        "flow": flow.flow, "nonce": flow.nonce, "expiresMs": flow.expires_ms,
                    });
                    let mut shared = lock(&shared);
                    // A cancelled sheet, or a flow renewed while the screen stays open.
                    shared
                        .native
                        .retain(|_, (started, _)| started.elapsed() < NATIVE_FLOW_LIFE);
                    let now = std::time::Instant::now();
                    shared.native.insert(flow.flow.clone(), (now, flow));
                    Ok::<_, mq_sync::SyncError>(answer)
                });
            }
            Request::NativeSignIn {
                id,
                flow,
                id_token,
                authorization_code,
            } => {
                let Some((started, native)) = lock(&self.shared).native.remove(&flow) else {
                    let refused = Failure::coded("wrongFlow", "this sign-in was not started here");
                    send_answer(&self.out, json!(id), Err(refused));
                    return;
                };
                let shared = self.shared.clone();
                self.spawn(id, async move {
                    let taken = native
                        .finish(core.http(), &id_token, authorization_code.as_deref())
                        .await;
                    // No definite answer: the flow stays for a retry with
                    // the same token.
                    if matches!(&taken, Err(e) if e.transient()) {
                        lock(&shared).native.insert(flow, (started, native));
                    }
                    to_value(&SignInSnapshot::taken(taken?)).map_err(Failure::from)
                });
            }
            Request::Identities { id, record } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    let (links, record) =
                        mq_sync::identity::identities(library, http, &record, clock).await?;
                    Ok::<_, mq_sync::SyncError>(json!({ "links": links, "record": record }))
                });
            }
            Request::LinkIdentity {
                id,
                record,
                ticket,
                move_link,
            } => {
                // Who signs in to the whole account: the owner decides (D16).
                if let Err(refused) = owner_only(&core) {
                    send_answer(&self.out, json!(id), Err(refused));
                    return;
                }
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    let record = mq_sync::identity::link_identity(
                        library, http, &record, clock, &ticket, move_link,
                    )
                    .await?;
                    Ok::<_, mq_sync::SyncError>(json!({ "record": record }))
                });
            }
            Request::UnlinkIdentity {
                id,
                record,
                link,
                receipt,
            } => {
                if let Err(refused) = owner_only(&core) {
                    send_answer(&self.out, json!(id), Err(refused));
                    return;
                }
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    let record = mq_sync::identity::unlink_identity(
                        library,
                        http,
                        &record,
                        clock,
                        link,
                        receipt.as_deref(),
                    )
                    .await?;
                    Ok::<_, mq_sync::SyncError>(json!({ "record": record }))
                });
            }
            Request::CancelDeletion { id, record, unlink } => {
                // Any profile: a cancel only protects the account.
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    let record =
                        mq_sync::identity::cancel_deletion(library, http, &record, clock, unlink)
                            .await?;
                    Ok::<_, mq_sync::SyncError>(json!({ "record": record }))
                });
            }
            Request::AckAnnouncements { id, ids } => {
                let owner = owner_only(&core);
                self.spawn(id, async move {
                    // After any round that is queueing more.
                    let _gate = core.sync_gate().await;
                    // Joins and passkeys are the owner's to dismiss: another
                    // profile sees them but cannot clear them unseen.
                    if let Err(refused) = owner {
                        let queued = mq_sync::queued_announcements(core.library());
                        if queued
                            .iter()
                            .any(|a| a.kind.security() && ids.contains(&a.id))
                        {
                            return Err(refused);
                        }
                    }
                    answer(mq_sync::ack_announcements(core.library(), &ids).map(|()| json!({})))
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
                // A kit is a portable copy of the account key (D17).
                if let Err(refused) = owner_only(&core) {
                    return send_answer(&self.out, json!(id), Err(refused));
                }
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::pair::make_kit(library, http, &record, clock)
                        .await
                        .map(|(kit, id, record)| json!({ "kit": kit, "id": id, "record": record }))
                });
            }
            Request::SavePasskey {
                id,
                record,
                prf,
                label,
            } => {
                let checked = owner_only(&core).and_then(|()| self.prf(&prf));
                let prf = match checked {
                    Ok(prf) => prf,
                    Err(refused) => return send_answer(&self.out, json!(id), Err(refused)),
                };
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::pair::save_passkey(library, http, &record, clock, &prf, &label)
                        .await
                        .map(|(id, record)| json!({ "id": id, "record": record }))
                });
            }
            Request::Recover {
                id,
                server,
                kit,
                prf,
                name,
                form,
            } => {
                let way = match (kit, prf) {
                    (Some(kit), None) => mq_sync::pair::Recovery::Kit(kit),
                    (None, Some(prf)) => match self.prf(&prf) {
                        Ok(prf) => mq_sync::pair::Recovery::Passkey(prf),
                        Err(refused) => return send_answer(&self.out, json!(id), Err(refused)),
                    },
                    _ => {
                        let refused =
                            Failure::from("recover takes a kit or a passkey's prf".to_owned());
                        return send_answer(&self.out, json!(id), Err(refused));
                    }
                };
                self.spawn(id, async move {
                    let joined = mq_sync::pair::recover_with(
                        core.http(),
                        &server,
                        way,
                        &name,
                        form.as_deref(),
                    )
                    .await;
                    if joined.is_ok() {
                        core.joined_sync_account();
                    }
                    let (record, via) = joined?;
                    Ok::<_, Failure>(json!({ "record": record, "via": via }))
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
            Request::SignOut {
                id,
                record,
                force,
                local,
                wipe,
                disconnect_debrid,
                remove_after,
            } => {
                // The removal empties every profile's library: the owner's
                // to ask and to run (D13).
                if (wipe || remove_after.is_some())
                    && let Err(refused) = owner_only(&core)
                {
                    return send_answer(&self.out, json!(id), Err(refused));
                }
                if let Some(record) = record {
                    // A wipe is its own call after the leave (D13), once the
                    // app dropped its keystore entries: a failed leave then
                    // never costs a Trakt sign-in or a debrid key.
                    if wipe || disconnect_debrid {
                        let refused =
                            Failure::from("leave first, then wipe without a record".to_owned());
                        return send_answer(&self.out, json!(id), Err(refused));
                    }
                    // Recorded before the leave goes out: an app closed while
                    // it runs still finds the removal at its next start. A
                    // sign-out without it is the user's latest choice: an
                    // older removal that never ran (its leave never went out)
                    // is off.
                    match &remove_after {
                        Some(after) => {
                            if let Err(e) = core.mark_removal(after.debrid) {
                                return send_answer(
                                    &self.out,
                                    json!(id),
                                    Err(Failure::from(e.to_string())),
                                );
                            }
                        }
                        None => core.clear_removal(),
                    }
                    let marked = remove_after.is_some();
                    self.spawn(id, async move {
                        let _gate = core.sync_gate().await;
                        let (library, http, clock) = (core.library(), core.http(), core.clock());
                        let left = match local {
                            true => mq_sync::leave_here(library, &record),
                            false => mq_sync::sign_out(library, http, &record, clock, force).await,
                        };
                        // Refused for sure: the device keeps its account, and
                        // its library with it. No answer (a network error, a
                        // 5xx) may mean the relay took the leave: the removal
                        // stays, and the next start runs it once the device
                        // holds no account.
                        if marked && matches!(&left, Err(e) if !e.transient()) {
                            core.clear_removal();
                        }
                        left?;
                        Ok::<_, Failure>(json!({}))
                    });
                    return;
                }
                if let Some(after) = remove_after.filter(|_| !wipe) {
                    // A device whose account was deleted: nothing to leave,
                    // only the removal to record before its steps run.
                    self.spawn(id, async move {
                        let _gate = core.sync_gate().await;
                        if core.library().in_sync_account().unwrap_or(true) {
                            return Err(Failure::from(mq_sync::SyncError::WrongRecord));
                        }
                        core.mark_removal(after.debrid)
                            .map_err(|e| Failure::from(e.to_string()))?;
                        Ok::<_, Failure>(json!({}))
                    });
                    return;
                }
                if !wipe {
                    return send_answer(
                        &self.out,
                        json!(id),
                        Err(Failure::from(mq_sync::SyncError::WrongRecord)),
                    );
                }
                let (shared, out) = (self.shared.clone(), self.out.clone());
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    // Nothing to leave: only a device that holds no account.
                    if core.library().in_sync_account().unwrap_or(true) {
                        return Err(Failure::from(mq_sync::SyncError::WrongRecord));
                    }
                    if disconnect_debrid && core.debrid_held() {
                        return Err(Failure::coded(
                            "debridConnected",
                            "disconnect each debrid account first",
                        ));
                    }
                    // Each profile's Trakt sign-in goes with it (D13).
                    if core.trakt_held().unwrap_or(true) {
                        return Err(Failure::coded(
                            "traktConnected",
                            "sign each profile out of Trakt first",
                        ));
                    }
                    let gone = core.wipe().map_err(|e| Failure::from(e.to_string()))?;
                    // The session goes on as the device's new owner, its
                    // views closed; then each profile that was here is gone.
                    let owner = core.for_profile(mq_library::OWNER);
                    owner
                        .make_active()
                        .map_err(|e| Failure::from(e.to_string()))?;
                    let closed = lock(&shared).transition(owner, false);
                    drop(closed);
                    for profile in gone {
                        let _ = out.send(&json!({ "profile": "gone", "id": profile }));
                    }
                    Ok::<_, Failure>(json!({}))
                });
            }
            Request::DeleteAccount {
                id,
                record,
                receipt,
            } => {
                // The account holds every profile: only the owner deletes it.
                if let Err(refused) = owner_only(&core) {
                    send_answer(&self.out, json!(id), Err(refused));
                    return;
                }
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::delete_account(library, http, &record, clock, receipt.as_deref())
                        .await
                        .map(|()| json!({}))
                });
            }
            Request::CancelPairing { id, record, qr } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(mq_sync::pair::cancel_pairing(library, http, &record, clock, &qr).await)
                });
            }
            Request::ForkPreview { id, record } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    answer(mq_sync::fork_preview(core.library(), core.http(), &record).await)
                });
            }
            Request::UseServerRoster {
                id,
                record,
                drop,
                head,
            } => {
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    answer(
                        mq_sync::use_server_roster(library, http, &record, clock, drop, Some(head))
                            .await,
                    )
                });
            }
            Request::Ping { id } => self.reply(json!(id), Ok(json!({}))),
            Request::RemoveFromContinue { id, item_id } => {
                let result = core.remove_from_continue(&item_id);
                self.reply(json!(id), done(result));
            }
            Request::Languages { id, device } => {
                let all = mq_runtime::languages::all(device.as_deref());
                self.reply(json!(id), Ok(json!(all)));
            }
            Request::SetTrakt { id, record } => {
                self.spawn_fenced(id, async move {
                    core.set_trakt(record).await.map_err(trakt_failure)?;
                    Ok::<_, Failure>(json!({}))
                });
            }
            Request::TraktSync { id } => {
                self.spawn_fenced(id, async move {
                    let report = core.trakt_sync().await.map_err(trakt_failure)?;
                    to_value(&report).map_err(Failure::from)
                });
            }
            Request::TraktSignOut { id, profile } => {
                let target = match profile.filter(|p| p != core.profile()) {
                    None => core,
                    Some(other) => {
                        if let Err(refused) = owner_only(&core) {
                            return send_answer(&self.out, json!(id), Err(refused));
                        }
                        if !matches!(core.library().profile(&other), Ok(Some(_))) {
                            let unknown = Failure::coded("unknownProfile", "no such profile");
                            return send_answer(&self.out, json!(id), Err(unknown));
                        }
                        core.for_profile(&other)
                    }
                };
                self.spawn_fenced(id, async move {
                    target.trakt_sign_out().await.map_err(trakt_failure)?;
                    Ok::<_, Failure>(json!({}))
                });
            }
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
            // Replies once running scrobbles are paused, or after 4 s.
            Request::Lifecycle { id, event, .. } if event == "background" => {
                self.background_ms = Some(mq_library::hlc::now_ms());
                self.spawn(id, async move {
                    let _ = tokio::time::timeout(TRAKT_FLUSH, core.trakt_background()).await;
                    Ok::<_, Failure>(json!({}))
                });
            }
            // Locks a PIN profile that was away too long, then checks the
            // stream server (it can take a second) and replies `{serverMoved}`.
            Request::Lifecycle { id, event, .. } if event == "foreground" => {
                refresh_addons(&core);
                core.trakt_foreground();
                let away = self
                    .background_ms
                    .take()
                    .map(|at| mq_library::hlc::now_ms().saturating_sub(at));
                if away.is_some_and(|ms| ms > AWAY_LOCK.as_millis() as u64) && core.has_pin() {
                    let mut shared = lock(&self.shared);
                    if !shared.locked {
                        let closed = shared.transition(core.clone(), true);
                        drop(shared);
                        drop(closed);
                    }
                }
                self.spawn(id, async move {
                    let moved = core.on_foreground().await;
                    Ok::<_, Failure>(json!({ "serverMoved": moved }))
                });
            }
            Request::Lifecycle { id, event, network } => {
                let result = match event.as_str() {
                    "memoryPressure" => {
                        core.on_memory_pressure();
                        Ok(json!({}))
                    }
                    "networkChanged" => {
                        core.on_network_changed(network);
                        Ok(json!({}))
                    }
                    other => Err(format!("unknown lifecycle event: {other}")),
                };
                self.reply(json!(id), result);
            }
            Request::StreamConditions { id } => {
                let result = to_value(&core.stream_conditions());
                self.reply(json!(id), result);
            }
            Request::ResetSpeed { id } => {
                let result = to_value(&core.reset_speed());
                self.reply(json!(id), result);
            }
            Request::ForgetLearned { id } => {
                let result = core
                    .forget_learned()
                    .map_err(|e| e.to_string())
                    .and_then(|p| to_value(&p));
                self.reply(json!(id), result);
            }
            Request::UndoLearned {
                id,
                screen_class,
                choice,
            } => {
                let result = core
                    .undo_learned(screen_class, choice)
                    .map_err(|e| e.to_string())
                    .and_then(|p| to_value(&p));
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
                    .map(|(service, key)| debrid_service(&service).map(|id| (id, key)))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|keys| json!({ "stores": core.set_debrid_keys(&keys) }));
                self.reply(json!(id), result);
            }
            Request::RestoreDebrid {
                id,
                records,
                tombstones,
            } => {
                let result = core
                    .restore_debrid(records, tombstones)
                    .map_err(|e| e.to_string())
                    .and_then(|answer| to_value(&answer));
                self.reply(json!(id), result);
            }
            Request::AckDebrid {
                id,
                service,
                generation,
                revision,
            } => {
                core.ack_debrid(&service, &generation, revision);
                self.reply(json!(id), Ok(json!({})));
            }
            Request::SetDebridKey { id, service, key } => {
                let result = debrid_service(&service)
                    .map(|s| json!({ "generation": core.set_debrid_key(s, &key).generation }));
                self.reply(json!(id), result);
            }
            Request::DisconnectDebrid {
                id,
                service,
                generation,
            } => {
                let result = debrid_service(&service).map(|s| {
                    core.disconnect_debrid(s, &generation);
                    json!({})
                });
                self.reply(json!(id), result);
            }
            Request::PreviewAddon {
                id,
                url,
                acknowledge_token,
            } => {
                if refuse_token_link(&self.out, id, &url, acknowledge_token) {
                    return;
                }
                // Writes nothing, so not bound to the profile; the install that follows is.
                self.spawn(id, async move {
                    core.preview_addon(&url)
                        .await
                        .map(|a| json!(a))
                        .map_err(|e| e.to_string())
                });
            }
            Request::InstallAddon {
                id,
                url,
                acknowledge_token,
            } => {
                if refuse_token_link(&self.out, id, &url, acknowledge_token) {
                    return;
                }
                // Bound to the profile: a switch first makes it `switched`.
                self.spawn_fenced(id, async move {
                    core.install_addon(&url)
                        .await
                        .map(|a| json!({ "id": a.id(), "name": a.manifest.name }))
                        .map_err(|e| e.to_string())
                });
            }
            Request::InstallListed { id, key } => {
                // Only a link an open list shows, with the id it named.
                let entry = lock(&self.shared)
                    .subs
                    .values()
                    .find_map(|s| match &s.view {
                        View::AddonCatalog(view) => view.listed_entry(&key).ok(),
                        _ => None,
                    });
                match entry {
                    // Bound to the profile, as installAddon.
                    Some(entry) => self.spawn_fenced(id, async move {
                        core.install_listed(&entry)
                            .await
                            .map(|(id, name)| json!({ "id": id, "name": name }))
                    }),
                    None => {
                        let refused = ListedError::Refused(ListedFailure::NotListed);
                        send_answer(&self.out, json!(id), Err(refused.into()));
                    }
                }
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
                send_answer(&self.out, json!(id), result);
            }
            Request::UnlockProfile { id, pin } => {
                let result = self.unlock(pin.as_deref()).await;
                self.reply_profiles(id, result);
            }
            Request::LockSession { id } => {
                let closed = lock(&self.shared).transition(core, true);
                drop(closed);
                self.reply_profiles(id, Ok(()));
            }
            Request::ResetOwnerPin { id } => {
                let result = core.reset_owner_pin();
                self.reply_profiles(id, result);
            }
        }
    }

    fn reply(&self, id: Value, result: Result<Value, String>) {
        send_answer(&self.out, id, result.map_err(Failure::from));
    }

    /// The picker's snapshot after a profile op, or the op's error.
    fn reply_profiles(&self, id: u64, result: Result<(), ProfileError>) {
        let result = result.map_err(Failure::from).map(|()| {
            let shared = lock(&self.shared);
            json!(shared.core.profiles_snapshot(shared.locked))
        });
        send_answer(&self.out, json!(id), result);
    }

    /// Moves the session to `profile` (its PIN when it has one). Refused
    /// while any view but the picker is open: the app closes them first, so
    /// playback has saved its position.
    async fn switch(&mut self, profile: &str, pin: Option<&str>) -> Result<Value, Failure> {
        let open: Vec<&str> = lock(&self.shared)
            .subs
            .values()
            .map(|s| s.view.name())
            // The device's debrid accounts stay open across a switch, as
            // `transition` keeps them.
            .filter(|name| *name != "profiles" && *name != "debrid")
            .collect();
        if !open.is_empty() {
            return Err(Failure::coded("viewsOpen", "close the open views first")
                .with(json!({ "views": open })));
        }
        let target = self.core().for_profile(profile);
        target.check_pin(profile, pin).await?;
        self.commit_switch(target)
    }

    /// The switch, once its PIN passed. The target is checked again under
    /// the session lock: a deletion can land while the PIN is checked, and
    /// the watcher only locks the session a removed profile is running.
    fn commit_switch(&mut self, target: Core) -> Result<Value, Failure> {
        let mut shared = lock(&self.shared);
        let usable = target
            .library()
            .profile(target.profile())
            .map_err(ProfileError::from)?;
        if !usable.is_some_and(|p| p.ready()) {
            return Err(ProfileError::Unknown.into());
        }
        target.make_active()?;
        let closed = shared.transition(target, false);
        let snapshot = json!(shared.core.profiles_snapshot(false));
        drop(shared);
        drop(closed);
        Ok(snapshot)
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
            send_answer(&out, json!(id), result);
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
            send_answer(&out, json!(id), result);
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
            // Device-wide: no profile fence.
            "debrid" => {
                let debrid = DebridView::open(&core);
                let (snapshot, updates) = debrid.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, None));
                (
                    View::Debrid { _view: debrid },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "debridLogin" => {
                let service = debrid_service(&param(params, "service")?)?;
                let login = DebridLoginView::open(&core, service);
                let (snapshot, updates) = login.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, None));
                (
                    View::DebridLogin { _view: login },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "trakt" => {
                let trakt = TraktView::open(&core);
                let (snapshot, updates) = trakt.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Trakt { _view: trakt }, to_value(&snapshot)?, forward)
            }
            "traktLogin" => {
                let login = TraktLoginView::open(&core);
                let (snapshot, updates) = login.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (
                    View::TraktLogin { _view: login },
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
                if let Some(network) = versions_network(params)? {
                    core.set_network(network);
                }
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
            "addonCatalog" => {
                let fresh = params.get("fresh").and_then(Value::as_bool) == Some(true);
                let catalog = AddonCatalogView::open_from(&core, &self.catalog_source(), fresh);
                let (snapshot, updates) = catalog.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::AddonCatalog(catalog), to_value(&snapshot)?, forward)
            }
            "import" => {
                let import = ImportView::open(&core, dev::stremio_endpoints());
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
                let form = param(params, "form").ok();
                let pairing =
                    PairingView::open(&core, &param(params, "server")?, &name, form.as_deref());
                let (snapshot, updates) = pairing.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Pairing(pairing), to_value(&snapshot)?, forward)
            }
            // A brokered sign-in; `appleRevoke` is Apple's revoke flow, for
            // surfaces without a native Apple sheet.
            "signIn" | "appleRevoke" => {
                let revoke = view == "appleRevoke";
                let provider = match revoke {
                    true => "apple".to_owned(),
                    false => param(params, "provider")?,
                };
                let sign_in = SignInView::open(
                    &core,
                    &param(params, "server")?,
                    &provider,
                    &param(params, "return")?,
                    revoke,
                );
                let (snapshot, updates) = sign_in.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::SignIn(sign_in), to_value(&snapshot)?, forward)
            }
            "discover" => {
                let discover = DiscoverView::open(&core);
                let (snapshot, updates) = discover.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Discover(discover), to_value(&snapshot)?, forward)
            }
            "genres" => {
                let genres = GenresView::open(&core);
                let (snapshot, updates) = genres.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (
                    View::Genres { _view: genres },
                    to_value(&snapshot)?,
                    forward,
                )
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
            (View::Title(title), "titleVisible") => {
                let visible = command
                    .get("visible")
                    .and_then(Value::as_bool)
                    .ok_or("visible missing")?;
                title.set_visible(visible);
            }
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
                let text = |key: &str| command.get(key).and_then(Value::as_str).map(str::to_owned);
                // Absent keeps the current value; null clears it.
                let maybe = |key: &str| command.get(key).map(|v| v.as_str().map(str::to_owned));
                discover.choose(mq_runtime::DiscoverChoice {
                    kind: text("kind"),
                    platform: maybe("platform"),
                    sort: text("sort"),
                    genre: maybe("genre"),
                    decade: maybe("decade"),
                    catalog: text("catalog"),
                });
            }
            (View::Discover(discover), "loadMore") => discover.load_more(),
            // An absent or null id clears the preview.
            (View::Discover(discover), "preview") => {
                discover.preview(command.get("id").and_then(Value::as_str));
            }
            (View::Discover(discover), "setInLibrary") => {
                let id = param(command, "id")?;
                let value = command
                    .get("value")
                    .and_then(Value::as_bool)
                    .ok_or("value missing")?;
                discover
                    .set_in_library(&id, value)
                    .map_err(|e| e.to_string())?;
            }
            (View::Pairing(pairing), "confirm") => pairing.confirm(),
            (View::Pairing(pairing), "reject") => pairing.reject(),
            (View::SignIn(sign_in), "returned") => sign_in.returned(&param(command, "fragment")?),
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
            (View::Home { _view: home }, "retryRow") => {
                let row_id = param(command, "rowId")?;
                if !home.retry_row(&row_id) {
                    return Err(format!("no Home row {row_id}"));
                }
            }
            // `query`, when given, is the field's text now (the debounce may not have caught up).
            (View::Search(search), "remember") => search
                .remember(command.get("query").and_then(Value::as_str))
                .map_err(|e| e.to_string())?,
            (View::Playback(playback), "progress") => {
                let ms = |key: &str| {
                    command
                        .get(key)
                        .and_then(Value::as_u64)
                        .ok_or(format!("{key} missing"))
                };
                let paused = command
                    .get("paused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                playback.report_progress(ms("positionMs")?, ms("durationMs")?, paused);
                // A film playing on (Picture in Picture, audio behind a
                // locked screen) is not time away (Apple journey review r2
                // P2-A): each report while it plays restarts the away clock.
                if !paused && self.background_ms.is_some() {
                    self.background_ms = Some(mq_library::hlc::now_ms());
                }
            }
            (View::Playback(playback), "duration") => {
                let duration_ms = command
                    .get("durationMs")
                    .and_then(Value::as_u64)
                    .ok_or("durationMs missing")?;
                playback.report_duration(&param(command, "key")?, duration_ms);
            }
            (View::Playback(playback), "retry") => playback.retry(),
            (View::Playback(playback), "playedOn") => {
                playback.played_on(&param(command, "key")?, &param(command, "engine")?)?;
            }
            (View::Playback(playback), "subtitleChosen") => {
                let url = command.get("url").and_then(Value::as_str);
                let language = command.get("language").and_then(Value::as_str);
                playback.subtitle_chosen(url, language);
            }
            (View::Playback(playback), "subtitleDelay") => {
                let ms = command
                    .get("ms")
                    .and_then(Value::as_i64)
                    .ok_or("ms missing")?;
                playback.subtitle_delay(&param(command, "url")?, ms);
            }
            (View::Playback(playback), "stalled") => {
                let stalled = command
                    .get("stalled")
                    .and_then(Value::as_bool)
                    .ok_or("stalled missing")?;
                let position_ms = command
                    .get("positionMs")
                    .and_then(Value::as_u64)
                    .ok_or("positionMs missing")?;
                playback.stalled(stalled, position_ms);
            }
            (View::Playback(playback), "failOver") => {
                let error = param(command, "error").unwrap_or_else(|_| "player error".into());
                let attempt = command.get("attempt").and_then(Value::as_u64);
                let kind = command.get("kind").and_then(Value::as_str);
                let status = command.get("status").and_then(Value::as_u64);
                let status = status.and_then(|s| u16::try_from(s).ok());
                playback.fail_over(attempt, kind, status, &error);
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
        // The pick counts toward learning and hears back when it fails.
        let pick = versions.pick(&param(params, "key")?);
        let plays = versions.plays();
        drop(shared);
        if pick.candidates.is_empty() {
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
        Ok(PlaybackView::open_picked(core, pick, target, &plays))
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

impl From<ListedError> for Failure {
    fn from(e: ListedError) -> Self {
        match e {
            ListedError::Refused(failure) => Failure::coded(failure.code(), failure.words()),
            ListedError::Core(e) => Failure::from(e.to_string()),
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

/// Queues a reply. One over the size limit becomes a small error reply with
/// `code: tooLarge`; a closed output drops it (the session is ending).
fn send_answer(out: &Output, id: Value, result: Result<Value, Failure>) {
    if out.send(&message(id.clone(), result)) == Err(SendError::TooLarge) {
        let _ = out.send(&message(id, Err(too_large())));
    }
}

fn too_large() -> Failure {
    Failure::coded("tooLarge", "the answer is too large to send")
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

/// How long `lifecycle background` waits for scrobbles to be paused.
const TRAKT_FLUSH: std::time::Duration = std::time::Duration::from_secs(4);
/// A PIN profile away longer than this locks on its return (journey review
/// 2026-10-20, Android P2-8).
const AWAY_LOCK: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// A Trakt op's failure: code `notConfigured` when this build has no Trakt
/// app.
fn trakt_failure(e: mq_runtime::CoreError) -> Failure {
    match e {
        mq_runtime::CoreError::TraktNotConfigured => {
            Failure::coded("notConfigured", "Trakt is not set up in this build")
        }
        other => Failure::from(other.to_string()),
    }
}

fn done(result: Result<(), mq_runtime::CoreError>) -> Result<Value, String> {
    result.map(|()| json!({})).map_err(|e| e.to_string())
}

fn debrid_service(name: &str) -> Result<StoreId, String> {
    StoreId::from_service(name).ok_or(format!("unknown debrid service: {name}"))
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

/// Ops that change the whole account (its deletion, who signs in to it):
/// the main profile only (D15, D16).
fn owner_only(core: &Core) -> Result<(), Failure> {
    if core.profile() == mq_library::OWNER {
        Ok(())
    } else {
        Err(Failure::coded(
            "notOwner",
            "only the main profile can change the account",
        ))
    }
}

fn answer<T: serde::Serialize>(result: Result<T, mq_sync::SyncError>) -> Result<Value, Failure> {
    to_value(&result?).map_err(Failure::from)
}

/// The session generation a view was opened in; `None` for the picker,
/// which outlives switches and locks.
type Fence = Option<(Weak<Mutex<Shared>>, u64)>;

async fn forward<S, U>(sub: u64, mut updates: S, out: Output, fence: Fence)
where
    S: futures_util::Stream<Item = Vec<U>> + Unpin,
    U: serde::Serialize,
{
    while let Some(batch) = updates.next().await {
        let Ok(updates) = serde_json::to_value(&batch) else {
            continue;
        };
        let push = json!({ "sub": sub, "updates": updates });
        // A push over the size limit ends the session as an overflow does:
        // the next session's snapshot of the view fails with `tooLarge`
        // instead of looping.
        let send = |push: &Value| match out.send(push) {
            Ok(()) => true,
            Err(SendError::TooLarge) => {
                out.overflow();
                false
            }
            Err(SendError::Closed) => false,
        };
        let sent = match &fence {
            None => send(&push),
            // Checked and sent under the session lock: once a switch or a
            // lock has bumped the generation, nothing older goes out.
            Some((shared, generation)) => shared.upgrade().is_some_and(|shared| {
                let shared = lock(&shared);
                shared.generation == *generation && send(&push)
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
    out: Output,
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
        // Checked against every tombstone on each pass, not only new ones,
        // so a session that reached a removed profile still locks.
        let mut closed = Vec::new();
        if !shared.locked && gone.iter().any(|id| id == shared.core.profile()) {
            let core = shared.core.clone();
            closed = shared.transition(core, true);
        }
        for id in gone.iter().filter(|id| !known.contains(*id)) {
            let _ = out.send(&json!({ "profile": "gone", "id": id }));
        }
        drop(shared);
        drop(closed);
        known = gone.into_iter().collect();
    }
}

/// Versions params: `kind`, `videoId`, `title` (for wrong-content checks),
/// optional `season`, `episode`, `bingeGroup`, and what the device can do:
/// `displayHeight`, `hdr` ("dolby_vision", "hdr10", "hdr10_plus", "hlg"),
/// `decoders` ("h264", "h265", "av1", "vp9", ...), `tenBitHevc`, `tenBitH264`
/// (true when left out: Apple sends false until its second engine is measured),
/// `passthrough` and `audioDecoders` ("aac", "ac3", "eac3", "dts",
/// "true_hd", "atmos", ...), `containers` ("mkv", "mp4", ...) and
/// `hasFallbackEngine`, and `hvc1Only` (Apple: HEVC in MP4 shows only from
/// an entry with its parameter sets). What is left out keeps a 1080p SDR
/// stick's default.
/// The title as the app knows it (plans/m13-stream-choice.md 4.3): `runtime`
/// ("142 min"), `releaseInfo` ("2002", "2022-"), the episode's `released`
/// ("2013-09-16") and `episodesInSeason`; the core reads the meta for what
/// is left out. `network` (`wifi`, `ethernet`, `cellular`, `other`) is
/// read by `versions_network`.
/// `language` (the device's, read as the core's tag: "nb" is "no") stands in
/// for audio preferences the user has not set: that language first, then
/// English.
fn versions_params(params: &Value, mut prefs: StreamPrefs) -> Result<VersionsParams, String> {
    let num = |key: &str| params.get(key).and_then(Value::as_u64).map(|n| n as u32);
    if prefs.languages.is_empty() {
        let device: Option<String> = field(params, "language")?;
        let device = device.map(|code| mq_runtime::languages::tag(&code));
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
    if let Some(ten_bit) = field(params, "tenBitH264")? {
        caps.ten_bit_h264 = ten_bit;
    }
    if let Some(passthrough) = field(params, "passthrough")? {
        caps.passthrough = passthrough;
    }
    if let Some(fallback) = field(params, "hasFallbackEngine")? {
        caps.has_fallback_engine = fallback;
    }
    if let Some(hvc1_only) = field(params, "hvc1Only")? {
        caps.hvc1_only = hvc1_only;
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
        facts: TitleFacts::read(
            field::<String>(params, "runtime")?.as_deref(),
            field::<String>(params, "releaseInfo")?.as_deref(),
            field::<String>(params, "released")?.as_deref(),
            num("episodesInSeason"),
        ),
    })
}

/// The network type a `versions` subscription names, if any.
fn versions_network(params: &Value) -> Result<Option<mq_runtime::speed::Network>, String> {
    field(params, "network")
}

/// A sign-in in a pasted link's settings: warned and acknowledged first, for
/// the preview and the install alike. True when the request was refused.
fn refuse_token_link(out: &Output, id: u64, url: &str, acknowledged: bool) -> bool {
    if acknowledged || !mq_runtime::link_carries_token(url) {
        return false;
    }
    let refused = Failure::coded(
        "tokenInLink",
        "this link carries a sign-in in its settings: install it only if it is yours",
    );
    send_answer(out, json!(id), Err(refused));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The caps the Apple app sends with its second engine on, read from the
    /// file its own test reads too (plans/apple-second-engine.md 2.5): every
    /// name is the core's, or every Versions request would fail.
    #[test]
    fn apples_engine_caps_are_the_cores_names() {
        let params: Value = serde_json::from_str(include_str!(
            "../../../apps/apple/Tests/Fixtures/engine-caps.json"
        ))
        .unwrap();
        let caps = versions_params(&params, StreamPrefs::default())
            .expect("every name is known")
            .caps;
        assert!(caps.opens("mkv") && caps.opens("avi") && !caps.ten_bit_h264);
        assert!(caps.decoders.contains(&mq_relparse::VideoCodec::Xvid));
        assert_eq!(caps.audio_decoders.len(), 13);
    }

    #[test]
    fn versions_params_carry_what_the_device_can_do() {
        let params = json!({
            "kind": "movie", "videoId": "tt1", "displayHeight": 2160,
            "decoders": ["h264", "h265", "av1"], "tenBitHevc": false,
            "passthrough": ["eac3", "atmos"], "hasFallbackEngine": false, "hvc1Only": true,
            "audioDecoders": ["aac", "opus"], "containers": ["MP4", "webm"],
            "language": "tr", "runtime": "52 min", "releaseInfo": "2008-2013",
            "released": "2013-09-29", "episodesInSeason": 16, "network": "wifi"
        });
        let parsed = versions_params(&params, StreamPrefs::default()).unwrap();
        let caps = parsed.caps;
        assert_eq!(caps.display_height, 2160);
        assert_eq!(json!(caps.decoders), json!(["h264", "h265", "av1"]));
        assert!(!caps.ten_bit_hevc && !caps.has_fallback_engine && caps.hvc1_only);
        // Left out, 10-bit H.264 stays on (the default); Apple says false.
        assert!(caps.ten_bit_h264);
        let apple = json!({ "kind": "movie", "videoId": "tt1", "tenBitH264": false });
        assert!(
            !versions_params(&apple, StreamPrefs::default())
                .unwrap()
                .caps
                .ten_bit_h264
        );
        assert_eq!(json!(caps.passthrough), json!(["eac3", "atmos"]));
        assert_eq!(json!(caps.audio_decoders), json!(["aac", "opus"]));
        assert_eq!(caps.containers, ["mp4", "webm"]);
        let facts = &parsed.facts;
        assert_eq!(
            (facts.runtime_s, facts.title_year),
            (Some(52 * 60), Some(2008))
        );
        assert_eq!(
            (facts.episode_year, facts.episodes_in_season),
            (Some(2013), Some(16))
        );
        assert_eq!(
            versions_network(&params).unwrap(),
            Some(mq_runtime::speed::Network::Wifi)
        );
        assert_eq!(
            parsed.prefs.languages,
            ["tr", "en"],
            "device language, then English"
        );

        // A device's own code reads as the core's tag: Bokmål is Norwegian.
        let mut norway = params.clone();
        norway["language"] = json!("nb");
        let parsed = versions_params(&norway, StreamPrefs::default()).unwrap();
        assert_eq!(parsed.prefs.languages, ["no", "en"]);

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
        let (tx, mut rx) = Output::channel();
        let mut session = Session::new(Core::temporary("p").unwrap(), tx);
        session
            .handle_text(&json!({"id": 1, "op": "ping"}).to_string())
            .await;
        assert_eq!(
            rx.recv().await.unwrap().json(),
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
            .unwrap()
            .json();
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
        let (tx, mut rx) = Output::channel();
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
            assert_eq!(rx.recv().await.unwrap().json()["ok"], true, "{event}");
        }
        session
            .handle_text(&json!({"id": 2, "op": "lifecycle", "event": "sleep"}).to_string())
            .await;
        assert_eq!(rx.recv().await.unwrap().json()["ok"], false);
    }

    /// Apple journey review r2 P2-A: forty minutes in the background with a
    /// film playing (reports with `paused` false) is no time away; the same
    /// with the film paused is, and locks.
    #[tokio::test]
    async fn a_film_playing_in_the_background_is_no_time_away() {
        let core = Core::temporary(mq_library::OWNER).unwrap();
        core.set_pin(mq_library::OWNER, Some("1357"), None)
            .await
            .unwrap();
        let playback = PlaybackView::open(&core, Vec::new(), PlayTarget::default());
        let (tx, mut rx) = Output::channel();
        let mut session = Session::new(core, tx);
        let unlock = json!({"id": 1, "op": "unlockProfile", "pin": "1357"});
        session.handle_text(&unlock.to_string()).await;
        assert_eq!(rx.recv().await.unwrap().json()["ok"], true);
        lock(&session.shared).subs.insert(
            7,
            Subscription {
                view: View::Playback(playback),
                forward: tokio::spawn(async {}),
            },
        );
        for (paused, locks) in [(false, false), (true, true)] {
            let background = json!({"id": 2, "op": "lifecycle", "event": "background"});
            session.handle_text(&background.to_string()).await;
            assert_eq!(rx.recv().await.unwrap().json()["ok"], true);
            let at = session.background_ms.unwrap();
            session.background_ms = Some(at - 40 * 60_000);
            let report = json!({"id": 3, "op": "command", "sub": 7, "command":
                {"type": "progress", "positionMs": 60_000, "durationMs": 888_000, "paused": paused}});
            session.handle_text(&report.to_string()).await;
            assert_eq!(
                rx.recv().await.unwrap().json()["ok"],
                true,
                "paused {paused}"
            );
            let foreground = json!({"id": 4, "op": "lifecycle", "event": "foreground"});
            session.handle_text(&foreground.to_string()).await;
            assert_eq!(rx.recv().await.unwrap().json()["ok"], true);
            assert_eq!(lock(&session.shared).locked, locks, "paused {paused}");
        }
    }

    /// Journey review 2026-10-20 (Android P2-8): a PIN profile back after
    /// more than 30 minutes away is locked; a short trip away is not.
    #[tokio::test]
    async fn a_pin_profile_away_for_half_an_hour_locks() {
        let core = Core::temporary(mq_library::OWNER).unwrap();
        core.set_pin(mq_library::OWNER, Some("1357"), None)
            .await
            .unwrap();
        let (tx, mut rx) = Output::channel();
        let mut session = Session::new(core, tx);
        let unlock = json!({"id": 1, "op": "unlockProfile", "pin": "1357"});
        session.handle_text(&unlock.to_string()).await;
        assert_eq!(rx.recv().await.unwrap().json()["ok"], true);
        for (minutes, want) in [(5, false), (31, true)] {
            let background = json!({"id": 2, "op": "lifecycle", "event": "background"});
            session.handle_text(&background.to_string()).await;
            assert_eq!(rx.recv().await.unwrap().json()["ok"], true);
            let at = session.background_ms.unwrap();
            session.background_ms = Some(at - minutes * 60_000);
            let foreground = json!({"id": 3, "op": "lifecycle", "event": "foreground"});
            session.handle_text(&foreground.to_string()).await;
            let reply = rx.recv().await.unwrap().json();
            assert_eq!(reply["result"]["serverMoved"], false, "{minutes}");
            assert_eq!(lock(&session.shared).locked, want, "{minutes} minutes away");
        }
    }
}

#[cfg(test)]
mod fence {
    use super::*;
    use tokio::sync::mpsc;

    /// A view task still running when a lock or switch moves the generation
    /// (the race the session lock closes) sends nothing older.
    #[tokio::test]
    async fn pushes_of_an_older_generation_never_go_out() {
        let (tx, mut rx) = Output::channel();
        let session = Session::new(Core::temporary("p").unwrap(), tx);
        let generation = lock(&session.shared).generation;
        let (feed, mut updates) = mpsc::unbounded_channel::<Vec<u32>>();
        let stream = futures_util::stream::poll_fn(move |cx| updates.poll_recv(cx));
        let fence = Some((Arc::downgrade(&session.shared), generation));
        let task = tokio::spawn(forward(7, stream, session.out.clone(), fence));
        feed.send(vec![1]).unwrap();
        assert_eq!(
            rx.recv().await.unwrap().json(),
            json!({"sub": 7, "updates": [1]})
        );

        lock(&session.shared).generation += 1;
        feed.send(vec![2]).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("the forwarder stops")
            .unwrap();
        assert!(rx.try_recv().is_err(), "an older push went out");
    }

    /// A deletion that lands while a switch checks the PIN: the switch is
    /// refused instead of leaving the session unlocked on a tombstone.
    #[tokio::test]
    async fn a_switch_whose_target_goes_during_the_pin_check_is_refused() {
        let core = Core::temporary("default").unwrap();
        let kid = core.create_profile("Kid", "fox").unwrap();
        let (tx, _rx) = Output::channel();
        let mut session = Session::new(core.clone(), tx);
        let target = core.for_profile(&kid);
        target.check_pin(&kid, None).await.unwrap();
        core.delete_profile(&kid).unwrap();
        let refused = session.commit_switch(target).expect_err("refused");
        assert_eq!(refused.code, Some("unknownProfile"), "{}", refused.error);
        let shared = lock(&session.shared);
        assert_eq!((shared.core.profile(), shared.locked), ("default", false));
    }

    /// A session running a profile whose removal was already announced (the
    /// race above, had it gone through) still locks at the next change.
    #[tokio::test]
    async fn a_session_on_a_known_tombstone_locks() {
        let core = Core::temporary("default").unwrap();
        let kid = core.create_profile("Kid", "fox").unwrap();
        core.delete_profile(&kid).unwrap();
        let (tx, _rx) = Output::channel();
        let session = Session::new(core.clone(), tx);
        lock(&session.shared).core = core.for_profile(&kid);
        core.update_profile("default", Some("Owner"), None, None)
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !lock(&session.shared).locked {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the session locks");
    }
}

#[cfg(test)]
mod closing {
    use super::*;

    /// A lock hands the views it closes back to the caller, which drops
    /// them once the session lock is released (playback saves on drop).
    #[tokio::test]
    async fn a_lock_hands_back_the_views_it_closes() {
        let core = Core::temporary("default").unwrap();
        let (tx, _rx) = Output::channel();
        let mut session = Session::new(core.clone(), tx);
        for (id, view) in [(1, "home"), (2, "profiles"), (3, "addons")] {
            let open = json!({"id": id, "op": "subscribe", "view": view});
            session.handle_text(&open.to_string()).await;
        }
        let mut shared = lock(&session.shared);
        let closed = shared.transition(core, true);
        let mut names: Vec<&str> = closed.iter().map(|s| s.view.name()).collect();
        names.sort();
        assert_eq!(names, ["addons", "home"]);
        let left: Vec<&str> = shared.subs.values().map(|s| s.view.name()).collect();
        assert_eq!(left, ["profiles"]);
    }
}

#[cfg(test)]
mod startup {
    use super::*;

    /// A profile removed between being chosen and the session reading the
    /// removals (another open handle, or sync) starts the session locked.
    #[tokio::test]
    async fn a_profile_removed_while_the_session_starts_starts_it_locked() {
        let core = Core::temporary("default").unwrap();
        let kid = core.create_profile("Kid", "fox").unwrap();
        let changes = core.library().subscribe();
        let chosen = core.for_profile(&kid);
        core.delete_profile(&kid).unwrap();
        let (tx, _rx) = Output::channel();
        let session = Session::start(chosen, changes, tx);
        let shared = lock(&session.shared);
        assert_eq!((shared.core.profile(), shared.locked), (kid.as_str(), true));
    }
}

#[cfg(test)]
mod catalog {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc::UnboundedReceiver;
    use tokio::sync::oneshot;

    const LOCALHOST: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    /// A list of one entry, an addon at `https://{host}:{port}`.
    fn one_entry(host: &str, port: u16, id: &str) -> Value {
        json!({"addons": [{
            "transportUrl": format!("https://{host}:{port}/manifest.json"),
            "transportName": "http",
            "manifest": {
                "id": id, "version": "1.0.0", "name": id,
                "resources": ["subtitles"], "types": ["movie"], "catalogs": []
            }
        }]})
    }

    /// A local Cinemeta answering both lists with what `list` holds at the
    /// time, cacheable for an hour as Cinemeta's are.
    async fn cinemeta(list: Arc<Mutex<Value>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let list = list.lock().unwrap().to_string();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: public, max-age=3600\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{list}",
                        list.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}/manifest.json")
    }

    /// An addon at `host` (standing for a public one, `set_public`) that
    /// accepts connections and never answers, until `release`: then every
    /// connection, and any later one, fails.
    async fn hanging(host: &str) -> (u16, oneshot::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, mut released) = oneshot::channel();
        tokio::spawn(async move {
            let mut held = Vec::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => match accepted {
                        Ok((socket, _)) => held.push(socket),
                        Err(_) => return,
                    },
                    _ = &mut released => return,
                }
            }
        });
        mq_http::test_dns::set_public(host, &[LOCALHOST]);
        (port, release)
    }

    /// The next message for `pick`, skipping the others.
    async fn next(rx: &mut UnboundedReceiver<Outgoing>, pick: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let message = rx.recv().await.expect("the session is open").json();
                if pick(&message) {
                    return message;
                }
            }
        })
        .await
        .expect("the message arrives")
    }

    async fn reply(rx: &mut UnboundedReceiver<Outgoing>, id: u64) -> Value {
        next(rx, |m| m["id"] == id).await
    }

    async fn send(session: &mut Session, request: Value) {
        session.handle_text(&request.to_string()).await;
    }

    /// Opens the view (`fresh` as given) and waits for both lists; returns
    /// the sub and the first list's entries.
    async fn open(
        session: &mut Session,
        rx: &mut UnboundedReceiver<Outgoing>,
        id: u64,
        fresh: bool,
    ) -> (u64, Value) {
        send(
            session,
            json!({"id": id, "op": "subscribe", "view": "addonCatalog", "params": {"fresh": fresh}}),
        )
        .await;
        let opened = reply(rx, id).await;
        let sub = opened["result"]["sub"].as_u64().expect("a sub");
        let ready = |lists: &Value| {
            lists
                .as_array()
                .is_some_and(|l| l.iter().all(|list| list["state"]["state"] == "ready"))
        };
        let lists = if ready(&opened["result"]["snapshot"]["lists"]) {
            opened["result"]["snapshot"]["lists"].clone()
        } else {
            let push = next(rx, |m| {
                m["sub"] == sub
                    && m["updates"]
                        .as_array()
                        .and_then(|u| u.last())
                        .is_some_and(|s| ready(&s["lists"]))
            })
            .await;
            push["updates"].as_array().unwrap().last().unwrap()["lists"].clone()
        };
        (sub, lists[0]["entries"].clone())
    }

    fn key(entries: &Value) -> String {
        entries[0]["key"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn install_listed_takes_only_what_an_open_list_shows() {
        let core = Core::temporary("default").unwrap();
        let kid = core.create_profile("Kid", "fox").unwrap();
        let (tx, mut rx) = Output::channel();
        let mut session = Session::new(core.clone(), tx);
        let (port, release) = hanging("hang.m12.test").await;
        let list = one_entry("hang.m12.test", port, "hang");
        session.catalog_source = Some(cinemeta(Arc::new(Mutex::new(list))).await);
        // Nothing open: refused with its code.
        let key_url = format!("https://hang.m12.test:{port}/manifest.json");
        send(
            &mut session,
            json!({"id": 1, "op": "installListed", "key": key_url}),
        )
        .await;
        let refused = reply(&mut rx, 1).await;
        assert_eq!(refused["code"], "notListed", "{refused}");
        assert_eq!(refused["error"], "This addon is no longer in the list");
        // Open: the entry is taken, and its install is in flight (the addon
        // does not answer yet).
        let (sub, entries) = open(&mut session, &mut rx, 2, false).await;
        let key = key(&entries);
        assert_eq!(key, key_url);
        send(
            &mut session,
            json!({"id": 3, "op": "installListed", "key": key}),
        )
        .await;
        // Closed: the same key is refused again.
        send(
            &mut session,
            json!({"id": 4, "op": "unsubscribe", "sub": sub}),
        )
        .await;
        assert_eq!(reply(&mut rx, 4).await["ok"], true);
        send(
            &mut session,
            json!({"id": 5, "op": "installListed", "key": key}),
        )
        .await;
        assert_eq!(reply(&mut rx, 5).await["code"], "notListed");
        // A switch while the install is in flight, then the addon fails
        // it: the answer is `switched`, not the old profile's failure.
        send(
            &mut session,
            json!({"id": 6, "op": "switchProfile", "profile": kid}),
        )
        .await;
        assert_eq!(reply(&mut rx, 6).await["ok"], true);
        release.send(()).unwrap();
        let in_flight = reply(&mut rx, 3).await;
        assert_eq!(in_flight["code"], "switched", "{in_flight}");
        assert!(core.library().addons("default").unwrap().is_empty());
        assert!(core.library().addons(&kid).unwrap().is_empty());
    }

    #[tokio::test]
    async fn fresh_opens_past_the_cache() {
        let core = Core::temporary("default").unwrap();
        let (tx, mut rx) = Output::channel();
        let mut session = Session::new(core, tx);
        let list = Arc::new(Mutex::new(one_entry("old.m12.test", 443, "old")));
        session.catalog_source = Some(cinemeta(list.clone()).await);
        let (sub, entries) = open(&mut session, &mut rx, 1, false).await;
        assert_eq!(entries[0]["id"], "old");
        send(
            &mut session,
            json!({"id": 2, "op": "unsubscribe", "sub": sub}),
        )
        .await;
        reply(&mut rx, 2).await;
        // Cinemeta's list changes: a plain open still shows the cached one,
        // Refresh the new one.
        *list.lock().unwrap() = one_entry("new.m12.test", 443, "new");
        let (sub, entries) = open(&mut session, &mut rx, 3, false).await;
        assert_eq!(entries[0]["id"], "old");
        send(
            &mut session,
            json!({"id": 4, "op": "unsubscribe", "sub": sub}),
        )
        .await;
        reply(&mut rx, 4).await;
        let (_, entries) = open(&mut session, &mut rx, 5, true).await;
        assert_eq!(entries[0]["id"], "new");
    }

    #[tokio::test]
    async fn install_listed_installs_over_the_wire() {
        let core = Core::temporary("default").unwrap();
        let (tx, mut rx) = Output::channel();
        let mut session = Session::new(core.clone(), tx);
        let manifest = json!({
            "id": "ok", "version": "1.0.0", "name": "Ok Subtitles",
            "resources": ["subtitles"], "types": ["movie"], "catalogs": []
        })
        .to_string();
        let port = mq_http::test_dns::https(move |path| match path {
            "/manifest.json" => format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{manifest}",
                manifest.len()
            ),
            _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        });
        mq_http::test_dns::set_public("ok.m12.test", &[LOCALHOST]);
        let list = one_entry("ok.m12.test", port, "ok");
        session.catalog_source = Some(cinemeta(Arc::new(Mutex::new(list))).await);
        let (sub, entries) = open(&mut session, &mut rx, 1, false).await;
        assert_eq!(entries[0]["installed"], "absent");
        let key = key(&entries);
        send(
            &mut session,
            json!({"id": 2, "op": "installListed", "key": key}),
        )
        .await;
        let installed = reply(&mut rx, 2).await;
        assert_eq!(installed["ok"], true, "{installed}");
        assert_eq!(
            installed["result"],
            json!({"id": "ok", "name": "Ok Subtitles"})
        );
        let rows = core.library().addons("default").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].transport_url, key);
        // The open list shows it on.
        let push = next(&mut rx, |m| {
            m["sub"] == sub
                && m["updates"]
                    .as_array()
                    .and_then(|u| u.last())
                    .is_some_and(|s| s["lists"][0]["entries"][0]["installed"] == "on")
        })
        .await;
        assert_eq!(push["sub"], sub);
    }

    #[test]
    fn install_failures_carry_their_code_and_words() {
        let wire = [
            (
                ListedFailure::Timeout,
                "timeout",
                "This addon's server did not answer in time",
            ),
            (
                ListedFailure::Unreachable,
                "unreachable",
                "This addon's server could not be reached",
            ),
            (
                ListedFailure::NeedsSetup,
                "needsSetup",
                "This addon now needs setup first",
            ),
            (
                ListedFailure::Mismatch,
                "mismatch",
                "This addon is not the one the list named",
            ),
            (
                ListedFailure::Adult,
                "adult",
                "This addon is for adults and is not shown",
            ),
            (
                ListedFailure::NotListed,
                "notListed",
                "This addon is no longer in the list",
            ),
        ];
        for (failure, code, words) in wire {
            let failure = Failure::from(ListedError::Refused(failure));
            let sent = message(json!(7), Err(failure));
            assert_eq!(
                sent,
                json!({"id": 7, "ok": false, "code": code, "error": words})
            );
        }
    }
}
