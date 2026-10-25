//! Unit tests and property tests for the DWH scheduler.
//!
//! `Harness` plays the adapter: it mirrors what was requested, received and
//! verified, and checks every action the scheduler emits against the
//! invariants.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use mq_engine_sched::{
    Action, BLOCK, Bitfield, BlockIdx, Class, Config, DemandId, Event, FileIndex, Geometry,
    HeldBytes, PeerId, PieceIdx, RESERVE_BYTES, Scheduler, StartBytes, Window, WindowInputs,
};
use proptest::prelude::*;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

struct Harness {
    s: Scheduler,
    geo: Geometry,
    cfg: Config,
    now: u64,
    peers: BTreeMap<PeerId, (Bitfield, bool, u32)>,
    out: BTreeSet<(BlockIdx, PeerId)>,
    received: Bitfield,
    verified: Bitfield,
    open: BTreeMap<DemandId, (usize, u64, Option<u64>)>,
    ready: BTreeSet<DemandId>,
    payload: VecDeque<(u64, u64)>,
    hedges: VecDeque<(u64, u64)>,
    log: Vec<Action>,
    /// Container index per file, as sent.
    index: BTreeMap<usize, FileIndex>,
}

impl Harness {
    fn new(geo: Geometry, cfg: Config) -> Self {
        Self {
            s: Scheduler::new(geo.clone(), cfg.clone()),
            received: Bitfield::new(geo.num_blocks()),
            verified: Bitfield::new(geo.num_pieces()),
            geo,
            cfg,
            now: 0,
            peers: BTreeMap::new(),
            out: BTreeSet::new(),
            open: BTreeMap::new(),
            ready: BTreeSet::new(),
            payload: VecDeque::new(),
            hedges: VecDeque::new(),
            log: Vec::new(),
            index: BTreeMap::new(),
        }
    }

    fn holders(&self, b: BlockIdx) -> Vec<PeerId> {
        self.out
            .range((b, 0)..=(b, PeerId::MAX))
            .map(|&(_, p)| p)
            .collect()
    }

    fn peer_out(&self, peer: PeerId) -> usize {
        self.out.iter().filter(|&&(_, p)| p == peer).count()
    }

    fn drop_peer_out(&mut self, peer: PeerId) {
        self.out.retain(|&(_, p)| p != peer);
    }

    fn blocking_pieces(&self, file: usize, start: u64, end: Option<u64>) -> Vec<PieceIdx> {
        let f = self.geo.files()[file];
        let limit = end.unwrap_or(f.len).min(f.len);
        let b_end = (start + self.cfg.blocking_bytes)
            .min(limit)
            .max(start + 1)
            .min(f.len);
        let mut v: Vec<PieceIdx> = self
            .geo
            .pieces_in(f.offset + start, f.offset + b_end)
            .collect();
        if let Some(&FileIndex::At { start, end }) = self.index.get(&file) {
            v.extend(
                self.geo
                    .pieces_in(f.offset + start, f.offset + end.min(f.len)),
            );
        } else if self.cfg.tail_bytes > 0 {
            let t = f.offset + f.len.saturating_sub(self.cfg.tail_bytes);
            v.extend(self.geo.pieces_in(t, f.offset + f.len));
        }
        v
    }

    fn send(&mut self, ev: Event) -> Vec<Action> {
        let seeks_before = self.s.stats().seeks;
        let out_before = self.out.clone();
        match &ev {
            Event::Tick { now_us } => self.now = self.now.max(*now_us),
            Event::PeerConnected { peer, have, reqq } => {
                self.drop_peer_out(*peer);
                self.peers.insert(*peer, (have.clone(), true, *reqq));
            }
            Event::PeerDisconnected { peer } => {
                self.drop_peer_out(*peer);
                self.peers.remove(peer);
            }
            Event::PeerHave { peer, piece } => {
                if let Some(p) = self.peers.get_mut(peer) {
                    p.0.set(*piece, true);
                }
            }
            Event::PeerChoked { peer } => {
                if let Some(p) = self.peers.get_mut(peer) {
                    p.1 = true;
                }
                self.drop_peer_out(*peer);
            }
            Event::PeerUnchoked { peer } => {
                if let Some(p) = self.peers.get_mut(peer) {
                    p.1 = false;
                }
            }
            Event::BlockReceived { peer, block } => {
                self.out.remove(&(*block, *peer));
                self.received.set(*block, true);
                self.payload
                    .push_back((self.now, self.geo.block_len(*block) as u64));
            }
            Event::BlockRejected { peer, block } | Event::BlockTimedOut { peer, block } => {
                self.out.remove(&(*block, *peer));
            }
            Event::PieceVerified { piece } => {
                self.verified.set(*piece, true);
                for b in self.geo.piece_blocks(*piece) {
                    self.received.set(b, true);
                }
            }
            Event::PieceFailed { piece } | Event::PieceEvicted { piece } => {
                self.verified.set(*piece, false);
                for b in self.geo.piece_blocks(*piece) {
                    self.received.set(b, false);
                }
            }
            Event::RangeOpened {
                demand,
                file,
                start,
                end_hint,
            } => {
                let start = (*start).min(self.geo.files()[*file].len);
                self.open.insert(*demand, (*file, start, *end_hint));
                self.ready.remove(demand);
            }
            Event::RangeClosed { demand } => {
                self.open.remove(demand);
            }
            Event::FileIndex { file, index } => {
                self.index.insert(*file, *index);
            }
            Event::RangeAdvanced { .. } | Event::PlayPosition { .. } | Event::PeerRate { .. } => {}
        }

        let mut acts = Vec::new();
        self.s.handle(ev, &mut acts);

        let mut requested = BTreeSet::new();
        for a in &acts {
            match *a {
                Action::Request { peer, block } => {
                    let piece = self.geo.piece_of(block);
                    let (p, begin, len) = self.geo.wire(block);
                    assert!(
                        p == piece && len > 0 && begin + len <= self.geo.piece_size(p),
                        "block {block} is not inside piece {piece}"
                    );
                    let (have, choked, _) = self.peers.get(&peer).expect("request to unknown peer");
                    assert!(
                        have.get(piece),
                        "requested block {block} from peer {peer} that lacks it"
                    );
                    assert!(!choked, "requested from choked peer {peer}");
                    assert!(
                        !self.received.get(block),
                        "requested a block we already have"
                    );
                    assert!(
                        self.out.insert((block, peer)),
                        "block {block} twice to peer {peer}"
                    );
                    let n = self.holders(block).len();
                    assert!(n <= 2, "block {block} in flight {n} times");
                    if n == 2 {
                        let class = self.s.class_of(piece);
                        assert!(
                            class.is_some_and(Class::is_urgent),
                            "duplicate of non-urgent block"
                        );
                        self.check_hedge_budget(self.geo.block_len(block) as u64);
                    }
                    requested.insert(peer);
                }
                Action::Cancel { peer, block } => {
                    assert!(self.out.remove(&(block, peer)), "cancel of unknown request");
                }
                Action::Ready { demand } => {
                    let &(file, start, end) =
                        self.open.get(&demand).expect("ready for closed demand");
                    assert!(self.ready.insert(demand), "ready twice");
                    for p in self.blocking_pieces(file, start, end) {
                        assert!(self.verified.get(p), "ready before piece {p} verified");
                    }
                }
            }
        }
        for peer in requested {
            let n = self.peer_out(peer);
            let reqq = self.peers[&peer].2;
            assert!(
                n <= self.s.peer_depth(peer).unwrap(),
                "peer {peer} over depth"
            );
            assert!(
                n as u32 <= reqq.min(self.cfg.max_depth).max(1),
                "peer {peer} over reqq"
            );
        }
        for &(b, p) in &self.out {
            if self.holders(b).len() > 1 {
                let class = self.s.class_of(self.geo.piece_of(b));
                assert!(
                    class.is_some_and(Class::is_urgent),
                    "stale duplicate on {b} at {p}"
                );
            }
        }
        for &peer in self.peers.keys() {
            assert_eq!(
                self.s.outstanding(peer),
                self.peer_out(peer),
                "peer {peer} queue drift"
            );
        }
        // Every request that is stale after a seek gets a CANCEL, so it
        // leaves the peer's FIFO (it may be requested again behind the new
        // urgent work).
        if self.s.stats().seeks > seeks_before {
            let cancelled: BTreeSet<(BlockIdx, PeerId)> = acts
                .iter()
                .filter_map(|a| match *a {
                    Action::Cancel { peer, block } => Some((block, peer)),
                    _ => None,
                })
                .collect();
            for &(b, p) in &out_before {
                let class = self.s.class_of(self.geo.piece_of(b));
                let stale = class.is_none_or(|c| c >= Class::Readahead);
                assert!(
                    !stale || cancelled.contains(&(b, p)),
                    "stale request {b} at {p} not cancelled on seek"
                );
            }
        }
        self.log.extend(acts.iter().cloned());
        acts
    }

    fn check_hedge_budget(&mut self, len: u64) {
        let horizon = self.now.saturating_sub(self.cfg.hedge_window_ms * 1000);
        self.payload.retain(|e| e.0 >= horizon);
        self.hedges.retain(|e| e.0 >= horizon);
        self.hedges.push_back((self.now, len));
        let payload: u64 = self.payload.iter().map(|e| e.1).sum();
        let hedged: u64 = self.hedges.iter().map(|e| e.1).sum();
        let cap = (payload * self.cfg.hedge_budget_pct / 100).max(self.cfg.hedge_floor_bytes);
        assert!(hedged <= cap, "hedged {hedged} bytes over cap {cap}");
    }

    fn tick(&mut self, ms: u64) -> Vec<Action> {
        let now_us = self.now + ms * 1000;
        self.send(Event::Tick { now_us })
    }

    fn peer(&mut self, peer: PeerId, rate: u64) {
        let have = Bitfield::full(self.geo.num_pieces());
        self.send(Event::PeerConnected {
            peer,
            have,
            reqq: 250,
        });
        self.send(Event::PeerRate {
            peer,
            bytes_per_s: rate,
            rtt_us: 50_000,
        });
        self.send(Event::PeerUnchoked { peer });
    }

    /// Deliver an outstanding block and verify its piece once complete.
    fn deliver(&mut self, peer: PeerId, block: BlockIdx) -> Vec<Action> {
        let mut acts = self.send(Event::BlockReceived { peer, block });
        let piece = self.geo.piece_of(block);
        if !self.verified.get(piece) && self.geo.piece_blocks(piece).all(|b| self.received.get(b)) {
            acts.extend(self.send(Event::PieceVerified { piece }));
        }
        acts
    }
}

fn requests(acts: &[Action]) -> Vec<(PeerId, BlockIdx)> {
    acts.iter()
        .filter_map(|a| match *a {
            Action::Request { peer, block } => Some((peer, block)),
            _ => None,
        })
        .collect()
}

fn small_cfg() -> Config {
    Config {
        blocking_bytes: 64 * KIB,
        critical_min_bytes: 128 * KIB,
        critical_secs: 1,
        playback_secs: 4,
        readahead_bytes: 256 * KIB,
        tail_bytes: 64 * KIB,
        default_bitrate: 32 * KIB,
        ..Config::standard()
    }
}

// ---- unit tests ----

#[test]
fn geometry_maps_blocks_pieces_and_files() {
    let g = Geometry::new(64 * KIB as u32, &[10_000, 200_000]).unwrap();
    assert_eq!(g.total_len(), 210_000);
    assert_eq!(g.num_pieces(), 4);
    assert_eq!(g.num_blocks(), 13);
    assert_eq!(g.files()[1].offset, 10_000);
    assert_eq!(g.block_len(12), 210_000 - 12 * BLOCK);
    assert_eq!(g.wire(5), (1, BLOCK, BLOCK));
    assert_eq!(g.piece_blocks(3), 12..13);
    assert_eq!(g.pieces_in(65_535, 65_537), 0..2);
    assert!(Geometry::new(0, &[1]).is_none());
    assert!(Geometry::new(BLOCK, &[0]).is_none());
    let full = Bitfield::full(70);
    assert_eq!(full.count_ones(), 70);
    assert!(!full.get(70));
}

/// The YTS Sintel shape (a779fa75): 1028 pieces of 136 KiB, 8.5 blocks, the
/// last piece shared by the video's end and three small files.
#[test]
fn unaligned_pieces_end_in_a_short_block() {
    let g = Geometry::new(136 * KIB as u32, &[143_100_616, 584, 53_226, 1_858]).unwrap();
    assert_eq!((g.num_pieces(), g.blocks_per_piece()), (1028, 9));
    assert_eq!(g.piece_blocks(0), 0..9);
    assert_eq!(g.wire(7), (0, 7 * BLOCK, BLOCK));
    assert_eq!(g.wire(8), (0, 8 * BLOCK, 8 * KIB as u32));
    assert_eq!(g.wire(9), (1, 0, BLOCK));
    assert_eq!(g.block_at(1, 8 * BLOCK), 17);
    assert_eq!(g.piece_of(17), 1);
    // The last piece holds 132,156 bytes: 8 full blocks and 1,084 bytes.
    assert_eq!(g.piece_size(1027), 132_156);
    assert_eq!(g.piece_blocks(1027), 9243..9252);
    assert_eq!(g.num_blocks(), 9252);
    assert_eq!(g.wire(9251), (1027, 8 * BLOCK, 1_084));
    let bytes: u64 = (0..g.num_blocks()).map(|b| u64::from(g.block_len(b))).sum();
    assert_eq!(bytes, g.total_len());
    for p in [0, 513, 1027] {
        let r = g.piece_blocks(p);
        let sum: u32 = r.clone().map(|b| g.block_len(b)).sum();
        assert_eq!(sum, g.piece_size(p));
        assert!(r.clone().all(|b| g.piece_of(b) == p));
        assert!(r.clone().all(|b| g.block_at(p, g.wire(b).1) == b));
    }
}

/// Aligned pieces keep the numbering they always had: block `b` starts at
/// torrent byte `b * BLOCK`, so their requests are unchanged.
#[test]
fn aligned_pieces_keep_global_block_numbers() {
    for (piece_len, files) in [
        (16 * KIB as u32, vec![1u64]),
        (64 * KIB as u32, vec![10_000, 200_000]),
        (256 * KIB as u32, vec![37_000, 5 * MIB + 12_345]),
        (4 << 20, vec![9 * MIB + 1]),
    ] {
        let g = Geometry::new(piece_len, &files).unwrap();
        let total = g.total_len();
        assert_eq!(u64::from(g.num_blocks()), total.div_ceil(u64::from(BLOCK)));
        for b in 0..g.num_blocks() {
            let start = u64::from(b) * u64::from(BLOCK);
            let len = (total - start).min(u64::from(BLOCK)) as u32;
            let piece = (start / u64::from(piece_len)) as u32;
            let begin = (start % u64::from(piece_len)) as u32;
            assert_eq!(g.wire(b), (piece, begin, len), "{piece_len} {b}");
        }
    }
}

/// Head and tail readiness on unaligned pieces shared across files, then a
/// seek into the middle and a read of the subtitle file, which lives in the
/// last piece with the end of the picture.
#[test]
fn unaligned_shared_pieces_reach_ready_and_seek() {
    // 40,000-byte pieces (2 blocks and 7,232 bytes): [nfo, video, jpg, srt].
    let g = Geometry::new(40_000, &[584, MIB + 1_234, 53_226, 1_858]).unwrap();
    let last = g.num_pieces() - 1;
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, MIB);
    let serve = |h: &mut Harness, demand: DemandId| {
        for _ in 0..1_000 {
            let Some(&(block, peer)) = h.out.iter().next() else {
                return false;
            };
            h.tick(1);
            if h.deliver(peer, block).contains(&Action::Ready { demand }) {
                return true;
            }
        }
        false
    };
    h.send(Event::RangeOpened {
        demand: 1,
        file: 1,
        start: 0,
        end_hint: None,
    });
    assert!(serve(&mut h, 1), "head and tail ready");
    let video = h.geo.files()[1];
    let tail = h.geo.piece_at(video.offset + video.len - 1);
    assert!(
        h.verified.get(0) && h.verified.get(tail),
        "shared pieces verified"
    );
    assert!(
        h.log.iter().any(
            |a| matches!(*a, Action::Request { block, .. } if h.geo.block_len(block) == 7_232)
        ),
        "short blocks requested"
    );
    h.send(Event::RangeOpened {
        demand: 2,
        file: 1,
        start: 600_000,
        end_hint: None,
    });
    assert!(serve(&mut h, 2), "seek ready");
    let acts = h.send(Event::RangeOpened {
        demand: 3,
        file: 3,
        start: 0,
        end_hint: None,
    });
    assert!(acts.contains(&Action::Ready { demand: 3 }) || serve(&mut h, 3));
    assert!(h.verified.get(last), "the srt's piece verified");
}

#[test]
fn depth_follows_bandwidth_delay_product() {
    let g = Geometry::new(256 * KIB as u32, &[64 * MIB]).unwrap();
    let mut h = Harness::new(g, Config::low_ram());
    h.peer(1, MIB);
    assert_eq!(h.s.peer_depth(1), Some(2 + 64));
    h.peer(2, 10 * MIB);
    assert_eq!(
        h.s.peer_depth(2),
        Some(128),
        "clamped to the 1 GB preset cap"
    );
    h.peer(3, 2 * KIB);
    assert_eq!(h.s.peer_depth(3), Some(4), "floor of 4");
    h.send(Event::PeerConnected {
        peer: 4,
        have: Bitfield::full(256),
        reqq: 3,
    });
    assert_eq!(h.s.peer_depth(4), Some(3), "never above the peer's reqq");
}

#[test]
fn urgent_blocks_of_one_piece_fan_out_across_peers() {
    let g = Geometry::new(4 * MIB as u32, &[64 * MIB]).unwrap();
    let mut h = Harness::new(g, Config::standard());
    for p in 1..=6 {
        h.peer(p, MIB);
    }
    let acts = h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    let head: BTreeSet<PeerId> = requests(&acts)
        .into_iter()
        .filter(|&(_, b)| b < 256)
        .map(|(p, _)| p)
        .collect();
    assert_eq!(head.len(), 6, "every peer works on the head piece");
    assert_eq!(h.s.class_of(0), Some(Class::Blocking));
    assert_eq!(h.s.class_of(15), Some(Class::Tail));
}

#[test]
fn focused_reader_gets_critical_window_and_older_keep_blocking() {
    let g = Geometry::new(256 * KIB as u32, &[256 * MIB]).unwrap();
    let mut h = Harness::new(g, Config::standard());
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    // 1 MiB blocking = pieces 0..4, critical 4 MiB = pieces 4..20.
    assert_eq!(h.s.class_of(3), Some(Class::Blocking));
    assert_eq!(h.s.class_of(4), Some(Class::Critical));
    assert_eq!(h.s.class_of(19), Some(Class::Critical));
    assert_eq!(h.s.class_of(20), Some(Class::Playback));
    h.send(Event::RangeOpened {
        demand: 2,
        file: 0,
        start: 128 * MIB,
        end_hint: None,
    });
    assert_eq!(
        h.s.class_of(0),
        Some(Class::Blocking),
        "older reader keeps blocking"
    );
    assert_eq!(
        h.s.class_of(4),
        None,
        "older reader loses its critical window"
    );
    assert_eq!(h.s.class_of(512), Some(Class::Blocking));
    assert_eq!(h.s.class_of(520), Some(Class::Critical));
}

#[test]
fn play_position_shrinks_critical_window() {
    let g = Geometry::new(256 * KIB as u32, &[256 * MIB]).unwrap();
    let mut h = Harness::new(g, Config::standard());
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 40 * MIB,
    });
    assert_eq!(h.s.class_of(164), Some(Class::Critical));
    // The player still plays at 34 MiB: 6 MiB are buffered, so nothing past
    // the blocking window is critical.
    h.send(Event::PlayPosition {
        file: 0,
        pos: 34 * MIB,
    });
    assert_eq!(h.s.class_of(160), Some(Class::Blocking));
    assert_eq!(h.s.class_of(164), Some(Class::Playback));
}

#[test]
fn first_read_in_tail_zone_merges_head() {
    let g = Geometry::new(256 * KIB as u32, &[256 * MIB]).unwrap();
    let mut h = Harness::new(g, Config::standard());
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 256 * MIB - 100 * KIB,
        end_hint: None,
    });
    assert_eq!(h.s.class_of(1023), Some(Class::Blocking));
    assert_eq!(h.s.class_of(0), Some(Class::Blocking), "head merged");
    assert_eq!(h.s.class_of(4), Some(Class::Critical));
}

#[test]
fn seek_cancels_stale_requests() {
    let g = Geometry::new(256 * KIB as u32, &[512 * MIB]).unwrap();
    let mut h = Harness::new(g, Config::standard());
    h.peer(1, 4 * MIB);
    h.peer(2, 4 * MIB);
    let acts = h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    assert!(requests(&acts).len() > 100);
    let acts = h.send(Event::RangeOpened {
        demand: 2,
        file: 0,
        start: 300 * MIB,
        end_hint: None,
    });
    let cancels = acts
        .iter()
        .filter(|a| matches!(a, Action::Cancel { .. }))
        .count();
    assert!(cancels > 50, "only {cancels} cancels on seek");
    assert_eq!(h.s.stats().seeks, 1);
    let new_head = requests(&acts)
        .iter()
        .filter(|&&(_, b)| b >= 300 * 64)
        .count();
    assert!(new_head > 0, "new position requested in the same step");
}

#[test]
fn ready_needs_blocking_and_tail_verified() {
    let g = Geometry::new(64 * KIB as u32, &[4 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, MIB);
    h.send(Event::RangeOpened {
        demand: 7,
        file: 0,
        start: 0,
        end_hint: None,
    });
    for b in 0..4 {
        let acts = h.deliver(1, b);
        assert!(!acts.contains(&Action::Ready { demand: 7 }));
    }
    assert!(h.verified.get(0));
    let tail: Vec<BlockIdx> = h.geo.piece_blocks(63).collect();
    let mut ready = false;
    for b in tail {
        ready |= h.deliver(1, b).contains(&Action::Ready { demand: 7 });
    }
    assert!(ready, "ready once the tail piece verified");
}

/// Serves every outstanding request, the lowest block first, until
/// `demand` is ready; the blocks asked for on the way.
fn serve_until_ready(h: &mut Harness, demand: DemandId) -> Option<Vec<BlockIdx>> {
    let mut asked: Vec<BlockIdx> = Vec::new();
    for _ in 0..10_000 {
        asked.extend(h.out.iter().map(|&(b, _)| b));
        let &(block, peer) = h.out.iter().next()?;
        h.tick(1);
        if h.deliver(peer, block).contains(&Action::Ready { demand }) {
            return Some(asked);
        }
    }
    None
}

/// An MP4 whose 5 MiB `moov` comes first (Tenzing's shape): while its
/// index is pending the head and the fixed tail are asked for and nothing
/// is ready; then Ready waits for the whole `moov`, and nothing more of the
/// tail is asked for.
#[test]
fn moov_first_ready_waits_for_the_moov_and_never_the_tail() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, MIB);
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::Pending,
    });
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    let head: Vec<BlockIdx> = h.geo.piece_blocks(0).collect();
    for b in head {
        let acts = h.deliver(1, b);
        assert!(
            !acts.contains(&Action::Ready { demand: 1 }),
            "ready while pending"
        );
    }
    let from = h.log.len();
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::At {
            start: 0,
            end: 5 * MIB,
        },
    });
    serve_until_ready(&mut h, 1).expect("ready");
    let moov = h.geo.pieces_in(0, 5 * MIB);
    assert!(
        moov.clone().all(|p| h.verified.get(p)),
        "Ready before the moov"
    );
    let tail = h.geo.piece_at(8 * MIB - small_cfg().tail_bytes);
    let late: Vec<_> = requests(&h.log[from..])
        .into_iter()
        .filter(|&(_, b)| h.geo.piece_of(b) >= tail)
        .collect();
    assert!(
        late.is_empty(),
        "tail asked once the moov was known: {late:?}"
    );
}

/// An MP4 whose head is on its way (cause 4 of
/// plans/research/start-latency.md): its fixed tail is urgent at once,
/// where a `moov` written after `mdat` sits, but nothing is ready until the
/// head says where the index is; then a `moov` inside that tail is Ready at
/// once.
#[test]
fn a_pending_index_fetches_the_tail_and_waits_for_the_head() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, MIB);
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::Pending,
    });
    open(&mut h, 1, 0, 0, None);
    assert_eq!(h.s.class_of(127), Some(Class::Tail));
    assert!(
        h.out.iter().any(|&(b, _)| h.geo.piece_of(b) == 127),
        "the tail is not asked for"
    );
    for _ in 0..64 {
        let Some(&(block, peer)) = h.out.iter().next() else {
            break;
        };
        let acts = h.deliver(peer, block);
        assert!(
            !acts.contains(&Action::Ready { demand: 1 }),
            "ready while pending"
        );
    }
    assert!(h.verified.get(0) && h.verified.get(127));
    let acts = h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::At {
            start: 8 * MIB - 40 * KIB,
            end: 8 * MIB,
        },
    });
    assert!(acts.contains(&Action::Ready { demand: 1 }));
}

/// A first read in the index zone of an MP4 whose index is pending, with
/// no read at the head (review P2-1): the head's blocking window is asked
/// for, so the head can say where the index is and the read reaches Ready.
#[test]
fn a_pending_mp4_read_in_the_index_zone_asks_for_the_head() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, 4 * MIB);
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::Pending,
    });
    open(&mut h, 1, 0, 8 * MIB - 100 * KIB, None);
    assert_eq!(h.s.class_of(0), Some(Class::Blocking), "no head asked for");
    let head: Vec<BlockIdx> = h.geo.piece_blocks(0).collect();
    for b in head {
        assert!(h.out.contains(&(b, 1)), "head block {b} not asked for");
        h.deliver(1, b);
    }
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::At {
            start: 8 * MIB - 40 * KIB,
            end: 8 * MIB,
        },
    });
    serve_until_ready(&mut h, 1).expect("ready");
}

/// What a start waits on, for the loading screen: the focused read's
/// blocking window and its file's index in whole pieces, the fixed tail
/// while an MP4's index is pending, and the verified part of it.
#[test]
fn start_bytes_count_what_ready_waits_on() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    assert_eq!(h.s.start_bytes(), None, "no read");
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::Pending,
    });
    open(&mut h, 1, 0, 0, None);
    let start = |need, have, index_pending| StartBytes {
        need,
        have,
        index_pending,
    };
    // Piece 0 and the tail piece 127.
    assert_eq!(h.s.start_bytes(), Some(start(128 * KIB, 0, true)));
    h.send(Event::PieceVerified { piece: 0 });
    assert_eq!(h.s.start_bytes(), Some(start(128 * KIB, 64 * KIB, true)));
    // A 200 KiB `moov` first: pieces 0..4, the blocking window inside it.
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::At {
            start: 0,
            end: 200 * KIB,
        },
    });
    assert_eq!(h.s.start_bytes(), Some(start(256 * KIB, 64 * KIB, false)));
    // A probe never takes the focus.
    open(&mut h, 2, 0, 4 * MIB, Some(4 * MIB + 1000));
    assert_eq!(h.s.start_bytes(), Some(start(256 * KIB, 64 * KIB, false)));
    // Once the file has started there is no start to show.
    for piece in 1..4 {
        h.send(Event::PieceVerified { piece });
    }
    assert_eq!(h.s.start_bytes(), None, "Ready");
    // A resume opens its head read as a probe and nothing else: that is
    // the start (review P1-1).
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut resume = Harness::new(g, small_cfg());
    open(&mut resume, 1, 0, 0, Some(64 * KIB));
    resume.send(Event::PieceVerified { piece: 127 });
    assert_eq!(
        resume.s.start_bytes(),
        Some(start(128 * KIB, 64 * KIB, false))
    );
}

/// An MP4 whose 5 MiB `moov` comes after `mdat`: Ready waits for all of it,
/// not a fixed 64 KiB tail.
#[test]
fn moov_last_ready_waits_for_the_whole_moov() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, MIB);
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::At {
            start: 3 * MIB,
            end: 8 * MIB,
        },
    });
    serve_until_ready(&mut h, 1).expect("ready");
    let moov = h.geo.pieces_in(3 * MIB, 8 * MIB);
    assert!(
        moov.clone().all(|p| h.verified.get(p)),
        "Ready before the moov"
    );
}

#[test]
fn late_urgent_block_is_hedged_and_loser_cancelled() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, 2 * KIB);
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    let slow: Vec<BlockIdx> = h.out.iter().filter(|e| e.1 == 1).map(|e| e.0).collect();
    assert!(!slow.is_empty());
    h.peer(2, 2 * MIB);
    let acts = h.tick(1000);
    let hedged: Vec<BlockIdx> = requests(&acts)
        .into_iter()
        .filter(|&(p, b)| p == 2 && slow.contains(&b))
        .map(|(_, b)| b)
        .collect();
    assert!(!hedged.is_empty(), "slow holder's urgent blocks hedged");
    assert!(h.s.stats().hedges > 0);
    let acts = h.deliver(2, hedged[0]);
    assert!(acts.contains(&Action::Cancel {
        peer: 1,
        block: hedged[0]
    }));
}

/// Seen on real swarms: a peer ignores CANCEL and keeps sending blocks we
/// no longer hold with it. That is not service; the urgent block queued
/// behind them must still move to another peer.
#[test]
fn stale_blocks_do_not_keep_an_urgent_holder_alive() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, MIB);
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    let held: Vec<BlockIdx> = h.out.iter().filter(|e| e.1 == 1).map(|e| e.0).collect();
    assert!(!held.is_empty());
    h.peer(2, MIB);
    let stale = h.geo.num_blocks() / 2;
    let mut rescued = false;
    for _ in 0..15 {
        let mut acts = h.tick(200);
        acts.extend(h.send(Event::BlockReceived {
            peer: 1,
            block: stale,
        }));
        let mine: Vec<BlockIdx> = h.out.iter().filter(|e| e.1 == 2).map(|e| e.0).collect();
        for b in mine {
            acts.extend(h.deliver(2, b));
        }
        rescued |= requests(&acts)
            .iter()
            .any(|&(p, b)| p == 2 && held.contains(&b));
    }
    assert!(
        rescued,
        "urgent blocks behind stale traffic moved to peer 2"
    );
}

#[test]
fn silent_peer_is_snubbed_not_dropped() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.peer(1, MIB);
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    assert!(h.s.outstanding(1) > 1);
    for _ in 0..12 {
        h.tick(1000);
    }
    assert_eq!(h.s.stats().snubs, 1);
    assert_eq!(h.s.outstanding(1), 1);
    let b = h.out.iter().next().unwrap().0;
    h.deliver(1, b);
    assert!(h.s.outstanding(1) > 1, "payload lifts the snub");
}

/// The readahead is set per file (seconds of a file that fits, the
/// window's for a windowed one), and the playback window follows the
/// file's bitrate.
#[test]
fn readahead_and_bitrate_are_set_per_file() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB, 8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    // Blocking 0, critical 1..3, playback 3..5, readahead 5..9.
    assert_eq!(h.s.class_of(8), Some(Class::Readahead));
    assert_eq!(h.s.class_of(9), None);
    h.s.set_readahead(0, MIB);
    h.s.set_readahead(1, 64 * KIB);
    h.send(Event::Tick { now_us: 1 });
    assert_eq!(h.s.class_of(20), Some(Class::Readahead));
    assert_eq!(h.s.class_of(21), None);
    // Twice the bitrate: playback 3..7 (8 s of 32 KiB/s at 64 KiB), the
    // same readahead after it.
    h.s.set_bitrate(0, 64 * KIB);
    h.send(Event::Tick { now_us: 2 });
    assert_eq!(h.s.class_of(6), Some(Class::Playback));
    assert_eq!(h.s.class_of(22), Some(Class::Readahead));
    assert_eq!(h.s.class_of(23), None);
}

#[test]
fn readahead_prefers_rare_pieces() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 2,
        min_depth: 1,
        ..small_cfg()
    };
    let mut h = Harness::new(g, cfg);
    let n = h.geo.num_pieces();
    // Peer 1 has everything; choked peers 2 and 3 lack piece 7.
    let mut common = Bitfield::full(n);
    common.set(7, false);
    for p in [2, 3] {
        h.send(Event::PeerConnected {
            peer: p,
            have: common.clone(),
            reqq: 250,
        });
    }
    h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    // Blocking 0, critical 1..3, playback 3..5, readahead 5..9, tail 127.
    assert_eq!(h.s.class_of(5), Some(Class::Readahead));
    assert_eq!(h.s.class_of(7), Some(Class::Readahead));
    for piece in (0..5).chain([127]) {
        h.send(Event::PieceVerified { piece });
    }
    h.send(Event::PeerConnected {
        peer: 1,
        have: Bitfield::full(n),
        reqq: 250,
    });
    h.send(Event::PeerRate {
        peer: 1,
        bytes_per_s: 1,
        rtt_us: 0,
    });
    let acts = h.send(Event::PeerUnchoked { peer: 1 });
    let first = requests(&acts)[0];
    assert_eq!(h.geo.piece_of(first.1), 7, "rarest readahead piece first");
}

/// File order (plans/streaming-window.md 2.2, phase 2 test s): readahead
/// runs by offset, a piece only one connected peer holds first; held by
/// two or by three is the same.
#[test]
fn readahead_runs_in_file_order_a_lone_piece_first() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 16,
        min_depth: 16,
        ..small_cfg()
    };
    let mut h = Harness::new(g, cfg);
    let n = h.geo.num_pieces();
    // Piece 7 only peer 1 holds; piece 8 peers 1 and 3; the rest all three.
    let mut lacks_7_8 = Bitfield::full(n);
    lacks_7_8.set(7, false);
    lacks_7_8.set(8, false);
    let mut lacks_7 = Bitfield::full(n);
    lacks_7.set(7, false);
    for (peer, have) in [(1, Bitfield::full(n)), (2, lacks_7_8), (3, lacks_7)] {
        h.send(Event::PeerConnected {
            peer,
            have,
            reqq: 250,
        });
    }
    for piece in (0..5).chain([127]) {
        h.send(Event::PieceVerified { piece });
    }
    open(&mut h, 1, 0, 0, None);
    let acts = h.send(Event::PeerUnchoked { peer: 1 });
    let mut order: Vec<PieceIdx> = requests(&acts)
        .iter()
        .map(|r| h.geo.piece_of(r.1))
        .collect();
    order.dedup();
    assert_eq!(order, [7, 5, 6, 8], "a lone piece, then file order");
}

fn window(file: usize) -> Window {
    Window {
        file,
        budget: u64::MAX / 2,
        slack: 0,
        behind: 128 * KIB,
        ramp_floor: 128 * KIB,
        open_floor: 256 * KIB,
        open_cap: 256 * KIB,
    }
}

fn readahead_pieces(h: &Harness) -> Vec<PieceIdx> {
    (0..h.geo.num_pieces())
        .filter(|&p| h.s.class_of(p) == Some(Class::Readahead))
        .collect()
}

fn open(h: &mut Harness, demand: DemandId, file: usize, start: u64, end_hint: Option<u64>) {
    h.send(Event::RangeOpened {
        demand,
        file,
        start,
        end_hint,
    });
}

/// A probe (its end within its blocking window: a hash range, a resume's
/// head) never takes the focus: the player read keeps its windows, the
/// probe fetches only its blocking window, and with only probes left the
/// anchor stays warm (plans/streaming-window.md 2.3, phase 2 test e).
#[test]
fn a_probe_never_takes_the_focus() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    // Piece 64: blocking 64, critical 65..67, playback 67..69, readahead 69..73.
    open(&mut h, 1, 0, 4 * MIB, None);
    let ahead = readahead_pieces(&h);
    assert_eq!(ahead, [69, 70, 71, 72]);
    open(&mut h, 2, 0, 0, Some(64 * KIB));
    assert_eq!(h.s.class_of(0), Some(Class::Blocking));
    assert_eq!(
        h.s.class_of(1),
        None,
        "nothing past the probe's blocking window"
    );
    assert_eq!(h.s.class_of(65), Some(Class::Critical));
    assert_eq!(readahead_pieces(&h), ahead);
    h.send(Event::RangeClosed { demand: 1 });
    assert_eq!(
        h.s.class_of(64),
        Some(Class::Playback),
        "the anchor is warm"
    );
    assert_eq!(h.s.class_of(1), None);
}

/// At a resume the anchor sits at the resume point (2.3, 2.8): with only
/// probes open its window is kept and warm and nothing is evictable; a
/// player's header read at the head leaves it kept; once a read opens near
/// it, it is an ordinary focus.
#[test]
fn a_seeded_anchor_keeps_the_resume_window_until_a_read_opens_near_it() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(window(0)));
    h.s.seed_anchor(0, 4 * MIB);
    for piece in 0..128 {
        h.send(Event::PieceVerified { piece });
    }
    open(&mut h, 1, 0, 0, Some(64 * KIB));
    // Pinned head 0 and tail 127; the anchor's [pos - B, playback end).
    let kept = h.s.kept();
    let kept_now: Vec<PieceIdx> = (0..128).filter(|&p| kept.get(p)).collect();
    assert_eq!(kept_now, [0, 62, 63, 64, 65, 66, 67, 68, 127]);
    assert!(h.s.evictable(u64::MAX).is_empty(), "no player read yet");
    // The player's header read at byte 0, past its grace.
    open(&mut h, 2, 0, 0, None);
    h.tick(6_000);
    assert!(
        (62..69).all(|p| h.s.kept().get(p)),
        "still the resume window"
    );
    assert!(!h.s.evictable(u64::MAX).is_empty());
    // A read at the resume point ends the seeded anchor: after it moves on
    // and its grace ends, the old window is no longer kept.
    open(&mut h, 3, 0, 4 * MIB, None);
    h.send(Event::RangeClosed { demand: 2 });
    open(&mut h, 4, 0, 6 * MIB, None);
    h.send(Event::RangeClosed { demand: 3 });
    h.tick(6_000);
    assert!(!(62..69).any(|p| h.s.kept().get(p)));
}

/// The keeper's order (2.4): other files' pieces first, then the windowed
/// file's verified pieces behind the read (farthest back first), then those
/// beyond the kept set (farthest first); never a kept piece.
#[test]
fn evictable_follows_the_keeper_order() {
    // File 0: pieces 0..16; file 1 (windowed): 16..144.
    let g = Geometry::new(64 * KIB as u32, &[MIB, 8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(window(1)));
    let verified: Vec<PieceIdx> = (0..21).chain(78..88).chain(100..103).chain([143]).collect();
    for &piece in &verified {
        h.send(Event::PieceVerified { piece });
    }
    // File 1 byte 4 MiB is piece 80: kept 78..87 (B, blocking, critical,
    // playback and the ramp's floor) plus the pinned 16 and 143.
    open(&mut h, 1, 1, 4 * MIB, None);
    let kept = h.s.kept();
    assert!((78..87).all(|p| kept.get(p)) && !kept.get(77) && !kept.get(87));
    let want: Vec<PieceIdx> = (0..16).chain(17..21).chain([102, 101, 100, 87]).collect();
    assert_eq!(h.s.evictable(u64::MAX), want);
    // Enough for three pieces: the first three.
    assert_eq!(h.s.evictable(3 * 64 * KIB), [0, 1, 2]);
    assert_eq!(h.s.held_bytes(), verified.len() as u64 * 64 * KIB);
}

/// The ramp (2.2, phase 2 test u): after a seek the readahead is the floor,
/// it grows by twice the bytes read up to the file's readahead, a new range
/// within a piece of the read position keeps it, and a jump starts again at
/// the floor.
#[test]
fn the_ramp_grows_with_the_bytes_read() {
    let g = Geometry::new(64 * KIB as u32, &[16 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(window(0)));
    h.s.set_readahead(0, MIB);
    open(&mut h, 1, 0, 0, None);
    assert_eq!(readahead_pieces(&h).len(), 2, "the floor");
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 256 * KIB,
    });
    assert_eq!(readahead_pieces(&h).len(), 8, "twice the bytes read");
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: MIB,
    });
    assert_eq!(readahead_pieces(&h).len(), 16, "the cap");
    h.send(Event::RangeClosed { demand: 1 });
    open(&mut h, 2, 0, MIB + 32 * KIB, None);
    assert_eq!(readahead_pieces(&h).len(), 16, "the run goes on");
    open(&mut h, 3, 0, 8 * MIB, None);
    h.tick(6_000);
    assert_eq!(readahead_pieces(&h).len(), 2, "a seek starts again");
}

/// A seek's grace in a window (2.3, phase 2 test r): for 5 s the new focus
/// has no readahead and the previous one keeps `[pos - B, playback end)`;
/// then the readahead comes back and the old window is free. A file that
/// is not windowed has no grace.
#[test]
fn a_seek_has_a_grace_in_a_window_only() {
    let g = Geometry::new(64 * KIB as u32, &[16 * MIB]).unwrap();
    let mut h = Harness::new(g.clone(), small_cfg());
    h.s.set_window(Some(window(0)));
    open(&mut h, 1, 0, 0, None);
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 2 * MIB,
    });
    open(&mut h, 2, 0, 8 * MIB, None);
    h.send(Event::RangeClosed { demand: 1 });
    assert!(readahead_pieces(&h).is_empty(), "no readahead in the grace");
    // The previous focus at piece 32: 30..37 kept.
    assert!((30..37).all(|p| h.s.kept().get(p)));
    h.tick(4_000);
    assert!(readahead_pieces(&h).is_empty());
    h.tick(1_500);
    assert!(!readahead_pieces(&h).is_empty());
    assert!(!(30..37).any(|p| h.s.kept().get(p)));
    let mut whole = Harness::new(g, small_cfg());
    open(&mut whole, 1, 0, 0, None);
    open(&mut whole, 2, 0, 8 * MIB, None);
    assert_eq!(readahead_pieces(&whole).len(), 4, "the preset at once");
}

/// r, second half: a player that reads in bounded chunks (the loopback
/// gives every range an end; here 4 MiB) on a file that fits gets today's
/// windows, its readahead included, chunk after chunk.
#[test]
fn bounded_chunks_on_a_file_that_fits_get_todays_windows() {
    let g = Geometry::new(64 * KIB as u32, &[16 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    for (demand, start) in [(1, 0), (2, 4 * MIB), (3, 8 * MIB)] {
        open(&mut h, demand, 0, start, Some(start + 4 * MIB));
        assert_eq!(readahead_pieces(&h).len(), 4, "chunk at {start}");
        h.send(Event::RangeClosed { demand });
    }
}

/// With no reader a stopped window keeps no readahead, a paused one does
/// (2.3); a file that is not windowed keeps its anchor warm as before.
#[test]
fn a_paused_window_keeps_its_readahead_and_a_stopped_one_does_not() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(window(0)));
    open(&mut h, 1, 0, 4 * MIB, None);
    h.send(Event::RangeClosed { demand: 1 });
    assert!(readahead_pieces(&h).is_empty());
    assert_eq!(h.s.class_of(64), Some(Class::Playback));
    h.s.set_paused(true);
    h.tick(1);
    assert_eq!(readahead_pieces(&h), [69, 70, 71, 72]);
    assert!(h.s.kept().get(72));
    h.s.set_paused(false);
    h.tick(1);
    assert!(readahead_pieces(&h).is_empty());
    assert!(!h.s.kept().get(72));
}

/// The open-pieces limit (2.2, phase 2 test t): in a window, readahead
/// pieces partly received at once stay within `clamp(2 s × fill, floor,
/// cap)`, here 4 pieces; a verified one makes room for the next. A file
/// that is not windowed has no limit.
#[test]
fn readahead_pieces_open_at_once_stay_within_the_limit() {
    let g = Geometry::new(64 * KIB as u32, &[16 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 256,
        min_depth: 256,
        readahead_bytes: 2 * MIB,
        ..small_cfg()
    };
    let open_ra = |h: &Harness| -> Vec<PieceIdx> {
        let mut v: Vec<PieceIdx> = h
            .out
            .iter()
            .map(|&(b, _)| h.geo.piece_of(b))
            .filter(|&p| h.s.class_of(p) == Some(Class::Readahead))
            .collect();
        v.dedup();
        v
    };
    let mut h = Harness::new(g.clone(), cfg.clone());
    h.s.set_window(Some(window(0)));
    h.s.set_readahead(0, 2 * MIB);
    open(&mut h, 1, 0, 0, None);
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 4 * MIB,
    });
    h.peer(1, 10 * MIB);
    let first = open_ra(&h);
    assert_eq!(first.len(), 4, "{first:?}");
    let blocks: Vec<BlockIdx> = h.geo.piece_blocks(first[0]).collect();
    for b in blocks {
        h.deliver(1, b);
    }
    let next = open_ra(&h);
    assert_eq!(next.len(), 4, "{next:?}");
    assert!(!next.contains(&first[0]));
    // A faster fill opens more, up to the cap: 2 s at 10 MiB/s is over 16
    // pieces' 1 MiB.
    let mut quick = Harness::new(g.clone(), cfg.clone());
    quick.s.set_window(Some(Window {
        open_cap: MIB,
        ..window(0)
    }));
    quick.s.set_readahead(0, 2 * MIB);
    open(&mut quick, 1, 0, 0, None);
    quick.send(Event::RangeAdvanced {
        demand: 1,
        pos: 4 * MIB,
    });
    quick.peer(1, 10 * MIB);
    assert_eq!(open_ra(&quick).len(), 16);
    let mut whole = Harness::new(g, cfg);
    whole.s.set_readahead(0, 2 * MIB);
    open(&mut whole, 1, 0, 4 * MIB, None);
    whole.peer(1, 10 * MIB);
    assert!(open_ra(&whole).len() > 4);
}

/// The window's budget (2.4): no piece outside the urgent windows starts
/// past its cap (the budget less a seek's blocking and critical windows
/// rounded out, plus a piece), the keeper is due past `cap - slack / 2`
/// and evicts down to `cap - slack`, farthest back first, a seek's urgent
/// windows stay within the budget, and a trim keeps the pinned pieces and
/// the region around the read.
#[test]
fn the_budget_bounds_what_starts_and_the_keeper_brings_it_down() {
    const P: u64 = 64 * KIB;
    let g = Geometry::new(P as u32, &[16 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 256,
        min_depth: 256,
        ..small_cfg()
    };
    let mut h = Harness::new(g, cfg);
    h.s.set_window(Some(Window {
        budget: 20 * P,
        slack: 4 * P,
        open_floor: 64 * MIB,
        open_cap: 64 * MIB,
        ..window(0)
    }));
    h.s.set_readahead(0, 4 * MIB);
    for piece in 2..11 {
        h.send(Event::PieceVerified { piece });
    }
    open(&mut h, 1, 0, 0, None);
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 4 * MIB,
    });
    h.peer(1, 10 * MIB);
    // The cap: 20 pieces less 64 KiB blocking and 128 KiB critical (3
    // pieces) and one more. 9 held, the urgent 64..67 and 255, then
    // playback and readahead up to 16 pieces in all.
    assert_eq!(h.s.held_bytes(), 16 * P);
    let started: BTreeSet<PieceIdx> = h.out.iter().map(|&(b, _)| h.geo.piece_of(b)).collect();
    assert_eq!(started.len(), 7, "{started:?}");
    assert!(h.s.keeper_due());
    assert_eq!(h.s.to_evict(), [2, 3, 4, 5]);
    for piece in 2..6 {
        h.send(Event::PieceEvicted { piece });
    }
    // The room fills again with readahead, and the next round is due.
    assert_eq!(h.s.held_bytes(), 16 * P);
    let started: BTreeSet<PieceIdx> = h.out.iter().map(|&(b, _)| h.geo.piece_of(b)).collect();
    assert_eq!(started.len(), 11, "{started:?}");
    assert_eq!(h.s.to_evict(), [6, 7, 8, 9]);
    let trim = h.s.trimmable(2 * P);
    assert_eq!(trim, (6..11).collect::<Vec<_>>());
    // A seek before the keeper's round: its urgent windows start past the
    // cap and stay within the budget.
    open(&mut h, 2, 0, 12 * MIB, None);
    let urgent = h.geo.pieces_in(12 * MIB, 12 * MIB + 192 * KIB);
    assert!(
        urgent
            .clone()
            .all(|p| h.s.class_of(p).is_some_and(Class::is_urgent))
    );
    let started: BTreeSet<PieceIdx> = h.out.iter().map(|&(b, _)| h.geo.piece_of(b)).collect();
    assert!(urgent.clone().all(|p| started.contains(&p)), "{started:?}");
    assert!(h.s.held_bytes() <= 20 * P, "{}", h.s.held_bytes() / P);
}

/// The hard ceiling (2.4, review r3): with nothing evicted (a round that
/// waits on its sync), seek after seek starts only blocking-window pieces
/// past the budget, and those only within one blocking span over it; the
/// critical windows wait for the round.
#[test]
fn urgent_fetches_stop_one_blocking_span_past_the_budget() {
    const P: u64 = 64 * KIB;
    let g = Geometry::new(P as u32, &[16 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 256,
        min_depth: 256,
        ..small_cfg()
    };
    let mut h = Harness::new(g, cfg);
    h.s.set_window(Some(Window {
        budget: 20 * P,
        slack: 4 * P,
        open_floor: 64 * MIB,
        open_cap: 64 * MIB,
        ..window(0)
    }));
    for piece in 2..19 {
        h.send(Event::PieceVerified { piece });
    }
    h.peer(1, 10 * MIB);
    // Three seeks, nothing evicted between them: each wants a blocking
    // piece and two critical ones, and the last blocking piece passes the
    // budget.
    for (demand, at) in [(1, 4 * MIB), (2, 8 * MIB), (3, 12 * MIB)] {
        open(&mut h, demand, 0, at, None);
    }
    // The ceiling: the budget, plus a blocking span (64 KiB rounded out,
    // and a piece) for blocking pieces only.
    let held = h.s.held_bytes() / P;
    assert!(held > 20 && held <= 22, "{held} pieces");
    let started: BTreeSet<PieceIdx> = h.out.iter().map(|&(b, _)| h.geo.piece_of(b)).collect();
    assert!(
        !started.contains(&193),
        "a critical piece past the budget: {started:?}"
    );
    assert!(
        started.contains(&192),
        "the last seek's blocking piece starts within its span: {started:?}"
    );
}

/// The space guard's budget change (2.4, review P1-1) keeps the player's
/// read: the keeper still has pieces to evict, by the smaller budget.
#[test]
fn a_budget_change_keeps_the_player_read() {
    const P: u64 = 64 * KIB;
    let g = Geometry::new(P as u32, &[16 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(Window {
        budget: 64 * P,
        slack: 4 * P,
        ..window(0)
    }));
    for piece in 0..40 {
        h.send(Event::PieceVerified { piece });
    }
    open(&mut h, 1, 0, 2 * MIB, None);
    assert!(h.s.to_evict().is_empty(), "within the budget");
    h.s.set_budget(30 * P);
    h.tick(1);
    assert!(h.s.keeper_due());
    assert!(!h.s.to_evict().is_empty(), "the read is still the player's");
}

/// Two ranges read at once (AVPlayer reads an MP4's index this way): when
/// the newer closes, the older one that reads on past its blocking window
/// takes the focus back (review r2 P2-1), with its windows and `B`, and
/// nothing past the closed range's blocking window stays wanted.
#[test]
fn an_older_range_that_reads_on_takes_the_focus_back() {
    let g = Geometry::new(64 * KIB as u32, &[64 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(window(0)));
    open(&mut h, 1, 0, 40 * MIB, None);
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 40 * MIB + 256 * KIB,
    });
    open(&mut h, 2, 0, 63 * MIB, None);
    h.tick(6_000);
    h.send(Event::RangeClosed { demand: 2 });
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 40 * MIB + 512 * KIB,
    });
    // Piece 648 is 40.5 MiB: its critical window, a readahead past it, and
    // `B` (128 KiB) behind it kept.
    assert_eq!(h.s.class_of(649), Some(Class::Critical));
    assert!(!readahead_pieces(&h).is_empty(), "the readahead is A's");
    assert!(readahead_pieces(&h).iter().all(|&p| p > 649 && p < 700));
    assert!(h.s.kept().get(646), "A's B");
    assert_eq!(h.s.class_of(1009), None, "nothing past N's blocking window");
}

/// The space guard counts what the torrent wrote (2.4, review r3 A R2):
/// verified pieces, and of a piece in flight only its received blocks, as
/// free space has dropped for those only; the kept set counts it whole.
#[test]
fn written_bytes_count_a_piece_in_flight_by_its_received_blocks() {
    let g = Geometry::new(64 * KIB as u32, &[4 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(window(0)));
    h.send(Event::PieceVerified { piece: 5 });
    h.peer(1, MIB);
    open(&mut h, 1, 0, 0, None);
    let block = h.geo.piece_blocks(0).start;
    assert!(h.out.contains(&(block, 1)), "piece 0 in flight");
    h.deliver(1, block);
    assert_eq!(h.s.written_bytes(), 64 * KIB + u64::from(BLOCK));
    assert!(h.s.held_bytes() >= 2 * 64 * KIB);
}

/// A budget the space guard shrank (2.4, review r3 B P2-1) takes its cut
/// first from the room past the kept set (the watched bytes behind `B`),
/// so the readahead stays whole while it fits; past that, off the
/// readahead's far end, byte for byte, so the kept set stays within the
/// keeper's target: the farthest part leaves the kept set first. It comes
/// back with the budget.
#[test]
fn a_shrunk_budget_takes_its_cut_off_the_farthest_readahead() {
    let g = Geometry::new(64 * KIB as u32, &[64 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(Window {
        budget: 64 * MIB,
        ..window(0)
    }));
    h.s.set_readahead(0, 4 * MIB);
    open(&mut h, 1, 0, 0, None);
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 8 * MIB,
    });
    let full = readahead_pieces(&h);
    assert_eq!(full.len(), 64, "4 MiB of readahead");
    // 56 MiB less: the room behind takes it.
    h.s.set_budget(8 * MIB);
    h.tick(1);
    assert_eq!(readahead_pieces(&h), full, "the readahead fits");
    // 60 MiB less: past that room, the farthest readahead leaves.
    h.s.set_budget(4 * MIB);
    h.tick(1);
    let cut = readahead_pieces(&h);
    assert!(cut.len() < 64 && cut == full[..cut.len()], "{cut:?}");
    assert!(!h.s.kept().get(*full.last().unwrap()));
    let kept: u64 = (0..h.geo.num_pieces())
        .filter(|&p| h.s.kept().get(p))
        .count() as u64;
    assert_eq!(
        kept * 64 * KIB,
        4 * MIB - 256 * KIB,
        "the kept set fills the target"
    );
    // Each MiB more comes off the readahead.
    h.s.set_budget(3 * MIB);
    h.tick(1);
    assert_eq!(readahead_pieces(&h), full[..cut.len() - 16], "1 MiB less");
    h.s.set_budget(64 * MIB);
    h.tick(1);
    assert_eq!(readahead_pieces(&h), full, "back with the budget");
}

/// The whole-file rule (2.1, review r3 A R3, R4, R7): a file of a pack
/// writes up to a piece of its neighbours at each end; a folder an earlier
/// launch left with one file counts that file's bytes for a row that names
/// none; and the row of a torrent playing in a window leaves that window
/// out of `C`, as the app does.
#[test]
fn a_rows_whole_file_room_counts_its_ends_its_file_and_not_its_own_window() {
    const MB: u64 = 1_000_000;
    let base = WindowInputs {
        free: RESERVE_BYTES + 100 * MB,
        ..Default::default()
    };
    let single = HeldBytes {
        folder: 30 * MB,
        files: vec![30 * MB],
        idle: true,
        committed: 0,
    };
    let left = WindowInputs {
        reclaimable: 30 * MB,
        ..base
    };
    // 130 MB with 30 of them on disk: fits with the folder's one file.
    assert!(
        left.with_held(Some(&single), None, 4 * MB)
            .fits_whole(130 * MB)
    );
    // A pack: the same file needs two more pieces for its ends.
    let pack = HeldBytes {
        files: vec![30 * MB, 0],
        ..single.clone()
    };
    let packed = left.with_held(Some(&pack), Some(0), 4 * MB);
    assert!(!packed.fits_whole(130 * MB));
    assert!(packed.fits_whole(122 * MB));
    // Its own window's 20 MB still to take is not counted against it.
    let playing = HeldBytes {
        idle: false,
        committed: 20 * MB,
        ..single
    };
    let own = WindowInputs {
        committed: 20 * MB,
        ..base
    };
    assert!(
        own.with_held(Some(&playing), Some(0), 4 * MB)
            .fits_whole(130 * MB)
    );
}

/// A range that reopens just behind its run's start (a demuxer stepping
/// back to a sync point) starts its ramp there (review P2-4: the debug
/// build's overflow check panicked).
#[test]
fn a_reopen_behind_the_run_start_keeps_the_ramp_sound() {
    let g = Geometry::new(64 * KIB as u32, &[16 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(window(0)));
    h.s.set_readahead(0, MIB);
    open(&mut h, 1, 0, 8 * MIB, None);
    h.tick(6_000);
    h.send(Event::RangeAdvanced {
        demand: 1,
        pos: 8 * MIB + 20_000,
    });
    h.send(Event::RangeClosed { demand: 1 });
    open(&mut h, 2, 0, 8 * MIB - 30_000, None);
    assert_eq!(readahead_pieces(&h).len(), 2, "the floor");
}

/// A range left open that nobody reads (an abandoned connection, a parked
/// header read) does not take the focus when the player's read closes,
/// and is not the previous focus of the player's next read (review P2-1):
/// nothing past its blocking window is wanted, and the read that reopens
/// near the player's position keeps its readahead with no grace.
#[test]
fn an_idle_open_range_never_takes_the_focus_back() {
    let g = Geometry::new(64 * KIB as u32, &[64 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_window(Some(window(0)));
    open(&mut h, 1, 0, 0, None);
    open(&mut h, 2, 0, 40 * MIB, None);
    h.tick(6_000);
    h.send(Event::RangeAdvanced {
        demand: 2,
        pos: 40 * MIB + 32 * KIB,
    });
    let ahead = readahead_pieces(&h);
    assert!(!ahead.is_empty());
    h.send(Event::RangeClosed { demand: 2 });
    assert_eq!(h.s.class_of(1), None, "the idle range is not the focus");
    assert_eq!(
        h.s.class_of(640),
        Some(Class::Playback),
        "the anchor is warm"
    );
    open(&mut h, 3, 0, 40 * MIB + 96 * KIB, None);
    assert!(!readahead_pieces(&h).is_empty(), "no grace");
    assert_eq!(h.s.class_of(1), None);
}

// ---- property tests ----

#[derive(Clone, Debug)]
enum Op {
    Connect {
        peer: u8,
        seed: u64,
        full: bool,
        reqq: u8,
    },
    Disconnect(u8),
    Have(u8, u16),
    Choke(u8),
    Unchoke(u8),
    Rate {
        peer: u8,
        kib: u16,
        rtt_ms: u16,
    },
    Open {
        demand: u8,
        file: u8,
        frac: u16,
        bounded: bool,
    },
    Advance {
        demand: u8,
        kib: u16,
    },
    Close(u8),
    Play {
        file: u8,
        frac: u16,
    },
    Deliver {
        k: u16,
        corrupt: bool,
    },
    Reject(u16),
    TimedOut(u16),
    Tick(u16),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..6u8, any::<u64>(), any::<bool>(), 1..12u8)
            .prop_map(|(peer, seed, full, reqq)| Op::Connect { peer, seed, full, reqq }),
        1 => (0..6u8).prop_map(Op::Disconnect),
        2 => (0..6u8, any::<u16>()).prop_map(|(p, x)| Op::Have(p, x)),
        1 => (0..6u8).prop_map(Op::Choke),
        3 => (0..6u8).prop_map(Op::Unchoke),
        2 => (0..6u8, 0..2048u16, 0..400u16)
            .prop_map(|(peer, kib, rtt_ms)| Op::Rate { peer, kib, rtt_ms }),
        2 => (0..4u8, 0..3u8, any::<u16>(), any::<bool>())
            .prop_map(|(demand, file, frac, bounded)| Op::Open { demand, file, frac, bounded }),
        2 => (0..4u8, 0..256u16).prop_map(|(demand, kib)| Op::Advance { demand, kib }),
        1 => (0..4u8).prop_map(Op::Close),
        1 => (0..3u8, any::<u16>()).prop_map(|(file, frac)| Op::Play { file, frac }),
        10 => (any::<u16>(), prop::bool::weighted(0.1))
            .prop_map(|(k, corrupt)| Op::Deliver { k, corrupt }),
        1 => any::<u16>().prop_map(Op::Reject),
        1 => any::<u16>().prop_map(Op::TimedOut),
        4 => (0..3000u16).prop_map(Op::Tick),
    ]
}

fn scenario() -> impl Strategy<Value = (Geometry, Config)> {
    (
        // 40,000 bytes: pieces that end in a short block, as YTS torrents do.
        prop::sample::select(vec![16 * 1024u32, 32 * 1024, 64 * 1024, 40_000]),
        prop::collection::vec(1..300_000u64, 1..=3),
        any::<bool>(),
    )
        .prop_map(|(piece_len, files, tail)| {
            let geo = Geometry::new(piece_len, &files).unwrap();
            let cfg = Config {
                blocking_bytes: 32 * KIB,
                critical_min_bytes: 48 * KIB,
                critical_secs: 1,
                playback_secs: 2,
                readahead_bytes: 96 * KIB,
                tail_bytes: if tail { 32 * KIB } else { 0 },
                default_bitrate: 16 * KIB,
                slow_peer_pct: 20,
                hedge_min_ms: 100,
                hedge_floor_bytes: 32 * KIB,
                min_depth: 2,
                max_depth: 6,
                snub_ms: 3000,
                initial_peer_rate: 32 * KIB,
                ..Config::standard()
            };
            (geo, cfg)
        })
}

fn xorshift(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

fn run(geo: &Geometry, cfg: &Config, ops: &[Op]) -> Harness {
    let mut h = Harness::new(geo.clone(), cfg.clone());
    let n = geo.num_pieces();
    let nfiles = geo.files().len();
    for op in ops {
        match *op {
            Op::Connect {
                peer,
                seed,
                full,
                reqq,
            } => {
                let mut have = Bitfield::new(n);
                let mut x = seed | 1;
                for i in 0..n {
                    x = xorshift(x);
                    have.set(i, full || x & 1 == 1);
                }
                h.send(Event::PeerConnected {
                    peer: peer.into(),
                    have,
                    reqq: reqq.into(),
                });
            }
            Op::Disconnect(p) => {
                h.send(Event::PeerDisconnected { peer: p.into() });
            }
            Op::Have(p, x) => {
                h.send(Event::PeerHave {
                    peer: p.into(),
                    piece: u32::from(x) % n,
                });
            }
            Op::Choke(p) => {
                h.send(Event::PeerChoked { peer: p.into() });
            }
            Op::Unchoke(p) => {
                h.send(Event::PeerUnchoked { peer: p.into() });
            }
            Op::Rate { peer, kib, rtt_ms } => {
                h.send(Event::PeerRate {
                    peer: peer.into(),
                    bytes_per_s: u64::from(kib) * KIB,
                    rtt_us: u64::from(rtt_ms) * 1000,
                });
            }
            Op::Open {
                demand,
                file,
                frac,
                bounded,
            } => {
                let file = usize::from(file) % nfiles;
                let len = geo.files()[file].len;
                let start = len * u64::from(frac) / 65_536;
                let end_hint = bounded.then_some(start + 20_000);
                h.send(Event::RangeOpened {
                    demand: demand.into(),
                    file,
                    start,
                    end_hint,
                });
                // The focused demand gets its blocking and critical windows;
                // a probe (its end within its blocking window) only its
                // blocking one (plans/streaming-window.md 2.3).
                let f = geo.files()[file];
                let b_end = (start + cfg.blocking_bytes)
                    .min(end_hint.unwrap_or(f.len).min(f.len))
                    .max(start + 1)
                    .min(f.len);
                let crit = cfg
                    .critical_min_bytes
                    .max(cfg.critical_secs * cfg.default_bitrate);
                let probe = end_hint.is_some_and(|e| e <= start + cfg.blocking_bytes);
                let end = if probe {
                    b_end
                } else {
                    (b_end + crit).min(f.len)
                };
                for p in geo.pieces_in(f.offset + start, f.offset + end) {
                    let c = h.s.class_of(p);
                    assert!(
                        c.is_some_and(Class::is_urgent),
                        "focused piece {p} is {c:?}"
                    );
                }
            }
            Op::Advance { demand, kib } => {
                if let Some(&(file, start, _)) = h.open.get(&demand.into()) {
                    let pos = (start + u64::from(kib) * KIB).min(geo.files()[file].len);
                    h.send(Event::RangeAdvanced {
                        demand: demand.into(),
                        pos,
                    });
                }
            }
            Op::Close(d) => {
                h.send(Event::RangeClosed { demand: d.into() });
            }
            Op::Play { file, frac } => {
                let file = usize::from(file) % nfiles;
                let pos = geo.files()[file].len * u64::from(frac) / 65_536;
                h.send(Event::PlayPosition { file, pos });
            }
            Op::Deliver { k, corrupt } => {
                let Some(&(block, peer)) = h.out.iter().nth(usize::from(k) % h.out.len().max(1))
                else {
                    continue;
                };
                h.send(Event::BlockReceived { peer, block });
                let piece = geo.piece_of(block);
                if !h.verified.get(piece) && geo.piece_blocks(piece).all(|b| h.received.get(b)) {
                    if corrupt {
                        h.send(Event::PieceFailed { piece });
                    } else {
                        h.send(Event::PieceVerified { piece });
                    }
                }
            }
            Op::Reject(k) | Op::TimedOut(k) => {
                let Some(&(block, peer)) = h.out.iter().nth(usize::from(k) % h.out.len().max(1))
                else {
                    continue;
                };
                let ev = if matches!(op, Op::Reject(_)) {
                    Event::BlockRejected { peer, block }
                } else {
                    Event::BlockTimedOut { peer, block }
                };
                h.send(ev);
            }
            Op::Tick(ms) => {
                h.tick(u64::from(ms));
            }
        }
    }
    h
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Every invariant in `Harness::send` holds over random event streams.
    #[test]
    fn invariants_hold((geo, cfg) in scenario(), ops in prop::collection::vec(op(), 1..300)) {
        run(&geo, &cfg, &ops);
    }

    #[test]
    fn same_events_same_actions((geo, cfg) in scenario(), ops in prop::collection::vec(op(), 1..200)) {
        let a = run(&geo, &cfg, &ops);
        let b = run(&geo, &cfg, &ops);
        prop_assert_eq!(a.log, b.log);
        prop_assert_eq!(a.s.stats(), b.s.stats());
    }
}

/// The random streams above must actually reach hedges, snubs, seeks,
/// cancels, readiness and piece failures, or the properties are vacuous.
#[test]
fn property_streams_cover_every_path() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let ops = prop::collection::vec(op(), 100..300);
    let (mut hedges, mut snubs, mut seeks, mut cancels, mut ready) = (0, 0, 0, 0, 0);
    for _ in 0..300 {
        let (geo, cfg) = scenario().new_tree(&mut runner).unwrap().current();
        let ops = ops.new_tree(&mut runner).unwrap().current();
        let h = run(&geo, &cfg, &ops);
        let s = h.s.stats();
        hedges += s.hedges;
        snubs += s.snubs;
        seeks += s.seeks;
        cancels += s.cancels;
        ready += h
            .log
            .iter()
            .filter(|a| matches!(a, Action::Ready { .. }))
            .count();
    }
    assert!(
        hedges > 0 && snubs > 0 && seeks > 0 && cancels > 0 && ready > 0,
        "hedges {hedges} snubs {snubs} seeks {seeks} cancels {cancels} ready {ready}"
    );
}

// ---- offline downloads (plans/offline-downloads.md 4.6) ----

/// Serves every outstanding request until none is left (or `rounds` pass).
fn serve_all(h: &mut Harness, rounds: usize) {
    for _ in 0..rounds {
        let out: Vec<(BlockIdx, PeerId)> = h.out.iter().copied().collect();
        if out.is_empty() {
            return;
        }
        for (b, p) in out {
            h.deliver(p, b);
        }
    }
}

/// With no reader at all, a download's file comes whole, in piece order.
#[test]
fn bulk_fetches_a_whole_file_in_order_with_no_reader() {
    let g = Geometry::new(64 * KIB as u32, &[2 * MIB, 2 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_bulk(1, true);
    let acts = h.tick(1);
    assert!(requests(&acts).is_empty(), "no peer yet");
    h.peer(1, 4 * MIB);
    let first: Vec<PieceIdx> = h.out.iter().map(|&(b, _)| h.geo.piece_of(b)).collect();
    assert_eq!(
        first.iter().min(),
        Some(&32),
        "the file's first piece first"
    );
    assert_eq!(h.s.class_of(32), Some(Class::Bulk));
    assert_eq!(h.s.class_of(31), None, "the other file is not fetched");
    serve_all(&mut h, 10_000);
    assert_eq!(h.s.file_verified_bytes(1), 2 * MIB);
    assert_eq!(h.s.file_verified_bytes(0), 0);
}

/// A reader comes to a torrent whose peers are full of the download's
/// requests: its first block is asked for in the same call, the queued
/// download blocks go, and while its pieces wait a peer holds one download
/// piece at most.
#[test]
fn a_reader_goes_before_a_download() {
    let g = Geometry::new(64 * KIB as u32, &[4 * MIB, 4 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 8,
        min_depth: 8,
        ..small_cfg()
    };
    let mut h = Harness::new(g, cfg);
    h.s.set_bulk(1, true);
    h.peer(1, 4 * MIB);
    h.peer(2, 4 * MIB);
    assert_eq!(h.peer_out(1), 8, "full of the download");
    let acts = h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    let asked: Vec<PieceIdx> = requests(&acts)
        .iter()
        .map(|r| h.geo.piece_of(r.1))
        .collect();
    assert!(
        asked.contains(&0),
        "the reader's first piece at once: {asked:?}"
    );
    for peer in [1, 2] {
        let mut bulk: Vec<PieceIdx> = h
            .out
            .iter()
            .filter(|&&(_, p)| p == peer)
            .map(|&(b, _)| h.geo.piece_of(b))
            .filter(|&p| h.s.class_of(p) == Some(Class::Bulk))
            .collect();
        bulk.dedup();
        assert!(
            bulk.len() <= 1,
            "peer {peer} holds {bulk:?} of the download"
        );
    }
}

/// Two downloads of one pack are on and off on their own.
#[test]
fn two_files_download_on_their_own() {
    let g = Geometry::new(64 * KIB as u32, &[MIB, MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    h.s.set_bulk(0, true);
    h.s.set_bulk(1, true);
    h.tick(1);
    assert_eq!(h.s.class_of(0), Some(Class::Bulk));
    assert_eq!(h.s.class_of(16), Some(Class::Bulk));
    h.s.set_bulk(0, false);
    h.tick(1);
    assert_eq!(h.s.class_of(0), None);
    assert_eq!(h.s.class_of(16), Some(Class::Bulk));
}

/// A kept file's pieces are not the window's: they leave its held bytes,
/// so a window on the pack's other file keeps its cap, and the keeper and
/// the trim never name one.
#[test]
fn a_kept_file_is_never_evicted_nor_counted_in_a_window() {
    let g = Geometry::new(64 * KIB as u32, &[2 * MIB, 2 * MIB]).unwrap();
    let mut h = Harness::new(g, small_cfg());
    let n = h.geo.num_pieces();
    let mut kept = Bitfield::new(n);
    (0..32).for_each(|p| kept.set(p, true));
    h.s.set_kept(kept);
    for piece in 0..32 {
        h.send(Event::PieceVerified { piece });
    }
    h.s.set_window(Some(Window {
        budget: 512 * KIB,
        ..window(1)
    }));
    open(&mut h, 1, 1, 0, None);
    assert_eq!(h.s.held_bytes(), 0, "the kept file is not held");
    assert_eq!(h.s.written_bytes(), 0);
    assert!(h.s.evictable(u64::MAX).iter().all(|&p| p >= 32));
    assert!(h.s.trimmable(0).iter().all(|&p| p >= 32));
    assert!(h.s.kept().get(0) && h.s.kept().get(31));
}

/// A reader's readahead goes before a download: with every piece the
/// reader needs now verified, the first free slots take its readahead,
/// not the other file's download.
#[test]
fn readahead_goes_before_a_download() {
    let g = Geometry::new(64 * KIB as u32, &[8 * MIB, 8 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 4,
        min_depth: 4,
        ..small_cfg()
    };
    let mut h = Harness::new(g, cfg);
    h.s.set_bulk(1, true);
    for piece in (0..5).chain([127]) {
        h.send(Event::PieceVerified { piece });
    }
    open(&mut h, 1, 0, 0, None);
    h.peer(1, 4 * MIB);
    let first: Vec<PieceIdx> = h.out.iter().map(|&(b, _)| h.geo.piece_of(b)).collect();
    assert!(
        first
            .iter()
            .all(|&p| h.s.class_of(p) == Some(Class::Readahead)),
        "{first:?}"
    );
}

/// A reader whose urgent pieces are all on disk takes the peers' slots from
/// a download at once: the download's queued blocks go when it opens, so
/// its playback pieces never wait behind them.
#[test]
fn a_readers_first_range_cancels_queued_download_blocks() {
    let g = Geometry::new(64 * KIB as u32, &[4 * MIB, 4 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 8,
        min_depth: 8,
        ..small_cfg()
    };
    let mut h = Harness::new(g, cfg);
    for piece in (0..3).chain([63]) {
        h.send(Event::PieceVerified { piece });
    }
    h.s.set_bulk(1, true);
    h.peer(1, 4 * MIB);
    assert_eq!(h.peer_out(1), 8, "full of the download");
    let acts = h.send(Event::RangeOpened {
        demand: 1,
        file: 0,
        start: 0,
        end_hint: None,
    });
    let asked: Vec<PieceIdx> = requests(&acts)
        .iter()
        .map(|r| h.geo.piece_of(r.1))
        .collect();
    assert!(
        asked
            .iter()
            .any(|&p| h.s.class_of(p) == Some(Class::Playback)),
        "no playback piece asked: {asked:?}"
    );
}

/// A player that closes a range just before it opens the next (AVPlayer's
/// small ranges) is the same reader: the download's queued blocks stay, and
/// only a reader after a quiet spell takes their place (code review r1
/// P3-1).
#[test]
fn a_range_reopened_at_once_keeps_the_download_queued() {
    let g = Geometry::new(64 * KIB as u32, &[4 * MIB, 4 * MIB]).unwrap();
    let cfg = Config {
        max_depth: 8,
        min_depth: 8,
        ..small_cfg()
    };
    let mut h = Harness::new(g, cfg);
    for piece in 0..64 {
        h.send(Event::PieceVerified { piece });
    }
    h.s.set_bulk(1, true);
    h.peer(1, 4 * MIB);
    open(&mut h, 1, 0, 0, None);
    h.send(Event::RangeClosed { demand: 1 });
    h.tick(1);
    assert_eq!(h.peer_out(1), 8, "full of the download");
    let cancels = |acts: &[Action]| {
        acts.iter()
            .filter(|a| matches!(a, Action::Cancel { .. }))
            .count()
    };
    let acts = h.send(Event::RangeOpened {
        demand: 2,
        file: 0,
        start: 1 << 20,
        end_hint: None,
    });
    assert_eq!(cancels(&acts), 0, "the same reader, a moment later");
    h.send(Event::RangeClosed { demand: 2 });
    h.tick(6_000);
    let acts = h.send(Event::RangeOpened {
        demand: 3,
        file: 0,
        start: 2 << 20,
        end_hint: None,
    });
    assert!(cancels(&acts) > 0, "a new reader after a quiet spell");
}
