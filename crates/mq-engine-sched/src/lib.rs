//! DWH streaming scheduler: deadline windows, block-level urgent fan-out,
//! hedging.
//!
//! The scheduler is pure and deterministic. It owns no sockets, threads or
//! clocks: a torrent engine adapter feeds it [`Event`]s (time arrives only
//! through [`Event::Tick`]) and executes the [`Action`]s it returns. The same
//! event sequence always produces the same actions.
//!
//! See the crate README for the algorithm, its sources and the simulator
//! results.

mod plan;
mod sched;

pub use plan::{RESERVE_BYTES, WindowInputs, WindowPlan, window_plan};
pub use sched::Scheduler;

use std::ops::Range;

/// Request size on the wire (BEP 3 convention).
pub const BLOCK: u32 = 16 * 1024;

pub type PeerId = u32;
pub type DemandId = u64;
pub type PieceIdx = u32;
/// Block index: block `b % bpp` of piece `b / bpp`, where `bpp` is
/// [`Geometry::blocks_per_piece`]. Blocks are counted from each piece's
/// start, as librqbit counts chunks, so a piece length that is not a
/// multiple of [`BLOCK`] ends every piece in a short block. Use
/// [`Geometry::wire`] for the `(piece, begin, length)` triple of a REQUEST
/// or CANCEL message.
pub type BlockIdx = u32;

/// Fixed-size bit set used for peer bitfields and local piece state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitfield {
    words: Vec<u64>,
    len: u32,
}

impl Bitfield {
    pub fn new(len: u32) -> Self {
        Self {
            words: vec![0; len.div_ceil(64) as usize],
            len,
        }
    }

    /// All bits set (a seeder, or BEP 6 HAVE ALL).
    pub fn full(len: u32) -> Self {
        let mut b = Self {
            words: vec![u64::MAX; len.div_ceil(64) as usize],
            len,
        };
        if !len.is_multiple_of(64)
            && let Some(last) = b.words.last_mut()
        {
            *last = (1u64 << (len % 64)) - 1;
        }
        b
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: u32) -> bool {
        i < self.len && (self.words[(i / 64) as usize] >> (i % 64)) & 1 == 1
    }

    /// Out-of-range indexes are ignored.
    pub fn set(&mut self, i: u32, v: bool) {
        if i >= self.len {
            return;
        }
        let w = &mut self.words[(i / 64) as usize];
        if v {
            *w |= 1 << (i % 64);
        } else {
            *w &= !(1 << (i % 64));
        }
    }

    pub fn count_ones(&self) -> u32 {
        self.words.iter().map(|w| w.count_ones()).sum()
    }
}

/// Byte extent of one file inside the torrent's concatenated payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSpan {
    pub offset: u64,
    pub len: u64,
}

/// Torrent shape: piece length, total length, file offsets, 16 KiB blocks
/// counted from each piece's start.
#[derive(Clone, Debug)]
pub struct Geometry {
    piece_len: u32,
    total: u64,
    files: Vec<FileSpan>,
}

impl Geometry {
    /// `None` unless `piece_len` is positive and the payload is non-empty and
    /// addressable with `u32` block indexes (64 TiB). Any piece length works:
    /// YTS torrents use 136 KiB, 8.5 blocks.
    pub fn new(piece_len: u32, file_lens: &[u64]) -> Option<Self> {
        if piece_len == 0 {
            return None;
        }
        let mut offset = 0u64;
        let mut files = Vec::with_capacity(file_lens.len());
        for &len in file_lens {
            files.push(FileSpan { offset, len });
            offset = offset.checked_add(len)?;
        }
        let geo = Self {
            piece_len,
            total: offset,
            files,
        };
        (offset > 0 && geo.blocks() <= u64::from(u32::MAX)).then_some(geo)
    }

    /// Blocks of every piece before the last, plus the last piece's own.
    fn blocks(&self) -> u64 {
        let len = u64::from(self.piece_len);
        let full = self.total.saturating_sub(1) / len;
        full * u64::from(self.blocks_per_piece())
            + (self.total - full * len).div_ceil(u64::from(BLOCK))
    }

    pub fn piece_len(&self) -> u32 {
        self.piece_len
    }

    pub fn total_len(&self) -> u64 {
        self.total
    }

    pub fn files(&self) -> &[FileSpan] {
        &self.files
    }

    pub fn num_pieces(&self) -> u32 {
        self.total.div_ceil(self.piece_len as u64) as u32
    }

    pub fn num_blocks(&self) -> u32 {
        self.blocks() as u32
    }

    pub fn blocks_per_piece(&self) -> u32 {
        self.piece_len.div_ceil(BLOCK)
    }

    /// Bytes in piece `p`: `piece_len`, less for the last, 0 past the end.
    pub fn piece_size(&self, p: PieceIdx) -> u32 {
        let start = u64::from(p) * u64::from(self.piece_len);
        self.total
            .saturating_sub(start)
            .min(u64::from(self.piece_len)) as u32
    }

    pub fn piece_of(&self, b: BlockIdx) -> PieceIdx {
        b / self.blocks_per_piece()
    }

    /// Piece holding torrent byte `offset`.
    pub fn piece_at(&self, offset: u64) -> PieceIdx {
        (offset / self.piece_len as u64) as u32
    }

    pub fn piece_blocks(&self, p: PieceIdx) -> Range<BlockIdx> {
        let start = p
            .saturating_mul(self.blocks_per_piece())
            .min(self.num_blocks());
        start..start + self.piece_size(p).div_ceil(BLOCK)
    }

    /// The block a PIECE message carries: byte `begin` of piece `piece`.
    pub fn block_at(&self, piece: PieceIdx, begin: u32) -> BlockIdx {
        piece
            .saturating_mul(self.blocks_per_piece())
            .saturating_add(begin / BLOCK)
    }

    pub fn block_len(&self, b: BlockIdx) -> u32 {
        let bpp = self.blocks_per_piece();
        self.piece_size(b / bpp)
            .saturating_sub(b % bpp * BLOCK)
            .min(BLOCK)
    }

    /// `(piece, begin, length)` as carried by REQUEST and CANCEL messages.
    pub fn wire(&self, b: BlockIdx) -> (PieceIdx, u32, u32) {
        let bpp = self.blocks_per_piece();
        (b / bpp, (b % bpp) * BLOCK, self.block_len(b))
    }

    /// Pieces overlapping torrent bytes `[start, end)`.
    pub fn pieces_in(&self, start: u64, end: u64) -> Range<PieceIdx> {
        let end = end.min(self.total);
        if start >= end {
            return 0..0;
        }
        self.piece_at(start)..self.piece_at(end - 1) + 1
    }
}

/// Scheduling knobs. Use a device-class preset and override fields as needed.
#[derive(Clone, Debug)]
pub struct Config {
    /// Bytes from a reader's position that it waits on now (rounded out to
    /// whole pieces, never less than the piece under the position).
    pub blocking_bytes: u64,
    /// Critical window is `max(critical_min_bytes, critical_secs * bitrate)`.
    pub critical_min_bytes: u64,
    pub critical_secs: u64,
    /// Playback window after the critical one, in seconds of media.
    pub playback_secs: u64,
    /// Readahead budget after the playback window.
    pub readahead_bytes: u64,
    /// Container index at the end of each opened file (MP4 `moov`, MKV Cues).
    /// Zero disables the tail class and the tail part of readiness.
    pub tail_bytes: u64,
    /// Bytes per second assumed when the adapter gave no bitrate
    /// (10 Mbit/s, a 1080p remux; the adapter passes 40 Mbit/s for 2160p).
    pub default_bitrate: u64,
    /// Critical block deadline is `now + (offset - play_pos) / bitrate -
    /// safety_ms`; blocking and tail blocks are due immediately.
    pub safety_ms: u64,
    /// Urgent fan-out stops once the best peer's queue exceeds this.
    pub max_queue_ms: u64,
    /// Slowest share of unchoked peers that never get urgent blocks.
    pub slow_peer_pct: u64,
    /// Hedge an urgent block that will miss its deadline and whose ETA
    /// exceeds `max(hedge_min_ms, p90 block service time)`.
    pub hedge_min_ms: u64,
    /// Hedge bytes in the last `hedge_window_ms` stay within this percentage
    /// of payload received in the same window...
    pub hedge_budget_pct: u64,
    /// ...or within this floor, so a cold start (no payload yet) can hedge.
    pub hedge_floor_bytes: u64,
    pub hedge_window_ms: u64,
    /// Per-peer depth is `2 + ceil(rate * queue_ms / BLOCK)` clamped to
    /// `[min_depth, min(reqq, max_depth)]`.
    pub queue_ms: u64,
    pub min_depth: u32,
    pub max_depth: u32,
    /// A peer that delivers no payload for this long while we wait on it is
    /// snubbed: depth 1, no urgent blocks, until it delivers again.
    pub snub_ms: u64,
    /// Rate assumed for a peer before any sample exists and no other peer
    /// has one either.
    pub initial_peer_rate: u64,
    /// Periodic work (snubbing, hedging) runs at most this often.
    pub tick_ms: u64,
}

const MIB: u64 = 1024 * 1024;

impl Config {
    /// 1 GB RAM sticks.
    pub fn low_ram() -> Self {
        Self {
            readahead_bytes: 64 * MIB,
            max_depth: 128,
            ..Self::standard()
        }
    }

    /// 2 GB RAM sticks and phones (the default).
    pub fn standard() -> Self {
        Self {
            blocking_bytes: MIB,
            critical_min_bytes: 4 * MIB,
            critical_secs: 3,
            playback_secs: 30,
            readahead_bytes: 128 * MIB,
            tail_bytes: 2 * MIB,
            default_bitrate: 10_000_000 / 8,
            safety_ms: 1000,
            max_queue_ms: 2000,
            slow_peer_pct: 10,
            hedge_min_ms: 750,
            hedge_budget_pct: 5,
            hedge_floor_bytes: 256 * 1024,
            hedge_window_ms: 10_000,
            queue_ms: 1000,
            min_depth: 4,
            max_depth: 256,
            snub_ms: 10_000,
            initial_peer_rate: 64 * 1024,
            tick_ms: 100,
        }
    }

    /// Shield, Apple TV 4K, desktops.
    pub fn high_end() -> Self {
        Self {
            readahead_bytes: 256 * MIB,
            ..Self::standard()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::standard()
    }
}

/// Inputs from the adapter. Every event is applied at the time of the last
/// [`Event::Tick`]; send a tick whenever the clock has moved.
#[derive(Clone, Debug)]
pub enum Event {
    /// Clock in microseconds from any fixed origin. Never goes backwards
    /// (earlier values are ignored).
    Tick {
        now_us: u64,
    },
    /// An HTTP range request was parsed: send this before reading the body.
    /// `start` and `end_hint` (exclusive) are file-relative byte offsets.
    RangeOpened {
        demand: DemandId,
        file: usize,
        start: u64,
        end_hint: Option<u64>,
    },
    /// The reader of `demand` now needs file byte `pos` next.
    RangeAdvanced {
        demand: DemandId,
        pos: u64,
    },
    RangeClosed {
        demand: DemandId,
    },
    /// Player position, if the adapter knows it. Bytes the player already
    /// buffered beyond it shrink the critical window.
    PlayPosition {
        file: usize,
        pos: u64,
    },
    /// `reqq` is the peer's advertised request queue limit (BEP 10), or the
    /// adapter's default (250) when the peer sent none.
    PeerConnected {
        peer: PeerId,
        have: Bitfield,
        reqq: u32,
    },
    PeerDisconnected {
        peer: PeerId,
    },
    PeerHave {
        peer: PeerId,
        piece: PieceIdx,
    },
    /// Payload rate sample (0 when nothing arrived in the sample window).
    PeerRate {
        peer: PeerId,
        bytes_per_s: u64,
        rtt_us: u64,
    },
    /// Without BEP 6 a choke drops every pending request; the scheduler
    /// forgets them all.
    PeerChoked {
        peer: PeerId,
    },
    PeerUnchoked {
        peer: PeerId,
    },
    BlockReceived {
        peer: PeerId,
        block: BlockIdx,
    },
    BlockRejected {
        peer: PeerId,
        block: BlockIdx,
    },
    BlockTimedOut {
        peer: PeerId,
        block: BlockIdx,
    },
    PieceVerified {
        piece: PieceIdx,
    },
    PieceFailed {
        piece: PieceIdx,
    },
    /// The piece left the disk (plans/streaming-window.md 2.5): its blocks
    /// are needed again, as after `PieceFailed`, with no failure.
    PieceEvicted {
        piece: PieceIdx,
    },
    /// Where `file` keeps the container index a player reads before its
    /// first frame (README 12); it replaces the fixed tail as urgent work
    /// and in readiness.
    FileIndex {
        file: usize,
        index: FileIndex,
    },
}

/// A file's container index, as the adapter knows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileIndex {
    /// No index of its own: the last `tail_bytes` (Matroska `Cues`).
    #[default]
    Tail,
    /// Not known yet (an MP4 whose head is still on its way): only the
    /// blocking window is urgent, and readiness waits.
    Pending,
    /// File bytes `[start, end)`: an MP4 `moov`, with what precedes it when
    /// it comes first.
    At { start: u64, end: u64 },
}

/// Decisions for the adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Request {
        peer: PeerId,
        block: BlockIdx,
    },
    Cancel {
        peer: PeerId,
        block: BlockIdx,
    },
    /// The demand's blocking window and the file's index (its tail until a
    /// `FileIndex`) are verified: the stream can answer without stalling on
    /// its first bytes.
    Ready {
        demand: DemandId,
    },
}

/// A windowed file (plans/streaming-window.md 2.2, 2.3): the torrent holds
/// only part of it, and [`Scheduler::kept`] and [`Scheduler::evictable`]
/// say which part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub file: usize,
    /// What the torrent may hold (`W`): no piece outside the urgent windows
    /// starts past it.
    pub budget: u64,
    /// The keeper's slack (`h`): it is due past `budget - slack / 2` and
    /// evicts down to `budget - slack`.
    pub slack: u64,
    /// Bytes kept behind the focused read (`B`).
    pub behind: u64,
    /// The ramp's floor: the focused read's readahead is
    /// `clamp(2 × bytes read since its last seek, floor, the file's
    /// readahead)`.
    pub ramp_floor: u64,
    /// Readahead pieces partly received at once, counted at full size, stay
    /// within `clamp(2 s × the fill rate, open_floor, open_cap)`.
    pub open_floor: u64,
    pub open_cap: u64,
}

/// Priority class of a wanted piece, most urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    Blocking,
    Tail,
    Critical,
    Playback,
    Readahead,
}

impl Class {
    /// Urgent classes get block-level fan-out and hedging.
    pub fn is_urgent(self) -> bool {
        self <= Class::Critical
    }
}

/// Counters for diagnostics and the simulator.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub requests: u64,
    pub cancels: u64,
    pub hedges: u64,
    pub hedge_bytes: u64,
    pub payload_bytes: u64,
    /// Payload that arrived for blocks we already had.
    pub dup_bytes: u64,
    pub snubs: u64,
    pub seeks: u64,
}
