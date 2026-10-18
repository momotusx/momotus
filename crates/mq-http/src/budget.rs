use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Request priority. Playback and resolve bypass the global cap so a busy
/// catalog refresh never delays the stream the user is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Prefetch,
    Preload,
    Addon,
    Probe,
    Resolve,
    Playback,
}

impl Priority {
    fn bypasses_global(self) -> bool {
        self >= Priority::Resolve
    }
}

/// Hosts kept before idle ones are dropped. Redirect hops add their hosts
/// (numbered CDN and debrid download hosts), so the map would otherwise grow
/// for the process's lifetime.
const MAX_HOSTS: usize = 1024;

// ponytail: a scan of the map once it is full; an LRU if hosts ever churn
// fast enough for the scan to show in profiles.
pub(crate) struct Budget {
    per_host_limit: usize,
    global: Arc<Semaphore>,
    hosts: Mutex<HashMap<String, Arc<Semaphore>>>,
}

pub(crate) struct Permit {
    _host: OwnedSemaphorePermit,
    _global: Option<OwnedSemaphorePermit>,
}

impl Budget {
    pub(crate) fn new(per_host_limit: usize, global_limit: usize) -> Self {
        Self {
            per_host_limit,
            global: Arc::new(Semaphore::new(global_limit)),
            hosts: Mutex::new(HashMap::new()),
        }
    }

    fn host_semaphore(&self, host: &str) -> Arc<Semaphore> {
        let mut hosts = self.hosts.lock().unwrap_or_else(|p| p.into_inner());
        if hosts.len() >= MAX_HOSTS {
            // A semaphore someone holds or waits on has another reference.
            hosts.retain(|_, semaphore| Arc::strong_count(semaphore) > 1);
        }
        hosts
            .entry(host.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(self.per_host_limit)))
            .clone()
    }

    pub(crate) async fn acquire(&self, host: &str, priority: Priority) -> Permit {
        let global = if priority.bypasses_global() {
            None
        } else {
            Some(acquire_owned(&self.global).await)
        };
        let host = acquire_owned(&self.host_semaphore(host)).await;
        Permit {
            _host: host,
            _global: global,
        }
    }
}

async fn acquire_owned(semaphore: &Arc<Semaphore>) -> OwnedSemaphorePermit {
    // The semaphores are never closed, so acquisition cannot fail.
    semaphore
        .clone()
        .acquire_owned()
        .await
        .expect("semaphore never closed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn per_host_limit_holds_and_playback_skips_global() {
        let budget = Budget::new(1, 1);
        let held = budget.acquire("a.example", Priority::Addon).await;
        // Global is exhausted, but playback on another host still proceeds.
        let playback = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            budget.acquire("b.example", Priority::Playback),
        )
        .await;
        assert!(playback.is_ok());
        // Same host is capped at one.
        let same_host = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            budget.acquire("a.example", Priority::Playback),
        )
        .await;
        assert!(same_host.is_err());
        drop(held);
    }

    #[tokio::test]
    async fn idle_hosts_leave_the_map() {
        let budget = Budget::new(1, 64);
        let held = budget.acquire("held.example", Priority::Playback).await;
        for i in 0..2 * MAX_HOSTS {
            drop(
                budget
                    .acquire(&format!("h{i}.example"), Priority::Playback)
                    .await,
            );
        }
        let len = budget.hosts.lock().unwrap().len();
        assert!(len <= MAX_HOSTS, "{len} hosts kept");
        // A host in use keeps its semaphore, and with it its limit.
        let again = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            budget.acquire("held.example", Priority::Playback),
        )
        .await;
        assert!(again.is_err(), "the held host's limit was lost");
        drop(held);
    }
}
