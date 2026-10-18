//! Stremio addon client: URL building, manifest fetch, a budgeted fan-out with
//! soft and hard deadlines, response caching and per-addon health.

mod cache;
mod fanout;
mod health;
mod transport;

pub use cache::{CacheControl, CacheStore, ResponseCache, StoredResponse};
pub use fanout::{AddonEvent, Deadlines, Fanout};
pub use health::{AddonHealth, HealthBook};
pub use transport::{AddonError, configure_url, normalize_transport_url, resource_url};

use bytes::Bytes;
use mq_http::{HttpClient, Priority, RequestOptions};
use mq_protocol::{Manifest, ResourcePath};
use std::sync::Arc;
use std::time::{Duration, Instant};
use url::Url;

/// An addon the user installed. `allow_private` is set when the user installed
/// it from a LAN or loopback address, so private-address fetches stay allowed
/// for that addon only.
#[derive(Debug, Clone)]
pub struct InstalledAddon {
    pub transport_url: Url,
    pub manifest: Manifest,
    pub allow_private: bool,
}

impl InstalledAddon {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn supports(&self, path: &ResourcePath) -> bool {
        self.manifest.is_resource_supported(path)
    }
}

#[derive(Clone)]
pub struct AddonClient {
    http: HttpClient,
    cache: Arc<ResponseCache>,
    health: Arc<HealthBook>,
}

impl AddonClient {
    pub fn new(http: HttpClient) -> Self {
        Self::with_cache(http, ResponseCache::new(512))
    }

    pub fn with_cache(http: HttpClient, cache: ResponseCache) -> Self {
        Self {
            http,
            cache: Arc::new(cache),
            health: Arc::new(HealthBook::default()),
        }
    }

    pub fn health(&self) -> &HealthBook {
        &self.health
    }

    /// Frees the in-memory response cache (the disk store stays).
    pub fn forget_memory(&self) {
        self.cache.forget_memory();
    }

    pub async fn fetch_manifest(&self, transport_url: &str) -> Result<InstalledAddon, AddonError> {
        let url = transport::normalize_transport_url(transport_url)?;
        let allow_private = url.host_str().is_some_and(mq_http::is_private_host);
        let opts = RequestOptions {
            allow_private,
            timeout: Duration::from_secs(10),
            ..Default::default()
        };
        let manifest: Manifest = self
            .http
            .get_json(url.as_str(), &opts)
            .await
            .map_err(AddonError::Http)?;
        Ok(InstalledAddon {
            transport_url: url,
            manifest,
            allow_private,
        })
    }

    /// Fetches one resource, serving from cache when fresh.
    pub async fn fetch(
        &self,
        addon: &InstalledAddon,
        path: &ResourcePath,
        timeout: Duration,
    ) -> Result<Fetched, AddonError> {
        let url = resource_url(&addon.transport_url, path)?;
        // Taken before the network: an answer that a profile removal
        // overtakes is not cached.
        let generation = self.cache.generation();
        if let Some(hit) = self.cache.get(&url) {
            if hit.is_stale_but_usable() {
                self.spawn_refresh(addon.clone(), url.clone(), timeout, generation);
            }
            return Ok(Fetched {
                body: hit.body,
                from_cache: true,
                latency: Duration::ZERO,
            });
        }
        match self.fetch_network(addon, &url, timeout, generation).await {
            Ok(fetched) => Ok(fetched),
            Err(error) => match self.cache.get_on_error(&url) {
                Some(hit) => Ok(Fetched {
                    body: hit.body,
                    from_cache: true,
                    latency: Duration::ZERO,
                }),
                None => Err(error),
            },
        }
    }

    /// `fetch` past the cache: waits for the network, and keeps the answer
    /// as `fetch` does. A failure is a failure, never a stale answer.
    pub async fn fetch_fresh(
        &self,
        addon: &InstalledAddon,
        path: &ResourcePath,
        timeout: Duration,
    ) -> Result<Fetched, AddonError> {
        let url = resource_url(&addon.transport_url, path)?;
        let generation = self.cache.generation();
        self.fetch_network(addon, &url, timeout, generation).await
    }

    async fn fetch_network(
        &self,
        addon: &InstalledAddon,
        url: &str,
        timeout: Duration,
        generation: u64,
    ) -> Result<Fetched, AddonError> {
        let opts = RequestOptions {
            priority: Priority::Addon,
            timeout,
            allow_private: addon.allow_private,
            ..Default::default()
        };
        let started = Instant::now();
        let result = self.http.get(url, &opts).await;
        let latency = started.elapsed();
        self.health.record(addon.id(), latency, result.is_ok());
        let response = result.map_err(AddonError::Http)?;
        let control = CacheControl::from_response(&response.headers, &response.body);
        self.cache
            .put(url.to_owned(), response.body.clone(), control, generation);
        Ok(Fetched {
            body: response.body,
            from_cache: false,
            latency,
        })
    }

    fn spawn_refresh(
        &self,
        addon: InstalledAddon,
        url: String,
        timeout: Duration,
        generation: u64,
    ) {
        let client = self.clone();
        tokio::spawn(async move {
            let _ = client
                .fetch_network(&addon, &url, timeout, generation)
                .await;
        });
    }

    /// Queries every addon that supports `path`, in parallel.
    pub fn fan_out(
        &self,
        addons: &[InstalledAddon],
        path: &ResourcePath,
        deadlines: Deadlines,
    ) -> Fanout {
        let eligible = addons
            .iter()
            .filter(|a| a.supports(path))
            .cloned()
            .collect();
        Fanout::start(self.clone(), eligible, path.clone(), deadlines)
    }
}

#[derive(Debug, Clone)]
pub struct Fetched {
    pub body: Bytes,
    pub from_cache: bool,
    pub latency: Duration,
}
