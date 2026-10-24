//! Lazy resolution: turn a torrent reference into a direct link when the user
//! presses play (or ahead of time with `prefetch`). Identical concurrent
//! requests share one flight, results are cached with a TTL that never
//! outlives the provider's own expiry.
//!
//! The resolver never removes torrents it added: an uncached torrent keeps
//! downloading on the account, and deleting something the user already owned
//! is worse than a little clutter.

use crate::select::pick_file;
use crate::util::{normalize_hash, unix_now};
use crate::{
    CacheStatus, DebridError, DynStore, ResolvedLink, Store, StoreId, TorrentInfo, TorrentStatus,
};
use futures_util::FutureExt;
use futures_util::future::{BoxFuture, Shared};
use mq_http::Priority;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

fn disconnected() -> DebridError {
    DebridError::provider("disconnected", "the account was disconnected")
}

/// Wall clock in unix seconds, injectable so TTL logic is testable.
pub trait Clock: Send + Sync {
    fn now(&self) -> u64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        unix_now()
    }
}

/// A clock moved by hand.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(start: u64) -> Self {
        Self(AtomicU64::new(start))
    }

    pub fn advance(&self, secs: u64) {
        self.0.fetch_add(secs, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone)]
pub struct ResolverConfig {
    /// How long a resolved link is reused (default 15 minutes).
    pub link_ttl: Duration,
    /// Delay between status polls of a torrent that is not ready yet.
    pub poll_interval: Duration,
    /// How long to wait for a torrent to become ready before giving up with
    /// `NotCached`.
    pub ready_timeout: Duration,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            link_ttl: Duration::from_secs(15 * 60),
            poll_interval: Duration::from_millis(750),
            ready_timeout: Duration::from_secs(20),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolveRequest {
    /// Which store to use; `None` means the first one added to the resolver.
    pub store: Option<StoreId>,
    pub info_hash: String,
    pub file_idx: Option<u32>,
    pub filename_hint: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl ResolveRequest {
    pub fn new(info_hash: impl Into<String>) -> Self {
        Self {
            info_hash: info_hash.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    store: StoreId,
    /// The account connection; a replaced one never shares results.
    generation: Arc<str>,
    hash: String,
    file_idx: Option<u32>,
    filename: Option<String>,
    season: Option<u32>,
    episode: Option<u32>,
}

type Flight = Shared<BoxFuture<'static, Result<ResolvedLink, DebridError>>>;

struct Cached {
    link: ResolvedLink,
    valid_until: u64,
    /// The connection's; once cancelled the entry is never served.
    gone: CancellationToken,
}

#[derive(Default)]
struct State {
    cache: Mutex<HashMap<Key, Cached>>,
    /// With the connection's token, so `prune` can drop a gone one's flights
    /// (a flight holds a resolver clone, and so its stores and credential).
    flights: Mutex<HashMap<Key, (u64, Flight, CancellationToken)>>,
    next_flight: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// One store registered twice: once per request priority.
#[derive(Clone)]
struct Slot {
    id: StoreId,
    generation: Arc<str>,
    /// Cancelled when the connection is replaced or disconnected: its
    /// flights stop and nothing it resolved is published.
    gone: CancellationToken,
    resolve: Arc<dyn DynStore>,
    prefetch: Arc<dyn DynStore>,
}

#[derive(Clone)]
pub struct Resolver {
    slots: Vec<Slot>,
    config: ResolverConfig,
    clock: Arc<dyn Clock>,
    state: Arc<State>,
}

impl Resolver {
    pub fn new(config: ResolverConfig) -> Self {
        Self {
            slots: Vec::new(),
            config,
            clock: Arc::new(SystemClock),
            state: Arc::default(),
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Adds a store. The factory is called once per priority (`Resolve` for
    /// user-initiated calls, `Prefetch` for warming) so the HTTP budget can
    /// tell them apart.
    pub fn add_store<S: Store + 'static>(self, factory: impl Fn(Priority) -> S) -> Self {
        self.add_connection("", CancellationToken::new(), factory)
    }

    /// Adds one account connection. Results are keyed by `generation`;
    /// cancelling `gone` stops its in-flight resolves and keeps anything
    /// they return out of the cache.
    pub fn add_connection<S: Store + 'static>(
        mut self,
        generation: &str,
        gone: CancellationToken,
        factory: impl Fn(Priority) -> S,
    ) -> Self {
        let resolve = factory(Priority::Resolve);
        self.slots.push(Slot {
            id: resolve.id(),
            generation: generation.into(),
            gone,
            resolve: Arc::new(resolve),
            prefetch: Arc::new(factory(Priority::Prefetch)),
        });
        self
    }

    /// A resolver with no stores that shares this one's cache and flights,
    /// so connections kept across a rebuild keep their results.
    pub fn sharing(&self) -> Self {
        Self {
            slots: Vec::new(),
            ..self.clone()
        }
    }

    /// The configured stores, default first.
    pub fn stores(&self) -> Vec<StoreId> {
        self.slots.iter().map(|s| s.id).collect()
    }

    pub async fn resolve(&self, req: ResolveRequest) -> Result<ResolvedLink, DebridError> {
        self.run(req, Priority::Resolve).await
    }

    /// Warms the cache for a request the user is likely to make. Same dedupe
    /// as `resolve`; callers usually `tokio::spawn` it and ignore the result.
    pub async fn prefetch(&self, req: ResolveRequest) -> Result<(), DebridError> {
        self.run(req, Priority::Prefetch).await.map(|_| ())
    }

    /// Whether `store` (the default when `None`) has each of `hashes` ready,
    /// one answer per hash in order, at prefetch priority, in batches of
    /// 100. A store without a reliable check answers `Unknown` and is not
    /// asked; an invalid hash is `Unknown` too.
    pub async fn check(
        &self,
        store: Option<StoreId>,
        hashes: &[String],
    ) -> Result<Vec<CacheStatus>, DebridError> {
        let slot = self.slot(store)?;
        if slot.gone.is_cancelled() {
            return Err(disconnected());
        }
        let mut out = vec![CacheStatus::Unknown; hashes.len()];
        if !slot.prefetch.capabilities().instant_check {
            return Ok(out);
        }
        let valid: Vec<(usize, String)> = hashes
            .iter()
            .enumerate()
            .filter_map(|(i, h)| normalize_hash(h).ok().map(|h| (i, h)))
            .collect();
        for batch in valid.chunks(100) {
            let asked: Vec<String> = batch.iter().map(|(_, h)| h.clone()).collect();
            let answers = slot.prefetch.check(&asked).await?;
            for ((i, _), answer) in batch.iter().zip(answers) {
                out[*i] = answer;
            }
        }
        Ok(out)
    }

    fn slot(&self, wanted: Option<StoreId>) -> Result<&Slot, DebridError> {
        let slot = match wanted {
            Some(id) => self.slots.iter().find(|s| s.id == id),
            None => self.slots.first(),
        };
        slot.ok_or_else(|| DebridError::provider("store_not_configured", "no matching store"))
    }

    async fn run(
        &self,
        req: ResolveRequest,
        priority: Priority,
    ) -> Result<ResolvedLink, DebridError> {
        let slot = self.slot(req.store)?;
        if slot.gone.is_cancelled() {
            return Err(disconnected());
        }
        let hash = normalize_hash(&req.info_hash)?;
        let key = Key {
            store: slot.id,
            generation: slot.generation.clone(),
            hash: hash.clone(),
            file_idx: req.file_idx,
            filename: req.filename_hint.as_ref().map(|f| f.to_lowercase()),
            season: req.season,
            episode: req.episode,
        };
        let (flight_id, flight) = {
            // Cache is checked under the flights lock so a flight that just
            // finished is always visible as a cache entry.
            let mut flights = lock(&self.state.flights);
            if let Some(link) = self.cached(&key) {
                return Ok(link);
            }
            match flights.get(&key) {
                Some((id, flight, _)) => (*id, flight.clone()),
                None => {
                    let id = self.state.next_flight.fetch_add(1, Ordering::Relaxed);
                    let store = match priority {
                        Priority::Prefetch => slot.prefetch.clone(),
                        _ => slot.resolve.clone(),
                    };
                    // No stores: an abandoned flight keeps only its own store alive,
                    // never another account's.
                    let this = self.sharing();
                    let flight_key = key.clone();
                    let gone = slot.gone.clone();
                    let flight: Flight = async move {
                        let work = this.resolve_uncached(store.as_ref(), &hash, &req);
                        let link = tokio::select! {
                            link = work => link?,
                            () = gone.cancelled() => return Err(disconnected()),
                        };
                        this.remember(flight_key, &link, &gone)?;
                        Ok(link)
                    }
                    .boxed()
                    .shared();
                    flights.insert(key.clone(), (id, flight.clone(), slot.gone.clone()));
                    (id, flight)
                }
            }
        };
        let result = flight.await;
        let mut flights = lock(&self.state.flights);
        if flights.get(&key).is_some_and(|(id, _, _)| *id == flight_id) {
            flights.remove(&key);
        }
        // A waiter resuming after a disconnect never gets a link the flight
        // finished before it.
        if slot.gone.is_cancelled() {
            return Err(disconnected());
        }
        result
    }

    fn cached(&self, key: &Key) -> Option<ResolvedLink> {
        let now = self.clock.now();
        let mut cache = lock(&self.state.cache);
        match cache.get(key) {
            Some(entry) if entry.valid_until > now && !entry.gone.is_cancelled() => {
                Some(entry.link.clone())
            }
            Some(_) => {
                cache.remove(key);
                None
            }
            None => None,
        }
    }

    /// Caches `link` unless its connection is gone, checked under the cache
    /// lock: whoever cancels a connection first and then prunes under that
    /// lock leaves nothing of it behind.
    fn remember(
        &self,
        key: Key,
        link: &ResolvedLink,
        gone: &CancellationToken,
    ) -> Result<(), DebridError> {
        let now = self.clock.now();
        let mut valid_until = now.saturating_add(self.config.link_ttl.as_secs());
        if let Some(expires) = link.expires_at {
            valid_until = valid_until.min(expires);
        }
        let mut cache = lock(&self.state.cache);
        if gone.is_cancelled() {
            return Err(disconnected());
        }
        // Expired entries and those of gone connections are dropped on every
        // insert, which bounds the map to the links of one TTL window.
        cache.retain(|_, e| e.valid_until > now && !e.gone.is_cancelled());
        if valid_until > now {
            cache.insert(
                key,
                Cached {
                    link: link.clone(),
                    valid_until,
                    gone: gone.clone(),
                },
            );
        }
        Ok(())
    }

    /// Forgets the cached link `url`: its origin refused it (an expired
    /// download link), so the next resolve asks the store again.
    pub fn forget(&self, url: &str) {
        lock(&self.state.cache).retain(|_, e| e.link.url != url);
    }

    /// Drops what gone connections left in the cache and their flights. Call
    /// after cancelling.
    pub fn prune(&self) {
        lock(&self.state.flights).retain(|_, (_, _, gone)| !gone.is_cancelled());
        lock(&self.state.cache).retain(|_, e| !e.gone.is_cancelled());
    }

    async fn resolve_uncached(
        &self,
        store: &dyn DynStore,
        hash: &str,
        req: &ResolveRequest,
    ) -> Result<ResolvedLink, DebridError> {
        // Stores with a reliable cache check fail fast instead of spending an
        // add (TorBox limits uncached adds to 60 per hour).
        if store.capabilities().instant_check
            && store.check(&[hash.to_owned()]).await?.first() == Some(&CacheStatus::NotCached)
        {
            return Err(DebridError::NotCached);
        }
        let handle = store.add_magnet(hash).await?;
        let deadline = Instant::now() + self.config.ready_timeout;
        let mut info = self
            .wait_until(store, &handle.id, deadline, |s| {
                matches!(
                    s,
                    TorrentStatus::WaitingFileSelection | TorrentStatus::Ready
                )
            })
            .await?;
        let file = pick_file(&info.files, req)
            .map(|f| f.index)
            .ok_or(DebridError::NoPlayableFile)?;
        if info.status == TorrentStatus::WaitingFileSelection {
            store.select_files(&handle.id, &[file]).await?;
            info = self
                .wait_until(store, &handle.id, deadline, |s| *s == TorrentStatus::Ready)
                .await?;
        }
        store.link(&info, file).await
    }

    async fn wait_until(
        &self,
        store: &dyn DynStore,
        id: &str,
        deadline: Instant,
        done: impl Fn(&TorrentStatus) -> bool,
    ) -> Result<TorrentInfo, DebridError> {
        loop {
            let info = store.torrent(id).await?;
            if let TorrentStatus::Failed(reason) = &info.status {
                return Err(DebridError::provider("torrent_failed", reason.clone()));
            }
            if done(&info.status) {
                return Ok(info);
            }
            if Instant::now() >= deadline {
                // Still fetching metadata is a stall; downloading means the
                // content was not cached.
                return Err(match info.status {
                    TorrentStatus::Resolving => DebridError::Timeout,
                    _ => DebridError::NotCached,
                });
            }
            tokio::time::sleep(self.config.poll_interval).await;
        }
    }
}
