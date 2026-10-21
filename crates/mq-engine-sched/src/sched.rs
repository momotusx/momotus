use std::collections::{BTreeMap, VecDeque};
use std::ops::Range;

use crate::{
    Action, BLOCK, Bitfield, BlockIdx, Class, Config, DemandId, Event, FileIndex, Geometry, PeerId,
    PieceIdx, StartBytes, Stats, Window,
};

const US: u64 = 1_000_000;
/// Floor for rate estimates so ETAs stay finite; also the rate assumed for a
/// peer that owes us data and has sent nothing for over a second.
const MIN_RATE: u64 = 1024;
/// Request latency samples kept for the p90 hedge threshold.
const LATENCY_SAMPLES: usize = 128;
/// A seek's grace in a window (plans/streaming-window.md 2.3).
const GRACE_US: u64 = 5 * US;

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
    /// Where the run of reads this one continues began (the ramp).
    run_start: u64,
    /// Its end lies within its blocking window (a hash range, a resume's
    /// head): it never takes the focus or moves the anchor (2.3).
    probe: bool,
    /// Where it was when a newer focus closed past it: once it reads a
    /// blocking window on from there, it is the player's read again.
    parked: Option<u64>,
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
    /// Readahead past each file's playback window, in bytes.
    readahead: Vec<u64>,
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
    /// The `seq` of the focused read whose close set the anchor: a range
    /// opened before it does not take the focus back (2.3).
    anchor_seq: u64,
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
    window: Option<Window>,
    /// A seeded resume anchor: kept until a player read opens near it.
    resume: Option<(usize, u64)>,
    /// A player read opened since the window began; until then nothing is
    /// evictable.
    player_read: bool,
    /// A seek's grace: until this time the previous focus keeps
    /// `[pos − B, playback end)` and the new focus has no readahead.
    grace: Option<(u64, usize, u64)>,
    /// The focused read's file, position and run start: a new range within
    /// a piece of the position continues its run.
    last_run: Option<(usize, u64, u64)>,
    /// The player is paused: the anchor keeps its readahead.
    paused: bool,
    /// Readahead pieces started under the open-pieces limit.
    ra_open: Vec<PieceIdx>,
    /// `held_bytes` at the start of a fill, kept up as pieces start.
    held: u64,
    /// The window's budget when it was set: a smaller one (the space
    /// guard's) takes its cut off the readahead, farthest part first.
    full_budget: u64,
    /// Per file: a range of it was Ready once, so its start is over.
    started: Vec<bool>,
}

impl Scheduler {
    pub fn new(geo: Geometry, cfg: Config) -> Self {
        let pieces = geo.num_pieces();
        let files = geo.files().len();
        Self {
            avg_rate: cfg.initial_peer_rate,
            bitrate: vec![cfg.default_bitrate.max(1); files],
            readahead: vec![cfg.readahead_bytes; files],
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
            anchor_seq: 0,
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
            window: None,
            resume: None,
            player_read: false,
            grace: None,
            last_run: None,
            paused: false,
            ra_open: Vec::new(),
            held: 0,
            full_budget: 0,
            started: vec![false; files],
        }
    }

    /// Windows a file (plans/streaming-window.md 2.2), or ends windowing.
    pub fn set_window(&mut self, window: Option<Window>) {
        self.window = window.filter(|w| w.file < self.geo.files().len());
        self.full_budget = self.window.map_or(0, |w| w.budget);
        self.player_read = false;
        self.grace = None;
        self.dirty = true;
    }

    /// The space guard's budget (2.4): the window goes on with its player
    /// read and its grace, only `W` changes.
    pub fn set_budget(&mut self, budget: u64) {
        if let Some(w) = self.window.as_mut() {
            w.budget = budget;
            self.dirty = true;
        }
    }

    /// The playback resumes at file byte `pos`: the anchor sits there, and
    /// its window stays kept until a player read opens near it (2.3, 2.8).
    pub fn seed_anchor(&mut self, file: usize, pos: u64) {
        if file < self.geo.files().len() {
            self.anchor = Some((file, pos));
            self.resume = Some((file, pos));
            self.dirty = true;
        }
    }

    /// With no reader, a paused player's anchor keeps its readahead (2.3).
    pub fn set_paused(&mut self, paused: bool) {
        if self.paused != paused {
            self.paused = paused;
            self.dirty = true;
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

    /// Readahead of `file` past its playback window, in bytes (the
    /// config's `readahead_bytes` until set): seconds of the file for one
    /// that fits, the window's for a windowed one.
    pub fn set_readahead(&mut self, file: usize, bytes: u64) {
        if let Some(r) = self.readahead.get_mut(file) {
            *r = bytes;
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
                if self.grace.is_some_and(|g| self.now >= g.0) {
                    self.grace = None;
                    self.dirty = true;
                }
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
                let probe =
                    end_hint.is_some_and(|e| e <= start.saturating_add(self.cfg.blocking_bytes));
                let piece = u64::from(self.geo.piece_len());
                let near = |(f2, p): (usize, u64)| f2 == file && start.abs_diff(p) <= piece;
                let run_start = match self.last_run {
                    Some((f2, p, from)) if near((f2, p)) => from.min(start),
                    _ => start,
                };
                if !probe {
                    self.player_read = true;
                    // A seek in a window: the previous focus (or the anchor)
                    // keeps its window for a moment (2.3).
                    let prev = self.focus().map(|d| (d.file, d.pos)).or(self.anchor);
                    if self.window.is_some()
                        && let Some(prev) = prev.filter(|&p| !near(p))
                    {
                        self.grace = Some((self.now + GRACE_US, prev.0, prev.1));
                    }
                    if let Some((f2, p)) = self.resume {
                        let behind = self.window.map_or(0, |w| w.behind);
                        if f2 == file && start + behind >= p && start <= p + self.readahead[file] {
                            self.resume = None;
                        }
                    }
                    // A new reader means the old play position may be stale.
                    self.play_pos[file] = None;
                    self.anchor = None;
                }
                self.next_seq += 1;
                self.demands.push(Demand {
                    id: demand,
                    file,
                    start,
                    end: end_hint,
                    pos: start,
                    seq: self.next_seq,
                    ready: false,
                    run_start,
                    probe,
                    parked: None,
                });
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
                // Parked by a newer focus's close (an MP4's index read in
                // parallel), it reads on: it is the focus again.
                if d.parked
                    .is_some_and(|p| d.pos >= p + self.cfg.blocking_bytes)
                {
                    self.next_seq += 1;
                    (d.seq, d.parked) = (self.next_seq, None);
                    self.anchor = None;
                    self.dirty = true;
                }
                let run = (d.file, d.pos, d.run_start);
                if self.focus().is_some_and(|f| f.id == demand) {
                    self.last_run = Some(run);
                }
                if moved || self.focus_key() != before {
                    self.dirty = true;
                }
            }
            Event::RangeClosed { demand } => {
                if let Some(i) = self.demands.iter().position(|d| d.id == demand) {
                    let focused = self.focus().is_some_and(|f| f.id == demand);
                    let d = self.demands.remove(i);
                    if focused {
                        // Its position is the anchor until a read opens: an
                        // older range left open (parked, abandoned) does not
                        // take the focus in the gap.
                        self.last_run = Some((d.file, d.pos, d.run_start));
                        self.anchor = Some((d.file, d.pos));
                        self.anchor_seq = d.seq;
                        for o in self.demands.iter_mut().filter(|o| !o.probe) {
                            o.parked = Some(o.pos);
                        }
                    } else if !d.probe && self.focus().is_none() {
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
                    // A verified piece frees a slot under the open-pieces
                    // limit: fill now, not at the next block.
                    self.fill_all |= self.window.is_some();
                    for b in self.geo.piece_blocks(piece) {
                        self.mark_received(b);
                        for o in self.holders.get(&b).cloned().unwrap_or_default() {
                            self.cancel(o, b, out);
                        }
                    }
                    self.check_ready(out);
                }
            }
            Event::PieceFailed { piece } | Event::PieceEvicted { piece } => {
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
        let w = self.windows_core(file, pos, end, play);
        let f = self.geo.files()[file];
        Windows {
            readahead_end: (w.playback_end + self.window_readahead(file)).min(f.offset + f.len),
            ..w
        }
    }

    /// [`Self::windows`] with no readahead: it ends at the playback end.
    fn windows_core(&self, file: usize, pos: u64, end: Option<u64>, play: Option<u64>) -> Windows {
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
        Windows {
            blocking: (f.offset + pos, f.offset + b_end),
            critical_end: f.offset + c_end,
            playback_end: f.offset + p_end,
            readahead_end: f.offset + p_end,
        }
    }

    /// The pieces of `file`'s container index; while it is pending, the
    /// fixed tail, where an MP4 written before its `moov` keeps it
    /// (plans/research/start-latency.md, cause 4).
    fn index_pieces(&self, file: usize) -> Range<PieceIdx> {
        let f = self.geo.files()[file];
        match self.index[file] {
            FileIndex::At { start, end } => {
                let end = end.min(f.len);
                self.geo
                    .pieces_in(f.offset + start.min(end), f.offset + end)
            }
            FileIndex::Tail | FileIndex::Pending if self.cfg.tail_bytes > 0 => {
                let tail = f.offset + f.len.saturating_sub(self.cfg.tail_bytes);
                self.geo.pieces_in(tail, f.offset + f.len)
            }
            FileIndex::Tail | FileIndex::Pending => 0..0,
        }
    }

    /// The newest read that is not a probe, unless the focus closed after
    /// it opened (then the anchor stands for the player).
    fn focus(&self) -> Option<&Demand> {
        self.demands
            .iter()
            .filter(|d| !d.probe && d.seq > self.anchor_seq)
            .max_by_key(|d| d.seq)
    }

    /// Where the focused read's readahead ends: the file's readahead, or in
    /// a window the ramp's, and none during a seek's grace (2.2, 2.3).
    fn readahead_end(&self, d: &Demand, w: &Windows) -> u64 {
        let f = self.geo.files()[d.file];
        let ahead = match self.window {
            Some(win) if win.file == d.file => {
                let cap = self.window_readahead(d.file);
                if self.grace.is_some() {
                    0
                } else {
                    (2 * d.pos.saturating_sub(d.run_start)).clamp(win.ramp_floor.min(cap), cap)
                }
            }
            _ => self.readahead[d.file],
        };
        (w.playback_end + ahead).min(f.offset + f.len)
    }

    /// `file`'s kept readahead (2.4). A budget the space guard shrank takes
    /// its cut first from the room past the kept set (the watched bytes
    /// behind `B`, which the keeper takes farthest back first), then off
    /// the readahead's far end, so that the kept set stays `slack / 2`
    /// under the keeper's target (`cap - slack`): the readahead stays whole
    /// while it fits, its farthest part leaves first, and it grows back
    /// with the budget. The `slack / 2` is one step of the guard: the
    /// keeper leaves that much past the kept set, nearest first, so a read
    /// that moves on between steps finds it on disk rather than fetching
    /// what the next step takes. A plan whose readahead passed the target
    /// at its full budget loses the cut itself.
    fn window_readahead(&self, file: usize) -> u64 {
        let full = self.readahead[file];
        let Some(w) = self.window.filter(|w| w.file == file) else {
            return full;
        };
        let cut = self.full_budget.saturating_sub(w.budget);
        if cut == 0 {
            return full;
        }
        // ponytail: a scan of every piece per call while the guard binds,
        // as `held_bytes` per fill.
        let kept = self.kept_with(false);
        let rest: u64 = (0..self.geo.num_pieces())
            .filter(|&p| kept.get(p))
            .map(|p| u64::from(self.geo.piece_size(p)))
            .sum();
        let step = w.slack / 2;
        let over = (rest + full + w.slack + step).saturating_sub(self.cap(&w));
        full.saturating_sub(cut.min(over))
    }

    /// Readahead pieces that may be open at once (2.2): `clamp(2 s × the
    /// fill rate, floor, cap)` in pieces, in a window.
    fn open_limit(&self) -> Option<usize> {
        let win = self.window?;
        let fill: u64 = self
            .peers
            .values()
            .filter(|p| !p.choked)
            .map(|p| self.rate_of(p))
            .sum();
        let bytes = (2 * fill).clamp(win.open_floor, win.open_cap.max(win.open_floor));
        Some((bytes / u64::from(self.geo.piece_len())).max(1) as usize)
    }

    /// Pieces the keeper never evicts (plans/streaming-window.md 2.3): the
    /// windowed file's head blocking window and index; the focused read's
    /// `[pos − B, readahead end)`; in a grace the previous focus's
    /// `[pos − B, playback end)`; a seeded resume anchor's the same until a
    /// player read opens near it; every other read's blocking window; and
    /// with no reader the anchor's `[pos − B, playback end)`, its readahead
    /// too while paused. Every piece with no window.
    pub fn kept(&self) -> Bitfield {
        self.kept_with(true)
    }

    /// [`Self::kept`], with the readahead or without it.
    fn kept_with(&self, readahead: bool) -> Bitfield {
        let n = self.geo.num_pieces();
        let Some(win) = self.window else {
            return Bitfield::full(n);
        };
        let mut kept = Bitfield::new(n);
        let g = &self.geo;
        let mut keep = |r: Range<PieceIdx>| {
            for p in r {
                kept.set(p, true);
            }
        };
        let back = |file: usize, pos: u64| g.files()[file].offset + pos.saturating_sub(win.behind);
        let head = self.windows_core(win.file, 0, None, None);
        keep(g.pieces_in(head.blocking.0, head.blocking.1));
        keep(self.index_pieces(win.file));
        let focus = self.focus().map(|d| d.id);
        for d in &self.demands {
            let w = self.windows_core(d.file, d.pos, d.end, self.play_pos[d.file]);
            if Some(d.id) == focus {
                let end = if readahead {
                    self.readahead_end(d, &w)
                } else {
                    w.playback_end
                };
                keep(g.pieces_in(back(d.file, d.pos), end));
            } else {
                keep(g.pieces_in(w.blocking.0, w.blocking.1));
            }
        }
        for (file, pos) in self
            .grace
            .map(|g| (g.1, g.2))
            .into_iter()
            .chain(self.resume)
        {
            let w = self.windows_core(file, pos, None, None);
            keep(g.pieces_in(back(file, pos), w.playback_end));
        }
        if focus.is_none()
            && let Some((file, pos)) = self.anchor
        {
            // With no readahead, `readahead_end` is the playback end.
            let w = if self.paused && readahead {
                self.windows(file, pos, None, None)
            } else {
                self.windows_core(file, pos, None, None)
            };
            keep(g.pieces_in(back(file, pos), w.readahead_end));
        }
        kept
    }

    /// Bytes the torrent holds for the keeper (2.4): verified pieces, and
    /// pieces partly received or requested, at full size.
    pub fn held_bytes(&self) -> u64 {
        (0..self.geo.num_pieces())
            .filter(|&p| self.verified.get(p) || self.busy[p as usize] > 0)
            .map(|p| u64::from(self.geo.piece_size(p)))
            .sum()
    }

    /// What the torrent may hold outside the urgent windows (2.4): the
    /// budget less one seek's blocking and critical windows rounded out to
    /// pieces, plus a piece, so a seek's urgent fetches stay within `W`.
    fn cap(&self, w: &Window) -> u64 {
        let crit = self
            .cfg
            .critical_min_bytes
            .max(self.cfg.critical_secs * self.bitrate[w.file]);
        let piece = u64::from(self.geo.piece_len());
        let urgent = (self.cfg.blocking_bytes + crit).div_ceil(piece) * piece + piece;
        w.budget.saturating_sub(urgent)
    }

    /// Bytes the torrent has written (2.4): verified pieces, and the blocks
    /// received of the others. The space guard counts these, not pieces in
    /// flight at full size, since free space has not dropped for those yet.
    pub fn written_bytes(&self) -> u64 {
        (0..self.geo.num_pieces())
            .map(|p| {
                if self.verified.get(p) {
                    u64::from(self.geo.piece_size(p))
                } else if self.busy[p as usize] > 0 {
                    self.geo
                        .piece_blocks(p)
                        .filter(|&b| self.received.get(b))
                        .map(|b| u64::from(self.geo.block_len(b)))
                        .sum()
                } else {
                    0
                }
            })
            .sum()
    }

    /// The keeper is due (2.4): the torrent holds more than
    /// `cap - slack / 2`.
    pub fn keeper_due(&self) -> bool {
        self.window
            .is_some_and(|w| self.held_bytes() > self.cap(&w).saturating_sub(w.slack / 2))
    }

    /// Of `pieces`, those of the windowed file with no block received or
    /// requested and outside the kept set: what the file system may hold
    /// of them is no data of ours (2.4, APFS's filled holes).
    pub fn empty_unkept(&self, pieces: &[PieceIdx]) -> Vec<PieceIdx> {
        let Some(win) = self.window else {
            return Vec::new();
        };
        let kept = self.kept();
        let f = self.geo.files()[win.file];
        let file = self.geo.pieces_in(f.offset, f.offset + f.len);
        pieces
            .iter()
            .copied()
            .filter(|&p| file.contains(&p) && !kept.get(p))
            .filter(|&p| !self.verified.get(p) && self.busy[p as usize] == 0)
            .collect()
    }

    /// What the keeper evicts now: enough to bring the torrent down to
    /// `cap - slack`, in [`Self::evictable`]'s order.
    pub fn to_evict(&self) -> Vec<PieceIdx> {
        let Some(w) = self.window else {
            return Vec::new();
        };
        let target = self.cap(&w).saturating_sub(w.slack);
        self.evictable(self.held_bytes().saturating_sub(target))
    }

    /// When the playback ends (2.4): every piece held but the windowed
    /// file's pinned head and index and `[pos - B, pos + ahead)` around the
    /// last read position, verified or partly written with no request out.
    pub fn trimmable(&self, ahead: u64) -> Vec<PieceIdx> {
        let Some(win) = self.window else {
            return Vec::new();
        };
        let g = &self.geo;
        let f = g.files()[win.file];
        let mut keep = Bitfield::new(g.num_pieces());
        let mut set = |r: Range<PieceIdx>| {
            for p in r {
                keep.set(p, true);
            }
        };
        let head = self.windows(win.file, 0, None, None);
        set(g.pieces_in(head.blocking.0, head.blocking.1));
        set(self.index_pieces(win.file));
        let at = self
            .focus()
            .map(|d| (d.file, d.pos))
            .or(self.anchor)
            .filter(|&(file, _)| file == win.file)
            .map(|(_, pos)| pos);
        if let Some(pos) = at {
            let from = f.offset + pos.saturating_sub(win.behind);
            set(g.pieces_in(from, f.offset + pos.saturating_add(ahead).min(f.len)));
        }
        (0..g.num_pieces())
            .filter(|&p| !keep.get(p))
            .filter(|&p| {
                self.verified.get(p)
                    || (self.busy[p as usize] > 0
                        && !g.piece_blocks(p).any(|b| self.holders.contains_key(&b)))
            })
            .collect()
    }

    /// Pieces to evict for at least `bytes` (2.4), in order: other files'
    /// pieces; the windowed file's verified pieces behind the read,
    /// farthest back first; those beyond it, farthest first; then its
    /// partly written pieces with no request out. Never a kept piece, and
    /// nothing before a player read opens.
    pub fn evictable(&self, bytes: u64) -> Vec<PieceIdx> {
        let Some(win) = self.window else {
            return Vec::new();
        };
        if !self.player_read || bytes == 0 {
            return Vec::new();
        }
        let kept = self.kept();
        let f = self.geo.files()[win.file];
        let file = self.geo.pieces_in(f.offset, f.offset + f.len);
        let at = self
            .focus()
            .map(|d| (d.file, d.pos))
            .or(self.anchor)
            .filter(|&(file, _)| file == win.file)
            .map_or(file.start, |(_, pos)| self.geo.piece_at(f.offset + pos));
        let orphan = |p: PieceIdx| {
            !self.verified.get(p)
                && self.busy[p as usize] > 0
                && !self
                    .geo
                    .piece_blocks(p)
                    .any(|b| self.holders.contains_key(&b))
        };
        let out = |p: PieceIdx| !kept.get(p);
        let verified = |p: PieceIdx| out(p) && self.verified.get(p);
        let others = (0..self.geo.num_pieces())
            .filter(|p| !file.contains(p))
            .filter(|&p| out(p) && (self.verified.get(p) || orphan(p)));
        let behind = (file.start..at).filter(|&p| verified(p));
        let beyond = (at..file.end).rev().filter(|&p| verified(p));
        let orphans = file.clone().filter(|&p| out(p) && orphan(p));
        let mut freed = 0;
        others
            .chain(behind)
            .chain(beyond)
            .chain(orphans)
            .take_while(|&p| {
                let more = freed < bytes;
                freed += u64::from(self.geo.piece_size(p));
                more
            })
            .collect()
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
                g.pieces_in(w.playback_end, self.readahead_end(d, &w)),
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
        if self.focus().is_none()
            && let Some((file, pos)) = self.anchor
        {
            // No reader left: keep the last position's windows warm,
            // non-urgent; a window's readahead only while paused (2.3).
            let w = self.windows(file, pos, None, None);
            let g = &self.geo;
            spans.push((g.pieces_in(w.blocking.0, w.playback_end), Class::Playback));
            if self.paused || self.window.is_none_or(|win| win.file != file) {
                spans.push((
                    g.pieces_in(w.playback_end, w.readahead_end),
                    Class::Readahead,
                ));
            }
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
        // Readahead in file order, a piece only one peer holds first
        // (plans/streaming-window.md 2.2: we upload nothing, so rarity
        // helps no peer, and the minutes ahead become one run).
        readahead.sort_unstable_by_key(|&p| (self.avail[p as usize] > 1, p));
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
            let ready = self.index[d.file] != FileIndex::Pending
                && self.ready_set(d).all(|p| self.verified.get(p));
            if ready {
                self.demands[i].ready = true;
                self.started[self.demands[i].file] = true;
                out.push(Action::Ready {
                    demand: self.demands[i].id,
                });
            }
        }
    }

    /// The pieces `d`'s Ready waits on: its blocking window from where it
    /// opened, and its file's index.
    fn ready_set(&self, d: &Demand) -> impl Iterator<Item = PieceIdx> {
        let w = self.windows_core(d.file, d.start, d.end, None);
        let blocking = self.geo.pieces_in(w.blocking.0, w.blocking.1);
        blocking.chain(self.index_pieces(d.file))
    }

    /// A start, for a loading screen: the bytes of the pieces its Ready
    /// waits on and of those verified. The focused read, or with none the
    /// newest (a resume's head read is a probe); `None` once its file has
    /// started, or with no read.
    pub fn start_bytes(&self) -> Option<StartBytes> {
        let waiting = |d: &&Demand| !self.started[d.file];
        let newest = || self.demands.iter().filter(waiting).max_by_key(|d| d.seq);
        let d = self.focus().filter(waiting).or_else(newest)?;
        let mut pieces: Vec<PieceIdx> = self.ready_set(d).collect();
        pieces.sort_unstable();
        pieces.dedup();
        let size = |&p: &PieceIdx| u64::from(self.geo.piece_size(p));
        Some(StartBytes {
            need: pieces.iter().map(size).sum(),
            have: pieces
                .iter()
                .filter(|&&p| self.verified.get(p))
                .map(size)
                .sum(),
            index_pending: self.index[d.file] == FileIndex::Pending,
        })
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
        // ponytail: a scan of every piece per fill, an incremental count if
        // torrents of 100k pieces show up.
        if self.window.is_some() {
            self.held = self.held_bytes();
        }
        self.assign_urgent(cands.clone(), out);
        for id in fill {
            self.fill_peer(id, &cands, out);
        }
    }

    /// The hard ceiling for urgent starts in a window (2.4): past the
    /// budget only blocking-window pieces start, and those only within one
    /// blocking span over it, so a round that waits on a slow sync never
    /// lets urgent fetches run on; the player's buffer covers the wait.
    /// A piece that starts counts in `held` at once.
    fn urgent_fits(&mut self, piece: PieceIdx) -> bool {
        let Some(w) = self.window else {
            return true;
        };
        if self.busy[piece as usize] > 0 {
            return true;
        }
        let size = u64::from(self.geo.piece_size(piece));
        let span = if self.class[piece as usize] == Some(Class::Blocking) {
            let piece_len = u64::from(self.geo.piece_len());
            self.cfg.blocking_bytes.div_ceil(piece_len) * piece_len + piece_len
        } else {
            0
        };
        if self.held + size > w.budget + span {
            return false;
        }
        self.held += size;
        true
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
            if !self.urgent_fits(piece) {
                continue;
            }
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
                if !others && self.peers[&id].have.get(piece) && self.urgent_fits(piece) {
                    room = self.take_blocks(id, piece, room, out);
                }
            }
        }
        let limit = self.open_limit();
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
            if !self.peers[&id].have.get(piece) {
                continue;
            }
            // Past the window's cap no new piece starts; the keeper
            // makes room (2.4).
            if let Some(w) = self.window
                && self.busy[piece as usize] == 0
            {
                let size = u64::from(self.geo.piece_size(piece));
                if self.held + size > self.cap(&w) {
                    continue;
                }
                self.held += size;
            }
            if let Some(limit) = limit
                && self.busy[piece as usize] == 0
                && self.class[piece as usize] == Some(Class::Readahead)
            {
                let (busy, verified) = (&self.busy, &self.verified);
                self.ra_open
                    .retain(|&p| busy[p as usize] > 0 && !verified.get(p));
                if self.ra_open.len() >= limit {
                    continue;
                }
                self.ra_open.push(piece);
            }
            room = self.take_blocks(id, piece, room, out);
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
