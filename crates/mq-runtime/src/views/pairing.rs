//! Pairing on the new device: shows the QR, waits for a device of the
//! account to approve it, then claims a token and hands the joined secrets
//! to the platform for its keystore (plans/tv-qr-login.md). The approving
//! phone checked that this screen is in front of it (the same network, or a
//! live scan behind its screen lock) before it sent anything, so nothing is
//! asked here. The code renews itself for as long as the view is open
//! (the owner's rule, Q5): each new code takes the old one's place a little
//! before it ends, and the old one is still collected through its grace, so
//! a scan right at the switch still signs the screen in. A code that cannot
//! start, or a join that fails, is tried again with a new code: the view
//! ends only joined.

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_sync::SyncConfig;
use mq_sync::pair::{Offer, PairingRequest, start_pairing_as};
use serde::Serialize;
use std::pin::Pin;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// How often the screen asks whether its code was answered: every 2 s while
/// a scan is likely (its first 5 minutes), then every 5 s, so a TV left on its
/// first screen for days costs the relay little and still sees a Sign in within 5 s.
fn poll_every(open: Duration) -> Duration {
    Duration::from_secs(if open.as_secs() < 300 { 2 } else { 5 })
}
/// The next code shows this long before the current one ends (at most a
/// fifth of a code's life).
const RENEW_EARLY: Duration = Duration::from_secs(30);
/// A code that gave way is still collected this long past its end (the
/// relay keeps a filled slot as long).
const GRACE: Duration = Duration::from_secs(60);
/// A code that could not start is tried again after this.
const RETRY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum PairingSnapshot {
    Starting,
    /// Show `qr` as a QR code; a new one replaces it in place.
    #[serde(rename_all = "camelCase")]
    Waiting {
        qr: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        expires_ms: u64,
    },
    /// A device of the account approved: this one is joining.
    Joining,
    /// Store this record in the keystore; sync can start. `by`: the name of
    /// the device that approved, as the account knows it.
    Joined {
        #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
        record: Box<SyncConfig>,
        by: Option<String>,
    },
}

pub struct PairingView {
    snapshot: SharedObservable<PairingSnapshot>,
    task: JoinHandle<()>,
}

impl PairingView {
    /// `name` is how other devices of the account will see this one (also
    /// shown by the approving phone), `form` what it is (`phone`, `tablet`,
    /// `tv`, `computer`, `web`).
    pub fn open(core: &Core, server: &str, name: &str, form: Option<&str>) -> Self {
        let snapshot = SharedObservable::new(PairingSnapshot::Starting);
        let task = tokio::spawn(run(
            core.clone(),
            server.to_owned(),
            [name.to_owned(), form.unwrap_or_default().to_owned()],
            snapshot.clone(),
        ));
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

/// A code on screen or in its grace: the request and until when it is
/// polled.
struct Live {
    request: PairingRequest,
    until: Instant,
}

async fn run(
    core: Core,
    server: String,
    [name, form]: [String; 2],
    snapshot: SharedObservable<PairingSnapshot>,
) {
    let form = Some(form.as_str()).filter(|f| !f.is_empty());
    let opened = Instant::now();
    // The code on screen, and the one that gave way to it.
    let mut current: Option<(Live, Instant)> = None;
    let mut previous: Option<Live> = None;
    loop {
        // Time for a new code: none yet, or the one shown is near its end.
        if current
            .as_ref()
            .is_none_or(|(_, renew_at)| Instant::now() >= *renew_at)
        {
            match start_pairing_as(core.http(), &server, &name, form).await {
                Ok(request) => {
                    let ends = Instant::from_std(request.sent) + request.ttl;
                    let early = RENEW_EARLY.min(request.ttl / 5);
                    snapshot.set(PairingSnapshot::Waiting {
                        qr: request.qr.clone(),
                        expires_ms: request.expires_ms,
                    });
                    let renew_at = ends.checked_sub(early).unwrap_or(ends);
                    if let Some((old, _)) = current.replace((
                        Live {
                            request,
                            until: ends + GRACE,
                        },
                        renew_at,
                    )) {
                        previous = Some(old);
                    }
                }
                // The last code still works until its end; a new one is asked
                // for again shortly (a relay that was away, or busy).
                Err(e) => {
                    tracing::debug!("pairing code not started: {e}");
                    if current.is_none() {
                        tokio::time::sleep(RETRY).await;
                        continue;
                    }
                    if let Some((_, renew_at)) = current.as_mut() {
                        *renew_at = Instant::now() + RETRY;
                    }
                }
            }
        }
        tokio::time::sleep(poll_every(opened.elapsed())).await;
        // Each live code once: the one that gave way first, then the current.
        if previous.as_ref().is_some_and(|p| Instant::now() >= p.until) {
            previous = None;
        }
        let mut found = None;
        if let Some(old) = &previous {
            match collect(&core, &old.request).await {
                Poll::Offer(offer) => found = previous.take().map(|l| (l, offer)),
                Poll::Ended => previous = None,
                Poll::Waiting => {}
            }
        }
        if found.is_none()
            && let Some((live, _)) = &current
        {
            match collect(&core, &live.request).await {
                Poll::Offer(offer) => found = current.take().map(|(l, _)| (l, offer)),
                // Answered with junk, or gone: the next round shows a new code.
                Poll::Ended => current = None,
                Poll::Waiting => {}
            }
        }
        let Some((live, offer)) = found else { continue };
        snapshot.set(PairingSnapshot::Joining);
        match live
            .request
            .claim_as(core.http(), *offer, &name, form)
            .await
        {
            Ok((record, by)) => {
                core.joined_sync_account();
                snapshot.set(PairingSnapshot::Joined {
                    record: Box::new(record),
                    by,
                });
                return;
            }
            // The account moved while joining, or the network went: a new
            // code, as if this one had ended.
            Err(e) => {
                tracing::info!("pairing join failed, showing a new code: {e}");
                current = None;
                previous = None;
            }
        }
    }
}

enum Poll {
    Waiting,
    Offer(Box<Offer>),
    /// Nothing more can come of this code.
    Ended,
}

/// One look at a slot.
async fn collect(core: &Core, request: &PairingRequest) -> Poll {
    match request.poll(core.http()).await {
        Ok(Some(offer)) => Poll::Offer(Box::new(offer)),
        Ok(None) => Poll::Waiting,
        // The relay's 404: the slot ended (or was taken).
        Err(mq_sync::SyncError::Http(mq_http::HttpError::Status { status: 404, .. })) => {
            Poll::Ended
        }
        // A dropped connection is retried (the kind only: the error text
        // keeps the relay's host).
        Err(mq_sync::SyncError::Http(e)) => {
            tracing::debug!("pairing poll failed: {}", e.kind());
            Poll::Waiting
        }
        // A reply that does not open, or that names another relay, ends
        // this code.
        Err(e) => {
            tracing::info!("pairing reply refused: {e}");
            Poll::Ended
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_poll_slows_as_the_screen_stays_open() {
        let at = |s| poll_every(Duration::from_secs(s)).as_secs();
        assert_eq!(
            [at(0), at(299), at(300), at(3600), at(86_400)],
            [2, 2, 5, 5, 5]
        );
    }
}
