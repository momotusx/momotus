//! Sign-in with the device code (`traktLogin` view): a code to type or
//! scan, polled until Trakt answers; on approval the core adopts the record
//! and the `trakt` view offers it to the app to store.

use super::Trakt;
use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_library::hlc::now_ms;
use mq_tracking::trakt::auth::{self, Poll};
use mq_tracking::trakt::{Gate, call};
use serde::Serialize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Added to the poll interval on every "slow down".
const SLOW_DOWN: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum TraktLoginSnapshot {
    Starting,
    /// Show `userCode` and `url`, and `qr` as a QR code, until `expiresMs`.
    #[serde(rename_all = "camelCase")]
    Code {
        user_code: String,
        url: String,
        qr: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        expires_ms: u64,
    },
    /// Signed in; the record waits in the `trakt` view.
    Done {
        user: String,
    },
    Expired,
    Denied,
    NotConfigured,
    Failed {
        reason: String,
    },
}

/// One sign-in. A second open stops the first; dropping it stops polling.
pub struct TraktLoginView {
    snapshot: SharedObservable<TraktLoginSnapshot>,
    cancel: CancellationToken,
}

impl TraktLoginView {
    pub fn open(core: &Core) -> Self {
        let snapshot = SharedObservable::new(TraktLoginSnapshot::Starting);
        let cancel = CancellationToken::new();
        let trakt = core.trakt();
        if trakt.config().is_none() {
            snapshot.set(TraktLoginSnapshot::NotConfigured);
        } else {
            let conn = core.trakt_conn();
            if let Some(older) = super::lock(&conn.login).replace(cancel.clone()) {
                older.cancel();
            }
            tokio::spawn(sign_in(
                core.clone(),
                trakt,
                snapshot.clone(),
                cancel.clone(),
            ));
        }
        Self { snapshot, cancel }
    }

    pub fn snapshot(&self) -> TraktLoginSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        TraktLoginSnapshot,
        Pin<Box<dyn Stream<Item = Vec<TraktLoginSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for TraktLoginView {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn sign_in(
    core: Core,
    trakt: Arc<Trakt>,
    snapshot: SharedObservable<TraktLoginSnapshot>,
    cancel: CancellationToken,
) {
    let set = |s: TraktLoginSnapshot| {
        if !cancel.is_cancelled() {
            snapshot.set(s);
        }
    };
    let Some((api, _)) = trakt.api() else {
        return set(TraktLoginSnapshot::NotConfigured);
    };
    let code = tokio::select! {
        () = cancel.cancelled() => return,
        code = auth::start(&api) => match code {
            Ok(code) => code,
            Err(reason) => return set(TraktLoginSnapshot::Failed { reason }),
        },
    };
    let deadline = tokio::time::Instant::now() + code.expires_in;
    set(TraktLoginSnapshot::Code {
        user_code: code.user_code.clone(),
        url: code.verification_url.clone(),
        // ponytail: the bare activation page; whether a code in its path
        // pre-fills is for the P0 live check.
        qr: code.verification_url.clone(),
        expires_ms: now_ms() + code.expires_in.as_millis() as u64,
    });
    let mut interval = code.interval;
    let tokens = loop {
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(interval) => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return set(TraktLoginSnapshot::Expired);
        }
        match auth::poll(&api, &code).await {
            Poll::Approved(tokens) => break tokens,
            Poll::Pending => {}
            Poll::SlowDown => interval += SLOW_DOWN,
            // Network trouble: polling goes on until the code expires.
            Poll::Transient(reason) => tracing::debug!("trakt poll: {reason}"),
            Poll::Expired => return set(TraktLoginSnapshot::Expired),
            Poll::Denied => return set(TraktLoginSnapshot::Denied),
            Poll::Invalid => {
                return set(TraktLoginSnapshot::Failed {
                    reason: "the code was already used".into(),
                });
            }
        }
    };
    // Once approved the sign-in completes even if the view closed: the
    // tokens are single use.
    let (account, user) = match call::user(&api, &Gate::default(), &tokens.access_token).await {
        Ok(found) => found,
        Err(e) => {
            return set(TraktLoginSnapshot::Failed {
                reason: super::reason(&e),
            });
        }
    };
    let conn = core.trakt_conn();
    let record = conn.auth.adopt(tokens, &account, &user).await;
    if let Err(e) = core.trakt_connected(&conn, &record.account, record.generation) {
        tracing::warn!("trakt: sign-in not recorded: {e}");
    }
    core.trakt_refresh(&conn);
    set(TraktLoginSnapshot::Done { user });
}
