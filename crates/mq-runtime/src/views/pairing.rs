//! Pairing on the new device: shows the QR, waits for a device of the
//! account to approve it, then hands the joined secrets to the platform for
//! its keystore.

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_sync::pair::{Joined, start_pairing};
use serde::Serialize;
use std::pin::Pin;
use std::time::Duration;
use tokio::task::JoinHandle;

const POLL_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum PairingSnapshot {
    Starting,
    /// Show `qr` as a QR code until `expiresMs`.
    #[serde(rename_all = "camelCase")]
    Waiting {
        qr: String,
        expires_ms: u64,
    },
    /// Store these in the keystore; sync can start.
    Joined {
        joined: Joined,
    },
    Expired,
    Failed {
        reason: String,
    },
}

pub struct PairingView {
    snapshot: SharedObservable<PairingSnapshot>,
    task: JoinHandle<()>,
}

impl PairingView {
    pub fn open(core: &Core, server: &str) -> Self {
        let snapshot = SharedObservable::new(PairingSnapshot::Starting);
        let task = tokio::spawn(run(core.clone(), server.to_owned(), snapshot.clone()));
        Self { snapshot, task }
    }

    pub fn snapshot(&self) -> PairingSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        PairingSnapshot,
        Pin<Box<dyn Stream<Item = Vec<PairingSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for PairingView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(core: Core, server: String, snapshot: SharedObservable<PairingSnapshot>) {
    let request = match start_pairing(core.http(), &server).await {
        Ok(request) => request,
        Err(e) => {
            snapshot.set(PairingSnapshot::Failed {
                reason: e.to_string(),
            });
            return;
        }
    };
    snapshot.set(PairingSnapshot::Waiting {
        qr: request.qr.clone(),
        expires_ms: request.expires_ms,
    });
    loop {
        tokio::time::sleep(POLL_EVERY).await;
        if mq_library::hlc::now_ms() >= request.expires_ms {
            snapshot.set(PairingSnapshot::Expired);
            return;
        }
        match request.poll(core.http()).await {
            Ok(Some(joined)) => {
                snapshot.set(PairingSnapshot::Joined { joined });
                return;
            }
            Ok(None) => {}
            // A dropped connection is retried; a reply that does not open is
            // final (someone else filled the slot).
            Err(mq_sync::SyncError::Http(e)) => tracing::debug!("pairing poll failed: {e}"),
            Err(e) => {
                snapshot.set(PairingSnapshot::Failed {
                    reason: e.to_string(),
                });
                return;
            }
        }
    }
}
