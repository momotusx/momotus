//! A window's size (plans/streaming-window.md 2.1, 2.2): the engine plays
//! a file larger than its room in it, a ring when the room is short
//! (plans/streaming-window-chunks.md), and the ranking says how far ahead
//! it holds (3.1). One function, so both count the same parts.

use crate::Config;

const MIB: u64 = 1 << 20;

/// Free space a window or a whole file never takes (2.1): Android counts a
/// volume low on storage at `min(5% of total, 500 MiB)`.
pub const RESERVE_BYTES: u64 = 512 * MIB;

/// The least ring a window plays in when the room cannot hold `W_min`
/// (plans/streaming-window-chunks.md): Stremio's 45 MiB RAM ring on the
/// owner's Fire TV, and at least two pieces (the one read and the next),
/// so a torrent of large pieces does not take a large share of a stick's
/// RAM.
pub const RING_BYTES: u64 = 45 * MIB;

/// The most a RAM ring holds, whatever the device's memory
/// (plans/no-space-warnings.md): under three times Stremio's 45 MiB ring.
pub const RAM_CEILING_BYTES: u64 = 128 * MIB;

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
    /// What a whole file writes into its neighbours: its first and last
    /// pieces' bytes of the files beside it in a multi-file torrent.
    pub boundary: u64,
    /// The memory the device has free, when the platform says: a RAM ring
    /// takes at most a quarter of it, within [`RAM_CEILING_BYTES`].
    pub available_ram: Option<u64>,
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
        len.saturating_sub(self.own_file) + self.boundary <= others.room()
    }
}

/// One torrent folder's bytes on disk (2.1, 3.1): the folder, each of its
/// files when the torrent is loaded (a folder an earlier launch left with
/// one file: that file), whether `R` counts it (idle), and what its own
/// window will still take (counted in `C`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeldBytes {
    pub folder: u64,
    pub files: Vec<u64>,
    pub idle: bool,
    pub committed: u64,
}

impl WindowInputs {
    /// A row's inputs from the counted `R` and `C`, as the app counts them
    /// for its torrent: its own folder in `R` (a window's keeper frees it)
    /// and as `own`, its own window out of `C`, and `file`'s bytes as
    /// `own_file` (a single file's when the row names none). A torrent
    /// with more than one file, or one not known yet, writes up to a piece
    /// of its neighbours at each end of the file (`piece`).
    pub fn with_held(mut self, held: Option<&HeldBytes>, file: Option<usize>, piece: u64) -> Self {
        self.boundary = if held.is_some_and(|h| h.files.len() == 1) {
            0
        } else {
            2 * piece
        };
        if let Some(h) = held {
            if !h.idle {
                self.reclaimable += h.folder;
            }
            self.committed = self.committed.saturating_sub(h.committed);
            self.own = h.folder;
            let file = file.or((h.files.len() == 1).then_some(0));
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
    /// The critical and playback windows, `(critical + playback) × r`; in
    /// a ring, what it holds ahead.
    pub playing: u64,
    /// A ring the disk cannot hold: its pieces stay in RAM
    /// (plans/streaming-window-chunks.md).
    pub memory: bool,
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

/// Sizes a window with `cfg`'s windows. A room under `W_min + 64 MiB` gets
/// a ring of `max(room, RING)` instead, in RAM when the room is under the
/// ring (plans/streaming-window-chunks.md): a version never fails for
/// space. A RAM ring holds at most a quarter of the free memory, within
/// [`RAM_CEILING_BYTES`], with two pieces open; one whose two pieces do not
/// fit there stays on disk (plans/no-space-warnings.md).
pub fn window_plan(cfg: &Config, i: WindowInputs) -> WindowPlan {
    let (r, piece) = (i.bitrate.max(1), i.piece_len.max(1));
    let round = |b: u64| b.div_ceil(piece) * piece;
    let playing = cfg.critical_min_bytes.max(cfg.critical_secs * r) + cfg.playback_secs * r;
    let pinned = round(cfg.blocking_bytes) + round(cfg.tail_bytes) + piece;
    // The parts that follow the room: `B`, `h`, the open pieces, and `F`
    // less the playing windows.
    let parts = |room: u64| {
        let behind = (90 * r).max(32 * MIB).min(room / 4);
        let slack = (8 * MIB).max(2 * piece).max(2 * i.fill);
        let open_floor = (4 * piece).max(8 * MIB);
        let open_cap = (64 * MIB).min(room / 8).max(open_floor);
        let open = (2 * i.fill).clamp(open_floor, open_cap);
        // The head's blocking window and the container index, rounded out
        // to pieces.
        let fixed = pinned + cfg.blocking_bytes + slack + open + APFS_HEAD_BYTES;
        (behind, slack, open_floor, open_cap, fixed)
    };
    let room = i.room();
    let (behind, slack, open_floor, open_cap, fixed) = parts(room);
    let fixed = fixed + playing;
    let floor = fixed + playing;
    if room < floor + 64 * MIB {
        let mut budget = room.max(RING_BYTES.max(2 * piece));
        let ram = i
            .available_ram
            .map_or(RAM_CEILING_BYTES, |a| a / 4)
            .min(RAM_CEILING_BYTES);
        let memory = room < budget.min(ram) && 2 * piece <= ram;
        if memory {
            budget = budget.min(ram);
        }
        let (_, slack, mut open_floor, mut open_cap, _) = parts(budget);
        if memory {
            (open_floor, open_cap) = (open_floor.min(2 * piece), open_cap.min(2 * piece));
        }
        let behind = budget / 4;
        return WindowPlan {
            budget,
            behind,
            slack: slack.min(budget / 4),
            ahead: 0,
            open_floor,
            open_cap,
            floor: budget,
            playing: budget.saturating_sub(behind + pinned + APFS_HEAD_BYTES),
            memory,
        };
    }
    let cap = fixed + behind + playing + 600 * r;
    let budget = cap.min(room);
    WindowPlan {
        budget,
        behind,
        slack,
        ahead: budget.saturating_sub(fixed + behind + playing).min(600 * r),
        open_floor,
        open_cap,
        floor,
        playing,
        memory: false,
    }
}
