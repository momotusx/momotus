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

pub use adapter::PeerTally;
pub use check::PieceCheck;
/// The fork itself, for tools and tests (creating a torrent to seed).
pub use librqbit;
pub use pick::FileHint;
pub use reader::{RangeReader, StreamReader};
pub use scrape::{SwarmCount, TrackerTrace};

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
            "Marquee {version} (librqbit {})",
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
    /// The torrent is paused (idle past its keep-warm minute, or held).
    pub paused: bool,
    /// [`Engine::pause_now`] holds it for some profile: none of that
    /// profile's ranges or file choices wakes it.
    pub held: bool,
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
}

impl Torrent {
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
        let session = Session::new_with_opts(cfg.data_dir.clone(), opts).await?;
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
    /// BEP 48 HTTP) under the trackers' address policy; answers after about
    /// a second are dropped, and it returns sooner once every tracker asked
    /// has answered or failed. A count is the answer with the most seeders,
    /// then leechers; `None` when no answer told anything (a default
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
    /// Nothing downloads until [`Engine::select_file`].
    pub async fn add_magnet(
        &self,
        info_hash: &str,
        trackers: &[String],
        peers: &[SocketAddr],
    ) -> Result<TorrentHandle> {
        // Every add reads the interfaces again, as every playback open does.
        self.inner.cfg.policy.refresh();
        let hash = info_hash.trim();
        let valid = (hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            || (hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_alphanumeric()));
        if !valid {
            return Err(EngineError::BadInfoHash);
        }
        // Known already (a failover or a second view of the same torrent):
        // no second metadata round.
        let lower = hash.to_ascii_lowercase();
        if let Some((&h, _)) = lock(&self.inner.torrents)
            .iter()
            .find(|(_, t)| t.info_hash == lower)
        {
            return Ok(h);
        }
        let magnet = magnet_uri(hash, trackers, self.inner.cfg.discovery);
        self.add(AddTorrent::from_url(magnet), peers).await
    }

    /// Adds a torrent from `.torrent` bytes. A torrent whose data already
    /// sits in `data_dir/<info hash>/` is verified and seeded once selected.
    pub async fn add_torrent_bytes(
        &self,
        bytes: Vec<u8>,
        peers: &[SocketAddr],
    ) -> Result<TorrentHandle> {
        self.inner.cfg.policy.refresh();
        self.add(AddTorrent::from_bytes(bytes), peers).await
    }

    async fn add(&self, add: AddTorrent<'static>, peers: &[SocketAddr]) -> Result<TorrentHandle> {
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
        let files = list
            .info
            .iter_file_details()
            .enumerate()
            .map(|(idx, fd)| FileInfo {
                idx,
                path: fd.filename.iter_components().collect::<Vec<_>>().join("/"),
                len: fd.len,
            })
            .collect();
        let info_hash = list.info_hash.as_string();
        let mut peers = peers.to_vec();
        peers.extend(list.seen_peers.iter().copied());
        let torrent = Arc::new(Torrent {
            dir: self.inner.cfg.data_dir.join(&info_hash),
            info_hash,
            torrent_bytes: list.torrent_bytes.to_vec(),
            files,
            piece_len: list.info.lengths().default_piece_length(),
            peers,
            metadata_ms,
            live: tokio::sync::Mutex::new(None),
            hold: Mutex::default(),
            readers: AtomicUsize::new(0),
            last_used: Mutex::new(Instant::now()),
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
        let t = self.torrent(h)?;
        let idx = pick::pick(&t.files, hint).ok_or(EngineError::NoFile)?;
        // The budget round runs before `live` is held for good: a round
        // holds the round lock while it takes an evicted torrent's `live`
        // (engine review r4, P3-1).
        let starts = t.live.lock().await.is_none();
        if starts {
            self.enforce_budget(Some(h)).await;
        }
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
        let mut sched = self.inner.cfg.device_class.sched();
        sched.readahead_bytes = sched
            .readahead_bytes
            .min(self.inner.cfg.cache_budget_bytes / 4);
        let iso_bmff = t.files.iter().map(|f| iso_bmff(&f.path)).collect();
        let tr = adapter::Translator::new(t.piece_len, &lens, sched, self.inner.cfg.dwh)
            .with_iso_bmff(iso_bmff);
        let dwh = tr.drives();
        let opts = AddTorrentOptions {
            only_files: Some(vec![idx]),
            overwrite: true,
            output_folder: Some(t.dir.to_string_lossy().into_owned()),
            initial_peers: Some(t.peers.clone()),
            ext_sched: Some(ExtSched {
                tx: ext_tx,
                drive: dwh,
            }),
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
        let snapshot = Arc::default();
        tokio::spawn(adapter::run(
            managed.clone(),
            tr,
            ext_rx,
            msg_rx,
            Arc::clone(&snapshot),
        ));
        *lock(&t.last_used) = Instant::now();
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
        s.refetched_bytes = snap.refetched_bytes;
        s.unchoked_ms = snap.unchoked_us.map(|us| us / 1000);
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
            let trash = t.dir.with_extension(SET_ASIDE);
            // One an interrupted round left (only ever ours).
            let _ = tokio::fs::remove_dir_all(&trash).await;
            let aside = tokio::fs::rename(&t.dir, &trash).await.is_ok();
            Some(if aside { trash } else { t.dir.clone() })
        };
        if let Some(l) = t.live.lock().await.take() {
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
        };
        if shared {
            return Ok(true);
        }
        if let Some(l) = live.as_ref().filter(|l| !l.managed.is_paused()) {
            self.inner.session.pause(&l.managed).await?;
            let _ = l.msgs.send(adapter::Msg::Stopped);
        }
        Ok(true)
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
    /// allocated size fits the budget. `keep`, a torrent with a range open
    /// and a claimed one are never deleted. Folders an earlier launch left
    /// count too and go first, the oldest first: they were used before
    /// anything this launch (engine review r2, P2-4).
    async fn enforce_budget(&self, keep: Option<TorrentHandle>) {
        let _round = self.inner.evicting.lock().await;
        let all: Vec<(TorrentHandle, Arc<Torrent>)> = lock(&self.inner.torrents)
            .iter()
            .map(|(h, t)| (*h, t.clone()))
            .collect();
        // Sizes, dates and the folder listing walk the disk: off the
        // runtime workers, which the app has two of (engine review r3,
        // P3-2).
        let dirs: Vec<(Instant, TorrentHandle, PathBuf)> = all
            .iter()
            .map(|(h, t)| (*lock(&t.last_used), *h, t.dir.clone()))
            .collect();
        let data_dir = self.inner.cfg.data_dir.clone();
        let scan = tokio::task::spawn_blocking(move || {
            let held: Vec<&Path> = dirs.iter().map(|d| d.2.as_path()).collect();
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
        for dir in trash {
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
        let mut total: u64 = used.iter().map(|u| u.2).sum();
        total += left.iter().map(|l| l.2).sum::<u64>();
        for (_, dir, bytes) in left {
            if total <= self.inner.cfg.cache_budget_bytes {
                break;
            }
            if let Some(trash) = self.set_aside(&dir) {
                let _ = tokio::fs::remove_dir_all(&trash).await;
                total -= bytes;
            }
        }
        used.sort_unstable_by_key(|u| u.0);
        for (_, h, bytes) in used {
            if total <= self.inner.cfg.cache_budget_bytes {
                break;
            }
            let Some(t) = all.iter().find(|(x, _)| *x == h).map(|(_, t)| t) else {
                continue;
            };
            if Some(h) == keep || t.readers.load(Ordering::Relaxed) > 0 || t.claimed() || bytes == 0
            {
                continue;
            }
            #[cfg(feature = "test-hooks")]
            test_hooks::wait_eviction(self).await;
            {
                // Again under the lock a claim counts under: one may have
                // come since the check above (engine review r1).
                let mut torrents = lock(&self.inner.torrents);
                if t.readers.load(Ordering::Relaxed) > 0 || t.claimed() {
                    continue;
                }
                torrents.remove(&h);
            }
            match self.drop_torrent(t, false).await {
                Ok(()) => total -= bytes,
                Err(e) => tracing::warn!("evicting {}: {e}", t.info_hash),
            }
        }
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
        // Its pieces go with it: a later add of the hash checks afresh.
        let _ = std::fs::rename(dir.with_extension(HAVE), trash.join(HAVE));
        Some(trash)
    }
}

/// The ending of a torrent folder on its way out.
const SET_ASIDE: &str = "evicted";

/// The ending of a torrent's have-bitfield, beside its folder (the
/// librqbit fork's fast resume).
const HAVE: &str = "have";

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
            if t.readers.load(Ordering::Relaxed) > 0 || lock(&t.last_used).elapsed() < KEEP_WARM {
                continue;
            }
            let live = t.live.lock().await;
            if let Some(l) = live.as_ref().filter(|l| !l.managed.is_paused()) {
                if let Err(e) = engine.inner.session.pause(&l.managed).await {
                    tracing::warn!("pausing idle torrent: {e:#}");
                }
                let _ = l.msgs.send(adapter::Msg::Stopped);
            }
        }
        engine.enforce_budget(None).await;
    }
}

/// Bytes actually allocated on disk (sparse files count what is written).
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

/// The defaults first when `defaults`, then at most `MAX_OWN` of the
/// stream's own trackers, each once. Discovery is off only in loopback
/// tests, which keep exactly theirs.
fn magnet_uri(hash: &str, trackers: &[String], defaults: bool) -> String {
    let extra = DEFAULT_TRACKERS.into_iter().filter(|_| defaults);
    let given = trackers
        .iter()
        .map(|t| t.strip_prefix("tracker:").unwrap_or(t))
        .filter(|t| t.contains("://"));
    let mut seen = std::collections::HashSet::new();
    let mut magnet = format!("magnet:?xt=urn:btih:{hash}");
    let mut push = |t: &str| {
        let new = seen.insert(t.to_owned());
        if new {
            magnet.push_str("&tr=");
            magnet.push_str(&percent_encode(t));
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
    magnet
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
            async move { engine.enforce_budget(None).await }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(older.exists(), "a round ran beside the running one");
        drop(running);
        next.await.unwrap();
        assert!(!older.exists(), "the older leftover stayed");
        assert!(newer.exists(), "the newer leftover went too");
    }
}
