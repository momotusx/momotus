use mq_addons::{
    AddonClient, CacheControl, CacheStore, InstalledAddon, ResponseCache, StoredResponse,
};
use mq_debrid::{AllDebrid, Premiumize, RealDebrid, Resolver, ResolverConfig, StoreId, TorBox};
use mq_http::{ClientConfig, HttpClient};
use mq_library::{AddonRow, Hlc, HlcClock, Library, OWNER};
use mq_protocol::{Manifest, MetaItem, ResourcePath, ResourceResponse};
use mq_server::StreamServer;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, Weak};
use std::time::Duration;
use tokio::sync::{Mutex, OnceCell, OwnedMutexGuard};

const META_TIMEOUT: Duration = Duration::from_secs(10);

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
    #[error("addon already installed: {0}")]
    AlreadyInstalled(String),
    #[error("addon is not installed: {0}")]
    NotInstalled(String),
    #[error("invalid setting: {0}")]
    BadPrefs(String),
    #[error("stored manifest is invalid: {0}")]
    BadManifest(String),
    #[error("torrent engine: {0}")]
    Torrent(String),
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
    debrid: RwLock<Option<Resolver>>,
    /// Skip segment lookups and their session cache.
    segments: crate::segments::Finder,
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
        // The owner, with the lowest hlc: what another device synced wins.
        library.ensure_profile(OWNER, OWNER, &Hlc::zero(device_id).encode())?;
        Ok(Arc::new(Self {
            library: Arc::new(library),
            clock,
            sync_gate: Arc::default(),
            pins: std::sync::Mutex::new(crate::profiles::Throttle::new(std::time::Instant::now())),
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
        Ok(Self::with_device(device, &config.profile_id, torrents)?.active())
    }

    /// A core over a throwaway database, for tests and the CLI. `profile` is
    /// created (with no PIN) and made the active one.
    pub fn temporary(profile: &str) -> Result<Self, CoreError> {
        let torrents = std::env::temp_dir().join(format!("mq-torrents-{}", std::process::id()));
        let device = Device::new(Library::temporary()?, 1)?;
        let core = Self::with_device(device, profile, torrents)?;
        core.library()
            .ensure_profile(profile, profile, &core.zero_hlc())?;
        core.library().set_active_profile(profile)?;
        Ok(core)
    }

    fn with_device(
        device: Arc<Device>,
        profile: &str,
        torrent_dir: PathBuf,
    ) -> Result<Self, CoreError> {
        // Build the release-name rule table off the UI path, so the first
        // stream list does not pay for it.
        std::thread::spawn(mq_relparse::warm_up);
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
                debrid: RwLock::new(None),
                segments: crate::segments::Finder::new(http.clone()),
            }),
            profile: profile.into(),
        })
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

    pub(crate) fn pins(&self) -> &std::sync::Mutex<crate::profiles::Throttle> {
        &self.inner.device.pins
    }

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
                StreamServer::start(self.http().clone()).await.map(Arc::new)
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
                let config = mq_engine::EngineConfig::new(&self.inner.torrent_dir);
                mq_engine::Engine::start(config)
                    .await
                    .map_err(|e| CoreError::Torrent(e.to_string()))
            })
            .await?;
        Ok(engine.clone())
    }

    /// Replaces the debrid accounts. Keys come from the platform keystore and
    /// live only in memory here; the first entry is the default store.
    pub fn set_debrid_keys(&self, keys: &[(StoreId, String)]) {
        let resolver = (!keys.is_empty()).then(|| {
            keys.iter()
                .fold(Resolver::new(ResolverConfig::default()), |r, (id, key)| {
                    let (http, key) = (self.http().clone(), key.clone());
                    match id {
                        StoreId::RealDebrid => r.add_store(move |p| {
                            RealDebrid::new(http.clone(), key.clone()).with_priority(p)
                        }),
                        StoreId::TorBox => r.add_store(move |p| {
                            TorBox::new(http.clone(), key.clone()).with_priority(p)
                        }),
                        StoreId::AllDebrid => r.add_store(move |p| {
                            AllDebrid::new(http.clone(), key.clone()).with_priority(p)
                        }),
                        StoreId::Premiumize => r.add_store(move |p| {
                            Premiumize::new(http.clone(), key.clone()).with_priority(p)
                        }),
                    }
                })
        });
        *self.inner.debrid.write().unwrap_or_else(|p| p.into_inner()) = resolver;
    }

    pub fn debrid(&self) -> Option<Resolver> {
        self.inner
            .debrid
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Installed addons in the user's order. Rows whose manifest no longer
    /// parses are skipped rather than failing the whole list.
    pub fn installed_addons(&self) -> Result<Vec<InstalledAddon>, CoreError> {
        let rows = self.library().addons(self.profile())?;
        Ok(rows
            .into_iter()
            .filter(|r| r.enabled)
            .filter_map(|r| {
                let manifest: Manifest = serde_json::from_str(&r.manifest_json).ok()?;
                Some(InstalledAddon {
                    transport_url: url::Url::parse(&r.transport_url).ok()?,
                    manifest,
                    allow_private: r.allow_private,
                })
            })
            .collect())
    }

    /// Details for one title: the first answer in addon order wins, as in
    /// Stremio; later addons are asked only when earlier ones fail. Returns the
    /// meta and the name of the addon that gave it.
    pub async fn fetch_meta(&self, kind: &str, id: &str) -> Result<(MetaItem, String), String> {
        let (body, name) = self.fetch_meta_body(kind, id).await?;
        match serde_json::from_slice::<ResourceResponse>(&body) {
            Ok(ResourceResponse::Meta { meta }) => Ok((meta, name)),
            Ok(_) => Err(format!("{name} returned no details")),
            Err(e) => Err(format!("{name} returned unreadable details: {e}")),
        }
    }

    /// Each episode's TVDB id by video id, from the same meta answer (served
    /// from the addon cache after the first look). stremio-core's `Video`
    /// has no such field, so it is read from the raw JSON. Metas that do not
    /// carry it give an empty map.
    pub async fn episode_tvdb_ids(
        &self,
        id: &str,
    ) -> Result<std::collections::HashMap<String, u64>, String> {
        let (body, _) = self.fetch_meta_body("series", id).await?;
        let raw: serde_json::Value =
            serde_json::from_slice(&body).map_err(|e| format!("unreadable details: {e}"))?;
        Ok(raw["meta"]["videos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| Some((v["id"].as_str()?.to_owned(), v["tvdb_id"].as_u64()?)))
            .collect())
    }

    /// The first meta answer in addon order that decodes, as raw bytes.
    async fn fetch_meta_body(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<(bytes::Bytes, String), String> {
        let path = ResourcePath {
            resource: "meta".into(),
            r#type: kind.to_owned(),
            id: id.to_owned(),
            extra: vec![],
        };
        let addons = self.installed_addons().map_err(|e| e.to_string())?;
        let mut last_error = "no installed addon provides details for this title".to_owned();
        for addon in addons.iter().filter(|a| a.supports(&path)) {
            let name = &addon.manifest.name;
            match self.addons().fetch(addon, &path, META_TIMEOUT).await {
                Ok(fetched) => match serde_json::from_slice::<ResourceResponse>(&fetched.body) {
                    Ok(ResourceResponse::Meta { .. }) => return Ok((fetched.body, name.clone())),
                    Ok(_) => last_error = format!("{name} returned no details"),
                    Err(e) => last_error = format!("{name} returned unreadable details: {e}"),
                },
                Err(e) => last_error = format!("{name}: {e}"),
            }
        }
        Err(last_error)
    }

    /// Fetches the manifest and appends the addon to the profile's list.
    pub async fn install_addon(&self, transport_url: &str) -> Result<InstalledAddon, CoreError> {
        let addon = self.addons().fetch_manifest(transport_url).await?;
        let manifest_json = serde_json::to_string(&addon.manifest)
            .map_err(|e| CoreError::BadManifest(e.to_string()))?;
        let url = addon.transport_url.to_string();
        self.edit_addons(|rows| {
            if rows.iter().any(|r| r.transport_url == url) {
                return Err(CoreError::AlreadyInstalled(redact(&url)));
            }
            rows.push(AddonRow {
                transport_url: url.clone(),
                manifest_json,
                position: 0,
                enabled: true,
                allow_private: addon.allow_private,
            });
            Ok(())
        })?;
        Ok(addon)
    }

    pub fn remove_addon(&self, transport_url: &str) -> Result<(), CoreError> {
        self.edit_addons(|rows| {
            let at = position(rows, transport_url)?;
            rows.remove(at);
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

    pub fn set_addon_enabled(&self, transport_url: &str, enabled: bool) -> Result<(), CoreError> {
        self.edit_addons(|rows| {
            let at = position(rows, transport_url)?;
            rows[at].enabled = enabled;
            Ok(())
        })
    }

    /// One write of the whole ordered list; positions follow the new order.
    fn edit_addons(
        &self,
        f: impl FnOnce(&mut Vec<AddonRow>) -> Result<(), CoreError>,
    ) -> Result<(), CoreError> {
        let mut rows = self.library().addons(self.profile())?;
        f(&mut rows)?;
        for (i, row) in rows.iter_mut().enumerate() {
            row.position = i as i64;
        }
        self.library()
            .set_addons(self.profile(), &rows, &self.hlc())?;
        Ok(())
    }
}

fn position(rows: &[AddonRow], transport_url: &str) -> Result<usize, CoreError> {
    rows.iter()
        .position(|r| r.transport_url == transport_url)
        .ok_or_else(|| CoreError::NotInstalled(redact(transport_url)))
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

    fn save(&self, key: &str, value: &StoredResponse) {
        let row = mq_library::CachedResponse {
            body: value.body.to_vec(),
            stored_ms: value.stored_ms as i64,
            max_age_s: value.control.max_age.as_secs() as i64,
            stale_revalidate_s: value.control.stale_revalidate.as_secs() as i64,
            stale_error_s: value.control.stale_error.as_secs() as i64,
        };
        if let Err(e) = self.0.cache_put(key, &row) {
            tracing::warn!("http cache write failed: {e}");
        }
    }
}
