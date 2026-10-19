//! Pairing on the new device: shows the QR, waits for a device of the
//! account to approve it, asks the user to confirm both screens show the
//! same code, then claims a token and hands the joined secrets to the
//! platform for its keystore. Nothing is kept, and no token exists, before
//! the user confirms.

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_sync::SyncConfig;
use mq_sync::pair::start_pairing;
use serde::Serialize;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const POLL_EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum PairingSnapshot {
    Starting,
    /// Show `qr` as a QR code until `expiresMs`.
    #[serde(rename_all = "camelCase")]
    Waiting {
        qr: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        expires_ms: u64,
    },
    /// Ask: does the other device show this code? Then `confirm` or `reject`.
    Confirm {
        code: String,
    },
    /// Store this record in the keystore; sync can start.
    Joined {
        #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
        record: Box<SyncConfig>,
    },
    Expired,
    Failed {
        reason: String,
    },
}

pub struct PairingView {
    snapshot: SharedObservable<PairingSnapshot>,
    decision: Mutex<Option<oneshot::Sender<bool>>>,
    task: JoinHandle<()>,
}

impl PairingView {
    /// `name` is how other devices of the account will see this one.
    pub fn open(core: &Core, server: &str, name: &str) -> Self {
        let snapshot = SharedObservable::new(PairingSnapshot::Starting);
        let (decide, decision) = oneshot::channel();
        let task = tokio::spawn(run(
            core.clone(),
            server.to_owned(),
            name.to_owned(),
            snapshot.clone(),
            decision,
        ));
        Self {
            snapshot,
            decision: Mutex::new(Some(decide)),
            task,
        }
    }

    /// The user saw the same code on both devices.
    pub fn confirm(&self) {
        self.decide(true);
    }

    /// The codes differ: someone else answered the QR. Nothing is kept.
    pub fn reject(&self) {
        self.decide(false);
    }

    fn decide(&self, accept: bool) {
        if matches!(self.snapshot.get(), PairingSnapshot::Confirm { .. })
            && let Some(decide) = self
                .decision
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
        {
            let _ = decide.send(accept);
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
    name: String,
    snapshot: SharedObservable<PairingSnapshot>,
    decision: oneshot::Receiver<bool>,
) {
    let fail = |reason: String| PairingSnapshot::Failed { reason };
    let end = |state: PairingSnapshot| drop(snapshot.set(state));
    let request = match start_pairing(core.http(), &server).await {
        Ok(request) => request,
        Err(e) => return end(fail(e.to_string())),
    };
    snapshot.set(PairingSnapshot::Waiting {
        qr: request.qr.clone(),
        expires_ms: request.expires_ms,
    });
    let offer = loop {
        tokio::time::sleep(POLL_EVERY).await;
        if mq_library::hlc::now_ms() >= request.expires_ms {
            return end(PairingSnapshot::Expired);
        }
        match request.poll(core.http()).await {
            Ok(Some(offer)) => break offer,
            Ok(None) => {}
            // A dropped connection is retried; a reply that does not open is
            // final.
            // The kind only: the error text keeps the relay's host.
            Err(mq_sync::SyncError::Http(e)) => {
                tracing::debug!("pairing poll failed: {}", e.kind())
            }
            Err(e) => return end(fail(e.to_string())),
        }
    };
    snapshot.set(PairingSnapshot::Confirm {
        code: offer.code.clone(),
    });
    match decision.await {
        Ok(true) => match request.claim(core.http(), offer, &name).await {
            Ok(record) => {
                core.joined_sync_account();
                end(PairingSnapshot::Joined {
                    record: Box::new(record),
                })
            }
            Err(e) => end(fail(e.to_string())),
        },
        _ => end(fail("the codes did not match; nothing was kept".into())),
    }
}
