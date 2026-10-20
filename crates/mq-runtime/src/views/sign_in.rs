//! Sign-in through a relay's brokered flow (plans/accounts.md 3.4): the view
//! opens the flow and shows the provider page's URL; the app opens it in a
//! browser and, when the redirect comes back to it, hands the fragment over
//! with `returned`. The result is a ticket: `found` (it signs in to an
//! account) or `new`; an Apple revoke ends with its receipt. The verifier
//! stays in memory, and the code in the fragment is used here only.

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_sync::identity::start_brokered;
use mq_sync::wire::Taken;
use serde::Serialize;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum SignInSnapshot {
    Starting,
    /// Open `url` in a browser; its redirect back goes to `returned`. The
    /// relay takes the flow until `expiresMs` (relay time).
    Browser {
        url: String,
        #[serde(rename = "expiresMs")]
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        expires_ms: u64,
    },
    /// The return arrived; the result is being taken.
    Waiting,
    /// The identity signs in to `account`.
    Found {
        ticket: String,
        account: String,
    },
    /// The identity is not linked: create an account with the ticket, or
    /// link it after joining.
    New {
        ticket: String,
    },
    /// An Apple revoke ran: send the receipt with the unlink or deletion.
    Receipt {
        receipt: String,
        ticket: String,
        account: Option<String>,
    },
    /// `code` as the core names errors (`cancelled`, `ticketExpired`,
    /// `signInRefused`, `busy`, ...), or `unreachable` when the relay or
    /// the provider did not answer.
    Failed {
        code: String,
    },
}

impl SignInSnapshot {
    /// What a finished flow (brokered or native) shows.
    pub fn taken(taken: Taken) -> Self {
        // An id writes itself as lowercase hex.
        let account = taken
            .account
            .and_then(|a| serde_json::to_value(a).ok())
            .and_then(|v| v.as_str().map(str::to_owned));
        match (taken.receipt, account) {
            (Some(receipt), account) => SignInSnapshot::Receipt {
                receipt,
                ticket: taken.ticket,
                account,
            },
            (None, Some(account)) => SignInSnapshot::Found {
                ticket: taken.ticket,
                account,
            },
            (None, None) => SignInSnapshot::New {
                ticket: taken.ticket,
            },
        }
    }

    pub fn failed(e: &mq_sync::SyncError) -> Self {
        SignInSnapshot::Failed {
            code: e.code().unwrap_or("unreachable").to_owned(),
        }
    }
}

pub struct SignInView {
    snapshot: SharedObservable<SignInSnapshot>,
    back: mpsc::Sender<String>,
    task: JoinHandle<()>,
}

impl SignInView {
    /// `return_to` is where the provider sends the user back (the app's
    /// `marquee://auth`, or a loopback page); `revoke` runs Apple's revoke.
    pub fn open(core: &Core, server: &str, provider: &str, return_to: &str, revoke: bool) -> Self {
        let snapshot = SharedObservable::new(SignInSnapshot::Starting);
        let (back, returned) = mpsc::channel(4);
        let task = tokio::spawn(run(
            core.clone(),
            [server.to_owned(), provider.to_owned(), return_to.to_owned()],
            revoke,
            snapshot.clone(),
            returned,
        ));
        Self {
            snapshot,
            back,
            task,
        }
    }

    /// The redirect came back to the app: its fragment. Only this flow's
    /// return from this relay goes on; any other is ignored and the view
    /// keeps waiting.
    pub fn returned(&self, fragment: &str) {
        if matches!(self.snapshot.get(), SignInSnapshot::Browser { .. }) {
            let _ = self.back.try_send(fragment.to_owned());
        }
    }

    pub fn subscribe(
        &self,
    ) -> (
        SignInSnapshot,
        Pin<Box<dyn Stream<Item = Vec<SignInSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for SignInView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(
    core: Core,
    [server, provider, return_to]: [String; 3],
    revoke: bool,
    snapshot: SharedObservable<SignInSnapshot>,
    mut returned: mpsc::Receiver<String>,
) {
    let end = |state: SignInSnapshot| drop(snapshot.set(state));
    let flow = match start_brokered(core.http(), &server, &provider, &return_to, revoke).await {
        Ok(flow) => flow,
        Err(e) => return end(SignInSnapshot::failed(&e)),
    };
    snapshot.set(SignInSnapshot::Browser {
        url: flow.url.clone(),
        expires_ms: flow.expires_ms,
    });
    let fragment = loop {
        let Some(fragment) = returned.recv().await else {
            return;
        };
        if flow.owns(&fragment) {
            break fragment;
        }
        // A late return of an abandoned flow, or a link another app fired.
        tracing::info!("a sign-in return for another flow was ignored");
    };
    snapshot.set(SignInSnapshot::Waiting);
    end(match flow.finish(core.http(), &fragment).await {
        Ok(taken) => SignInSnapshot::taken(taken),
        Err(e) => SignInSnapshot::failed(&e),
    });
}
