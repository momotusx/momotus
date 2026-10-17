use mq_addons::{AddonClient, InstalledAddon};
use mq_http::{ClientConfig, HttpClient};
use mq_library::hlc::now_ms;
use mq_library::{AddonRow, HlcClock, Library};
use mq_protocol::Manifest;
use std::path::PathBuf;
use std::sync::Arc;

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
    #[error("stored manifest is invalid: {0}")]
    BadManifest(String),
}

#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub data_dir: PathBuf,
    pub profile_id: String,
    pub device_id: u64,
}

/// The shared core. Cheap to clone; every view holds one.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

struct Inner {
    addons: AddonClient,
    library: Library,
    clock: HlcClock,
    profile: String,
}

impl Core {
    pub fn open(config: CoreConfig) -> Result<Self, CoreError> {
        std::fs::create_dir_all(&config.data_dir)?;
        let library = Library::open(config.data_dir.join("library.sqlite"))?;
        Self::with_library(library, config.profile_id, config.device_id)
    }

    /// A core over a throwaway database, for tests and the CLI.
    pub fn temporary(profile: &str) -> Result<Self, CoreError> {
        Self::with_library(Library::temporary()?, profile.to_owned(), 1)
    }

    fn with_library(library: Library, profile: String, device_id: u64) -> Result<Self, CoreError> {
        let http = HttpClient::new(ClientConfig::default())?;
        let clock = HlcClock::new(device_id);
        library.ensure_profile(&profile, &profile, &clock.tick(now_ms()).encode())?;
        Ok(Self {
            inner: Arc::new(Inner {
                addons: AddonClient::new(http),
                library,
                clock,
                profile,
            }),
        })
    }

    pub fn addons(&self) -> &AddonClient {
        &self.inner.addons
    }

    pub fn library(&self) -> &Library {
        &self.inner.library
    }

    pub fn profile(&self) -> &str {
        &self.inner.profile
    }

    pub fn hlc(&self) -> String {
        self.inner.clock.tick(now_ms()).encode()
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

    /// Fetches the manifest and appends the addon to the profile's list.
    pub async fn install_addon(&self, transport_url: &str) -> Result<InstalledAddon, CoreError> {
        let addon = self.addons().fetch_manifest(transport_url).await?;
        let mut rows = self.library().addons(self.profile())?;
        if rows
            .iter()
            .any(|r| r.transport_url == addon.transport_url.as_str())
        {
            return Err(CoreError::AlreadyInstalled(addon.transport_url.to_string()));
        }
        rows.push(AddonRow {
            transport_url: addon.transport_url.to_string(),
            manifest_json: serde_json::to_string(&addon.manifest)
                .map_err(|e| CoreError::BadManifest(e.to_string()))?,
            position: rows.len() as i64,
            enabled: true,
            allow_private: addon.allow_private,
        });
        self.library()
            .set_addons(self.profile(), &rows, &self.hlc())?;
        Ok(addon)
    }
}
