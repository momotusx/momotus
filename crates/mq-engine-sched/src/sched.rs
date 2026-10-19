use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;

use crate::{
    Action, BLOCK, Bitfield, BlockIdx, Class, Config, DemandId, Event, FileIndex, Geometry, PeerId,
    PieceIdx, Stats,
};

const US: u64 = 1_000_000;
/// Floor for rate estimates so ETAs stay finite; also the rate assumed for a
/// peer that owes us data and has sent nothing for over a second.
const MIN_RATE: u64 = 1024;
/// Request latency samples kept for the p90 hedge threshold.
const LATENCY_SAMPLES: usize = 128;

#[derive(Clone, Copy)]
struct Req {
    block: BlockIdx,
    sent: u64,
}

struct Peer {
    have: Bitfield,
    reqq: u32,
    choked: bool,
    rate: u64,
    peak: u64,
    rtt: u64,
    /// Outstanding requests in send order (the remote serves them FIFO).
    queue: VecDeque<Req>,
    queued_bytes: u64,
    /// Requests in `queue` whose block is not urgent, so preemptable.
    normal_reqs: u32,
    busy_since: u64,
    last_payload: u64,
    snubbed: bool,
    /// The queue was empty at some point since the last rate sample.
    idle_in_sample: bool,
}

struct Demand {
    id: DemandId,
    file: usize,
    start: u64,
    end: Option<u64>,
    pos: u64,
    seq: u64,
    ready: bool,
}

/// Window bounds in torrent byte offsets.
struct Windows {
    blocking: (u64, u64),
    critical_end: u64,
    playback_end: u64,
    readahead_end: u64,
}

/// The DWH scheduler for one torrent. See the crate docs.
pub struct Scheduler {
    cfg: Config,
    geo: Geometry,
    now: u64,
    last_periodic: Option<u64>,
    bitrate: Vec<u64>,
    play_pos: Vec<Option<u64>>,
    /// Each file's container index as the adapter knows it (file offsets).
    index: Vec<FileIndex>,
    peers: BTreeMap<PeerId, Peer>,
    avg_rate: u64,
    avail: Vec<u16>,
    verified: Bitfield,
    received: Bitfield,
    /// Peers with an outstanding request per block (at most two).
    holders: BTreeMap<BlockIdx, Vec<PeerId>>,
    /// Blocks per piece that are received or requested.
    busy: Vec<u16>,
    demands: Vec<Demand>,
    next_seq: u64,
    anchor: Option<(usize, u64)>,
    class: Vec<Option<Class>>,
    urgent: Vec<PieceIdx>,
    /// Deadline (microseconds) of each urgent piece.
    deadline: BTreeMap<PieceIdx, u64>,
    normal: Vec<PieceIdx>,
    urgent_cursor: usize,
    normal_cursor: usize,
    /// Focused demand, its file, and its urgent piece span at the last rebuild.
    last_focus: Option<(DemandId, usize, Range<PieceIdx>)>,
    dirty: bool,
    fill_all: bool,
    touched: Vec<PeerId>,
    latencies: VecDeque<u64>,
    payload_log: VecDeque<(u64, u64)>,
    payload_in_window: u64,
    hedge_log: VecDeque<(u64, u64)>,
    hedge_in_window: u64,
    stats: Stats,
}

impl Scheduler {
    pub fn new(geo: Geometry, cfg: Config) -> Self {
        let pieces = geo.num_pieces();
        let files = geo.files().len();
        Self {
            avg_rate: cfg.initial_peer_rate,
            bitrate: vec![cfg.default_bitrate.max(1); files],
            play_pos: vec![None; files],
            index: vec![FileIndex::Tail; files],
            avail: vec![0; pieces as usize],
            verified: Bitfield::new(pieces),
            received: Bitfield::new(geo.num_blocks()),
            busy: vec![0; pieces as usize],
            class: vec![None; pieces as usize],
            cfg,
            geo,
            now: 0,
            last_periodic: None,
            peers: BTreeMap::new(),
            holders: BTreeMap::new(),
            demands: Vec::new(),
            next_seq: 0,
            anchor: None,
            urgent: Vec::new(),
            deadline: BTreeMap::new(),
            normal: Vec::new(),
            urgent_cursor: 0,
            normal_cursor: 0,
            last_focus: None,
            dirty: false,
            fill_all: false,
            touched: Vec::new(),
            latencies: VecDeque::new(),
            payload_log: VecDeque::new(),
            payload_in_window: 0,
            hedge_log: VecDeque::new(),
            hedge_in_window: 0,
            stats: Stats::default(),
        }
    }

    /// Media bitrate of `file` in bytes per second (file size / duration from
    /// the addon or the probe). Windows sized in seconds use it.
    pub fn set_bitrate(&mut self, file: usize, bytes_per_s: u64) {
        if let Some(b) = self.bitrate.get_mut(file) {
            *b = bytes_per_s.max(1);
            self.dirty = true;
        }
    }

    pub fn geometry(&self) -> &Geometry {
        &self.geo
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Current class of a piece, `None` when no window wants it.
    pub fn class_of(&self, piece: PieceIdx) -> Option<Class> {
        self.class.get(piece as usize).copied().flatten()
    }

    /// Current request depth limit for a peer.
    pub fn peer_depth(&self, peer: PeerId) -> Option<usize> {
        self.peers.get(&peer).map(|p| self.depth(p))
    }

    pub fn outstanding(&self, peer: PeerId) -> usize {
        self.peers.get(&peer).map_or(0, |p| p.queue.len())
    }

    /// Apply one event and append the resulting decisions to `out`.
    pub fn handle(&mut self, ev: Event, out: &mut Vec<Action>) {
        match ev {
            Event::Tick { now_us } => {
                self.now = self.now.max(now_us);
                let due = self
                    .last_periodic
                    .is_none_or(|t| self.now >= t + self.cfg.tick_ms * 1000);
                if due {
                    self.periodic(out);
                }
            }
            Event::RangeOpened {
                demand,
                file,
                start,
                end_hint,
            } => {
                let Some(f) = self.geo.files().get(file).copied() else {
                    return;
                };
                let start = start.min(f.len);
                self.demands.retain(|d| d.id != demand);
                self.next_seq += 1;
                self.demands.push(Demand {
                    id: demand,
                    file,
                    start,
                    end: end_hint,
                    pos: start,
                    seq: self.next_seq,
                    ready: false,
                });
                // A new reader means the old play position may be stale.
                self.play_pos[file] = None;
                self.anchor = None;
                self.dirty = true;
                self.check_ready(out);
            }
            Event::RangeAdvanced { demand, pos } => {
                let before = self.focus_key();
                let Some(d) = self.demands.iter_mut().find(|d| d.id == demand) else {
                    return;
                };
                let flen = self.geo.files()[d.file].len;
                let old = d.pos;
                d.pos = pos.min(flen).max(old);
                let off = self.geo.files()[d.file].offset;
                let moved = self.geo.piece_at(off + old) != self.geo.piece_at(off + d.pos);
                if moved || self.focus_key() != before {
                    self.dirty = true;
                }
            }
            Event::RangeClosed { demand } => {
                if let Some(i) = self.demands.iter().position(|d| d.id == demand) {
                    let d = self.demands.remove(i);
                    if self.demands.is_empty() {
                        self.anchor = Some((d.file, d.pos));
                    }
                    self.dirty = true;
                }
            }
            Event::FileIndex { file, index } => {
                if let Some(slot) = self.index.get_mut(file) {
                    *slot = index;
                    self.dirty = true;
                    self.check_ready(out);
                }
            }
            Event::PlayPosition { file, pos } => {
                if file < self.play_pos.len() {
                    let before = self.focus_key();
                    self.play_pos[file] = Some(pos);
                    if self.focus_key() != before {
                        self.dirty = true;
                    }
                }
            }
            Event::PeerConnected { peer, have, reqq } => self.connect(peer, have, reqq),
            Event::PeerDisconnected { peer } => self.disconnect(peer),
            Event::PeerHave { peer, piece } => {
                if let Some(p) = self.peers.get_mut(&peer)
                    && piece < self.geo.num_pieces()
                    && !p.have.get(piece)
                {
                    p.have.set(piece, true);
                    let a = &mut self.avail[piece as usize];
                    *a = a.saturating_add(1);
                    self.touched.push(peer);
                }
            }
            Event::PeerRate {
                peer,
                bytes_per_s,
                rtt_us,
            } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    // A sample taken while we left the peer idle measures us,
                    // not the peer: it may raise the estimate, never lower it.
                    p.rate = if p.idle_in_sample {
                        p.rate.max(bytes_per_s)
                    } else {
                        bytes_per_s
                    };
                    p.idle_in_sample = p.queue.is_empty();
                    p.peak = p.peak.max(bytes_per_s);
                    p.rtt = rtt_us;
                    self.update_avg_rate();
                    self.touched.push(peer);
                }
            }
            Event::PeerChoked { peer } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.choked = true;
                    self.drop_queue(peer);
                }
            }
            Event::PeerUnchoked { peer } => {
                if let Some(p) = self.peers.get_mut(&peer) {
                    p.choked = false;
                    self.touched.push(peer);
                }
            }
            Event::BlockReceived { peer, block } => self.on_block(peer, block, out),
            Event::BlockRejected { peer, block } | Event::BlockTimedOut { peer, block } => {
                self.release(peer, block);
            }
            Event::PieceVerified { piece } => {
                if piece < self.geo.num_pieces() {
                    self.verified.set(piece, true);
                    for b in self.geo.piece_blocks(piece) {
                        self.mark_received(b);
                        for o in self.holders.get(&b).cloned().unwrap_or_default() {
                            self.cancel(o, b, out);
                        }
                    }
                    self.check_ready(out);
                }
            }
            Event::PieceFailed { piece } => {
                if piece < self.geo.num_pieces() {
                    self.verified.set(piece, false);
                    for b in self.geo.piece_blocks(piece) {
                        if self.received.get(b) {
                            self.received.set(b, false);
                            if !self.holders.contains_key(&b) {
                                self.busy[piece as usize] -= 1;
                            }
                        }
                    }
                    self.urgent_cursor = 0;
                    self.normal_cursor = 0;
                    self.fill_all = true;
                }
            }
        }
        self.pump(out);
    }

    // ---- events ----

    fn connect(&mut self, peer: PeerId, have: Bitfield, reqq: u32) {
        self.disconnect(peer);
        let n = self.geo.num_pieces();
        let have = if have.len() == n {
            have
        } else {
            let mut h = Bitfield::new(n);
            (0..n.min(have.len())).for_each(|i| h.set(i, have.get(i)));
            h
        };
        for i in 0..n {
            if have.get(i) {
                let a = &mut self.avail[i as usize];
                *a = a.saturating_add(1);
            }
        }
        self.peers.insert(
            peer,
            Peer {
                have,
                reqq: reqq.max(1),
                choked: true,
                rate: 0,
                peak: 0,
                rtt: 0,
                queue: VecDeque::new(),
                queued_bytes: 0,
                normal_reqs: 0,
                busy_since: self.now,
                last_payload: self.now,
                snubbed: false,
                idle_in_sample: true,
            },
        );
    }

    fn disconnect(&mut self, peer: PeerId) {
        if !self.peers.contains_key(&peer) {
            return;
        }
        self.drop_queue(peer);
        if let Some(p) = self.peers.remove(&peer) {
            for i in 0..self.geo.num_pieces() {
                if p.have.get(i) {
                    self.avail[i as usize] = self.avail[i as usize].saturating_sub(1);
                }
            }
        }
        self.update_avg_rate();
        self.fill_all = true;
    }

    fn drop_queue(&mut self, peer: PeerId) {
        let blocks: Vec<BlockIdx> = self.peers[&peer].queue.iter().map(|r| r.block).collect();
        for b in blocks {
            self.release(peer, b);
        }
    }

    fn on_block(&mut self, peer: PeerId, b: BlockIdx, out: &mut Vec<Action>) {
        if b >= self.geo.num_blocks() {
            return;
        }
        let len = self.geo.block_len(b) as u64;
        self.stats.payload_bytes += len;
        self.payload_log.push_back((self.now, len));
        self.payload_in_window += len;
        // Only a block we are waiting on from this peer shows it serves us.
        // Real peers often ignore CANCEL for requests already queued and keep
        // sending stale blocks for seconds; counting those kept a peer that
        // held an urgent block behind them from looking stalled.
        let mut prev_payload = self.now;
        if let Some(p) = self.peers.get_mut(&peer)
            && p.queue.iter().any(|r| r.block == b)
        {
            prev_payload = p.last_payload;
            p.last_payload = self.now;
            p.snubbed = false;
        }
        if self.received.get(b) {
            self.stats.dup_bytes += len;
        } else {
            self.mark_received(b);
        }
        if let Some(sent) = self.release(peer, b) {
            // Service time, not time spent queued behind our own requests.
            if self.latencies.len() == LATENCY_SAMPLES {
                self.latencies.pop_front();
            }
            self.latencies.push_back(self.now - sent.max(prev_payload));
        }
        // First arrival wins.
        for o in self.holders.get(&b).cloned().unwrap_or_default() {
            self.cancel(o, b, out);
        }
        self.touched.push(peer);
    }

    fn periodic(&mut self, out: &mut Vec<Action>) {
        self.last_periodic = Some(self.now);
        let horizon = self.now.saturating_sub(self.cfg.hedge_window_ms * 1000);
        while self.payload_log.front().is_some_and(|e| e.0 < horizon) {
            self.payload_in_window -= self.payload_log.pop_front().map_or(0, |e| e.1);
        }
        while self.hedge_log.front().is_some_and(|e| e.0 < horizon) {
            self.hedge_in_window -= self.hedge_log.pop_front().map_or(0, |e| e.1);
        }
        let snub_us = self.cfg.snub_ms * 1000;
        let snubbed: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|(_, p)| {
                !p.snubbed
                    && !p.queue.is_empty()
                    && self.now - p.last_payload.max(p.busy_since) >= snub_us
            })
            .map(|(&id, _)| id)
            .collect();
        for id in snubbed {
            self.stats.snubs += 1;
            let p = self.peers.get_mut(&id).expect("snubbed peer exists");
            p.snubbed = true;
            // Keep the oldest request: it may still arrive.
            let rest: Vec<BlockIdx> = p.queue.iter().skip(1).map(|r| r.block).collect();
            for b in rest {
                self.cancel(id, b, out);
            }
        }
        self.hedge(out);
        // Hedging keeps a possibly late request alive; once the duplicate
        // budget is spent, stalled holders hand urgent blocks back instead.
        if self.hedge_in_window + u64::from(BLOCK) > self.hedge_allowance() {
            self.release_stalled(out);
        }
        self.fill_all = true;
    }

    // ---- windows ----

    fn windows(&self, file: usize, pos: u64, end: Option<u64>, play: Option<u64>) -> Windows {
        let f = self.geo.files()[file];
        let rate = self.bitrate[file];
        let pos = pos.min(f.len);
        let limit = end.unwrap_or(f.len).min(f.len);
        let b_end = (pos + self.cfg.blocking_bytes)
            .min(limit)
            .max(pos + 1)
            .min(f.len);
        let crit = self
            .cfg
            .critical_min_bytes
            .max(self.cfg.critical_secs * rate);
        // Bytes the player already holds past its play position are not
        // critical any more.
        let lag = play.filter(|&p| p <= pos).map_or(0, |p| pos - p);
        let c_end = (b_end + crit.saturating_sub(lag)).min(f.len);
        let p_end = (b_end + crit + self.cfg.playback_secs * rate).min(f.len);
        let r_end = (p_end + self.cfg.readahead_bytes).min(f.len);
        Windows {
            blocking: (f.offset + pos, f.offset + b_end),
            critical_end: f.offset + c_end,
            playback_end: f.offset + p_end,
            readahead_end: f.offset + r_end,
        }
    }

    /// The pieces of `file`'s container index; none while it is pending.
    fn index_pieces(&self, file: usize) -> Range<PieceIdx> {
        let f = self.geo.files()[file];
        match self.index[file] {
            FileIndex::At { start, end } => {
                let end = end.min(f.len);
                self.geo
                    .pieces_in(f.offset + start.min(end), f.offset + end)
            }
            FileIndex::Tail if self.cfg.tail_bytes > 0 => {
                let tail = f.offset + f.len.saturating_sub(self.cfg.tail_bytes);
                self.geo.pieces_in(tail, f.offset + f.len)
            }
            FileIndex::Tail | FileIndex::Pending => 0..0,
        }
    }

    fn focus(&self) -> Option<&Demand> {
        self.demands.iter().max_by_key(|d| d.seq)
    }

    /// Urgent piece span of the focused demand: what a rebuild depends on.
    fn focus_key(&self) -> Option<(DemandId, Range<PieceIdx>)> {
        let d = self.focus()?;
        let w = self.windows(d.file, d.pos, d.end, self.play_pos[d.file]);
        let span = self
            .geo
            .pieces_in(w.blocking.0, w.critical_end.max(w.blocking.1));
        Some((d.id, span))
    }

    fn rebuild(&mut self, out: &mut Vec<Action>) {
        self.dirty = false;
        for &p in self.urgent.iter().chain(&self.normal) {
            self.class[p as usize] = None;
        }
        let focus_seq = self.focus().map(|d| d.seq);
        let mut spans: Vec<(Range<PieceIdx>, Class)> = Vec::new();
        let mut focus_blocking: Vec<Range<PieceIdx>> = Vec::new();
        // Critical spans with the torrent offset playing now and the bitrate.
        let mut critical: Vec<(Range<PieceIdx>, u64, u64)> = Vec::new();
        for d in &self.demands {
            let f = self.geo.files()[d.file];
            let play = self.play_pos[d.file];
            let w = self.windows(d.file, d.pos, d.end, play);
            let blocking = self.geo.pieces_in(w.blocking.0, w.blocking.1);
            spans.push((blocking.clone(), Class::Blocking));
            spans.push((self.index_pieces(d.file), Class::Tail));
            if Some(d.seq) != focus_seq {
                continue; // older readers keep only their blocking window
            }
            focus_blocking.push(blocking);
            let g = &self.geo;
            let rate = self.bitrate[d.file];
            let now_at = f.offset + play.filter(|&p| p <= d.pos).unwrap_or(d.pos);
            critical.push((g.pieces_in(w.blocking.1, w.critical_end), now_at, rate));
            spans.push((g.pieces_in(w.critical_end, w.playback_end), Class::Playback));
            spans.push((
                g.pieces_in(w.playback_end, w.readahead_end),
                Class::Readahead,
            ));
            // A first read in the container index zone: the player comes back
            // to the head next, so fetch it now (head and tail merged).
            let zone = self.cfg.tail_bytes.max(f.len / 64);
            if d.start > 0 && d.start >= f.len.saturating_sub(zone) {
                let h = self.windows(d.file, 0, None, None);
                let head = g.pieces_in(h.blocking.0, h.blocking.1);
                focus_blocking.push(head.clone());
                spans.push((head, Class::Blocking));
                critical.push((g.pieces_in(h.blocking.1, h.critical_end), f.offset, rate));
            }
        }
        spans.extend(critical.iter().map(|c| (c.0.clone(), Class::Critical)));
        if self.demands.is_empty()
            && let Some((file, pos)) = self.anchor
        {
            // No reader left: keep the last position's windows warm, non-urgent.
            let w = self.windows(file, pos, None, None);
            let g = &self.geo;
            spans.push((g.pieces_in(w.blocking.0, w.playback_end), Class::Playback));
            spans.push((
                g.pieces_in(w.playback_end, w.readahead_end),
                Class::Readahead,
            ));
        }

        let mut marked = Vec::new();
        for (range, c) in spans {
            for p in range {
                let slot = &mut self.class[p as usize];
                match slot {
                    None => {
                        *slot = Some(c);
                        marked.push(p);
                    }
                    Some(old) if c < *old => *slot = Some(c),
                    _ => {}
                }
            }
        }
        let class = &self.class;
        let (mut urgent, rest): (Vec<_>, Vec<_>) = marked
            .into_iter()
            .partition(|&p| class[p as usize].is_some_and(Class::is_urgent));
        let (mut playback, mut readahead): (Vec<_>, Vec<_>) = rest
            .into_iter()
            .partition(|&p| class[p as usize] == Some(Class::Playback));
        // Deadline order: the focused reader's blocking pieces, then the tail,
        // then other readers' blocking, then critical by offset (one focused
        // reader, so offset order is deadline order).
        urgent.sort_unstable_by_key(|&p| {
            let theirs = !focus_blocking.iter().any(|r| r.contains(&p));
            (class[p as usize], theirs, p)
        });
        playback.sort_unstable();
        // Readahead: rarest first, then offset (swarm friendly).
        readahead.sort_unstable_by_key(|&p| (self.avail[p as usize], p));
        playback.extend(readahead);
        // Deadlines: blocking and tail are due now; a critical piece is due
        // when playback reaches it, minus the safety margin.
        let safety = self.cfg.safety_ms * 1000;
        self.deadline = urgent
            .iter()
            .map(|&p| {
                let start = u64::from(p) * u64::from(self.geo.piece_len());
                let due = critical
                    .iter()
                    .filter(|_| class[p as usize] == Some(Class::Critical))
                    .filter(|c| c.0.contains(&p))
                    .map(|&(_, at, rate)| {
                        (self.now + start.saturating_sub(at) * US / rate).saturating_sub(safety)
                    })
                    .min()
                    .unwrap_or(0);
                (p, due)
            })
            .collect();
        self.urgent = urgent;
        self.normal = playback;
        self.urgent_cursor = 0;
        self.normal_cursor = 0;

        let geo = &self.geo;
        for p in self.peers.values_mut() {
            p.normal_reqs = p
                .queue
                .iter()
                .filter(|r| !class[geo.piece_of(r.block) as usize].is_some_and(Class::is_urgent))
                .count() as u32;
        }

        // Seek: the focused reader moved outside the previous urgent span.
        let focus = self.focus().map(|d| {
            let span = self.focus_key().map(|k| k.1).unwrap_or(0..0);
            (d.id, d.file, span)
        });
        let seek = match (&self.last_focus, &focus) {
            (Some((old_id, old_file, old_span)), Some((id, file, span))) => {
                id != old_id && (file != old_file || !old_span.contains(&span.start))
            }
            _ => false,
        };
        self.last_focus = focus;
        if seek {
            self.stats.seeks += 1;
            // Free the per-peer FIFOs for the new position: drop requests
            // that are now only readahead or not wanted at all.
            let stale: Vec<(PeerId, BlockIdx)> = self
                .peers
                .iter()
                .flat_map(|(&id, p)| p.queue.iter().map(move |r| (id, r.block)))
                .filter(|&(_, b)| {
                    self.class[self.geo.piece_of(b) as usize].is_none_or(|c| c >= Class::Readahead)
                })
                .collect();
            for (id, b) in stale {
                self.cancel(id, b, out);
            }
        }
        // Duplicates are only for urgent blocks.
        let dups: Vec<(PeerId, BlockIdx)> = self
            .holders
            .iter()
            .filter(|(b, h)| h.len() > 1 && !self.is_urgent(**b))
            .map(|(&b, h)| (h[1], b))
            .collect();
        for (id, b) in dups {
            self.cancel(id, b, out);
        }
        self.fill_all = true;
    }

    fn check_ready(&mut self, out: &mut Vec<Action>) {
        for i in 0..self.demands.len() {
            let d = &self.demands[i];
            if d.ready {
                continue;
            }
            let w = self.windows(d.file, d.start, d.end, None);
            let ready = self.index[d.file] != FileIndex::Pending
                && self
                    .geo
                    .pieces_in(w.blocking.0, w.blocking.1)
                    .chain(self.index_pieces(d.file))
                    .all(|p| self.verified.get(p));
            if ready {
                self.demands[i].ready = true;
                out.push(Action::Ready {
                    demand: self.demands[i].id,
                });
            }
        }
    }

    // ---- peers ----

    fn update_avg_rate(&mut self) {
        let (sum, n) = self
            .peers
            .values()
            .filter(|p| p.rate > 0)
            .fold((0u64, 0u64), |(s, n), p| (s + p.rate, n + 1));
        self.avg_rate = sum.checked_div(n).unwrap_or(self.cfg.initial_peer_rate);
    }

    /// libtorrent's estimate: the current sample, else the peak, else the
    /// swarm average. A peer that owes us data and sent nothing for over a
    /// second plus its RTT counts as stalled.
    fn rate_of(&self, p: &Peer) -> u64 {
        let waited = self.now.saturating_sub(p.last_payload.max(p.busy_since));
        let stalled = !p.queue.is_empty() && waited > US + p.rtt;
        let r = if stalled {
            MIN_RATE
        } else if p.rate > 0 {
            p.rate
        } else if p.peak > 0 {
            p.peak
        } else {
            self.avg_rate
        };
        r.max(MIN_RATE)
    }

    /// `2 + ceil(rate * queue_ms / BLOCK)` clamped to `[min_depth,
    /// min(reqq, max_depth)]` (bandwidth-delay sizing); 1 while snubbed.
    fn depth(&self, p: &Peer) -> usize {
        if p.snubbed {
            return 1;
        }
        let cap = p.reqq.min(self.cfg.max_depth).max(1) as u64;
        let want = 2 + (self.rate_of(p) * self.cfg.queue_ms).div_ceil(1000 * BLOCK as u64);
        want.clamp((self.cfg.min_depth as u64).min(cap), cap) as usize
    }

    fn queue_time(&self, p: &Peer) -> u64 {
        p.queued_bytes * US / self.rate_of(p)
    }

    /// When a new request of `len` bytes to `p` should arrive.
    fn eta(&self, p: &Peer, len: u64) -> u64 {
        self.queue_time(p).max(p.rtt) + len * US / self.rate_of(p)
    }

    /// Unchoked, unsnubbed peers minus the slowest `slow_peer_pct`.
    fn eligible(&self) -> Vec<PeerId> {
        let mut v: Vec<(u64, PeerId)> = self
            .peers
            .iter()
            .filter(|(_, p)| !p.choked && !p.snubbed)
            .map(|(&id, p)| (self.rate_of(p), id))
            .collect();
        v.sort_unstable();
        let skip = v.len() * self.cfg.slow_peer_pct as usize / 100;
        v.into_iter().skip(skip).map(|(_, id)| id).collect()
    }

    /// Lowest-ETA candidate that has the block's piece, does not already
    /// hold the block, and has a free slot or a non-urgent request to give
    /// up. Drops candidates that can take nothing more. Returns the peer and
    /// its queue time.
    fn best_peer(&self, cands: &mut Vec<PeerId>, b: BlockIdx) -> Option<(PeerId, u64)> {
        cands.retain(|id| {
            let p = &self.peers[id];
            let (len, depth) = (p.queue.len(), self.depth(p));
            len < depth || (len == depth && p.normal_reqs > 0)
        });
        let piece = self.geo.piece_of(b);
        let len = self.geo.block_len(b) as u64;
        let holders = self.holders.get(&b);
        cands
            .iter()
            .filter(|&id| {
                self.peers[id].have.get(piece) && !holders.is_some_and(|h| h.contains(id))
            })
            .map(|&id| (self.eta(&self.peers[&id], len), id))
            .min()
            .map(|(_, id)| (id, self.queue_time(&self.peers[&id])))
    }

    fn is_urgent(&self, b: BlockIdx) -> bool {
        self.class[self.geo.piece_of(b) as usize].is_some_and(Class::is_urgent)
    }

    fn hedge_allowance(&self) -> u64 {
        (self.payload_in_window * self.cfg.hedge_budget_pct / 100).max(self.cfg.hedge_floor_bytes)
    }

    fn latency_p90(&self) -> u64 {
        let mut v: Vec<u64> = self.latencies.iter().copied().collect();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        v[(v.len() * 9 / 10).min(v.len() - 1)]
    }

    // ---- request bookkeeping ----

    fn mark_received(&mut self, b: BlockIdx) {
        if self.received.get(b) {
            return;
        }
        if !self.holders.contains_key(&b) {
            self.busy[self.geo.piece_of(b) as usize] += 1;
        }
        self.received.set(b, true);
    }

    fn request(&mut self, peer: PeerId, b: BlockIdx, out: &mut Vec<Action>) {
        let urgent = self.is_urgent(b);
        let len = self.geo.block_len(b) as u64;
        let p = self.peers.get_mut(&peer).expect("request to a known peer");
        if p.queue.is_empty() {
            p.busy_since = self.now;
        }
        p.queue.push_back(Req {
            block: b,
            sent: self.now,
        });
        p.queued_bytes += len;
        p.normal_reqs += u32::from(!urgent);
        let h = self.holders.entry(b).or_default();
        if h.is_empty() && !self.received.get(b) {
            self.busy[self.geo.piece_of(b) as usize] += 1;
        }
        h.push(peer);
        self.stats.requests += 1;
        out.push(Action::Request { peer, block: b });
    }

    /// Request `b` from `peer`; if its queue is full, urgent work takes the
    /// slot of the newest non-urgent request.
    fn place(&mut self, peer: PeerId, b: BlockIdx, out: &mut Vec<Action>) {
        let p = &self.peers[&peer];
        if p.queue.len() >= self.depth(p) {
            let victim = p
                .queue
                .iter()
                .rev()
                .find(|r| !self.is_urgent(r.block))
                .map(|r| r.block);
            if let Some(v) = victim {
                self.cancel(peer, v, out);
            }
        }
        self.request(peer, b, out);
    }

    /// Forget an outstanding request (answered, rejected, choked away or
    /// cancelled). Returns its send time.
    fn release(&mut self, peer: PeerId, b: BlockIdx) -> Option<u64> {
        let urgent = self.is_urgent(b);
        let len = self.geo.block_len(b) as u64;
        let p = self.peers.get_mut(&peer)?;
        let i = p.queue.iter().position(|r| r.block == b)?;
        let r = p.queue.remove(i)?;
        p.queued_bytes -= len;
        p.idle_in_sample |= p.queue.is_empty();
        if !urgent {
            p.normal_reqs = p.normal_reqs.saturating_sub(1);
        }
        if let Some(h) = self.holders.get_mut(&b) {
            h.retain(|&x| x != peer);
            if h.is_empty() {
                self.holders.remove(&b);
                if !self.received.get(b) {
                    self.busy[self.geo.piece_of(b) as usize] -= 1;
                    if urgent {
                        self.urgent_cursor = 0;
                    } else {
                        self.normal_cursor = 0;
                    }
                }
            }
        }
        self.touched.push(peer);
        Some(r.sent)
    }

    fn cancel(&mut self, peer: PeerId, b: BlockIdx, out: &mut Vec<Action>) {
        if self.release(peer, b).is_some() {
            self.stats.cancels += 1;
            out.push(Action::Cancel { peer, block: b });
        }
    }

    // ---- scheduling passes ----

    fn pump(&mut self, out: &mut Vec<Action>) {
        if self.dirty {
            self.rebuild(out);
        }
        let fill = if self.fill_all {
            self.touched.clear();
            self.peers.keys().copied().collect()
        } else {
            let mut t = std::mem::take(&mut self.touched);
            t.sort_unstable();
            t.dedup();
            t
        };
        self.fill_all = false;
        if fill.is_empty() && self.urgent_cursor >= self.urgent.len() {
            return;
        }
        let cands = self.eligible();
        self.assign_urgent(cands.clone(), out);
        for id in fill {
            self.fill_peer(id, &cands, out);
        }
    }

    /// Block-level fan-out (libtorrent's time-critical rule): each free
    /// urgent block, in deadline order, goes to the eligible peer with the
    /// lowest ETA; stop once that peer's queue exceeds `max_queue_ms`.
    fn assign_urgent(&mut self, mut cands: Vec<PeerId>, out: &mut Vec<Action>) {
        let max_queue = self.cfg.max_queue_ms * 1000;
        let mut advancing = true;
        let mut i = self.urgent_cursor;
        while i < self.urgent.len() && !cands.is_empty() {
            let piece = self.urgent[i];
            i += 1;
            let blocks = self.geo.piece_blocks(piece);
            if self.verified.get(piece) || self.busy[piece as usize] as usize >= blocks.len() {
                if advancing {
                    self.urgent_cursor = i;
                }
                continue;
            }
            advancing = false;
            for b in blocks {
                if self.received.get(b) || self.holders.contains_key(&b) {
                    continue;
                }
                let Some((peer, queue)) = self.best_peer(&mut cands, b) else {
                    break; // nobody left who has this piece
                };
                if queue > max_queue {
                    return;
                }
                self.place(peer, b, out);
            }
        }
    }

    /// Peer-centric fill of free slots: playback by offset, then readahead
    /// rarest first. Peers kept out of the urgent pass (the slowest ones)
    /// still take urgent pieces nobody else has, so nothing starves.
    fn fill_peer(&mut self, id: PeerId, cands: &[PeerId], out: &mut Vec<Action>) {
        let Some(p) = self.peers.get(&id) else {
            return;
        };
        if p.choked {
            return;
        }
        let mut room = self.depth(p).saturating_sub(p.queue.len());
        if room == 0 {
            return;
        }
        if !cands.contains(&id) {
            for i in 0..self.urgent.len() {
                let piece = self.urgent[i];
                if room == 0 {
                    return;
                }
                let others = cands.iter().any(|c| self.peers[c].have.get(piece));
                if !others && self.peers[&id].have.get(piece) {
                    room = self.take_blocks(id, piece, room, out);
                }
            }
        }
        let mut advancing = true;
        let mut i = self.normal_cursor;
        while i < self.normal.len() && room > 0 {
            let piece = self.normal[i];
            i += 1;
            let n = self.geo.piece_blocks(piece).len();
            if self.verified.get(piece) || self.busy[piece as usize] as usize >= n {
                if advancing {
                    self.normal_cursor = i;
                }
                continue;
            }
            advancing = false;
            if self.peers[&id].have.get(piece) {
                room = self.take_blocks(id, piece, room, out);
            }
        }
    }

    fn take_blocks(
        &mut self,
        id: PeerId,
        piece: PieceIdx,
        mut room: usize,
        out: &mut Vec<Action>,
    ) -> usize {
        for b in self.geo.piece_blocks(piece) {
            if room == 0 {
                break;
            }
            if !self.received.get(b) && !self.holders.contains_key(&b) {
                self.request(id, b, out);
                room -= 1;
            }
        }
        room
    }

    /// Stall budget (used once the hedge budget is spent): a peer that sent
    /// nothing for over a second plus its RTT while we wait on it gives its
    /// urgent blocks back to the urgent pass (cancel and reassign, no
    /// duplicate), as long as another eligible peer has the piece.
    fn release_stalled(&mut self, out: &mut Vec<Action>) {
        let cands = self.eligible();
        let stuck: Vec<(PeerId, BlockIdx)> = self
            .peers
            .iter()
            .filter(|(_, p)| {
                !p.queue.is_empty()
                    && self.now.saturating_sub(p.last_payload.max(p.busy_since)) > US + p.rtt
            })
            .flat_map(|(&id, p)| p.queue.iter().map(move |r| (id, r.block)))
            .filter(|&(id, b)| {
                let piece = self.geo.piece_of(b);
                self.is_urgent(b)
                    && cands
                        .iter()
                        .any(|c| *c != id && self.peers[c].have.get(piece))
            })
            .collect();
        for (id, b) in stuck {
            self.cancel(id, b, out);
        }
    }

    /// Tail at Scale hedge: an urgent block with one requester that will miss
    /// its deadline, and whose ETA exceeds `max(hedge_min_ms, p90 block
    /// service time)`, is also requested from the best other peer if that
    /// peer would beat it, within the duplicate byte budget.
    fn hedge(&mut self, out: &mut Vec<Action>) {
        let threshold = self.latency_p90().max(self.cfg.hedge_min_ms * 1000);
        let mut late = Vec::new();
        for p in self.peers.values() {
            let rate = self.rate_of(p);
            let mut ahead = 0u64;
            for r in &p.queue {
                ahead += self.geo.block_len(r.block) as u64;
                let piece = self.geo.piece_of(r.block);
                let Some(c) = self.class[piece as usize] else {
                    continue;
                };
                if !c.is_urgent() || self.holders.get(&r.block).map_or(0, Vec::len) != 1 {
                    continue;
                }
                let eta = (r.sent + p.rtt).saturating_sub(self.now) + ahead * US / rate;
                let due = self.deadline.get(&piece).copied().unwrap_or(0);
                if eta > threshold && self.now + eta > due {
                    late.push((c, r.block, eta));
                }
            }
        }
        if late.is_empty() {
            return;
        }
        late.sort_unstable();
        let mut cands = self.eligible();
        for (_, b, eta) in late {
            let len = self.geo.block_len(b) as u64;
            if self.hedge_in_window + len > self.hedge_allowance() {
                break;
            }
            let Some((peer, _)) = self.best_peer(&mut cands, b) else {
                continue;
            };
            if self.eta(&self.peers[&peer], len) >= eta {
                continue;
            }
            self.place(peer, b, out);
            self.stats.hedges += 1;
            self.stats.hedge_bytes += len;
            self.hedge_log.push_back((self.now, len));
            self.hedge_in_window += len;
        }
    }
}
