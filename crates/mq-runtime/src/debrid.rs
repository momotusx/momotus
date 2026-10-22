//! Debrid accounts (plans/c10-handoff.md part A). Each service has at most
//! one connection, key or OAuth, with a `generation` (new per connection)
//! and a `revision` (one more per token rotation). The app's keystore holds
//! the records; the core keeps them in memory, refreshes Real-Debrid tokens,
//! and offers every new or rotated record through the `debrid` view until
//! the app acknowledges it. Sign-in by code runs in a core-owned task.
//!
//! Every service has a mutation epoch: a replacement, a disconnect and a new
//! sign-in bump it, and a sign-in that finishes under an older epoch is
//! discarded. Device codes, keys, tokens and secrets are never logged.

use crate::live::{FRAME, coalesce};
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_debrid::{
    AllDebrid, Auth, BoxFuture, Credential, Credentials, DebridError, Login, LoginPoll, Premiumize,
    RealDebrid, Resolver, ResolverConfig, StoreId, TorBox,
};
use mq_http::{HttpClient, HttpError};
use mq_library::hlc::now_ms;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, Weak};
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const MINUTE_MS: u64 = 60_000;
const YEAR_MS: u64 = 365 * 24 * 60 * MINUTE_MS;
/// A refresh that could not reach the service is tried again after this.
const RETRY: Duration = Duration::from_secs(60);
const SLOW_DOWN: Duration = Duration::from_secs(5);

/// One account connection, as the app's keystore stores it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct DebridRecord {
    /// "realdebrid", "alldebrid", "torbox" or "premiumize".
    pub service: String,
    /// 32 hex characters, new for every connection. A string, so JavaScript
    /// keeps every bit.
    pub generation: String,
    /// 1 for a new connection, one more for each token rotation.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub revision: u64,
    pub credential: DebridCredential,
}

impl fmt::Debug for DebridRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DebridRecord")
            .field("service", &self.service)
            .field("generation", &self.generation)
            .field("revision", &self.revision)
            .field("credential", &self.credential)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DebridCredential {
    Key {
        key: String,
    },
    #[serde(rename_all = "camelCase")]
    Oauth {
        client_id: String,
        client_secret: String,
        refresh_token: String,
        access_token: String,
        /// Wall clock when the access token was issued, unix milliseconds.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        issued_at_ms: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        expires_in_ms: u64,
    },
}

impl fmt::Debug for DebridCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DebridCredential::Key { .. } => f.write_str("Key(<redacted>)"),
            DebridCredential::Oauth {
                issued_at_ms,
                expires_in_ms,
                ..
            } => f
                .debug_struct("Oauth")
                .field("issued_at_ms", issued_at_ms)
                .field("expires_in_ms", expires_in_ms)
                .finish_non_exhaustive(),
        }
    }
}

/// Written by the app before it asks to disconnect: a record with this
/// generation is never restored.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct DebridTombstone {
    pub service: String,
    pub generation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum AccountState {
    Ok,
    /// The last refresh could not reach the service; it is retried.
    Failing,
    /// The service refused the key or the refresh: sign in again.
    Reconnect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct DebridAccount {
    pub service: String,
    /// What `disconnectDebrid` names.
    pub generation: String,
    pub state: AccountState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct DebridSnapshot {
    /// Connected services, the default store first.
    pub accounts: Vec<DebridAccount>,
    /// Per service, the newest record the app has not acknowledged: store
    /// it, then `ackDebrid`.
    pub pending: Vec<DebridRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum DebridLoginSnapshot {
    Starting,
    /// Show `userCode` and `url`, and `qr` as a QR code, until `expiresMs`.
    #[serde(rename_all = "camelCase")]
    Code {
        user_code: String,
        url: String,
        qr: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        expires_ms: u64,
    },
    /// Connected; the record waits in the `debrid` view.
    Done,
    Expired,
    Denied,
    /// The code was already used or overtaken: start again.
    Used,
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct RestoreAnswer {
    /// Generations not restored because they have a tombstone.
    pub refused: Vec<String>,
    /// Tombstones whose record is gone from the keystore: delete them.
    pub tombstones: Vec<DebridTombstone>,
}

/// The connected streaming accounts as they move (`Core::account_changes`):
/// (service, generation) in order.
pub(crate) type AccountChanges = tokio::sync::watch::Receiver<Vec<(StoreId, String)>>;

/// The device's debrid accounts, shared by every profile.
pub(crate) struct Accounts {
    me: Weak<Accounts>,
    http: HttpClient,
    /// Tests point every service at one local server.
    origin: RwLock<Option<String>>,
    state: Mutex<State>,
    snapshot: SharedObservable<DebridSnapshot>,
    /// Holds the cache and flights every rebuilt resolver shares.
    base: Resolver,
    resolver: RwLock<Option<Resolver>>,
    /// The connected accounts, as (service, generation) in order: moves
    /// only when one is added, replaced or removed, for views that rank by
    /// them (`Core::account_changes`).
    set: tokio::sync::watch::Sender<Vec<(StoreId, String)>>,
}

#[derive(Default)]
struct State {
    /// Provider order, the default store first.
    order: Vec<StoreId>,
    conns: HashMap<StoreId, Arc<Connection>>,
    epochs: HashMap<StoreId, u64>,
    pending: HashMap<StoreId, DebridRecord>,
    /// Generations disconnected or tombstoned while running.
    gone: HashSet<String>,
    logins: HashMap<StoreId, CancellationToken>,
    /// One sign-in request in flight per service, across sign-ins.
    collecting: HashMap<StoreId, Arc<tokio::sync::Mutex<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

fn bump(state: &mut State, service: StoreId) -> u64 {
    let epoch = state.epochs.entry(service).or_default();
    *epoch += 1;
    *epoch
}

fn service(name: &str) -> Result<StoreId, CoreError> {
    StoreId::from_service(name).ok_or_else(|| CoreError::Debrid(format!("unknown service {name}")))
}

fn new_generation() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("os randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn new_record(service: StoreId, credential: DebridCredential) -> DebridRecord {
    DebridRecord {
        service: service.name().to_owned(),
        generation: new_generation(),
        revision: 1,
        credential,
    }
}

fn oauth(credentials: Credentials, issued_at_ms: u64) -> DebridCredential {
    DebridCredential::Oauth {
        client_id: credentials.client_id,
        client_secret: credentials.client_secret,
        refresh_token: credentials.refresh_token,
        access_token: credentials.access_token,
        issued_at_ms,
        expires_in_ms: credentials.expires_in.saturating_mul(1000),
    }
}

/// How long until an OAuth token is refreshed: a quarter of its lifetime
/// before it expires. A token issued in the future (the clock went back) or
/// with a lifetime outside 1 minute to 1 year counts as expired.
pub(crate) fn refresh_wait(issued_at_ms: u64, expires_in_ms: u64, now_ms: u64) -> Duration {
    if issued_at_ms > now_ms || !(MINUTE_MS..=YEAR_MS).contains(&expires_in_ms) {
        return Duration::ZERO;
    }
    match issued_at_ms.checked_add(expires_in_ms - expires_in_ms / 4) {
        Some(due) => Duration::from_millis(due.saturating_sub(now_ms)),
        None => Duration::ZERO,
    }
}

pub(crate) fn slow_down(interval: Duration) -> Duration {
    interval.saturating_add(SLOW_DOWN)
}

/// What a log says about an error: its kind, never a provider's own text.
fn kind(e: &DebridError) -> &'static str {
    match e {
        DebridError::Auth => "refused",
        DebridError::NotPremium => "not premium",
        DebridError::NotCached => "not cached",
        DebridError::RateLimited { .. } => "rate limited",
        DebridError::Quota => "over quota",
        DebridError::Provider { .. } => "provider error",
        DebridError::Http(_) => "network",
        DebridError::Timeout => "timed out",
        DebridError::NoPlayableFile => "no playable file",
    }
}

/// Network trouble, a busy service or a server fault: worth another try.
fn transient(e: &DebridError) -> bool {
    match e {
        DebridError::Http(_) | DebridError::Timeout | DebridError::RateLimited { .. } => true,
        DebridError::Provider { code, .. } => code.starts_with('5'),
        _ => false,
    }
}

/// One live connection: the credential every store of it reads per request.
struct Connection {
    service: StoreId,
    generation: String,
    record: Mutex<DebridRecord>,
    health: Mutex<AccountState>,
    /// When the OAuth token is refreshed, on the monotonic clock.
    due: Mutex<Instant>,
    refreshing: tokio::sync::Mutex<()>,
    /// Wakes the refresh worker when `due` moves.
    wake: tokio::sync::Notify,
    /// Cancelled when the connection is replaced or disconnected.
    gone: CancellationToken,
    accounts: Weak<Accounts>,
}

impl Connection {
    fn health(&self) -> AccountState {
        *lock(&self.health)
    }

    fn set_health(&self, health: AccountState) {
        *lock(&self.health) = health;
        if let Some(accounts) = self.accounts.upgrade() {
            accounts.publish(&accounts.lock());
        }
    }

    fn reschedule(&self) {
        let wait = match &lock(&self.record).credential {
            DebridCredential::Oauth {
                issued_at_ms,
                expires_in_ms,
                ..
            } => refresh_wait(*issued_at_ms, *expires_in_ms, now_ms()),
            DebridCredential::Key { .. } => Duration::MAX,
        };
        self.set_due(
            Instant::now()
                .checked_add(wait)
                .unwrap_or_else(|| Instant::now() + YEAR),
        );
    }

    fn set_due(&self, due: Instant) {
        *lock(&self.due) = due;
        self.wake.notify_one();
    }

    /// Trades the refresh token for a new access token, once at a time.
    /// `used` is the token the caller saw; if it changed while waiting, the
    /// current one is the answer and nothing is sent.
    async fn refresh(&self, used: &str) -> Option<String> {
        let _single = self.refreshing.lock().await;
        let current = self.token();
        if current != used {
            return Some(current);
        }
        let accounts = self.accounts.upgrade()?;
        let credentials = match lock(&self.record).credential.clone() {
            DebridCredential::Oauth {
                client_id,
                client_secret,
                refresh_token,
                access_token,
                ..
            } => Credentials {
                client_id,
                client_secret,
                access_token,
                refresh_token,
                expires_in: 0,
            },
            DebridCredential::Key { .. } => return None,
        };
        let work = async {
            match accounts.origin() {
                Some(origin) => {
                    let base = format!("{origin}/oauth/v2");
                    credentials.refresh_at(&accounts.http, &base).await
                }
                None => credentials.refresh(&accounts.http).await,
            }
        };
        let result = tokio::select! {
            result = work => result,
            () = self.gone.cancelled() => return None,
        };
        match result {
            Ok(fresh) => accounts.rotated(self, fresh),
            // Whoever asked (the worker or a refused request), a failure that
            // may pass is tried again in a minute.
            Err(e) if transient(&e) => {
                tracing::debug!(
                    service = self.service.name(),
                    "refresh failed: {}",
                    kind(&e)
                );
                self.set_due(Instant::now() + RETRY);
                self.set_health(AccountState::Failing);
                None
            }
            Err(e) => {
                tracing::info!(
                    service = self.service.name(),
                    "refresh refused: {}",
                    kind(&e)
                );
                self.set_health(AccountState::Reconnect);
                None
            }
        }
    }
}

const YEAR: Duration = Duration::from_millis(YEAR_MS);

impl Auth for Connection {
    fn token(&self) -> String {
        match &lock(&self.record).credential {
            DebridCredential::Key { key } => key.clone(),
            DebridCredential::Oauth { access_token, .. } => access_token.clone(),
        }
    }

    /// An older token than the current one is retried with the current one;
    /// the current one refused means one refresh (OAuth) or a reconnect (a
    /// key).
    fn refused<'a>(&'a self, used: &'a str) -> BoxFuture<'a, Option<String>> {
        Box::pin(async move {
            if self.gone.is_cancelled() {
                return None;
            }
            if matches!(lock(&self.record).credential, DebridCredential::Key { .. }) {
                self.set_health(AccountState::Reconnect);
                return None;
            }
            self.refresh(used).await
        })
    }
}

/// Refreshes an OAuth token a quarter of its lifetime before it expires,
/// until the connection goes or the service refuses it.
async fn keep_fresh(conn: Arc<Connection>) {
    loop {
        let due = *lock(&conn.due);
        if Instant::now() < due {
            tokio::select! {
                () = conn.gone.cancelled() => return,
                () = tokio::time::sleep_until(due) => continue,
                () = conn.wake.notified() => continue,
            }
        }
        if conn.gone.is_cancelled() || conn.health() == AccountState::Reconnect {
            return;
        }
        let used = conn.token();
        if conn.refresh(&used).await.is_some() {
            // At most one scheduled refresh a minute, even when the service
            // hands out a token with no usable lifetime.
            let due = (*lock(&conn.due)).max(Instant::now() + RETRY);
            conn.set_due(due);
        }
    }
}

impl Accounts {
    pub(crate) fn new(http: HttpClient) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            me: me.clone(),
            http,
            origin: RwLock::new(None),
            state: Mutex::default(),
            snapshot: SharedObservable::new(DebridSnapshot::default()),
            base: Resolver::new(ResolverConfig::default()),
            resolver: RwLock::new(None),
            set: tokio::sync::watch::Sender::new(Vec::new()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn origin(&self) -> Option<String> {
        self.origin
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub(crate) fn resolver(&self) -> Option<Resolver> {
        self.resolver
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Makes `record` the service's connection, replacing any other.
    fn install(&self, state: &mut State, service: StoreId, record: DebridRecord) {
        self.retire(state, service);
        bump(state, service);
        let oauth = matches!(record.credential, DebridCredential::Oauth { .. });
        let conn = Arc::new(Connection {
            service,
            generation: record.generation.clone(),
            record: Mutex::new(record),
            health: Mutex::new(AccountState::Ok),
            due: Mutex::new(Instant::now()),
            refreshing: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
            gone: CancellationToken::new(),
            accounts: self.me.clone(),
        });
        conn.reschedule();
        if oauth {
            tokio::spawn(keep_fresh(conn.clone()));
        }
        state.conns.insert(service, conn);
        // One order, the services' own, live and after a restore alike: which
        // store is the default never depends on which connected first.
        if !state.order.contains(&service) {
            let rank = |s: &StoreId| StoreId::ALL.iter().position(|x| x == s);
            let at = state
                .order
                .iter()
                .position(|s| rank(s) > rank(&service))
                .unwrap_or(state.order.len());
            state.order.insert(at, service);
        }
    }

    /// Ends the service's connection: its work stops and nothing it
    /// produces is published. The order keeps its place for a replacement.
    fn retire(&self, state: &mut State, service: StoreId) {
        let Some(conn) = state.conns.remove(&service) else {
            return;
        };
        conn.gone.cancel();
        state.pending.retain(|_, r| r.generation != conn.generation);
        state.gone.insert(conn.generation.clone());
        bump(state, service);
    }

    fn disconnect_service(&self, state: &mut State, service: StoreId) {
        self.retire(state, service);
        state.order.retain(|s| *s != service);
    }

    /// After any change: a new resolver, the cache cleaned of gone
    /// connections, a new snapshot.
    fn changed(&self, state: &State) {
        let mut resolver = self.base.sharing();
        for conn in state.order.iter().filter_map(|s| state.conns.get(s)) {
            resolver = self.add(resolver, conn.clone());
        }
        *self.resolver.write().unwrap_or_else(|p| p.into_inner()) =
            (!state.conns.is_empty()).then_some(resolver);
        self.base.prune();
        let set: Vec<(StoreId, String)> = state
            .order
            .iter()
            .filter_map(|s| state.conns.get(s))
            .map(|c| (c.service, c.generation.clone()))
            .collect();
        self.set.send_if_modified(|old| {
            let moved = *old != set;
            *old = set;
            moved
        });
        self.publish(state);
    }

    /// Moves whenever an account is added, replaced or removed; marked seen
    /// as of now.
    pub(crate) fn changes(&self) -> AccountChanges {
        self.set.subscribe()
    }

    fn add(&self, resolver: Resolver, conn: Arc<Connection>) -> Resolver {
        let (http, origin) = (self.http.clone(), self.origin());
        let (service, generation, gone) =
            (conn.service, conn.generation.clone(), conn.gone.clone());
        let auth: Arc<dyn Auth> = conn;
        let at = move |path: &str| origin.as_ref().map(|o| format!("{o}{path}"));
        match service {
            StoreId::RealDebrid => {
                let base = at("/rest/1.0");
                resolver.add_connection(&generation, gone, move |p| {
                    let store = RealDebrid::with_auth(http.clone(), auth.clone()).with_priority(p);
                    match &base {
                        Some(base) => store.with_base_url(base),
                        None => store,
                    }
                })
            }
            StoreId::AllDebrid => {
                let base = at("");
                resolver.add_connection(&generation, gone, move |p| {
                    let store = AllDebrid::with_auth(http.clone(), auth.clone()).with_priority(p);
                    match &base {
                        Some(base) => store.with_base_url(base),
                        None => store,
                    }
                })
            }
            StoreId::TorBox => {
                let base = at("");
                resolver.add_connection(&generation, gone, move |p| {
                    let store = TorBox::with_auth(http.clone(), auth.clone()).with_priority(p);
                    match &base {
                        Some(base) => store.with_base_url(base),
                        None => store,
                    }
                })
            }
            StoreId::Premiumize => {
                let base = at("/api");
                resolver.add_connection(&generation, gone, move |p| {
                    let store = Premiumize::with_auth(http.clone(), auth.clone()).with_priority(p);
                    match &base {
                        Some(base) => store.with_base_url(base),
                        None => store,
                    }
                })
            }
        }
    }

    fn publish(&self, state: &State) {
        let accounts = state
            .order
            .iter()
            .filter_map(|s| state.conns.get(s))
            .map(|c| DebridAccount {
                service: c.service.name().to_owned(),
                generation: c.generation.clone(),
                state: c.health(),
            })
            .collect();
        let pending = StoreId::ALL
            .iter()
            .filter_map(|s| state.pending.get(s).cloned())
            .collect();
        self.snapshot
            .set_if_not_eq(DebridSnapshot { accounts, pending });
    }

    /// A refresh came back: the connection's new revision, unless it was
    /// replaced or disconnected meanwhile.
    fn rotated(&self, conn: &Connection, fresh: Credentials) -> Option<String> {
        let mut state = self.lock();
        if conn.gone.is_cancelled() {
            return None;
        }
        let token = fresh.access_token.clone();
        let record = {
            let mut record = lock(&conn.record);
            record.revision = record.revision.saturating_add(1);
            record.credential = oauth(fresh, now_ms());
            record.clone()
        };
        conn.reschedule();
        *lock(&conn.health) = AccountState::Ok;
        state.pending.insert(conn.service, record);
        self.publish(&state);
        Some(token)
    }

    /// A sign-in finished. Its credential becomes the connection and the
    /// pending record, unless the service changed since the sign-in began.
    fn collected(&self, service: StoreId, epoch: u64, credential: Credential) -> bool {
        let mut state = self.lock();
        if state.epochs.get(&service).copied().unwrap_or_default() != epoch {
            return false;
        }
        let credential = match credential {
            Credential::Key(key) => DebridCredential::Key { key },
            Credential::OAuth(credentials) => oauth(credentials, now_ms()),
        };
        let record = new_record(service, credential);
        self.install(&mut state, service, record.clone());
        state.pending.insert(service, record);
        state.logins.remove(&service);
        self.changed(&state);
        true
    }

    fn restore(
        &self,
        records: Vec<DebridRecord>,
        tombstones: Vec<DebridTombstone>,
    ) -> Result<RestoreAnswer, CoreError> {
        for r in &records {
            service(&r.service)?;
        }
        for t in &tombstones {
            service(&t.service)?;
        }
        let mut state = self.lock();
        for t in &tombstones {
            let id = service(&t.service)?;
            state.gone.insert(t.generation.clone());
            state.pending.retain(|_, r| r.generation != t.generation);
            if state
                .conns
                .get(&id)
                .is_some_and(|c| c.generation == t.generation)
            {
                self.disconnect_service(&mut state, id);
            }
        }
        let mut answer = RestoreAnswer::default();
        let handed: HashSet<String> = records.iter().map(|r| r.generation.clone()).collect();
        for record in records {
            let id = service(&record.service)?;
            if state.gone.contains(&record.generation) {
                answer.refused.push(record.generation);
                continue;
            }
            match state.conns.get(&id) {
                // The same connection: the higher revision wins.
                Some(conn) if conn.generation == record.generation => {
                    let mut current = lock(&conn.record);
                    if record.revision > current.revision {
                        *current = record;
                        drop(current);
                        conn.reschedule();
                    }
                }
                _ => self.install(&mut state, id, record),
            }
        }
        answer.tombstones = tombstones
            .into_iter()
            .filter(|t| !handed.contains(&t.generation))
            .collect();
        self.changed(&state);
        Ok(answer)
    }

    fn ack(&self, service: StoreId, generation: &str, revision: u64) {
        let mut state = self.lock();
        if state
            .pending
            .get(&service)
            .is_some_and(|r| r.generation == generation && r.revision == revision)
        {
            state.pending.remove(&service);
            self.publish(&state);
        }
    }

    fn set_key(&self, service: StoreId, key: String) -> DebridRecord {
        let mut state = self.lock();
        let record = new_record(service, DebridCredential::Key { key });
        self.install(&mut state, service, record.clone());
        state.pending.insert(service, record.clone());
        self.changed(&state);
        record
    }

    fn held(&self) -> bool {
        let state = self.lock();
        !state.conns.is_empty() || !state.pending.is_empty()
    }

    fn disconnect(&self, service: StoreId, generation: &str) {
        let mut state = self.lock();
        state.gone.insert(generation.to_owned());
        state.pending.retain(|_, r| r.generation != generation);
        if state
            .conns
            .get(&service)
            .is_some_and(|c| c.generation == generation)
        {
            self.disconnect_service(&mut state, service);
        }
        self.changed(&state);
    }

    /// The legacy call: an ordered full set of key services. An unchanged
    /// key keeps its generation; a key service not listed is disconnected;
    /// an OAuth service is replaced only when listed. Order: the call's,
    /// then the OAuth services in the order they connected.
    fn set_keys(&self, keys: &[(StoreId, String)]) -> usize {
        let mut state = self.lock();
        let mut listed: Vec<StoreId> = Vec::new();
        for (id, key) in keys {
            if !listed.contains(id) {
                listed.push(*id);
            }
            let same = state.conns.get(id).is_some_and(|c| {
                matches!(&lock(&c.record).credential, DebridCredential::Key { key: k } if k == key)
            });
            if !same {
                let record = new_record(*id, DebridCredential::Key { key: key.clone() });
                self.install(&mut state, *id, record);
            }
        }
        let unlisted: Vec<StoreId> = state
            .conns
            .iter()
            .filter(|(id, c)| {
                !listed.contains(id)
                    && matches!(lock(&c.record).credential, DebridCredential::Key { .. })
            })
            .map(|(id, _)| *id)
            .collect();
        for id in unlisted {
            self.disconnect_service(&mut state, id);
        }
        let rest: Vec<StoreId> = state
            .order
            .iter()
            .filter(|id| !listed.contains(id) && state.conns.contains_key(id))
            .copied()
            .collect();
        state.order = listed.into_iter().chain(rest).collect();
        self.changed(&state);
        state.conns.len()
    }

    fn start_login(
        &self,
        service: StoreId,
    ) -> (SharedObservable<DebridLoginSnapshot>, CancellationToken) {
        let snapshot = SharedObservable::new(DebridLoginSnapshot::Starting);
        let cancel = CancellationToken::new();
        let epoch = {
            let mut state = self.lock();
            if let Some(older) = state.logins.insert(service, cancel.clone()) {
                older.cancel();
            }
            bump(&mut state, service)
        };
        if let Some(me) = self.me.upgrade() {
            tokio::spawn(login(me, service, epoch, cancel.clone(), snapshot.clone()));
        }
        (snapshot, cancel)
    }
}

impl Drop for Accounts {
    fn drop(&mut self) {
        let state = lock(&self.state);
        for conn in state.conns.values() {
            conn.gone.cancel();
        }
        for login in state.logins.values() {
            login.cancel();
        }
    }
}

fn failed(e: &DebridError) -> DebridLoginSnapshot {
    // The kind, and for the network its text with addresses redacted: never a
    // code, a key or a token.
    match e {
        DebridError::Http(http) => tracing::warn!(error = %http, "debrid sign-in failed"),
        e => tracing::warn!(error = kind(e), "debrid sign-in failed"),
    }
    let reason = match e {
        DebridError::Http(_) | DebridError::Timeout => {
            "Could not reach the service. Check the connection and try again.".to_owned()
        }
        DebridError::RateLimited { .. } => "The service is busy. Try again in a minute.".into(),
        DebridError::Provider { code, .. } if code == "no_code_sign_in" => {
            "This service signs in with a key.".into()
        }
        DebridError::Provider { code, .. } => format!("The service refused the sign-in ({code})."),
        e => format!("The service refused the sign-in ({}).", kind(e)),
    };
    DebridLoginSnapshot::Failed { reason }
}

/// One sign-in. Closing the view stops new polls; a poll already sent
/// completes, and its credential counts only under the epoch it began in.
async fn login(
    accounts: Arc<Accounts>,
    service: StoreId,
    epoch: u64,
    cancel: CancellationToken,
    snapshot: SharedObservable<DebridLoginSnapshot>,
) {
    let set = |state: DebridLoginSnapshot| drop(snapshot.set(state));
    let start = || async {
        match accounts.origin() {
            Some(origin) => {
                let base = match service {
                    StoreId::RealDebrid => format!("{origin}/oauth/v2"),
                    _ => origin,
                };
                Login::start_at(&accounts.http, service, &base).await
            }
            None => Login::start(&accounts.http, service).await,
        }
    };
    // A connection the service dropped gets one more try before the sheet
    // says it could not reach it (seen on Real-Debrid from one of its hosts).
    // Only the network: an answer it could not read, a blocked address or a
    // timeout is said at once.
    let first_or_again = async {
        match start().await {
            Err(DebridError::Http(HttpError::Network(_))) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                start().await
            }
            other => other,
        }
    };
    let login = tokio::select! {
        () = cancel.cancelled() => return,
        login = first_or_again => match login {
            Ok(login) => login,
            Err(e) => return set(failed(&e)),
        },
    };
    let code = login.code().clone();
    let lifetime = u64::try_from(code.expires_in.as_millis()).unwrap_or(u64::MAX);
    set(DebridLoginSnapshot::Code {
        user_code: code.user_code,
        url: code.url,
        qr: code.qr,
        expires_ms: now_ms().saturating_add(lifetime),
    });
    let mut interval = code.interval;
    loop {
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(interval) => {}
        }
        // Single flight per service: an older sign-in's request in flight
        // finishes first. Once sent, a request completes even if closed.
        let gate = accounts
            .lock()
            .collecting
            .entry(service)
            .or_default()
            .clone();
        let _turn = tokio::select! {
            () = cancel.cancelled() => return,
            turn = gate.lock() => turn,
        };
        if cancel.is_cancelled() {
            return;
        }
        match login.poll().await {
            Ok(LoginPoll::Pending) => {}
            Ok(LoginPoll::SlowDown) => interval = slow_down(interval),
            Ok(LoginPoll::Approved(credential)) => {
                return set(if accounts.collected(service, epoch, credential) {
                    DebridLoginSnapshot::Done
                } else {
                    DebridLoginSnapshot::Used
                });
            }
            Ok(LoginPoll::Expired) => return set(DebridLoginSnapshot::Expired),
            Ok(LoginPoll::Denied) => return set(DebridLoginSnapshot::Denied),
            Ok(LoginPoll::Used) => return set(DebridLoginSnapshot::Used),
            // Polling goes on until the code expires.
            Err(e) if transient(&e) => {
                tracing::debug!(service = service.name(), "poll: {}", kind(&e))
            }
            Err(e) => return set(failed(&e)),
        }
    }
}

/// The device's accounts and the records waiting for the keystore.
pub struct DebridView {
    snapshot: SharedObservable<DebridSnapshot>,
}

impl DebridView {
    pub fn open(core: &Core) -> Self {
        Self {
            snapshot: core.accounts().snapshot.clone(),
        }
    }

    pub fn subscribe(
        &self,
    ) -> (
        DebridSnapshot,
        Pin<Box<dyn Stream<Item = Vec<DebridSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

/// Sign-in by code for one service. Dropping it stops the polls.
pub struct DebridLoginView {
    snapshot: SharedObservable<DebridLoginSnapshot>,
    cancel: CancellationToken,
}

impl DebridLoginView {
    /// A second open for the same service stops the first one's polls.
    pub fn open(core: &Core, service: StoreId) -> Self {
        let (snapshot, cancel) = core.accounts().start_login(service);
        Self { snapshot, cancel }
    }

    pub fn snapshot(&self) -> DebridLoginSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        DebridLoginSnapshot,
        Pin<Box<dyn Stream<Item = Vec<DebridLoginSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for DebridLoginView {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Core {
    /// Moves whenever a streaming account is added, replaced or removed:
    /// a view that ranks or captions by them follows it. Take it before
    /// reading `debrid`, so no change falls in between.
    pub(crate) fn account_changes(&self) -> AccountChanges {
        self.accounts().changes()
    }

    /// The resolver over the connected accounts, `None` when there are none.
    pub fn debrid(&self) -> Option<Resolver> {
        self.accounts().resolver()
    }

    /// The legacy call (keys from the platform keystore, the first is the
    /// default store). Returns how many services are connected.
    pub fn set_debrid_keys(&self, keys: &[(StoreId, String)]) -> usize {
        self.accounts().set_keys(keys)
    }

    /// At start, before any view: what the keystore holds.
    pub fn restore_debrid(
        &self,
        records: Vec<DebridRecord>,
        tombstones: Vec<DebridTombstone>,
    ) -> Result<RestoreAnswer, CoreError> {
        self.accounts().restore(records, tombstones)
    }

    /// The app stored this record. Ignored unless it is the pending one.
    pub fn ack_debrid(&self, service: &str, generation: &str, revision: u64) {
        if let Some(id) = StoreId::from_service(service) {
            self.accounts().ack(id, generation, revision);
        }
    }

    /// A typed or handed-off key: a new connection, offered as pending.
    pub fn set_debrid_key(&self, service: StoreId, key: &str) -> DebridRecord {
        self.accounts().set_key(service, key.to_owned())
    }

    /// Sent after the app wrote the tombstone. Drops the connection when it
    /// still has `generation`; the generation is never restored again.
    pub fn disconnect_debrid(&self, service: StoreId, generation: &str) {
        self.accounts().disconnect(service, generation);
    }

    /// Whether any debrid connection or pending record is left. A wipe that
    /// takes them (D13) is refused while one is: each goes the way
    /// `disconnect_debrid` takes, after its tombstone, so a crash never
    /// brings a key back.
    pub fn debrid_held(&self) -> bool {
        self.accounts().held()
    }

    /// Points every debrid service at one origin (tests): Real-Debrid at
    /// `/rest/1.0` and `/oauth/v2`, Premiumize at `/api`, the others at the
    /// root. Set it before connecting.
    #[doc(hidden)]
    pub fn set_debrid_origin(&self, origin: &str) {
        *self
            .accounts()
            .origin
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(origin.trim_end_matches('/').to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_MS: u64 = 60 * MINUTE_MS;

    #[test]
    fn refresh_comes_a_quarter_before_expiry() {
        let now = 1_000 * HOUR_MS;
        assert_eq!(
            refresh_wait(now, HOUR_MS, now),
            Duration::from_millis(45 * MINUTE_MS)
        );
    }

    #[test]
    fn refresh_deadline_survives_restart() {
        // Issued 50 minutes before a restart: past the 45 minute mark.
        let now = 1_000 * HOUR_MS;
        assert_eq!(
            refresh_wait(now - 50 * MINUTE_MS, HOUR_MS, now),
            Duration::ZERO
        );
        // Issued 30 minutes before: 15 minutes to go.
        assert_eq!(
            refresh_wait(now - 30 * MINUTE_MS, HOUR_MS, now),
            Duration::from_millis(15 * MINUTE_MS)
        );
    }

    #[test]
    fn bad_clock_refreshes_at_once() {
        let now = 1_000 * HOUR_MS;
        // Issued in the future: the clock went back.
        assert_eq!(refresh_wait(now + MINUTE_MS, HOUR_MS, now), Duration::ZERO);
        // Lifetimes outside 1 minute to 1 year.
        assert_eq!(refresh_wait(now, MINUTE_MS - 1, now), Duration::ZERO);
        assert_eq!(refresh_wait(now, YEAR_MS + 1, now), Duration::ZERO);
        // A deadline past the end of time.
        let late = u64::MAX - 10_000;
        assert_eq!(refresh_wait(late, MINUTE_MS, late), Duration::ZERO);
        assert!(refresh_wait(now, YEAR_MS, now) > Duration::ZERO);
    }

    #[test]
    fn slow_down_adds_5_s() {
        assert_eq!(slow_down(Duration::from_secs(5)), Duration::from_secs(10));
        assert_eq!(slow_down(Duration::MAX), Duration::MAX);
    }

    #[test]
    fn generations_are_32_hex_characters() {
        let g = new_generation();
        assert_eq!(g.len(), 32);
        assert!(
            g.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_ne!(g, new_generation());
    }

    #[test]
    fn secrets_never_reach_debug() {
        let record = DebridRecord {
            service: "realdebrid".into(),
            generation: "g".into(),
            revision: 1,
            credential: DebridCredential::Oauth {
                client_id: "CLIENTID".into(),
                client_secret: "CLIENTSECRET".into(),
                refresh_token: "REFRESHTOKEN".into(),
                access_token: "ACCESSTOKEN".into(),
                issued_at_ms: 1,
                expires_in_ms: 2,
            },
        };
        let key = DebridCredential::Key {
            key: "APIKEY".into(),
        };
        let printed = format!("{record:?} {key:?}");
        for secret in [
            "CLIENTID",
            "CLIENTSECRET",
            "REFRESHTOKEN",
            "ACCESSTOKEN",
            "APIKEY",
        ] {
            assert!(!printed.contains(secret), "{printed}");
        }
    }

    #[tokio::test]
    async fn a_failed_refresh_from_a_request_is_retried_in_a_minute() {
        // Nothing listens there: the refresh meets a network error.
        let accounts = Accounts::new(HttpClient::new(Default::default()).unwrap());
        *accounts.origin.write().unwrap() = Some("http://127.0.0.1:9".into());
        let record = new_record(
            StoreId::RealDebrid,
            DebridCredential::Oauth {
                client_id: "C".into(),
                client_secret: "S".into(),
                refresh_token: "R".into(),
                access_token: "TOKEN".into(),
                issued_at_ms: now_ms(),
                expires_in_ms: 60 * MINUTE_MS,
            },
        );
        accounts.install(&mut accounts.lock(), StoreId::RealDebrid, record);
        let conn = accounts.lock().conns[&StoreId::RealDebrid].clone();
        // The worker's own refresh is 45 minutes away.
        assert!(*lock(&conn.due) > Instant::now() + Duration::from_secs(40 * 60));
        assert_eq!(conn.refused("TOKEN").await, None);
        assert_eq!(conn.health(), AccountState::Failing);
        let due = *lock(&conn.due);
        let now = Instant::now();
        assert!(due <= now + RETRY && due > now + RETRY - Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_401_with_an_old_token_retries_without_refreshing() {
        // No server: a refresh would fail and answer `None`.
        let accounts = Accounts::new(HttpClient::new(Default::default()).unwrap());
        *accounts.origin.write().unwrap() = Some("http://127.0.0.1:9".into());
        let record = new_record(
            StoreId::RealDebrid,
            DebridCredential::Oauth {
                client_id: "C".into(),
                client_secret: "S".into(),
                refresh_token: "R".into(),
                access_token: "NEW".into(),
                issued_at_ms: now_ms(),
                expires_in_ms: 60 * MINUTE_MS,
            },
        );
        accounts.install(&mut accounts.lock(), StoreId::RealDebrid, record);
        let conn = accounts.lock().conns[&StoreId::RealDebrid].clone();
        assert_eq!(conn.refused("OLD").await.as_deref(), Some("NEW"));
        assert_eq!(conn.health(), AccountState::Ok);
        assert_eq!(lock(&conn.record).revision, 1);
    }
}
