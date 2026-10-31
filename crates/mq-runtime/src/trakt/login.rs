//! Sign-in with the device code (`traktLogin` view): a code to type or
//! scan, polled until Trakt answers; on approval the core adopts the record
//! and the `trakt` view offers it to the app to store. A sign-in adopts only
//! while it still owns the connection: a newer sign-in, a sign-out or a
//! record from the app meanwhile wins.

use crate::Core;
use crate::core::WeakCore;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_library::hlc::now_ms;
use mq_tracking::trakt::auth::{self, Poll};
use mq_tracking::trakt::{Gate, Ticket, call};
use serde::Serialize;
use std::pin::Pin;
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

/// One sign-in. A second open overtakes the first; dropping it stops the
/// polls (an approval already given still completes).
pub struct TraktLoginView {
    snapshot: SharedObservable<TraktLoginSnapshot>,
    polls: CancellationToken,
}

impl TraktLoginView {
    pub fn open(core: &Core) -> Self {
        let snapshot = SharedObservable::new(TraktLoginSnapshot::Starting);
        let polls = CancellationToken::new();
        let trakt = core.trakt();
        if trakt.config().is_none() {
            snapshot.set(TraktLoginSnapshot::NotConfigured);
        } else if super::kids(core) {
            snapshot.set(TraktLoginSnapshot::Failed {
                reason: super::KIDS.into(),
            });
        } else {
            let conn = core.trakt_conn();
            // Owned until overtaken, and never past the profile's end.
            let owner = conn.stop.child_token();
            if let Some(older) = super::lock(&conn.login).replace(owner.clone()) {
                older.cancel();
            }
            let ticket = futures_util::FutureExt::now_or_never(conn.auth.ticket())
                .expect("the auth lock never waits");
            let login = Login {
                core: core.downgrade(),
                snapshot: snapshot.clone(),
                polls: polls.clone(),
                owner,
                ticket,
            };
            tokio::spawn(login.run());
        }
        Self { snapshot, polls }
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
        self.polls.cancel();
    }
}

struct Login {
    /// Weak: a sign-in never keeps the runtime alive.
    core: WeakCore,
    snapshot: SharedObservable<TraktLoginSnapshot>,
    /// The view's: polling stops when it closes.
    polls: CancellationToken,
    /// Cancelled by a newer sign-in, a sign-out or the profile's end: the
    /// polls stop.
    owner: CancellationToken,
    /// Taken when it started; only the newest sign-in's adopts.
    ticket: Ticket,
}

impl Login {
    fn set(&self, s: TraktLoginSnapshot) {
        if !self.polls.is_cancelled() {
            self.snapshot.set(s);
        }
    }

    /// A newer sign-in, a sign-out or the profile's end took the
    /// connection: this one ends without adopting anything.
    fn overtaken(&self) {
        self.set(TraktLoginSnapshot::Failed {
            reason: "Trakt was signed out or signed in again meanwhile".into(),
        });
    }

    async fn run(self) {
        let api = self.core.upgrade().and_then(|core| core.trakt().api());
        let Some((api, _)) = api else {
            return self.set(TraktLoginSnapshot::NotConfigured);
        };
        let code = tokio::select! {
            () = self.polls.cancelled() => return,
            () = self.owner.cancelled() => return self.overtaken(),
            code = auth::start(&api) => match code {
                Ok(code) => code,
                Err(reason) => {
                    // Its status and Trakt's code go to the log only.
                    tracing::debug!("trakt sign-in not started: {reason}");
                    let reason = "Trakt did not start the sign-in".into();
                    return self.set(TraktLoginSnapshot::Failed { reason });
                }
            },
        };
        let deadline = tokio::time::Instant::now() + code.expires_in;
        self.set(TraktLoginSnapshot::Code {
            user_code: code.user_code.clone(),
            url: code.verification_url.clone(),
            // The activation page with the code in its path: Trakt fills the
            // code in (checked live, 2026-10-19). The page and the code stay
            // as text for typing.
            qr: format!(
                "{}/{}",
                code.verification_url.trim_end_matches('/'),
                code.user_code
            ),
            expires_ms: now_ms() + code.expires_in.as_millis() as u64,
        });
        let mut interval = code.interval;
        let tokens = loop {
            tokio::select! {
                () = self.polls.cancelled() => return,
                () = self.owner.cancelled() => return self.overtaken(),
                () = tokio::time::sleep(interval) => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return self.set(TraktLoginSnapshot::Expired);
            }
            match auth::poll(&api, &code).await {
                Poll::Approved(tokens) => break tokens,
                Poll::Pending => {}
                Poll::SlowDown => interval += SLOW_DOWN,
                // Network trouble: polling goes on until the code expires.
                Poll::Transient(reason) => tracing::debug!("trakt poll: {reason}"),
                Poll::Expired => return self.set(TraktLoginSnapshot::Expired),
                Poll::Denied => return self.set(TraktLoginSnapshot::Denied),
                Poll::Invalid => {
                    return self.set(TraktLoginSnapshot::Failed {
                        reason: "the code was already used".into(),
                    });
                }
            }
        };
        // Once approved the sign-in completes even if the view closed (the
        // tokens are single use), but never once overtaken.
        let found = call::user(&api, &Gate::default(), &tokens.access_token).await;
        let (account, user) = match found {
            Ok(found) => found,
            Err(e) => {
                self.set(TraktLoginSnapshot::Failed {
                    reason: super::reason(&e),
                });
                // Approved but never taken: Trakt drops them too.
                return auth::revoke(&api, &tokens.access_token).await;
            }
        };
        let Some(core) = self.core.upgrade() else {
            return;
        };
        // A read error is no turn to kids: the tokens are taken, and
        // `trakt_connected` asks again.
        if super::kids_now(&core) == Some(true) {
            // Turned kids while it polled: approved, never taken.
            self.set(TraktLoginSnapshot::Failed {
                reason: super::KIDS.into(),
            });
            return auth::revoke(&api, &tokens.access_token).await;
        }
        let conn = core.trakt_conn();
        // The ticket is checked and the record written under one lock: a
        // newer sign-in, a sign-out or a record from the app meanwhile wins.
        let approved = tokens.access_token.clone();
        let Some(record) = conn.auth.adopt(tokens, &account, &user, self.ticket).await else {
            self.overtaken();
            // Approved but not taken: Trakt drops them too.
            return auth::revoke(&api, &approved).await;
        };
        if let Err(e) = core.trakt_connected(&conn, &record.account, record.generation) {
            tracing::warn!("trakt: sign-in not recorded: {e}");
        }
        core.trakt_refresh(&conn);
        self.snapshot.set(TraktLoginSnapshot::Done { user });
    }
}
