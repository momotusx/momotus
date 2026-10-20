//! Discrete-event swarm and player model shared by the `sim` example and the
//! smoke test. Time advances in 5 ms steps; block transfers are fluid with
//! max-min fair sharing of the client's downlink, and messages travel with
//! each peer's one-way delay. All swarm randomness is drawn from per-seed and
//! per-peer streams that do not depend on the scheduler, so every scheduler
//! sees the same swarm for the same seed.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};

use mq_engine_sched::{
    Action, Bitfield, BlockIdx, Config, DemandId, Event, Geometry, PeerId, PieceIdx, Scheduler,
};

use crate::baselines::{RqbitLike, StremioLike};

pub const MIB: u64 = 1 << 20;
const US: u64 = 1_000_000;
const STEP: u64 = 5_000;
pub const TTP_CAP_S: f64 = 120.0;
pub const SEEK_CAP_S: f64 = 60.0;
pub const SEEKS: usize = 5;
pub const PLAY_BETWEEN_SEEKS: u64 = 6 * US;
const NFO_LEN: u64 = 37_000;
const VIDEO_LEN: u64 = 8 << 30;
const DURATION_S: u64 = 7200;
/// The player starts once this much of the head (or seek target) is servable.
pub const HEAD_BYTES: u64 = MIB;
/// Container index the player reads from the end before starting.
const TAIL_BYTES: u64 = 2 * MIB;
const PLAYER_BUFFER_S: u64 = 30;
const VIDEO: usize = 1;
/// Swarm events are generated up to this horizon.
const HORIZON: u64 = 900 * US;

pub trait Sched {
    fn handle(&mut self, ev: Event, out: &mut Vec<Action>);
}

impl Sched for Scheduler {
    fn handle(&mut self, ev: Event, out: &mut Vec<Action>) {
        Scheduler::handle(self, ev, out);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dwh,
    Rqbit,
    Stremio,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Dwh, Kind::Rqbit, Kind::Stremio];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Dwh => "dwh",
            Kind::Rqbit => "rqbit-like",
            Kind::Stremio => "stremio-like",
        }
    }
}

pub struct Profile {
    pub name: &'static str,
    pub peers: (u64, u64),
    pub seed_frac: f64,
    /// Lognormal upload rate to us, bytes per second.
    pub rate_median: f64,
    pub rate_sigma: f64,
    pub rate_min: f64,
    pub rate_max: f64,
    pub rtt_ms: (f64, f64),
    /// Client downlink, bytes per second.
    pub link: f64,
    pub connect_min_s: f64,
    pub connect_mean_s: f64,
    pub unchoke_s: (f64, f64),
    pub lifetime_mean_s: f64,
    pub flaky_frac: f64,
    pub stall_per_s: f64,
}

/// 25 to 50 peers, 70% seeders, 50 Mbit/s downlink.
pub const HEALTHY: Profile = Profile {
    name: "healthy",
    peers: (25, 50),
    seed_frac: 0.7,
    rate_median: 300e3,
    rate_sigma: 1.0,
    rate_min: 10e3,
    rate_max: 6e6,
    rtt_ms: (20.0, 250.0),
    link: 50e6 / 8.0,
    connect_min_s: 0.3,
    connect_mean_s: 1.5,
    unchoke_s: (0.1, 1.5),
    lifetime_mean_s: 180.0,
    flaky_frac: 0.15,
    stall_per_s: 0.01,
};

/// 4 to 8 slower peers, half of them partial: an old or niche release.
pub const SPARSE: Profile = Profile {
    name: "sparse",
    peers: (4, 8),
    seed_frac: 0.5,
    rate_median: 150e3,
    rate_sigma: 0.8,
    rate_min: 10e3,
    rate_max: 2e6,
    rtt_ms: (50.0, 400.0),
    link: 50e6 / 8.0,
    connect_min_s: 0.5,
    connect_mean_s: 3.0,
    unchoke_s: (0.2, 3.0),
    lifetime_mean_s: 180.0,
    flaky_frac: 0.15,
    stall_per_s: 0.01,
};

/// SplitMix64.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    pub fn exp(&mut self, mean: f64) -> f64 {
        -mean * (1.0 - self.f64()).ln()
    }

    pub fn normal(&mut self) -> f64 {
        let (u, v) = (1.0 - self.f64(), self.f64());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

struct PeerPlan {
    connect_at: u64,
    leave_at: u64,
    rate: f64,
    rtt: u64,
    unchoke_at: u64,
    flaky: bool,
    seeder: bool,
    have: Bitfield,
    rng: Rng,
}

fn secs(s: f64) -> u64 {
    (s * US as f64) as u64
}

fn plan_swarm(p: &Profile, pieces: u32, rng: &mut Rng) -> Vec<PeerPlan> {
    let n = p.peers.0 + rng.next_u64() % (p.peers.1 - p.peers.0 + 1);
    let mut arrivals: VecDeque<u64> = (0..n)
        .map(|_| secs(p.connect_min_s + rng.exp(p.connect_mean_s)))
        .collect();
    let mut plans = Vec::new();
    while let Some(connect_at) = arrivals.pop_front() {
        let leave_at = connect_at + secs(rng.exp(p.lifetime_mean_s));
        if leave_at < HORIZON {
            // Churn: a replacement shows up a few seconds later.
            arrivals.push_back(leave_at + secs(rng.exp(5.0)));
        }
        let rate =
            (p.rate_median * (p.rate_sigma * rng.normal()).exp()).clamp(p.rate_min, p.rate_max);
        let seeder = rng.chance(p.seed_frac);
        let have = if seeder {
            Bitfield::full(pieces)
        } else {
            let frac = rng.range(0.2, 0.9);
            let mut b = Bitfield::new(pieces);
            (0..pieces).for_each(|i| b.set(i, rng.chance(frac)));
            b
        };
        plans.push(PeerPlan {
            connect_at,
            leave_at,
            rate,
            rtt: secs(rng.range(p.rtt_ms.0, p.rtt_ms.1) / 1000.0),
            unchoke_at: connect_at + secs(rng.range(p.unchoke_s.0, p.unchoke_s.1)),
            flaky: rng.chance(p.flaky_frac),
            seeder,
            have,
            rng: Rng::new(rng.next_u64()),
        });
    }
    plans
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Msg {
    Req(u32, BlockIdx),
    Cancel(u32, BlockIdx),
    Block(u32, BlockIdx),
    Reject(u32, BlockIdx),
    Choke(u32),
    Unchoke(u32),
    Have(u32, PieceIdx),
}

#[derive(PartialEq, Eq)]
enum State {
    Pending,
    Live,
    Gone,
}

struct SimPeer {
    plan: PeerPlan,
    state: State,
    choking: bool,
    queue: VecDeque<BlockIdx>,
    serving: Option<(BlockIdx, f64)>,
    stall_until: u64,
    jitter: f64,
    next_flaky: u64,
    rx: u64,
    rate_est: f64,
    unchoked_once: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Head,
    Tail,
    HeadAgain,
    Playing,
    Seeking(u64),
    Done,
}

#[derive(Debug, Default)]
pub struct RunResult {
    /// Seconds from start to playable; `None` when it hit the cap.
    pub ttp: Option<f64>,
    pub seeks: Vec<Option<f64>>,
    pub rebuffer_s: f64,
    pub played_s: f64,
    /// Contiguous verified seconds ahead of the playhead, once a second of
    /// playback (plans/streaming-window.md phase 0, readahead order).
    pub ahead_s: Vec<f64>,
    pub payload: u64,
    pub dup: u64,
    pub requests: u64,
    pub cancels: u64,
    /// The scheduler's own `Ready` times for the same demands (DWH only).
    pub ready_ttp: Option<f64>,
    pub ready_seeks: Vec<Option<f64>>,
}

struct Sim {
    geo: Geometry,
    sched: Box<dyn Sched>,
    now: u64,
    link: f64,
    stall_per_s: f64,
    peers: Vec<SimPeer>,
    net: BinaryHeap<Reverse<(u64, u64, Msg)>>,
    seq: u64,
    received: Bitfield,
    got: Vec<u16>,
    verified: Bitfield,
    acts: Vec<Action>,
    ready: BTreeMap<DemandId, u64>,
    res: RunResult,
    // player
    phase: Phase,
    demand: DemandId,
    next_demand: DemandId,
    play_pos: u64,
    reader: u64,
    frontier: u64,
    phase_at: u64,
    next_seek_at: u64,
    targets: Vec<u64>,
    /// Demands that reached playback, with their open time.
    ready_ids: Vec<(DemandId, u64)>,
    play_between: u64,
}

#[allow(dead_code)] // tests/sim_smoke.rs
pub fn run(profile: &Profile, piece_len: u32, seed: u64, kind: Kind) -> RunResult {
    run_for(profile, piece_len, seed, kind, PLAY_BETWEEN_SEEKS)
}

/// [`run`] playing `play_between` microseconds before each seek.
pub fn run_for(
    profile: &Profile,
    piece_len: u32,
    seed: u64,
    kind: Kind,
    play_between: u64,
) -> RunResult {
    let geo = Geometry::new(piece_len, &[NFO_LEN, VIDEO_LEN]).expect("valid geometry");
    let cell = seed ^ (u64::from(piece_len) << 32) ^ (profile.name.len() as u64) << 56;
    let mut rng = Rng::new(cell.wrapping_mul(0x2545_F491_4F6C_DD1D));
    let plans = plan_swarm(profile, geo.num_pieces(), &mut rng);
    let targets = (0..SEEKS)
        .map(|_| (rng.range(0.05, 0.95) * VIDEO_LEN as f64) as u64)
        .collect();
    let bitrate = VIDEO_LEN / DURATION_S;
    let sched: Box<dyn Sched> = match kind {
        Kind::Dwh => {
            let mut s = Scheduler::new(geo.clone(), Config::standard());
            s.set_bitrate(VIDEO, bitrate);
            Box::new(s)
        }
        Kind::Rqbit => Box::new(RqbitLike::new(geo.clone(), VIDEO)),
        Kind::Stremio => Box::new(StremioLike::new(geo.clone(), VIDEO)),
    };
    let mut sim = Sim {
        received: Bitfield::new(geo.num_blocks()),
        got: vec![0; geo.num_pieces() as usize],
        verified: Bitfield::new(geo.num_pieces()),
        geo,
        sched,
        now: 0,
        link: profile.link,
        stall_per_s: profile.stall_per_s,
        peers: plans
            .into_iter()
            .map(|plan| SimPeer {
                next_flaky: plan.connect_at + 10 * US,
                plan,
                state: State::Pending,
                choking: true,
                queue: VecDeque::new(),
                serving: None,
                stall_until: 0,
                jitter: 1.0,
                rx: 0,
                rate_est: 0.0,
                unchoked_once: false,
            })
            .collect(),
        net: BinaryHeap::new(),
        seq: 0,
        acts: Vec::new(),
        ready: BTreeMap::new(),
        res: RunResult::default(),
        phase: Phase::Head,
        demand: 0,
        next_demand: 0,
        play_pos: 0,
        reader: 0,
        frontier: 0,
        phase_at: 0,
        next_seek_at: 0,
        targets,
        ready_ids: Vec::new(),
        play_between,
    };
    sim.open(0);
    let bitrate_step = bitrate * STEP / US;
    while sim.phase != Phase::Done && sim.now < HORIZON {
        sim.feed(Event::Tick { now_us: sim.now });
        sim.lifecycle();
        sim.deliver();
        sim.upload();
        sim.player(bitrate, bitrate_step);
        sim.now += STEP;
    }
    let mut res = std::mem::take(&mut sim.res);
    // The playable demand first (timed from 0), then one per seek.
    let since = |d: &DemandId, from: u64| sim.ready.get(d).map(|&t| (t - from) as f64 / US as f64);
    let mut ids = sim.ready_ids.iter();
    if let Some((d, _)) = ids.next() {
        res.ready_ttp = since(d, 0);
    }
    res.ready_seeks = ids.map(|(d, open)| since(d, *open)).collect();
    res
}

impl Sim {
    fn feed(&mut self, ev: Event) {
        self.sched.handle(ev, &mut self.acts);
        for a in std::mem::take(&mut self.acts) {
            match a {
                Action::Request { peer, block } => {
                    self.res.requests += 1;
                    self.send(peer, Msg::Req(peer, block));
                }
                Action::Cancel { peer, block } => {
                    self.res.cancels += 1;
                    self.send(peer, Msg::Cancel(peer, block));
                }
                Action::Ready { demand } => {
                    self.ready.entry(demand).or_insert(self.now);
                }
            }
        }
    }

    /// Queue a message on the wire with the peer's one-way delay.
    fn send(&mut self, peer: PeerId, msg: Msg) {
        self.send_at(self.now, peer, msg);
    }

    fn send_at(&mut self, at: u64, peer: PeerId, msg: Msg) {
        let owd = self.peers[peer as usize].plan.rtt / 2;
        self.seq += 1;
        self.net.push(Reverse((at + owd, self.seq, msg)));
    }

    fn set_choke(&mut self, i: usize, choke: bool) {
        let p = &mut self.peers[i];
        if p.choking == choke {
            return;
        }
        p.choking = choke;
        if choke {
            p.queue.clear();
            p.serving = None;
            self.send(i as u32, Msg::Choke(i as u32));
        } else {
            self.send(i as u32, Msg::Unchoke(i as u32));
        }
    }

    fn lifecycle(&mut self) {
        let second = self.now.is_multiple_of(US);
        for i in 0..self.peers.len() {
            let id = i as u32;
            let p = &mut self.peers[i];
            match p.state {
                State::Pending if self.now >= p.plan.connect_at => {
                    p.state = State::Live;
                    let have = p.plan.have.clone();
                    self.feed(Event::PeerConnected {
                        peer: id,
                        have,
                        reqq: 250,
                    });
                }
                State::Live if self.now >= p.plan.leave_at => {
                    p.state = State::Gone;
                    p.queue.clear();
                    p.serving = None;
                    self.feed(Event::PeerDisconnected { peer: id });
                }
                State::Live => {
                    if !p.unchoked_once && self.now >= p.plan.unchoke_at {
                        p.unchoked_once = true;
                        self.set_choke(i, false);
                    }
                    if !second {
                        continue;
                    }
                    let p = &mut self.peers[i];
                    p.jitter = p.plan.rng.range(0.6, 1.4);
                    if p.stall_until <= self.now && p.plan.rng.chance(self.stall_per_s) {
                        p.stall_until = self.now + secs(p.plan.rng.range(2.0, 8.0));
                    }
                    let mut flip = None;
                    if p.plan.flaky && self.now >= p.next_flaky && self.now > p.plan.unchoke_at {
                        p.next_flaky += 10 * US;
                        flip = Some(p.plan.rng.chance(0.3));
                    }
                    let mut gained = None;
                    if !p.plan.seeder {
                        let n = p.plan.have.len();
                        let piece = (p.plan.rng.next_u64() % u64::from(n)) as u32;
                        if !p.plan.have.get(piece) {
                            p.plan.have.set(piece, true);
                            gained = Some(piece);
                        }
                    }
                    p.rate_est = 0.5 * p.rate_est + 0.5 * p.rx as f64;
                    p.rx = 0;
                    let (bytes_per_s, rtt_us) = (p.rate_est as u64, p.plan.rtt);
                    if let Some(choke) = flip {
                        self.set_choke(i, choke);
                    }
                    if let Some(piece) = gained {
                        self.send(id, Msg::Have(id, piece));
                    }
                    self.feed(Event::PeerRate {
                        peer: id,
                        bytes_per_s,
                        rtt_us,
                    });
                }
                _ => {}
            }
        }
    }

    fn deliver(&mut self) {
        while let Some(&Reverse((at, _, msg))) = self.net.peek() {
            if at > self.now {
                break;
            }
            self.net.pop();
            match msg {
                Msg::Req(i, b) => {
                    let p = &mut self.peers[i as usize];
                    if p.state != State::Live || p.choking {
                        continue;
                    }
                    let busy = p.queue.len() + usize::from(p.serving.is_some());
                    if !p.plan.have.get(self.geo.piece_of(b)) || busy >= 250 {
                        self.send(i, Msg::Reject(i, b));
                    } else {
                        p.queue.push_back(b);
                    }
                }
                Msg::Cancel(i, b) => {
                    let q = &mut self.peers[i as usize].queue;
                    if let Some(k) = q.iter().position(|&x| x == b) {
                        q.remove(k);
                    }
                }
                Msg::Block(i, b) => self.on_block(i, b),
                Msg::Reject(i, b) => {
                    self.deliver_live(i, Event::BlockRejected { peer: i, block: b })
                }
                Msg::Choke(i) => self.deliver_live(i, Event::PeerChoked { peer: i }),
                Msg::Unchoke(i) => self.deliver_live(i, Event::PeerUnchoked { peer: i }),
                Msg::Have(i, piece) => self.deliver_live(i, Event::PeerHave { peer: i, piece }),
            }
        }
    }

    fn deliver_live(&mut self, i: u32, ev: Event) {
        if self.peers[i as usize].state == State::Live {
            self.feed(ev);
        }
    }

    fn on_block(&mut self, i: u32, b: BlockIdx) {
        if self.peers[i as usize].state != State::Live {
            return; // connection is gone
        }
        let len = u64::from(self.geo.block_len(b));
        self.peers[i as usize].rx += len;
        self.res.payload += len;
        let piece = self.geo.piece_of(b);
        if self.received.get(b) {
            self.res.dup += len;
        } else {
            self.received.set(b, true);
            self.got[piece as usize] += 1;
        }
        self.feed(Event::BlockReceived { peer: i, block: b });
        let complete = self.got[piece as usize] as usize == self.geo.piece_blocks(piece).len();
        if complete && !self.verified.get(piece) {
            self.verified.set(piece, true);
            self.feed(Event::PieceVerified { piece });
        }
    }

    /// Fluid transfer for one step, max-min fair over the client downlink.
    fn upload(&mut self) {
        let mut active: Vec<(f64, usize)> = self
            .peers
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.state == State::Live
                    && !p.choking
                    && self.now >= p.stall_until
                    && (p.serving.is_some() || !p.queue.is_empty())
            })
            .map(|(i, p)| (p.plan.rate * p.jitter, i))
            .collect();
        active.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let (mut cap, mut left) = (self.link, active.len());
        let dt = STEP as f64 / US as f64;
        for (want, i) in active {
            let rate = want.min(cap / left as f64);
            cap -= rate;
            left -= 1;
            let mut budget = rate * dt;
            let mut spent = 0.0;
            loop {
                let p = &mut self.peers[i];
                if p.serving.is_none() {
                    let Some(b) = p.queue.pop_front() else { break };
                    p.serving = Some((b, f64::from(self.geo.block_len(b))));
                }
                let (b, rest) = p.serving.expect("serving");
                if rest > budget {
                    p.serving = Some((b, rest - budget));
                    break;
                }
                budget -= rest;
                spent += rest;
                p.serving = None;
                let at = self.now + (spent / rate * US as f64) as u64;
                self.send_at(at, i as u32, Msg::Block(i as u32, b));
            }
        }
    }

    fn video_pieces(&self, start: u64, end: u64) -> std::ops::Range<PieceIdx> {
        let off = self.geo.files()[VIDEO].offset;
        self.geo.pieces_in(off + start, off + end.min(VIDEO_LEN))
    }

    fn servable(&self, start: u64, end: u64) -> bool {
        self.video_pieces(start, end).all(|p| self.verified.get(p))
    }

    fn open(&mut self, start: u64) {
        let old = (self.next_demand > 0).then_some(self.demand);
        self.next_demand += 1;
        self.demand = self.next_demand;
        self.feed(Event::RangeOpened {
            demand: self.demand,
            file: VIDEO,
            start,
            end_hint: None,
        });
        // Players open the new connection, then drop the old one.
        if let Some(d) = old {
            self.feed(Event::RangeClosed { demand: d });
        }
        self.phase_at = self.now;
    }

    fn start_playing(&mut self, pos: u64) {
        self.play_pos = pos;
        self.reader = pos;
        self.frontier = pos;
        self.phase = Phase::Playing;
        self.next_seek_at = self.now + self.play_between;
        self.ready_ids.push((self.demand, self.phase_at));
    }

    fn player(&mut self, bitrate: u64, step_bytes: u64) {
        while self.transition() {}
        if self.phase == Phase::Playing {
            self.play(bitrate, step_bytes);
            while self.transition() {}
        }
        if matches!(self.phase, Phase::Head | Phase::Tail | Phase::HeadAgain)
            && self.now as f64 / US as f64 > TTP_CAP_S
        {
            self.phase = Phase::Done;
        }
    }

    /// Start-up and seek state machine. The player reads the head, then the
    /// container index at the end, then the head again; a seek waits for
    /// `HEAD_BYTES` at the target. Returns true when the phase changed.
    fn transition(&mut self) -> bool {
        let elapsed = (self.now - self.phase_at) as f64 / US as f64;
        match self.phase {
            Phase::Head if self.servable(0, 1) => {
                self.open(VIDEO_LEN - TAIL_BYTES);
                self.phase = Phase::Tail;
            }
            Phase::Tail if self.servable(VIDEO_LEN - TAIL_BYTES, VIDEO_LEN) => {
                self.open(0);
                self.phase = Phase::HeadAgain;
            }
            Phase::HeadAgain if self.servable(0, HEAD_BYTES) => {
                self.res.ttp = Some(self.now as f64 / US as f64);
                self.start_playing(0);
            }
            Phase::Seeking(x) if self.servable(x, x + HEAD_BYTES) => {
                self.res.seeks.push(Some(elapsed));
                self.start_playing(x);
            }
            Phase::Seeking(x) if elapsed > SEEK_CAP_S => {
                self.res.seeks.push(None);
                self.start_playing(x);
                self.next_seek_at = self.now;
            }
            _ => return false,
        }
        true
    }

    fn play(&mut self, bitrate: u64, step_bytes: u64) {
        let off = self.geo.files()[VIDEO].offset;
        while self.frontier < VIDEO_LEN && self.verified.get(self.geo.piece_at(off + self.frontier))
        {
            let piece = self.geo.piece_at(off + self.frontier);
            self.frontier = (u64::from(piece) + 1) * u64::from(self.geo.piece_len()) - off;
        }
        let frontier = self.frontier.min(VIDEO_LEN);
        let want = (self.play_pos + step_bytes).min(VIDEO_LEN);
        if frontier >= want {
            self.play_pos = want;
            self.res.played_s += STEP as f64 / US as f64;
        } else {
            self.res.rebuffer_s += STEP as f64 / US as f64;
        }
        if self.now.is_multiple_of(US) {
            let ahead = frontier.saturating_sub(self.play_pos) as f64 / bitrate as f64;
            self.res.ahead_s.push(ahead);
        }
        let reader = frontier.min(self.play_pos + PLAYER_BUFFER_S * bitrate);
        if reader >= self.reader + 64 * 1024 || (reader > self.reader && reader == frontier) {
            self.reader = reader;
            self.feed(Event::RangeAdvanced {
                demand: self.demand,
                pos: reader,
            });
        }
        if self.now.is_multiple_of(250 * 1000) {
            self.feed(Event::PlayPosition {
                file: VIDEO,
                pos: self.play_pos,
            });
        }
        if self.now >= self.next_seek_at {
            let k = self.res.seeks.len();
            if k == SEEKS {
                self.phase = Phase::Done;
                return;
            }
            let x = self.targets[k];
            self.open(x);
            self.phase = Phase::Seeking(x);
        }
    }
}
