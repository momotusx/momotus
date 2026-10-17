//! Pairing on the new device: shows the QR, waits for a device of the
//! account to approve it, asks the user to confirm both screens show the
//! same code, then hands the joined secrets to the platform for its
//! keystore. Nothing is kept before the user confirms.

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_sync::pair::{Joined, Offer, start_pairing};
use serde::Serialize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
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
    /// Ask: does the other device show this code? Then `confirm` or `reject`.
    Confirm {
        code: String,
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
    offer: Arc<Mutex<Option<Offer>>>,
    task: JoinHandle<()>,
}

impl PairingView {
    pub fn open(core: &Core, server: &str) -> Self {
        let snapshot = SharedObservable::new(PairingSnapshot::Starting);
        let offer = Arc::new(Mutex::new(None));
        let task = tokio::spawn(run(
            core.clone(),
            server.to_owned(),
            snapshot.clone(),
            offer.clone(),
        ));
        Self {
            snapshot,
            offer,
            task,
        }
    }

    /// The user saw the same code on both devices.
    pub fn confirm(&self) {
        if let Some(offer) = self.offer.lock().unwrap_or_else(|p| p.into_inner()).take() {
            self.snapshot.set(PairingSnapshot::Joined {
                joined: offer.joined,
            });
        }
    }

    /// The codes differ: someone else answered the QR. Nothing was kept.
    pub fn reject(&self) {
        if self
            .offer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .is_some()
        {
            self.snapshot.set(PairingSnapshot::Failed {
                reason: "the codes did not match; nothing was kept".into(),
            });
        }
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

async fn run(
    core: Core,
    server: String,
    snapshot: SharedObservable<PairingSnapshot>,
    offer: Arc<Mutex<Option<Offer>>>,
) {
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
            Ok(Some(received)) => {
                let code = received.code.clone();
                *offer.lock().unwrap_or_else(|p| p.into_inner()) = Some(received);
                snapshot.set(PairingSnapshot::Confirm { code });
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
