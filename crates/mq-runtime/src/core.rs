use crate::defaults::{self, DefaultAddonInfo, DefaultAddons, DefaultChoice, DefaultInstalled};
use mq_addons::{
    AddonClient, AddonError, CacheControl, CacheStore, InstalledAddon, ResponseCache,
    StoredResponse,
};
use mq_http::{ClientConfig, HttpClient};
use mq_library::{AddonRow, Hlc, HlcClock, Library, OWNER, OWNER_NAME, Policy, Table};
use mq_protocol::{Manifest, MetaItem, ResourcePath, ResourceResponse};
use mq_server::StreamServer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{Mutex, OnceCell, OwnedMutexGuard};

const META_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a series has no TVDB ids (`Core::episode_tvdb_ids`), with the words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TvdbMiss {
    /// Every meta addon answered that it has no such title (404, 410, or
    /// an answer without a meta).
    Unknown(String),
    /// None gave them now: an addon failed, or answered without TVDB ids.
    Unavailable(String),
    /// No addon answered at all: each one asked could not be reached or
    /// did not answer in time (the device may be offline).
    Unreached(String),
}

/// Why no addon gave a title's details (`Core::fetch_meta_or_miss`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MetaMiss {
    /// In a sentence the title page shows as is.
    pub reason: String,
    /// No addon answered at all, as with `TvdbMiss::Unreached`.
    pub unreached: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Library(#[from] mq_library::LibraryError),
    #[error(transparent)]
    Http(#[from] mq_http::HttpError),
    #[error(transparent)]
    Addon(#[from] mq_addons::AddonError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("addon is not installed: {0}")]
    NotInstalled(String),
    #[error("invalid setting: {0}")]
    BadPrefs(String),
    #[error("stored manifest is invalid: {0}")]
    BadManifest(String),
    #[error("torrent engine: {0}")]
    Torrent(String),
    #[error("debrid: {0}")]
    Debrid(String),
    #[error("Trakt is not set up in this build")]
    TraktNotConfigured,
    #[error("Trakt: {0}")]
    Trakt(String),
    /// `installDefaults` refused: an unknown key, or a kids profile (it
    /// reads the owner's addons).
    #[error("default addons: {0}")]
    Defaults(String),
}

#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub data_dir: PathBuf,
    /// The profile to open on first run, when this device has never switched
    /// and the profile exists; otherwise the owner. Never created.
    pub profile_id: String,
    /// Sync needs every device to have its own id. Apps pass one kept in the
    /// keystore with this-device-only access, so a restored backup on a new
    /// phone does not share it; 0 means a random id stored in the data dir
    /// (the CLI and tests).
    pub device_id: u64,
}

/// A handle on the device's runtime for one profile. Cheap to clone; every
/// view holds one. Handles for other profiles (`for_profile`) share the
/// whole runtime.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
    /// The profile this handle reads and writes.
    profile: Arc<str>,
}

/// A `Core` that does not keep the runtime alive: background work holds
/// one and ends once every handle is gone.
#[derive(Clone)]
pub(crate) struct WeakCore {
    inner: Weak<Inner>,
    profile: Arc<str>,
}

impl WeakCore {
    pub(crate) fn upgrade(&self) -> Option<Core> {
        Some(Core {
            inner: self.inner.upgrade()?,
            profile: self.profile.clone(),
        })
    }
}

/// Everything device-wide: the device's state, one stream server, torrent
/// engine, addon cache, skip cache and debrid resolver, whichever profile is
/// in use.
struct Inner {
    device: Arc<Device>,
    http: HttpClient,
    addons: AddonClient,
    server: OnceCell<Arc<StreamServer>>,
    /// Torrent data and DHT state (full builds start the engine on first use).
    #[cfg_attr(not(feature = "torrent"), allow(dead_code))]
    torrent_dir: PathBuf,
    #[cfg(feature = "torrent")]
    engine: OnceCell<mq_engine::Engine>,
    debrid: Arc<crate::debrid::Accounts>,
    /// Skip segment lookups and their session cache.
    segments: crate::segments::Finder,
    /// Tests: the free space read instead of the disk's.
    free_bytes: std::sync::OnceLock<u64>,
    /// Tests: live seeders answered instead of the trackers'.
    live_seeders: std::sync::Mutex<Option<FakeSeeders>>,
    /// The torrent folder's volume punches holes (plans/streaming-window.md
    /// 2.6), probed when the engine starts.
    #[cfg(feature = "torrent")]
    punches: std::sync::OnceLock<bool>,
    /// Walk builds: the window override (plans/streaming-window.md 4).
    #[cfg_attr(not(feature = "torrent"), allow(dead_code))]
    walk: Option<Walk>,
    /// TurkceAltyazi.org, read on this device: its caches and its one
    /// request at a time.
    turkcealtyazi: std::sync::OnceLock<Arc<mq_turkcealtyazi::Site>>,
}

/// What is one per data directory however often it is opened: the library
/// (its rows and change notifications), the clock, the sync gate and the PIN
/// throttle. Two opens with their own would count PIN tries apart and miss
/// each other's deletions. Services bound to an async runtime (the stream
/// server, the torrent engine) stay with each open.
struct Device {
    library: Arc<Library>,
    clock: HlcClock,
    /// One sync or roster operation at a time, across every session.
    sync_gate: Arc<Mutex<()>>,
    /// PIN attempts, one throttle for every session and profile.
    pins: std::sync::Mutex<crate::profiles::Throttle>,
    /// Trakt connections, sessions and rounds, started on first use.
    trakt: std::sync::OnceLock<Arc<crate::trakt::Trakt>>,
}

/// Devices open in this process, by canonical data directory.
static OPEN: std::sync::Mutex<Vec<(PathBuf, Weak<Device>)>> = std::sync::Mutex::new(Vec::new());

impl Device {
    fn new(library: Library, device_id: u64) -> Result<Arc<Self>, CoreError> {
        let device_id = match device_id {
            0 => stored_device_id(&library)?,
            id => id,
        };
        let clock = HlcClock::new(device_id);
        if let Some(newest) = library.newest_hlc()?.as_deref().and_then(Hlc::decode) {
            // Never below what this library holds; the clock's own ceiling
            // bounds a row stamped by a lying relay.
            clock.observe(newest, clock.wall_now());
        }
        // A library from before default addons (its owner is there): its
        // profiles get the card for new addons, never the first-run step.
        if library.device_value(DEFAULTS_ERA)?.is_none() {
            let era = if library.profile(OWNER)?.is_some() {
                "legacy"
            } else {
                "fresh"
            };
            library.set_device_value(DEFAULTS_ERA, era)?;
        }
        // The owner, with the lowest hlc: what another device synced wins.
        library.ensure_profile(OWNER, OWNER_NAME, &Hlc::zero(device_id).encode())?;
        Ok(Arc::new(Self {
            library: Arc::new(library),
            clock,
            sync_gate: Arc::default(),
            pins: std::sync::Mutex::new(crate::profiles::Throttle::new(std::time::Instant::now())),
            trakt: std::sync::OnceLock::new(),
        }))
    }

    /// The device over `data_dir`, shared with every open handle on it.
    fn open(data_dir: &Path, device_id: u64) -> Result<Arc<Self>, CoreError> {
        let path = std::fs::canonicalize(data_dir)?;
        let mut open = OPEN.lock().unwrap_or_else(|p| p.into_inner());
        open.retain(|(_, device)| device.strong_count() > 0);
        if let Some(device) = open
            .iter()
            .find(|(p, _)| *p == path)
            .and_then(|(_, device)| device.upgrade())
        {
            return Ok(device);
        }
        let device = Self::new(Library::open(path.join("library.sqlite"))?, device_id)?;
        open.push((path, Arc::downgrade(&device)));
        Ok(device)
    }
}

impl Core {
    /// Opens the core over `config.data_dir`. A directory already open in
    /// this process shares its device state with the new handle.
    pub fn open(config: CoreConfig) -> Result<Self, CoreError> {
        std::fs::create_dir_all(&config.data_dir)?;
        let device = Device::open(&config.data_dir, config.device_id)?;
        let torrents = config.data_dir.join("torrents");
        let walk = Walk::read(&config.data_dir);
        let core = Self::with_device(device, &config.profile_id, torrents, walk)?.active();
        if let Some(walk) = walk {
            walk.watch(core.inner.torrent_dir.clone());
        }
        // The probe before the first rank (plans/streaming-window.md 2.6),
        // off the opening thread.
        #[cfg(feature = "torrent")]
        {
            let inner = Arc::downgrade(&core.inner);
            std::thread::spawn(move || {
                let Some(inner) = inner.upgrade() else {
                    return;
                };
                let _ = std::fs::create_dir_all(&inner.torrent_dir);
                let punches = mq_engine::librqbit::punch_supported(&inner.torrent_dir);
                let _ = inner.punches.set(punches);
                tracing::info!(punches, "torrent folder probed");
            });
        }
        Ok(core)
    }

    /// A core over a throwaway database, for tests and the CLI. `profile` is
    /// created (with no PIN) and made the active one.
    pub fn temporary(profile: &str) -> Result<Self, CoreError> {
        let torrents = std::env::temp_dir().join(format!("mq-torrents-{}", std::process::id()));
        let device = Device::new(Library::temporary()?, 1)?;
        let core = Self::with_device(device, profile, torrents, None)?;
        // A throwaway core never asks real trackers: no swarm has a live
        // count unless a test gives one.
        core.set_live_seeders(HashMap::new());
        core.library()
            .ensure_profile(profile, profile, &core.zero_hlc())?;
        core.library().set_active_profile(profile)?;
        Ok(core)
    }

    fn with_device(
        device: Arc<Device>,
        profile: &str,
        torrent_dir: PathBuf,
        walk: Option<Walk>,
    ) -> Result<Self, CoreError> {
        // Build the release-name rule table off the UI path, so the first
        // stream list does not pay for it.
        std::thread::spawn(mq_relparse::warm_up);
        // Older version lists kept raw stream keys (code review 2026-10-20 P2-5).
        crate::speed::digest_version_lists(&device.library);
        let http = HttpClient::new(ClientConfig::default())?;
        let store = Arc::new(LibraryCache(device.library.clone()));
        let addons = AddonClient::with_cache(http.clone(), ResponseCache::with_store(512, store));
        Ok(Self {
            inner: Arc::new(Inner {
                device,
                http: http.clone(),
                addons,
                server: OnceCell::new(),
                torrent_dir,
                #[cfg(feature = "torrent")]
                engine: OnceCell::new(),
                debrid: crate::debrid::Accounts::new(http.clone()),
                segments: crate::segments::Finder::new(http.clone()),
                free_bytes: std::sync::OnceLock::new(),
                live_seeders: std::sync::Mutex::default(),
                turkcealtyazi: std::sync::OnceLock::new(),
                #[cfg(feature = "torrent")]
                punches: std::sync::OnceLock::new(),
                walk,
            }),
            profile: profile.into(),
        })
    }

    pub(crate) fn downgrade(&self) -> WeakCore {
        WeakCore {
            inner: Arc::downgrade(&self.inner),
            profile: self.profile.clone(),
        }
    }

    /// A handle for another profile over the same runtime.
    pub fn for_profile(&self, profile: &str) -> Core {
        Core {
            inner: self.inner.clone(),
            profile: profile.into(),
        }
    }

    pub fn http(&self) -> &HttpClient {
        &self.inner.http
    }

    /// The built-in TurkceAltyazi.org source.
    pub(crate) fn turkcealtyazi(&self) -> Arc<mq_turkcealtyazi::Site> {
        self.inner
            .turkcealtyazi
            .get_or_init(|| Arc::new(mq_turkcealtyazi::Site::new(self.http().clone())))
            .clone()
    }

    /// Tests: the built-in source at a fixture `origin`; only before its
    /// first use.
    #[doc(hidden)]
    pub fn use_turkcealtyazi_at(&self, origin: url::Url, limits: mq_turkcealtyazi::Limits) {
        let site = mq_turkcealtyazi::Site::with(self.http().clone(), origin, limits);
        let _ = self.inner.turkcealtyazi.set(Arc::new(site));
    }

    pub fn segments(&self) -> &crate::segments::Finder {
        &self.inner.segments
    }

    pub fn addons(&self) -> &AddonClient {
        &self.inner.addons
    }

    pub fn library(&self) -> &Library {
        &self.inner.device.library
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// What this profile may see (plans/m11-kids.md 4.1); fails closed.
    // ponytail: one read per call, the whole list for kids; a device-level
    // cache keyed by the `Access` notification if traces show it.
    pub fn policy(&self) -> Policy {
        self.library().policy(self.profile())
    }

    pub(crate) fn pins(&self) -> &std::sync::Mutex<crate::profiles::Throttle> {
        &self.inner.device.pins
    }

    pub(crate) fn trakt_service(&self) -> &Arc<crate::trakt::Trakt> {
        self.inner
            .device
            .trakt
            .get_or_init(|| crate::trakt::Trakt::new(self.http().clone(), self.library()))
    }

    /// A fresh hlc. Local library writes take `clock()` instead, so they are
    /// stamped inside their write transaction (`mq_library::Stamp`).
    pub fn hlc(&self) -> String {
        self.clock().tick(self.clock().wall_now()).encode()
    }

    /// The lowest hlc of this device, for rows that only cache what it saw.
    pub fn zero_hlc(&self) -> String {
        Hlc::zero(self.clock().device()).encode()
    }

    /// One sync round with the relay: push local changes, pull and apply
    /// remote ones. Views refresh through the usual change notifications.
    pub async fn sync(
        &self,
        config: &mq_sync::SyncConfig,
    ) -> Result<mq_sync::SyncReport, mq_sync::SyncError> {
        let _gate = self.sync_gate().await;
        mq_sync::sync_once(self.library(), self.http(), config, self.clock()).await
    }

    /// Held for the whole of a sync round or a roster change: two at once
    /// would race over the outbox, the cursors and the roster pin.
    pub async fn sync_gate(&self) -> OwnedMutexGuard<()> {
        self.inner.device.sync_gate.clone().lock_owned().await
    }

    /// The OS is short of memory: drop what can be read again (the addon
    /// responses in memory; their disk copies stay).
    pub fn on_memory_pressure(&self) {
        self.addons().forget_memory();
    }

    /// Back in the foreground: a stream server whose listener the system
    /// took while the app was suspended binds again (journey review
    /// 2026-10-20, Apple P1-1), so later sources and Try again play. True
    /// when its address moved: URLs handed out before are dead.
    pub async fn on_foreground(&self) -> bool {
        let Some(server) = self.inner.server.get() else {
            return false;
        };
        match server.revive().await {
            Ok(moved) => moved,
            Err(e) => {
                tracing::warn!("stream server not bound again: {e}");
                false
            }
        }
    }

    /// The device's network changed: connections pooled on the old one are
    /// dropped (each would hold a request until its timeout), and so are the
    /// trackers the engine kept addresses and ids for. `network` is its type,
    /// when the app knows it.
    pub fn on_network_changed(&self, network: Option<crate::speed::Network>) {
        self.http().network_changed();
        #[cfg(feature = "torrent")]
        if let Some(engine) = self.inner.engine.get() {
            engine.network_changed();
        }
        if let Some(network) = network {
            self.set_network(network);
        }
    }

    /// The network type speed is measured and read for: the one the app
    /// last reported (`versions`, `networkChanged`), kept in the device's
    /// store so a restart does not read "other" until the app reports again.
    pub fn network(&self) -> crate::speed::Network {
        crate::speed::last_network(self.library())
    }

    pub fn set_network(&self, network: crate::speed::Network) {
        crate::speed::remember_network(self.library(), network);
    }

    /// This build has the torrent engine.
    pub fn has_engine(&self) -> bool {
        cfg!(feature = "torrent")
    }

    /// Free space where the engine keeps torrents (the nearest folder that
    /// exists); `None` without the engine or where it cannot be read.
    pub fn free_bytes(&self) -> Option<u64> {
        if let Some(&bytes) = self.inner.free_bytes.get() {
            return Some(bytes);
        }
        #[cfg(all(unix, feature = "torrent"))]
        {
            let mut dir = self.inner.torrent_dir.as_path();
            while !dir.exists() {
                dir = dir.parent()?;
            }
            mq_engine::free_space(dir)
        }
        #[cfg(not(all(unix, feature = "torrent")))]
        None
    }

    /// The measured speed of `kind` on the current network, or its prior.
    pub fn speed(&self, kind: crate::speed::Kind) -> mq_streams::Speed {
        let now = mq_library::hlc::now_ms();
        crate::speed::estimate(self.library(), self.network(), kind, now)
    }

    /// Tests: the ranking, Settings and the engine's cache read `bytes` as
    /// the free space, so no test depends on the host's disk. Set it once,
    /// before opening a view.
    #[doc(hidden)]
    pub fn set_free_bytes(&self, bytes: u64) {
        let _ = self.inner.free_bytes.set(bytes);
    }

    /// Tests: `live_seeders` answers from `counts` (lowercase info hash to
    /// seeders; a hash left out has no answer), in any build. Set it before
    /// opening a view.
    #[doc(hidden)]
    pub fn set_live_seeders(&self, counts: HashMap<String, u32>) {
        *self.fake_seeders() = Some(FakeSeeders { counts, misses: 0 });
    }

    /// Tests: the next `times` asks of the fake counts answer nothing, as
    /// a cold first scrape does.
    #[doc(hidden)]
    pub fn miss_live_seeders(&self, times: u32) {
        if let Some(fake) = self.fake_seeders().as_mut() {
            fake.misses = times;
        }
    }

    fn fake_seeders(&self) -> std::sync::MutexGuard<'_, Option<FakeSeeders>> {
        self.inner
            .live_seeders
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Live seeders can be asked here: the engine counts swarms.
    pub fn counts_swarms(&self) -> bool {
        self.fake_seeders().is_some() || self.has_engine()
    }

    /// The seeders of `torrents` (lowercase info hash, the stream's
    /// trackers) as their trackers count them now, within about a second;
    /// `None` where no answer told anything (`Engine::scrape`: a default
    /// tracker's 0/0 does not), and for every one without the engine.
    pub async fn live_seeders(&self, torrents: &[(String, Vec<String>)]) -> Vec<Option<u32>> {
        if let Some(fake) = self.fake_seeders().as_mut() {
            if fake.misses > 0 {
                fake.misses -= 1;
                return vec![None; torrents.len()];
            }
            return torrents
                .iter()
                .map(|(h, _)| fake.counts.get(h).copied())
                .collect();
        }
        #[cfg(feature = "torrent")]
        if let Ok(engine) = self.torrent_engine().await {
            let asked: Vec<(&str, &[String])> = torrents
                .iter()
                .map(|(h, t)| (h.as_str(), t.as_slice()))
                .collect();
            let counts = engine.scrape(&asked).await;
            return counts.into_iter().map(|c| c.map(|c| c.seeders)).collect();
        }
        vec![None; torrents.len()]
    }

    /// Readies the trackers for the scrape that usually follows: a title's
    /// page opened, its versions next (crates/mq-engine README, "The cold
    /// first scrape"). Starts the engine. Nothing without it, and nothing
    /// for a core whose counts are given (tests).
    pub async fn warm_swarms(&self) {
        let given = self.fake_seeders().is_some();
        #[cfg(feature = "torrent")]
        if !given && let Ok(engine) = self.torrent_engine().await {
            engine.warm_trackers().await;
        }
        #[cfg(not(feature = "torrent"))]
        let _ = given;
    }

    /// This device's connection and disk, for Settings.
    pub fn stream_conditions(&self) -> crate::speed::StreamConditions {
        crate::speed::StreamConditions {
            network: self.network(),
            http: self.speed(crate::speed::Kind::Http),
            p2p: self.speed(crate::speed::Kind::P2p),
            free_bytes: self.free_bytes(),
            engine: self.has_engine(),
        }
    }

    /// "Measure again": the current network's estimates go back to their
    /// priors.
    pub fn reset_speed(&self) -> crate::speed::StreamConditions {
        crate::speed::reset(self.library(), self.network());
        self.stream_conditions()
    }

    /// The clock that stamps this device's writes (sync and roster calls
    /// take it).
    pub fn clock(&self) -> &HlcClock {
        &self.inner.device.clock
    }

    /// The loopback stream server, started on first use.
    pub async fn stream_server(&self) -> Result<Arc<StreamServer>, CoreError> {
        let server = self
            .inner
            .server
            .get_or_try_init(|| async {
                let server = StreamServer::start(self.http().clone()).await?;
                server.set_logos(defaults::LOGOS);
                Ok::<_, std::io::Error>(Arc::new(server))
            })
            .await?;
        Ok(server.clone())
    }

    /// The embedded torrent engine, started on first use (full builds only).
    #[cfg(feature = "torrent")]
    pub async fn torrent_engine(&self) -> Result<mq_engine::Engine, CoreError> {
        let engine = self
            .inner
            .engine
            .get_or_try_init(|| async {
                let engine = mq_engine::Engine::start(self.engine_config())
                    .await
                    .map_err(|e| CoreError::Torrent(e.to_string()))?;
                // The probe, once per launch, unless core open ran it
                // (2.6).
                if self.inner.punches.get().is_none() {
                    let dir = self.inner.torrent_dir.clone();
                    let punches = tokio::task::spawn_blocking(move || {
                        mq_engine::librqbit::punch_supported(&dir)
                    })
                    .await
                    .unwrap_or(false);
                    let _ = self.inner.punches.set(punches);
                }
                Ok::<_, CoreError>(engine)
            })
            .await?;
        Ok(engine.clone())
    }

    /// A torrent playback may play in a window (plans/streaming-window.md
    /// 2.6): the torrent folder's volume punches holes, and the engine runs
    /// DWH with upload off (as the app's always does).
    #[cfg(feature = "torrent")]
    pub(crate) fn windows_ok(&self) -> bool {
        self.inner.punches.get().copied().unwrap_or(false)
    }

    /// `R` and `C` for the ranking (plans/streaming-window.md 3.1): the
    /// engine's last count, or before it starts every torrent folder (none
    /// is in use) and no window.
    #[cfg(feature = "torrent")]
    pub(crate) fn room_counts(&self) -> (Option<u64>, u64) {
        match self.inner.engine.get() {
            Some(engine) => {
                let (r, c) = engine.counted();
                (Some(r), c)
            }
            None => (Some(mq_engine::reclaimable(&self.inner.torrent_dir)), 0),
        }
    }

    /// Tests: the probe's answer, before the engine starts.
    #[cfg(feature = "torrent")]
    #[doc(hidden)]
    pub fn set_punches(&self, punches: bool) {
        let _ = self.inner.punches.set(punches);
    }

    /// Walk builds: the window every torrent playback gets
    /// (plans/streaming-window.md 4).
    #[cfg(feature = "torrent")]
    pub(crate) fn walk_window(&self) -> Option<u64> {
        self.inner.walk.and_then(|w| w.window)
    }

    /// How the app's engine runs. No exception for a LAN or an addon's host:
    /// one engine serves every addon (plans/c27-torrent-policy.md 3.2). It
    /// uploads nothing: seeding the torrents viewers stream is what draws
    /// legal notices, and every live run played as well without it. The
    /// cache holds one file at the 5 GB default plus read-ahead, at most half
    /// the free space (plans/m13-stream-choice.md 5).
    #[cfg(feature = "torrent")]
    fn engine_config(&self) -> mq_engine::EngineConfig {
        let budget: u64 = 6 << 30;
        mq_engine::EngineConfig {
            upload: false,
            cache_budget_bytes: self.free_bytes().map_or(budget, |f| budget.min(f / 2)),
            ..mq_engine::EngineConfig::new(&self.inner.torrent_dir, self.http().torrent_scope(&[]))
        }
    }

    /// Uses `engine` as the device's torrent engine, when none started yet
    /// (tests run one on loopback with discovery off).
    #[cfg(feature = "torrent")]
    #[doc(hidden)]
    pub fn set_torrent_engine(&self, engine: mq_engine::Engine) {
        let _ = self.inner.engine.set(engine);
    }

    pub(crate) fn accounts(&self) -> &crate::debrid::Accounts {
        &self.inner.debrid
    }

    /// Installed addons in the user's order. Rows whose manifest no longer
    /// parses are skipped rather than failing the whole list. A kids profile
    /// reads the owner's list without adult-flagged addons; its own list is
    /// unused (plans/m11-kids.md 4.6). When the profile cannot be read it
    /// reads its own list: the owner's, whose addresses can hold keys, only
    /// for a profile known to be kids.
    pub fn installed_addons(&self) -> Result<Vec<InstalledAddon>, CoreError> {
        let kids = matches!(
            self.library().policy_checked(self.profile()),
            Ok((policy, _)) if policy.kids()
        );
        let rows = self
            .library()
            .addons(if kids { OWNER } else { self.profile() })?;
        Ok(rows
            .into_iter()
            .filter(|r| r.enabled)
            .filter_map(|r| {
                let manifest: Manifest = serde_json::from_str(&r.manifest_json).ok()?;
                // The row's `allow_private` grants nothing: sync may set it.
                Some(InstalledAddon {
                    transport_url: url::Url::parse(&r.transport_url).ok()?,
                    manifest,
                })
            })
            .filter(|a| !(kids && a.manifest.behavior_hints.adult))
            .collect())
    }

    /// Details for one title: the first answer in addon order wins, as in
    /// Stremio; later addons are asked only when earlier ones fail. Returns the
    /// meta and the name of the addon that gave it.
    pub async fn fetch_meta(&self, kind: &str, id: &str) -> Result<(MetaItem, String), String> {
        self.fetch_meta_body(kind, id).await.map_err(|m| m.reason)
    }

    /// As `fetch_meta`; a failure also says whether no addon answered.
    pub(crate) async fn fetch_meta_or_miss(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<MetaItem, MetaMiss> {
        let (meta, _) = self.fetch_meta_body(kind, id).await?;
        Ok(meta)
    }

    /// Each episode's TVDB id by video id, from every meta addon in order
    /// (served from the addon cache after the first look): an earlier addon's
    /// id wins, a later one fills the episodes it lacks (a season it does not
    /// list yet). stremio-core's `Video` has no such field, so it is read
    /// from the raw JSON; a number or a numeric string is read.
    pub async fn episode_tvdb_ids(&self, id: &str) -> Result<HashMap<String, u64>, TvdbMiss> {
        let path = ResourcePath {
            resource: "meta".into(),
            r#type: "series".into(),
            id: id.to_owned(),
            extra: vec![],
        };
        let addons = self
            .installed_addons()
            .map_err(|e| TvdbMiss::Unavailable(e.to_string()))?;
        let mut reason = "None of your addons has details for this title".to_owned();
        // Every addon asked answered that it has no such title.
        let mut unknown = None;
        let mut ids = HashMap::new();
        // Addons asked, and those that gave no answer at all.
        let (mut asked, mut silent) = (0, 0);
        for addon in addons.iter().filter(|a| a.supports(&path)) {
            let name = &addon.manifest.name;
            asked += 1;
            let raw = match self.addons().fetch(addon, &path, META_TIMEOUT).await {
                Ok(fetched) => serde_json::from_slice::<serde_json::Value>(&fetched.body).ok(),
                Err(AddonError::Http(mq_http::HttpError::Status {
                    status: 404 | 410, ..
                })) => {
                    unknown.get_or_insert(true);
                    reason = format!("{name} has no details for this title");
                    continue;
                }
                Err(e) => {
                    unknown = Some(false);
                    silent += usize::from(unanswered(&e));
                    reason = meta_failure(name, &e);
                    continue;
                }
            };
            let Some(raw) = raw else {
                unknown = Some(false);
                reason = format!("{name} sent details Marquee cannot read");
                continue;
            };
            if !raw["meta"].is_object() {
                unknown.get_or_insert(true);
                reason = format!("{name} has no details for this title");
                continue;
            }
            unknown = Some(false);
            let before = ids.len();
            for v in raw["meta"]["videos"].as_array().into_iter().flatten() {
                let tvdb = &v["tvdb_id"];
                let tvdb = tvdb.as_u64().or_else(|| tvdb.as_str()?.trim().parse().ok());
                if let (Some(video), Some(tvdb)) = (v["id"].as_str(), tvdb) {
                    ids.entry(video.to_owned()).or_insert(tvdb);
                }
            }
            if ids.len() == before {
                reason = format!("{name} gave no TVDB ids for this title");
            }
        }
        match unknown {
            _ if !ids.is_empty() => Ok(ids),
            Some(true) => Err(TvdbMiss::Unknown(reason)),
            _ if asked > 0 && silent == asked => Err(TvdbMiss::Unreached(reason)),
            _ => Err(TvdbMiss::Unavailable(reason)),
        }
    }

    /// The first meta answer in addon order that decodes, with the addon's
    /// name. Otherwise the last addon's failure as the title page shows it.
    /// For kids (plans/m11-kids.md 4.2) an answer for another kind or id is
    /// that addon failing, and videos not of the title are dropped, so no
    /// consumer ever sees them.
    async fn fetch_meta_body(&self, kind: &str, id: &str) -> Result<(MetaItem, String), MetaMiss> {
        let path = ResourcePath {
            resource: "meta".into(),
            r#type: kind.to_owned(),
            id: id.to_owned(),
            extra: vec![],
        };
        let addons = self.installed_addons().map_err(|e| MetaMiss {
            reason: e.to_string(),
            unreached: false,
        })?;
        let kids = self.policy().kids();
        let mut last_error = "None of your addons has details for this title".to_owned();
        // Addons asked, and those that gave no answer at all.
        let (mut asked, mut silent) = (0, 0);
        for addon in addons.iter().filter(|a| a.supports(&path)) {
            let name = &addon.manifest.name;
            asked += 1;
            match self.addons().fetch(addon, &path, META_TIMEOUT).await {
                Ok(fetched) => match serde_json::from_slice::<ResourceResponse>(&fetched.body) {
                    Ok(ResourceResponse::Meta { meta })
                        if kids && (meta.preview.id != id || meta.preview.r#type != kind) =>
                    {
                        last_error = format!("{name} sent details for another title");
                    }
                    Ok(ResourceResponse::Meta { mut meta }) => {
                        if kids {
                            meta.videos.retain(|v| Policy::belongs(id, &v.id));
                        }
                        return Ok((meta, name.clone()));
                    }
                    Ok(_) => last_error = format!("{name} has no details for this title"),
                    Err(e) => {
                        tracing::debug!("{name} sent unreadable details: {e}");
                        last_error = format!("{name} sent details Marquee cannot read");
                    }
                },
                Err(e) => {
                    // An HTTP error's text can carry the addon's URL, so only its kind.
                    match &e {
                        AddonError::Http(h) => {
                            tracing::debug!("{name} gave no details: {}", h.kind())
                        }
                        other => tracing::debug!("{name} gave no details: {other}"),
                    }
                    silent += usize::from(unanswered(&e));
                    last_error = meta_failure(name, &e);
                }
            }
        }
        Err(MetaMiss {
            reason: last_error,
            unreached: asked > 0 && silent == asked,
        })
    }

    /// Fetches the manifest and appends the addon to the profile's list, on.
    /// A link already installed gets the new manifest in place instead, its
    /// position and whether it is on kept (Stremio's `UpgradeAddon`).
    pub async fn install_addon(&self, transport_url: &str) -> Result<InstalledAddon, CoreError> {
        let addon = self.addons().fetch_manifest(transport_url).await?;
        let manifest_json = serde_json::to_string(&addon.manifest)
            .map_err(|e| CoreError::BadManifest(e.to_string()))?;
        let url = addon.transport_url.to_string();
        self.edit_addons(|rows| {
            put_addon(rows, &url, manifest_json);
            Ok(())
        })?;
        Ok(addon)
    }

    /// Fetches the manifest of every installed addon of every profile again,
    /// at most once a day, and puts each one that changed in place, as
    /// `install_addon` does. A link that fails keeps its manifest; a round
    /// in which none answered counts as not done. `now_ms` is the time the
    /// day is counted from. Returns how many rows changed.
    pub async fn refresh_addons(&self, now_ms: u64) -> Result<usize, CoreError> {
        let library = self.library();
        let last = library.device_value(ADDONS_REFRESHED)?;
        if last
            .and_then(|v| v.parse::<u64>().ok())
            .is_some_and(|last| now_ms < last.saturating_add(REFRESH_EVERY_MS))
        {
            return Ok(0);
        }
        let profiles: Vec<String> = library.profiles()?.into_iter().map(|p| p.id).collect();
        let mut links: Vec<String> = Vec::new();
        for profile in &profiles {
            for row in library.addons(profile)? {
                // The built-in source runs here: its old host is never asked.
                let builtin = crate::turkcealtyazi::is_builtin(
                    &row.transport_url,
                    manifest_id(&row.manifest_json).as_deref(),
                );
                if !builtin && !links.contains(&row.transport_url) {
                    links.push(row.transport_url);
                }
            }
        }
        let fetched = futures_util::future::join_all(links.iter().map(|link| async move {
            let addon = self.addons().fetch_manifest(link).await.ok()?;
            let json = serde_json::to_string(&addon.manifest).ok()?;
            Some((link.as_str(), json))
        }))
        .await;
        let manifests: HashMap<&str, String> = fetched.into_iter().flatten().collect();
        if manifests.is_empty() && !links.is_empty() {
            return Ok(0);
        }
        library.set_device_value(ADDONS_REFRESHED, &now_ms.to_string())?;
        let mut changed = 0;
        for profile in &profiles {
            let newer = |row: &AddonRow| {
                let fetched = manifests.get(row.transport_url.as_str());
                fetched.filter(|json| **json != row.manifest_json)
            };
            // No write, and nothing to sync, when nothing changed.
            if !library
                .addons(profile)?
                .iter()
                .any(|row| newer(row).is_some())
            {
                continue;
            }
            // Not an edit: the manifests change in place, the list keeps its
            // stamp and nothing is pushed (`refresh_addon_manifests`), so a
            // refresh never changes which device's list wins.
            let fresh: Vec<(String, String)> = library
                .addons(profile)?
                .iter()
                .filter_map(|row| newer(row).map(|json| (row.transport_url.clone(), json.clone())))
                .collect();
            let written = library
                .refresh_addon_manifests(profile, &fresh)
                .map_err(CoreError::from);
            match written {
                Ok(n) => changed += n,
                // A profile removed meanwhile, or a failed write: the next day tries again.
                Err(e) => tracing::debug!(
                    "addon refresh for a profile not written: {}",
                    error_kind(&e)
                ),
            }
        }
        Ok(changed)
    }

    /// Removes the addon; the built-in source's rows all go together.
    pub fn remove_addon(&self, transport_url: &str) -> Result<(), CoreError> {
        self.edit_addons(|rows| {
            for at in together(rows, position(rows, transport_url)?)
                .into_iter()
                .rev()
            {
                rows.remove(at);
            }
            Ok(())
        })
    }

    /// Moves an addon to `to` in the list (clamped); order decides which
    /// addon answers first.
    pub fn move_addon(&self, transport_url: &str, to: usize) -> Result<(), CoreError> {
        self.edit_addons(|rows| {
            let row = rows.remove(position(rows, transport_url)?);
            rows.insert(to.min(rows.len()), row);
            Ok(())
        })
    }

    /// The default addons and whether each is in this profile's list, by
    /// manifest id or its own link (plans/m13-default-addons.md 5).
    pub async fn default_addons(&self) -> Result<DefaultAddons, CoreError> {
        let rows = self.library().addons(self.profile())?;
        // No server (it could not bind): the apps show initials.
        let server = self.stream_server().await.ok();
        let state = |d: &defaults::DefaultAddon| {
            default_rows(&rows, d)
                .into_iter()
                .map(|i| rows[i].enabled)
                .reduce(|a, b| a || b)
                .map_or(DefaultInstalled::Absent, |on| {
                    if on {
                        DefaultInstalled::On
                    } else {
                        DefaultInstalled::Off
                    }
                })
        };
        Ok(DefaultAddons {
            version: defaults::LIST_VERSION,
            addons: defaults::list()
                .iter()
                .map(|d| DefaultAddonInfo {
                    key: d.key.clone(),
                    name: d.name.clone(),
                    line: d.line.clone(),
                    group: d.group.clone(),
                    installed: state(d),
                    logo: server.as_ref().and_then(|s| s.logo_url(&d.key)),
                })
                .collect(),
        })
    }

    /// Installs the defaults `choices` names, on or off as chosen, in one
    /// write with their bundled manifests: no network. Only named keys are
    /// touched: a default already in the list (by manifest id, or still on
    /// its own link) takes its switch, an absent one is appended, and a
    /// key left out is never added or switched (the Review card names only
    /// the missing ones; a resent call never brings back what a pull
    /// removed). A profile that never had a list gets wall time zero, so
    /// any list a person built on another device wins over it: all nine on
    /// is this device's zero hlc (it ties with other untouched defaults),
    /// any other answer counter 1 (it beats untouched defaults only). A
    /// list that was there makes it an edit. A call that changes nothing (a
    /// retried Continue) writes nothing. The profile has then seen this
    /// list version (`defaultsSeen`), and the live manifests are fetched in
    /// the background.
    pub fn install_defaults(&self, choices: &[DefaultChoice]) -> Result<(), CoreError> {
        if self.policy().kids() {
            return Err(CoreError::Defaults(
                "a kids profile uses the owner's addons".into(),
            ));
        }
        if let Some(c) = choices
            .iter()
            .find(|c| !defaults::list().iter().any(|d| d.key == c.key))
        {
            return Err(CoreError::Defaults(format!("no default named {}", c.key)));
        }
        if choices.is_empty() {
            return Ok(());
        }
        let chosen = |key: &str| choices.iter().find(|c| c.key == key).map(|c| c.on);
        let edit = |rows: &mut Vec<AddonRow>| {
            for d in defaults::list() {
                let Some(on) = chosen(&d.key) else { continue };
                let present = default_rows(rows, d);
                if present.is_empty() {
                    let json = serde_json::to_string(&d.manifest)
                        .map_err(|e| CoreError::BadManifest(e.to_string()))?;
                    put_addon(rows, &d.url, json);
                    let at = position(rows, &d.url)?;
                    rows[at].enabled = on;
                } else {
                    for i in present {
                        rows[i].enabled = on;
                    }
                }
            }
            Ok(())
        };
        if self.library().addon_list_hlc(self.profile())?.is_none() {
            let all_on = defaults::list()
                .iter()
                .all(|d| chosen(&d.key) == Some(true));
            let first = Hlc {
                wall_ms: 0,
                counter: if all_on { 0 } else { 1 },
                device: self.clock().device(),
            };
            self.library()
                .edit_addons(self.profile(), &first.encode(), edit)?;
        } else {
            self.edit_addons(edit)?;
        }
        if self.prefs().defaults_seen < defaults::LIST_VERSION {
            self.set_prefs(&serde_json::json!({ "defaultsSeen": defaults::LIST_VERSION }))?;
        }
        // The live manifests now, not in a day: bundled ones may be older.
        self.library().set_device_value(ADDONS_REFRESHED, "0")?;
        if tokio::runtime::Handle::try_current().is_ok() {
            let core = self.clone();
            tokio::spawn(async move {
                let now = mq_library::hlc::now_ms();
                if let Err(e) = core.refresh_addons(now).await {
                    tracing::debug!("addon refresh after the defaults: {}", error_kind(&e));
                }
            });
        }
        Ok(())
    }

    /// Removes this device's profiles and library (a sign-out that takes
    /// them, D13): every table goes, then the device is as new, its owner
    /// profile again and the defaults step first. Returns the profiles that
    /// were here, for the apps to drop their keystore entries (each one's
    /// Trakt record). After the leave: nothing of it is sent.
    pub fn wipe(&self) -> Result<Vec<String>, CoreError> {
        let library = self.library();
        let ids: Vec<String> = library.profiles()?.into_iter().map(|p| p.id).collect();
        library.wipe()?;
        // Their Trakt rounds stop and their tokens leave memory: the new
        // owner is `default` again and must not go on with the old one's.
        for id in &ids {
            self.trakt().forget(id);
        }
        library.set_device_value(DEFAULTS_ERA, "fresh")?;
        let first = Hlc::zero(self.clock().device()).encode();
        library.ensure_profile(OWNER, OWNER_NAME, &first)?;
        Ok(ids)
    }

    /// This device joined a sync account (paired or recovered): it gets the
    /// account's addons by sync and never starts at the defaults step, also
    /// before its first sync stores the roster.
    pub fn joined_sync_account(&self) {
        // Announced as an addons change: an open addons view drops `fresh`.
        if let Err(e) =
            self.library()
                .set_device_value_announced(DEFAULTS_ERA, "joined", Table::Addons)
        {
            tracing::warn!("could not mark the sync join: {e}");
        }
    }

    /// This device created a sync account: a genesis receives no addon list,
    /// so a device that started fresh still gets the recommended addons once
    /// its roster is stored (plans/accounts.md 3.3).
    pub fn created_sync_account(&self) {
        let library = self.library();
        let fresh = library
            .device_value(DEFAULTS_ERA)
            .is_ok_and(|era| era.as_deref() == Some("fresh"));
        if fresh && let Err(e) = library.set_device_value(DEFAULTS_ERA, "created") {
            tracing::warn!("could not mark the sync account made here: {e}");
        }
    }

    /// Whether this profile starts at the defaults step: it never had an
    /// addon list, this device belongs to no sync account it did not create
    /// (a joined device gets the account's list), and it is not a kids
    /// profile (it reads the owner's list).
    pub fn addons_fresh(&self) -> bool {
        let library = self.library();
        let era = library.device_value(DEFAULTS_ERA).ok().flatten();
        let starts = match era.as_deref() {
            Some("fresh") => library.in_sync_account().is_ok_and(|joined| !joined),
            Some("created") => true,
            _ => false,
        };
        starts
            && library
                .addon_list_hlc(self.profile())
                .is_ok_and(|h| h.is_none())
            && !self.policy().kids()
    }

    /// Turns the addon on or off; the built-in source's rows switch
    /// together, so they never disagree.
    pub fn set_addon_enabled(&self, transport_url: &str, enabled: bool) -> Result<(), CoreError> {
        self.edit_addons(|rows| {
            for at in together(rows, position(rows, transport_url)?) {
                rows[at].enabled = enabled;
            }
            Ok(())
        })
    }

    /// Edits the profile's addon list as stored, in one library transaction
    /// (`Library::edit_addons`): no pull or other edit lands between the
    /// read and the write.
    pub(crate) fn edit_addons(
        &self,
        f: impl FnOnce(&mut Vec<AddonRow>) -> Result<(), CoreError>,
    ) -> Result<(), CoreError> {
        self.library().edit_addons(self.profile(), self.clock(), f)
    }
}

/// Device state key of the time installed manifests were last refreshed.
const ADDONS_REFRESHED: &str = "addons_refreshed_ms";
/// Device state key: "fresh" on a device first opened with default addons,
/// "legacy" on one that existed before (its profiles never start at the
/// defaults step; Home offers the new ones once).
const DEFAULTS_ERA: &str = "addon_defaults";
/// Installed manifests are fetched again at most this often.
const REFRESH_EVERY_MS: u64 = 24 * 60 * 60 * 1000;

/// A library error's kind for logs: never a link, which can carry keys.
fn error_kind(e: &CoreError) -> &'static str {
    match e {
        CoreError::Library(_) => "library",
        _ => "other",
    }
}

/// Row `at`, or every row of the built-in source when it is one of them.
fn together(rows: &[AddonRow], at: usize) -> Vec<usize> {
    let builtin = |row: &AddonRow| {
        crate::turkcealtyazi::is_builtin(
            &row.transport_url,
            manifest_id(&row.manifest_json).as_deref(),
        )
    };
    if !builtin(&rows[at]) {
        return vec![at];
    }
    (0..rows.len()).filter(|&i| builtin(&rows[i])).collect()
}

/// The rows of a default: by manifest id, or still on the default's own
/// link after a refresh changed its id.
fn default_rows(rows: &[AddonRow], d: &defaults::DefaultAddon) -> Vec<usize> {
    (0..rows.len())
        .filter(|&i| {
            rows[i].transport_url == d.url
                || manifest_id(&rows[i].manifest_json).as_deref() == Some(d.manifest.id.as_str())
        })
        .collect()
}

/// Puts a fetched manifest in the list: in place over the row the link
/// names (position, whether it is on and the stored link kept), else at the
/// end, on.
pub(crate) fn put_addon(rows: &mut Vec<AddonRow>, url: &str, manifest_json: String) {
    if let Ok(at) = position(rows, url) {
        rows[at].manifest_json = manifest_json;
        return;
    }
    // A configured copy of a default (its manifest id, another link) takes
    // the row of that default while it still has the default's own link,
    // its place and switch kept: configuring Torrentio or Trakt on the
    // addon's site never leaves two of them. A row the user moved to
    // another link is never touched.
    let default = manifest_id(&manifest_json).and_then(|id| defaults::by_id(&id));
    if let Some(at) = default.and_then(|d| rows.iter().position(|r| r.transport_url == d.url)) {
        rows[at].transport_url = url.to_owned();
        rows[at].manifest_json = manifest_json;
        return;
    }
    rows.push(AddonRow {
        transport_url: url.to_owned(),
        manifest_json,
        position: 0,
        enabled: true,
        // Grants nothing here (`AddonClient::client_for` reads the
        // link); written for older app versions, which still read it.
        allow_private: url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(mq_http::is_private_host))
            .unwrap_or(false),
    });
}

/// The id in a stored manifest.
fn manifest_id(manifest_json: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Id {
        id: String,
    }
    serde_json::from_str::<Id>(manifest_json).ok().map(|m| m.id)
}

/// The installed addon a command names (`find_row`).
pub(crate) fn position(rows: &[AddonRow], transport_url: &str) -> Result<usize, CoreError> {
    let canonicals: Vec<String> = rows.iter().map(|r| canonical(&r.transport_url)).collect();
    let rows = rows.iter().zip(&canonicals);
    find_row(
        rows.map(|(r, c)| (r.transport_url.as_str(), c.as_str(), r.enabled)),
        transport_url,
    )
    .ok_or_else(|| CoreError::NotInstalled(redact(transport_url)))
}

/// The row a link names, one rule for the addon commands and Find addons'
/// `installed`: the exact stored link, else a row with the same canonical
/// link (the Stremio importer keeps fragments the lists drop), an enabled
/// one first. Rows are `(stored link, canonical link, enabled)`.
pub(crate) fn find_row<'a>(
    rows: impl IntoIterator<Item = (&'a str, &'a str, bool)>,
    link: &str,
) -> Option<usize> {
    let wanted = canonical(link);
    let (mut same, mut same_on) = (None, None);
    for (at, (stored, canonical, enabled)) in rows.into_iter().enumerate() {
        if stored == link {
            return Some(at);
        }
        if canonical == wanted {
            same.get_or_insert(at);
            if enabled {
                same_on.get_or_insert(at);
            }
        }
    }
    same_on.or(same)
}

/// The canonical form of a link, or the link as given.
pub(crate) fn canonical(transport_url: &str) -> String {
    mq_addons::normalize_transport_url(transport_url)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| transport_url.to_owned())
}

/// A random device id kept in the library on first open.
fn stored_device_id(library: &Library) -> Result<u64, CoreError> {
    if let Some(id) = library
        .sync_state("device_id")?
        .and_then(|s| s.parse().ok())
    {
        return Ok(id);
    }
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("os randomness");
    // Small ids are left for tests and the Stremio import.
    let id = u64::from_le_bytes(bytes) | (1 << 63);
    library.set_sync_state("device_id", &id.to_string())?;
    Ok(id)
}

/// Transport URLs often carry API keys (debrid tokens in Torrentio configs), so
/// errors that reach a UI or a log show only the redacted form.
fn redact(transport_url: &str) -> String {
    url::Url::parse(transport_url)
        .map(|u| mq_http::redact_url(&u))
        .unwrap_or_else(|_| "<invalid url>".into())
}

/// Persists addon responses in the library database so Home can render from
/// cache on the next launch while it revalidates.
struct LibraryCache(Arc<Library>);

impl CacheStore for LibraryCache {
    fn load(&self, key: &str) -> Option<StoredResponse> {
        let row = self.0.cache_get(key).ok()??;
        let secs = |s: i64| std::time::Duration::from_secs(s.max(0) as u64);
        Some(StoredResponse {
            body: row.body.into(),
            stored_ms: row.stored_ms.max(0) as u64,
            control: CacheControl {
                max_age: secs(row.max_age_s),
                stale_revalidate: secs(row.stale_revalidate_s),
                stale_error: secs(row.stale_error_s),
            },
        })
    }

    fn save(&self, key: &str, value: &StoredResponse, generation: u64) {
        let row = mq_library::CachedResponse {
            body: value.body.to_vec(),
            stored_ms: value.stored_ms as i64,
            max_age_s: value.control.max_age.as_secs() as i64,
            stale_revalidate_s: value.control.stale_revalidate.as_secs() as i64,
            stale_error_s: value.control.stale_error.as_secs() as i64,
        };
        if let Err(e) = self.0.cache_put(key, &row, generation) {
            tracing::warn!("http cache write failed: {e}");
        }
    }

    /// Every open of the data directory shares the library, so a removal
    /// anywhere retires every memory cache's older answers.
    fn generation(&self) -> u64 {
        self.0.removals()
    }
}

/// Tests: live seeders answered instead of the trackers'.
struct FakeSeeders {
    counts: HashMap<String, u32>,
    /// Asks still to answer with nothing.
    misses: u32,
}

/// No answer came at all: no connection (no network, no route, a name that
/// does not resolve) or none in time.
fn unanswered(error: &AddonError) -> bool {
    use mq_http::HttpError;
    matches!(
        error,
        AddonError::TimedOut | AddonError::Http(HttpError::Timeout(_) | HttpError::Network(_))
    )
}

/// Why an addon gave no details, in a sentence the title page shows as is,
/// worded as the playback attempt list words a source's answer
/// (`mq_server::refused`), with the status kept for support.
fn meta_failure(name: &str, error: &AddonError) -> String {
    use mq_http::HttpError;
    match error {
        AddonError::TimedOut | AddonError::Http(HttpError::Timeout(_)) => {
            format!("{name} did not answer in time")
        }
        AddonError::Http(HttpError::Status { status, .. }) => match status {
            404 | 410 => format!("{name} has no details for this title ({status})"),
            401 | 403 => format!("{name} refused access ({status})"),
            429 => format!("{name} is busy, try again soon ({status})"),
            500..=599 => format!("{name} had an error ({status})"),
            _ => format!("{name} answered with an error ({status})"),
        },
        AddonError::Http(HttpError::BlockedPrivate(_)) => {
            format!("{name} points to a private network address")
        }
        AddonError::Http(HttpError::TooLarge { .. } | HttpError::Decode(_)) => {
            format!("{name} sent details Marquee cannot read")
        }
        _ => format!("{name} could not be reached"),
    }
}

/// The walk-only setting (plans/streaming-window.md 4): debug builds read
/// `walk-window` in the data folder, one or two numbers: the window `W` in
/// bytes for every torrent playback (0 for none, a file that fits then
/// plays whole), and the watchdog's free-space line (700 MB with a window,
/// 480 MB without). Release builds never read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Walk {
    window: Option<u64>,
    line: u64,
}

impl Walk {
    fn read(data_dir: &Path) -> Option<Self> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let text = std::fs::read_to_string(data_dir.join("walk-window")).ok()?;
        Self::parse(&text)
    }

    fn parse(text: &str) -> Option<Self> {
        let mut n = text.split_whitespace().map(str::parse::<u64>);
        let window = Some(n.next()?.ok()?).filter(|&w| w > 0);
        let line = match n.next() {
            Some(line) => line.ok()?,
            None if window.is_some() => 700_000_000,
            None => 480_000_000,
        };
        Some(Self { window, line })
    }

    /// The watchdog: every second the free space where torrents are kept,
    /// logged; under the line the process stops, playback and engine with
    /// it (our writes never take free space under the 512 MiB reserve).
    fn watch(self, torrent_dir: PathBuf) {
        tracing::info!(window = ?self.window, line = self.line, "walk setting");
        std::thread::spawn(move || {
            loop {
                let mut dir = torrent_dir.as_path();
                while !dir.exists() {
                    match dir.parent() {
                        Some(up) => dir = up,
                        None => break,
                    }
                }
                #[cfg(feature = "torrent")]
                if let Some(free) = mq_engine::free_space(dir) {
                    tracing::info!(free, "walk free space");
                    if free < self.line {
                        tracing::error!(free, line = self.line, "walk watchdog: stopping");
                        std::process::exit(86);
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mq_http::HttpError;

    /// The walk-only setting: a window and an optional line; no window is
    /// 0; anything else is no setting.
    #[test]
    fn the_walk_setting_reads_a_window_and_a_line() {
        let walk = |w, line| Some(Walk { window: w, line });
        assert_eq!(
            Walk::parse("157286400\n"),
            walk(Some(157_286_400), 700_000_000)
        );
        assert_eq!(Walk::parse("0"), walk(None, 480_000_000));
        assert_eq!(
            Walk::parse("200000000 650000000"),
            walk(Some(200_000_000), 650_000_000)
        );
        assert_eq!(Walk::parse(""), None);
        assert_eq!(Walk::parse("x"), None);
    }

    #[cfg(feature = "torrent")]
    #[test]
    fn the_app_engine_uploads_nothing() {
        let core = Core::temporary("default").unwrap();
        let config = core.engine_config();
        assert!(!config.upload, "the app's engine seeds");
        assert!(config.discovery, "the DHT stays on");
    }

    #[test]
    fn meta_failures_read_as_sentences() {
        let status = |status| {
            AddonError::Http(HttpError::Status {
                status,
                body: Default::default(),
                retry_after: None,
            })
        };
        let cases = [
            (AddonError::TimedOut, "did not answer in time"),
            (
                AddonError::Http(HttpError::Timeout(Duration::from_secs(10))),
                "did not answer in time",
            ),
            (status(404), "has no details for this title (404)"),
            (status(410), "has no details for this title (410)"),
            (status(401), "refused access (401)"),
            (status(403), "refused access (403)"),
            (status(429), "is busy, try again soon (429)"),
            (status(500), "had an error (500)"),
            (status(599), "had an error (599)"),
            (status(400), "answered with an error (400)"),
            (
                AddonError::Http(HttpError::BlockedPrivate("10.0.0.1".into())),
                "points to a private network address",
            ),
            (
                AddonError::Http(HttpError::TooLarge { limit: 1 }),
                "sent details Marquee cannot read",
            ),
            (
                AddonError::Http(HttpError::Decode("x".into())),
                "sent details Marquee cannot read",
            ),
            (
                AddonError::Http(HttpError::Network("dns".into())),
                "could not be reached",
            ),
            (
                AddonError::Http(HttpError::InvalidUrl("x".into())),
                "could not be reached",
            ),
            (AddonError::LegacyTransport, "could not be reached"),
            (
                AddonError::InvalidTransportUrl("no scheme"),
                "could not be reached",
            ),
        ];
        for (error, words) in cases {
            assert_eq!(
                meta_failure("Cinemeta", &error),
                format!("Cinemeta {words}")
            );
        }
    }
}
