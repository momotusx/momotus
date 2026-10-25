//! Embedded torrent streaming engine: a thin librqbit fork for the protocol
//! (DHT, trackers, PEX, storage, hashing) with the DWH scheduler
//! (`mq-engine-sched`) choosing every block of a streamed torrent. Every
//! peer, in and out, passes the address policy (plans/c27-torrent-policy.md).
//!
//! ```text
//! let engine = Engine::start(EngineConfig::new(dir, http.torrent_scope(&[]))).await?;
//! let t = engine.add_magnet(info_hash, &trackers, &[]).await?;   // metadata
//! let file = engine.select_file(t, "default", &FileHint::Episode { season: 1, episode: 2 }).await?;
//! let reader = engine.open_reader(t, "default", file).await?;
//! let mut range = reader.range(start, None).await?;  // RangeOpened
//! range.ready().await?;                               // blocking window + tail
//! range.read(&mut buf).await?;                        // verified bytes only
//! ```

mod adapter;
mod check;
mod pick;
mod reader;
mod scrape;
#[cfg(feature = "test-hooks")]
pub mod test_hooks;
mod window;

pub use adapter::PeerTally;
pub use check::PieceCheck;
/// The fork itself, for tools and tests (creating a torrent to seed).
pub use librqbit;
pub use pick::FileHint;
pub use reader::{RangeReader, StreamReader};
pub use scrape::{SwarmCount, TrackerTrace};
pub use window::{
    APFS_HEAD_BYTES, Counted, HeldBytes, RESERVE_BYTES, RING_BYTES, WindowInputs, WindowPlan,
    counted_on_disk, counted_on_disk_keeping, free_space, window_plan,
};

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, Destinations,
    DhtSessionConfig, ExtSched, ListenerMode, ListenerOptions, ManagedTorrent, Session,
    SessionOptions, generate_azereus_style,
};
use mq_engine_sched::{Config as SchedConfig, DemandId};
use mq_http::AddressScope;
use tokio::sync::mpsc;

/// Hardware tier: scheduler windows and connection limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceClass {
    /// 1 GB sticks.
    LowRam,
    /// 2 GB sticks and phones.
    Standard,
    /// Shield, Apple TV 4K, desktops.
    HighEnd,
}

impl DeviceClass {
    fn sched(self) -> SchedConfig {
        match self {
            Self::LowRam => SchedConfig::low_ram(),
            Self::Standard => SchedConfig::standard(),
            Self::HighEnd => SchedConfig::high_end(),
        }
    }

    fn peer_limit(self) -> usize {
        match self {
            Self::LowRam => 40,
            Self::Standard => 80,
            Self::HighEnd => 128,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Torrent data (one folder per info hash) and the DHT state.
    pub data_dir: PathBuf,
    /// Allocated bytes on disk across torrents; least recently used idle
    /// torrents are deleted whole when it is exceeded.
    pub cache_budget_bytes: u64,
    /// Inbound TCP and uTP port; `None` picks a free one.
    pub listen_port: Option<u16>,
    /// Serve pieces to other peers.
    pub upload: bool,
    pub device_class: DeviceClass,
    /// DWH chooses every block (the default); `false` keeps librqbit's stock
    /// picker and streaming lookahead, for A/B runs.
    pub dwh: bool,
    /// The DHT. Off only for tests on loopback; trackers run either way.
    pub discovery: bool,
    /// Where the engine may connect: `HttpClient::torrent_scope(&[])`.
    pub policy: AddressScope,
    /// Session rate limits; none by default (a slow seeder in live checks).
    pub limits: librqbit::limits::LimitsConfig,
    /// Files kept for offline download (plans/offline-downloads.md 4.6), by
    /// lowercase info hash and file index: known from the first sweep on.
    pub kept: Vec<(String, usize)>,
}

impl EngineConfig {
    pub fn new(data_dir: impl Into<PathBuf>, policy: AddressScope) -> Self {
        Self {
            data_dir: data_dir.into(),
            cache_budget_bytes: 4 << 30,
            listen_port: None,
            upload: true,
            device_class: DeviceClass::Standard,
            dwh: true,
            discovery: true,
            policy,
            limits: Default::default(),
            kept: Vec::new(),
        }
    }
}

/// Addresses the policy refused, per kind of destination; never which ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Refused {
    pub peers: u64,
    pub trackers: u64,
    pub dht: u64,
}

impl Refused {
    /// What was refused after `earlier` was read.
    pub fn since(self, earlier: Refused) -> Refused {
        Refused {
            peers: self.peers.saturating_sub(earlier.peers),
            trackers: self.trackers.saturating_sub(earlier.trackers),
            dht: self.dht.saturating_sub(earlier.dht),
        }
    }
}

/// The policy once per kind of destination, each counting its own refusals.
struct Scopes {
    peers: AddressScope,
    trackers: AddressScope,
    dht: AddressScope,
}

impl Scopes {
    fn new(policy: &AddressScope) -> Self {
        Self {
            peers: policy.with_own_count(),
            trackers: policy.with_own_count(),
            dht: policy.with_own_count(),
        }
    }
}

/// The checks the fork runs for the session (plans/c27-torrent-policy.md):
/// peers; trackers (every UDP datagram and every HTTP literal at send
/// time, names through the scope's resolver); every DHT datagram.
pub fn destinations(
    peers: &AddressScope,
    trackers: &AddressScope,
    dht: &AddressScope,
) -> Destinations {
    let (peers, tracker_permits, dht) = (peers.clone(), trackers.clone(), dht.clone());
    Destinations {
        peers: Arc::new(move |ip: IpAddr| peers.permits(ip)),
        trackers: librqbit::tracker_comms::Policy {
            permits: Arc::new(move |ip: IpAddr| tracker_permits.permits(ip)),
            resolver: trackers.resolver(),
        },
        dht: Arc::new(move |ip: IpAddr| dht.permits(ip)),
    }
}

/// The session the engine runs: LSD and UPnP off (each talks to addresses
/// before any check could run), TCP and uTP under the peer policy (the
/// vendored uTP drops refused sources before answering, C27 11), trackers
/// on, the DHT only with `discovery`.
fn session_options(cfg: &EngineConfig, scopes: &Scopes) -> SessionOptions {
    let version = env!("CARGO_PKG_VERSION");
    let v: Vec<u8> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    let dht = cfg.discovery.then(|| DhtSessionConfig {
        persistence: Some(DhtPersistenceConfig {
            config_filename: Some(cfg.data_dir.join("dht.json")),
            ..Default::default()
        }),
        ..Default::default()
    });
    SessionOptions {
        dht,
        disable_local_service_discovery: true,
        // Honest: says what we are, never another client's id.
        peer_id: Some(generate_azereus_style(
            *b"MQ",
            (
                v.first().copied().unwrap_or(0),
                v.get(1).copied().unwrap_or(0),
                v.get(2).copied().unwrap_or(0),
                0,
            ),
        )),
        client_name_and_version: Some(format!(
            "Momotus {version} (librqbit {})",
            librqbit::version()
        )),
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpAndUtp,
            listen_addr: (
                std::net::Ipv6Addr::UNSPECIFIED,
                cfg.listen_port.unwrap_or(0),
            )
                .into(),
            enable_upnp_port_forwarding: false,
            ..Default::default()
        }),
        connect: Some(ConnectionOptions::default()),
        peer_limit: Some(cfg.device_class.peer_limit()),
        disable_upload: !cfg.upload,
        ratelimits: cfg.limits,
        destinations: Some(destinations(&scopes.peers, &scopes.trackers, &scopes.dht)),
        tls_roots: mq_http::device_roots(),
        // Fast resume: `<info hash>.have` beside each torrent's folder, so
        // a relaunch hashes a sample of its pieces, not every byte (engine
        // review r3, P2-2); a wrong hash means the full check (r4, P1-1).
        bitv_folder: Some(cfg.data_dir.clone()),
        ..Default::default()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown torrent")]
    UnknownTorrent,
    #[error("bad info hash")]
    BadInfoHash,
    #[error("no file in the torrent matches")]
    NoFile,
    /// Paused for this profile by [`Engine::pause_now`] and not released
    /// since: none of its file choices or ranges wakes it.
    #[error("the torrent is held")]
    Held,
    /// [`Engine::remove`] left it: a playback still claims it.
    #[error("another playback still uses the torrent")]
    Claimed,
    /// Another engine holds `data_dir` (its `.lock`): two engines on one
    /// folder would evict each other's data.
    #[error("the torrent data folder is in use by another engine")]
    DataInUse,
    /// A window would not fit (plans/streaming-window.md 2.1): refused
    /// before any write.
    #[error("this device is full")]
    DeviceFull,
    /// A window or whole file ended its playback before a write could fail
    /// (2.4): "storage full", or "this storage cannot free space while
    /// playing".
    #[error("{0}")]
    Storage(String),
    #[error("torrent: {0:#}")]
    Torrent(#[from] anyhow::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = EngineError> = std::result::Result<T, E>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TorrentHandle(u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub idx: usize,
    /// Path inside the torrent, `/` separated.
    pub path: String,
    pub len: u64,
}

/// One peer of a torrent for [`Engine::peer_report`]: librqbit's view and
/// the scheduler's tally, no address.
#[derive(Clone, Debug)]
pub struct PeerReport {
    pub ipv6: bool,
    /// librqbit's state: live, connecting, queued, dead or not_needed.
    pub state: String,
    pub attempts: u32,
    pub connections: u32,
    pub errors: u32,
    pub fetched_bytes: u64,
    pub client: Option<String>,
    /// `None` when its handshake never reached the scheduler.
    pub tally: Option<PeerTally>,
}

#[derive(Clone, Debug, Default)]
pub struct TorrentStats {
    pub peers: usize,
    pub down_bytes_per_s: u64,
    pub up_bytes_per_s: u64,
    pub pieces_have: u32,
    pub pieces_total: u32,
    pub downloaded_bytes: u64,
    pub metadata_ms: u64,
    /// From `select_file` to the first ready range.
    pub ready_ms: Option<u64>,
    pub dwh: bool,
    /// DWH counters: requests, cancels, hedges, duplicate bytes.
    pub requests: u64,
    pub cancels: u64,
    pub hedges: u64,
    pub dup_bytes: u64,
    /// Bytes of pieces verified again after they had verified and left.
    pub refetched_bytes: u64,
    pub snubs: u64,
    pub seeks: u64,
    /// Ranges open on the torrent, each one scheduler demand.
    pub readers: usize,
    /// The capacity sample: payload bytes received while block requests
    /// were outstanding, and that time, in the first 30 s.
    pub sample_bytes: u64,
    pub sample_busy_ms: u64,
    /// Payload bytes received from peers since the file was chosen,
    /// verified or not: a stalled swarm shows here (`downloaded_bytes`
    /// grows only by whole verified pieces).
    pub received_bytes: u64,
    /// From `select_file` to the first peer unchoking us.
    pub unchoked_ms: Option<u64>,
    /// What the start waits on, for a loading screen: the bytes of the
    /// newest range's blocking window and its file's index in whole pieces
    /// (a resume's head read counts), and of those the verified ones. Both
    /// 0 once a range of the file was Ready, with no range open, or with
    /// the stock picker. While an MP4's index is not known yet
    /// (`start_index_pending`), `start_need` counts the fixed tail and
    /// changes once the head says where the index is, so the two are not
    /// monotone: clamp the fill.
    pub start_need: u64,
    pub start_have: u64,
    pub start_index_pending: bool,
    /// The torrent is paused (idle past its keep-warm minute, or held).
    pub paused: bool,
    /// [`Engine::pause_now`] holds it for some profile: none of that
    /// profile's ranges or file choices wakes it.
    pub held: bool,
    /// The rate the live torrent's scheduler runs with (the last
    /// [`Engine::set_rate`]): bytes per second, and the readahead it gave.
    pub bytes_per_s: Option<u64>,
    pub readahead_bytes: Option<u64>,
    /// The window's budget `W` while one plays ([`Engine::window`]).
    pub window_bytes: Option<u64>,
    /// What the scheduler counts as held: verified pieces and open ones at
    /// full size (a window's only).
    pub held_bytes: Option<u64>,
    /// Eviction rounds the keeper ran for the window.
    pub evict_rounds: u64,
}

/// What this device keeps of a torrent ([`Engine::cached`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cached {
    /// What a start at the asked offset waits on (its blocking window and
    /// the file's index) is verified on disk: Ready needs no peer.
    pub ready: bool,
}

/// A windowed playback (plans/streaming-window.md 2.1): its plan, the
/// file's rate, the network, and the position it resumes at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub plan: WindowPlan,
    pub bitrate: u64,
    pub metered: bool,
    pub resume_at: Option<u64>,
}

const KEEP_WARM: Duration = Duration::from_secs(60);
const MAINTENANCE_EVERY: Duration = Duration::from_secs(5);

/// Public trackers added to every magnet while discovery is on. Addons such
/// as Torrentio often send a bare info hash; with no tracker the engine finds
/// peers through the DHT alone, and on the Mac that missed the 60 s start
/// window on Sintel and Big Buck Bunny (2026-10-19). Long-lived entries of
/// ngosang/trackerslist "best", plus HTTP for networks that drop UDP. Each
/// one still passes the address policy like any tracker.
const DEFAULT_TRACKERS: [&str; 8] = [
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://explodie.org:6969/announce",
    "http://tracker.opentrackr.org:1337/announce",
    "http://tracker2.dler.org:80/announce",
];

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("data_dir", &self.inner.cfg.data_dir)
            .finish_non_exhaustive()
    }
}

struct Inner {
    session: Arc<Session>,
    cfg: EngineConfig,
    scopes: Scopes,
    scraper: scrape::Scraper,
    torrents: Mutex<HashMap<TorrentHandle, Arc<Torrent>>>,
    next: AtomicU64,
    next_demand: AtomicU64,
    /// `data_dir/.lock`, held while this engine lives (engine review r3,
    /// P3-3).
    _lock: std::fs::File,
    /// One eviction round at a time: overlapping rounds each counted what
    /// the other was deleting and evicted too much (engine review r3, P3-2).
    evicting: tokio::sync::Mutex<()>,
    #[cfg(feature = "test-hooks")]
    hold: Mutex<Option<Arc<test_hooks::Hold>>>,
    #[cfg(feature = "test-hooks")]
    evict_hold: Mutex<Option<Arc<test_hooks::Hold>>>,
    /// Free space as the engine reads it, for tests of a full disk.
    #[cfg(feature = "test-hooks")]
    free: Mutex<Option<u64>>,
    /// Free space that follows the data folder: this less what it holds.
    #[cfg(feature = "test-hooks")]
    free_base: Mutex<Option<u64>>,
    /// `R`, `C` and the folders' bytes as maintenance last counted them,
    /// for a ranking that cannot wait (plans/streaming-window.md 3.1).
    counted: Mutex<Option<window::Counted>>,
    /// The data folder cannot punch holes: files are kept as piece files,
    /// whose pieces a ring may keep in RAM (plans/streaming-window-chunks.md).
    chunked: bool,
    /// Files kept for offline download, by lowercase info hash, loaded or
    /// not (plans/offline-downloads.md 4.6).
    kept: Mutex<HashMap<String, std::collections::BTreeSet<usize>>>,
}

/// The session's tasks hang off its token, each torrent's as a child: a
/// dropped engine stops them before its folder lock goes, or a torrent
/// held alive by its tracker stream (an `Arc<Session>`) would write into
/// a folder the next engine owns (engine review r4, P3-3).
impl Drop for Inner {
    fn drop(&mut self) {
        self.session.cancellation_token().cancel();
    }
}

/// `data_dir/.lock` held by another engine is `DataInUse`; a filesystem
/// with no lock support is an I/O error, not another engine (engine
/// review r4, P3-2).
fn lock_error(e: std::fs::TryLockError) -> EngineError {
    match e {
        std::fs::TryLockError::WouldBlock => EngineError::DataInUse,
        std::fs::TryLockError::Error(e) => EngineError::Io(e),
    }
}

pub(crate) struct Torrent {
    info_hash: String,
    torrent_bytes: Vec<u8>,
    /// Trackers its own add announces to beside those `torrent_bytes`
    /// lists: a start from kept details asks this stream's too.
    trackers: Vec<String>,
    files: Vec<FileInfo>,
    piece_len: u32,
    peers: Vec<SocketAddr>,
    metadata_ms: u64,
    dir: PathBuf,
    /// Also the torrent's lock for holds: pausing, releasing and waking
    /// each take it first.
    live: tokio::sync::Mutex<Option<Live>>,
    hold: Mutex<Holds>,
    readers: AtomicUsize,
    last_used: Mutex<Instant>,
    /// Held across an eviction round (plans/streaming-window.md 2.4).
    /// Every path that pauses or removes the torrent takes it after
    /// `live`, and a round never takes `live` while it holds it, so rounds
    /// and pauses never overlap.
    keeper: tokio::sync::Mutex<()>,
    /// The last rate set: file, bytes per second, readahead.
    rate: Mutex<Option<(usize, u64, u64)>>,
    /// The window playing, if any (plans/streaming-window.md 2.1).
    window: Mutex<Option<window::WindowState>>,
    /// The adapter wakes the keeper with it when a round is due.
    keeper_wake: Arc<tokio::sync::Notify>,
    /// A keeper task runs.
    keeping: std::sync::atomic::AtomicBool,
    /// The last window's generation.
    window_generation: AtomicU64,
    /// Why a window ended the playback; ranges fail with it until the next
    /// window or file choice.
    ended: Mutex<Option<String>>,
    /// The player is paused (`Engine::set_paused`).
    player_paused: std::sync::atomic::AtomicBool,
    /// A whole file's readahead stopped under the reserve (2.4).
    starved: std::sync::atomic::AtomicBool,
    /// Rounds the keeper ran.
    rounds: AtomicU64,
    /// Its piece files keep new pieces in RAM: a ring the disk cannot hold
    /// plays (plans/streaming-window-chunks.md).
    memory: Arc<std::sync::atomic::AtomicBool>,
    /// Its files are piece files, so a RAM ring can play it.
    chunked: std::sync::atomic::AtomicBool,
    /// Running downloads of its files, by file (plans/offline-downloads.md
    /// 4.6): no playback claim, so the keeper, the whole-file guard and
    /// `remove` read playbacks alone.
    downloading: Mutex<HashMap<usize, usize>>,
    /// The disk fell under the reserve: its downloads stopped fetching.
    out_of_space: std::sync::atomic::AtomicBool,
}

impl Torrent {
    /// A download of one of its files runs.
    fn downloading(&self) -> bool {
        !lock(&self.downloading).is_empty()
    }

    /// A playback claims it or a download runs: keep-warm and a kids hold
    /// leave it running.
    fn active(&self) -> bool {
        self.claimed() || self.downloading()
    }

    /// Some playback uses it: neither a removal nor the disk budget deletes
    /// it (a paused playback has no range open but still plays it).
    fn claimed(&self) -> bool {
        !lock(&self.hold).claims.is_empty()
    }
}

/// A torrent's holds, one per profile (plans/m11-kids.md 4.4): a kids
/// profile's closure holds it against that profile's file choices and
/// ranges only, and pauses it only while no other profile has a claim.
#[derive(Default)]
struct Holds {
    by_profile: HashMap<String, Hold>,
    /// Live claims by profile; a profile with none has no entry.
    claims: HashMap<String, usize>,
}

impl Holds {
    fn held(&self, profile: &str) -> bool {
        self.by_profile.get(profile).is_some_and(|h| h.held)
    }
}

/// A hold and the authorization that may lift it: two generations that
/// only rise, so a pause read before a newer authorization, or an
/// authorization read before a newer pause, loses.
#[derive(Default)]
struct Hold {
    held: bool,
    hold_gen: u64,
    auth_gen: u64,
}

/// A profile's live use of a torrent: a playback keeps one for as long as
/// its lease lives, so a hold for another profile leaves the torrent
/// running. Clones share it; it ends when the last one drops.
#[derive(Clone)]
pub struct Claim(Arc<ClaimInner>);

struct ClaimInner {
    engine: Engine,
    handle: TorrentHandle,
    torrent: Arc<Torrent>,
    profile: String,
}

impl Claim {
    pub fn handle(&self) -> TorrentHandle {
        self.0.handle
    }

    pub fn profile(&self) -> &str {
        &self.0.profile
    }

    /// A reader for `file`, for this claim's profile.
    pub async fn open_reader(&self, file: usize) -> Result<StreamReader> {
        let c = &self.0;
        c.engine.open_reader(c.handle, &c.profile, file).await
    }
}

impl std::fmt::Debug for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Claim")
            .field("handle", &self.0.handle)
            .field("profile", &self.0.profile)
            .finish()
    }
}

impl Drop for ClaimInner {
    fn drop(&mut self) {
        let mut holds = lock(&self.torrent.hold);
        if let Some(n) = holds.claims.get_mut(&self.profile) {
            *n -= 1;
            if *n == 0 {
                holds.claims.remove(&self.profile);
            }
        }
        if holds.claims.is_empty() {
            self.torrent.keeper_wake.notify_one();
        }
    }
}

pub(crate) struct Live {
    managed: Arc<ManagedTorrent>,
    msgs: mpsc::UnboundedSender<adapter::Msg>,
    snapshot: Arc<Mutex<adapter::Snapshot>>,
    dwh: bool,
    /// Files chosen while live: added to, never swapped, so a second
    /// playback of a pack never takes the first one's file away.
    selected: HashSet<usize>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Engine {
    /// One session for the process: shared DHT (persisted in `data_dir`),
    /// inbound TCP listener, PEX, IPv6 dual stack.
    pub async fn start(cfg: EngineConfig) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let lock_file = std::fs::File::create(cfg.data_dir.join(".lock"))?;
        lock_file.try_lock().map_err(lock_error)?;
        let scopes = Scopes::new(&cfg.policy);
        let opts = session_options(&cfg, &scopes);
        // Where holes do not punch, files are kept as piece files, so a
        // window frees space everywhere (plans/streaming-window-chunks.md).
        let dir = cfg.data_dir.clone();
        // A probe that could not tell (a full disk) keeps whole files.
        let punches = tokio::task::spawn_blocking(move || librqbit::punch_probe(&dir))
            .await
            .unwrap_or(None);
        tracing::info!(?punches, "torrent folder probed");
        let session = Session::new_with_opts(cfg.data_dir.clone(), opts).await?;
        let mut kept: HashMap<String, std::collections::BTreeSet<usize>> = HashMap::new();
        for (hash, file) in &cfg.kept {
            kept.entry(hash.to_ascii_lowercase())
                .or_default()
                .insert(*file);
        }
        let scraper = scrape::Scraper::new(scopes.trackers.clone());
        let engine = Self {
            inner: Arc::new(Inner {
                session,
                cfg,
                scopes,
                scraper,
                torrents: Mutex::default(),
                next: AtomicU64::new(1),
                next_demand: AtomicU64::new(1),
                _lock: lock_file,
                evicting: tokio::sync::Mutex::new(()),
                #[cfg(feature = "test-hooks")]
                hold: Mutex::default(),
                #[cfg(feature = "test-hooks")]
                evict_hold: Mutex::default(),
                #[cfg(feature = "test-hooks")]
                free: Mutex::default(),
                #[cfg(feature = "test-hooks")]
                free_base: Mutex::default(),
                counted: Mutex::default(),
                chunked: punches == Some(false),
                kept: Mutex::new(kept),
            }),
        };
        tokio::spawn(maintenance(Arc::downgrade(&engine.inner)));
        Ok(engine)
    }

    /// Where other peers reach us (tests point a leecher at a seeder with it).
    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.inner.session.listen_addr()
    }

    /// Peer connections this engine dialed and opened so far: (TCP, uTP).
    pub fn connections(&self) -> (u64, u64) {
        let c = self.inner.session.stats_snapshot().connections;
        (
            c.tcp.v4.successes + c.tcp.v6.successes,
            c.utp.v4.successes + c.utp.v6.successes,
        )
    }

    /// The swarms of `torrents` (info hash, the stream's `sources`) as their
    /// trackers count them now, for ranking on live seeders rather than an
    /// addon's number. Each hash's own trackers, plus the default ones when
    /// the DHT is on (as for a bare hash), are scraped at once (BEP 15 UDP,
    /// BEP 48 HTTP) under the trackers' address policy; it returns after
    /// about a second, sooner once every tracker asked has answered or
    /// failed, and an answer later than that still counts for the next
    /// call. A count is the answer with the most seeders, then leechers,
    /// and a later answer never lowers it; `None` when no answer told anything (a default
    /// tracker's zero needs 3 or more leechers: one that does not track a
    /// hash answers 0/0, and one or two may be our own viewers). Counts
    /// stand for five minutes per hash. At most 16 own trackers per hash and
    /// 24 trackers per call.
    pub async fn scrape(&self, torrents: &[(&str, &[String])]) -> Vec<Option<SwarmCount>> {
        let asked: Vec<(String, Vec<String>)> = torrents
            .iter()
            .map(|(hash, sources)| {
                let mut own: Vec<String> = Vec::new();
                for t in sources
                    .iter()
                    .map(|t| t.strip_prefix("tracker:").unwrap_or(t))
                {
                    // The scraper keeps `MAX_OWN`: an addon's 8 MiB answer
                    // can list 400,000 sources (engine review r2, P3-2).
                    if own.len() == scrape::MAX_OWN {
                        break;
                    }
                    if t.contains("://") && !own.iter().any(|x| x == t) {
                        own.push(t.to_owned());
                    }
                }
                (hash.to_ascii_lowercase(), own)
            })
            .collect();
        let defaults: &[&str] = if self.inner.cfg.discovery {
            &DEFAULT_TRACKERS
        } else {
            &[]
        };
        self.inner.scraper.scrape(&asked, defaults).await
    }

    /// Readies the default trackers for a scrape soon after (a title's page
    /// opened before its versions): names resolved and each UDP tracker's
    /// BEP 15 connection id kept for a minute, so that scrape costs one
    /// round trip. At most about 3 s; nothing without discovery.
    pub async fn warm_trackers(&self) {
        if self.inner.cfg.discovery {
            self.inner.scraper.warm(&DEFAULT_TRACKERS).await;
        }
    }

    /// The device's network changed (the apps say so): trackers' kept
    /// addresses and connection ids are dropped, so the next scrape resolves
    /// and connects afresh.
    pub fn network_changed(&self) {
        self.inner.scraper.forget_trackers();
    }

    /// Each tracker's stages in the last scrape that asked any (diagnosis:
    /// `mq scrape`, bench/m13/cold-scrape.jsonl).
    pub fn last_scrape(&self) -> Vec<TrackerTrace> {
        self.inner.scraper.last()
    }

    /// Addresses the policy refused so far, per kind.
    pub fn refused(&self) -> Refused {
        let s = &self.inner.scopes;
        Refused {
            peers: s.peers.refused(),
            trackers: s.trackers.refused(),
            dht: s.dht.refused(),
        }
    }

    /// Resolves metadata for an info hash (40 hex or 32 base32 characters).
    /// `trackers` may carry Stremio `sources` (`tracker:` prefixes are
    /// stripped, `dht:` entries are skipped since the DHT is always on).
    /// A torrent played on this device before is read from the details
    /// kept beside its folder, with no peer (an unreadable or mismatched
    /// file falls back to the peers). Nothing downloads until
    /// [`Engine::select_file`].
    pub async fn add_magnet(
        &self,
        info_hash: &str,
        trackers: &[String],
        peers: &[SocketAddr],
    ) -> Result<TorrentHandle> {
        // Every add reads the interfaces again, as every playback open does.
        self.inner.cfg.policy.refresh();
        let hash = info_hash.trim();
        let hex = hex_hash(hash).ok_or(EngineError::BadInfoHash)?;
        // Known already (a failover or a second view of the same torrent):
        // no second metadata round.
        if let Some((&h, _)) = lock(&self.inner.torrents)
            .iter()
            .find(|(_, t)| t.info_hash == hex)
        {
            return Ok(h);
        }
        let kept = self.inner.cfg.data_dir.join(&hex).with_extension(TORRENT);
        if let Some(bytes) = tokio::task::spawn_blocking(move || kept_torrent(&kept, &hex))
            .await
            .ok()
            .flatten()
        {
            // This stream's trackers, not only the kept ones (review P2-3).
            let fresh = tracker_list(trackers, self.inner.cfg.discovery);
            match self.add(AddTorrent::from_bytes(bytes), peers, fresh).await {
                Ok(h) => return Ok(h),
                Err(e) => tracing::warn!("{hash}: the kept details failed: {e:#}"),
            }
        }
        let magnet = magnet_uri(hash, trackers, self.inner.cfg.discovery);
        self.add(AddTorrent::from_url(magnet), peers, Vec::new())
            .await
    }

    /// Loads `hex` from its kept details alone, for a delete: no metadata
    /// round and no added trackers; `None` with no details kept, so a delete
    /// offline fails fast instead of waiting on the network.
    async fn add_kept(&self, hex: &str) -> Option<TorrentHandle> {
        let path = self.inner.cfg.data_dir.join(hex).with_extension(TORRENT);
        let owned = hex.to_owned();
        let bytes = tokio::task::spawn_blocking(move || kept_torrent(&path, &owned))
            .await
            .ok()
            .flatten()?;
        match self
            .add(AddTorrent::from_bytes(bytes), &[], Vec::new())
            .await
        {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::warn!("{hex}: the kept details failed: {e:#}");
                None
            }
        }
    }

    /// Adds a torrent from `.torrent` bytes. A torrent whose data already
    /// sits in `data_dir/<info hash>/` is verified and seeded once selected.
    pub async fn add_torrent_bytes(
        &self,
        bytes: Vec<u8>,
        peers: &[SocketAddr],
    ) -> Result<TorrentHandle> {
        self.inner.cfg.policy.refresh();
        self.add(AddTorrent::from_bytes(bytes), peers, Vec::new())
            .await
    }

    /// `trackers` go to the torrent's own add beside those its details
    /// list.
    async fn add(
        &self,
        add: AddTorrent<'static>,
        peers: &[SocketAddr],
        trackers: Vec<String>,
    ) -> Result<TorrentHandle> {
        let started = Instant::now();
        let opts = AddTorrentOptions {
            list_only: true,
            initial_peers: Some(peers.to_vec()),
            ..Default::default()
        };
        let AddTorrentResponse::ListOnly(list) =
            self.inner.session.add_torrent(add, Some(opts)).await?
        else {
            return Err(anyhow::anyhow!("metadata resolution did not list the torrent").into());
        };
        let metadata_ms = started.elapsed().as_millis() as u64;
        let files = file_infos(&list.info);
        let info_hash = list.info_hash.as_string();
        let peers = dial_order(&list.seen_peers, peers);
        let torrent = Arc::new(Torrent {
            dir: self.inner.cfg.data_dir.join(&info_hash),
            info_hash,
            torrent_bytes: list.torrent_bytes.to_vec(),
            trackers,
            files,
            piece_len: list.info.lengths().default_piece_length(),
            peers,
            metadata_ms,
            live: tokio::sync::Mutex::new(None),
            hold: Mutex::default(),
            readers: AtomicUsize::new(0),
            last_used: Mutex::new(Instant::now()),
            keeper: tokio::sync::Mutex::new(()),
            rate: Mutex::new(None),
            window: Mutex::new(None),
            keeper_wake: Arc::default(),
            keeping: Default::default(),
            window_generation: AtomicU64::new(0),
            ended: Mutex::new(None),
            player_paused: Default::default(),
            starved: Default::default(),
            rounds: AtomicU64::new(0),
            memory: Arc::default(),
            chunked: std::sync::atomic::AtomicBool::new(false),
            downloading: Mutex::default(),
            out_of_space: Default::default(),
        });
        let mut torrents = lock(&self.inner.torrents);
        if let Some((&h, _)) = torrents
            .iter()
            .find(|(_, t)| t.info_hash == torrent.info_hash)
        {
            return Ok(h);
        }
        let handle = TorrentHandle(self.inner.next.fetch_add(1, Ordering::Relaxed));
        torrents.insert(handle, torrent);
        Ok(handle)
    }

    fn torrent(&self, h: TorrentHandle) -> Result<Arc<Torrent>> {
        lock(&self.inner.torrents)
            .get(&h)
            .cloned()
            .ok_or(EngineError::UnknownTorrent)
    }

    pub fn files(&self, h: TorrentHandle) -> Result<Vec<FileInfo>> {
        Ok(self.torrent(h)?.files.clone())
    }

    /// What this device holds for a start of `file` at byte `offset`, read
    /// from the data folder alone: no torrent starts and nothing goes on
    /// the network. `None` when the torrent's details are neither kept on
    /// disk (played here before) nor loaded in this launch; with them its
    /// add needs no peer.
    pub async fn cached(&self, info_hash: &str, file: usize, offset: u64) -> Option<Cached> {
        let hex = hex_hash(info_hash.trim())?;
        let loaded = lock(&self.inner.torrents)
            .values()
            .find(|t| t.info_hash == hex)
            .cloned();
        // A live torrent's own bitfield: its `.have` claims a piece only
        // at the next flush. One busy starting answers from the file.
        let live_have = loaded.as_ref().and_then(|t| {
            let live = t.live.try_lock().ok()?;
            live.as_ref()?.managed.ext_have_pieces()
        });
        let shape = loaded.map(|t| Shape {
            piece_len: t.piece_len,
            files: t.files.clone(),
        });
        let data_dir = self.inner.cfg.data_dir.clone();
        tokio::task::spawn_blocking(move || match shape {
            Some(shape) => Some(cached_from(
                &data_dir.join(&hex),
                &shape,
                live_have,
                file,
                offset,
            )),
            None => cached_on_disk(&data_dir, &hex, file, offset),
        })
        .await
        .ok()
        .flatten()
    }

    /// The piece hashes over `file`, to check the bytes a reader got.
    pub fn piece_check(&self, h: TorrentHandle, file: usize) -> Result<PieceCheck> {
        let t = self.torrent(h)?;
        let meta = librqbit::torrent_from_bytes(&t.torrent_bytes)
            .map_err(|e| anyhow::anyhow!("torrent bytes: {e}"))?;
        let hashes = meta.info.data.pieces.as_ref().as_chunks::<20>().0.to_vec();
        let len = t.files.get(file).ok_or(EngineError::NoFile)?.len;
        let start = t.files[..file].iter().map(|f| f.len).sum();
        let total = t.files.iter().map(|f| f.len).sum();
        Ok(PieceCheck::new(t.piece_len, start, len, total, hashes))
    }

    /// Picks the file to play for `profile` and starts the torrent on it (or
    /// adds it to the running torrent's wanted files). With DWH nothing
    /// beyond the readahead budget downloads unless a reader asks. `Held`
    /// when `profile` holds it.
    pub async fn select_file(
        &self,
        h: TorrentHandle,
        profile: &str,
        hint: &FileHint,
    ) -> Result<usize> {
        self.select(h, profile, hint, true).await
    }

    /// [`Engine::select_file`]; `budget` false for a delete or a recovery,
    /// which must not delete idle cache to make room for a file it is about
    /// to free or already holds (code review r1, engine P2-4).
    async fn select(
        &self,
        h: TorrentHandle,
        profile: &str,
        hint: &FileHint,
        budget: bool,
    ) -> Result<usize> {
        let t = self.torrent(h)?;
        let idx = pick::pick(&t.files, hint).ok_or(EngineError::NoFile)?;
        // The budget round runs before `live` is held for good: a round
        // holds the round lock while it takes an evicted torrent's `live`
        // (engine review r4, P3-1).
        let starts = t
            .live
            .lock()
            .await
            .as_ref()
            .is_none_or(|l| !l.selected.contains(&idx));
        if starts && budget {
            let need = self.free_for_whole(&t, idx, t.files[idx].len).await;
            self.enforce_budget(Some(h), need).await;
        }
        lock(&t.ended).take();
        let mut live = t.live.lock().await;
        // A round between the two may have evicted it.
        if !lock(&self.inner.torrents).contains_key(&h) {
            return Err(EngineError::UnknownTorrent);
        }
        if lock(&t.hold).held(profile) {
            return Err(EngineError::Held);
        }
        if let Some(l) = live.as_mut() {
            // ponytail: a file stays wanted until the torrent goes; with
            // DWH that costs nothing (only readers' ranges download).
            if !l.selected.contains(&idx) {
                // librqbit refuses a change while it checks the files: a
                // second playback that starts at once waits for that.
                l.managed.wait_until_initialized().await?;
                let mut wanted = l.selected.clone();
                wanted.insert(idx);
                self.inner
                    .session
                    .update_only_files(&l.managed, &wanted)
                    .await?;
                l.selected = wanted;
            }
            return Ok(idx);
        }
        let (ext_tx, ext_rx) = mpsc::unbounded_channel();
        let lens: Vec<u64> = t.files.iter().map(|f| f.len).collect();
        let sched = self.sched_config();
        let iso_bmff = t.files.iter().map(|f| iso_bmff(&f.path)).collect();
        let tr = adapter::Translator::new(t.piece_len, &lens, sched, self.inner.cfg.dwh)
            .with_iso_bmff(iso_bmff);
        let dwh = tr.drives();
        // The details beside the `.have`, so the next start reads them with
        // no peer (start-latency cause 5); they go with the folder.
        keep_torrent(t.dir.with_extension(TORRENT), t.torrent_bytes.clone()).await;
        // Piece files where holes do not punch, and on any volume with no
        // room for a ring on disk, so a RAM ring can play it
        // (plans/streaming-window-chunks.md).
        let chunked = self.inner.chunked
            || self
                .free()
                .is_some_and(|free| free < RESERVE_BYTES + window::RING_BYTES);
        // What the chosen file will be: a whole file already there stays
        // whole where the volume punches (the storage's own rule), and then
        // no RAM ring, so the disk guards stay on (review r2).
        let whole = !self.inner.chunked && t.dir.join(&t.files[idx].path).is_file();
        t.chunked.store(chunked && !whole, Ordering::Relaxed);
        let opts = AddTorrentOptions {
            only_files: Some(vec![idx]),
            overwrite: true,
            output_folder: Some(t.dir.to_string_lossy().into_owned()),
            initial_peers: Some(t.peers.clone()),
            trackers: (!t.trackers.is_empty()).then(|| t.trackers.clone()),
            ext_sched: Some(ExtSched {
                tx: ext_tx,
                drive: dwh,
            }),
            storage_factory: Some(librqbit::storage::StorageFactoryExt::boxed(
                librqbit::storage::filesystem::FilesystemStorageFactory {
                    // `chunked`: this torrent's new files are piece files;
                    // `migrate`: the volume cannot punch, so an old whole
                    // file goes for them.
                    chunked,
                    memory: t.memory.clone(),
                    migrate: self.inner.chunked,
                },
            )),
            ..Default::default()
        };
        let managed = self
            .inner
            .session
            .add_torrent(AddTorrent::from_bytes(t.torrent_bytes.clone()), Some(opts))
            .await?
            .into_handle()
            .ok_or_else(|| anyhow::anyhow!("torrent was not added"))?;
        let (msgs, msg_rx) = mpsc::unbounded_channel();
        // A rate set before the torrent went live.
        if let Some((file, bytes_per_s, readahead)) = *lock(&t.rate) {
            let _ = msgs.send(adapter::Msg::Rate {
                file,
                bytes_per_s,
                readahead,
            });
        }
        let snapshot = Arc::default();
        tokio::spawn(adapter::run(
            managed.clone(),
            tr,
            ext_rx,
            msg_rx,
            Arc::clone(&snapshot),
            t.keeper_wake.clone(),
        ));
        *lock(&t.last_used) = Instant::now();
        // Its kept files before any range: a stream of another file of the
        // pack never counts them as its window's, so its keeper never
        // evicts a finished download (code review r1, engine P1-1).
        let kept: Vec<usize> = self.kept_of(&t).into_iter().collect();
        if !kept.is_empty() {
            let _ = msgs.send(adapter::Msg::Kept(kept));
        }
        *live = Some(Live {
            managed,
            msgs,
            snapshot,
            dwh,
            selected: HashSet::from([idx]),
        });
        Ok(idx)
    }

    /// A reader for one file, for `profile`: its ranges wake the torrent
    /// unless that profile holds it. Selects the file first when nothing
    /// was selected.
    pub async fn open_reader(
        &self,
        h: TorrentHandle,
        profile: &str,
        file: usize,
    ) -> Result<StreamReader> {
        let t = self.torrent(h)?;
        let info = t.files.get(file).cloned().ok_or(EngineError::NoFile)?;
        if t.live.lock().await.is_none() {
            self.select_file(h, profile, &FileHint::Index(file)).await?;
        }
        Ok(StreamReader::new(self.clone(), t, info, profile))
    }

    /// Marks `profile`'s live use of `h` until the claim (every clone of
    /// it) drops.
    pub fn claim(&self, h: TorrentHandle, profile: &str) -> Result<Claim> {
        // Counted under the torrents lock, the one `remove` and the disk
        // budget check claims under before they take a torrent out: a claim
        // never lands on a torrent already on its way out (engine review r1).
        let torrents = lock(&self.inner.torrents);
        let torrent = torrents
            .get(&h)
            .cloned()
            .ok_or(EngineError::UnknownTorrent)?;
        *lock(&torrent.hold)
            .claims
            .entry(profile.to_owned())
            .or_default() += 1;
        drop(torrents);
        Ok(Claim(Arc::new(ClaimInner {
            engine: self.clone(),
            handle: h,
            torrent,
            profile: profile.to_owned(),
        })))
    }

    /// Every peer librqbit knows for `h` and what each did for us, for start
    /// diagnostics: counts only, in no particular order, no address. Empty
    /// before the file is chosen.
    pub async fn peer_report(&self, h: TorrentHandle) -> Result<Vec<PeerReport>> {
        let t = self.torrent(h)?;
        let (managed, msgs) = {
            let live = t.live.lock().await;
            let Some(l) = live.as_ref() else {
                return Ok(Vec::new());
            };
            (l.managed.clone(), l.msgs.clone())
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = msgs.send(adapter::Msg::Report(tx));
        let tallies: HashMap<String, adapter::PeerTally> = rx
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(addr, t)| (addr.to_string(), t))
            .collect();
        let filter = librqbit::PeerStatsFilter {
            state: librqbit::PeerStatsFilterState::All,
        };
        let known = managed
            .live()
            .map(|l| l.per_peer_stats_snapshot(filter).peers)
            .unwrap_or_default();
        Ok(known
            .into_iter()
            .map(|(addr, p)| PeerReport {
                ipv6: addr.starts_with('['),
                state: p.state.to_owned(),
                attempts: p.counters.connection_attempts,
                connections: p.counters.connections,
                errors: p.counters.errors,
                fetched_bytes: p.counters.fetched_bytes,
                client: p.client_name,
                tally: tallies.get(&addr).cloned(),
            })
            .collect())
    }

    pub async fn stats(&self, h: TorrentHandle) -> Result<TorrentStats> {
        let t = self.torrent(h)?;
        let live = t.live.lock().await;
        let mut s = TorrentStats {
            metadata_ms: t.metadata_ms,
            readers: t.readers.load(Ordering::Relaxed),
            held: lock(&t.hold).by_profile.values().any(|h| h.held),
            ..Default::default()
        };
        let Some(l) = live.as_ref() else {
            return Ok(s);
        };
        s.paused = l.managed.is_paused();
        let rq = l.managed.stats();
        let total = l.managed.with_metadata(|m| m.lengths().total_pieces())?;
        s.pieces_total = total;
        s.downloaded_bytes = rq.progress_bytes;
        if let Some(live) = rq.live {
            s.peers = live.snapshot.peer_stats.live as usize;
            s.down_bytes_per_s = (live.download_speed.mbps * 1_048_576.0) as u64;
            s.up_bytes_per_s = (live.upload_speed.mbps * 1_048_576.0) as u64;
        }
        if let Some(bytes) = l.managed.ext_have_pieces() {
            s.pieces_have = bytes.iter().map(|b| b.count_ones()).sum();
        }
        let snap = lock(&l.snapshot).clone();
        s.ready_ms = snap.ready_us.map(|us| us / 1000);
        (s.sample_bytes, s.sample_busy_ms) = (snap.sample_bytes, snap.sample_busy_us / 1000);
        s.received_bytes = snap.received_bytes;
        (s.bytes_per_s, s.readahead_bytes) = (snap.rate.map(|r| r.1), snap.rate.map(|r| r.2));
        s.window_bytes = lock(&t.window).map(|w| w.plan.budget);
        s.held_bytes = snap.held_bytes;
        s.evict_rounds = t.rounds.load(Ordering::Relaxed);
        s.refetched_bytes = snap.refetched_bytes;
        s.unchoked_ms = snap.unchoked_us.map(|us| us / 1000);
        let start = snap.start.unwrap_or_default();
        (s.start_need, s.start_have, s.start_index_pending) =
            (start.need, start.have, start.index_pending);
        s.dwh = l.dwh;
        if let Some(d) = snap.dwh {
            (
                s.requests,
                s.cancels,
                s.hedges,
                s.dup_bytes,
                s.snubs,
                s.seeks,
            ) = (
                d.requests,
                d.cancels,
                d.hedges,
                d.dup_bytes,
                d.snubs,
                d.seeks,
            );
        }
        Ok(s)
    }

    /// Stops the torrent; `keep_data: false` also deletes its files. A
    /// torrent some playback still claims stays as it is (`Claimed`): an Up
    /// Next open that gave up on a pack must not take the episode that is
    /// playing from it. A playback drops its own claim before removing.
    pub async fn remove(&self, h: TorrentHandle, keep_data: bool) -> Result<()> {
        let t = {
            let mut torrents = lock(&self.inner.torrents);
            let t = torrents.get(&h).ok_or(EngineError::UnknownTorrent)?;
            if t.claimed() {
                return Err(EngineError::Claimed);
            }
            torrents.remove(&h).ok_or(EngineError::UnknownTorrent)?
        };
        self.drop_torrent(&t, keep_data).await
    }

    async fn drop_torrent(&self, t: &Torrent, keep_data: bool) -> Result<()> {
        // Data that goes is set aside first: the bitfield the session drops
        // then finds no file to sync, so a removal or an eviction makes the
        // disk write nothing it deletes a moment later (engine review r5,
        // P3-1). A crash in between leaves a folder the next round deletes.
        let gone = if keep_data {
            None
        } else {
            let _ = tokio::fs::remove_file(t.dir.with_extension(HAVE)).await;
            let _ = tokio::fs::remove_file(t.dir.with_extension(TORRENT)).await;
            let trash = t.dir.with_extension(SET_ASIDE);
            // One an interrupted round left (only ever ours).
            let _ = tokio::fs::remove_dir_all(&trash).await;
            let aside = tokio::fs::rename(&t.dir, &trash).await.is_ok();
            Some(if aside { trash } else { t.dir.clone() })
        };
        let mut live = t.live.lock().await;
        let _round = t.keeper.lock().await;
        if let Some(l) = live.take() {
            self.inner
                .session
                .delete(l.managed.id().into(), false)
                .await?;
        }
        if let Some(dir) = gone {
            match tokio::fs::remove_dir_all(&dir).await {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(())
    }

    pub(crate) fn next_demand(&self) -> DemandId {
        self.inner.next_demand.fetch_add(1, Ordering::Relaxed)
    }

    /// Holds `h` for `profile`: none of its range reads or file choices
    /// wakes it until [`Engine::release_hold`] passes a newer authorization
    /// for it. The torrent pauses at once unless another profile has a
    /// claim on it, whose playback goes on. The `generation` is the access
    /// generation the denial was read at; a pause at or below the profile's
    /// newest authorization is ignored (`false`).
    pub async fn pause_now(
        &self,
        h: TorrentHandle,
        profile: &str,
        generation: u64,
    ) -> Result<bool> {
        let t = self.torrent(h)?;
        let live = t.live.lock().await;
        let shared = {
            let mut holds = lock(&t.hold);
            let hold = holds.by_profile.entry(profile.to_owned()).or_default();
            if generation <= hold.auth_gen {
                return Ok(false);
            }
            hold.hold_gen = hold.hold_gen.max(generation);
            hold.held = true;
            holds.claims.keys().any(|p| p != profile)
        } || t.downloading();
        if shared {
            return Ok(true);
        }
        if let Some(l) = live.as_ref().filter(|l| !l.managed.is_paused()) {
            let _round = t.keeper.lock().await;
            self.inner.session.pause(&l.managed).await?;
            let _ = l.msgs.send(adapter::Msg::Stopped);
        }
        Ok(true)
    }

    /// The device's scheduler settings, its readahead within a quarter of
    /// the disk budget.
    fn sched_config(&self) -> SchedConfig {
        let mut sched = self.inner.cfg.device_class.sched();
        sched.readahead_bytes = sched
            .readahead_bytes
            .min(self.inner.cfg.cache_budget_bytes / 4);
        sched
    }

    /// `file` of `h` plays at `bytes_per_s` (its size over the player's
    /// duration, the meta runtime, or a guess from its resolution): the
    /// scheduler's seconds become seconds of this file, and off a metered
    /// network it keeps 120 s of it ahead (plans/streaming-window.md 2.2).
    pub async fn set_rate(
        &self,
        h: TorrentHandle,
        file: usize,
        bytes_per_s: u64,
        metered: bool,
    ) -> Result<()> {
        let t = self.torrent(h)?;
        let sched = self.sched_config();
        let windowed = {
            let mut window = lock(&t.window);
            // The window keeps the rate and network from now on: a budget
            // change or the trim sends them, never the plan's guess.
            if let Some(w) = window.as_mut().filter(|w| w.file == file) {
                (w.bitrate, w.metered) = (bytes_per_s, metered);
            }
            (*window).filter(|w| w.file == file)
        };
        let readahead = match windowed {
            // A window keeps the readahead its plan gave (2.2).
            Some(w) => w.ahead(sched.readahead_bytes),
            None => readahead_for(
                sched.readahead_bytes,
                sched.playback_secs,
                bytes_per_s,
                metered,
                self.inner.cfg.cache_budget_bytes,
            ),
        };
        *lock(&t.rate) = Some((file, bytes_per_s, readahead));
        // A whole file starved under the reserve keeps no readahead until
        // the guard lets it go (2.4, review r2 B P3-1).
        let starved = windowed.is_none() && t.starved.load(Ordering::Relaxed);
        if let Some(l) = t.live.lock().await.as_ref() {
            let _ = l.msgs.send(adapter::Msg::Rate {
                file,
                bytes_per_s,
                readahead: if starved { 0 } else { readahead },
            });
        }
        Ok(())
    }

    /// The device's network is now metered (mobile data) or not: every
    /// playing torrent's readahead follows at the rate it plays, a window's
    /// and a whole file's alike (plans/streaming-window.md 2.2).
    pub async fn set_metered(&self, metered: bool) {
        let rates: Vec<(TorrentHandle, usize, u64)> = lock(&self.inner.torrents)
            .iter()
            .filter_map(|(h, t)| lock(&t.rate).map(|(file, rate, _)| (*h, file, rate)))
            .collect();
        for (h, file, rate) in rates {
            let _ = self.set_rate(h, file, rate, metered).await;
        }
    }

    /// Free space on the data folder's volume.
    pub fn free(&self) -> Option<u64> {
        #[cfg(feature = "test-hooks")]
        if let Some(free) = *lock(&self.inner.free) {
            return Some(free);
        }
        #[cfg(feature = "test-hooks")]
        if let Some(base) = *lock(&self.inner.free_base) {
            return Some(base.saturating_sub(allocated(&self.inner.cfg.data_dir)));
        }
        free_space(&self.inner.cfg.data_dir)
    }

    /// The blocking window's bytes: a range that ends within it from its
    /// start is a probe (plans/streaming-window.md 2.3).
    pub fn blocking_bytes(&self) -> u64 {
        self.sched_config().blocking_bytes
    }

    /// The scheduler presets this engine runs with, for [`window_plan`].
    pub fn device_class(&self) -> DeviceClass {
        self.inner.cfg.device_class
    }

    /// The piece length of `h`, for [`WindowInputs`].
    pub fn piece_len(&self, h: TorrentHandle) -> Result<u32> {
        Ok(self.torrent(h)?.piece_len)
    }

    /// `R` (plans/streaming-window.md 2.1): what torrents with no reader,
    /// claim or hold, and folders an earlier launch left, hold on disk;
    /// with `own`, also what that torrent holds (a window counts its own
    /// folder: what a resume finds there is its own).
    pub async fn reclaimable(&self, own: Option<TorrentHandle>) -> u64 {
        // The own folder less its kept files: no keeper frees a download
        // (code review r1, engine P2-2).
        let idle: Vec<(PathBuf, Vec<PathBuf>)> = lock(&self.inner.torrents)
            .iter()
            .filter(|(h, t)| {
                Some(**h) == own
                    || (t.readers.load(Ordering::Relaxed) == 0
                        && !self.busy(t)
                        && !lock(&t.hold).by_profile.values().any(|h| h.held))
            })
            .map(|(_, t)| (t.dir.clone(), self.kept_paths(t)))
            .collect();
        let held = self.unreclaimable_dirs();
        let data_dir = self.inner.cfg.data_dir.clone();
        tokio::task::spawn_blocking(move || {
            let held: Vec<&Path> = held.iter().map(PathBuf::as_path).collect();
            let left: u64 = leftovers(&data_dir, &held).0.iter().map(|l| l.2).sum();
            left + idle
                .iter()
                .map(|(d, kept)| {
                    allocated(d).saturating_sub(kept.iter().map(|p| file_allocated(p)).sum())
                })
                .sum::<u64>()
        })
        .await
        .unwrap_or(0)
    }

    /// `R`, `C` and the folders' bytes as maintenance last counted them
    /// (every few seconds); `None` before its first count.
    pub fn counted(&self) -> Option<window::Counted> {
        lock(&self.inner.counted).clone()
    }

    /// Each torrent folder's bytes, by info hash: the torrents of this
    /// launch with each file's, and the folders an earlier one left.
    async fn held_bytes(&self) -> HashMap<String, HeldBytes> {
        let all: Vec<(TorrentHandle, Arc<Torrent>)> = lock(&self.inner.torrents)
            .iter()
            .map(|(h, t)| (*h, t.clone()))
            .collect();
        // Hash, folder, files, kept files, idle, committed.
        type Loaded = (String, PathBuf, Vec<PathBuf>, Vec<PathBuf>, bool, u64);
        let mut loaded: Vec<Loaded> = Vec::new();
        for (h, t) in all {
            let idle = t.readers.load(Ordering::Relaxed) == 0
                && !self.busy(&t)
                && !lock(&t.hold).by_profile.values().any(|h| h.held);
            let files = t.files.iter().map(|f| t.dir.join(&f.path)).collect();
            // Its own window's part of `C`: the ranking leaves it out for
            // the row of the title playing, as the app does (review r3 A R7).
            let committed =
                (self.committed(None).await).saturating_sub(self.committed(Some(h)).await);
            loaded.push((
                t.info_hash.to_ascii_lowercase(),
                t.dir.clone(),
                files,
                self.kept_paths(&t),
                idle,
                committed,
            ));
        }
        let data_dir = self.inner.cfg.data_dir.clone();
        let kept = self.kept_unloaded();
        tokio::task::spawn_blocking(move || {
            let dirs: Vec<&Path> = loaded.iter().map(|l| l.1.as_path()).collect();
            let mut held = counted_on_disk_except(&data_dir, &dirs, &kept);
            for (hash, dir, files, kept, idle, committed) in &loaded {
                let files = files.iter().map(|f| file_allocated(f)).collect();
                // Kept files are no part of what the folder can free.
                let kept: u64 = kept.iter().map(|f| file_allocated(f)).sum();
                let folder = allocated(dir).saturating_sub(kept);
                held.insert(
                    hash.clone(),
                    HeldBytes {
                        folder,
                        files,
                        idle: *idle,
                        committed: *committed,
                    },
                );
            }
            held
        })
        .await
        .unwrap_or_default()
    }

    /// What a window of `h` is sized with (2.1), the one source for the
    /// app and the CLI: `free` (the caller's, as its ranking read it), `R`
    /// with `h`'s own folder counted, `C` of the other windows, and `h`'s
    /// pieces.
    pub async fn window_inputs(
        &self,
        h: TorrentHandle,
        file: usize,
        free: u64,
        bitrate: u64,
        fill: u64,
    ) -> Result<WindowInputs> {
        let t = self.torrent(h)?;
        let path = t.files.get(file).ok_or(EngineError::NoFile)?.path.clone();
        let dir = t.dir.clone();
        // A kept file's bytes are not the window's (plans/offline-downloads.md
        // 4.6).
        let kept: Vec<PathBuf> = self
            .kept_of(&t)
            .iter()
            .filter(|&&f| f != file)
            .filter_map(|&f| t.files.get(f).map(|f| dir.join(&f.path)))
            .collect();
        let (own, own_file) = tokio::task::spawn_blocking(move || {
            let kept: u64 = kept.iter().map(|p| file_allocated(p)).sum();
            (
                allocated(&dir).saturating_sub(kept),
                file_allocated(&dir.join(path)),
            )
        })
        .await
        .unwrap_or((0, 0));
        Ok(WindowInputs {
            free,
            reclaimable: self.reclaimable(Some(h)).await,
            committed: self.committed(Some(h)).await,
            bitrate,
            piece_len: u64::from(t.piece_len),
            fill,
            own,
            own_file,
            // Verifying the file's first and last pieces writes its
            // neighbours' bytes too (review r3 A R4).
            boundary: if t.files.len() > 1 {
                2 * u64::from(t.piece_len)
            } else {
                0
            },
        })
    }

    /// `C` (2.1): what the windows playing, but `except`'s, will still take
    /// (`W` less what each holds), and what running downloads will still
    /// write, `except`'s too (plans/offline-downloads.md 4.9).
    pub async fn committed(&self, except: Option<TorrentHandle>) -> u64 {
        let all: Vec<(TorrentHandle, Arc<Torrent>)> = lock(&self.inner.torrents)
            .iter()
            .map(|(h, t)| (*h, t.clone()))
            .collect();
        let mut sum = 0;
        for (h, t) in all {
            let files: Vec<usize> = lock(&t.downloading).keys().copied().collect();
            for file in files {
                let have = self
                    .ask(&t, |tx| adapter::Msg::FileHave { file, tx })
                    .await
                    .unwrap_or(0);
                sum += t.files[file].len.saturating_sub(have);
            }
            let Some(w) = *lock(&t.window) else {
                continue;
            };
            if Some(h) == except {
                continue;
            }
            let held = t
                .live
                .lock()
                .await
                .as_ref()
                .map_or(0, |l| lock(&l.snapshot).held_bytes.unwrap_or(0));
            sum += w.plan.budget.saturating_sub(held);
        }
        sum
    }

    /// Plays `file` of `h` in a window (plans/streaming-window.md 2.1): a
    /// budget round first deletes idle torrents, oldest first, until free
    /// space is `W - held + reserve`; then the scheduler keeps the torrent
    /// to the plan and a keeper task evicts what it may, until the last
    /// claim drops and the keeper trims it. The file is selected first
    /// (`NoFile` otherwise), and before its reader opens.
    pub async fn window(&self, h: TorrentHandle, file: usize, w: Window) -> Result<()> {
        let t = self.torrent(h)?;
        if t.live.lock().await.is_none() {
            return Err(EngineError::NoFile);
        }
        // A file kept for download, or downloading, is whole on disk: no
        // window, cap or keeper (plans/offline-downloads.md 4.6).
        if self.kept_of(&t).contains(&file) || lock(&t.downloading).contains_key(&file) {
            return self.play_whole(h, file).await;
        }
        // A ring the disk cannot hold keeps its pieces in RAM, where they are
        // piece files (plans/streaming-window-chunks.md); it needs no disk.
        // Never on a torrent with a download: the switch is the torrent's,
        // and a download's pieces must reach the disk.
        let memory = w.plan.memory
            && t.chunked.load(Ordering::Relaxed)
            && self.kept_of(&t).is_empty()
            && !t.downloading();
        t.memory.store(memory, Ordering::Relaxed);
        if !memory {
            let dir = t.dir.clone();
            let held = tokio::task::spawn_blocking(move || allocated(&dir))
                .await
                .unwrap_or(0);
            let need = (w.plan.budget + RESERVE_BYTES).saturating_sub(held);
            self.enforce_budget(Some(h), need).await;
        }
        lock(&t.ended).take();
        let state = window::WindowState {
            file,
            plan: w.plan,
            bitrate: w.bitrate,
            metered: w.metered,
            shortfall: 0,
            generation: t.window_generation.fetch_add(1, Ordering::Relaxed) + 1,
            claimed: t.claimed(),
        };
        let sched = self.sched_config();
        *lock(&t.rate) = Some((file, w.bitrate, state.ahead(sched.readahead_bytes)));
        self.put_window(&t, Some(state), w.resume_at, None).await;
        if !t.keeping.swap(true, Ordering::AcqRel) {
            tokio::spawn(window::keep(Arc::downgrade(&self.inner), h, t.clone()));
        }
        Ok(())
    }

    /// The player paused or plays again (`report_progress`): a paused
    /// window's anchor keeps its readahead, and maintenance does not pause
    /// the torrent (plans/streaming-window.md 2.3, 2.4).
    pub async fn set_paused(&self, h: TorrentHandle, paused: bool) -> Result<()> {
        let t = self.torrent(h)?;
        t.player_paused.store(paused, Ordering::Relaxed);
        if let Some(l) = t.live.lock().await.as_ref() {
            let _ = l.msgs.send(adapter::Msg::Paused(paused));
        }
        Ok(())
    }

    /// `file` of `h` plays whole: a window on another of its files (the
    /// last episode of a pack) ends, its cap with it.
    pub async fn play_whole(&self, h: TorrentHandle, file: usize) -> Result<()> {
        let t = self.torrent(h)?;
        t.memory.store(false, Ordering::Relaxed);
        let other = (*lock(&t.window)).filter(|w| w.file != file);
        if let Some(w) = other {
            self.put_window(&t, None, None, Some(w.generation)).await;
        }
        Ok(())
    }

    /// Makes `state` (or none) the torrent's window and sends it to the
    /// adapter's scheduler, both under the `live` lock so a trim and a new
    /// window never cross; with `only`, only while the window is still that
    /// generation. Whether it was put.
    async fn put_window(
        &self,
        t: &Torrent,
        state: Option<window::WindowState>,
        resume_at: Option<u64>,
        only: Option<u64>,
    ) -> bool {
        let sched = self.sched_config();
        let floor = |bitrate| {
            readahead_for(
                sched.readahead_bytes,
                sched.playback_secs,
                bitrate,
                false,
                self.inner.cfg.cache_budget_bytes,
            )
        };
        let msg = adapter::Msg::Window {
            window: state.map(|s| s.sched(floor(s.bitrate))),
            readahead: state.map_or(0, |s| s.ahead(sched.readahead_bytes)),
            bitrate: state.map_or(0, |s| s.bitrate),
            resume_at,
        };
        let live = t.live.lock().await;
        {
            let mut window = lock(&t.window);
            if only.is_some() && window.map(|w| w.generation) != only {
                return false;
            }
            *window = state;
        }
        if let Some(l) = live.as_ref() {
            let _ = l.msgs.send(msg);
        }
        true
    }

    /// The space guard's shortfall for `state`'s window (2.4): its budget
    /// and readahead change, its player read and rate stay.
    pub(crate) async fn set_shortfall(&self, t: &Torrent, generation: u64, shortfall: u64) {
        let preset = self.sched_config().readahead_bytes;
        let live = t.live.lock().await;
        // The stored state, so a rate `set_rate` wrote meanwhile stays.
        let state = {
            let mut window = lock(&t.window);
            let Some(w) = window.as_mut().filter(|w| w.generation == generation) else {
                return;
            };
            w.shortfall = shortfall;
            *w
        };
        if let Some(l) = live.as_ref() {
            let _ = l.msgs.send(adapter::Msg::Budget {
                budget: state.budget(),
                readahead: state.ahead(preset),
            });
        }
    }

    /// A question to the torrent's adapter; `None` when it is not live.
    async fn ask<T>(
        &self,
        t: &Torrent,
        msg: impl FnOnce(tokio::sync::oneshot::Sender<T>) -> adapter::Msg,
    ) -> Option<T> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        t.live.lock().await.as_ref()?.msgs.send(msg(tx)).ok()?;
        rx.await.ok()
    }

    /// The last claim dropped (2.4): the torrent keeps its pinned pieces and
    /// `[position - B, position + 180 s × r]`, within the engine's budget,
    /// and the window ends; what stays counts in `R`.
    async fn trim(&self, h: TorrentHandle, t: &Torrent, w: window::WindowState) {
        // A RAM ring leaves the session instead, so no RAM is held between
        // playbacks; the next file choice starts it again
        // (plans/streaming-window-chunks.md). A playback claims before it
        // chooses a file, so one that came meanwhile keeps the torrent; a
        // reader with no claim is a closed playback's.
        if t.memory.load(Ordering::Relaxed) {
            let mut live = t.live.lock().await;
            if !t.claimed() {
                let _round = t.keeper.lock().await;
                t.memory.store(false, Ordering::Relaxed);
                if let Some(l) = live.take()
                    && let Err(e) = self
                        .inner
                        .session
                        .delete(l.managed.id().into(), false)
                        .await
                {
                    tracing::warn!("unloading {}: {e:#}", t.info_hash);
                }
            }
            drop(live);
            self.put_window(t, None, None, Some(w.generation)).await;
            return;
        }
        let pinned = 4 * u64::from(t.piece_len);
        let room = self
            .inner
            .cfg
            .cache_budget_bytes
            .saturating_sub(w.plan.behind + pinned);
        let ahead = (180 * w.bitrate).min(room);
        if let Some(pieces) = self.ask(t, |tx| adapter::Msg::Trim { ahead, tx }).await
            && !pieces.is_empty()
            && let Err(e) = self.evict_pieces(h, &pieces).await
        {
            tracing::warn!("trimming {}: {e}", t.info_hash);
        }
        // A window set meanwhile (the same file opened again) stays.
        self.put_window(t, None, None, Some(w.generation)).await;
    }

    /// Ends a windowed playback before a write fails (2.4): fetching stops,
    /// and every range fails with `why` until the next window or file
    /// choice.
    async fn end_window(&self, t: &Torrent, why: &str, generation: Option<u64>) {
        tracing::warn!("{}: {why}", t.info_hash);
        *lock(&t.ended) = Some(why.to_owned());
        // The scheduler's window and cap end too: the same file played again
        // after space was freed may fit (review P2-5). A window set since
        // (a reopen) is not this one's to end.
        self.put_window(t, None, None, generation).await;
        let live = t.live.lock().await;
        if let Some(l) = live.as_ref() {
            if !l.managed.is_paused() {
                let _round = t.keeper.lock().await;
                if let Err(e) = self.inner.session.pause(&l.managed).await {
                    tracing::warn!("pausing a full torrent: {e:#}");
                }
                let _ = l.msgs.send(adapter::Msg::Stopped);
            }
            l.managed.ext_wake_streams();
        }
    }

    /// The free space a whole file needs before it starts (2.1): when it
    /// fits only by counting `R`, `size - held + reserve`; else none (it
    /// fits, or it plays in a window).
    async fn free_for_whole(&self, t: &Torrent, file: usize, size: u64) -> u64 {
        // The file's own bytes only: no keeper frees the torrent's other
        // files (a pack's earlier episodes) for a whole file.
        let path = t.dir.join(&t.files[file].path);
        let held = tokio::task::spawn_blocking(move || file_allocated(&path))
            .await
            .unwrap_or(0);
        let Some(free) = self.free() else {
            return 0;
        };
        let need = (size + RESERVE_BYTES).saturating_sub(held);
        if need <= free || need > free + self.reclaimable(None).await {
            return 0;
        }
        need
    }

    /// One eviction round over `pieces` of `h` (plans/streaming-window.md
    /// 2.5): verified pieces and partly written ones leave the disk, busy
    /// ones are skipped. A torrent that is not live skips them all.
    pub async fn evict_pieces(
        &self,
        h: TorrentHandle,
        pieces: &[u32],
    ) -> Result<librqbit::Evicted> {
        let t = self.torrent(h)?;
        let managed = t.live.lock().await.as_ref().map(|l| l.managed.clone());
        let Some(managed) = managed else {
            return Ok(librqbit::Evicted {
                skipped: pieces.to_vec(),
                ..Default::default()
            });
        };
        let _round = t.keeper.lock().await;
        Ok(managed.evict_pieces(pieces).await?)
    }

    /// Lifts `profile`'s hold of `h` for an authorization read at
    /// `generation`, only when that is newer than the hold (`false`
    /// otherwise). The torrent wakes with its next range.
    pub async fn release_hold(
        &self,
        h: TorrentHandle,
        profile: &str,
        generation: u64,
    ) -> Result<bool> {
        let t = self.torrent(h)?;
        let _live = t.live.lock().await;
        let mut holds = lock(&t.hold);
        let hold = holds.by_profile.entry(profile.to_owned()).or_default();
        if generation <= hold.hold_gen {
            return Ok(false);
        }
        hold.auth_gen = hold.auth_gen.max(generation);
        hold.held = false;
        Ok(true)
    }

    /// Unpauses a torrent for a range of `profile`; one that profile holds
    /// stays paused.
    pub(crate) async fn wake(
        &self,
        t: &Torrent,
        profile: &str,
    ) -> Result<Option<(Arc<ManagedTorrent>, mpsc::UnboundedSender<adapter::Msg>)>> {
        let live = t.live.lock().await;
        if lock(&t.hold).held(profile) {
            return Err(EngineError::Held);
        }
        let Some(l) = live.as_ref() else {
            return Ok(None);
        };
        if l.managed.is_paused() {
            self.inner.session.unpause(&l.managed).await?;
        }
        Ok(Some((l.managed.clone(), l.msgs.clone())))
    }

    /// Deletes whole idle torrents, least recently used first, until the
    /// allocated size fits the budget and the volume has `need_free` bytes
    /// free (a window's or a whole file's round, plans/streaming-window.md
    /// 2.1; 0 for none). `keep`, a torrent with a range open and a claimed
    /// one are never deleted. Folders an earlier launch left count too and
    /// go first, the oldest first: they were used before anything this
    /// launch (engine review r2, P2-4).
    async fn enforce_budget(&self, keep: Option<TorrentHandle>, need_free: u64) {
        let _round = self.inner.evicting.lock().await;
        let all: Vec<(TorrentHandle, Arc<Torrent>)> = lock(&self.inner.torrents)
            .iter()
            .map(|(h, t)| (*h, t.clone()))
            .collect();
        // Sizes, dates and the folder listing walk the disk: off the
        // runtime workers, which the app has two of (engine review r3,
        // P3-2).
        // Kept torrents are no cache: out of the scan and the total, so a
        // season kept over the budget never makes every round delete the
        // whole idle cache (plans/offline-downloads.md 4.6, engine r2 P1-1).
        // Only kept ones: a torrent playing still counts in the total, and
        // is skipped below (code review r1, engine P2-1).
        let dirs: Vec<(Instant, TorrentHandle, PathBuf)> = all
            .iter()
            .filter(|(_, t)| self.kept_of(t).is_empty())
            .map(|(h, t)| (*lock(&t.last_used), *h, t.dir.clone()))
            .collect();
        let held_dirs = self.unreclaimable_dirs();
        let data_dir = self.inner.cfg.data_dir.clone();
        let scan = tokio::task::spawn_blocking(move || {
            let held: Vec<&Path> = held_dirs.iter().map(PathBuf::as_path).collect();
            let (left, trash) = leftovers(&data_dir, &held);
            let used: Vec<(Instant, TorrentHandle, u64)> = dirs
                .iter()
                .map(|(at, h, dir)| (*at, *h, allocated(dir)))
                .collect();
            (used, left, trash)
        });
        let Ok((mut used, left, trash)) = scan.await else {
            return;
        };
        let free = if need_free > 0 {
            self.free().unwrap_or(u64::MAX)
        } else {
            u64::MAX
        };
        let mut freed = 0u64;
        let budget = self.inner.cfg.cache_budget_bytes;
        let done =
            |total: u64, freed: u64| total <= budget && free.saturating_add(freed) >= need_free;
        for dir in trash {
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
        let mut total: u64 = used.iter().map(|u| u.2).sum();
        total += left.iter().map(|l| l.2).sum::<u64>();
        for (_, dir, bytes) in left {
            if done(total, freed) {
                break;
            }
            if let Some(trash) = self.set_aside(&dir) {
                let _ = tokio::fs::remove_dir_all(&trash).await;
                total -= bytes;
                freed += bytes;
            }
        }
        used.sort_unstable_by_key(|u| u.0);
        for (_, h, bytes) in used {
            if done(total, freed) {
                break;
            }
            let Some(t) = all.iter().find(|(x, _)| *x == h).map(|(_, t)| t) else {
                continue;
            };
            if Some(h) == keep
                || t.readers.load(Ordering::Relaxed) > 0
                || self.busy(t)
                || bytes == 0
            {
                continue;
            }
            #[cfg(feature = "test-hooks")]
            test_hooks::wait_eviction(self).await;
            {
                // Again under the lock a claim counts under: one may have
                // come since the check above (engine review r1).
                let mut torrents = lock(&self.inner.torrents);
                if t.readers.load(Ordering::Relaxed) > 0 || self.busy(t) {
                    continue;
                }
                torrents.remove(&h);
            }
            match self.drop_torrent(t, false).await {
                Ok(()) => {
                    total -= bytes;
                    freed += bytes;
                }
                Err(e) => tracing::warn!("evicting {}: {e}", t.info_hash),
            }
        }
    }

    // ---- offline downloads (plans/offline-downloads.md 4.6) ----

    /// A playback claims it, a download runs, or a file of it is kept: no
    /// sweep or budget round deletes it, and it is not reclaimable.
    fn busy(&self, t: &Torrent) -> bool {
        t.active() || !self.kept_of(t).is_empty()
    }

    /// The files of `t` kept for download.
    fn kept_of(&self, t: &Torrent) -> std::collections::BTreeSet<usize> {
        lock(&self.inner.kept)
            .get(&t.info_hash.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    /// Where `t`'s kept files are on disk.
    fn kept_paths(&self, t: &Torrent) -> Vec<PathBuf> {
        self.kept_of(t)
            .iter()
            .filter_map(|&f| t.files.get(f).map(|f| t.dir.join(&f.path)))
            .collect()
    }

    /// Selects `files` of a live torrent with no budget round (a delete, a
    /// recovery).
    async fn select_quiet(
        &self,
        t: &Torrent,
        files: impl IntoIterator<Item = usize>,
    ) -> Result<()> {
        let mut live = t.live.lock().await;
        let Some(l) = live.as_mut() else {
            return Ok(());
        };
        let mut wanted = l.selected.clone();
        wanted.extend(files.into_iter().filter(|&f| f < t.files.len()));
        if wanted == l.selected {
            return Ok(());
        }
        l.managed.wait_until_initialized().await?;
        self.inner
            .session
            .update_only_files(&l.managed, &wanted)
            .await?;
        l.selected = wanted;
        Ok(())
    }

    /// The folders of kept torrents that are not loaded.
    fn kept_unloaded(&self) -> Vec<PathBuf> {
        let loaded: Vec<String> = lock(&self.inner.torrents)
            .values()
            .map(|t| t.info_hash.to_ascii_lowercase())
            .collect();
        lock(&self.inner.kept)
            .keys()
            .filter(|h| !loaded.contains(h))
            .map(|h| self.inner.cfg.data_dir.join(h))
            .collect()
    }

    /// Folders that are never leftovers: every loaded torrent's, and every
    /// kept one's.
    fn unreclaimable_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = lock(&self.inner.torrents)
            .values()
            .map(|t| t.dir.clone())
            .collect();
        dirs.extend(self.kept_unloaded());
        dirs
    }

    /// Tells `t`'s scheduler which of its files are kept.
    async fn send_kept(&self, t: &Torrent) {
        let files: Vec<usize> = self.kept_of(t).into_iter().collect();
        if let Some(l) = t.live.lock().await.as_ref() {
            let _ = l.msgs.send(adapter::Msg::Kept(files));
        }
    }

    /// Downloads `file` of `h` whole for offline viewing: the file is
    /// selected and kept, and the scheduler fetches it in piece order on
    /// the slots readers leave (`Class::Bulk`). The torrent wakes and stays
    /// awake while the handle lives. A torrent left in librqbit's error
    /// state (a full disk) is loaded again from its kept details first.
    pub async fn download(&self, h: TorrentHandle, file: usize) -> Result<DownloadHandle> {
        let t = self.torrent(h)?;
        if file >= t.files.len() {
            return Err(EngineError::NoFile);
        }
        // An errored torrent goes and comes back from its details, files
        // kept: `select_file` alone neither unpauses nor restarts it.
        let errored = t
            .live
            .lock()
            .await
            .as_ref()
            .is_some_and(|l| l.managed.with_state(|s| s.name() == "error"));
        if errored {
            let mut live = t.live.lock().await;
            if let Some(l) = live.take() {
                let _round = t.keeper.lock().await;
                let _ = self
                    .inner
                    .session
                    .delete(l.managed.id().into(), false)
                    .await;
            }
        }
        self.select_file(h, "", &FileHint::Index(file)).await?;
        if errored {
            // Back from its details with one file: the pack's other kept
            // and running files are selected again, and each running one
            // fetches on (code review r1, engine P2-5).
            let running: Vec<usize> = lock(&t.downloading).keys().copied().collect();
            let others: Vec<usize> = self
                .kept_of(&t)
                .into_iter()
                .chain(running.clone())
                .collect();
            self.select_quiet(&t, others).await?;
            if let Some(l) = t.live.lock().await.as_ref() {
                for f in running {
                    let _ = l.msgs.send(adapter::Msg::Bulk { file: f, on: true });
                }
            }
        }
        lock(&self.inner.kept)
            .entry(t.info_hash.to_ascii_lowercase())
            .or_default()
            .insert(file);
        // Raised before the scheduler is told, so no maintenance round in
        // between finds a fresh torrent with no reader idle (engine r2
        // P3-1).
        *lock(&t.downloading).entry(file).or_default() += 1;
        t.out_of_space.store(false, Ordering::Relaxed);
        // The switch is the torrent's: a ring's RAM never takes a
        // download's pieces. A RAM ring playing this torrent meanwhile would
        // write its next pieces to disk with its guards off; only TVs play
        // from a ring, and TVs never download (decision 14), so it cannot
        // happen today (code review r1, engine P3-7).
        t.memory.store(false, Ordering::Relaxed);
        self.send_kept(&t).await;
        {
            let live = t.live.lock().await;
            if let Some(l) = live.as_ref() {
                if l.managed.is_paused() {
                    self.inner.session.unpause(&l.managed).await?;
                }
                let _ = l.msgs.send(adapter::Msg::Bulk { file, on: true });
            }
        }
        *lock(&t.last_used) = Instant::now();
        Ok(DownloadHandle {
            engine: self.clone(),
            torrent: t,
            handle: h,
            file,
            stopped: false,
        })
    }

    /// Deletes idle stream cache, oldest first, until the volume has
    /// `need_free` bytes free or nothing idle is left: a download that fits
    /// only by it comes first (plans/offline-downloads.md 4.9). Kept and
    /// claimed torrents stay.
    pub async fn make_room(&self, need_free: u64) {
        self.enforce_budget(None, need_free).await;
    }

    /// Ends one running download of `t`'s `file`; the last one stops the
    /// scheduler's fetch. The file stays kept.
    async fn end_download(&self, t: &Torrent, file: usize) {
        let last = {
            let mut running = lock(&t.downloading);
            match running.get_mut(&file) {
                Some(n) if *n > 1 => {
                    *n -= 1;
                    false
                }
                Some(_) => {
                    running.remove(&file);
                    true
                }
                None => false,
            }
        };
        if last && let Some(l) = t.live.lock().await.as_ref() {
            let _ = l.msgs.send(adapter::Msg::Bulk { file, on: false });
        }
    }

    /// The disk fell under the reserve: every download stops fetching
    /// before a write fails, and says so on its handle; the caller waits
    /// for space and downloads again (engine r2 P2-3).
    async fn guard_downloads(&self) {
        let low = self.free().is_some_and(|free| free < RESERVE_BYTES);
        if !low {
            return;
        }
        let all: Vec<Arc<Torrent>> = lock(&self.inner.torrents).values().cloned().collect();
        for t in all {
            let files: Vec<usize> = lock(&t.downloading).keys().copied().collect();
            if files.is_empty() || t.out_of_space.swap(true, Ordering::Relaxed) {
                continue;
            }
            tracing::warn!("{}: downloads stop under the reserve", t.info_hash);
            if let Some(l) = t.live.lock().await.as_ref() {
                for file in files {
                    let _ = l.msgs.send(adapter::Msg::Bulk { file, on: false });
                }
            }
        }
    }

    /// Lets go of downloaded `file` of `info_hash` and frees its bytes
    /// (plans/offline-downloads.md 4.6 step 9): a torrent not loaded is
    /// loaded from its details first; only the pieces wholly inside the
    /// file that no other kept file shares are evicted, the boundary ones
    /// stay; with no kept file left and no playback, the torrent goes with
    /// its folder, else its folder is left to the budget's sweep.
    pub async fn delete_file(&self, info_hash: &str, file: usize) -> Result<()> {
        let hex = hex_hash(info_hash.trim()).ok_or(EngineError::BadInfoHash)?;
        // Played now: nothing changes under its reader, offline the rest of
        // the file is nowhere else (code review r1, engine P2-3). ponytail:
        // it stays kept until the next launch, whose kept list no longer
        // names it, and the sweep takes it then.
        let playing = lock(&self.inner.torrents)
            .values()
            .find(|t| t.info_hash == hex)
            .is_some_and(|t| t.claimed() || t.readers.load(Ordering::Relaxed) > 0);
        if playing {
            return Ok(());
        }
        let left = {
            let mut kept = lock(&self.inner.kept);
            let files = kept.entry(hex.clone()).or_default();
            files.remove(&file);
            let left = files.clone();
            if left.is_empty() {
                kept.remove(&hex);
            }
            left
        };
        let loaded = lock(&self.inner.torrents)
            .iter()
            .find(|(_, t)| t.info_hash == hex)
            .map(|(h, _)| *h);
        let h = match loaded {
            Some(h) => h,
            None if left.is_empty() => {
                // Nothing loaded and nothing kept: the folder goes now.
                let dir = self.inner.cfg.data_dir.join(&hex);
                if let Some(trash) = self.set_aside(&dir) {
                    let _ = tokio::fs::remove_dir_all(&trash).await;
                }
                return Ok(());
            }
            // From its kept details only, no swarm joined for a delete; with
            // none the folder waits for the sweep (code review r1, engine
            // P3-5).
            None => match self.add_kept(&hex).await {
                Some(h) => h,
                None => return Ok(()),
            },
        };
        let t = self.torrent(h)?;
        if let Some(l) = t.live.lock().await.as_ref() {
            let _ = l.msgs.send(adapter::Msg::Bulk { file, on: false });
        }
        lock(&t.downloading).remove(&file);
        if left.is_empty() {
            return match self.remove(h, false).await {
                Err(EngineError::Claimed) => Ok(()),
                other => other,
            };
        }
        // Live, with the file and the kept ones selected and its check
        // over: an eviction skips pieces of a torrent that is not. No budget
        // round for a file being deleted.
        self.select(h, "", &FileHint::Index(file), false).await?;
        self.select_quiet(&t, left.iter().copied()).await?;
        let managed = t.live.lock().await.as_ref().map(|l| l.managed.clone());
        if let Some(managed) = managed {
            managed.wait_until_initialized().await?;
        }
        self.send_kept(&t).await;
        let pieces = exclusive_pieces(&t, file);
        if !pieces.is_empty() {
            self.evict_pieces(h, &pieces).await?;
        }
        Ok(())
    }

    /// Moves a folder an earlier launch left out of the way under the
    /// torrents lock, unless a torrent of this launch took it up since: an
    /// add of the same info hash then never writes into data being deleted.
    fn set_aside(&self, dir: &Path) -> Option<PathBuf> {
        let torrents = lock(&self.inner.torrents);
        if torrents.values().any(|t| t.dir == dir) {
            return None;
        }
        let trash = dir.with_extension(SET_ASIDE);
        std::fs::rename(dir, &trash).ok()?;
        // Its pieces and details go with it: a later add of the hash checks
        // and asks afresh.
        let _ = std::fs::rename(dir.with_extension(HAVE), trash.join(HAVE));
        let _ = std::fs::rename(dir.with_extension(TORRENT), trash.join(TORRENT));
        Some(trash)
    }
}

/// Whether a [`DownloadProgress::failed`] is the disk's: a window's space
/// guard ("storage full", or a volume that cannot free space) or a write
/// that found no space. Anything else is the torrent's.
pub fn is_space_failure(why: &str) -> bool {
    why.contains("storage") || why.contains("No space left") || why.contains("os error 28")
}

/// A running download ([`Engine::download`]): it fetches while this lives,
/// and [`DownloadHandle::progress`] says how far it got.
pub struct DownloadHandle {
    engine: Engine,
    torrent: Arc<Torrent>,
    handle: TorrentHandle,
    file: usize,
    stopped: bool,
}

/// Where a download stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    /// Bytes of the file in verified pieces.
    pub have: u64,
    pub len: u64,
    /// The disk fell under the reserve and fetching stopped.
    pub out_of_space: bool,
    /// The torrent ended (a window's space guard) or librqbit stopped it
    /// with an error (its words): download again to go on.
    /// [`is_space_failure`] tells a full disk from the rest.
    pub failed: Option<String>,
    /// Connected peers.
    pub peers: usize,
}

impl DownloadHandle {
    pub fn handle(&self) -> TorrentHandle {
        self.handle
    }

    pub async fn progress(&self) -> DownloadProgress {
        let t = &self.torrent;
        let file = self.file;
        let have = self
            .engine
            .ask(t, |tx| adapter::Msg::FileHave { file, tx })
            .await
            .unwrap_or(0);
        // librqbit's own words, so a full disk reads as space and anything
        // else as the source's (code review r1, runtime P2-4).
        let errored = t.live.lock().await.as_ref().and_then(|l| {
            l.managed.with_state(|s| match s {
                librqbit::ManagedTorrentState::Error(e) => Some(format!("{e:#}")),
                _ => None,
            })
        });
        let peers = self
            .engine
            .stats(self.handle)
            .await
            .map(|s| s.peers)
            .unwrap_or(0);
        DownloadProgress {
            have,
            len: t.files[file].len,
            out_of_space: t.out_of_space.load(Ordering::Relaxed),
            failed: lock(&t.ended).clone().or(errored),
            peers,
        }
    }

    /// Ends the download; the file stays kept. A done download's torrent
    /// pauses when nothing else uses it, which writes its bitfield out.
    pub async fn stop(mut self) {
        self.stopped = true;
        let (engine, t, file) = (self.engine.clone(), self.torrent.clone(), self.file);
        engine.end_download(&t, file).await;
        // Its bitfield on disk now, whether the torrent pauses or goes on
        // for a stream: a relaunch finds the file whole.
        if let Some(l) = t.live.lock().await.as_ref() {
            l.managed.ext_flush_have();
        }
        if !t.active() && t.readers.load(Ordering::Relaxed) == 0 {
            let live = t.live.lock().await;
            if let Some(l) = live.as_ref().filter(|l| !l.managed.is_paused()) {
                let _round = t.keeper.lock().await;
                if let Err(e) = engine.inner.session.pause(&l.managed).await {
                    tracing::warn!("pausing a finished download: {e:#}");
                }
                let _ = l.msgs.send(adapter::Msg::Stopped);
            }
        }
    }
}

impl Drop for DownloadHandle {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        let (engine, t, file) = (self.engine.clone(), self.torrent.clone(), self.file);
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(async move { engine.end_download(&t, file).await });
        }
    }
}

/// The pieces of `t` wholly inside `file`: what a delete of `file` may
/// evict. A piece it shares with a neighbour (another kept file's, or one a
/// stream reads) stays.
fn exclusive_pieces(t: &Torrent, file: usize) -> Vec<u32> {
    let piece = u64::from(t.piece_len);
    let start: u64 = t.files[..file].iter().map(|f| f.len).sum();
    let end = start + t.files[file].len;
    let total: u64 = t.files.iter().map(|f| f.len).sum();
    // The torrent's last, shorter piece is the last file's alone.
    let last = if end == total {
        end.div_ceil(piece)
    } else {
        end / piece
    };
    (start.div_ceil(piece)..last).map(|p| p as u32).collect()
}

/// Whether `file` of the torrent `info_hash` is whole on this device
/// (plans/offline-downloads.md 4.8): every piece holding a byte of it is
/// in the torrent's `.have`, and its data is all there: a whole file at
/// full length whose allocated blocks cover it but a piece (a punched hole
/// keeps the length), or a folder of piece files, each at its size.
/// Blocking: reads the disk. A filesystem with transparent compression
/// counts fewer blocks than bytes and reads as not whole here.
pub fn complete_on_disk(data_dir: &Path, info_hash: &str, file: usize) -> bool {
    let Some(hex) = hex_hash(info_hash.trim()) else {
        return false;
    };
    let dir = data_dir.join(&hex);
    let Some(shape) = kept_shape(&dir, &hex) else {
        return false;
    };
    let Some(f) = shape.files.get(file) else {
        return false;
    };
    let have = std::fs::read(dir.with_extension(HAVE)).unwrap_or_default();
    let piece = u64::from(shape.piece_len);
    let start: u64 = shape.files[..file].iter().map(|f| f.len).sum();
    if f.len == 0 {
        return true;
    }
    let pieces = start / piece..(start + f.len).div_ceil(piece);
    let has = |p: u64| {
        have.get((p / 8) as usize)
            .is_some_and(|byte| byte & (0x80 >> (p % 8)) != 0)
    };
    if !pieces.clone().all(has) {
        return false;
    }
    let path = dir.join(&f.path);
    if path.is_dir() {
        // Piece files, named by piece index, each holding the file's part of
        // its piece: a short one after a crash is no whole file (code review
        // r1, engine P3-4).
        let end = start + f.len;
        return pieces.clone().all(|p| {
            let part = ((p + 1) * piece).min(end) - (p * piece).max(start);
            std::fs::metadata(path.join(p.to_string()))
                .is_ok_and(|m| m.is_file() && m.len() == part)
        });
    }
    std::fs::metadata(&path)
        .is_ok_and(|m| m.is_file() && m.len() == f.len && file_allocated(&path) + piece >= f.len)
}

/// The ending of a torrent folder on its way out.
const SET_ASIDE: &str = "evicted";

/// The ending of a torrent's have-bitfield, beside its folder (the
/// librqbit fork's fast resume).
const HAVE: &str = "have";

/// The ending of a torrent's details (its `.torrent` file), beside its
/// folder.
const TORRENT: &str = "torrent";

/// The torrent's own add dials in this order: the peers its metadata came
/// with, the one that sent it first (the fork lists it so, start-latency
/// cause 6), then the caller's not among them.
fn dial_order(seen: &[SocketAddr], given: &[SocketAddr]) -> Vec<SocketAddr> {
    let mut order = seen.to_vec();
    order.extend(given.iter().filter(|p| !seen.contains(p)));
    order
}

/// Lowercase hex of a v1 info hash given as 40 hex or 32 base32
/// characters.
fn hex_hash(hash: &str) -> Option<String> {
    use std::str::FromStr;
    let id = librqbit::dht::Id20::from_str(&hash.to_ascii_uppercase()).ok()?;
    Some(id.as_string())
}

/// The details kept at `path` when they parse and are `hex`'s (a file cut
/// short or another torrent's is no use). Blocking: reads the disk.
fn kept_torrent(path: &Path, hex: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let file = std::fs::File::open(path).ok()?;
    file.take(MAX_TORRENT + 1).read_to_end(&mut bytes).ok()?;
    let small = bytes.len() as u64 <= MAX_TORRENT;
    match librqbit::torrent_from_bytes(&bytes) {
        Ok(meta) if small && meta.info_hash.as_string() == hex => Some(bytes),
        _ => {
            tracing::warn!("{}: not the details of {hex}", path.display());
            None
        }
    }
}

/// [`Engine::cached`] with no engine running (a launch that has not started
/// one): the same answer from `data_dir` alone, a torrent's kept details and
/// its `.have`. Blocking: reads the disk, so call it off the async workers.
pub fn cached_on_disk(
    data_dir: &Path,
    info_hash: &str,
    file: usize,
    offset: u64,
) -> Option<Cached> {
    let hex = hex_hash(info_hash.trim())?;
    let dir = data_dir.join(&hex);
    let shape = kept_shape(&dir, &hex)?;
    Some(cached_from(&dir, &shape, None, file, offset))
}

/// What the on-device check needs of a torrent's details.
struct Shape {
    piece_len: u32,
    files: Vec<FileInfo>,
}

/// The shape of the details kept beside `dir`, parsed once per version of
/// the file (its length and change time): a list of rows asks again and
/// again, and a parse hashes the whole info. Everything else the check
/// reads afresh every time. Blocking.
fn kept_shape(dir: &Path, hex: &str) -> Option<Arc<Shape>> {
    let path = dir.with_extension(TORRENT);
    let meta = std::fs::metadata(&path).ok()?;
    let stamp = (meta.len(), meta.modified().ok());
    if let Some((s, shape)) = lock(&SHAPES).get(&path)
        && *s == stamp
    {
        return shape.clone();
    }
    let shape = kept_torrent(&path, hex)
        .and_then(|bytes| {
            let info = librqbit::torrent_from_bytes(&bytes)
                .ok()?
                .info
                .data
                .validate()
                .ok()?;
            Some(Shape {
                piece_len: info.lengths().default_piece_length(),
                files: file_infos(&info),
            })
        })
        .map(Arc::new);
    let mut memo = lock(&SHAPES);
    // ponytail: dropped whole at 1024 torrents, an LRU if a device ever
    // keeps more.
    if memo.len() >= 1024 {
        memo.clear();
    }
    memo.insert(path, (stamp, shape.clone()));
    shape
}

/// [`kept_shape`]'s parses by `.torrent` path, with the length and change
/// time they were read at (a file that does not parse is kept as `None`).
#[allow(clippy::type_complexity)]
static SHAPES: std::sync::LazyLock<
    Mutex<HashMap<PathBuf, ((u64, Option<std::time::SystemTime>), Option<Arc<Shape>>)>>,
> = std::sync::LazyLock::new(Mutex::default);

/// A `.torrent` larger than this is not read: a v1 file for a terabyte in
/// 4 MiB pieces holds 5 MB of hashes.
const MAX_TORRENT: u64 = 16 << 20;

/// The torrent of `shape` in `dir` at `offset` of `file`, from `have` (a
/// live bitfield) or, with none, its `.have`. Blocking.
fn cached_from(
    dir: &Path,
    shape: &Shape,
    have: Option<Vec<u8>>,
    file: usize,
    offset: u64,
) -> Cached {
    let have = have.unwrap_or_else(|| std::fs::read(dir.with_extension(HAVE)).unwrap_or_default());
    // Every device class waits on the same Ready set: the presets differ
    // in readahead and request depths only.
    let sched = SchedConfig::standard();
    Cached {
        ready: ready_on_disk(dir, shape, &have, sched, file, offset),
    }
}

/// Whether a range of `file` opened at `offset` would be Ready from the
/// pieces in `have` (a BEP 3 bitfield), by the adapter's own rule: the
/// blocking window and the file's index, an MP4's found from its head on
/// disk in `dir`. Blocking: reads the disk.
fn ready_on_disk(
    dir: &Path,
    shape: &Shape,
    have: &[u8],
    sched: SchedConfig,
    file: usize,
    offset: u64,
) -> bool {
    let (piece_len, files) = (shape.piece_len, &shape.files);
    let Some(f) = files.get(file) else {
        return false;
    };
    let path = dir.join(&f.path);
    // A `.have` whose data is gone or cut short (a purge, a crash in an
    // eviction) is no start: the engine gives a file its whole length when
    // it starts, or keeps it as piece files.
    let chunked = path.is_dir().then(|| {
        let start = files[..file].iter().map(|f| f.len).sum();
        librqbit::storage::filesystem::Chunked::new(
            path.clone(),
            start,
            u64::from(piece_len),
            f.len,
        )
    });
    if chunked.is_none() && !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == f.len)
    {
        return false;
    }
    let lens: Vec<u64> = files.iter().map(|f| f.len).collect();
    let iso = files.iter().map(|f| iso_bmff(&f.path)).collect();
    let mut tr = adapter::Translator::new(piece_len, &lens, sched, true).with_iso_bmff(iso);
    tr.verified_bitfield(have);
    let (ready, rx) = tokio::sync::watch::channel(false);
    tr.on_msg(adapter::Msg::Open {
        demand: 0,
        file,
        start: offset,
        end: None,
        ready,
    });
    if let Some((file, n)) = tr.index_wanted() {
        use std::io::Read;
        let mut head = vec![0u8; n as usize];
        let read = if let Some(c) = &chunked {
            c.read_exact_at(0, &mut head).is_ok()
        } else {
            std::fs::File::open(&path)
                .and_then(|mut f| f.read_exact(&mut head))
                .is_ok()
        };
        if !read {
            return false;
        }
        tr.index_found(file, &head);
    }
    *rx.borrow()
}

/// A torrent's files, paths `/` separated.
fn file_infos<B: AsRef<[u8]>>(info: &librqbit::ValidatedTorrentMetaV1Info<B>) -> Vec<FileInfo> {
    info.iter_file_details()
        .enumerate()
        .map(|(idx, fd)| FileInfo {
            idx,
            path: fd.filename.iter_components().collect::<Vec<_>>().join("/"),
            len: fd.len,
        })
        .collect()
}

/// Writes a torrent's details to `path`, whole or not at all: synced before
/// the rename, so a crash leaves the old file, the new one or none.
async fn keep_torrent(path: PathBuf, bytes: Vec<u8>) {
    use tokio::io::AsyncWriteExt;
    let tmp = path.with_extension("torrent.tmp");
    let kept = async {
        let mut file = tokio::fs::File::create(&tmp).await?;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        tokio::fs::rename(&tmp, &path).await
    };
    if let Err(e) = kept.await {
        tracing::warn!("{}: details not kept: {e}", path.display());
    }
}

/// A leftover: when a file in it last changed, the folder, its allocated
/// size.
type Leftover = (std::time::SystemTime, PathBuf, u64);

/// Torrent folders in `data_dir` no torrent of this launch holds (named by
/// an info hash, not in `held`), the least recently changed first; and
/// the folders an interrupted round set aside (`<info hash>.evicted`
/// only: the CLI's data folder can be any folder). Blocking: walks the
/// disk.
fn leftovers(data_dir: &Path, held: &[&Path]) -> (Vec<Leftover>, Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return (Vec::new(), Vec::new());
    };
    let is_hash = |n: &str| n.len() == 40 && n.bytes().all(|b| b.is_ascii_hexdigit());
    let (mut left, mut trash) = (Vec::new(), Vec::new());
    for e in entries.flatten() {
        let path = e.path();
        if !e.metadata().is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let name = e.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        match name.strip_suffix(&format!(".{SET_ASIDE}")) {
            Some(stem) if is_hash(stem) => trash.push(path),
            Some(_) => {}
            None if is_hash(name) && !held.contains(&path.as_path()) => {
                left.push((last_change(&path), path.clone(), allocated(&path)));
            }
            None => {}
        }
    }
    left.sort_by_key(|l| l.0);
    (left, trash)
}

/// When a file under `dir` last changed (the epoch when none can be read).
fn last_change(dir: &Path) -> std::time::SystemTime {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return std::time::SystemTime::UNIX_EPOCH;
    };
    entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            if m.is_dir() {
                Some(last_change(&e.path()))
            } else {
                m.modified().ok()
            }
        })
        .max()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}

/// Keep-warm expiry and the disk budget, at start (what earlier launches
/// left) and every few seconds after.
async fn maintenance(inner: std::sync::Weak<Inner>) {
    let mut first = true;
    loop {
        if !first {
            tokio::time::sleep(MAINTENANCE_EVERY).await;
        }
        first = false;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        let engine = Engine { inner };
        let torrents: Vec<Arc<Torrent>> = lock(&engine.inner.torrents).values().cloned().collect();
        for t in torrents {
            // A paused player keeps filling its anchor's readahead
            // (plans/streaming-window.md 2.4).
            if t.readers.load(Ordering::Relaxed) > 0
                || lock(&t.last_used).elapsed() < KEEP_WARM
                || t.player_paused.load(Ordering::Relaxed)
                || t.downloading()
            {
                continue;
            }
            let live = t.live.lock().await;
            if let Some(l) = live.as_ref().filter(|l| !l.managed.is_paused()) {
                let _round = t.keeper.lock().await;
                if let Err(e) = engine.inner.session.pause(&l.managed).await {
                    tracing::warn!("pausing idle torrent: {e:#}");
                }
                let _ = l.msgs.send(adapter::Msg::Stopped);
            }
        }
        engine.enforce_budget(None, 0).await;
        engine.guard_whole_files().await;
        engine.guard_downloads().await;
        let counts = window::Counted {
            reclaimable: engine.reclaimable(None).await,
            committed: engine.committed(None).await,
            held: engine.held_bytes().await,
        };
        *lock(&engine.inner.counted) = Some(counts);
    }
}

/// Readahead past the playback window for a file that fits: the device's
/// preset on a metered network, else enough for 120 s ahead in all at
/// `bytes_per_s`, never under the preset and never over the idle budget.
fn readahead_for(
    preset: u64,
    playback_secs: u64,
    bytes_per_s: u64,
    metered: bool,
    budget: u64,
) -> u64 {
    if metered {
        return preset;
    }
    let ahead = (120u64.saturating_sub(playback_secs)).saturating_mul(bytes_per_s);
    ahead.max(preset).min(budget.max(preset))
}

/// Bytes actually allocated on disk (sparse files count what is written).
/// What one file takes on disk (none when it is not there).
fn file_allocated(path: &Path) -> u64 {
    librqbit::storage::filesystem::allocated(path)
}

/// [`window::counted_on_disk`]'s folders, but those in `held`.
/// Kept folders (`kept`, not loaded) are held, never idle.
fn counted_on_disk_except(
    data_dir: &Path,
    held: &[&Path],
    kept: &[PathBuf],
) -> HashMap<String, HeldBytes> {
    let mut not_idle: Vec<&Path> = held.to_vec();
    not_idle.extend(kept.iter().map(PathBuf::as_path));
    let mut counted: HashMap<String, HeldBytes> = kept
        .iter()
        .filter_map(|path| {
            let hash = path.file_name()?.to_str()?.to_ascii_lowercase();
            let held = HeldBytes {
                folder: allocated(path),
                files: single_file(path).into_iter().collect(),
                idle: false,
                committed: 0,
            };
            Some((hash, held))
        })
        .collect();
    counted.extend(leftovers_counted(data_dir, &not_idle));
    counted
}

fn leftovers_counted(data_dir: &Path, held: &[&Path]) -> HashMap<String, HeldBytes> {
    leftovers(data_dir, held)
        .0
        .into_iter()
        .filter_map(|(_, path, folder)| {
            let hash = path.file_name()?.to_str()?.to_ascii_lowercase();
            // A folder of one file: that file's bytes, as the app counts
            // them once the torrent loads (review r3 A R3).
            let files = single_file(&path).into_iter().collect();
            let held = HeldBytes {
                folder,
                files,
                idle: true,
                committed: 0,
            };
            Some((hash, held))
        })
        .collect()
}

/// What the one file under `dir` takes on disk; `None` when there are more.
fn single_file(dir: &Path) -> Option<u64> {
    let mut found = None;
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let m = e.metadata().ok()?;
            // A file kept as piece files is a folder of them.
            if m.is_dir() && !librqbit::storage::filesystem::is_chunked(&e.path()) {
                dirs.push(e.path());
            } else if found.replace(file_allocated(&e.path())).is_some() {
                return None;
            }
        }
    }
    found
}

fn allocated(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => allocated(&e.path()),
            #[cfg(unix)]
            Ok(m) => std::os::unix::fs::MetadataExt::blocks(&m) * 512,
            #[cfg(not(unix))]
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// An MP4 or MOV file by its name in the torrent.
fn iso_bmff(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    matches!(ext.as_deref(), Some("mp4" | "m4v" | "mov"))
}

/// A magnet with [`tracker_list`]'s trackers.
fn magnet_uri(hash: &str, trackers: &[String], defaults: bool) -> String {
    let mut magnet = format!("magnet:?xt=urn:btih:{hash}");
    for t in tracker_list(trackers, defaults) {
        magnet.push_str("&tr=");
        magnet.push_str(&percent_encode(&t));
    }
    magnet
}

/// The defaults first when `defaults`, then at most `MAX_OWN` of the
/// stream's own trackers, each once. Discovery is off only in loopback
/// tests, which keep exactly theirs.
fn tracker_list(trackers: &[String], defaults: bool) -> Vec<String> {
    let extra = DEFAULT_TRACKERS.into_iter().filter(|_| defaults);
    let given = trackers
        .iter()
        .map(|t| t.strip_prefix("tracker:").unwrap_or(t))
        .filter(|t| t.contains("://"));
    let mut list: Vec<String> = Vec::new();
    let mut push = |t: &str| {
        let new = !list.iter().any(|x| x == t);
        if new {
            list.push(t.to_owned());
        }
        new
    };
    for t in extra {
        push(t);
    }
    // An addon's list is unbounded, and each tracker is a task that looks
    // its name up on the shared blocking pool, again every minute: the
    // scrape's cap holds here too (engine review r2, P2-3).
    let mut own = 0;
    for t in given {
        if own == scrape::MAX_OWN {
            break;
        }
        if push(t) {
            own += 1;
        }
    }
    list
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file that fits keeps 120 s of itself ahead off a metered network
    /// (the playback window's 30 s plus 90 s of readahead), never under the
    /// preset and never over the idle budget; on a metered network, the
    /// preset (plans/streaming-window.md 2.2, phase 2 test l).
    #[test]
    fn a_file_that_fits_keeps_120_s_ahead_off_cellular() {
        const MIB: u64 = 1 << 20;
        let preset = 128 * MIB;
        let budget = 6 << 30;
        // A 38.2 Mbit/s remux: 90 s of it is 430 MB.
        let remux = 4_775_000;
        assert_eq!(readahead_for(preset, 30, remux, false, budget), 90 * remux);
        assert_eq!(readahead_for(preset, 30, remux, true, budget), preset);
        // A 3 Mbit/s file: 90 s is 34 MB, under the preset.
        assert_eq!(readahead_for(preset, 30, 375_000, false, budget), preset);
        // A small idle budget caps it, never under the preset.
        assert_eq!(
            readahead_for(preset, 30, remux, false, 200 * MIB),
            200 * MIB
        );
        assert_eq!(readahead_for(preset, 30, remux, false, 64 * MIB), preset);
    }

    #[test]
    fn a_bare_info_hash_gets_the_default_trackers_once() {
        let hash = "a779fa75c15ab1d2e8fb12e7b7ee4ab3d5e0a7a3";
        let trs = |m: &str| m.matches("&tr=").count();
        let bare = magnet_uri(hash, &[], true);
        assert_eq!(trs(&bare), DEFAULT_TRACKERS.len(), "{bare}");
        let own = "udp://tracker.opentrackr.org:1337/announce".to_string();
        let mixed = magnet_uri(hash, &["tracker:udp://own.example:80".into(), own], true);
        assert_eq!(
            trs(&mixed),
            DEFAULT_TRACKERS.len() + 1,
            "a default listed once"
        );
        assert!(
            mixed.find("stealth") < mixed.find("own.example"),
            "the defaults first"
        );
        // An addon may list any number: 16 of the stream's own, then no more
        // (engine review r2, P2-3).
        let many: Vec<String> = (0..100)
            .map(|i| format!("udp://t{i}.example:80/announce"))
            .collect();
        let capped = magnet_uri(hash, &many, true);
        assert_eq!(trs(&capped), DEFAULT_TRACKERS.len() + scrape::MAX_OWN);
        assert!(capped.contains("t15.example") && !capped.contains("t16.example"));
        assert_eq!(
            trs(&magnet_uri(hash, &[], false)),
            0,
            "loopback tests keep theirs"
        );
    }

    #[test]
    fn the_session_turns_off_upnp_and_lsd() {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        for discovery in [true, false] {
            let cfg = EngineConfig {
                discovery,
                ..EngineConfig::new("unused", http.torrent_scope(&[]))
            };
            let opts = session_options(&cfg, &Scopes::new(&cfg.policy));
            let listen = opts.listen.as_ref().expect("a listener");
            assert!(!listen.enable_upnp_port_forwarding, "UPnP off");
            assert!(opts.disable_local_service_discovery, "LSD off");
            // uTP listens only behind the policy's check (C27 11).
            assert!(matches!(listen.mode, ListenerMode::TcpAndUtp), "uTP on");
            assert!(opts.destinations.is_some(), "the policy is set");
            assert!(!opts.disable_trackers, "trackers run either way");
            assert_eq!(opts.dht.is_some(), discovery);
        }
    }

    fn config(dir: &Path, budget: u64) -> EngineConfig {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        EngineConfig {
            discovery: false,
            cache_budget_bytes: budget,
            ..EngineConfig::new(dir, http.torrent_scope(&[]))
        }
    }

    /// Only a lock another engine holds is "in use"; a filesystem with no
    /// lock support is an I/O error (engine review r4, P3-2).
    #[test]
    fn a_lock_error_is_in_use_only_when_another_engine_holds_it() {
        assert!(matches!(
            lock_error(std::fs::TryLockError::WouldBlock),
            EngineError::DataInUse
        ));
        let unsupported = std::io::Error::other("no flock here");
        assert!(matches!(
            lock_error(std::fs::TryLockError::Error(unsupported)),
            EngineError::Io(_)
        ));
    }

    /// Two engines on one folder would evict each other's data: the second
    /// is refused while the first lives (engine review r3, P3-3).
    #[tokio::test]
    async fn two_engines_never_share_a_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        let first = Engine::start(config(dir.path(), u64::MAX)).await.unwrap();
        assert!(matches!(
            Engine::start(config(dir.path(), u64::MAX)).await,
            Err(EngineError::DataInUse)
        ));
        drop(first);
        // Its first maintenance round may still hold it a moment.
        let mut again = Engine::start(config(dir.path(), u64::MAX)).await;
        for _ in 0..40 {
            if again.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            again = Engine::start(config(dir.path(), u64::MAX)).await;
        }
        assert!(again.is_ok(), "the folder stayed locked");
    }

    /// A round waits for the one running (engine review r3, P3-2: two at
    /// once each counted what the other was deleting and evicted the newest
    /// leftover too).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn eviction_rounds_run_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        // One leftover fits, two do not.
        let engine = Engine::start(config(dir.path(), 3 << 19)).await.unwrap();
        let running = engine.inner.evicting.lock().await;
        let (older, newer) = (
            dir.path().join("1".repeat(40)),
            dir.path().join("2".repeat(40)),
        );
        let now = std::time::SystemTime::now();
        for (folder, age) in [(&older, 7200), (&newer, 3600)] {
            std::fs::create_dir_all(folder).unwrap();
            let file = folder.join("movie.mkv");
            std::fs::write(&file, vec![7u8; 1 << 20]).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(now - Duration::from_secs(age))
                .unwrap();
        }
        let next = tokio::spawn({
            let engine = engine.clone();
            async move { engine.enforce_budget(None, 0).await }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(older.exists(), "a round ran beside the running one");
        drop(running);
        next.await.unwrap();
        assert!(!older.exists(), "the older leftover stayed");
        assert!(newer.exists(), "the newer leftover went too");
    }

    const MIB: u64 = 1 << 20;

    /// A torrent of one file kept in `data_dir` as a playback leaves it:
    /// `name` of `len` bytes in `piece` pieces (a path such as `Season 1/a.mkv`
    /// makes it a folder torrent, a pack's shape), its data file given its
    /// whole length as the engine does and starting with `head` (every byte
    /// written with `whole`), and a `.have` with the pieces `have`. Its info
    /// hash.
    fn kept(
        data_dir: &Path,
        name: &str,
        (len, piece): (u64, u64),
        head: &[u8],
        whole: bool,
        have: impl IntoIterator<Item = u64>,
    ) -> String {
        let n = len.div_ceil(piece);
        let info = if name.contains('/') {
            let path: String = name
                .split('/')
                .map(|p| format!("{}:{p}", p.len()))
                .collect();
            format!("d5:filesld6:lengthi{len}e4:pathl{path}eee4:name4:Pack")
        } else {
            format!("d6:lengthi{len}e4:name{}:{name}", name.len())
        };
        let mut t =
            format!("d4:info{info}12:piece lengthi{piece}e6:pieces{}:", n * 20).into_bytes();
        t.extend(vec![0xab; (n * 20) as usize]);
        t.extend(b"ee");
        let hex = librqbit::torrent_from_bytes(&t)
            .unwrap()
            .info_hash
            .as_string();
        let dir = data_dir.join(&hex);
        std::fs::create_dir_all(&dir).unwrap();
        let mut data = head.to_vec();
        data.resize(if whole { len as usize } else { head.len() }, 7);
        let file = dir.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &data).unwrap();
        let opened = std::fs::File::options().write(true).open(&file).unwrap();
        opened.set_len(len).unwrap();
        std::fs::write(dir.with_extension(TORRENT), &t).unwrap();
        let mut bits = vec![0u8; n.div_ceil(8) as usize];
        for p in have {
            bits[(p / 8) as usize] |= 0x80 >> (p % 8);
        }
        std::fs::write(dir.with_extension(HAVE), bits).unwrap();
        hex
    }

    /// An MP4 head: `ftyp`, then an `mdat` that leaves the last 200 KiB of
    /// `len` to the `moov`.
    fn mp4_head(len: u64) -> Vec<u8> {
        let mut head = vec![0, 0, 0, 24];
        head.extend(b"ftypisom");
        head.extend([0; 12]);
        head.extend(((len - 24 - (200 << 10)) as u32).to_be_bytes());
        head.extend(b"mdat");
        head.resize(4096, 0);
        head
    }

    /// The on-device check at an offset (review P2-5): a kept MP4 with its
    /// head and `moov` is ready at the head, not half way until that part
    /// is kept too; with its data gone, or its head unreadable, it is not
    /// ready at all (P2-4). An MKV waits on its tail.
    #[test]
    fn the_on_device_check_follows_the_offset_and_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let at = |hex: &str, offset| cached_on_disk(dir.path(), hex, 0, offset).map(|c| c.ready);
        // 8 MiB in 256 KiB pieces: head 0..4, `moov` in 31, half way 16..20.
        let geo = (8 * MIB, 256 << 10);
        let mp4 = kept(
            dir.path(),
            "a.mp4",
            geo,
            &mp4_head(8 * MIB),
            false,
            (0..4).chain([31]),
        );
        assert_eq!(at(&mp4, 0), Some(true));
        assert_eq!(at(&mp4, 4 * MIB), Some(false), "half way is not kept");
        // More pieces claimed through the mapping, its change time as before:
        // the kept answer goes.
        let have = dir.path().join(&mp4).with_extension(HAVE);
        let was = std::fs::metadata(&have).unwrap().modified().unwrap();
        std::fs::write(&have, [0xff; 4]).unwrap();
        let file = std::fs::File::options().write(true).open(&have).unwrap();
        file.set_modified(was).unwrap();
        assert_eq!(at(&mp4, 4 * MIB), Some(true), "a stale answer");
        let all = kept(dir.path(), "b.mp4", geo, &mp4_head(8 * MIB), false, 0..32);
        assert_eq!(at(&all, 4 * MIB), Some(true));
        // A data file cut short, or one whose head cannot be read: no start.
        let short = kept(dir.path(), "c.mp4", geo, &mp4_head(8 * MIB), false, 0..32);
        assert_eq!(at(&short, 0), Some(true));
        let data = dir.path().join(&short).join("c.mp4");
        let opened = std::fs::File::options().write(true).open(&data).unwrap();
        opened.set_len(8 * MIB - 1).unwrap();
        assert_eq!(at(&short, 0), Some(false), "a file cut short");
        opened.set_len(8 * MIB).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o200)).unwrap();
            assert_eq!(at(&short, 0), Some(false), "an unreadable head");
        }
        // An MKV kept half way and at its 2 MiB tail (pieces 24..32), not at
        // its head: no head read tells that the data went.
        let mkv = kept(dir.path(), "d.mkv", geo, b"", true, (16..20).chain(24..32));
        assert_eq!(at(&mkv, 4 * MIB), Some(true));
        assert_eq!(at(&mkv, 0), Some(false));
        std::fs::remove_dir_all(dir.path().join(&mkv)).unwrap();
        assert_eq!(at(&mkv, 4 * MIB), Some(false), "the data is gone");
        assert_eq!(at(&"0".repeat(40), 0), None, "nothing kept");
    }

    /// Kept details over 16 MiB are not read (review P2-5), even ones that
    /// parse and are the right torrent's.
    #[test]
    fn kept_details_over_the_cap_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        // 17.2 MB of piece hashes.
        let n: u64 = 860_000;
        let mut t = format!(
            "d4:infod6:lengthi{}e4:name5:x.mkv12:piece lengthi16384e6:pieces{}:",
            n * 16384,
            n * 20
        )
        .into_bytes();
        t.extend(vec![0xab; (n * 20) as usize]);
        t.extend(b"ee");
        assert!(t.len() as u64 > MAX_TORRENT);
        let hex = librqbit::torrent_from_bytes(&t)
            .unwrap()
            .info_hash
            .as_string();
        let path = dir.path().join(&hex).with_extension(TORRENT);
        std::fs::write(&path, &t).unwrap();
        assert_eq!(kept_torrent(&path, &hex), None);
    }

    /// RFC 4648 base32, no padding.
    fn base32(bytes: &[u8]) -> String {
        const ABC: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let (mut out, mut acc, mut bits) = (String::new(), 0u64, 0);
        for &b in bytes {
            (acc, bits) = ((acc << 8) | u64::from(b), bits + 8);
            while bits >= 5 {
                bits -= 5;
                out.push(ABC[((acc >> bits) & 31) as usize] as char);
            }
            acc &= (1 << bits) - 1;
        }
        out
    }

    /// A hash given in base32, either case, is the same torrent as its hex
    /// (review P2-5).
    #[test]
    fn a_base32_hash_finds_the_kept_details() {
        let dir = tempfile::tempdir().unwrap();
        let hex = kept(dir.path(), "a.mkv", (2 * MIB, 256 << 10), b"", false, 0..8);
        let bytes: Vec<u8> = (0..20)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        let b32 = base32(&bytes);
        assert_eq!(b32.len(), 32);
        for given in [b32.clone(), b32.to_lowercase(), hex.to_uppercase()] {
            assert_eq!(hex_hash(&given).as_deref(), Some(hex.as_str()), "{given}");
            let ready = cached_on_disk(dir.path(), &given, 0, 0);
            assert_eq!(ready, Some(Cached { ready: true }), "{given}");
        }
    }

    /// A pack keeps its videos in a folder (review r2 P2-1): one deleted or
    /// cut short there is not ready, though the torrent's own folder is
    /// unchanged.
    #[test]
    fn a_pack_video_deleted_in_its_folder_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let name = "Season 1/a.mkv";
        let geo = (8 * MIB, 256 << 10);
        let hex = kept(dir.path(), name, geo, b"", false, (0..4).chain(24..32));
        let at = || cached_on_disk(dir.path(), &hex, 0, 0).map(|c| c.ready);
        assert_eq!(at(), Some(true));
        let video = dir.path().join(&hex).join(name);
        std::fs::remove_file(&video).unwrap();
        assert_eq!(at(), Some(false), "deleted");
        std::fs::write(&video, b"short").unwrap();
        assert_eq!(at(), Some(false), "cut short");
    }

    /// A folder an earlier launch left goes with its `.have` and `.torrent`
    /// when the budget evicts it (review P2-5).
    #[tokio::test]
    async fn an_evicted_leftover_takes_its_details() {
        let dir = tempfile::tempdir().unwrap();
        let hex = kept(dir.path(), "a.mkv", (2 * MIB, 256 << 10), b"", true, 0..8);
        let engine = Engine::start(config(dir.path(), MIB)).await.unwrap();
        engine.enforce_budget(None, 0).await;
        let gone = |end| !dir.path().join(&hex).with_extension(end).exists();
        assert!(gone(TORRENT) && gone(HAVE), "the details stayed");
        assert!(!dir.path().join(&hex).exists(), "the folder stayed");
    }

    /// The torrent's own add dials the peers met with the metadata in their
    /// order (the sender first), then the caller's not among them (cause 6).
    #[test]
    fn the_metadata_peers_are_dialed_first() {
        let p = |n: u8| SocketAddr::from(([10, 0, 0, n], 6881));
        let seen = [p(3), p(1), p(2)];
        assert_eq!(dial_order(&seen, &[p(9), p(1)]), [p(3), p(1), p(2), p(9)]);
        assert_eq!(dial_order(&[], &[p(9)]), [p(9)]);
    }
}
