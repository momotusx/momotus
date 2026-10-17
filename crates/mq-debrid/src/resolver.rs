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
}

#[derive(Default)]
struct State {
    cache: Mutex<HashMap<Key, Cached>>,
    flights: Mutex<HashMap<Key, (u64, Flight)>>,
    next_flight: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// One store registered twice: once per request priority.
#[derive(Clone)]
struct Slot {
    id: StoreId,
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
    pub fn add_store<S: Store + 'static>(mut self, factory: impl Fn(Priority) -> S) -> Self {
        let resolve = factory(Priority::Resolve);
        self.slots.push(Slot {
            id: resolve.id(),
            resolve: Arc::new(resolve),
            prefetch: Arc::new(factory(Priority::Prefetch)),
        });
        self
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
        let hash = normalize_hash(&req.info_hash)?;
        let key = Key {
            store: slot.id,
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
                Some((id, flight)) => (*id, flight.clone()),
                None => {
                    let id = self.state.next_flight.fetch_add(1, Ordering::Relaxed);
                    let store = match priority {
                        Priority::Prefetch => slot.prefetch.clone(),
                        _ => slot.resolve.clone(),
                    };
                    let this = self.clone();
                    let flight_key = key.clone();
                    let flight: Flight = async move {
                        let link = this.resolve_uncached(store.as_ref(), &hash, &req).await?;
                        this.remember(flight_key, &link);
                        Ok(link)
                    }
                    .boxed()
                    .shared();
                    flights.insert(key.clone(), (id, flight.clone()));
                    (id, flight)
                }
            }
        };
        let result = flight.await;
        let mut flights = lock(&self.state.flights);
        if flights.get(&key).is_some_and(|(id, _)| *id == flight_id) {
            flights.remove(&key);
        }
        result
    }

    fn cached(&self, key: &Key) -> Option<ResolvedLink> {
        let now = self.clock.now();
        let mut cache = lock(&self.state.cache);
        match cache.get(key) {
            Some(entry) if entry.valid_until > now => Some(entry.link.clone()),
            Some(_) => {
                cache.remove(key);
                None
            }
            None => None,
        }
    }

    fn remember(&self, key: Key, link: &ResolvedLink) {
        let now = self.clock.now();
        let mut valid_until = now.saturating_add(self.config.link_ttl.as_secs());
        if let Some(expires) = link.expires_at {
            valid_until = valid_until.min(expires);
        }
        let mut cache = lock(&self.state.cache);
        // Expired entries are dropped on every insert, which bounds the map to
        // the links resolved within one TTL window.
        cache.retain(|_, e| e.valid_until > now);
        if valid_until > now {
            cache.insert(
                key,
                Cached {
                    link: link.clone(),
                    valid_until,
                },
            );
        }
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
