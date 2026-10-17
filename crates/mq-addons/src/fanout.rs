use crate::{AddonClient, AddonError, InstalledAddon};
use bytes::Bytes;
use mq_protocol::ResourcePath;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

/// `soft`: after this, pending addons are reported as slow (results still
/// arrive). `hard`: pending addons are cancelled and reported as timed out.
#[derive(Debug, Clone, Copy)]
pub struct Deadlines {
    pub soft: Duration,
    pub hard: Duration,
}

impl Default for Deadlines {
    fn default() -> Self {
        Self {
            soft: Duration::from_millis(2500),
            hard: Duration::from_secs(12),
        }
    }
}

#[derive(Debug)]
pub enum AddonEvent {
    Arrived {
        addon_id: String,
        body: Bytes,
        from_cache: bool,
        latency: Duration,
    },
    Slow {
        addon_ids: Vec<String>,
    },
    Failed {
        addon_id: String,
        error: AddonError,
    },
    /// Emitted once, after every addon has arrived, failed or timed out.
    Done,
}

/// A running fan-out. Dropping it cancels every pending request.
pub struct Fanout {
    rx: mpsc::UnboundedReceiver<AddonEvent>,
    _tasks: JoinSet<()>,
}

impl Fanout {
    pub(crate) fn start(
        client: AddonClient,
        addons: Vec<InstalledAddon>,
        path: ResourcePath,
        deadlines: Deadlines,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut tasks = JoinSet::new();
        tasks.spawn(drive(client, addons, path, deadlines, tx));
        Self { rx, _tasks: tasks }
    }

    pub async fn next(&mut self) -> Option<AddonEvent> {
        self.rx.recv().await
    }
}

async fn drive(
    client: AddonClient,
    addons: Vec<InstalledAddon>,
    path: ResourcePath,
    deadlines: Deadlines,
    tx: mpsc::UnboundedSender<AddonEvent>,
) {
    let mut pending: Vec<String> = addons.iter().map(|a| a.id().to_owned()).collect();
    let mut calls = JoinSet::new();
    for addon in addons {
        let client = client.clone();
        let path = path.clone();
        calls.spawn(async move {
            let result = client.fetch(&addon, &path, deadlines.hard).await;
            (addon.id().to_owned(), result)
        });
    }
    let soft = tokio::time::sleep(deadlines.soft);
    let hard = tokio::time::sleep(deadlines.hard);
    tokio::pin!(soft, hard);
    let mut soft_fired = false;
    while !pending.is_empty() {
        tokio::select! {
            joined = calls.join_next() => {
                let (addon_id, result) = match joined {
                    None => break,
                    // A panicked call stays pending and is reported at the hard deadline.
                    Some(Err(_)) => continue,
                    Some(Ok(done)) => done,
                };
                pending.retain(|id| id != &addon_id);
                let event = match result {
                    Ok(f) => AddonEvent::Arrived { addon_id, body: f.body, from_cache: f.from_cache, latency: f.latency },
                    Err(error) => AddonEvent::Failed { addon_id, error },
                };
                if tx.send(event).is_err() { return; }
            }
            _ = &mut soft, if !soft_fired => {
                soft_fired = true;
                if tx.send(AddonEvent::Slow { addon_ids: pending.clone() }).is_err() { return; }
            }
            _ = &mut hard => {
                calls.abort_all();
                for addon_id in pending.drain(..) {
                    let _ = tx.send(AddonEvent::Failed { addon_id, error: AddonError::TimedOut });
                }
            }
        }
    }
    let _ = tx.send(AddonEvent::Done);
}
