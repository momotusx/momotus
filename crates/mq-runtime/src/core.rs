use mq_addons::{
    AddonClient, AddonError, CacheControl, CacheStore, InstalledAddon, ResponseCache,
    StoredResponse,
};
use mq_http::{ClientConfig, HttpClient};
use mq_library::{AddonRow, Hlc, HlcClock, Library, OWNER, OWNER_NAME};
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
                debrid: crate::debrid::Accounts::new(http.clone()),
                segments: crate::segments::Finder::new(http.clone()),
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

    /// The device's network changed: connections pooled on the old one are
    /// dropped (each would hold a request until its timeout).
    pub fn on_network_changed(&self) {
        self.http().network_changed();
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

    pub(crate) fn accounts(&self) -> &crate::debrid::Accounts {
        &self.inner.debrid
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
                // The row's `allow_private` grants nothing: sync may set it.
                Some(InstalledAddon {
                    transport_url: url::Url::parse(&r.transport_url).ok()?,
                    manifest,
                })
            })
            .collect())
    }

    /// Details for one title: the first answer in addon order wins, as in
    /// Stremio; later addons are asked only when earlier ones fail. Returns the
    /// meta and the name of the addon that gave it.
    pub async fn fetch_meta(&self, kind: &str, id: &str) -> Result<(MetaItem, String), String> {
        let (_, meta, name) = self.fetch_meta_body(kind, id).await?;
        Ok((meta, name))
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
        for addon in addons.iter().filter(|a| a.supports(&path)) {
            let name = &addon.manifest.name;
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
            _ => Err(TvdbMiss::Unavailable(reason)),
        }
    }

    /// The first meta answer in addon order that decodes, as raw bytes and
    /// decoded, with the addon's name. Otherwise the last addon's failure as
    /// the title page shows it.
    async fn fetch_meta_body(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<(bytes::Bytes, MetaItem, String), String> {
        let path = ResourcePath {
            resource: "meta".into(),
            r#type: kind.to_owned(),
            id: id.to_owned(),
            extra: vec![],
        };
        let addons = self.installed_addons().map_err(|e| e.to_string())?;
        let mut last_error = "None of your addons has details for this title".to_owned();
        for addon in addons.iter().filter(|a| a.supports(&path)) {
            let name = &addon.manifest.name;
            match self.addons().fetch(addon, &path, META_TIMEOUT).await {
                Ok(fetched) => match serde_json::from_slice::<ResourceResponse>(&fetched.body) {
                    Ok(ResourceResponse::Meta { meta }) => {
                        return Ok((fetched.body, meta, name.clone()));
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
                    last_error = meta_failure(name, &e);
                }
            }
        }
        Err(last_error)
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
                if !links.contains(&row.transport_url) {
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
            let written = self.for_profile(profile).edit_addons(|rows| {
                for row in rows.iter_mut() {
                    if let Some(json) = newer(row) {
                        row.manifest_json = json.clone();
                        changed += 1;
                    }
                }
                Ok(())
            });
            if let Err(e) = written {
                // A profile removed meanwhile, or a failed write: the next day tries again.
                tracing::debug!(
                    "addon refresh for a profile not written: {}",
                    error_kind(&e)
                );
            }
        }
        Ok(changed)
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
/// Installed manifests are fetched again at most this often.
const REFRESH_EVERY_MS: u64 = 24 * 60 * 60 * 1000;

/// A library error's kind for logs: never a link, which can carry keys.
fn error_kind(e: &CoreError) -> &'static str {
    match e {
        CoreError::Library(_) => "library",
        _ => "other",
    }
}

/// Puts a fetched manifest in the list: in place over the row the link
/// names (position, whether it is on and the stored link kept), else at the
/// end, on.
pub(crate) fn put_addon(rows: &mut Vec<AddonRow>, url: &str, manifest_json: String) {
    match position(rows, url) {
        Ok(at) => rows[at].manifest_json = manifest_json,
        Err(_) => rows.push(AddonRow {
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
        }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use mq_http::HttpError;

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
