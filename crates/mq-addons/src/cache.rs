use bytes::Bytes;
use mq_http::HeaderMap;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// Caching rules for one response. Addon body fields (`cacheMaxAge`,
/// `staleRevalidate`, `staleError`, seconds) win over `Cache-Control`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheControl {
    pub max_age: Duration,
    pub stale_revalidate: Duration,
    pub stale_error: Duration,
}

impl CacheControl {
    pub fn from_response(headers: &HeaderMap, body: &[u8]) -> Self {
        let mut control = Self::from_header(headers);
        if let Ok(serde_json::Value::Object(root)) = serde_json::from_slice(body) {
            let secs = |key: &str| {
                root.get(key)
                    .and_then(|v| v.as_u64())
                    .map(Duration::from_secs)
            };
            control.max_age = secs("cacheMaxAge").unwrap_or(control.max_age);
            control.stale_revalidate = secs("staleRevalidate").unwrap_or(control.stale_revalidate);
            control.stale_error = secs("staleError").unwrap_or(control.stale_error);
        }
        control
    }

    fn from_header(headers: &HeaderMap) -> Self {
        let mut control = Self::default();
        let Some(value) = headers.get("cache-control").and_then(|v| v.to_str().ok()) else {
            return control;
        };
        for directive in value.split(',').map(str::trim) {
            let (name, arg) = directive.split_once('=').unwrap_or((directive, ""));
            let secs = arg.trim_matches('"').parse().ok().map(Duration::from_secs);
            match name.to_ascii_lowercase().as_str() {
                "no-store" | "no-cache" => return Self::default(),
                "max-age" => control.max_age = secs.unwrap_or_default(),
                "stale-while-revalidate" => control.stale_revalidate = secs.unwrap_or_default(),
                "stale-if-error" => control.stale_error = secs.unwrap_or_default(),
                _ => {}
            }
        }
        control
    }

    fn cacheable(&self) -> bool {
        !(self.max_age + self.stale_revalidate + self.stale_error).is_zero()
    }
}

#[derive(Debug, Clone)]
pub struct CacheHit {
    pub body: Bytes,
    stale: bool,
}

impl CacheHit {
    pub fn is_stale_but_usable(&self) -> bool {
        self.stale
    }
}

/// A cached response as stored by a persistent backend.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredResponse {
    pub body: Bytes,
    /// Wall-clock milliseconds since the Unix epoch.
    pub stored_ms: u64,
    pub control: CacheControl,
}

/// Second cache tier that survives restarts (the library database in apps).
pub trait CacheStore: Send + Sync {
    fn load(&self, key: &str) -> Option<StoredResponse>;
    /// Stores `value`, fetched while `generation` was current; the store
    /// drops it when the generation moved on meanwhile.
    fn save(&self, key: &str, value: &StoredResponse, generation: u64);
    /// Moves on when cached answers must no longer be used (the library
    /// counts profile removals: their URLs can carry the profile's keys).
    fn generation(&self) -> u64 {
        0
    }
}

fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

type Entry = StoredResponse;

/// In-memory response cache with a fixed entry budget.
// ponytail: evicts the oldest entry with a linear scan; fine at a few hundred
// entries, switch to an LRU list if the budget grows past a few thousand.
pub struct ResponseCache {
    capacity: usize,
    /// Each entry with the store generation it was fetched or loaded under;
    /// one from an older generation is never served.
    entries: Mutex<HashMap<String, (Entry, u64)>>,
    store: Option<std::sync::Arc<dyn CacheStore>>,
}

impl ResponseCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Mutex::new(HashMap::new()),
            store: None,
        }
    }

    pub fn with_store(capacity: usize, store: std::sync::Arc<dyn CacheStore>) -> Self {
        Self {
            store: Some(store),
            ..Self::new(capacity)
        }
    }

    /// The store's generation; capture it before a fetch and pass it to
    /// `put`.
    pub fn generation(&self) -> u64 {
        self.store.as_ref().map_or(0, |s| s.generation())
    }

    /// Fresh hit, or stale-while-revalidate hit flagged as stale.
    pub fn get(&self, key: &str) -> Option<CacheHit> {
        self.lookup(key, |age, c| {
            if age < c.max_age {
                Some(false)
            } else if age < c.max_age + c.stale_revalidate {
                Some(true)
            } else {
                None
            }
        })
    }

    /// Hit allowed only because the network failed (stale-if-error).
    pub fn get_on_error(&self, key: &str) -> Option<CacheHit> {
        self.lookup(key, |age, c| {
            (age < c.max_age + c.stale_error.max(c.stale_revalidate)).then_some(true)
        })
    }

    fn lookup(
        &self,
        key: &str,
        rule: impl Fn(Duration, &CacheControl) -> Option<bool>,
    ) -> Option<CacheHit> {
        let entry = self.memory_or_store(key)?;
        let age = Duration::from_millis(wall_ms().saturating_sub(entry.stored_ms));
        let stale = rule(age, &entry.control)?;
        Some(CacheHit {
            body: entry.body,
            stale,
        })
    }

    /// Drops the in-memory copies (under memory pressure); the store keeps
    /// them, so the next read loads from disk instead of the network.
    pub fn forget_memory(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// Keeps an answer fetched while `generation` (from `generation()`) was
    /// current; one the generation moved past meanwhile is dropped.
    pub fn put(&self, key: String, body: Bytes, control: CacheControl, generation: u64) {
        if !control.cacheable() || generation != self.generation() {
            return;
        }
        let entry = Entry {
            body,
            stored_ms: wall_ms(),
            control,
        };
        if let Some(store) = &self.store {
            store.save(&key, &entry, generation);
        }
        self.remember(key, entry, generation);
    }

    fn memory_or_store(&self, key: &str) -> Option<Entry> {
        let current = self.generation();
        if let Some((entry, generation)) = self
            .entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            && *generation == current
        {
            return Some(entry.clone());
        }
        let entry = self.store.as_ref()?.load(key)?;
        self.remember(key.to_owned(), entry.clone(), current);
        Some(entry)
    }

    fn remember(&self, key: String, entry: Entry, generation: u64) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        if entries.len() >= self.capacity && !entries.contains_key(&key) {
            let oldest = entries
                .iter()
                .min_by_key(|(_, (e, _))| e.stored_ms)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                entries.remove(&oldest);
            }
        }
        entries.insert(key, (entry, generation));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("cache-control", value.parse().unwrap());
        h
    }

    #[test]
    fn body_fields_override_header() {
        let c = CacheControl::from_response(
            &headers("public, max-age=10800"),
            br#"{"metas":[],"cacheMaxAge":60,"staleRevalidate":3600}"#,
        );
        assert_eq!(c.max_age, Duration::from_secs(60));
        assert_eq!(c.stale_revalidate, Duration::from_secs(3600));
        let c = CacheControl::from_response(&headers("max-age=120, stale-if-error=600"), b"{}");
        assert_eq!(c.max_age, Duration::from_secs(120));
        assert_eq!(c.stale_error, Duration::from_secs(600));
        assert_eq!(
            CacheControl::from_response(&headers("no-store"), b"{}"),
            CacheControl::default()
        );
    }

    #[test]
    fn fresh_stale_and_error_windows() {
        let cache = ResponseCache::new(2);
        let fresh = CacheControl {
            max_age: Duration::from_secs(60),
            ..Default::default()
        };
        cache.put("a".into(), Bytes::from_static(b"1"), fresh, 0);
        assert!(!cache.get("a").unwrap().is_stale_but_usable());

        let stale = CacheControl {
            max_age: Duration::ZERO,
            stale_revalidate: Duration::from_secs(60),
            stale_error: Duration::ZERO,
        };
        cache.put("b".into(), Bytes::from_static(b"2"), stale, 0);
        assert!(cache.get("b").unwrap().is_stale_but_usable());

        cache.put(
            "c".into(),
            Bytes::from_static(b"3"),
            CacheControl::default(),
            0,
        );
        assert!(
            cache.get("c").is_none(),
            "uncacheable responses are not stored"
        );

        let only_error = CacheControl {
            stale_error: Duration::from_secs(60),
            ..Default::default()
        };
        cache.put("d".into(), Bytes::from_static(b"4"), only_error, 0);
        assert!(cache.get("d").is_none());
        assert!(cache.get_on_error("d").is_some());
        assert_eq!(cache.entries.lock().unwrap().len(), 2, "capacity enforced");
    }
}

#[cfg(test)]
mod memory {
    use super::*;

    #[test]
    fn forgetting_memory_keeps_nothing_in_ram() {
        let cache = ResponseCache::new(8);
        let control = CacheControl {
            max_age: Duration::from_secs(60),
            ..Default::default()
        };
        cache.put("k".into(), Bytes::from_static(b"v"), control, 0);
        assert!(cache.get("k").is_some());
        cache.forget_memory();
        assert!(cache.get("k").is_none(), "no store, so nothing left");
    }
}

#[cfg(test)]
mod generations {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A store whose generation moves when the test says so (a profile
    /// removal), clearing what it holds.
    #[derive(Default)]
    struct Moving {
        generation: AtomicU64,
        held: Mutex<HashMap<String, StoredResponse>>,
    }

    impl Moving {
        fn remove_a_profile(&self) {
            self.generation.fetch_add(1, Ordering::SeqCst);
            self.held.lock().unwrap().clear();
        }
    }

    impl CacheStore for Moving {
        fn load(&self, key: &str) -> Option<StoredResponse> {
            self.held.lock().unwrap().get(key).cloned()
        }

        fn save(&self, key: &str, value: &StoredResponse, generation: u64) {
            if generation == self.generation() {
                self.held.lock().unwrap().insert(key.into(), value.clone());
            }
        }

        fn generation(&self) -> u64 {
            self.generation.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn answers_of_an_older_generation_are_neither_kept_nor_served() {
        let store = Arc::new(Moving::default());
        let cache = ResponseCache::with_store(8, store.clone());
        let fresh = CacheControl {
            max_age: Duration::from_secs(60),
            ..CacheControl::default()
        };
        let before = cache.generation();
        cache.put("a".into(), Bytes::from_static(b"1"), fresh, before);
        assert!(cache.get("a").is_some());

        store.remove_a_profile();
        assert!(cache.get("a").is_none(), "the memory copy is retired too");
        // An answer whose fetch began before the removal.
        cache.put("b".into(), Bytes::from_static(b"2"), fresh, before);
        assert!(cache.get("b").is_none());
        assert!(store.held.lock().unwrap().is_empty());
        // Fetched after it: kept.
        cache.put(
            "c".into(),
            Bytes::from_static(b"3"),
            fresh,
            cache.generation(),
        );
        assert!(cache.get("c").is_some());
    }
}
