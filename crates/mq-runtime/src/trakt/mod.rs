//! Trakt in the runtime (plans/m7-trakt.md, P3 to P5): per profile, the
//! tokens the app hands over, scrobbles from the playback views, pull rounds
//! into the library through the tracker intents, and their views.

mod login;
mod lookups;
mod map;
mod pull;
mod scrobble;
mod search;

pub use login::{TraktLoginSnapshot, TraktLoginView};
pub(crate) use lookups::MIN_VOTES;
pub use lookups::TmdbHost;
pub use pull::TraktReport;
pub use scrobble::TraktPlayback;
pub(crate) use scrobble::{Scrobble, Target as ScrobbleTarget};

use crate::live::{FRAME, coalesce};
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_http::HttpClient;
use mq_library::TrackerKey;
use mq_library::hlc::now_ms;
use mq_tracking::trakt::{Api, Auth, AuthError, Bound, Gate, Hosts, TraktRecord};
use serde::Serialize;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// The tracker's name in the library's tables.
pub(crate) const TRACKER: &str = "trakt";
/// Rounds run this often while the app is in the foreground.
const ROUND_EVERY: Duration = Duration::from_secs(5 * 60);
/// No Trakt call worked for this long while calls were made, or a write
/// waited this long: the status says `failing`.
const FAILING_AFTER_MS: u64 = 30 * 60 * 1000;
/// Shown in the status: the newest unmapped items.
const UNMAPPED_SHOWN: usize = 50;
/// Synced settings: the account the profile uses, and per device the
/// account it signed in to (the profile is off once no device is).
const ACCOUNT_SETTING: &str = "trakt.account";
/// On the owner profile, `sync.member.<device id>`: the account member a
/// device is (synced).
const SYNC_MEMBER: &str = "sync.member.";
const OWNER: &str = mq_library::OWNER;

/// The Trakt app and where Trakt answers: from the build
/// (`mq_tracking::trakt::CLIENT_ID` and `REDIRECT_URI`), or set by tests.
#[derive(Debug, Clone)]
pub struct TraktConfig {
    pub client_id: String,
    pub redirect_uri: String,
    pub hosts: Hosts,
}

impl TraktConfig {
    fn from_build() -> Option<Self> {
        Some(Self {
            client_id: mq_tracking::trakt::CLIENT_ID?.to_owned(),
            redirect_uri: mq_tracking::trakt::REDIRECT_URI?.to_owned(),
            hosts: Hosts::default(),
        })
    }
}

/// The profile's Trakt status (`trakt` view).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct TraktStatus {
    #[serde(flatten)]
    pub state: TraktState,
    pub user: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub last_ok_ms: Option<u64>,
    /// A status and Trakt's `error` field, never a body.
    pub last_error: Option<String>,
    pub pending_writes: u32,
    /// The newest items Trakt could not take or give, newest first.
    pub unmapped: Vec<TraktUnmapped>,
    pub importing: Option<TraktImporting>,
    /// A record the app has not stored: store it, then `setTrakt(record)`.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub record: Option<TraktRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum TraktState {
    Connected,
    /// No Trakt call worked for 30 minutes while calls were made, or a
    /// write waits longer than that.
    #[serde(rename_all = "camelCase")]
    Failing {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        since_ms: u64,
        reason: String,
    },
    /// The tokens are dead: sign in again.
    Reconnect {
        reason: String,
    },
    Off,
    /// The profile uses Trakt, this device is not signed in.
    ThisDeviceOff,
    /// This build has no Trakt app.
    NotConfigured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct TraktUnmapped {
    pub item_id: String,
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct TraktImporting {
    pub done: u32,
    pub total: u32,
}

/// The device's Trakt state, shared by every open of the data directory.
pub(crate) struct Trakt {
    http: HttpClient,
    config: RwLock<Option<TraktConfig>>,
    conns: Mutex<HashMap<String, Arc<Conn>>>,
    /// One pacing gate per Trakt account in the process.
    gates: Mutex<HashMap<String, Arc<Gate>>>,
    foreground: AtomicBool,
    /// Names this process's playback sessions in the outbox: a held write
    /// of another name is from a process that is gone.
    boot: String,
    sessions: AtomicU64,
    /// Title search answers for the search page (`search.rs`).
    search: Mutex<search::Memory>,
    /// The title page's TMDB and Trakt lookups (`lookups.rs`).
    lookups: Mutex<lookups::Memory>,
    /// Cancelled when the device's runtime goes: every worker, scheduler
    /// and sign-in ends, and the tokens leave memory with them.
    shutdown: CancellationToken,
}

impl Drop for Trakt {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

/// One profile's connection.
pub(crate) struct Conn {
    profile: String,
    auth: Auth,
    status: SharedObservable<TraktStatus>,
    health: Mutex<Health>,
    /// One round at a time.
    round: tokio::sync::Mutex<()>,
    /// Held across a round's connection check and the write it guards, and
    /// across a sign-out's generation move: a round's write either lands
    /// before the sign-out drops the account's rows, or not at all.
    fence: Mutex<()>,
    /// Held by the scrobble worker while it sends: the outbox waits for it.
    lane: tokio::sync::Mutex<()>,
    /// Wakes the scheduler.
    wake: Notify,
    /// A record arrived (scrobbles wait up to 5 s for one).
    record_in: Notify,
    events: OnceLock<tokio::sync::mpsc::UnboundedSender<scrobble::Event>>,
    scheduler: AtomicBool,
    /// The running sign-in's ownership: cancelled when a newer sign-in, a
    /// sign-out or the end of the profile overtakes it.
    login: Mutex<Option<CancellationToken>>,
    /// Ends this profile's worker, scheduler and sign-in (a child of the
    /// runtime's `shutdown`).
    stop: CancellationToken,
    /// When the outbox next has a retry due (ms).
    retry_at: Mutex<Option<u64>>,
    created: tokio::time::Instant,
}

#[derive(Default)]
struct Health {
    last_ok_ms: Option<u64>,
    failing_since: Option<u64>,
    last_error: Option<String>,
    importing: Option<TraktImporting>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Trakt {
    pub(crate) fn new(http: HttpClient, library: &mq_library::Library) -> Arc<Self> {
        // Writes held by sessions of a process that is gone (killed while
        // playing) can be sent now.
        if let Err(e) = library.release_orphan_tracker_intents(&[]) {
            tracing::warn!("trakt: held writes not released: {e}");
        }
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("os randomness");
        Arc::new(Self {
            http,
            config: RwLock::new(TraktConfig::from_build()),
            conns: Mutex::default(),
            gates: Mutex::default(),
            foreground: AtomicBool::new(true),
            shutdown: CancellationToken::new(),
            boot: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            sessions: AtomicU64::new(0),
            search: Mutex::default(),
            lookups: Mutex::default(),
        })
    }

    fn config(&self) -> Option<TraktConfig> {
        self.config
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn api(&self) -> Option<(Api, TraktConfig)> {
        let config = self.config()?;
        let agent = format!("Momotus/{}", env!("CARGO_PKG_VERSION"));
        let api = Api::new(
            self.http.clone(),
            config.hosts.clone(),
            &config.client_id,
            &agent,
        );
        Some((api, config))
    }

    fn gate(&self, account: &str) -> Arc<Gate> {
        lock(&self.gates)
            .entry(account.to_owned())
            .or_default()
            .clone()
    }

    fn conn(&self, profile: &str) -> Arc<Conn> {
        let redirect = self.config().map(|c| c.redirect_uri).unwrap_or_default();
        lock(&self.conns)
            .entry(profile.to_owned())
            .or_insert_with(|| {
                Arc::new(Conn {
                    profile: profile.to_owned(),
                    auth: Auth::new(&redirect),
                    status: SharedObservable::new(TraktStatus {
                        state: TraktState::Off,
                        user: None,
                        last_ok_ms: None,
                        last_error: None,
                        pending_writes: 0,
                        unmapped: Vec::new(),
                        importing: None,
                        record: None,
                    }),
                    health: Mutex::default(),
                    round: tokio::sync::Mutex::new(()),
                    fence: Mutex::default(),
                    lane: tokio::sync::Mutex::new(()),
                    wake: Notify::new(),
                    record_in: Notify::new(),
                    events: OnceLock::new(),
                    scheduler: AtomicBool::new(false),
                    login: Mutex::new(None),
                    retry_at: Mutex::new(None),
                    stop: self.shutdown.child_token(),
                    created: tokio::time::Instant::now(),
                })
            })
            .clone()
    }

    /// The profile is gone: its work stops and its tokens are dropped.
    pub(crate) fn forget(&self, profile: &str) {
        if let Some(conn) = lock(&self.conns).remove(profile) {
            conn.stop.cancel();
            if let Some(login) = lock(&conn.login).take() {
                login.cancel();
            }
            let _ = futures_util::FutureExt::now_or_never(conn.auth.set(None));
        }
    }

    fn conns(&self) -> Vec<Arc<Conn>> {
        lock(&self.conns).values().cloned().collect()
    }
}

/// A connection as work started under it: the account's rows that work
/// writes land only while it is still the connection in use.
#[derive(Clone, Copy)]
pub(super) struct Bind<'a> {
    pub conn: &'a Conn,
    pub bound: &'a Bound,
}

impl<'a> Bind<'a> {
    pub(super) fn key(self) -> TrackerKey<'a> {
        self.conn.key(&self.bound.account)
    }

    /// Whether this is still the connection in use, for a profile that is
    /// not kids (one turned kids while work ran writes nothing more). It
    /// stays so while the guard is held: a sign-out takes the same lock to
    /// move the generation.
    pub(super) fn fence(self, core: &Core) -> Option<MutexGuard<'a, ()>> {
        let fence = lock(&self.conn.fence);
        let now = futures_util::FutureExt::now_or_never(self.conn.auth.bound()).flatten();
        (now.as_ref() == Some(self.bound) && !kids(core)).then_some(fence)
    }

    /// Writes one of the account's state rows, behind the fence.
    pub(super) fn set_state(self, core: &Core, name: &str, value: &str) {
        if let Some(_fence) = self.fence(core)
            && let Err(e) = core.library().set_tracker_state(self.key(), name, value)
        {
            tracing::debug!("trakt: state not written: {e}");
        }
    }

    /// Deletes one of the account's state rows, behind the fence.
    pub(super) fn delete_state(self, core: &Core, name: &str) {
        if let Some(_fence) = self.fence(core) {
            let _ = core.library().delete_tracker_state(self.key(), name);
        }
    }
}

impl Conn {
    fn key<'a>(&'a self, account: &'a str) -> TrackerKey<'a> {
        TrackerKey {
            profile: &self.profile,
            tracker: TRACKER,
            account,
        }
    }

    fn ok(&self) {
        let mut h = lock(&self.health);
        h.last_ok_ms = Some(now_ms());
        h.failing_since = None;
        h.last_error = None;
    }

    fn failed(&self, reason: &str) {
        let mut h = lock(&self.health);
        h.failing_since.get_or_insert_with(now_ms);
        h.last_error = Some(reason.to_owned());
    }

    fn set_importing(&self, importing: Option<TraktImporting>) {
        lock(&self.health).importing = importing;
    }
}

/// An error's words for the status: a status and Trakt's `error` field, or
/// the transport's reason. Never a body.
fn reason(e: &mq_tracking::trakt::CallError) -> String {
    match e {
        mq_tracking::trakt::CallError::Refused {
            status,
            error: Some(error),
        } => format!("status {status} ({error})"),
        other => other.to_string(),
    }
}

impl Core {
    pub(crate) fn trakt(&self) -> Arc<Trakt> {
        self.trakt_service().clone()
    }

    fn trakt_conn(&self) -> Arc<Conn> {
        self.trakt().conn(self.profile())
    }

    /// Points Trakt at another app and host (tests), or turns it off.
    #[doc(hidden)]
    pub fn set_trakt_config(&self, config: Option<TraktConfig>) {
        *self
            .trakt()
            .config
            .write()
            .unwrap_or_else(|p| p.into_inner()) = config;
    }

    /// The record the app keeps for this profile (at start, after `open`,
    /// and as the acknowledgement of a record the `trakt` view offered), or
    /// `None`: this device holds no token for the profile.
    pub async fn set_trakt(&self, record: Option<TraktRecord>) -> Result<(), CoreError> {
        let trakt = self.trakt();
        if record.is_some() && trakt.config().is_none() {
            return Err(CoreError::TraktNotConfigured);
        }
        if record.is_some() && kids(self) {
            return Err(CoreError::Trakt(KIDS.into()));
        }
        let conn = self.trakt_conn();
        let account = record.as_ref().map(|r| (r.account.clone(), r.generation));
        if conn.auth.set(record).await
            && let Some((account, generation)) = account
        {
            self.trakt_connected(&conn, &account, generation)?;
        }
        self.trakt_refresh(&conn);
        Ok(())
    }

    /// A record is held for `account`: the library knows the account (work
    /// of another account is dropped), the synced settings say this device
    /// is signed in, scrobbles waiting for a record go on, and rounds run.
    fn trakt_connected(
        &self,
        conn: &Arc<Conn>,
        account: &str,
        generation: u64,
    ) -> Result<(), CoreError> {
        match kids_now(self) {
            Some(false) => {}
            // Turned kids while a sign-in ran: its tokens go too.
            Some(true) => {
                self.trakt().forget(self.profile());
                return Err(CoreError::Trakt(KIDS.into()));
            }
            // Not read: not connected now, the tokens kept; the app's next
            // record (at start, after `open`) connects it.
            None => return Err(CoreError::Trakt(READ_AGAIN.into())),
        }
        let library = self.library();
        let profile = self.profile();
        let known = library.tracker_account(profile, TRACKER)?;
        if known
            .as_ref()
            .is_none_or(|(a, g)| a != account || *g != generation as i64)
        {
            library.set_tracker_account(
                profile,
                TRACKER,
                Some(account),
                generation as i64,
                now_ms() as i64,
            )?;
        }
        let value = serde_json::to_string(account).unwrap_or_default();
        for key in [ACCOUNT_SETTING.to_owned(), self.trakt_device_setting()] {
            if setting(self, &key).as_deref() != Some(account) {
                library.set_setting(profile, &key, &value, self.clock())?;
            }
        }
        conn.record_in.notify_waiters();
        self.trakt_schedule(conn);
        Ok(())
    }

    fn trakt_device_setting(&self) -> String {
        format!("trakt.device.{}", self.clock().device())
    }

    /// Before a round: names the account member this device is, in a synced
    /// setting on the owner profile, so the other devices can tell its
    /// `trakt.device.<id>` flag apart once it left the account. The setting
    /// stays after a device leaves: it then only says who it was.
    pub(crate) fn note_sync_member(&self, config: &mq_sync::SyncConfig) {
        let library = self.library();
        let key = format!("{SYNC_MEMBER}{}", self.clock().device());
        let me = serde_json::to_string(&config.member_hex()).unwrap_or_default();
        let held = library
            .settings(OWNER)
            .ok()
            .and_then(|all| all.into_iter().find_map(|(k, v)| (k == key).then_some(v)));
        if held.as_deref() != Some(me.as_str())
            && let Err(e) = library.set_setting(OWNER, &key, &me, self.clock())
        {
            tracing::warn!("could not name this device's account member: {e}");
        }
    }

    /// Whether another device's `trakt.device.<id>` flag still counts as
    /// signed in: not when that device's member is missing from the roster
    /// this device holds now, since a device that left can never sync its
    /// sign-out (B P3-3). Outside an account, or without the member's name,
    /// it counts.
    fn trakt_flag_counts(&self) -> Result<impl Fn(&str) -> bool, CoreError> {
        let library = self.library();
        // The roster store is caught up before any record is pulled, so a
        // device whose `sync.member` arrived is already in it.
        let devices = mq_sync::stored_devices(library).unwrap_or_else(|e| {
            tracing::warn!("could not read the account's devices: {e}");
            None
        });
        let members: HashMap<String, String> = library
            .settings(OWNER)?
            .into_iter()
            .filter_map(|(k, v)| {
                let device = k.strip_prefix(SYNC_MEMBER)?.to_owned();
                Some((device, serde_json::from_str::<String>(&v).ok()?))
            })
            .collect();
        Ok(move |device: &str| match (&devices, members.get(device)) {
            (Some(devices), Some(member)) => devices.contains(member),
            _ => true,
        })
    }

    /// Whether this device is still signed in to Trakt for any profile (its
    /// `trakt.device.<id>` setting). A wipe waits until each is signed out
    /// (D13): no Trakt record outlives the profiles it belonged to.
    pub fn trakt_held(&self) -> Result<bool, CoreError> {
        let library = self.library();
        let key = self.trakt_device_setting();
        for profile in library.profiles()? {
            let settings = library.settings(&profile.id)?;
            if settings.into_iter().any(|(k, v)| k == key && v != "null") {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// One round now ("Sync now").
    pub async fn trakt_sync(&self) -> Result<TraktReport, CoreError> {
        let trakt = self.trakt();
        if trakt.config().is_none() {
            return Err(CoreError::TraktNotConfigured);
        }
        if kids(self) {
            return Err(CoreError::Trakt(KIDS.into()));
        }
        let conn = self.trakt_conn();
        let report = pull::round(self, &trakt, &conn).await;
        self.trakt_refresh(&conn);
        report.map_err(|e| CoreError::Trakt(reason(&e)))
    }

    /// Signs this device out of Trakt for the profile: the token is revoked
    /// (best effort, in the background), this account's pending writes and
    /// cursors are dropped, the library keeps what it has. The profile
    /// stops using Trakt when no other device is signed in. The app deletes
    /// its record. Nothing here waits for the network.
    pub async fn trakt_sign_out(&self) -> Result<(), CoreError> {
        let trakt = self.trakt();
        let conn = self.trakt_conn();
        let token = conn.auth.held_token().await;
        // A sign-in still running loses its claim.
        if let Some(login) = lock(&conn.login).take() {
            login.cancel();
        }
        // The generation moves under the round's fence. A round in flight
        // is not waited for: its calls answer `Off`, and a write of it
        // lands before the rows below are dropped, or not at all.
        {
            let _fence = lock(&conn.fence);
            let _ = futures_util::FutureExt::now_or_never(conn.auth.set(None));
        }
        if let (Some(token), Some((api, _))) = (token, trakt.api()) {
            tokio::spawn(async move { mq_tracking::trakt::auth::revoke(&api, &token).await });
        }
        let (library, profile) = (self.library(), self.profile());
        library.set_tracker_account(profile, TRACKER, None, 0, now_ms() as i64)?;
        library.set_setting(profile, &self.trakt_device_setting(), "null", self.clock())?;
        let counts = self.trakt_flag_counts()?;
        let others = library
            .settings(profile)?
            .into_iter()
            .any(|(k, v)| v != "null" && k.strip_prefix("trakt.device.").is_some_and(&counts));
        if !others && setting(self, ACCOUNT_SETTING).is_some() {
            library.set_setting(profile, ACCOUNT_SETTING, "null", self.clock())?;
        }
        self.trakt_refresh(&conn);
        Ok(())
    }

    /// The app went to the background: running scrobbles become pauses (a
    /// picture in picture may play on). Returns once they are sent.
    pub async fn trakt_background(&self) {
        let trakt = self.trakt();
        trakt.foreground.store(false, Ordering::SeqCst);
        let mut flushed = Vec::new();
        for conn in trakt.conns() {
            if let Some(events) = conn.events.get() {
                let (done, wait) = tokio::sync::oneshot::channel();
                if events.send(scrobble::Event::Background(done)).is_ok() {
                    flushed.push(wait);
                }
            }
        }
        for wait in flushed {
            let _ = wait.await;
        }
    }

    /// The app is in the foreground again: rounds resume.
    pub fn trakt_foreground(&self) {
        let trakt = self.trakt();
        trakt.foreground.store(true, Ordering::SeqCst);
        for conn in trakt.conns() {
            conn.wake.notify_one();
        }
    }

    /// Starts the profile's round scheduler, or wakes it. A kids profile
    /// never gets here with a record (`set_trakt` and scrobbles refuse it);
    /// one turned kids later is stopped by the loop's own check.
    fn trakt_schedule(&self, conn: &Arc<Conn>) {
        if !conn.scheduler.swap(true, Ordering::SeqCst) {
            tokio::spawn(schedule(self.downgrade(), conn.clone()));
        }
        conn.wake.notify_one();
    }

    /// Recomputes the profile's status.
    fn trakt_refresh(&self, conn: &Conn) {
        let status = self.trakt_status(conn);
        conn.status.set_if_not_eq(status);
    }

    fn trakt_status(&self, conn: &Conn) -> TraktStatus {
        let trakt = self.trakt();
        let health = {
            let h = lock(&conn.health);
            (
                h.last_ok_ms,
                h.failing_since,
                h.last_error.clone(),
                h.importing,
            )
        };
        let (last_ok_ms, failing_since, last_error, importing) = health;
        // The auth calls are async only by signature: they never wait.
        let (account, user, record, auth) = futures_util::FutureExt::now_or_never(async {
            (
                conn.auth.account().await,
                conn.auth.user().await,
                conn.auth.unacked().await,
                conn.auth.status().await,
            )
        })
        .unwrap_or((None, None, None, Err(AuthError::Off)));
        let mut status = TraktStatus {
            state: TraktState::Off,
            user,
            last_ok_ms,
            last_error: last_error.clone(),
            pending_writes: 0,
            unmapped: Vec::new(),
            importing,
            record,
        };
        if let Some(account) = &account {
            let key = conn.key(account);
            let (pending, oldest) = self.library().tracker_pending(key).unwrap_or((0, None));
            status.pending_writes = pending as u32;
            status.unmapped = unmapped(self, key);
            let now = now_ms();
            let stale_write =
                oldest.is_some_and(|at| now.saturating_sub(at as u64) > FAILING_AFTER_MS);
            let stale_calls =
                failing_since.is_some_and(|since| now.saturating_sub(since) > FAILING_AFTER_MS);
            status.state = match auth {
                Err(AuthError::Reconnect(reason)) => TraktState::Reconnect { reason },
                _ if stale_calls || stale_write => TraktState::Failing {
                    since_ms: failing_since.or(oldest.map(|o| o as u64)).unwrap_or(now),
                    reason: last_error.unwrap_or_else(|| "writes are waiting".into()),
                },
                _ => TraktState::Connected,
            };
        } else if setting(self, ACCOUNT_SETTING).is_some() {
            status.state = TraktState::ThisDeviceOff;
        }
        if trakt.config().is_none() {
            status.state = TraktState::NotConfigured;
        }
        status
    }
}

/// A synced setting's string value, `None` when unset or null.
fn setting(core: &Core, key: &str) -> Option<String> {
    core.library()
        .settings(core.profile())
        .ok()?
        .into_iter()
        .rev()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| serde_json::from_str::<Option<String>>(&v).ok().flatten())
}

fn unmapped(core: &Core, key: TrackerKey<'_>) -> Vec<TraktUnmapped> {
    let mut rows: Vec<(u64, TraktUnmapped)> = core
        .library()
        .tracker_states(key, map::UNMAPPED)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| {
            let v: serde_json::Value = serde_json::from_str(&v).ok()?;
            Some((
                v["at"].as_u64().unwrap_or(0),
                TraktUnmapped {
                    item_id: k.strip_prefix(map::UNMAPPED)?.to_owned(),
                    name: v["name"].as_str().unwrap_or_default().to_owned(),
                    reason: v["reason"].as_str().unwrap_or_default().to_owned(),
                },
            ))
        })
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    rows.into_iter()
        .take(UNMAPPED_SHOWN)
        .map(|(_, u)| u)
        .collect()
}

/// Kids profiles have no Trakt (plans/m11-kids.md 4.5): no record, no
/// sign-in, no round, no scheduler, no scrobble, no call and no write. A
/// profile whose access is not known, or cannot be read, counts as kids
/// here (the policy fails closed): this refuses, it never drops tokens.
pub(super) fn kids(core: &Core) -> bool {
    core.library().policy(core.profile()).kids()
}

/// Whether the profile reads as kids, `None` when that could not be read:
/// what drops a profile's tokens asks this, and asks again later on `None`.
pub(super) fn kids_now(core: &Core) -> Option<bool> {
    match core.library().policy_checked(core.profile()) {
        Ok((policy, _)) => Some(policy.kids()),
        Err(e) => {
            tracing::debug!("trakt: profile check failed: {e}");
            None
        }
    }
}

/// Ends once an `Access` change (or a missed one) leaves the profile kids.
async fn turned_kids(
    core: &crate::core::WeakCore,
    changes: &mut tokio::sync::broadcast::Receiver<Vec<mq_library::Table>>,
) {
    while let Some(tables) = crate::views::next_change(changes).await {
        if tables.contains(&mq_library::Table::Access)
            && core.upgrade().is_some_and(|c| kids_now(&c) == Some(true))
        {
            return;
        }
    }
    std::future::pending().await
}

pub(super) const KIDS: &str = "Trakt is off for kids profiles";

/// A profile check that failed: refused for now, nothing dropped.
const READ_AGAIN: &str = "the profile could not be read; try again";

/// Whether the library says the profile was deleted, here or by sync. A
/// read that fails is not a deletion: the next look asks again.
fn gone(core: &Core) -> bool {
    core.library().is_gone(core.profile()).unwrap_or_else(|e| {
        tracing::debug!("trakt: profile check failed: {e}");
        false
    })
}

/// Rounds for one profile: when woken (a record, the foreground, a closed
/// playback, a sign-in), every 5 minutes in the foreground, and when an
/// outbox retry is due while the sign-in is usable. It holds the core
/// weakly: once every handle is gone, or the profile is, it ends. A profile
/// that turns kids (an `Access` change) ends it at once, a round in flight
/// included, and its tokens leave memory.
async fn schedule(weak: crate::core::WeakCore, conn: Arc<Conn>) {
    let Some(mut changes) = weak.upgrade().map(|c| c.library().subscribe()) else {
        return;
    };
    loop {
        let Some(trakt) = weak.upgrade().map(|c| c.trakt()) else {
            return;
        };
        // A retry cannot go while the sign-in is unusable: a new record
        // wakes this, else the round interval does. Never a due time that
        // would come back at once, round after round.
        let usable = conn.auth.status().await.is_ok();
        let retry = lock(&conn.retry_at)
            .filter(|_| usable)
            .map(|at| Duration::from_millis(at.saturating_sub(now_ms())));
        let every = trakt
            .foreground
            .load(Ordering::SeqCst)
            .then_some(ROUND_EVERY);
        let wait = match (retry, every) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => Duration::from_secs(24 * 60 * 60),
        };
        drop(trakt);
        tokio::select! {
            () = conn.stop.cancelled() => return,
            () = conn.wake.notified() => {}
            () = tokio::time::sleep(wait) => {}
            () = turned_kids(&weak, &mut changes) => {}
        }
        let Some(core) = weak.upgrade() else { return };
        let trakt = core.trakt();
        if gone(&core) {
            trakt.forget(core.profile());
            return;
        }
        match kids_now(&core) {
            // A record held from before (never taken for kids), or the
            // profile turned kids: its work stops, its tokens leave memory.
            Some(true) => {
                trakt.forget(core.profile());
                return;
            }
            Some(false) => {}
            // Not read: asked again at the next wake, the tokens kept.
            None => continue,
        }
        if conn.auth.account().await.is_none() || trakt.config().is_none() {
            continue;
        }
        tokio::select! {
            result = pull::round(&core, &trakt, &conn) => if let Err(e) = result {
                tracing::debug!("trakt round: {}", reason(&e));
            },
            () = turned_kids(&weak, &mut changes) => {
                trakt.forget(core.profile());
                return;
            }
        }
        core.trakt_refresh(&conn);
    }
}

/// The profile's Trakt status.
pub struct TraktView {
    snapshot: SharedObservable<TraktStatus>,
}

impl TraktView {
    pub fn open(core: &Core) -> Self {
        let conn = core.trakt_conn();
        core.trakt_refresh(&conn);
        Self {
            snapshot: conn.status.clone(),
        }
    }

    pub fn snapshot(&self) -> TraktStatus {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        TraktStatus,
        Pin<Box<dyn Stream<Item = Vec<TraktStatus>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokens adopted for a profile that turned kids meanwhile (between a
    /// sign-in's own check and its write) are not recorded, start no
    /// rounds, and leave memory.
    #[tokio::test]
    async fn a_connection_for_a_kids_profile_is_refused() {
        let core = Core::temporary("p").unwrap();
        let access = mq_library::SyncRecord {
            coll: mq_library::coll::PROFILE_ACCESS,
            key: "p".into(),
            value: serde_json::json!({"profile": "p", "kids": true}),
            hlc: core.hlc(),
        };
        core.library().apply(&[access]).unwrap();
        let conn = core.trakt_conn();
        assert!(core.trakt_connected(&conn, "acct", 1).is_err());
        assert_eq!(core.library().tracker_account("p", TRACKER).unwrap(), None);
        assert!(!conn.scheduler.load(Ordering::SeqCst), "no rounds");
        assert!(conn.stop.is_cancelled(), "forgotten");
    }

    /// A profile that cannot be read is not connected now, and its tokens
    /// stay: a read error is never a turn to kids.
    #[tokio::test]
    async fn a_connection_whose_profile_cannot_be_read_keeps_its_tokens() {
        let dir = std::env::temp_dir().join(format!("mq-connect-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let core = Core::open(crate::CoreConfig {
            data_dir: dir.clone(),
            profile_id: mq_library::OWNER.into(),
            device_id: 0,
        })
        .unwrap();
        rusqlite::Connection::open(dir.join("library.sqlite"))
            .unwrap()
            .execute(
                "UPDATE profile_access SET kids = 'unreadable' WHERE profile_id = 'default'",
                [],
            )
            .unwrap();
        let conn = core.trakt_conn();
        assert!(core.trakt_connected(&conn, "acct", 1).is_err());
        assert!(!conn.stop.is_cancelled(), "kept");
        drop(core);
        let _ = std::fs::remove_dir_all(dir);
    }
}
