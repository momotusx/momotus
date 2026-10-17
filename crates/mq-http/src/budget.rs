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

// ponytail: per-host semaphores are never evicted; fine for the few hundred
// hosts a client talks to, add LRU eviction if that ever grows unbounded.
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
}
