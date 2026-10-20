//! Baseline schedulers for the simulator: behavioral models of stock
//! librqbit 9 (research/10 section 3 and appendix A) and Stremio's
//! torrent-stream engine (research/04 section 3). They consume the same
//! events and emit the same actions as DWH.

use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;

use mq_engine_sched::{Action, Bitfield, BlockIdx, DemandId, Event, Geometry, PeerId, PieceIdx};

use crate::swarm::Sched;

const US: u64 = 1_000_000;
const MIB: u64 = 1 << 20;
/// Pieces scanned past a cursor before giving up (keeps the sim fast).
const SCAN: u32 = 4096;

fn file_pieces(geo: &Geometry, file: usize) -> Range<PieceIdx> {
    let f = geo.files()[file];
    geo.pieces_in(f.offset, f.offset + f.len)
}

/// Stock librqbit 9: a whole piece per peer requested from block 0, a FIFO
/// window of `min(128, reqq)`, steal from a 10x then 3x slower holder only
/// once the stealer has an average piece time, 32 MiB lookahead per stream
/// interleaved across streams, first and last piece queued first, then the
/// rest of the file in order.
pub struct RqbitLike {
    geo: Geometry,
    file: usize,
    now: u64,
    last_tick: u64,
    peers: BTreeMap<PeerId, RPeer>,
    inflight: BTreeMap<PieceIdx, (PeerId, u64)>,
    verified: Bitfield,
    received: Bitfield,
    streams: Vec<(DemandId, u64)>,
    natural: Vec<PieceIdx>,
    cursor: usize,
}

struct RPeer {
    have: Bitfield,
    choked: bool,
    window: usize,
    queue: Vec<BlockIdx>,
    /// Reserved pieces with the next block to request.
    owned: Vec<(PieceIdx, BlockIdx)>,
    avg: Option<u64>,
}

impl RqbitLike {
    pub fn new(geo: Geometry, file: usize) -> Self {
        let r = file_pieces(&geo, file);
        let mut natural = vec![r.start];
        if r.end - 1 > r.start {
            natural.push(r.end - 1);
        }
        natural.extend(r.start + 1..r.end - 1);
        Self {
            verified: Bitfield::new(geo.num_pieces()),
            received: Bitfield::new(geo.num_blocks()),
            geo,
            file,
            now: 0,
            last_tick: 0,
            peers: BTreeMap::new(),
            inflight: BTreeMap::new(),
            streams: Vec::new(),
            natural,
            cursor: 0,
        }
    }

    fn free(&self, piece: PieceIdx) -> bool {
        !self.verified.get(piece) && !self.inflight.contains_key(&piece)
    }

    fn reserve(&mut self, peer: PeerId, piece: PieceIdx) {
        self.inflight.insert(piece, (peer, self.now));
        let start = self.geo.piece_blocks(piece).start;
        self.peers
            .get_mut(&peer)
            .expect("peer")
            .owned
            .push((piece, start));
    }

    fn try_steal(&mut self, peer: PeerId, factor: u64, out: &mut Vec<Action>) -> bool {
        // No average piece time yet: nothing can be stolen (the cold-start stall).
        let Some(avg) = self.peers[&peer].avg else {
            return false;
        };
        let have = &self.peers[&peer].have;
        let victim = self
            .inflight
            .iter()
            .filter(|&(&p, &(o, _))| o != peer && have.get(p))
            .map(|(&p, &(o, t))| (self.now - t, p, o))
            .filter(|&(e, _, _)| e >= avg * factor)
            .max();
        let Some((_, piece, old)) = victim else {
            return false;
        };
        let blocks = self.geo.piece_blocks(piece);
        let o = self.peers.get_mut(&old).expect("owner");
        o.owned.retain(|&(p, _)| p != piece);
        let cancelled: Vec<BlockIdx> = o
            .queue
            .iter()
            .copied()
            .filter(|b| blocks.contains(b))
            .collect();
        o.queue.retain(|b| !blocks.contains(b));
        for block in cancelled {
            out.push(Action::Cancel { peer: old, block });
        }
        self.inflight.remove(&piece);
        self.reserve(peer, piece);
        true
    }

    fn acquire(&mut self, peer: PeerId, out: &mut Vec<Action>) -> bool {
        if self.try_steal(peer, 10, out) {
            return true;
        }
        let look = (32 * MIB / u64::from(self.geo.piece_len())).max(1) as u32;
        let off = self.geo.files()[self.file].offset;
        let last = file_pieces(&self.geo, self.file).end - 1;
        for k in 0..look {
            for i in 0..self.streams.len() {
                let piece = self.geo.piece_at(off + self.streams[i].1) + k;
                if piece <= last && self.free(piece) && self.peers[&peer].have.get(piece) {
                    self.reserve(peer, piece);
                    return true;
                }
            }
        }
        while self.cursor < self.natural.len() && self.verified.get(self.natural[self.cursor]) {
            self.cursor += 1;
        }
        let end = (self.cursor + SCAN as usize).min(self.natural.len());
        for i in self.cursor..end {
            let piece = self.natural[i];
            if self.free(piece) && self.peers[&peer].have.get(piece) {
                self.reserve(peer, piece);
                return true;
            }
        }
        self.try_steal(peer, 3, out)
    }

    fn fill(&mut self, peer: PeerId, out: &mut Vec<Action>) {
        if self.peers.get(&peer).is_none_or(|p| p.choked) {
            return;
        }
        loop {
            let p = self.peers.get_mut(&peer).expect("peer");
            if p.queue.len() >= p.window {
                return;
            }
            let mut next = None;
            for (piece, cur) in p.owned.iter_mut() {
                let end = self.geo.piece_blocks(*piece).end;
                while *cur < end && (self.received.get(*cur) || p.queue.contains(cur)) {
                    *cur += 1;
                }
                if *cur < end {
                    next = Some(*cur);
                    *cur += 1;
                    break;
                }
            }
            match next {
                Some(block) => {
                    p.queue.push(block);
                    out.push(Action::Request { peer, block });
                }
                None if self.acquire(peer, out) => {}
                None => return,
            }
        }
    }

    fn release(&mut self, peer: PeerId) {
        let Some(p) = self.peers.get_mut(&peer) else {
            return;
        };
        for (piece, _) in p.owned.drain(..) {
            if self.inflight.get(&piece).is_some_and(|o| o.0 == peer) {
                self.inflight.remove(&piece);
            }
        }
        p.queue.clear();
    }

    fn fill_all(&mut self, out: &mut Vec<Action>) {
        for id in self.peers.keys().copied().collect::<Vec<_>>() {
            self.fill(id, out);
        }
    }
}

impl Sched for RqbitLike {
    fn handle(&mut self, ev: Event, out: &mut Vec<Action>) {
        match ev {
            Event::Tick { now_us } => {
                self.now = self.now.max(now_us);
                if self.now >= self.last_tick + 100_000 {
                    self.last_tick = self.now;
                    self.fill_all(out);
                }
            }
            Event::RangeOpened {
                demand,
                file,
                start,
                ..
            } if file == self.file => {
                self.streams.push((demand, start));
                self.fill_all(out);
            }
            Event::RangeAdvanced { demand, pos } => {
                if let Some(s) = self.streams.iter_mut().find(|s| s.0 == demand) {
                    s.1 = pos;
                }
            }
            Event::RangeClosed { demand } => self.streams.retain(|s| s.0 != demand),
            Event::PeerConnected { peer, have, reqq } => {
                self.release(peer);
                self.peers.insert(
                    peer,
                    RPeer {
                        have,
                        choked: true,
                        window: 128.min(reqq as usize),
                        queue: Vec::new(),
                        owned: Vec::new(),
                        avg: None,
                    },
                );
            }
            Event::PeerDisconnected { peer } => {
                self.release(peer);
                self.peers.remove(&peer);
            }
            Event::PeerHave { peer, piece } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.have.set(piece, true);
                    self.fill(peer, out);
                }
            }
            Event::PeerChoked { peer } => {
                self.release(peer);
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.choked = true;
                }
            }
            Event::PeerUnchoked { peer } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.choked = false;
                    self.fill(peer, out);
                }
            }
            Event::BlockReceived { peer, block } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.queue.retain(|&b| b != block);
                }
                self.received.set(block, true);
                self.fill(peer, out);
            }
            Event::BlockRejected { peer, block } | Event::BlockTimedOut { peer, block } => {
                let piece = self.geo.piece_of(block);
                let start = self.geo.piece_blocks(piece).start;
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.queue.retain(|&b| b != block);
                    p.owned
                        .iter_mut()
                        .filter(|o| o.0 == piece)
                        .for_each(|o| o.1 = start);
                }
            }
            Event::PieceVerified { piece } => {
                self.verified.set(piece, true);
                if let Some((owner, started)) = self.inflight.remove(&piece)
                    && let Some(p) = self.peers.get_mut(&owner)
                {
                    let e = self.now - started;
                    p.avg = Some(p.avg.map_or(e, |a| (3 * a + e) / 4));
                    p.owned.retain(|o| o.0 != piece);
                    self.fill(owner, out);
                }
            }
            Event::PieceFailed { piece } | Event::PieceEvicted { piece } => {
                for b in self.geo.piece_blocks(piece) {
                    self.received.set(b, false);
                }
                self.verified.set(piece, false);
            }
            _ => {}
        }
    }
}

/// Stremio's torrent-stream engine: sequential block reservations from each
/// reader's position (newest first, then the whole file in the background,
/// cached mode), per-wire depth `round(45 r^4 + 5)` from the unchoked count,
/// wires slower than 48 KiB/s kept off critical pieces, hotswap of a critical
/// block from a slow wire to a 2x faster one, and a wire that answers nothing
/// for 4 s is destroyed and usable again 5 s later. No tail prefetch, no
/// cancels on seek.
pub struct StremioLike {
    geo: Geometry,
    file: usize,
    now: u64,
    last_tick: u64,
    peers: BTreeMap<PeerId, SPeer>,
    reserved: BTreeMap<BlockIdx, PeerId>,
    busy: Vec<u16>,
    received: Bitfield,
    verified: Bitfield,
    streams: Vec<(DemandId, u64)>,
}

struct SPeer {
    have: Bitfield,
    choked: bool,
    reqq: usize,
    queue: VecDeque<BlockIdx>,
    rate: u64,
    last_answer: u64,
    dead_until: u64,
}

const SLOW: u64 = 48 * 1024;
const HOTSWAP_MIN: u64 = 16 * 1024;
const WIRE_TIMEOUT: u64 = 4 * US;
const RECONNECT: u64 = 5 * US;

impl StremioLike {
    pub fn new(geo: Geometry, file: usize) -> Self {
        Self {
            busy: vec![0; geo.num_pieces() as usize],
            received: Bitfield::new(geo.num_blocks()),
            verified: Bitfield::new(geo.num_pieces()),
            geo,
            file,
            now: 0,
            last_tick: 0,
            peers: BTreeMap::new(),
            reserved: BTreeMap::new(),
            streams: Vec::new(),
        }
    }

    fn alive(&self, p: &SPeer) -> bool {
        !p.choked && self.now >= p.dead_until
    }

    fn depth(&self) -> usize {
        let unchoked = self.peers.values().filter(|p| self.alive(p)).count() as f64;
        let r = 1.0 - ((unchoked - 1.0) / 29.0).clamp(0.0, 1.0);
        (45.0 * r.powi(4) + 5.0).round() as usize
    }

    /// `min(1 MiB / piece, 4)` pieces at the newest reader (512 KiB virtual
    /// pieces make that 1 MiB of bytes).
    fn critical(&self) -> Range<PieceIdx> {
        let off = self.geo.files()[self.file].offset;
        self.streams
            .last()
            .map_or(0..0, |s| self.geo.pieces_in(off + s.1, off + s.1 + MIB))
    }

    fn next_block(&self, peer: PeerId) -> Option<BlockIdx> {
        let p = &self.peers[&peer];
        let slow = p.rate > 0 && p.rate < SLOW;
        let crit = self.critical();
        let off = self.geo.files()[self.file].offset;
        let last = file_pieces(&self.geo, self.file).end - 1;
        let starts = self.streams.iter().rev().map(|s| s.1).chain([0]);
        for pos in starts {
            let first = self.geo.piece_at(off + pos);
            for piece in first..=last.min(first + SCAN) {
                let blocks = self.geo.piece_blocks(piece);
                if self.verified.get(piece)
                    || !p.have.get(piece)
                    || self.busy[piece as usize] as usize >= blocks.len()
                    || (slow && crit.contains(&piece))
                {
                    continue;
                }
                if let Some(b) = blocks
                    .into_iter()
                    .find(|&b| !self.received.get(b) && !self.reserved.contains_key(&b))
                {
                    return Some(b);
                }
            }
        }
        None
    }

    fn reserve(&mut self, peer: PeerId, b: BlockIdx, out: &mut Vec<Action>) {
        if !self.received.get(b) && !self.reserved.contains_key(&b) {
            self.busy[self.geo.piece_of(b) as usize] += 1;
        }
        self.reserved.insert(b, peer);
        let p = self.peers.get_mut(&peer).expect("peer");
        if p.queue.is_empty() {
            p.last_answer = self.now;
        }
        p.queue.push_back(b);
        out.push(Action::Request { peer, block: b });
    }

    fn unreserve(&mut self, peer: PeerId, b: BlockIdx) {
        if self.reserved.get(&b) == Some(&peer) {
            self.reserved.remove(&b);
            if !self.received.get(b) {
                self.busy[self.geo.piece_of(b) as usize] -= 1;
            }
        }
    }

    fn drop_queue(&mut self, peer: PeerId, cancel: bool, out: &mut Vec<Action>) {
        let blocks: Vec<BlockIdx> = self.peers[&peer].queue.iter().copied().collect();
        self.peers.get_mut(&peer).expect("peer").queue.clear();
        for block in blocks {
            self.unreserve(peer, block);
            if cancel {
                out.push(Action::Cancel { peer, block });
            }
        }
    }

    fn fill(&mut self, peer: PeerId, out: &mut Vec<Action>) {
        let depth = self.depth();
        loop {
            let Some(p) = self.peers.get(&peer) else {
                return;
            };
            if !self.alive(p) || p.queue.len() >= depth.min(p.reqq) {
                return;
            }
            let Some(b) = self.next_block(peer) else {
                return;
            };
            self.reserve(peer, b, out);
        }
    }

    fn tick(&mut self, out: &mut Vec<Action>) {
        // A wire that answered nothing for 4 s is destroyed.
        let dead: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|(_, p)| {
                self.alive(p) && !p.queue.is_empty() && self.now - p.last_answer > WIRE_TIMEOUT
            })
            .map(|(&id, _)| id)
            .collect();
        for id in dead {
            self.drop_queue(id, true, out);
            self.peers.get_mut(&id).expect("peer").dead_until = self.now + RECONNECT;
        }
        // Hotswap critical blocks off slow wires.
        for piece in self.critical() {
            for b in self.geo.piece_blocks(piece) {
                let Some(&holder) = self.reserved.get(&b) else {
                    continue;
                };
                let hr = self.peers.get(&holder).map_or(0, |p| p.rate);
                if hr == 0 || hr >= SLOW {
                    continue;
                }
                let depth = self.depth();
                let better = self
                    .peers
                    .iter()
                    .filter(|&(&id, p)| {
                        id != holder
                            && self.alive(p)
                            && p.rate > HOTSWAP_MIN
                            && p.rate > 2 * hr
                            && p.have.get(piece)
                            && p.queue.len() < depth.min(p.reqq)
                    })
                    .max_by_key(|&(&id, p)| (p.rate, id))
                    .map(|(&id, _)| id);
                if let Some(to) = better {
                    if let Some(p) = self.peers.get_mut(&holder) {
                        p.queue.retain(|&x| x != b);
                    }
                    self.unreserve(holder, b);
                    out.push(Action::Cancel {
                        peer: holder,
                        block: b,
                    });
                    self.reserve(to, b, out);
                }
            }
        }
        for id in self.peers.keys().copied().collect::<Vec<_>>() {
            self.fill(id, out);
        }
    }
}

impl Sched for StremioLike {
    fn handle(&mut self, ev: Event, out: &mut Vec<Action>) {
        match ev {
            Event::Tick { now_us } => {
                self.now = self.now.max(now_us);
                if self.now >= self.last_tick + 100_000 {
                    self.last_tick = self.now;
                    self.tick(out);
                }
            }
            Event::RangeOpened {
                demand,
                file,
                start,
                ..
            } if file == self.file => {
                self.streams.push((demand, start));
                self.tick(out);
            }
            Event::RangeAdvanced { demand, pos } => {
                if let Some(s) = self.streams.iter_mut().find(|s| s.0 == demand) {
                    s.1 = pos;
                }
            }
            Event::RangeClosed { demand } => self.streams.retain(|s| s.0 != demand),
            Event::PeerConnected { peer, have, reqq } => {
                if self.peers.contains_key(&peer) {
                    self.drop_queue(peer, false, out);
                }
                self.peers.insert(
                    peer,
                    SPeer {
                        have,
                        choked: true,
                        reqq: reqq as usize,
                        queue: VecDeque::new(),
                        rate: 0,
                        last_answer: self.now,
                        dead_until: 0,
                    },
                );
            }
            Event::PeerDisconnected { peer } => {
                if self.peers.contains_key(&peer) {
                    self.drop_queue(peer, false, out);
                    self.peers.remove(&peer);
                }
            }
            Event::PeerHave { peer, piece } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.have.set(piece, true);
                    self.fill(peer, out);
                }
            }
            Event::PeerRate {
                peer, bytes_per_s, ..
            } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.rate = bytes_per_s;
                }
            }
            Event::PeerChoked { peer } => {
                if self.peers.contains_key(&peer) {
                    self.drop_queue(peer, false, out);
                    self.peers.get_mut(&peer).expect("peer").choked = true;
                }
            }
            Event::PeerUnchoked { peer } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.choked = false;
                    self.fill(peer, out);
                }
            }
            Event::BlockReceived { peer, block } => {
                if !self.received.get(block) {
                    if !self.reserved.contains_key(&block) {
                        self.busy[self.geo.piece_of(block) as usize] += 1;
                    }
                    self.received.set(block, true);
                }
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.queue.retain(|&b| b != block);
                    p.last_answer = self.now;
                }
                if self.reserved.get(&block) == Some(&peer) {
                    self.reserved.remove(&block);
                }
                self.fill(peer, out);
            }
            Event::BlockRejected { peer, block } | Event::BlockTimedOut { peer, block } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.queue.retain(|&b| b != block);
                }
                self.unreserve(peer, block);
            }
            Event::PieceVerified { piece } => self.verified.set(piece, true),
            Event::PieceFailed { piece } | Event::PieceEvicted { piece } => {
                self.verified.set(piece, false);
                for b in self.geo.piece_blocks(piece) {
                    self.received.set(b, false);
                }
                let n = self
                    .geo
                    .piece_blocks(piece)
                    .filter(|b| self.reserved.contains_key(b))
                    .count();
                self.busy[piece as usize] = n as u16;
            }
            _ => {}
        }
    }
}
