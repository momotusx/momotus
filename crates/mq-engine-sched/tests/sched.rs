//! Unit tests and property tests for the DWH scheduler.
//!
//! `Harness` plays the adapter: it mirrors what was requested, received and
//! verified, and checks every action the scheduler emits against the
//! invariants.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use mq_engine_sched::{
    Action, BLOCK, Bitfield, BlockIdx, Class, Config, DemandId, Event, FileIndex, Geometry, PeerId,
    PieceIdx, Scheduler,
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
/// index is pending only the head is asked for and nothing is ready; then
/// Ready waits for the whole `moov`, and the file's tail is never asked for.
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
    let mut asked: Vec<BlockIdx> = h.out.iter().map(|&(b, _)| b).collect();
    for b in head {
        let acts = h.deliver(1, b);
        assert!(
            !acts.contains(&Action::Ready { demand: 1 }),
            "ready while pending"
        );
    }
    h.send(Event::FileIndex {
        file: 0,
        index: FileIndex::At {
            start: 0,
            end: 5 * MIB,
        },
    });
    asked.extend(serve_until_ready(&mut h, 1).expect("ready"));
    let moov = h.geo.pieces_in(0, 5 * MIB);
    assert!(
        moov.clone().all(|p| h.verified.get(p)),
        "Ready before the moov"
    );
    let tail = h.geo.piece_at(8 * MIB - small_cfg().tail_bytes);
    let late: Vec<_> = asked
        .iter()
        .filter(|&&b| h.geo.piece_of(b) >= tail)
        .collect();
    assert!(late.is_empty(), "tail asked before Ready: {late:?}");
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
                // The focused demand gets its blocking and critical windows.
                let f = geo.files()[file];
                let b_end = (start + cfg.blocking_bytes)
                    .min(end_hint.unwrap_or(f.len).min(f.len))
                    .max(start + 1)
                    .min(f.len);
                let crit = cfg
                    .critical_min_bytes
                    .max(cfg.critical_secs * cfg.default_bitrate);
                let end = (b_end + crit).min(f.len);
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
