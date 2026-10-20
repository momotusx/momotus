//! Per-torrent adapter between librqbit (the fork's `ExtEvent`s and
//! `ext_request` / `ext_cancel`) and the DWH scheduler, plus readiness for
//! open ranges in both modes.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use librqbit::{ExtEvent, ManagedTorrent};
use mq_engine_sched::{
    Action, Bitfield, BlockIdx, Config, DemandId, Event, FileIndex, Geometry, PeerId, Scheduler,
    Stats,
};
use tokio::sync::{mpsc, watch};

/// `reqq` assumed when a peer sent no BEP 10 handshake (libtorrent's default).
const DEFAULT_REQQ: u32 = 250;
const TICK: Duration = Duration::from_millis(100);
const RATE_EVERY: Duration = Duration::from_secs(1);

/// From open readers to the adapter.
#[derive(Debug)]
pub(crate) enum Msg {
    Open {
        demand: DemandId,
        file: usize,
        start: u64,
        end: Option<u64>,
        ready: watch::Sender<bool>,
    },
    Advance {
        demand: DemandId,
        pos: u64,
    },
    Close {
        demand: DemandId,
    },
    /// librqbit stopped the torrent (keep-alive expiry): every peer is gone.
    Stopped,
    /// Every peer's tally, connected or gone (`Engine::peer_report`).
    Report(tokio::sync::oneshot::Sender<Vec<(SocketAddr, PeerTally)>>),
}

/// What one peer did for us since the file was chosen: counts only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerTally {
    /// Pieces it has, by its bitfield and HAVE messages.
    pub pieces: u32,
    pub unchokes: u32,
    pub chokes: u32,
    pub first_unchoke_ms: Option<u64>,
    /// REQUESTs the scheduler sent it.
    pub requests: u64,
    pub blocks: u64,
    pub bytes: u64,
    /// When its handshake finished, from the file choice.
    pub connected_ms: u64,
    /// When it left, if it did.
    pub gone_ms: Option<u64>,
}

/// Gone peers kept for the report.
const GONE_KEPT: usize = 512;

/// A REQUEST or CANCEL for the executor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wire {
    pub cancel: bool,
    pub peer: SocketAddr,
    pub piece: u32,
    pub begin: u32,
    pub len: u32,
}

struct Slot {
    id: PeerId,
    have: Bitfield,
    choked: bool,
    reqq: u32,
    /// Payload bytes since the last rate sample.
    bytes: u64,
    /// Smallest request-to-arrival time seen (RTT plus one block), 0 if none.
    rtt_us: u64,
    tally: PeerTally,
}

struct Demand {
    file: usize,
    start: u64,
    end: Option<u64>,
    ready: watch::Sender<bool>,
}

/// Counters for `Engine::stats`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    /// Microseconds from adapter start to the first ready range.
    pub ready_us: Option<u64>,
    pub dwh: Option<Stats>,
    /// The capacity sample (plans/m13-stream-choice.md 5): payload bytes
    /// received while block requests were outstanding, and that time, in
    /// the first 30 s.
    pub sample_bytes: u64,
    pub sample_busy_us: u64,
    /// Payload bytes received from peers, whether or not their piece has
    /// verified yet: a piece verifies only whole, so this moves first.
    pub received_bytes: u64,
    /// Microseconds from adapter start to the first peer unchoking us.
    pub unchoked_us: Option<u64>,
    /// Bytes of pieces verified again after they had verified and left.
    pub refetched_bytes: u64,
}

/// The capacity sample covers the adapter's first 30 s.
const SAMPLE_WINDOW_US: u64 = 30_000_000;

/// Pure translation state: librqbit events in, wire actions out. Time comes
/// only through `tick`, so tests drive it without a runtime.
pub(crate) struct Translator {
    sched: Option<Scheduler>,
    /// Block numbering, in both modes; `None` only for an empty torrent.
    geo: Option<Geometry>,
    piece_len: u64,
    files: Vec<(u64, u64)>,
    num_pieces: u32,
    blocking_bytes: u64,
    tail_bytes: u64,
    peers: HashMap<SocketAddr, Slot>,
    addrs: HashMap<PeerId, SocketAddr>,
    reqq: HashMap<SocketAddr, u32>,
    next_peer: PeerId,
    sent: HashMap<(PeerId, BlockIdx), u64>,
    have: Vec<bool>,
    /// Pieces that verified at least once, for `refetched_bytes`.
    verified_once: Vec<bool>,
    refetched_bytes: u64,
    demands: HashMap<DemandId, Demand>,
    now_us: u64,
    ready_us: Option<u64>,
    actions: Vec<Action>,
    sample_bytes: u64,
    sample_busy_us: u64,
    received_bytes: u64,
    unchoked_us: Option<u64>,
    gone: Vec<(SocketAddr, PeerTally)>,
    /// Per file: its container index (README 12 of mq-engine-sched), and
    /// whether its head was read for it.
    index: Vec<(FileIndex, bool)>,
    /// Per file: named as an MP4 or MOV, so its index waits for the head.
    iso_bmff: Vec<bool>,
}

/// Bytes at a file's head read for its container index: every top-level
/// box before `moov` or `mdat` sits here.
const INDEX_HEAD: u64 = 4096;

/// The largest `moov` taken after `mdat`: tens of MiB for a long 4K remux.
const MAX_INDEX: u64 = 64 << 20;

/// Where an MP4 or MOV file keeps the index a player reads before its first
/// frame, from the file's first bytes: `[0, moov end)` when `moov` comes
/// first, `[mdat end, file end)` when it follows `mdat`. `None` for any other
/// container, or when the head does not say.
pub(crate) fn container_index(head: &[u8], len: u64) -> Option<(u64, u64)> {
    let mut pos = 0u64;
    for first in [true, false, false, false, false, false, false, false] {
        let at = usize::try_from(pos).ok()?;
        // A box size near u64::MAX puts `at` there: every offset is checked.
        let field = |from: usize, to: usize| head.get(at.checked_add(from)?..at.checked_add(to)?);
        let kind = field(4, 8)?;
        if first
            && !matches!(
                kind,
                b"ftyp" | b"moov" | b"mdat" | b"free" | b"skip" | b"wide"
            )
        {
            return None;
        }
        let size = match u32::from_be_bytes(field(0, 4)?.try_into().ok()?) {
            0 => len.checked_sub(pos)?,
            1 => u64::from_be_bytes(field(8, 16)?.try_into().ok()?),
            n => u64::from(n),
        };
        let end = pos.checked_add(size).filter(|_| size >= 8)?;
        match kind {
            b"moov" => return Some((0, end.min(len))),
            // More than `MAX_INDEX` after `mdat` is not one `moov` (a second
            // `mdat`, say, which the head cannot show): the fixed tail.
            b"mdat" => return (end < len && len - end <= MAX_INDEX).then_some((end, len)),
            _ => pos = end,
        }
    }
    None
}

impl Translator {
    /// `drive` hands block choice to DWH, for any piece length.
    pub fn new(piece_len: u32, file_lens: &[u64], cfg: Config, drive: bool) -> Self {
        let mut offset = 0;
        let files = file_lens
            .iter()
            .map(|&len| {
                offset += len;
                (offset - len, len)
            })
            .collect();
        let num_pieces = offset.div_ceil(u64::from(piece_len.max(1))) as u32;
        let (blocking_bytes, tail_bytes) = (cfg.blocking_bytes, cfg.tail_bytes);
        let geo = Geometry::new(piece_len, file_lens);
        let sched = geo
            .clone()
            .filter(|_| drive)
            .map(|geo| Scheduler::new(geo, cfg));
        Self {
            sched,
            geo,
            piece_len: u64::from(piece_len.max(1)),
            files,
            num_pieces,
            blocking_bytes,
            tail_bytes,
            peers: HashMap::new(),
            addrs: HashMap::new(),
            reqq: HashMap::new(),
            next_peer: 0,
            sent: HashMap::new(),
            have: vec![false; num_pieces as usize],
            verified_once: vec![false; num_pieces as usize],
            refetched_bytes: 0,
            demands: HashMap::new(),
            now_us: 0,
            ready_us: None,
            actions: Vec::new(),
            sample_bytes: 0,
            sample_busy_us: 0,
            received_bytes: 0,
            unchoked_us: None,
            gone: Vec::new(),
            index: vec![(FileIndex::Tail, false); file_lens.len()],
            iso_bmff: vec![false; file_lens.len()],
        }
    }

    /// Which files are named MP4 or MOV: their tail is not fetched before
    /// their head says where the `moov` is.
    pub fn with_iso_bmff(mut self, files: Vec<bool>) -> Self {
        self.iso_bmff = files;
        self
    }

    /// A file an open range reads whose head is verified but not yet read
    /// for its container index, and how many head bytes to read.
    pub fn index_wanted(&self) -> Option<(usize, u64)> {
        self.demands.values().find_map(|d| {
            let &(off, len) = self.files.get(d.file)?;
            if self.index.get(d.file).is_none_or(|i| i.1) || len == 0 {
                return None;
            }
            let n = len.min(INDEX_HEAD);
            let pieces = off / self.piece_len..=(off + n - 1) / self.piece_len;
            pieces
                .clone()
                .all(|p| self.have.get(p as usize).copied().unwrap_or(false))
                .then_some((d.file, n))
        })
    }

    /// `head`, the file's first bytes, read: its container index (if it has
    /// one) replaces the fixed tail.
    pub fn index_found(&mut self, file: usize, head: &[u8]) -> Vec<Wire> {
        let (Some(&(_, len)), Some(&(was, _))) = (self.files.get(file), self.index.get(file))
        else {
            return Vec::new();
        };
        let index = match container_index(head, len) {
            Some((start, end)) => FileIndex::At { start, end },
            None => FileIndex::Tail,
        };
        self.index[file] = (index, true);
        if index == was {
            return Vec::new();
        }
        let out = self.feed(Event::FileIndex { file, index });
        self.check_ready_stock();
        out
    }

    /// Every peer's tally: the connected ones, then those that left.
    pub fn report(&self) -> Vec<(SocketAddr, PeerTally)> {
        let live = self.peers.iter().map(|(addr, s)| {
            let mut t = s.tally.clone();
            t.pieces = s.have.count_ones();
            (*addr, t)
        });
        live.chain(self.gone.iter().cloned()).collect()
    }

    pub fn drives(&self) -> bool {
        self.sched.is_some()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            ready_us: self.ready_us,
            dwh: self.sched.as_ref().map(|s| s.stats().clone()),
            sample_bytes: self.sample_bytes,
            sample_busy_us: self.sample_busy_us,
            received_bytes: self.received_bytes,
            unchoked_us: self.unchoked_us,
            refetched_bytes: self.refetched_bytes,
        }
    }

    pub fn tick(&mut self, now_us: u64) -> Vec<Wire> {
        // Time with a block request outstanding counts toward the sample.
        if !self.sent.is_empty() && now_us <= SAMPLE_WINDOW_US {
            self.sample_busy_us += now_us.saturating_sub(self.now_us);
        }
        self.now_us = self.now_us.max(now_us);
        self.feed(Event::Tick {
            now_us: self.now_us,
        })
    }

    /// Pieces already verified (found on disk at start).
    pub fn verified_bitfield(&mut self, bytes: &[u8]) -> Vec<Wire> {
        let mut out = Vec::new();
        for piece in 0..self.num_pieces {
            if bit(bytes, piece) {
                out.extend(self.on_ext(ExtEvent::PieceChecked { piece, ok: true }));
            }
        }
        out
    }

    pub fn on_ext(&mut self, ev: ExtEvent) -> Vec<Wire> {
        match ev {
            ExtEvent::PeerConnected { peer, bitfield } => {
                let mut have = Bitfield::new(self.num_pieces);
                (0..self.num_pieces).for_each(|p| have.set(p, bit(&bitfield, p)));
                let id = match self.peers.get(&peer) {
                    Some(s) => s.id,
                    None => {
                        self.next_peer += 1;
                        self.next_peer
                    }
                };
                let reqq = self.reqq.get(&peer).copied().unwrap_or(DEFAULT_REQQ);
                self.addrs.insert(id, peer);
                let tally = self.peers.get(&peer).map_or_else(
                    || PeerTally {
                        connected_ms: self.now_us / 1000,
                        ..Default::default()
                    },
                    |s| s.tally.clone(),
                );
                self.peers.insert(
                    peer,
                    Slot {
                        id,
                        have: have.clone(),
                        choked: true,
                        reqq,
                        bytes: 0,
                        rtt_us: 0,
                        tally,
                    },
                );
                self.feed(Event::PeerConnected {
                    peer: id,
                    have,
                    reqq,
                })
            }
            ExtEvent::PeerReqq { peer, reqq } => {
                self.reqq.insert(peer, reqq);
                // Usually the BEP 10 handshake comes before the first unchoke,
                // so nothing is queued yet and the peer can be re-announced.
                let idle = self.peers.get(&peer).is_some_and(|s| {
                    s.reqq != reqq && self.sched.as_ref().is_none_or(|x| x.outstanding(s.id) == 0)
                });
                if !idle {
                    return Vec::new();
                }
                let s = self.peers.get_mut(&peer).expect("checked above");
                s.reqq = reqq;
                let (id, have, choked) = (s.id, s.have.clone(), s.choked);
                let mut out = self.feed(Event::PeerConnected {
                    peer: id,
                    have,
                    reqq,
                });
                if !choked {
                    out.extend(self.feed(Event::PeerUnchoked { peer: id }));
                }
                out
            }
            ExtEvent::PeerHave { peer, piece } => match self.peers.get_mut(&peer) {
                Some(s) => {
                    s.have.set(piece, true);
                    let id = s.id;
                    self.feed(Event::PeerHave { peer: id, piece })
                }
                None => Vec::new(),
            },
            ExtEvent::PeerChoked { peer } | ExtEvent::PeerUnchoked { peer } => {
                let choked = matches!(ev, ExtEvent::PeerChoked { .. });
                match self.peers.get_mut(&peer) {
                    Some(s) => {
                        s.choked = choked;
                        let id = s.id;
                        if choked {
                            s.tally.chokes += 1;
                            self.sent.retain(|k, _| k.0 != id);
                            self.feed(Event::PeerChoked { peer: id })
                        } else {
                            s.tally.unchokes += 1;
                            s.tally.first_unchoke_ms.get_or_insert(self.now_us / 1000);
                            self.unchoked_us.get_or_insert(self.now_us);
                            self.feed(Event::PeerUnchoked { peer: id })
                        }
                    }
                    None => Vec::new(),
                }
            }
            ExtEvent::PeerGone { peer } => self.drop_peer(peer),
            ExtEvent::Block { peer, piece, begin } => {
                let (Some(s), Some(geo)) = (self.peers.get_mut(&peer), &self.geo) else {
                    return Vec::new();
                };
                let block = geo.block_at(piece, begin);
                let payload = u64::from(geo.block_len(block));
                s.bytes += payload;
                s.tally.blocks += 1;
                s.tally.bytes += payload;
                self.received_bytes += payload;
                // Capacity counts only while a request is out: a block that
                // comes after its cancel, with nothing in flight, is not the
                // line's speed.
                if self.now_us <= SAMPLE_WINDOW_US && !self.sent.is_empty() {
                    self.sample_bytes += payload;
                }
                if let Some(sent) = self.sent.remove(&(s.id, block)) {
                    let sample = self.now_us.saturating_sub(sent).max(1);
                    s.rtt_us = if s.rtt_us == 0 {
                        sample
                    } else {
                        s.rtt_us.min(sample)
                    };
                }
                let id = s.id;
                self.feed(Event::BlockReceived { peer: id, block })
            }
            ExtEvent::PieceChecked { piece, ok } => {
                let p = piece as usize;
                if ok && !self.have.get(p).copied().unwrap_or(true) {
                    if self.verified_once[p] {
                        let total = self.files.last().map_or(0, |&(o, l)| o + l);
                        let start = u64::from(piece) * self.piece_len;
                        self.refetched_bytes += self.piece_len.min(total - start);
                    }
                    self.verified_once[p] = true;
                }
                if let Some(h) = self.have.get_mut(p) {
                    *h = ok;
                }
                let out = self.feed(if ok {
                    Event::PieceVerified { piece }
                } else {
                    Event::PieceFailed { piece }
                });
                self.check_ready_stock();
                out
            }
        }
    }

    pub fn on_msg(&mut self, msg: Msg) -> Vec<Wire> {
        match msg {
            Msg::Open {
                demand,
                file,
                start,
                end,
                ready,
            } => {
                self.demands.insert(
                    demand,
                    Demand {
                        file,
                        start,
                        end,
                        ready,
                    },
                );
                // An MP4's tail may hold nothing it needs: wait for its head.
                let mut out = Vec::new();
                let unread = self.index.get(file) == Some(&(FileIndex::Tail, false));
                if unread && self.iso_bmff.get(file) == Some(&true) {
                    self.index[file].0 = FileIndex::Pending;
                    out = self.feed(Event::FileIndex {
                        file,
                        index: FileIndex::Pending,
                    });
                }
                out.extend(self.feed(Event::RangeOpened {
                    demand,
                    file,
                    start,
                    end_hint: end,
                }));
                self.check_ready_stock();
                out
            }
            Msg::Advance { demand, pos } => self.feed(Event::RangeAdvanced { demand, pos }),
            Msg::Close { demand } => {
                self.demands.remove(&demand);
                self.feed(Event::RangeClosed { demand })
            }
            Msg::Stopped => {
                let peers: Vec<SocketAddr> = self.peers.keys().copied().collect();
                peers.into_iter().flat_map(|p| self.drop_peer(p)).collect()
            }
            Msg::Report(tx) => {
                let _ = tx.send(self.report());
                Vec::new()
            }
        }
    }

    /// A REQUEST could not be sent: the peer is gone even if librqbit has
    /// not reported it yet. Dropping it (rather than rejecting the block)
    /// keeps the scheduler from picking the same dead peer again.
    pub fn send_failed(&mut self, peer: SocketAddr) -> Vec<Wire> {
        self.drop_peer(peer)
    }

    /// One payload rate sample per peer (call about once a second).
    pub fn sample_rates(&mut self, elapsed: Duration) -> Vec<Wire> {
        let us = (elapsed.as_micros() as u64).max(1);
        let samples: Vec<(PeerId, u64, u64)> = self
            .peers
            .values_mut()
            .map(|s| {
                (
                    s.id,
                    std::mem::take(&mut s.bytes) * 1_000_000 / us,
                    s.rtt_us,
                )
            })
            .collect();
        let mut out = Vec::new();
        for (peer, bytes_per_s, rtt_us) in samples {
            out.extend(self.feed(Event::PeerRate {
                peer,
                bytes_per_s,
                rtt_us,
            }));
        }
        out
    }

    fn drop_peer(&mut self, peer: SocketAddr) -> Vec<Wire> {
        let Some(s) = self.peers.remove(&peer) else {
            return Vec::new();
        };
        self.addrs.remove(&s.id);
        self.sent.retain(|k, _| k.0 != s.id);
        if self.gone.len() < GONE_KEPT {
            let mut t = s.tally;
            t.pieces = s.have.count_ones();
            t.gone_ms = Some(self.now_us / 1000);
            self.gone.push((peer, t));
        }
        self.feed(Event::PeerDisconnected { peer: s.id })
    }

    fn feed(&mut self, ev: Event) -> Vec<Wire> {
        let Some(sched) = self.sched.as_mut() else {
            return Vec::new();
        };
        sched.handle(ev, &mut self.actions);
        let geo = sched.geometry();
        let mut out = Vec::new();
        let mut ready = Vec::new();
        for a in std::mem::take(&mut self.actions) {
            match a {
                Action::Request { peer, block } | Action::Cancel { peer, block } => {
                    let cancel = matches!(a, Action::Cancel { .. });
                    let Some(&addr) = self.addrs.get(&peer) else {
                        continue;
                    };
                    if cancel {
                        self.sent.remove(&(peer, block));
                    } else {
                        self.sent.insert((peer, block), self.now_us);
                        if let Some(s) = self.peers.get_mut(&addr) {
                            s.tally.requests += 1;
                        }
                    }
                    let (piece, begin, len) = geo.wire(block);
                    out.push(Wire {
                        cancel,
                        peer: addr,
                        piece,
                        begin,
                        len,
                    });
                }
                Action::Ready { demand } => ready.push(demand),
            }
        }
        for demand in ready {
            self.mark_ready(demand);
        }
        out
    }

    fn mark_ready(&mut self, demand: DemandId) {
        if let Some(d) = self.demands.get(&demand) {
            d.ready.send_replace(true);
            self.ready_us.get_or_insert(self.now_us);
        }
    }

    /// Stock mode: the same readiness rule as DWH (`blocking_bytes` from the
    /// range start plus the file's tail, all verified), checked here because
    /// the stock picker emits no `Ready`.
    fn check_ready_stock(&mut self) {
        if self.sched.is_some() {
            return;
        }
        let ready: Vec<DemandId> = self
            .demands
            .iter()
            .filter(|(_, d)| !*d.ready.borrow() && self.servable(d))
            .map(|(&id, _)| id)
            .collect();
        for id in ready {
            self.mark_ready(id);
        }
    }

    fn servable(&self, d: &Demand) -> bool {
        let Some(&(off, len)) = self.files.get(d.file) else {
            return false;
        };
        let start = d.start.min(len);
        let end = (start + self.blocking_bytes)
            .min(d.end.unwrap_or(len))
            .max(start + 1)
            .min(len);
        let all = |a: u64, b: u64| {
            a >= b || (a / self.piece_len..=(b - 1) / self.piece_len).all(|p| self.have[p as usize])
        };
        let index = match self.index.get(d.file).map_or(FileIndex::Tail, |i| i.0) {
            FileIndex::At { start, end } => all(off + start, off + end.min(len)),
            FileIndex::Pending => false,
            FileIndex::Tail => {
                self.tail_bytes == 0 || all(off + len.saturating_sub(self.tail_bytes), off + len)
            }
        };
        all(off + start, off + end) && index
    }
}

/// BEP 3 bitfield: piece 0 is the high bit of byte 0.
fn bit(bytes: &[u8], piece: u32) -> bool {
    bytes
        .get(piece as usize / 8)
        .is_some_and(|b| b & (0x80 >> (piece % 8)) != 0)
}

/// Runs until the engine drops the message sender.
pub(crate) async fn run(
    torrent: Arc<ManagedTorrent>,
    mut tr: Translator,
    mut ext: mpsc::UnboundedReceiver<ExtEvent>,
    mut msgs: mpsc::UnboundedReceiver<Msg>,
    snapshot: Arc<Mutex<Snapshot>>,
) {
    let started = Instant::now();
    let now = || started.elapsed().as_micros() as u64;
    if let Err(e) = torrent.wait_until_initialized().await {
        tracing::warn!("torrent failed to initialize: {e:#}");
    }
    let mut pending = tr.tick(now());
    if let Some(bytes) = torrent.ext_have_pieces() {
        pending.extend(tr.verified_bitfield(&bytes));
    }
    let mut tick = tokio::time::interval(TICK);
    let mut last_rate = Instant::now();
    loop {
        execute(&torrent, &mut tr, pending);
        // A read file's verified head says where its container index is.
        if let Some((file, n)) = tr.index_wanted() {
            let head = read_head(&torrent, file, n).await;
            let more = tr.index_found(file, &head);
            execute(&torrent, &mut tr, more);
        }
        *snapshot.lock().unwrap_or_else(|p| p.into_inner()) = tr.snapshot();
        pending = tokio::select! {
            Some(ev) = ext.recv() => {
                let mut out = tr.tick(now());
                out.extend(tr.on_ext(ev));
                // Drain what is already queued so a burst costs one wakeup.
                while let Ok(ev) = ext.try_recv() {
                    out.extend(tr.on_ext(ev));
                }
                out
            }
            msg = msgs.recv() => match msg {
                Some(msg) => {
                    let mut out = tr.tick(now());
                    out.extend(tr.on_msg(msg));
                    out
                }
                None => return,
            },
            _ = tick.tick() => {
                let mut out = tr.tick(now());
                if last_rate.elapsed() >= RATE_EVERY {
                    out.extend(tr.sample_rates(last_rate.elapsed()));
                    last_rate = Instant::now();
                }
                out
            }
        };
    }
}

/// The first `n` bytes of `file`, already verified (empty when they cannot
/// be read, which leaves the fixed tail in place).
async fn read_head(torrent: &Arc<ManagedTorrent>, file: usize, n: u64) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let read = async {
        let mut stream = torrent.clone().stream(file).await.ok()?;
        let mut head = vec![0u8; usize::try_from(n).ok()?];
        stream.read_exact(&mut head).await.ok()?;
        Some(head)
    };
    tokio::time::timeout(Duration::from_secs(2), read)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Puts wire actions on the peers' connections. A failed send drops the peer
/// from the scheduler, which may produce more actions.
fn execute(torrent: &ManagedTorrent, tr: &mut Translator, mut wire: Vec<Wire>) {
    let live = torrent.live();
    while !wire.is_empty() {
        let mut failed = Vec::new();
        for w in wire {
            match &live {
                Some(l) if w.cancel => l.ext_cancel(w.peer, w.piece, w.begin, w.len),
                Some(l) if l.ext_request(w.peer, w.piece, w.begin, w.len) => {}
                _ if !w.cancel => failed.push(w.peer),
                _ => {}
            }
        }
        failed.dedup();
        wire = failed.into_iter().flat_map(|p| tr.send_failed(p)).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: u64 = 1024;

    fn addr(n: u8) -> SocketAddr {
        SocketAddr::from(([10, 0, 0, n], 6881))
    }

    /// One 4 MiB file in 256 KiB pieces (16 pieces).
    fn translator(drive: bool) -> Translator {
        Translator::new(256 * 1024, &[4096 * KIB], Config::standard(), drive)
    }

    fn open(
        tr: &mut Translator,
        demand: DemandId,
        start: u64,
    ) -> (Vec<Wire>, watch::Receiver<bool>) {
        let (ready, rx) = watch::channel(false);
        let out = tr.on_msg(Msg::Open {
            demand,
            file: 0,
            start,
            end: None,
            ready,
        });
        (out, rx)
    }

    #[test]
    fn bep3_bit_order() {
        assert!(bit(&[0x80], 0));
        assert!(!bit(&[0x80], 1));
        assert!(bit(&[0x00, 0x01], 15));
        assert!(!bit(&[], 3));
    }

    #[test]
    fn translates_a_peer_into_urgent_block_requests() {
        let mut tr = translator(true);
        tr.tick(1_000);
        let (out, _) = open(&mut tr, 1, 0);
        assert!(out.is_empty(), "no peers yet");
        // reqq before the bitfield, as BEP 10 allows.
        tr.on_ext(ExtEvent::PeerReqq {
            peer: addr(1),
            reqq: 64,
        });
        let out = tr.on_ext(ExtEvent::PeerConnected {
            peer: addr(1),
            bitfield: vec![0xff, 0xff],
        });
        assert!(out.is_empty(), "peers start choked");
        let out = tr.on_ext(ExtEvent::PeerUnchoked { peer: addr(1) });
        assert!(!out.is_empty());
        assert!(out.iter().all(|w| !w.cancel && w.peer == addr(1)));
        // The head piece goes first, in 16 KiB blocks, as deep as the
        // bandwidth-delay rule allows for a peer with no rate sample yet.
        assert_eq!((out[0].piece, out[0].begin, out[0].len), (0, 0, 16 * 1024));
        let depth = tr.sched.as_ref().unwrap().peer_depth(1).unwrap();
        assert_eq!(out.len(), depth);
        assert!(depth < 64);
    }

    #[test]
    fn block_arrivals_feed_rtt_and_rate_and_choke_forgets_requests() {
        let mut tr = translator(true);
        tr.tick(1_000);
        open(&mut tr, 1, 0);
        tr.on_ext(ExtEvent::PeerConnected {
            peer: addr(1),
            bitfield: vec![0xff, 0xff],
        });
        let out = tr.on_ext(ExtEvent::PeerUnchoked { peer: addr(1) });
        let first = out[0];
        tr.tick(51_000);
        tr.on_ext(ExtEvent::Block {
            peer: addr(1),
            piece: first.piece,
            begin: first.begin,
        });
        assert_eq!(tr.peers[&addr(1)].rtt_us, 50_000);
        assert_eq!(tr.peers[&addr(1)].bytes, 16 * KIB);
        tr.sample_rates(Duration::from_secs(1));
        assert_eq!(tr.peers[&addr(1)].bytes, 0);
        tr.on_ext(ExtEvent::PeerChoked { peer: addr(1) });
        assert!(tr.sent.is_empty());
        assert_eq!(tr.sched.as_ref().unwrap().outstanding(1), 0);
    }

    /// plans/m13-stream-choice.md 5: the sample counts payload while block
    /// requests are out, and only in the first 30 s.
    #[test]
    fn a_capacity_sample_counts_only_time_with_requests_out() {
        let mut tr = translator(true);
        tr.tick(1_000);
        open(&mut tr, 1, 0);
        // Nothing asked yet: idle time does not count.
        tr.tick(2_001_000);
        assert_eq!(tr.snapshot().sample_busy_us, 0);
        tr.on_ext(ExtEvent::PeerConnected {
            peer: addr(1),
            bitfield: vec![0xff, 0xff],
        });
        let out = tr.on_ext(ExtEvent::PeerUnchoked { peer: addr(1) });
        let first = out[0];
        tr.tick(2_051_000);
        tr.on_ext(ExtEvent::Block {
            peer: addr(1),
            piece: first.piece,
            begin: first.begin,
        });
        let s = tr.snapshot();
        assert_eq!((s.sample_bytes, s.sample_busy_us), (16 * KIB, 50_000));
        // Past 30 s nothing more counts.
        tr.tick(40_000_000);
        let second = out[1];
        tr.on_ext(ExtEvent::Block {
            peer: addr(1),
            piece: second.piece,
            begin: second.begin,
        });
        let s = tr.snapshot();
        assert_eq!(s.sample_bytes, 16 * KIB);
        assert!(s.sample_busy_us < 30_000_000);
    }

    /// A block that comes after its request was given up on, with nothing
    /// in flight, is not capacity; it is still received payload.
    #[test]
    fn a_late_block_with_nothing_in_flight_is_not_capacity() {
        let mut tr = translator(true);
        tr.tick(1_000);
        open(&mut tr, 1, 0);
        tr.on_ext(ExtEvent::PeerConnected {
            peer: addr(1),
            bitfield: vec![0xff, 0xff],
        });
        let out = tr.on_ext(ExtEvent::PeerUnchoked { peer: addr(1) });
        tr.on_ext(ExtEvent::PeerChoked { peer: addr(1) });
        assert!(tr.sent.is_empty());
        tr.tick(51_000);
        tr.on_ext(ExtEvent::Block {
            peer: addr(1),
            piece: out[0].piece,
            begin: out[0].begin,
        });
        let s = tr.snapshot();
        assert_eq!(s.sample_bytes, 0);
        assert_eq!(s.received_bytes, 16 * KIB);
    }

    /// A piece verified again after it had verified and left counts in
    /// `refetched_bytes`, at its own length (the last piece is short); a
    /// repeated report while it is held, or a first verify after a
    /// failure, does not.
    #[test]
    fn a_piece_verified_again_after_it_left_counts_as_refetched() {
        let mut tr = Translator::new(256 * 1024, &[4000 * KIB], Config::standard(), true);
        let check = |tr: &mut Translator, piece, ok| {
            tr.on_ext(ExtEvent::PieceChecked { piece, ok });
        };
        check(&mut tr, 2, false);
        check(&mut tr, 2, true);
        check(&mut tr, 2, true);
        check(&mut tr, 15, true);
        assert_eq!(tr.snapshot().refetched_bytes, 0);
        for piece in [2, 15] {
            check(&mut tr, piece, false);
            check(&mut tr, piece, true);
        }
        assert_eq!(tr.snapshot().refetched_bytes, 256 * KIB + 160 * KIB);
    }

    /// The stall rule's inputs: every payload block counts at once, before
    /// any piece verifies, and the first unchoke's time stays the first.
    #[test]
    fn received_payload_and_the_first_unchoke_are_reported() {
        let mut tr = translator(true);
        tr.tick(1_000);
        open(&mut tr, 1, 0);
        for p in 1..=2 {
            tr.on_ext(ExtEvent::PeerConnected {
                peer: addr(p),
                bitfield: vec![0xff, 0xff],
            });
        }
        assert_eq!(tr.snapshot().unchoked_us, None, "connected, still choked");
        tr.tick(3_001_000);
        let out = tr.on_ext(ExtEvent::PeerUnchoked { peer: addr(1) });
        tr.tick(4_001_000);
        tr.on_ext(ExtEvent::PeerUnchoked { peer: addr(2) });
        for w in &out[..3] {
            tr.on_ext(ExtEvent::Block {
                peer: addr(1),
                piece: w.piece,
                begin: w.begin,
            });
        }
        let s = tr.snapshot();
        assert_eq!(s.unchoked_us, Some(3_001_000));
        assert_eq!(s.received_bytes, 48 * KIB);
        assert!(!tr.have.iter().any(|h| *h), "no piece verified yet");
    }

    #[test]
    fn verified_pieces_make_a_range_ready_and_failed_sends_drop_the_peer() {
        let mut tr = translator(true);
        tr.tick(1_000);
        let (_, ready) = open(&mut tr, 7, 0);
        // Blocking window (1 MiB = pieces 0..4) and tail (2 MiB = pieces 8..16).
        let mut bits = [0u8; 2];
        for p in (0..4).chain(8..16) {
            bits[p / 8] |= 0x80 >> (p % 8);
        }
        tr.verified_bitfield(&bits);
        assert!(*ready.borrow());
        assert!(tr.snapshot().ready_us.is_some());

        tr.on_ext(ExtEvent::PeerConnected {
            peer: addr(2),
            bitfield: vec![0xff, 0xff],
        });
        let out = tr.on_ext(ExtEvent::PeerUnchoked { peer: addr(2) });
        assert!(!out.is_empty(), "critical window still wanted");
        let out = tr.send_failed(addr(2));
        assert!(out.is_empty());
        assert!(tr.peers.is_empty() && tr.sent.is_empty());
        // librqbit reports the same peer later: ignored.
        assert!(tr.on_ext(ExtEvent::PeerGone { peer: addr(2) }).is_empty());
    }

    #[test]
    fn stock_mode_requests_nothing_but_tracks_readiness() {
        let mut tr = translator(false);
        assert!(!tr.drives());
        let (_, ready) = open(&mut tr, 1, 3 * 1024 * KIB);
        let out = tr.on_ext(ExtEvent::PeerConnected {
            peer: addr(1),
            bitfield: vec![0xff, 0xff],
        });
        assert!(out.is_empty());
        // A range starting at 3 MiB needs pieces 12..16, all inside the tail.
        for piece in 12..15 {
            tr.on_ext(ExtEvent::PieceChecked { piece, ok: true });
        }
        assert!(!*ready.borrow());
        tr.on_ext(ExtEvent::PieceChecked {
            piece: 8,
            ok: false,
        });
        tr.on_ext(ExtEvent::PieceChecked {
            piece: 15,
            ok: true,
        });
        assert!(!*ready.borrow(), "tail piece 8..12 still missing");
        for piece in 8..12 {
            tr.on_ext(ExtEvent::PieceChecked { piece, ok: true });
        }
        assert!(*ready.borrow());
    }

    /// A 24-byte `ftyp`, then `rest`.
    fn ftyp(rest: &[u8]) -> Vec<u8> {
        let mut head = vec![0, 0, 0, 24];
        head.extend(b"ftypisom");
        head.extend([0; 12]);
        head.extend(rest);
        head
    }

    fn header(size: u32, kind: &[u8; 4]) -> Vec<u8> {
        let mut b = size.to_be_bytes().to_vec();
        b.extend(kind);
        b
    }

    #[test]
    fn the_container_index_is_read_from_the_head() {
        let len = 8 << 20;
        let moov = header(5 << 20, b"moov");
        assert_eq!(
            container_index(&ftyp(&moov), len),
            Some((0, 24 + (5 << 20)))
        );
        let mdat = [header(8, b"free"), header(3 << 20, b"mdat")].concat();
        assert_eq!(
            container_index(&ftyp(&mdat), len),
            Some((24 + 8 + (3 << 20), len))
        );
        let mut large = header(1, b"mdat");
        large.extend((6u64 << 20).to_be_bytes());
        assert_eq!(
            container_index(&ftyp(&large), len),
            Some((24 + (6 << 20), len))
        );
        assert_eq!(container_index(&ftyp(&header(0, b"mdat")), len), None);
        let mkv = [
            0x1a, 0x45, 0xdf, 0xa3, 0x01, 0, 0, 0, 0, 0, 0, 0x23, 0x42, 0x86,
        ];
        assert_eq!(container_index(&mkv, len), None);
        assert_eq!(container_index(&ftyp(&[]), len), None);
    }

    /// A head that would overflow an offset gives no index instead of a
    /// panic, and more than 64 MiB after `mdat` is not taken for one `moov`
    /// (engine review r1, P3).
    #[test]
    fn a_hostile_head_neither_panics_nor_stretches_the_index() {
        // A `free` box whose largesize ends 6 bytes short of u64::MAX: the
        // next box's fields lie past it.
        let mut huge = header(1, b"free");
        huge.extend((u64::MAX - 30).to_be_bytes());
        assert_eq!(container_index(&ftyp(&huge), 8 << 20), None);
        // A 16 MiB `mdat` in a 2 GiB file: nearly 2 GiB after it.
        let mdat = header(16 << 20, b"mdat");
        assert_eq!(container_index(&ftyp(&mdat), 2 << 30), None);
        assert_eq!(
            container_index(&ftyp(&mdat), (16 << 20) + (64 << 20) + 24),
            Some((24 + (16 << 20), (16 << 20) + (64 << 20) + 24)),
            "64 MiB after mdat is still the index"
        );
    }

    /// An MP4-named file: nothing is ready while its head is out, the head
    /// is read once, and then the `moov` (here first, 1.5 MiB) is all the
    /// index Ready waits for: no tail.
    #[test]
    fn an_mp4_waits_for_its_head_then_its_moov() {
        fn bits(pieces: impl IntoIterator<Item = usize>) -> [u8; 2] {
            let mut b = [0u8; 2];
            for p in pieces {
                b[p / 8] |= 0x80 >> (p % 8);
            }
            b
        }
        let mut tr = translator(true).with_iso_bmff(vec![true]);
        tr.tick(1_000);
        let (_, ready) = open(&mut tr, 1, 0);
        assert_eq!(tr.index_wanted(), None, "the head is not in");
        // The old rule's pieces (1 MiB head, 2 MiB tail) are not enough.
        tr.verified_bitfield(&bits((0..4).chain(8..16)));
        assert!(!*ready.borrow(), "ready before the head was read");
        assert_eq!(tr.index_wanted(), Some((0, 4096)));
        tr.index_found(0, &ftyp(&header(3 << 19, b"moov")));
        assert_eq!(tr.index_wanted(), None, "read once");
        assert!(!*ready.borrow(), "ready before the moov");
        // The moov ends in piece 6.
        tr.verified_bitfield(&bits(0..7));
        assert!(*ready.borrow(), "ready on the moov, with no tail");
    }

    /// A piece length that is not a multiple of 16 KiB (YTS uses 136 KiB)
    /// is still driven: every piece ends in a short block, and a delivered
    /// block maps back to the request it answers.
    #[test]
    fn unaligned_pieces_are_driven_in_blocks_per_piece() {
        let mut tr = Translator::new(40_000, &[400_000], Config::standard(), true);
        assert!(tr.drives());
        tr.tick(1_000);
        open(&mut tr, 1, 0);
        tr.on_ext(ExtEvent::PeerConnected {
            peer: addr(1),
            bitfield: vec![0xff, 0xc0],
        });
        let out = tr.on_ext(ExtEvent::PeerUnchoked { peer: addr(1) });
        let wire: Vec<_> = out.iter().map(|w| (w.piece, w.begin, w.len)).collect();
        assert_eq!(
            wire[..4],
            [
                (0, 0, 16_384),
                (0, 16_384, 16_384),
                (0, 32_768, 7_232),
                (1, 0, 16_384)
            ]
        );
        tr.tick(51_000);
        for w in &out[2..4] {
            tr.on_ext(ExtEvent::Block {
                peer: addr(1),
                piece: w.piece,
                begin: w.begin,
            });
        }
        assert_eq!(tr.peers[&addr(1)].bytes, 7_232 + 16_384);
        assert_eq!(tr.peers[&addr(1)].rtt_us, 50_000);
        assert!(!tr.sent.contains_key(&(1, 2)) && !tr.sent.contains_key(&(1, 3)));
        let stats = tr.sched.as_ref().unwrap().stats();
        assert_eq!(
            stats.payload_bytes,
            7_232 + 16_384,
            "both requests answered"
        );
    }
}
