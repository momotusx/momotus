//! A window's size (plans/streaming-window.md 2.1, 2.2): the engine plays
//! a file larger than its room in it, and the ranking shows such a file
//! only when it holds 120 s ahead (3.1). One function, so both count the
//! same parts.

use crate::Config;

const MIB: u64 = 1 << 20;

/// Free space a window or a whole file never takes (2.1): Android counts a
/// volume low on storage at `min(5% of total, 500 MiB)`.
pub const RESERVE_BYTES: u64 = 512 * MIB;

/// APFS keeps a file's first 16 MiB allocated through a punch (2.6): on
/// Apple targets a window counts it in `F`, and the scheduler's budget
/// leaves it out, so the folder stays within `W`.
pub const APFS_HEAD_BYTES: u64 = if cfg!(target_vendor = "apple") {
    16 * MIB
} else {
    0
};

/// What a window needs to know of the device and the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowInputs {
    /// Free bytes on the torrent folder's volume.
    pub free: u64,
    /// `R`: what idle torrents hold.
    pub reclaimable: u64,
    /// `C`: what the other windows playing will still take.
    pub committed: u64,
    /// `r`, the file's bytes per second.
    pub bitrate: u64,
    pub piece_len: u64,
    /// The P2P speed estimate in bytes per second (`h` and `open`).
    pub fill: u64,
    /// Of `reclaimable`, the file's own torrent's folder: a window's keeper
    /// frees it (its other files first), nothing frees it for a whole file.
    pub own: u64,
    /// What the file itself already holds.
    pub own_file: u64,
}

impl WindowInputs {
    /// `room = free + R - C - reserve`: what a window may hold.
    pub fn room(&self) -> u64 {
        (self.free + self.reclaimable).saturating_sub(self.committed + RESERVE_BYTES)
    }

    /// A file of `len` bytes plays whole (2.1): what it still needs fits in
    /// the room without its own torrent's folder, which no keeper frees for
    /// a whole file (a pack's other episodes stay).
    pub fn fits_whole(&self, len: u64) -> bool {
        let others = Self {
            reclaimable: self.reclaimable.saturating_sub(self.own),
            ..*self
        };
        len.saturating_sub(self.own_file) <= others.room()
    }
}

/// One torrent folder's bytes on disk (2.1, 3.1): the folder, each of its
/// files when the torrent is loaded, and whether `R` counts it (idle).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldBytes {
    pub folder: u64,
    pub files: Vec<u64>,
    pub idle: bool,
}

impl WindowInputs {
    /// A row's inputs from the counted `R`: its own torrent's folder counts
    /// in `R` (a window's keeper frees it) and as `own`, and `file`'s bytes
    /// as `own_file`.
    pub fn with_held(mut self, held: Option<&HeldBytes>, file: Option<usize>) -> Self {
        if let Some(h) = held {
            if !h.idle {
                self.reclaimable += h.folder;
            }
            self.own = h.folder;
            self.own_file = file.and_then(|i| h.files.get(i)).copied().unwrap_or(0);
        }
        self
    }
}

/// A window's parts, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPlan {
    /// `W = min(W_cap, room)`: what the torrent may hold.
    pub budget: u64,
    /// `B`, kept behind the focused read.
    pub behind: u64,
    /// `h`: the keeper starts past `W - h / 2` and evicts down to `W - h`.
    pub slack: u64,
    /// The readahead past the playback window, fixed for the window.
    pub ahead: u64,
    /// Readahead pieces partly received at once stay within
    /// `clamp(2 s × fill, open_floor, open_cap)`.
    pub open_floor: u64,
    pub open_cap: u64,
    /// `W_min = F + (critical + playback) × r`: the least a window can play
    /// in.
    pub floor: u64,
    /// The critical and playback windows, `(critical + playback) × r`.
    pub playing: u64,
}

impl WindowPlan {
    /// The walk-only override (plans/streaming-window.md 4): a budget of at
    /// most `limit`, the cut taken off the readahead and then off `B`, so
    /// the kept set still fits; `None` under the window's floor.
    pub fn within(self, limit: u64) -> Option<Self> {
        if limit < self.floor {
            return None;
        }
        let cut = self.budget.saturating_sub(limit);
        Some(Self {
            budget: self.budget - cut,
            ahead: self.ahead.saturating_sub(cut),
            behind: self.behind.saturating_sub(cut.saturating_sub(self.ahead)),
            ..self
        })
    }

    /// Seconds of the file kept ahead of the read: `(W - F - B) / r`, the
    /// readahead and the critical and playback windows (3.1).
    pub fn ahead_secs(&self, bitrate: u64) -> u64 {
        (self.ahead + self.playing) / bitrate.max(1)
    }
}

/// Sizes a window with `cfg`'s windows, or `None` when `room` is under
/// `W_min + 64 MiB` (the device is full).
pub fn window_plan(cfg: &Config, i: WindowInputs) -> Option<WindowPlan> {
    let (r, piece) = (i.bitrate.max(1), i.piece_len.max(1));
    let room = i.room();
    let round = |b: u64| b.div_ceil(piece) * piece;
    let playing = cfg.critical_min_bytes.max(cfg.critical_secs * r) + cfg.playback_secs * r;
    let behind = (90 * r).max(32 * MIB).min(room / 4);
    let slack = (8 * MIB).max(2 * piece).max(2 * i.fill);
    let open_floor = (4 * piece).max(8 * MIB);
    let open_cap = (64 * MIB).min(room / 8).max(open_floor);
    let open = (2 * i.fill).clamp(open_floor, open_cap);
    // The head's blocking window and the container index, rounded out to
    // pieces.
    let pinned = round(cfg.blocking_bytes) + round(cfg.tail_bytes) + piece;
    let fixed = pinned + cfg.blocking_bytes + slack + open + playing + APFS_HEAD_BYTES;
    let cap = fixed + behind + playing + 600 * r;
    let floor = fixed + playing;
    if room < floor + 64 * MIB {
        return None;
    }
    let budget = cap.min(room);
    Some(WindowPlan {
        budget,
        behind,
        slack,
        ahead: budget.saturating_sub(fixed + behind + playing).min(600 * r),
        open_floor,
        open_cap,
        floor,
        playing,
    })
}
