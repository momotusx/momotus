//! Debrid stores behind one trait, plus a resolver that turns a torrent
//! reference into a direct HTTPS link: file pick, add, select, link, with
//! in-flight dedupe and a link cache.
//!
//! API keys are passed in by the caller (the platform keystore owns them) and
//! are never logged. Library code never reads the environment.

mod alldebrid;
mod api;
mod error;
mod premiumize;
mod realdebrid;
mod resolver;
mod select;
mod torbox;
mod util;

pub use alldebrid::AllDebrid;
pub use error::DebridError;
pub use mq_http::Priority;
pub use premiumize::Premiumize;
pub use realdebrid::{Credentials, DeviceLogin, LoginPoll, RealDebrid};
pub use resolver::{Clock, ManualClock, ResolveRequest, Resolver, ResolverConfig, SystemClock};
pub use select::pick_file;
pub use torbox::TorBox;

use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreId {
    RealDebrid,
    TorBox,
    AllDebrid,
    Premiumize,
}

impl StoreId {
    pub const ALL: [StoreId; 4] = [
        Self::RealDebrid,
        Self::TorBox,
        Self::AllDebrid,
        Self::Premiumize,
    ];

    /// Environment variable the dev tools (CLI, bridge) read the key from.
    /// Apps pass keys from the platform keystore instead.
    pub fn env_var(self) -> &'static str {
        match self {
            Self::RealDebrid => "MQ_RD_TOKEN",
            Self::TorBox => "MQ_TORBOX_KEY",
            Self::AllDebrid => "MQ_ALLDEBRID_KEY",
            Self::Premiumize => "MQ_PREMIUMIZE_KEY",
        }
    }

    /// Keys found in the environment, for the dev tools only.
    pub fn keys_from_env() -> Vec<(StoreId, String)> {
        Self::ALL
            .into_iter()
            .filter_map(|id| Some((id, std::env::var(id.env_var()).ok()?)))
            .collect()
    }

    /// Maps the service names addons use (`ClientResolve.service`).
    pub fn from_service(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "realdebrid" | "real-debrid" | "rd" => Some(Self::RealDebrid),
            "torbox" | "tb" => Some(Self::TorBox),
            "alldebrid" | "ad" => Some(Self::AllDebrid),
            "premiumize" | "pm" => Some(Self::Premiumize),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// `check` gives real answers (otherwise it returns `Unknown`).
    pub instant_check: bool,
    /// The provider needs `select_files` before links exist.
    pub file_select: bool,
    /// Generated links or API calls are bound to the requesting IP.
    pub ip_locked: bool,
    pub device_code_login: bool,
    /// Documented request ceiling, `None` when the provider documents none.
    pub rate_limit_per_min: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreUser {
    pub name: String,
    pub is_premium: bool,
    /// Unix seconds.
    pub premium_until: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStatus {
    Cached,
    NotCached,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentHandle {
    pub id: String,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorrentStatus {
    /// Fetching metadata.
    Resolving,
    /// Real-Debrid: files are known, none selected yet.
    WaitingFileSelection,
    Queued,
    Downloading,
    /// Files can be linked.
    Ready,
    /// Dead, errored or infected; carries the provider's status text.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentFile {
    /// The id `select_files` and `link` take (provider specific numbering).
    pub index: u32,
    pub path: String,
    pub size: u64,
    pub selected: bool,
    /// Provider specific link to unlock, when the provider hands them out per
    /// file. Opaque to callers.
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TorrentInfo {
    pub id: String,
    /// Lowercase info hash; empty when the provider does not report it.
    pub hash: String,
    pub name: String,
    pub status: TorrentStatus,
    /// 0.0 to 1.0.
    pub progress: f32,
    pub files: Vec<TorrentFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLink {
    pub url: String,
    pub size: Option<u64>,
    pub filename: String,
    /// Unix seconds, when the provider states a validity window.
    pub expires_at: Option<u64>,
}

/// One debrid provider. Methods return `impl Future + Send` so callers can
/// spawn them; implementations may still be written as `async fn`.
pub trait Store: Send + Sync {
    fn id(&self) -> StoreId;
    fn capabilities(&self) -> Capabilities;
    fn user(&self) -> impl Future<Output = Result<StoreUser, DebridError>> + Send;
    /// One status per input hash, in order. Providers without instant checks
    /// return `Unknown` for every hash and make no request.
    fn check(
        &self,
        hashes: &[String],
    ) -> impl Future<Output = Result<Vec<CacheStatus>, DebridError>> + Send;
    fn add_magnet(
        &self,
        info_hash: &str,
    ) -> impl Future<Output = Result<TorrentHandle, DebridError>> + Send;
    fn torrent(&self, id: &str) -> impl Future<Output = Result<TorrentInfo, DebridError>> + Send;
    fn select_files(
        &self,
        id: &str,
        file_ids: &[u32],
    ) -> impl Future<Output = Result<(), DebridError>> + Send;
    fn link(
        &self,
        torrent: &TorrentInfo,
        file_index: u32,
    ) -> impl Future<Output = Result<ResolvedLink, DebridError>> + Send;
    fn remove(&self, id: &str) -> impl Future<Output = Result<(), DebridError>> + Send;
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Object-safe twin of [`Store`] so the resolver can hold a mixed set.
pub(crate) trait DynStore: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn check<'a>(
        &'a self,
        hashes: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<CacheStatus>, DebridError>>;
    fn add_magnet<'a>(&'a self, hash: &'a str)
    -> BoxFuture<'a, Result<TorrentHandle, DebridError>>;
    fn torrent<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<TorrentInfo, DebridError>>;
    fn select_files<'a>(
        &'a self,
        id: &'a str,
        file_ids: &'a [u32],
    ) -> BoxFuture<'a, Result<(), DebridError>>;
    fn link<'a>(
        &'a self,
        torrent: &'a TorrentInfo,
        file_index: u32,
    ) -> BoxFuture<'a, Result<ResolvedLink, DebridError>>;
}

impl<T: Store> DynStore for T {
    fn capabilities(&self) -> Capabilities {
        Store::capabilities(self)
    }
    fn check<'a>(
        &'a self,
        hashes: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<CacheStatus>, DebridError>> {
        Box::pin(Store::check(self, hashes))
    }
    fn add_magnet<'a>(
        &'a self,
        hash: &'a str,
    ) -> BoxFuture<'a, Result<TorrentHandle, DebridError>> {
        Box::pin(Store::add_magnet(self, hash))
    }
    fn torrent<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<TorrentInfo, DebridError>> {
        Box::pin(Store::torrent(self, id))
    }
    fn select_files<'a>(
        &'a self,
        id: &'a str,
        file_ids: &'a [u32],
    ) -> BoxFuture<'a, Result<(), DebridError>> {
        Box::pin(Store::select_files(self, id, file_ids))
    }
    fn link<'a>(
        &'a self,
        torrent: &'a TorrentInfo,
        file_index: u32,
    ) -> BoxFuture<'a, Result<ResolvedLink, DebridError>> {
        Box::pin(Store::link(self, torrent, file_index))
    }
}
