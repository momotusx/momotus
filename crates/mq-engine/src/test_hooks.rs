//! A barrier inside range setup, so a test can cancel a caller while its
//! range is half made: its demand open, the torrent's initialization not yet
//! awaited. Only the `test-hooks` feature compiles it.

use std::sync::Arc;

use tokio::sync::{Notify, watch};

use crate::{Engine, lock};

pub struct Hold {
    reached: Notify,
    open: watch::Sender<bool>,
}

impl Hold {
    /// Waits until a range's setup stops at the barrier.
    pub async fn reached(&self) {
        self.reached.notified().await;
    }

    /// Lets every held setup, and every later one, go on.
    pub fn open(&self) {
        self.open.send_replace(true);
    }
}

/// Holds every range `engine` opens from now on at the barrier, until
/// [`Hold::open`].
pub fn hold_ranges(engine: &Engine) -> Arc<Hold> {
    let hold = Arc::new(Hold {
        reached: Notify::new(),
        open: watch::channel(false).0,
    });
    *lock(&engine.inner.hold) = Some(hold.clone());
    hold
}

pub(crate) async fn wait(engine: &Engine) {
    let hold = lock(&engine.inner.hold).clone();
    if let Some(hold) = hold {
        hold.reached.notify_one();
        let mut open = hold.open.subscribe();
        let _ = open.wait_for(|open| *open).await;
    }
}
