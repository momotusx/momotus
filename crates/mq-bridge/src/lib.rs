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
//! "cancelPairing" | "readPairingCode" |
//! "forkPreview" | "useServerRoster" | "createProfile" | "allowTitle" | "updateProfile" | "deleteProfile" |
//! "setPin" | "switchProfile" | "unlockProfile" | "lockSession" | "removeFromContinue" |
//! "languages" | "languagePicker" | "plugins" | "pluginDirectory" | "previewPluginRepo" | "addPluginRepo" | "removePluginRepo" |
//! "setPluginRepoEnabled" | "setPluginEnabled",
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
use mq_runtime::downloads::Action;
use mq_runtime::{
    AddonCatalogView, AddonsView, CalendarView, Core, DebridLoginView, DebridRecord,
    DebridTombstone, DebridView, DiscoverView, DownloadsView, GenresView, HomeView, ImportView,
    LibrarySort, LibraryView, ListedError, ListedFailure, PairingView, PlayTarget, PlaybackView,
    ProfileError, ProfilesView, SearchView, SessionState, SettingsView, SignInSnapshot, SignInView,
    StoreId, TitleView, TraktLoginView, TraktRecord, TraktView, VersionsParams, VersionsView,
};
use mq_streams::StreamPrefs;
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
    /// The still first run frames its questions with (`Core::first_run_still`),
    /// or null within 3 s. Replies `FirstRunStill` or null.
    FirstRunStill {
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
    /// The profile's Nuvio plugin repositories and their providers
    /// (`PluginsAnswer`); stale manifests are read again first.
    Plugins {
        id: u64,
    },
    /// The Nuvio repositories nuvioplugins.com lists, for Find addons (R8). Replies
    /// `PluginDirectory`.
    PluginDirectory {
        id: u64,
    },
    /// What a pasted repository link would add, for the consent sheet;
    /// writes nothing. Replies `PluginRepoPreview`.
    PreviewPluginRepo {
        id: u64,
        url: String,
    },
    /// Adds a repository the user agreed to. Replies `PluginsAnswer`.
    AddPluginRepo {
        id: u64,
        url: String,
    },
    RemovePluginRepo {
        id: u64,
        url: String,
    },
    SetPluginRepoEnabled {
        id: u64,
        url: String,
        enabled: bool,
    },
    /// One provider of repository `url` on or off.
    SetPluginEnabled {
        id: u64,
        url: String,
        plugin: String,
        enabled: bool,
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
    /// Mails a sign-in code to `email` (plans/accounts-email.md):
    /// `{flow, expiresMs}`; the flow's verifier stays in the core.
    EmailSignInStart {
        id: u64,
        server: String,
        email: String,
    },
    /// The mailed code for `flow`: `found` or `new`. `wrongCode` and
    /// `badCode` keep the flow for another try.
    EmailSignIn {
        id: u64,
        flow: String,
        code: String,
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
        /// `scan` (the app's own scanner) or `link` (the default: the system
        /// camera, the link host's page, a message), plans/tv-qr-login.md Q2.
        #[serde(default)]
        via: Option<String>,
        /// The screen lock was just passed (asked on `unlockRequired`).
        #[serde(default)]
        unlocked: bool,
    },
    /// What a pairing code says, for the approving sheet.
    ReadPairingCode {
        id: u64,
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
    /// Answers at once from the request loop: the core is alive. Replies
    /// `{liveScopes}`, the Incognito scopes alive on the device (the e2e
    /// guard, plans/batch-10-incognito.md 2.7).
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
    /// The language picker's answer (plans/onboarding-language-picker.md):
    /// replies `{suggested, noneFirst, results}` for what was typed
    /// (`query`), the profile's `chosen` languages in rank order and the
    /// device's code; `audio` gives one row per primary language.
    LanguagePicker {
        id: u64,
        #[serde(default)]
        device: Option<String>,
        #[serde(default)]
        query: String,
        #[serde(default)]
        chosen: Vec<String>,
        #[serde(default)]
        audio: bool,
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
    /// Downloads a video, a season's aired episodes or every regular
    /// season's (plans/offline-downloads.md 4.15). `params` carries what
    /// `versions` takes (the device's caps, `language`); `pick` a version
    /// the user chose on a `purpose: "download"` Versions screen. Replies
    /// `{ids}`.
    DownloadAdd {
        id: u64,
        kind: String,
        #[serde(rename = "itemId")]
        item_id: String,
        scope: mq_runtime::downloads::Scope,
        #[serde(default, rename = "videoId")]
        video_id: Option<String>,
        #[serde(default)]
        season: Option<u32>,
        #[serde(default)]
        params: Value,
        #[serde(default)]
        pick: Option<DownloadPick>,
    },
    /// Pauses, resumes, removes (cancels and deletes) or tries again the
    /// downloads `ids`, or with `all` every one; remove also takes a
    /// title's (`itemId`). Retry with `mobileData` lets one waiting for
    /// Wi-Fi go on mobile data. Replies `{}`.
    DownloadPause {
        id: u64,
        #[serde(flatten)]
        which: DownloadIds,
    },
    DownloadResume {
        id: u64,
        #[serde(flatten)]
        which: DownloadIds,
    },
    DownloadRemove {
        id: u64,
        #[serde(flatten)]
        which: DownloadIds,
    },
    DownloadRetry {
        id: u64,
        #[serde(flatten)]
        which: DownloadIds,
        #[serde(default, rename = "mobileData")]
        mobile_data: bool,
    },
    /// The device's downloads settings. Replies `{}`.
    SetDownloadSettings {
        id: u64,
        #[serde(default, rename = "wifiOnly")]
        wifi_only: Option<bool>,
        #[serde(default)]
        smart: Option<bool>,
    },
    /// The platform's say: whether downloads may run now (an iPhone left
    /// in the background without a continued task, an Android job the
    /// system stopped) and its free-space figure. Send it on every change
    /// of the `downloads` view's `active`, once a minute while downloads
    /// run, and at foreground. Replies `{}`.
    DownloadsAllowed {
        id: u64,
        allowed: bool,
        #[serde(default, rename = "freeBytes")]
        free_bytes: Option<u64>,
    },
    /// The app's life as the OS reports it: `foreground`, `background`,
    /// `idle` and `active` (no input for a while, and input again: they feed
    /// only the Incognito clock and answer `{}`), `memoryPressure` (frees
    /// caches) or `networkChanged` (drops pooled
    /// connections; `network` is the new type when the app knows it:
    /// `wifi`, `ethernet`, `cellular`, `other`, or `none` with no
    /// connection; `metered` from the platform's cost flag, `lowData` for
    /// Low Data Mode, plans/offline-downloads.md 4.13).
    Lifecycle {
        id: u64,
        event: String,
        #[serde(default)]
        network: Option<NetworkWord>,
        #[serde(default)]
        metered: Option<bool>,
        #[serde(default, rename = "lowData")]
        low_data: Option<bool>,
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
        /// A kids profile (plans/m11-kids.md): its titles come from
        /// `allowTitle`.
        #[serde(default)]
        kids: bool,
    },
    /// The owner allows `kind` `itemId` for kids profile `profile`.
    /// Replies `ProfilesSnapshot`.
    AllowTitle {
        id: u64,
        profile: String,
        kind: String,
        #[serde(rename = "itemId")]
        item_id: String,
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
    /// Moves the session to `profile`; only the picker may be open. With
    /// `incognito` it enters Incognito from the session's profile, and
    /// `fromChooser` says Leave goes back to the chooser
    /// (plans/batch-10-incognito.md 2.7).
    SwitchProfile {
        id: u64,
        profile: String,
        pin: Option<String>,
        #[serde(default, rename = "fromChooser")]
        from_chooser: bool,
    },
    /// Leaves Incognito for the profile it was entered from, without a PIN.
    /// Replies `ProfilesSnapshot`; `notIncognito` outside Incognito.
    LeaveIncognito {
        id: u64,
    },
    /// The person has read the Incognito ended notice: clears
    /// `incognitoEnded`. Replies `ProfilesSnapshot`.
    AckIncognitoEnded {
        id: u64,
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
            Request::Subscribe { view, .. } => {
                view == "profiles" || view == "debrid" || view == "downloads"
            }
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
            // The ended notice shows over the chooser too, and a Leave
            // pressed as the away end locked the session answers it.
            | Request::AckIncognitoEnded { .. }
            | Request::LeaveIncognito { .. }
            // The platform's say on downloads is the device's.
            | Request::DownloadsAllowed { .. }
            | Request::Lifecycle { .. } => true,
            _ => false,
        }
    }
}

impl Request {
    /// What a session in Incognito answers (plans/batch-10-incognito.md
    /// 2.7, wall 3); everything else is `incognito`. No wildcard: a new op
    /// does not compile until it is classified.
    fn allowed_in_incognito(&self) -> bool {
        match self {
            Request::Subscribe { view, .. } => matches!(
                view.as_str(),
                "profiles"
                    | "home"
                    | "title"
                    | "versions"
                    | "discover"
                    | "genres"
                    | "calendar"
                    | "library"
                    | "search"
                    | "playback"
                    | "settings"
                    | "addons"
                    | "addonCatalog"
                    | "downloads"
                    | "debrid"
            ),
            Request::Unsubscribe { .. }
            | Request::Command { .. }
            | Request::PreviewAddon { .. }
            | Request::InstallAddon { .. }
            | Request::InstallListed { .. }
            | Request::RemoveAddon { .. }
            | Request::MoveAddon { .. }
            | Request::SetAddonEnabled { .. }
            | Request::SetPrefs { .. }
            | Request::HomeCatalogs { .. }
            | Request::SetHomeRow { .. }
            | Request::MoveHomeRow { .. }
            | Request::RemoveFromContinue { .. }
            | Request::StreamConditions { .. }
            | Request::ResetSpeed { .. }
            | Request::ForgetLearned { .. }
            | Request::UndoLearned { .. }
            | Request::Languages { .. }
            | Request::LanguagePicker { .. }
            | Request::Plugins { .. }
            | Request::PluginDirectory { .. }
            | Request::PreviewPluginRepo { .. }
            | Request::AddPluginRepo { .. }
            | Request::RemovePluginRepo { .. }
            | Request::SetPluginRepoEnabled { .. }
            | Request::SetPluginEnabled { .. }
            | Request::DownloadAdd { .. }
            | Request::DownloadPause { .. }
            | Request::DownloadResume { .. }
            | Request::DownloadRemove { .. }
            | Request::DownloadRetry { .. }
            | Request::DownloadsAllowed { .. }
            | Request::SwitchProfile { .. }
            | Request::LeaveIncognito { .. }
            | Request::AckIncognitoEnded { .. }
            | Request::LockSession { .. }
            | Request::SyncNow { .. }
            | Request::Ping { .. }
            | Request::Lifecycle { .. }
            | Request::SetDebridKeys { .. }
            | Request::RestoreDebrid { .. }
            | Request::AckDebrid { .. } => true,
            Request::DefaultAddons { .. }
            | Request::FirstRunStill { .. }
            | Request::InstallDefaults { .. }
            | Request::SetDebridKey { .. }
            | Request::DisconnectDebrid { .. }
            | Request::CreateSyncAccount { .. }
            | Request::RelayInfo { .. }
            | Request::NativeSignInStart { .. }
            | Request::NativeSignIn { .. }
            | Request::EmailSignInStart { .. }
            | Request::EmailSignIn { .. }
            | Request::Identities { .. }
            | Request::LinkIdentity { .. }
            | Request::UnlinkIdentity { .. }
            | Request::CancelDeletion { .. }
            | Request::AckAnnouncements { .. }
            | Request::ApprovePairing { .. }
            | Request::ReadPairingCode { .. }
            | Request::CreateRecoveryKit { .. }
            | Request::SavePasskey { .. }
            | Request::Recover { .. }
            | Request::SyncDevices { .. }
            | Request::LostPreview { .. }
            | Request::RemoveDevice { .. }
            | Request::ReportLost { .. }
            | Request::SignOut { .. }
            | Request::DeleteAccount { .. }
            | Request::CancelPairing { .. }
            | Request::ForkPreview { .. }
            | Request::UseServerRoster { .. }
            | Request::SetDownloadSettings { .. }
            | Request::SetTrakt { .. }
            | Request::TraktSync { .. }
            | Request::TraktSignOut { .. }
            | Request::CreateProfile { .. }
            | Request::AllowTitle { .. }
            | Request::UpdateProfile { .. }
            | Request::DeleteProfile { .. }
            | Request::SetPin { .. }
            | Request::UnlockProfile { .. }
            | Request::ResetOwnerPin { .. } => false,
        }
    }

    /// Whether a person caused this request (plans/batch-10-incognito.md
    /// 2.7): only these keep Incognito's 60-minute bound from running out.
    /// What the apps send on their own never counts: the web's `syncNow`
    /// every minute, `ping`, every `lifecycle` event, the debrid
    /// handshakes, Calendar's `setToday`, a `duration` or a paused report.
    /// No wildcard.
    fn person_driven(&self) -> bool {
        match self {
            Request::Command { command, .. } => match command.get("type").and_then(Value::as_str) {
                Some("setToday" | "duration") => false,
                Some("progress") => command.get("paused").and_then(Value::as_bool) != Some(true),
                _ => true,
            },
            Request::Unsubscribe { .. }
            | Request::SetDebridKeys { .. }
            | Request::RestoreDebrid { .. }
            | Request::AckDebrid { .. }
            | Request::SyncNow { .. }
            | Request::Ping { .. }
            | Request::DownloadsAllowed { .. }
            | Request::Lifecycle { .. } => false,
            Request::Subscribe { .. }
            | Request::PreviewAddon { .. }
            | Request::InstallAddon { .. }
            | Request::DefaultAddons { .. }
            | Request::FirstRunStill { .. }
            | Request::InstallDefaults { .. }
            | Request::InstallListed { .. }
            | Request::RemoveAddon { .. }
            | Request::Plugins { .. }
            | Request::PluginDirectory { .. }
            | Request::PreviewPluginRepo { .. }
            | Request::AddPluginRepo { .. }
            | Request::RemovePluginRepo { .. }
            | Request::SetPluginRepoEnabled { .. }
            | Request::SetPluginEnabled { .. }
            | Request::MoveAddon { .. }
            | Request::SetAddonEnabled { .. }
            | Request::SetDebridKey { .. }
            | Request::DisconnectDebrid { .. }
            | Request::CreateSyncAccount { .. }
            | Request::RelayInfo { .. }
            | Request::NativeSignInStart { .. }
            | Request::NativeSignIn { .. }
            | Request::EmailSignInStart { .. }
            | Request::EmailSignIn { .. }
            | Request::Identities { .. }
            | Request::LinkIdentity { .. }
            | Request::UnlinkIdentity { .. }
            | Request::CancelDeletion { .. }
            | Request::AckAnnouncements { .. }
            | Request::ApprovePairing { .. }
            | Request::ReadPairingCode { .. }
            | Request::CreateRecoveryKit { .. }
            | Request::SavePasskey { .. }
            | Request::Recover { .. }
            | Request::SyncDevices { .. }
            | Request::LostPreview { .. }
            | Request::RemoveDevice { .. }
            | Request::ReportLost { .. }
            | Request::SignOut { .. }
            | Request::DeleteAccount { .. }
            | Request::CancelPairing { .. }
            | Request::ForkPreview { .. }
            | Request::UseServerRoster { .. }
            | Request::SetPrefs { .. }
            | Request::StreamConditions { .. }
            | Request::ResetSpeed { .. }
            | Request::ForgetLearned { .. }
            | Request::UndoLearned { .. }
            | Request::RemoveFromContinue { .. }
            | Request::Languages { .. }
            | Request::LanguagePicker { .. }
            | Request::HomeCatalogs { .. }
            | Request::SetHomeRow { .. }
            | Request::MoveHomeRow { .. }
            | Request::DownloadAdd { .. }
            | Request::DownloadPause { .. }
            | Request::DownloadResume { .. }
            | Request::DownloadRemove { .. }
            | Request::DownloadRetry { .. }
            | Request::SetDownloadSettings { .. }
            | Request::SetTrakt { .. }
            | Request::TraktSync { .. }
            | Request::TraktSignOut { .. }
            | Request::CreateProfile { .. }
            | Request::AllowTitle { .. }
            | Request::UpdateProfile { .. }
            | Request::DeleteProfile { .. }
            | Request::SetPin { .. }
            | Request::SwitchProfile { .. }
            | Request::LeaveIncognito { .. }
            | Request::AckIncognitoEnded { .. }
            | Request::UnlockProfile { .. }
            | Request::LockSession { .. }
            | Request::ResetOwnerPin { .. } => true,
        }
    }

    /// The ops that show a person read an Incognito ended notice: at the
    /// chooser one of them is the only way on (plans/batch-10-incognito.md
    /// 2.7).
    fn reads_the_ended_notice(&self) -> bool {
        matches!(
            self,
            Request::SwitchProfile { .. }
                | Request::UnlockProfile { .. }
                | Request::LeaveIncognito { .. }
                | Request::CreateProfile { .. }
                | Request::AckIncognitoEnded { .. }
        )
    }
}

/// `networkChanged`'s `network`: a type, or `none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum NetworkWord {
    Known(mq_runtime::speed::Network),
    Other(NoNetwork),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum NoNetwork {
    None,
}

/// Which downloads an op names.
#[derive(Debug, Clone, Default, Deserialize)]
struct DownloadIds {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default, rename = "itemId")]
    item_id: Option<String>,
    #[serde(default)]
    all: bool,
}

/// A version picked on a Versions screen to download.
#[derive(Debug, Clone, Deserialize)]
struct DownloadPick {
    #[serde(rename = "versionsSub")]
    versions_sub: u64,
    key: String,
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
    Pairing { _view: PairingView },
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
    Downloads { _view: DownloadsView },
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
            View::Pairing { .. } => "pairing",
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
            View::Downloads { .. } => "downloads",
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
    /// (active profile, locked, Incognito host) for the picker view.
    state: watch::Sender<SessionState>,
    /// Native sign-in flows started and not yet finished, by flow id, with
    /// when they started: their verifiers stay here, in memory only, for as
    /// long as the relay takes the flow.
    native: HashMap<String, (std::time::Instant, mq_sync::identity::NativeFlow)>,
    /// Email sign-in flows, the same way.
    email: HashMap<String, (std::time::Instant, mq_sync::identity::EmailFlow)>,
    /// Where pushes go, and this session itself: the downloads views are
    /// opened again under the lock with a fence of their own.
    out: Output,
    me: Weak<Mutex<Shared>>,
    /// The session's Incognito while it runs (plans/batch-10-incognito.md
    /// 2.7); `core` is then its scope's.
    incognito: Option<Incognito>,
    /// An Incognito end this session has not shown yet: the snapshot's
    /// `incognitoEnded`, until a profile op or `ackIncognitoEnded`.
    ended: bool,
    /// Started while a scope lived: that scope's end is this session's too.
    behind: bool,
    /// Tests: added to the wall clock the Incognito clock reads.
    #[cfg(test)]
    skew_ms: u64,
}

/// The Incognito clock (plans/batch-10-incognito.md 2.7), apart from the
/// PIN lock's `background_ms`. Times are the wall clock in ms: a monotonic
/// clock stops while a Mac or a phone sleeps.
struct Incognito {
    /// Where Leave goes: the profile it copied, and whether it was entered
    /// from the chooser (then Leave locks, so the chooser shows).
    host: String,
    to_chooser: bool,
    /// Since `idle` or `background`, until `active`, `foreground` or a
    /// playing report.
    away_since_ms: Option<u64>,
    /// The absence is `idle` (the window is in view, unused): only then the
    /// one-minute warning goes out, once.
    idle: bool,
    warned: bool,
    /// The last request a person caused (`person_driven`).
    last_request_ms: u64,
    timer: JoinHandle<()>,
}

impl Drop for Incognito {
    fn drop(&mut self) {
        self.timer.abort();
    }
}

/// Incognito ends after this long away or without use, as `AWAY_LOCK`.
const INCOGNITO_AWAY: u64 = 30 * 60_000;
/// The desktop's warning goes out this long into an idle absence.
const INCOGNITO_WARN: u64 = 29 * 60_000;
/// With no request a person caused for this long, Incognito ends: a client
/// that never sends `idle` is still bounded.
const INCOGNITO_BOUND: u64 = 60 * 60_000;
/// How often the Incognito clock looks while the process runs.
const INCOGNITO_TICK: std::time::Duration = std::time::Duration::from_secs(15);

/// Why the Incognito clock ends a scope now, if it does.
fn clock_ran_out(incognito: &Incognito, now: u64) -> bool {
    let away = incognito
        .away_since_ms
        .is_some_and(|at| now.saturating_sub(at) > INCOGNITO_AWAY);
    away || now.saturating_sub(incognito.last_request_ms) > INCOGNITO_BOUND
}

/// A native flow lives this long at the relay; then the core drops it too.
const NATIVE_FLOW_LIFE: std::time::Duration = std::time::Duration::from_secs(10 * 60);

impl Shared {
    /// A switch or a lock: every view but the picker, the device's debrid
    /// accounts and the downloads keeper's view leaves the session and older
    /// work is fenced off; the downloads view goes on for the new session
    /// (`repoint_downloads`). The views it returns close (playback saves its
    /// position) when the caller drops them, after the session lock.
    #[must_use = "drop the closed views after releasing the session lock"]
    fn transition(&mut self, core: Core, locked: bool) -> Vec<Subscription> {
        self.generation += 1;
        if !core.incognito() {
            self.incognito = None;
        }
        self.core = core;
        let (kept, closed) = std::mem::take(&mut self.subs)
            .into_iter()
            .partition(|(_, s)| {
                matches!(
                    s.view,
                    View::Profiles { .. } | View::Debrid { .. } | View::Downloads { .. }
                )
            });
        self.subs = kept;
        self.locked = locked;
        self.publish();
        self.repoint_downloads();
        closed.into_values().collect()
    }

    /// An unlock, or a lock with no switch.
    fn set_locked(&mut self, locked: bool) {
        let changed = self.locked != locked;
        self.locked = locked;
        self.publish();
        if changed {
            self.repoint_downloads();
        }
    }

    /// The picker's state as it is now.
    fn publish(&self) {
        let host = self.incognito.as_ref().map(|i| i.host.clone());
        let state = (
            self.core.profile().to_owned(),
            self.locked,
            host,
            self.ended,
        );
        self.state.send_replace(state);
    }

    /// The picker's snapshot for this session now.
    fn snapshot(&self) -> Value {
        json!(self.core.profiles_snapshot_for(&self.state.borrow()))
    }

    /// The wall clock the Incognito clock reads.
    fn wall_ms(&self) -> u64 {
        #[cfg(test)]
        return mq_library::hlc::now_ms() + self.skew_ms;
        #[cfg(not(test))]
        mq_library::hlc::now_ms()
    }

    /// Ends the session's Incognito (plans/batch-10-incognito.md 2.3) and
    /// moves to `to`: the scope's end, then the switch. `loud` for an end
    /// the person did not ask for (a lock, the clock, the host removed):
    /// the session is told with `{"incognito": "ended"}` and its snapshots
    /// carry `incognitoEnded` until it is read. A Leave or a switch says
    /// nothing.
    #[must_use = "drop the closed views after releasing the session lock"]
    fn end_incognito(&mut self, loud: bool, to: Core, locked: bool) -> Vec<Subscription> {
        if self.core.end_incognito() && loud {
            self.ended = true;
            self.core.incognito_ended_unseen().send_replace(true);
            let _ = self.out.send(&json!({ "incognito": "ended" }));
        }
        self.transition(to, locked)
    }

    /// Where an end the person did not ask for goes: the chooser over the
    /// host, or over the owner when the host is gone.
    fn chooser_after(&self) -> Option<Core> {
        let host = &self.incognito.as_ref()?.host;
        let device = self.core.device();
        Some(if usable(&device, host) {
            device.for_profile(host)
        } else {
            device.for_profile(mq_library::OWNER)
        })
    }

    /// The Incognito clock ran out (`clock_ran_out`): the scope ends, and
    /// the chooser shows with the ended line.
    #[must_use = "drop the closed views after releasing the session lock"]
    fn end_when_out_of_time(&mut self) -> Vec<Subscription> {
        let now = self.wall_ms();
        match &self.incognito {
            Some(incognito) if clock_ran_out(incognito, now) => {
                let to = self.chooser_after().expect("in Incognito");
                self.end_incognito(true, to, true)
            }
            _ => Vec::new(),
        }
    }

    /// The downloads views go on across a switch, a lock and an unlock: the
    /// platform keeper subscribes once for the life of the core (plans/
    /// offline-downloads.md 5), and each sub is opened again for the session
    /// as it is now (the new profile's rows, or the locked counts alone),
    /// with a fresh snapshot pushed at once (code review r2, Apple C1 and
    /// Android N1).
    fn repoint_downloads(&mut self) {
        let ids: Vec<u64> = self
            .subs
            .iter()
            .filter(|(_, s)| matches!(s.view, View::Downloads { .. }))
            .map(|(id, _)| *id)
            .collect();
        for sub in ids {
            let downloads = if self.locked {
                DownloadsView::open_locked(&self.core)
            } else {
                DownloadsView::open(&self.core)
            };
            let (snapshot, updates) = downloads.subscribe();
            let updates = futures_util::stream::once(async move { vec![snapshot] }).chain(updates);
            let fence = Some((self.me.clone(), self.generation));
            let forward = tokio::spawn(forward(sub, Box::pin(updates), self.out.clone(), fence));
            // The old view and its forward end as it is replaced.
            self.subs.insert(
                sub,
                Subscription {
                    view: View::Downloads { _view: downloads },
                    forward,
                },
            );
        }
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

/// A device profile a session can run as now: there, not gone, synced.
fn usable(device: &Core, profile: &str) -> bool {
    matches!(device.library().profile(profile), Ok(Some(p)) if p.ready())
}

/// The Incognito clock's timer (plans/batch-10-incognito.md 2.7): while the
/// process runs and the machine is awake it ends a scope whose time ran out
/// and sends the one-minute warning. A sleeping device stops it; the wall
/// clock checks at the next request cover that.
async fn incognito_clock(shared: Weak<Mutex<Shared>>, generation: u64) {
    loop {
        tokio::time::sleep(INCOGNITO_TICK).await;
        let Some(shared) = shared.upgrade() else {
            return;
        };
        let mut shared = lock(&shared);
        if shared.generation != generation || shared.incognito.is_none() {
            return;
        }
        let closed = shared.end_when_out_of_time();
        if !closed.is_empty() || shared.incognito.is_none() {
            drop(shared);
            drop(closed);
            return;
        }
        let now = shared.wall_ms();
        let incognito = shared.incognito.as_mut().expect("in Incognito");
        let warn = incognito.idle
            && !incognito.warned
            && incognito
                .away_since_ms
                .is_some_and(|at| now.saturating_sub(at) >= INCOGNITO_WARN);
        if warn {
            incognito.warned = true;
            let _ = shared.out.send(&json!({ "incognito": "endsSoon" }));
        }
    }
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
    /// A closed app, window or tab ends its Incognito, with or without an
    /// async runtime; the next session shows the ended line.
    fn drop(&mut self) {
        self.watcher.abort();
        let mut shared = lock(&self.shared);
        if shared.incognito.take().is_some() && shared.core.end_incognito() {
            shared.core.incognito_ended_unseen().send_replace(true);
        }
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
        // Behind a live Incognito scope (a new tab or window) the chooser
        // shows, and that scope's end is this session's too.
        let behind = core.live_scope_count() > 0;
        // An end no session has shown yet shows here.
        let ended = *core.incognito_ended_unseen().borrow();
        // A profile removed after it was chosen starts locked.
        let locked = gone.iter().any(|id| id == core.profile()) || core.has_pin() || behind;
        // Removals made while the app was closed: it may still hold their
        // keystore entries.
        for id in &gone {
            let _ = out.send(&json!({ "profile": "gone", "id": id }));
        }
        let (state, _) = watch::channel((core.profile().to_owned(), locked, None, ended));
        let shared = Arc::new_cyclic(|me| {
            Mutex::new(Shared {
                core: core.clone(),
                locked,
                generation: 0,
                subs: HashMap::new(),
                state,
                native: HashMap::new(),
                email: HashMap::new(),
                out: out.clone(),
                me: me.clone(),
                incognito: None,
                ended,
                behind,
                #[cfg(test)]
                skew_ms: 0,
            })
        });
        refresh_addons(&core);
        // A launch resumes the downloads a kill or a reboot left.
        let _ = core.downloads();
        let ends = core.incognito_ended_unseen().subscribe();
        let watcher = tokio::spawn(watch_gone(
            Arc::downgrade(&shared),
            core,
            (changes, ends),
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
        // The Incognito clock first: a scope whose time ran out (the wall
        // clock, a sleeping Mac included) ends before anything is handled.
        let closed = lock(&self.shared).end_when_out_of_time();
        drop(closed);
        if lock(&self.shared).locked && !request.allowed_while_locked() {
            let refused = Failure::coded("locked", "the profile is locked: unlock it first");
            send_answer(&self.out, id, Err(refused));
            return;
        }
        {
            let mut shared = lock(&self.shared);
            if shared.incognito.is_some() && !request.allowed_in_incognito() {
                drop(shared);
                let refused = Failure::coded("incognito", "not available in Incognito");
                send_answer(&self.out, id, Err(refused));
                return;
            }
            let now = shared.wall_ms();
            if let Some(incognito) = shared.incognito.as_mut()
                && request.person_driven()
            {
                // The person is here: a lost `active` leaves no stale absence.
                incognito.last_request_ms = now;
                incognito.away_since_ms = None;
                incognito.warned = false;
            }
            // Read: the ended notice goes, here and for the next session.
            if shared.ended && request.reads_the_ended_notice() {
                shared.ended = false;
                shared.core.incognito_ended_unseen().send_replace(false);
                shared.publish();
            }
        }
        let core = self.core();
        match request {
            Request::Subscribe { id, view, params } => {
                let result = self.subscribe(&view, &params);
                let sub = result.as_ref().ok().and_then(|r| r["sub"].as_u64());
                let reply = message(json!(id), result);
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
            Request::Plugins { id } => {
                self.spawn_fenced(id, async move {
                    let answer = core.plugin_repos().await?;
                    Ok::<_, Failure>(to_value(&answer)?)
                });
            }
            Request::PluginDirectory { id } => {
                self.spawn_fenced(id, async move { to_value(&core.plugin_directory().await) });
            }
            Request::PreviewPluginRepo { id, url } => {
                self.spawn_fenced(id, async move {
                    let preview = core.preview_plugin_repo(&url).await?;
                    Ok::<_, Failure>(to_value(&preview)?)
                });
            }
            Request::AddPluginRepo { id, url } => {
                self.spawn_fenced(id, async move {
                    let answer = core.add_plugin_repo(&url).await?;
                    Ok::<_, Failure>(to_value(&answer)?)
                });
            }
            Request::RemovePluginRepo { id, url } => {
                let result = core.remove_plugin_repo(&url);
                self.reply(json!(id), done(result));
            }
            Request::SetPluginRepoEnabled { id, url, enabled } => {
                let result = core.set_plugin_repo_enabled(&url, enabled);
                self.reply(json!(id), done(result));
            }
            Request::SetPluginEnabled {
                id,
                url,
                plugin,
                enabled,
            } => {
                let result = core.set_plugin_enabled(&url, &plugin, enabled);
                self.reply(json!(id), done(result));
            }
            Request::DefaultAddons { id } => {
                self.spawn_fenced(id, async move {
                    let defaults = core.default_addons().await?;
                    Ok::<_, Failure>(to_value(&defaults)?)
                });
            }
            Request::FirstRunStill { id } => {
                self.spawn_fenced(id, async move { to_value(&core.first_run_still().await) });
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
            Request::EmailSignInStart { id, server, email } => {
                let shared = self.shared.clone();
                self.spawn(id, async move {
                    let flow = mq_sync::identity::start_email(core.http(), &server, &email).await?;
                    let answer = json!({ "flow": flow.flow, "expiresMs": flow.expires_ms });
                    let mut shared = lock(&shared);
                    shared
                        .email
                        .retain(|_, (started, _)| started.elapsed() < NATIVE_FLOW_LIFE);
                    shared
                        .email
                        .insert(flow.flow.clone(), (std::time::Instant::now(), flow));
                    Ok::<_, mq_sync::SyncError>(answer)
                });
            }
            Request::EmailSignIn { id, flow, code } => {
                let Some((started, email)) = lock(&self.shared).email.remove(&flow) else {
                    let refused = Failure::coded("wrongFlow", "this sign-in was not started here");
                    send_answer(&self.out, json!(id), Err(refused));
                    return;
                };
                // Past the relay's 10 minutes: no call, the same answer it gives.
                if started.elapsed() >= NATIVE_FLOW_LIFE {
                    let expired = Failure::from(mq_sync::SyncError::TicketExpired);
                    send_answer(&self.out, json!(id), Err(expired));
                    return;
                }
                let shared = self.shared.clone();
                self.spawn(id, async move {
                    let taken = email.finish(core.http(), &code).await;
                    // Another try with the same flow: a wrong or short code,
                    // or no definite answer.
                    let again = matches!(
                        &taken,
                        Err(mq_sync::SyncError::WrongCode | mq_sync::SyncError::BadCode)
                    ) || matches!(&taken, Err(e) if e.transient());
                    if again {
                        lock(&shared).email.insert(flow, (started, email));
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
            Request::ApprovePairing {
                id,
                record,
                qr,
                via,
                unlocked,
            } => {
                let how = mq_sync::pair::Approval {
                    scanned: via.as_deref() == Some("scan"),
                    unlocked,
                };
                self.spawn(id, async move {
                    let _gate = core.sync_gate().await;
                    let (library, http, clock) = (core.library(), core.http(), core.clock());
                    mq_sync::pair::approve_pairing_with(library, http, &record, clock, &qr, how)
                        .await
                        .map(|(member, record)| json!({ "member": member, "record": record }))
                });
            }
            Request::ReadPairingCode { id, qr } => {
                send_answer(
                    &self.out,
                    json!(id),
                    answer(mq_sync::pair::read_pairing_code(&qr)),
                );
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
            Request::Ping { id } => {
                let live = core.live_scope_count();
                self.reply(json!(id), Ok::<_, Failure>(json!({ "liveScopes": live })))
            }
            Request::RemoveFromContinue { id, item_id } => {
                let result = core.remove_from_continue(&item_id);
                self.reply(json!(id), done(result));
            }
            Request::DownloadAdd {
                id,
                kind,
                item_id,
                scope,
                video_id,
                season,
                params,
                pick,
            } => {
                let pick = match pick {
                    Some(pick) => match self.download_pick(&pick) {
                        Ok(chain) => Some(chain),
                        Err(e) => {
                            self.reply(json!(id), Err(e));
                            return;
                        }
                    },
                    None => None,
                };
                let request = mq_runtime::downloads::AddRequest {
                    kind,
                    item_id,
                    scope,
                    video_id,
                    season,
                    params,
                    pick,
                };
                self.spawn(id, async move {
                    let ids = mq_runtime::downloads::add(&core, request).await;
                    ids.map(|ids| json!({ "ids": ids })).map_err(Failure::from)
                });
            }
            Request::DownloadPause { id, which } => {
                let result = downloads_act(&core, &which, Action::Pause);
                self.reply(json!(id), done(result));
            }
            Request::DownloadResume { id, which } => {
                let result = downloads_act(&core, &which, Action::Resume);
                self.reply(json!(id), done(result));
            }
            Request::DownloadRemove { id, which } => {
                let result = downloads_act(&core, &which, Action::Remove);
                self.reply(json!(id), done(result));
            }
            Request::DownloadRetry {
                id,
                which,
                mobile_data,
            } => {
                let result = downloads_act(&core, &which, Action::Retry { mobile_data });
                self.reply(json!(id), done(result));
            }
            Request::SetDownloadSettings {
                id,
                wifi_only,
                smart,
            } => {
                let result = mq_runtime::downloads::owner(&core)
                    .and_then(|d| d.set_settings(wifi_only, smart));
                self.reply(json!(id), done(result));
            }
            Request::DownloadsAllowed {
                id,
                allowed,
                free_bytes,
            } => {
                core.downloads().set_allowed(allowed, free_bytes);
                self.reply(json!(id), Ok::<_, Failure>(json!({})));
            }
            Request::Languages { id, device } => {
                let all = mq_runtime::languages::all(device.as_deref());
                self.reply(json!(id), Ok::<_, Failure>(json!(all)));
            }
            Request::LanguagePicker {
                id,
                device,
                query,
                chosen,
                audio,
            } => {
                let list = mq_runtime::languages::picker(device.as_deref(), &query, &chosen, audio);
                self.reply(json!(id), Ok::<_, Failure>(json!(list)));
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
                self.reply(json!(id), Ok::<_, Failure>(json!(catalogs)))
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
                self.incognito_away(false);
                self.spawn(id, async move {
                    let _ = tokio::time::timeout(TRAKT_FLUSH, core.trakt_background()).await;
                    Ok::<_, Failure>(json!({}))
                });
            }
            // Locks a PIN profile that was away too long, then checks the
            // stream server (it can take a second) and replies `{serverMoved}`.
            Request::Lifecycle { id, event, .. } if event == "foreground" => {
                self.incognito_back();
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
            Request::Lifecycle {
                id,
                event,
                network,
                metered,
                low_data,
            } => {
                let result = match event.as_str() {
                    "memoryPressure" => {
                        core.on_memory_pressure();
                        Ok(json!({}))
                    }
                    // The Incognito clock's input only; outside Incognito
                    // they do nothing, and never act as `background` or
                    // `foreground`.
                    "idle" => {
                        self.incognito_away(true);
                        Ok(json!({}))
                    }
                    "active" => {
                        self.incognito_back();
                        Ok(json!({}))
                    }
                    "networkChanged" => {
                        let known = match network {
                            Some(NetworkWord::Known(n)) => Some(n),
                            _ => None,
                        };
                        let offline = network == Some(NetworkWord::Other(NoNetwork::None));
                        core.downloads().set_link(mq_runtime::downloads::Link {
                            offline,
                            metered: metered
                                .unwrap_or(known == Some(mq_runtime::speed::Network::Cellular)),
                            low_data: low_data.unwrap_or(false),
                        });
                        core.on_network_changed(known);
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
                self.reply(json!(id), Ok::<_, Failure>(json!({})));
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
                        .map_err(Failure::from)
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
                        .map_err(Failure::from)
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
            Request::CreateProfile {
                id,
                name,
                avatar,
                kids,
            } => {
                let result = if kids {
                    core.create_kids_profile(&name, &avatar)
                } else {
                    core.create_profile(&name, &avatar)
                };
                self.reply_profiles(id, result.map(drop));
            }
            Request::AllowTitle {
                id,
                profile,
                kind,
                item_id,
            } => {
                let result = core.allow_title(&profile, &kind, &item_id).await;
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
            Request::SwitchProfile {
                id,
                profile,
                pin,
                from_chooser,
            } => {
                let result = if profile == mq_runtime::incognito::INCOGNITO {
                    self.enter_incognito(from_chooser, pin.as_deref()).await
                } else {
                    self.switch(&profile, pin.as_deref()).await
                };
                send_answer(&self.out, json!(id), result);
            }
            Request::LeaveIncognito { id } => {
                let result = self.leave_incognito();
                send_answer(&self.out, json!(id), Ok(result));
            }
            Request::AckIncognitoEnded { id } => self.reply_profiles(id, Ok(())),
            Request::UnlockProfile { id, pin } => {
                let result = self.unlock(pin.as_deref()).await;
                self.reply_profiles(id, result);
            }
            Request::LockSession { id } => {
                let mut shared = lock(&self.shared);
                // In Incognito: its end, and the chooser over the host.
                let closed = match shared.chooser_after() {
                    Some(host) => shared.end_incognito(true, host, true),
                    None => shared.transition(core, true),
                };
                drop(shared);
                drop(closed);
                self.reply_profiles(id, Ok(()));
            }
            Request::ResetOwnerPin { id } => {
                let result = core.reset_owner_pin();
                self.reply_profiles(id, result);
            }
        }
    }

    fn reply<E: Into<Failure>>(&self, id: Value, result: Result<Value, E>) {
        send_answer(&self.out, id, result.map_err(Into::into));
    }

    /// The picker's snapshot after a profile op, or the op's error.
    fn reply_profiles(&self, id: u64, result: Result<(), ProfileError>) {
        let result = result
            .map_err(Failure::from)
            .map(|()| lock(&self.shared).snapshot());
        send_answer(&self.out, json!(id), result);
    }

    /// Moves the session to `profile` (its PIN when it has one). Refused
    /// while any view but the picker is open: the app closes them first, so
    /// playback has saved its position.
    async fn switch(&mut self, profile: &str, pin: Option<&str>) -> Result<Value, Failure> {
        self.only_the_picker_open()?;
        let target = self.core().for_profile(profile);
        target.check_pin(profile, pin).await?;
        self.commit_switch(target)
    }

    /// A switch or an entry is refused while any view but the picker, the
    /// debrid accounts and the downloads keeper's view is open.
    fn only_the_picker_open(&self) -> Result<(), Failure> {
        let open: Vec<&str> = lock(&self.shared)
            .subs
            .values()
            .map(|s| s.view.name())
            // The device's debrid accounts and the downloads keeper's view
            // stay open across a switch, as `transition` keeps them (code
            // review r2, Apple C1).
            .filter(|name| !matches!(*name, "profiles" | "debrid" | "downloads"))
            .collect();
        if !open.is_empty() {
            return Err(Failure::coded("viewsOpen", "close the open views first")
                .with(json!({ "views": open })));
        }
        Ok(())
    }

    /// Enters Incognito (plans/batch-10-incognito.md 2.7): a new scope
    /// copied from the host, the session's profile when it is unlocked, the
    /// owner otherwise, with the owner's PIN when `incognito_pin` asks for
    /// it. The device's active profile stays. Answers `kids`, `noSpace`,
    /// `incognitoFailed`, `viewsOpen` and the PIN codes; in Incognito
    /// already, the snapshot.
    async fn enter_incognito(
        &mut self,
        from_chooser: bool,
        pin: Option<&str>,
    ) -> Result<Value, Failure> {
        self.only_the_picker_open()?;
        let (core, locked) = {
            let shared = lock(&self.shared);
            if shared.incognito.is_some() {
                return Ok(shared.snapshot());
            }
            (shared.core.clone(), shared.locked)
        };
        if !locked && core.policy().kids() {
            return Err(Failure::coded(
                "kids",
                "a kids profile cannot open Incognito",
            ));
        }
        if let Some(asked) = core.incognito_pin(locked) {
            core.for_profile(&asked).check_pin(&asked, pin).await?;
        }
        let host = if locked {
            core.for_profile(mq_library::OWNER)
        } else {
            core
        };
        let scope = host
            .open_incognito()
            .map_err(|e| Failure::coded(e.code(), &e.to_string()))?;
        let mut shared = lock(&self.shared);
        let generation = shared.generation + 1;
        let timer = tokio::spawn(incognito_clock(shared.me.clone(), generation));
        let now = shared.wall_ms();
        shared.incognito = Some(Incognito {
            host: host.profile().to_owned(),
            to_chooser: from_chooser,
            away_since_ms: None,
            idle: false,
            warned: false,
            last_request_ms: now,
            timer,
        });
        let closed = shared.transition(scope, false);
        let snapshot = shared.snapshot();
        drop(shared);
        drop(closed);
        Ok(snapshot)
    }

    /// Leaves Incognito without a PIN (plans/batch-10-incognito.md 2.7): to
    /// the host, unlocked, or locked when it was entered from the chooser;
    /// the owner, locked, when the host is gone. Outside Incognito (the
    /// clock may end it just before Leave is pressed) the snapshot.
    fn leave_incognito(&mut self) -> Value {
        let mut shared = lock(&self.shared);
        let Some(incognito) = &shared.incognito else {
            return shared.snapshot();
        };
        let device = shared.core.device();
        let (to, locked) = if usable(&device, &incognito.host) {
            (device.for_profile(&incognito.host), incognito.to_chooser)
        } else {
            (device.for_profile(mq_library::OWNER), true)
        };
        let closed = shared.end_incognito(false, to, locked);
        let snapshot = shared.snapshot();
        drop(shared);
        drop(closed);
        snapshot
    }

    /// The Incognito clock: the app is away from now on (`idle` with its
    /// window in view, `background` without).
    fn incognito_away(&mut self, idle: bool) {
        let mut shared = lock(&self.shared);
        let now = shared.wall_ms();
        if let Some(incognito) = shared.incognito.as_mut() {
            // A repeated `idle` never moves the start of the absence.
            incognito.away_since_ms.get_or_insert(now);
            incognito.idle = idle;
        }
    }

    /// The Incognito clock: the app is back (`active`, `foreground`, a
    /// Mac's wake). An absence too long already ended the scope.
    fn incognito_back(&mut self) {
        if let Some(incognito) = lock(&self.shared).incognito.as_mut() {
            incognito.away_since_ms = None;
            incognito.warned = false;
        }
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
        // From Incognito: its end, with nothing to tell.
        let closed = shared.end_incognito(false, target, false);
        let snapshot = shared.snapshot();
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

    fn subscribe(&mut self, view: &str, params: &Value) -> Result<Value, Failure> {
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
                let parsed = versions_params(params, core.prefs().streams)?;
                // `purpose: "download"`: the Versions screen that picks a
                // version to keep (plans/offline-downloads.md 3.2).
                let versions = if params.get("purpose").and_then(Value::as_str) == Some("download")
                {
                    let item = params.get("itemId").and_then(Value::as_str);
                    VersionsView::open_to_download(&core, parsed, item)
                } else {
                    VersionsView::open(&core, parsed)
                };
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
                (
                    View::Pairing { _view: pairing },
                    to_value(&snapshot)?,
                    forward,
                )
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
                    .ok_or_else(|| Failure::from("today must be YYYY-MM-DD".to_owned()))?;
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
            "downloads" => {
                // Locked or on the picker: the keeper's counts alone, so the
                // app keeps downloads running (and its Dock bar and Quit
                // question) with no profile open (code review r1, Apple M2).
                let downloads = if lock(&self.shared).locked {
                    DownloadsView::open_locked(&core)
                } else {
                    DownloadsView::open(&core)
                };
                let (snapshot, updates) = downloads.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (
                    View::Downloads { _view: downloads },
                    to_value(&snapshot)?,
                    forward,
                )
            }
            "playback" => {
                let playback = self.playback(&core, params)?;
                let (snapshot, updates) = playback.subscribe();
                let forward = tokio::spawn(forward(sub, updates, out, fence));
                (View::Playback(playback), to_value(&snapshot)?, forward)
            }
            other => return Err(format!("unknown view: {other}").into()),
        };
        lock(&self.shared)
            .subs
            .insert(sub, Subscription { view, forward });
        Ok(json!({ "sub": sub, "snapshot": snapshot }))
    }

    fn command(&mut self, sub: u64, command: &Value) -> Result<Value, String> {
        let mut shared = lock(&self.shared);
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
            // An absent or null kind lists every kind again.
            (View::Versions(versions), "setKind") => {
                let kind = command.get("kind").cloned().unwrap_or(Value::Null);
                let kind = serde_json::from_value::<Option<mq_streams::SourceKind>>(kind)
                    .map_err(|_| "kind is torrent, debrid, direct or null")?;
                versions.set_kind(kind);
            }
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
            (View::SignIn(sign_in), "returned") => sign_in.returned(&param(command, "fragment")?),
            (View::Calendar(calendar), "setToday") => {
                let today = command
                    .get("today")
                    .and_then(Value::as_str)
                    .and_then(mq_runtime::views::calendar::parse_day)
                    .ok_or("today must be YYYY-MM-DD")?;
                calendar.set_today(today);
            }
            (View::Calendar(calendar), "retry") => calendar.retry(&param(command, "itemId")?),
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
                if !paused && let Some(incognito) = shared.incognito.as_mut() {
                    incognito.away_since_ms = None;
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
            (View::Playback(playback), "secondSubtitleChosen") => {
                let url = command.get("url").and_then(Value::as_str);
                let spoken = command
                    .get("spoken")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                playback.second_subtitle_chosen(url, spoken);
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
    /// The version a download add names: its row of an open Versions view
    /// and that row's failover chain. No learning: a download is no pick.
    fn download_pick(&self, pick: &DownloadPick) -> Result<Vec<mq_streams::ParsedStream>, Failure> {
        let shared = lock(&self.shared);
        let Some(Subscription {
            view: View::Versions(versions),
            ..
        }) = shared.subs.get(&pick.versions_sub)
        else {
            return Err(format!("no versions subscription {}", pick.versions_sub).into());
        };
        let chain = versions.candidates(&pick.key);
        if chain.is_empty() {
            return Err(not_listed());
        }
        Ok(chain)
    }

    fn playback(&self, core: &Core, params: &Value) -> Result<PlaybackView, Failure> {
        let versions_sub = params
            .get("versionsSub")
            .and_then(Value::as_u64)
            .ok_or_else(|| Failure::from("missing parameter: versionsSub".to_owned()))?;
        let shared = lock(&self.shared);
        let Some(Subscription {
            view: View::Versions(versions),
            ..
        }) = shared.subs.get(&versions_sub)
        else {
            return Err(format!("no versions subscription {versions_sub}").into());
        };
        // The pick counts toward learning and hears back when it fails.
        let pick = versions.pick(&param(params, "key")?);
        let plays = versions.plays();
        drop(shared);
        if pick.candidates.is_empty() {
            return Err(not_listed());
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

/// A failed request: its `kind` (what the apps word, from
/// apps/shared/error-words.json; `said` when `error` is already a sentence
/// for the screen), the raw `error` text for the log and, for the errors an
/// app acts on (a dropped device, a fork, a wrong PIN), a stable `code`,
/// with any fields that go with it (`retryAfterMs`, `views`).
struct Failure {
    error: String,
    kind: &'static str,
    code: Option<&'static str>,
    extra: Value,
}

impl Failure {
    fn coded(code: &'static str, error: &str) -> Self {
        Self {
            code: Some(code),
            ..Failure::from(error.to_owned())
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
            kind: "failed",
            code: None,
            extra: Value::Null,
        }
    }
}

impl From<mq_runtime::CoreError> for Failure {
    fn from(e: mq_runtime::CoreError) -> Self {
        let kind = e.failure_kind();
        // A Trakt sentence is shown as it is, without the log's "Trakt: ".
        let error = match e {
            mq_runtime::CoreError::Trakt(said) => said,
            e => e.to_string(),
        };
        Self {
            kind,
            ..Failure::from(error)
        }
    }
}

impl From<mq_sync::SyncError> for Failure {
    fn from(e: mq_sync::SyncError) -> Self {
        let kind = match &e {
            mq_sync::SyncError::Http(h) => h.failure_kind(),
            _ => "failed",
        };
        Self {
            kind,
            code: e.code(),
            ..Failure::from(e.to_string())
        }
    }
}

impl From<ListedError> for Failure {
    fn from(e: ListedError) -> Self {
        match e {
            ListedError::Refused(failure) => Failure {
                kind: "said",
                ..Failure::coded(failure.code(), failure.words())
            },
            ListedError::Core(e) => Failure::from(e),
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
            extra,
            ..Failure::from(e.to_string())
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

/// A version a pick names that its Versions view no longer lists: said to
/// the viewer as it is (the player's card names it).
fn not_listed() -> Failure {
    Failure {
        kind: "said",
        ..Failure::from("This version is no longer listed".to_owned())
    }
}

fn too_large() -> Failure {
    Failure::coded("tooLarge", "the answer is too large to send")
}

fn message(id: Value, result: Result<Value, Failure>) -> Value {
    match result {
        Ok(value) => json!({ "id": id, "ok": true, "result": value }),
        Err(Failure {
            error,
            kind,
            code,
            extra,
        }) => {
            let mut reply = json!({ "id": id, "ok": false, "error": error, "kind": kind });
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
        mq_runtime::CoreError::TraktNotConfigured => Failure {
            kind: "said",
            ..Failure::coded("notConfigured", "Trakt is not set up in this build")
        },
        other => Failure::from(other),
    }
}

fn done(result: Result<(), mq_runtime::CoreError>) -> Result<Value, Failure> {
    result.map(|()| json!({})).map_err(Failure::from)
}

/// A download op on the downloads `which` names; refused for a kids
/// profile.
fn downloads_act(
    core: &Core,
    which: &DownloadIds,
    action: Action,
) -> Result<(), mq_runtime::CoreError> {
    let downloads = mq_runtime::downloads::owner(core)?;
    let ids: Vec<String> = if which.all {
        downloads.ids()
    } else if let Some(item) = &which.item_id {
        downloads.ids_of(item)
    } else {
        which.ids.clone()
    };
    downloads.act(&ids, action)
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
/// entries), and a session running as one is locked, its views closed; in
/// Incognito, a removed host ends it at the owner's chooser. A session
/// started behind a live Incognito scope hears that scope's end.
async fn watch_gone(
    shared: Weak<Mutex<Shared>>,
    core: Core,
    (mut changes, mut ends): (broadcast::Receiver<Vec<Table>>, watch::Receiver<bool>),
    mut known: HashSet<String>,
    out: Output,
) {
    loop {
        tokio::select! {
            change = changes.recv() => match change {
                Ok(tables) if !tables.contains(&Table::Profiles) => continue,
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            },
            changed = ends.changed() => {
                if changed.is_err() {
                    return;
                }
                let ended = *ends.borrow_and_update();
                let Some(shared) = shared.upgrade() else {
                    return;
                };
                let mut shared = lock(&shared);
                if ended && shared.behind && shared.incognito.is_none() {
                    shared.behind = false;
                    shared.ended = true;
                    shared.publish();
                    let _ = out.send(&json!({ "incognito": "ended" }));
                }
                continue;
            }
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
        let host = shared.incognito.as_ref().map(|i| i.host.clone());
        if host.is_some_and(|host| gone.contains(&host)) {
            let owner = shared.core.device().for_profile(mq_library::OWNER);
            closed = shared.end_incognito(true, owner, true);
        } else if !shared.locked && gone.iter().any(|id| id == shared.core.profile()) {
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
fn versions_params(params: &Value, prefs: StreamPrefs) -> Result<VersionsParams, String> {
    VersionsParams::from_json(params, prefs)
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
        // The device's language reaches playback as the core's tag, also when
        // the profile has audio languages of its own (the start subtitle's
        // fallback, plans/onboarding-language-picker.md).
        assert_eq!(parsed.device_language.as_deref(), Some("no"));
        let own = StreamPrefs {
            languages: vec!["en".into()],
            ..StreamPrefs::default()
        };
        let parsed = versions_params(&norway, own).unwrap();
        assert_eq!(
            (parsed.prefs.languages, parsed.device_language.as_deref()),
            (vec!["en".to_owned()], Some("no"))
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
        assert_eq!(caps, mq_streams::DeviceCaps::basic_1080p());

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
            json!({"id": 1, "ok": true, "result": {"liveScopes": 0}})
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
                json!({"id": 7, "ok": false, "code": code, "error": words, "kind": "said"})
            );
        }
    }

    /// The sync flows read a relay's status from the kind (an invite on
    /// start, a used code on approve, a lost token on compare), never the
    /// text; a Trakt sentence goes out without the log's prefix.
    #[test]
    fn sync_and_trakt_failures_carry_their_kind() {
        let http = |status| {
            mq_sync::SyncError::Http(mq_http::HttpError::Status {
                status,
                body: Default::default(),
                retry_after: None,
            })
        };
        for (status, kind) in [(401, "unauthorized"), (403, "forbidden"), (404, "notFound")] {
            let sent = message(json!(1), Err(Failure::from(http(status))));
            assert_eq!(sent["kind"], kind, "{sent}");
        }
        let trakt = mq_runtime::CoreError::Trakt("Trakt did not answer".into());
        let sent = message(json!(2), Err(Failure::from(trakt)));
        assert_eq!(
            (&sent["kind"], &sent["error"]),
            (&json!("said"), &json!("Trakt did not answer"))
        );
    }
}

/// The Incognito clock (plans/batch-10-incognito.md 2.7 and 7.1): the
/// wall clock is read through `skew_ms`, the timer runs under paused tokio
/// time.
#[cfg(test)]
mod incognito_clock {
    use super::*;
    use tokio::sync::mpsc::UnboundedReceiver;

    const MIN: u64 = 60_000;

    /// A device whose owner has an addon list (empty, so no fetch), with
    /// `pin` as the owner's PIN when given.
    async fn device(pin: Option<&str>) -> Core {
        let core = Core::temporary(mq_library::OWNER).unwrap();
        let row = mq_library::AddonRow {
            transport_url: "http://127.0.0.1:9/manifest.json".into(),
            manifest_json: r#"{"id":"x","version":"1.0.0","name":"X","resources":[],"types":[]}"#
                .into(),
            position: 0,
            enabled: true,
            allow_private: true,
        };
        core.library()
            .set_addons(mq_library::OWNER, &[row], core.clock())
            .unwrap();
        core.remove_addon("http://127.0.0.1:9/manifest.json")
            .unwrap();
        if pin.is_some() {
            core.set_pin(mq_library::OWNER, pin, None).await.unwrap();
        }
        core
    }

    /// A relay on loopback and an account on it for `c`: its record.
    async fn account(c: &mut Client) -> Value {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("mq-incognito-clock-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
        c.ok(json!({"op": "createSyncAccount", "server": server, "name": "Mac"}))
            .await
    }

    struct Client {
        session: Session,
        rx: UnboundedReceiver<Outgoing>,
        pushes: Vec<Value>,
        next: u64,
    }

    impl Client {
        fn new(core: Core) -> Self {
            let (tx, rx) = Output::channel();
            Self {
                session: Session::new(core, tx),
                rx,
                pushes: Vec::new(),
                next: 1,
            }
        }

        async fn call(&mut self, mut request: Value) -> Value {
            let id = self.next;
            self.next += 1;
            request["id"] = json!(id);
            self.session.handle_text(&request.to_string()).await;
            loop {
                let message = self.rx.recv().await.unwrap().json();
                if message["id"] == json!(id) {
                    return message;
                }
                self.pushes.push(message);
            }
        }

        async fn ok(&mut self, request: Value) -> Value {
            let reply = self.call(request.clone()).await;
            assert_eq!(reply["ok"], true, "{request} -> {reply}");
            reply["result"].clone()
        }

        async fn event(&mut self, event: &str) -> Value {
            self.ok(json!({"op": "lifecycle", "event": event})).await
        }

        async fn enter(&mut self) {
            let entered = self
                .ok(json!({"op": "switchProfile", "profile": "incognito"}))
                .await;
            assert_eq!(entered["active"], "incognito", "{entered}");
        }

        fn incognito(&self) -> bool {
            lock(&self.session.shared).incognito.is_some()
        }

        /// The wall clock moves on by `ms`; the timer does not tick.
        fn jump(&self, ms: u64) {
            lock(&self.session.shared).skew_ms += ms;
        }

        /// Both clocks move on by `ms`, and the timer gets to run.
        async fn pass(&self, ms: u64) {
            self.jump(ms);
            tokio::time::advance(std::time::Duration::from_millis(ms)).await;
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
        }

        fn drain(&mut self) {
            while let Ok(message) = self.rx.try_recv() {
                self.pushes.push(message.json());
            }
        }

        fn pushed(&mut self, what: &str) -> usize {
            self.drain();
            let push = json!({ "incognito": what });
            self.pushes.iter().filter(|p| **p == push).count()
        }
    }

    /// While the process runs, the timer ends an idle Incognito after 30
    /// minutes, with the warning at 29: the chooser over the host, the
    /// ended push.
    #[tokio::test(start_paused = true)]
    async fn the_timer_warns_at_29_minutes_and_ends_at_30() {
        let mut c = Client::new(device(None).await);
        c.enter().await;
        c.event("idle").await;
        c.pass(28 * MIN).await;
        assert_eq!(c.pushed("endsSoon"), 0);
        c.pass(MIN + 20_000).await;
        assert_eq!(c.pushed("endsSoon"), 1);
        assert!(c.incognito());
        c.pass(MIN).await;
        assert!(!c.incognito());
        assert_eq!(c.pushed("ended"), 1);
        assert_eq!(c.pushed("endsSoon"), 1, "warned once");
        let shared = lock(&c.session.shared);
        assert_eq!((shared.core.profile(), shared.locked), ("default", true));
        assert_eq!(shared.core.live_scope_count(), 0);
    }

    /// `active`, `foreground` and a playing report each end the absence;
    /// `background` is away with no warning (the window is not in view).
    #[tokio::test(start_paused = true)]
    async fn coming_back_cancels_the_end_and_background_gets_no_warning() {
        let mut c = Client::new(device(None).await);
        c.enter().await;
        for back in ["active", "foreground"] {
            c.event("idle").await;
            c.pass(25 * MIN).await;
            c.event(back).await;
            // A person's request: the hour's bound starts again.
            c.ok(json!({"op": "languages"})).await;
            c.pass(25 * MIN).await;
            assert!(c.incognito(), "{back}");
        }
        c.event("background").await;
        c.pass(29 * MIN + 30_000).await;
        assert_eq!(c.pushed("endsSoon"), 0);
        c.event("foreground").await;
        c.ok(json!({"op": "languages"})).await;
        let core = c.session.core();
        let playback = PlaybackView::open(&core, Vec::new(), PlayTarget::default());
        lock(&c.session.shared).subs.insert(
            7,
            Subscription {
                view: View::Playback(playback),
                forward: tokio::spawn(async {}),
            },
        );
        c.event("idle").await;
        for _ in 0..8 {
            c.pass(5 * MIN).await;
            let report = json!({"op": "command", "sub": 7, "command":
                {"type": "progress", "positionMs": 60_000, "durationMs": 888_000}});
            c.ok(report).await;
        }
        assert!(c.incognito(), "40 minutes of playing is no absence");
        assert_eq!(c.pushed("ended"), 0);
    }

    /// A repeated `idle`, or `background` after it, never moves the start
    /// of the absence: the end comes 30 minutes after the first (review
    /// P3-6).
    #[tokio::test]
    async fn a_repeated_idle_keeps_the_start_of_the_absence() {
        let mut c = Client::new(device(None).await);
        c.enter().await;
        c.event("idle").await;
        c.jump(20 * MIN);
        c.event("idle").await;
        c.event("background").await;
        c.jump(11 * MIN);
        c.ok(json!({"op": "ping"})).await;
        assert!(!c.incognito(), "31 minutes after the first idle");
    }

    /// A sleeping Mac or phone stops the timer: the wall clock decides at
    /// the return, before the request is handled. 29 minutes keep it.
    #[tokio::test]
    async fn the_wall_clock_ends_it_at_the_return() {
        for back in ["active", "foreground"] {
            let mut c = Client::new(device(None).await);
            c.enter().await;
            c.event("idle").await;
            c.jump(29 * MIN);
            c.event(back).await;
            assert!(c.incognito(), "{back} after 29 minutes");
            c.event("idle").await;
            c.jump(31 * MIN);
            let reply = c.call(json!({"op": "lifecycle", "event": back})).await;
            assert!(!c.incognito(), "{back} after 31 minutes");
            // The end went out before the reply.
            assert_eq!(c.pushed("ended"), 1, "{reply}");
        }
    }

    /// 60 minutes with no request a person caused end it, though `syncNow`
    /// and `ping` come every minute and the network changes; a request a
    /// person caused after the gap is answered after the end.
    #[tokio::test]
    async fn an_hour_without_a_person_ends_it() {
        let mut c = Client::new(device(None).await);
        let record = account(&mut c).await;
        c.enter().await;
        for minute in 0..61 {
            c.jump(MIN);
            c.ok(json!({"op": "ping"})).await;
            if minute == 30 {
                c.event("networkChanged").await;
            }
            c.ok(json!({"op": "syncNow", "record": record})).await;
        }
        c.jump(MIN);
        let home = c.call(json!({"op": "subscribe", "view": "home"})).await;
        assert_eq!(home["code"], "locked", "{home}");
        assert!(!c.incognito());
        assert_eq!(c.pushed("ended"), 1);
    }

    /// Progress reports every 5 seconds for two hours keep it.
    #[tokio::test]
    async fn playing_for_two_hours_keeps_it() {
        let mut c = Client::new(device(None).await);
        c.enter().await;
        let core = c.session.core();
        let playback = PlaybackView::open(&core, Vec::new(), PlayTarget::default());
        lock(&c.session.shared).subs.insert(
            7,
            Subscription {
                view: View::Playback(playback),
                forward: tokio::spawn(async {}),
            },
        );
        for _ in 0..(2 * 60 * 12) {
            c.jump(5_000);
            let report = json!({"op": "command", "sub": 7, "command":
                {"type": "progress", "positionMs": 1, "durationMs": 9}});
            c.ok(report).await;
        }
        assert!(c.incognito());
    }

    /// Outside Incognito `idle` and `active` do nothing: a PIN profile idle
    /// for 31 minutes is still open, and `idle` never sets the away clock
    /// of the PIN lock (C P2-14).
    #[tokio::test]
    async fn idle_and_active_do_nothing_outside_incognito() {
        let mut c = Client::new(device(Some("1357")).await);
        c.ok(json!({"op": "unlockProfile", "pin": "1357"})).await;
        assert_eq!(c.event("idle").await, json!({}));
        assert!(c.session.background_ms.is_none());
        c.jump(31 * MIN);
        assert_eq!(c.event("active").await, json!({}));
        assert!(!lock(&c.session.shared).locked);
    }

    /// `active` never re-runs the addon refresh `foreground` runs.
    #[tokio::test]
    async fn active_never_refreshes_the_addons() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/manifest.json", listener.local_addr().unwrap());
        let manifest =
            r#"{"id":"x","version":"1.0.0","name":"X","resources":[],"types":["movie"]}"#;
        let fetched = Arc::new(AtomicUsize::new(0));
        let count = fetched.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{manifest}",
                    manifest.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        let core = Core::temporary(mq_library::OWNER).unwrap();
        let row = mq_library::AddonRow {
            transport_url: url,
            manifest_json: manifest.into(),
            position: 0,
            enabled: true,
            allow_private: true,
        };
        core.library()
            .set_addons(mq_library::OWNER, &[row], core.clock())
            .unwrap();
        let mut c = Client::new(core);
        c.enter().await;
        let settle = || tokio::time::sleep(std::time::Duration::from_millis(300));
        settle().await;
        let start = fetched.load(Ordering::SeqCst);
        c.event("active").await;
        settle().await;
        assert_eq!(fetched.load(Ordering::SeqCst), start);
        c.event("foreground").await;
        settle().await;
        assert_eq!(fetched.load(Ordering::SeqCst), start + 1);
    }

    /// The ended notice is session state: automatic requests never clear it,
    /// and each of the profile ops that read it does.
    #[tokio::test]
    async fn the_ended_notice_stays_until_a_person_reads_it() {
        let reads = [
            json!({"op": "switchProfile", "profile": "default"}),
            json!({"op": "unlockProfile"}),
            json!({"op": "leaveIncognito"}),
            json!({"op": "createProfile", "name": "Ada", "avatar": "fox:amber"}),
            json!({"op": "ackIncognitoEnded"}),
        ];
        for read in reads {
            let mut c = Client::new(device(None).await);
            let record = account(&mut c).await;
            c.enter().await;
            c.ok(json!({"op": "lockSession"})).await;
            assert_eq!(c.pushed("ended"), 1);
            c.ok(json!({"op": "syncNow", "record": record})).await;
            c.ok(json!({"op": "ping"})).await;
            c.event("foreground").await;
            let view = c.ok(json!({"op": "subscribe", "view": "profiles"})).await;
            assert_eq!(view["snapshot"]["incognitoEnded"], true, "{view}");
            let _ = c.call(read.clone()).await;
            let after = c.ok(json!({"op": "ackIncognitoEnded"})).await;
            assert_eq!(after["incognitoEnded"], false, "{read}");
        }
    }

    /// Every op is classified (the match has no wildcard); a sample of the
    /// split the bound and the allowlist make.
    #[test]
    fn requests_are_classified() {
        let request = |v: Value| serde_json::from_value::<Request>(v).unwrap();
        let person = [
            json!({"op": "subscribe", "id": 1, "view": "home"}),
            json!({"op": "command", "id": 1, "sub": 1, "command": {"type": "setQuery"}}),
            json!({"op": "command", "id": 1, "sub": 1, "command": {"type": "progress"}}),
            json!({"op": "installAddon", "id": 1, "url": "x"}),
            json!({"op": "leaveIncognito", "id": 1}),
        ];
        let automatic = [
            json!({"op": "ping", "id": 1}),
            json!({"op": "lifecycle", "id": 1, "event": "idle"}),
            json!({"op": "unsubscribe", "id": 1, "sub": 1}),
            json!({"op": "downloadsAllowed", "id": 1, "allowed": true}),
            json!({"op": "command", "id": 1, "sub": 1, "command": {"type": "setToday"}}),
            json!({"op": "command", "id": 1, "sub": 1, "command": {"type": "duration"}}),
            json!({"op": "command", "id": 1, "sub": 1, "command": {"type": "progress", "paused": true}}),
        ];
        for r in person {
            assert!(request(r.clone()).person_driven(), "{r}");
        }
        for r in automatic {
            assert!(!request(r.clone()).person_driven(), "{r}");
        }
    }
}
