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

/// The fork's clear of empty pieces on `h` (the keeper's answer to APFS's
/// filled holes), under the keeper lock.
pub async fn clear_empty(
    engine: &Engine,
    h: crate::TorrentHandle,
    pieces: &[u32],
) -> Option<librqbit::Evicted> {
    let t = engine.torrent(h).ok()?;
    let managed = t.live.lock().await.as_ref()?.managed.clone();
    let _round = t.keeper.lock().await;
    managed.clear_empty_pieces(pieces).await.ok()
}

/// The free space `engine` reads from now on (`None`: the volume's).
pub fn set_free(engine: &Engine, free: Option<u64>) {
    *lock(&engine.inner.free) = free;
}

/// Free space that follows the data folder from now on: `base` less what
/// the folder holds, as a volume's would (`None`: the volume's).
pub fn set_free_base(engine: &Engine, base: Option<u64>) {
    *lock(&engine.inner.free_base) = base;
}

/// `h` stops as on a fatal write error (review r1 m5).
pub async fn fail_torrent(engine: &Engine, h: crate::TorrentHandle) {
    let Ok(t) = engine.torrent(h) else { return };
    if let Some(l) = t.live.lock().await.as_ref() {
        l.managed
            .stop_with_error_for_tests(anyhow::Error::from(std::io::Error::from_raw_os_error(28)));
    }
}

/// What a replan of `h` would read of its windows now (review r1 M1).
pub struct SeenWindows(crate::window::Seen);

pub fn seen(engine: &Engine, h: crate::TorrentHandle) -> Option<SeenWindows> {
    let t = engine.torrent(h).ok()?;
    Some(SeenWindows(crate::window::Seen::of(&t)))
}

/// A replan under the floor of `file` at `free`, after `seen`: whether it
/// set its window.
pub async fn replan(
    engine: &Engine,
    h: crate::TorrentHandle,
    file: usize,
    bitrate: u64,
    free: u64,
    seen: SeenWindows,
) -> bool {
    engine.replan(h, file, bitrate, false, free, seen.0).await
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

/// The fork's three counts of what is held: the per-file counts summed,
/// `have_bytes`, and the have-needed-selected count. They agree whenever no
/// piece is between its claim and its count.
pub async fn have_counts(engine: &Engine, h: crate::TorrentHandle) -> Option<(u64, u64, u64)> {
    let t = engine.torrent(h).ok()?;
    let live = t.live.lock().await;
    let managed = &live.as_ref()?.managed;
    let state = managed.live()?;
    let files: u64 = managed.stats().file_progress.iter().sum();
    Some((
        files,
        state.get_approx_have_bytes(),
        state.get_hns()?.have_bytes,
    ))
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
