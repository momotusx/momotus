//! Barriers for tests of callers that race the engine: one inside range
//! setup, so a test can cancel a caller while its range is half made (its
//! demand open, the torrent's initialization not yet awaited); one inside a
//! disk budget eviction, between its first check and the one under the
//! torrents lock, so a test can claim the torrent in between. Only the
//! `test-hooks` feature compiles them.

use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, watch};

use crate::{Engine, lock};

pub struct Hold {
    reached: Notify,
    open: watch::Sender<bool>,
}

impl Hold {
    /// Waits until a held step stops at the barrier.
    pub async fn reached(&self) {
        self.reached.notified().await;
    }

    /// Lets every held step, and every later one, go on.
    pub fn open(&self) {
        self.open.send_replace(true);
    }
}

/// Holds every range `engine` opens from now on at the barrier, until
/// [`Hold::open`].
pub fn hold_ranges(engine: &Engine) -> Arc<Hold> {
    hold_in(&engine.inner.hold)
}

/// Holds every disk budget eviction `engine` makes from now on after its
/// first check, until [`Hold::open`].
pub fn hold_evictions(engine: &Engine) -> Arc<Hold> {
    hold_in(&engine.inner.evict_hold)
}

pub(crate) async fn wait(engine: &Engine) {
    pass(&engine.inner.hold).await;
}

pub(crate) async fn wait_eviction(engine: &Engine) {
    pass(&engine.inner.evict_hold).await;
}

fn hold_in(slot: &Mutex<Option<Arc<Hold>>>) -> Arc<Hold> {
    let hold = Arc::new(Hold {
        reached: Notify::new(),
        open: watch::channel(false).0,
    });
    *lock(slot) = Some(hold.clone());
    hold
}

async fn pass(slot: &Mutex<Option<Arc<Hold>>>) {
    let hold = lock(slot).clone();
    if let Some(hold) = hold {
        hold.reached.notify_one();
        let mut open = hold.open.subscribe();
        let _ = open.wait_for(|open| *open).await;
    }
}
