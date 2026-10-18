//! Embedded torrent streaming engine: a thin librqbit fork for the protocol
//! (DHT, trackers, PEX, LSD, uTP, UPnP, storage, hashing) with the DWH
//! scheduler (`mq-engine-sched`) choosing every block of a streamed torrent.
//!
//! ```text
//! let engine = Engine::start(EngineConfig::new(dir)).await?;
//! let t = engine.add_magnet(info_hash, &trackers, &[]).await?;   // metadata
//! let file = engine.select_file(t, &FileHint::Episode { season: 1, episode: 2 }).await?;
//! let reader = engine.open_reader(t, file).await?;
//! let mut range = reader.range(start, None).await?;  // RangeOpened
//! range.ready().await?;                               // blocking window + tail
//! range.read(&mut buf).await?;                        // verified bytes only
//! ```

mod adapter;
mod pick;
mod reader;

/// The fork itself, for tools and tests (creating a torrent to seed).
pub use librqbit;
pub use pick::FileHint;
pub use reader::{RangeReader, StreamReader};

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, DhtSessionConfig,
    ExtSched, ListenerMode, ListenerOptions, ManagedTorrent, Session, SessionOptions,
    generate_azereus_style,
};
use mq_engine_sched::{Config as SchedConfig, DemandId};
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
    /// DHT, trackers, LSD and UPnP. Off only for tests on loopback.
    pub discovery: bool,
}

impl EngineConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            cache_budget_bytes: 4 << 30,
            listen_port: None,
            upload: true,
            device_class: DeviceClass::Standard,
            dwh: true,
            discovery: true,
        }
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
    pub snubs: u64,
    pub seeks: u64,
    /// Ranges open on the torrent, each one scheduler demand.
    pub readers: usize,
}

const KEEP_WARM: Duration = Duration::from_secs(60);
const MAINTENANCE_EVERY: Duration = Duration::from_secs(5);

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
    torrents: Mutex<HashMap<TorrentHandle, Arc<Torrent>>>,
    next: AtomicU64,
    next_demand: AtomicU64,
}

pub(crate) struct Torrent {
    info_hash: String,
    torrent_bytes: Vec<u8>,
    files: Vec<FileInfo>,
    piece_len: u32,
    peers: Vec<SocketAddr>,
    metadata_ms: u64,
    dir: PathBuf,
    live: tokio::sync::Mutex<Option<Live>>,
    readers: AtomicUsize,
    last_used: Mutex<Instant>,
}

pub(crate) struct Live {
    managed: Arc<ManagedTorrent>,
    msgs: mpsc::UnboundedSender<adapter::Msg>,
    snapshot: Arc<Mutex<adapter::Snapshot>>,
    dwh: bool,
    selected: usize,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Engine {
    /// One session for the process: shared DHT (persisted in `data_dir`),
    /// inbound TCP and uTP listener with UPnP, PEX, IPv6 dual stack.
    pub async fn start(cfg: EngineConfig) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let version = env!("CARGO_PKG_VERSION");
        let v: Vec<u8> = version.split('.').filter_map(|p| p.parse().ok()).collect();
        let dht = cfg.discovery.then(|| DhtSessionConfig {
            persistence: Some(DhtPersistenceConfig {
                config_filename: Some(cfg.data_dir.join("dht.json")),
                ..Default::default()
            }),
            ..Default::default()
        });
        let opts = SessionOptions {
            dht,
            disable_trackers: !cfg.discovery,
            disable_local_service_discovery: !cfg.discovery,
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
                enable_upnp_port_forwarding: cfg.discovery,
                ..Default::default()
            }),
            connect: Some(ConnectionOptions::default()),
            peer_limit: Some(cfg.device_class.peer_limit()),
            disable_upload: !cfg.upload,
            ..Default::default()
        };
        let session = Session::new_with_opts(cfg.data_dir.clone(), opts).await?;
        let engine = Self {
            inner: Arc::new(Inner {
                session,
                cfg,
                torrents: Mutex::default(),
                next: AtomicU64::new(1),
                next_demand: AtomicU64::new(1),
            }),
        };
        tokio::spawn(maintenance(Arc::downgrade(&engine.inner)));
        Ok(engine)
    }

    /// Where other peers reach us (tests point a leecher at a seeder with it).
    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.inner.session.listen_addr()
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
        let mut magnet = format!("magnet:?xt=urn:btih:{hash}");
        for t in trackers {
            let t = t.strip_prefix("tracker:").unwrap_or(t);
            if t.contains("://") {
                magnet.push_str("&tr=");
                magnet.push_str(&percent_encode(t));
            }
        }
        self.add(AddTorrent::from_url(magnet), peers).await
    }

    /// Adds a torrent from `.torrent` bytes. A torrent whose data already
    /// sits in `data_dir/<info hash>/` is verified and seeded once selected.
    pub async fn add_torrent_bytes(
        &self,
        bytes: Vec<u8>,
        peers: &[SocketAddr],
    ) -> Result<TorrentHandle> {
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

    /// Picks the file to play and starts the torrent on it (or switches the
    /// running torrent to it). Only that file is wanted; with DWH nothing
    /// beyond the readahead budget downloads unless a reader asks.
    pub async fn select_file(&self, h: TorrentHandle, hint: &FileHint) -> Result<usize> {
        let t = self.torrent(h)?;
        let idx = pick::pick(&t.files, hint).ok_or(EngineError::NoFile)?;
        let mut live = t.live.lock().await;
        if let Some(l) = live.as_mut() {
            if l.selected != idx {
                let only = HashSet::from([idx]);
                self.inner
                    .session
                    .update_only_files(&l.managed, &only)
                    .await?;
                l.selected = idx;
            }
            return Ok(idx);
        }
        self.enforce_budget(Some(h)).await;
        let (ext_tx, ext_rx) = mpsc::unbounded_channel();
        let lens: Vec<u64> = t.files.iter().map(|f| f.len).collect();
        let mut sched = self.inner.cfg.device_class.sched();
        sched.readahead_bytes = sched
            .readahead_bytes
            .min(self.inner.cfg.cache_budget_bytes / 4);
        let tr = adapter::Translator::new(t.piece_len, &lens, sched, self.inner.cfg.dwh);
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
            selected: idx,
        });
        Ok(idx)
    }

    /// A reader for one file; selects it first when nothing was selected.
    pub async fn open_reader(&self, h: TorrentHandle, file: usize) -> Result<StreamReader> {
        let t = self.torrent(h)?;
        let info = t.files.get(file).cloned().ok_or(EngineError::NoFile)?;
        if t.live.lock().await.is_none() {
            self.select_file(h, &FileHint::Index(file)).await?;
        }
        Ok(StreamReader::new(self.clone(), t, info))
    }

    pub async fn stats(&self, h: TorrentHandle) -> Result<TorrentStats> {
        let t = self.torrent(h)?;
        let live = t.live.lock().await;
        let mut s = TorrentStats {
            metadata_ms: t.metadata_ms,
            readers: t.readers.load(Ordering::Relaxed),
            ..Default::default()
        };
        let Some(l) = live.as_ref() else {
            return Ok(s);
        };
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

    /// Stops the torrent; `keep_data: false` also deletes its files.
    pub async fn remove(&self, h: TorrentHandle, keep_data: bool) -> Result<()> {
        let t = lock(&self.inner.torrents)
            .remove(&h)
            .ok_or(EngineError::UnknownTorrent)?;
        self.drop_torrent(&t, keep_data).await
    }

    async fn drop_torrent(&self, t: &Torrent, keep_data: bool) -> Result<()> {
        if let Some(l) = t.live.lock().await.take() {
            self.inner
                .session
                .delete(l.managed.id().into(), false)
                .await?;
        }
        if !keep_data {
            match tokio::fs::remove_dir_all(&t.dir).await {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(())
    }

    pub(crate) fn next_demand(&self) -> DemandId {
        self.inner.next_demand.fetch_add(1, Ordering::Relaxed)
    }

    /// Unpauses a torrent whose keep-warm window ran out.
    pub(crate) async fn wake(
        &self,
        t: &Torrent,
    ) -> Result<Option<(Arc<ManagedTorrent>, mpsc::UnboundedSender<adapter::Msg>)>> {
        let live = t.live.lock().await;
        let Some(l) = live.as_ref() else {
            return Ok(None);
        };
        if l.managed.is_paused() {
            self.inner.session.unpause(&l.managed).await?;
        }
        Ok(Some((l.managed.clone(), l.msgs.clone())))
    }

    /// Deletes whole idle torrents, least recently used first, until the
    /// allocated size fits the budget. `keep` is never deleted.
    async fn enforce_budget(&self, keep: Option<TorrentHandle>) {
        let all: Vec<(TorrentHandle, Arc<Torrent>)> = lock(&self.inner.torrents)
            .iter()
            .map(|(h, t)| (*h, t.clone()))
            .collect();
        let mut used: Vec<(Instant, TorrentHandle, u64)> = all
            .iter()
            .map(|(h, t)| (*lock(&t.last_used), *h, allocated(&t.dir)))
            .collect();
        let mut total: u64 = used.iter().map(|u| u.2).sum();
        used.sort_unstable_by_key(|u| u.0);
        for (_, h, bytes) in used {
            if total <= self.inner.cfg.cache_budget_bytes {
                break;
            }
            let Some(t) = all.iter().find(|(x, _)| *x == h).map(|(_, t)| t) else {
                continue;
            };
            if Some(h) == keep || t.readers.load(Ordering::Relaxed) > 0 || bytes == 0 {
                continue;
            }
            lock(&self.inner.torrents).remove(&h);
            match self.drop_torrent(t, false).await {
                Ok(()) => total -= bytes,
                Err(e) => tracing::warn!("evicting {}: {e}", t.info_hash),
            }
        }
    }
}

/// Keep-warm expiry and the disk budget, every few seconds.
async fn maintenance(inner: std::sync::Weak<Inner>) {
    loop {
        tokio::time::sleep(MAINTENANCE_EVERY).await;
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
