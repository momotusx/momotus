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

/// Asks `peer` for one block of `piece` outside the scheduler, as a late
/// or stale request would land (plans/streaming-window.md phase 1). `false`
/// when the torrent is not live or the peer is not connected.
pub async fn request_block(
    engine: &Engine,
    h: crate::TorrentHandle,
    peer: std::net::SocketAddr,
    (piece, begin, len): (u32, u32, u32),
) -> bool {
    let Ok(t) = engine.torrent(h) else {
        return false;
    };
    let live = t.live.lock().await;
    let Some(state) = live.as_ref().and_then(|l| l.managed.live()) else {
        return false;
    };
    state.ext_request(peer, piece, begin, len)
}

/// Pauses `h` without the keeper lock, as librqbit's own pause on a fatal
/// error does, so a test can pause in the middle of a round.
pub async fn pause_skipping_the_keeper(engine: &Engine, h: crate::TorrentHandle) {
    let Ok(t) = engine.torrent(h) else {
        return;
    };
    let live = t.live.lock().await;
    if let Some(l) = live.as_ref() {
        let _ = engine.inner.session.pause(&l.managed).await;
        let _ = l.msgs.send(crate::adapter::Msg::Stopped);
    }
}

/// Piece `p`'s state in the fork (claim, chunk bits set and in all, gated)
/// and the live peers, for a test's failure message.
pub async fn piece_state(engine: &Engine, h: crate::TorrentHandle, p: u32) -> String {
    let Ok(t) = engine.torrent(h) else {
        return "no torrent".into();
    };
    let live = t.live.lock().await;
    let Some(l) = live.as_ref() else {
        return "not live".into();
    };
    let peers = l.managed.live().map(|s| {
        let mut states: Vec<&str> = s
            .per_peer_stats_snapshot(librqbit::PeerStatsFilter {
                state: librqbit::PeerStatsFilterState::All,
            })
            .peers
            .values()
            .map(|p| p.state)
            .collect();
        states.sort_unstable();
        states
    });
    format!(
        "piece {p}: {:?} (have, bits set, bits, gated); peers {peers:?}; paused {}",
        l.managed.piece_state(p),
        l.managed.is_paused()
    )
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
