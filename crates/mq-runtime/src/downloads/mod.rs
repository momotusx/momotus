//! Offline downloads (plans/offline-downloads.md): one job per video, kept
//! whole on this device with what its player needs, and played with no
//! network through the stream server like any source.
//!
//! The manager owns the queue: up to two jobs run at once, in queue order,
//! while the device allows it (a connection, Wi-Fi when asked, the app's
//! say, no stream playing from the network). Rows live in the library's
//! `downloads` table; what changes by the second (bytes, speed) lives here
//! and is published with them at most twice a second.

mod http;
mod subtitles;
#[cfg(feature = "torrent")]
mod torrent;

use crate::core::WeakCore;
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use mq_library::DownloadRecord;
use mq_streams::ParsedStream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, watch};
use tokio_util::sync::CancellationToken;

/// Jobs that run at once (decision 9): one dead source never holds the
/// whole queue.
const RUNNING: usize = 2;
/// Free space a download leaves (decision 11).
pub(crate) const RESERVE: u64 = 1 << 30;
/// Under this much free space a running download waits (4.9).
const FLOOR: u64 = 512 << 20;
/// How often the board is published while bytes move.
const PUBLISH_EVERY: Duration = Duration::from_millis(500);
/// A reported free-space figure older than this is unknown (4.9).
const REPORT_FRESH: Duration = Duration::from_secs(120);
/// Moves to the next source of a pick before a job fails (4.3).
const MAX_MOVES: i64 = 2;
/// The first wait before a subtitle step no source answered asks again.
const SUBTITLE_WAIT: Duration = Duration::from_secs(10 * 60);
const SUBTITLE_WAIT_MAX: Duration = Duration::from_secs(6 * 60 * 60);

/// Where a download stands.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    Queued,
    Running,
    Paused,
    Waiting,
    Done,
    Failed,
    /// Its file is gone from the disk: "Download again".
    Missing,
}

/// Why a download waits or failed.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadReason {
    /// Wi-Fi only, on a metered network.
    Wifi,
    /// Low Data Mode is on.
    LowData,
    Space,
    /// No connection.
    Network,
    /// A stream plays from the network.
    Playing,
    /// The platform holds downloads (the app left the foreground where
    /// that stops them).
    App,
    /// No version of this video can be downloaded.
    NoVersion,
    /// The source stopped answering.
    Source,
    /// Interrupted too many times.
    Retries,
    /// The file was not a video.
    NotVideo,
}

impl DownloadState {
    fn word(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Waiting => "waiting",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Missing => "missing",
        }
    }

    fn read(word: &str) -> Self {
        serde_json::from_value(serde_json::Value::String(word.to_owned())).unwrap_or(Self::Queued)
    }
}

impl DownloadReason {
    fn word(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    fn read(word: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(word.to_owned())).ok()
    }
}

/// What the Downloads screen, the versions params and the player need with
/// no addon (4.1 `facts`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Facts {
    pub name: String,
    pub episode_title: Option<String>,
    pub poster: Option<String>,
    pub still: Option<String>,
    pub runtime: Option<String>,
    pub release_info: Option<String>,
    pub released: Option<String>,
    pub episodes_in_season: Option<u32>,
}

/// A source as it was ranked, kept so a queued or resumed job, and a play
/// with no addon, rebuild it exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredStream {
    stream: ParsedStream,
    #[serde(default)]
    size_is_file: bool,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    subtitles: Vec<mq_protocol::WireSubtitle>,
    #[serde(default)]
    addon_url: Option<url::Url>,
}

impl StoredStream {
    pub(crate) fn of(stream: &ParsedStream) -> Self {
        Self {
            stream: stream.clone(),
            size_is_file: stream.size_is_file,
            headers: stream.headers.clone(),
            subtitles: stream.subtitles.clone(),
            addon_url: stream.addon_url.clone(),
        }
    }

    pub(crate) fn stream(&self) -> ParsedStream {
        let mut stream = self.stream.clone();
        stream.size_is_file = self.size_is_file;
        stream.headers = self.headers.clone();
        stream.subtitles = self.subtitles.clone();
        stream.addon_url = self.addon_url.clone();
        stream
    }
}

/// The version a job keeps: its pick and the pick's failover chain, the
/// source in use being `chain[moves]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Version {
    pub(crate) key: String,
    pub(crate) chain: Vec<StoredStream>,
    /// The file's extension on disk ("mkv"), once its first answer named it.
    #[serde(default)]
    pub(crate) ext: Option<String>,
    /// A torrent the engine keeps: its file, once chosen.
    #[serde(default)]
    pub(crate) torrent: Option<TorrentRef>,
    /// The first answer's `ETag` or `Last-Modified`, sent as `If-Range` on
    /// a resume: a changed file answers whole instead of a wrong range.
    #[serde(default)]
    pub(crate) validator: Option<String>,
}

/// A downloaded file of a torrent, in the engine's folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TorrentRef {
    /// Lowercase info hash.
    pub(crate) hash: String,
    pub(crate) file: usize,
    /// Its path in the torrent.
    pub(crate) name: String,
}

/// One download as the screens show it.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRow {
    pub id: String,
    pub kind: String,
    pub item_id: String,
    pub video_id: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub name: String,
    pub episode_title: Option<String>,
    pub poster: Option<String>,
    pub still: Option<String>,
    pub runtime: Option<String>,
    /// The episode's air date ("2013-09-16").
    pub released: Option<String>,
    pub state: DownloadState,
    pub reason: Option<DownloadReason>,
    /// Released onto mobile data by the user.
    pub mobile_data: bool,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub bytes_done: u64,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub bytes_total: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub speed_bps: Option<u64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub eta_secs: Option<u64>,
    /// "1080p", from the version kept.
    pub quality: Option<String>,
    /// The languages of the subtitles kept with it, as tags.
    pub subtitles: Vec<String>,
}

/// The download control's state for one video (3.1).
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBadge {
    pub id: String,
    pub state: DownloadState,
    pub reason: Option<DownloadReason>,
    pub mobile_data: bool,
    /// 0 to 1, when the size is known.
    pub progress: Option<f32>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub bytes_total: Option<u64>,
}

impl DownloadRow {
    pub fn badge(&self) -> DownloadBadge {
        DownloadBadge {
            id: self.id.clone(),
            state: self.state,
            reason: self.reason,
            mobile_data: self.mobile_data,
            progress: self
                .bytes_total
                .filter(|t| *t > 0)
                .map(|t| (self.bytes_done as f64 / t as f64).clamp(0.0, 1.0) as f32),
            bytes_total: self.bytes_total,
        }
    }
}

/// The device's downloads settings (3.7), kept in `device_state`.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadSettings {
    pub wifi_only: bool,
    pub smart: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            wifi_only: true,
            smart: true,
        }
    }
}

const SETTINGS_KEY: &str = "downloads";

/// The connection as the apps report it (4.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Link {
    pub offline: bool,
    pub metered: bool,
    pub low_data: bool,
}

/// What the platform says (`downloadsAllowed`).
#[derive(Debug, Clone, Copy)]
struct Allowed {
    allowed: bool,
    free: Option<(u64, std::time::Instant)>,
}

/// Everything the screens read, published as one value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Board {
    /// Queue order.
    pub rows: Vec<DownloadRow>,
    pub settings: DownloadSettings,
    pub link: LinkWords,
    /// Bytes a waiting-for-space job still needs.
    pub needed_bytes: Option<u64>,
}

/// `Link` as the board carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkWords {
    pub offline: bool,
}

/// A running job's handles.
struct Running {
    cancel: CancellationToken,
    /// Bytes on disk from the start, for a play of the file while it is
    /// written (`Upstream::File::held`).
    held: watch::Sender<u64>,
    /// Bytes this run received, for its speed.
    total: Option<u64>,
    speed: Speed,
    /// A torrent's connected peers at its last look.
    peers: usize,
}

/// Bytes over the last seconds.
#[derive(Default)]
struct Speed {
    samples: std::collections::VecDeque<(std::time::Instant, u64)>,
}

impl Speed {
    fn add(&mut self, bytes: u64) {
        let now = std::time::Instant::now();
        self.samples.push_back((now, bytes));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > Duration::from_secs(5))
        {
            self.samples.pop_front();
        }
    }

    fn bps(&self) -> Option<u64> {
        let first = self.samples.front()?.0;
        let span = first.elapsed().as_secs_f64().max(1.0);
        let bytes: u64 = self.samples.iter().map(|(_, b)| b).sum();
        Some((bytes as f64 / span) as u64)
    }
}

/// A file a download keeps, for a play (4.12).
pub(crate) struct Local {
    pub(crate) path: PathBuf,
    pub(crate) len: u64,
    /// For a file still being written: what is on disk, closed when its job
    /// is not running.
    pub(crate) held: Option<watch::Receiver<u64>>,
    pub(crate) file_name: String,
}

/// The device's downloads: the queue, its runs and what they show.
pub struct Downloads {
    core: WeakCore,
    dir: PathBuf,
    board: SharedObservable<Board>,
    running: Mutex<HashMap<String, Running>>,
    link: Mutex<Link>,
    allowed: Mutex<Allowed>,
    /// Network playbacks right now (the playing rule, 4.4), each with the
    /// torrent it plays, if one.
    streams: Mutex<HashMap<u64, Option<String>>>,
    next_stream: std::sync::atomic::AtomicU64,
    wake: Arc<Notify>,
    started: std::sync::Once,
    /// Jobs that met a space error: admitted on the disk's own figure until
    /// it grows past their need, so a system that has not purged yet never
    /// loops them (4.9, engine r2 P2-4).
    strict: Mutex<std::collections::HashSet<String>>,
    /// When each job began waiting for space: one whose size is unknown
    /// looks again after a minute, not at every wake.
    space_since: Mutex<HashMap<String, std::time::Instant>>,
    /// Downloads whose subtitles are being fetched.
    subtitling: Mutex<std::collections::HashSet<String>>,
    /// Subtitle steps no source answered: when each asks again, and the
    /// wait after that (code review r1, runtime P2-3).
    subtitle_waits: Mutex<HashMap<String, (std::time::Instant, Duration)>>,
    /// Smart steps whose next episode is being looked up (4.14).
    smart_looking: Mutex<std::collections::HashSet<String>>,
    /// The loopback URLs of downloads' posters and stills (4.11), by file,
    /// with the server port they were made on.
    images: Mutex<HashMap<PathBuf, (u16, String, mq_server::Lease)>>,
}

/// A smart download's step, kept on the episode it finished (4.14).
#[derive(Debug, Serialize, Deserialize)]
struct SmartStep {
    /// The profile that finished it.
    by: String,
    /// The next episode's download id once known; empty when there is no
    /// next episode; none while it was not looked up (or could not be).
    #[serde(default)]
    next: Option<String>,
}

/// Held by a playback streaming from the network: downloads wait while
/// one lives, but a torrent's own download while it plays (its scheduler
/// puts the reader first, 4.4).
pub struct Streaming {
    id: u64,
    downloads: std::sync::Weak<Downloads>,
}

impl Drop for Streaming {
    fn drop(&mut self) {
        if let Some(d) = self.downloads.upgrade() {
            lock(&d.streams).remove(&self.id);
            d.wake.notify_one();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Downloads {
    pub(crate) fn new(core: WeakCore, dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            core,
            dir,
            board: SharedObservable::new(Board::default()),
            running: Mutex::default(),
            link: Mutex::default(),
            allowed: Mutex::new(Allowed {
                allowed: true,
                free: None,
            }),
            streams: Mutex::default(),
            next_stream: std::sync::atomic::AtomicU64::new(1),
            wake: Arc::new(Notify::new()),
            started: std::sync::Once::new(),
            strict: Mutex::default(),
            space_since: Mutex::default(),
            subtitling: Mutex::default(),
            subtitle_waits: Mutex::default(),
            smart_looking: Mutex::default(),
            images: Mutex::default(),
        })
    }

    /// Starts the queue's task once, on the caller's runtime: a launch
    /// resumes what a kill or a reboot left (`running` reads as queued).
    pub(crate) fn start(self: &Arc<Self>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        self.started.call_once(|| {
            let me = Arc::downgrade(self);
            handle.spawn(async move {
                if let Some(d) = me.upgrade() {
                    d.recover();
                }
                run_queue(me).await;
            });
        });
    }

    fn core(&self) -> Option<Core> {
        self.core.upgrade()
    }

    /// The board as it is, and its changes.
    pub fn board(&self) -> &SharedObservable<Board> {
        &self.board
    }

    /// The download folder of job `id`.
    fn job_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    pub fn settings(&self) -> DownloadSettings {
        self.core()
            .and_then(|c| c.library().device_value(SETTINGS_KEY).ok().flatten())
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    pub fn set_settings(
        &self,
        wifi_only: Option<bool>,
        smart: Option<bool>,
    ) -> Result<(), CoreError> {
        let Some(core) = self.core() else {
            return Ok(());
        };
        let mut s = self.settings();
        s.wifi_only = wifi_only.unwrap_or(s.wifi_only);
        s.smart = smart.unwrap_or(s.smart);
        let value = serde_json::to_string(&s).map_err(|e| CoreError::Downloads(e.to_string()))?;
        core.library().set_device_value(SETTINGS_KEY, &value)?;
        self.changed();
        Ok(())
    }

    pub fn set_link(&self, link: Link) {
        let was = std::mem::replace(&mut *lock(&self.link), link);
        // Back online: a subtitle step that found no source asks at once.
        if was.offline && !link.offline {
            lock(&self.subtitle_waits).clear();
        }
        self.changed();
    }

    pub fn link(&self) -> Link {
        *lock(&self.link)
    }

    /// The platform's say and its free-space figure (`downloadsAllowed`).
    pub fn set_allowed(&self, allowed: bool, free: Option<u64>) {
        let mut a = lock(&self.allowed);
        a.allowed = allowed;
        if let Some(free) = free {
            a.free = Some((free, std::time::Instant::now()));
        }
        drop(a);
        self.changed();
    }

    /// A playback streams from the network until the guard drops; `torrent`
    /// is the info hash it plays, if a torrent.
    pub fn streaming(self: &Arc<Self>, torrent: Option<&str>) -> Streaming {
        let id = self
            .next_stream
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        lock(&self.streams).insert(id, torrent.map(str::to_ascii_lowercase));
        self.wake.notify_one();
        Streaming {
            id,
            downloads: Arc::downgrade(self),
        }
    }

    /// Something the queue reads changed: it looks again, and the board
    /// shows it.
    fn changed(&self) {
        self.wake.notify_one();
        self.publish();
    }

    /// A launch: jobs a kill or a reboot left running go back in the queue,
    /// a done file that is gone turns `missing` (4.8), and folders no row
    /// names (a wipe, a crash between a delete and its files) go.
    fn recover(&self) {
        let Some(core) = self.core() else { return };
        // The folders first, then the rows: a job added in between has its
        // row in the list, so its new folder is never taken for a leftover
        // (code review r1 P3-1).
        let folders: Vec<(String, PathBuf)> = std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
                    .collect()
            })
            .unwrap_or_default();
        let Ok(rows) = core.library().downloads() else {
            return;
        };
        for mut row in rows.iter().cloned() {
            let state = DownloadState::read(&row.state);
            let fix = match state {
                DownloadState::Running => Some(DownloadState::Queued),
                // A torrent whose folder is there goes on from what it
                // holds; anything else is gone.
                DownloadState::Done if !self.complete(&row) => Some(if self.folder_there(&row) {
                    DownloadState::Queued
                } else {
                    DownloadState::Missing
                }),
                _ => None,
            };
            if let Some(fix) = fix {
                row.state = fix.word().to_owned();
                row.reason = None;
                let _ = core.library().save_download(&row);
            }
        }
        for (name, path) in folders {
            if !rows.iter().any(|r| r.id == name) {
                let _ = std::fs::remove_dir_all(path);
            }
        }
        self.publish();
    }

    /// A done download's file is all there (4.8): its length is the size
    /// its source gave.
    fn complete(&self, row: &DownloadRecord) -> bool {
        #[cfg(feature = "torrent")]
        if let Some(t) = self.torrent_ref(row) {
            let Some(core) = self.core() else {
                return false;
            };
            return mq_engine::complete_on_disk(core.torrent_dir(), &t.hash, t.file);
        }
        let Some(local) = self.file_of(row) else {
            return false;
        };
        std::fs::metadata(&local)
            .is_ok_and(|m| Some(m.len() as i64) == row.bytes_total && row.bytes_total.is_some())
    }

    /// The torrent file a download keeps, once chosen.
    fn torrent_ref(&self, row: &DownloadRecord) -> Option<TorrentRef> {
        serde_json::from_str::<Version>(row.version.as_deref()?)
            .ok()?
            .torrent
    }

    /// A torrent download's folder is there (its data may only lag its
    /// bitfield).
    fn folder_there(&self, row: &DownloadRecord) -> bool {
        #[cfg(feature = "torrent")]
        if let (Some(t), Some(core)) = (self.torrent_ref(row), self.core()) {
            return core.torrent_dir().join(&t.hash).exists();
        }
        let _ = row;
        false
    }

    /// Every torrent file downloads keep, for the engine's start: none of
    /// them is ever swept (4.6).
    #[cfg(feature = "torrent")]
    pub(crate) fn kept_torrents(&self) -> Vec<(String, usize)> {
        let Some(core) = self.core() else {
            return Vec::new();
        };
        core.library()
            .downloads()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| self.torrent_ref(r))
            .map(|t| (t.hash, t.file))
            .collect()
    }

    /// The video file of a download, where it is or will be.
    fn file_of(&self, row: &DownloadRecord) -> Option<PathBuf> {
        #[cfg(feature = "torrent")]
        if let (Some(t), Some(core)) = (self.torrent_ref(row), self.core()) {
            return Some(core.torrent_dir().join(&t.hash).join(&t.name));
        }
        let version: Version = serde_json::from_str(row.version.as_deref()?).ok()?;
        let ext = version.ext.unwrap_or_else(|| "mkv".into());
        let name = if DownloadState::read(&row.state) == DownloadState::Done {
            format!("video.{ext}")
        } else {
            format!("video.{ext}.part")
        };
        Some(self.job_dir(&row.id).join(name))
    }

    /// Recomputes what the screens show from the rows and the runs.
    fn publish(&self) {
        let Some(core) = self.core() else { return };
        let rows = core.library().downloads().unwrap_or_default();
        let running = lock(&self.running);
        let images: HashMap<PathBuf, String> = lock(&self.images)
            .iter()
            .map(|(path, (_, url, _))| (path.clone(), url.clone()))
            .collect();
        let shown: Vec<DownloadRow> = rows
            .iter()
            .map(|r| {
                let mut row = row_of(r, running.get(&r.id));
                // The kept copy, which plays with no network.
                let dir = self.job_dir(&r.id);
                for (slot, name) in [(&mut row.poster, "poster"), (&mut row.still, "still")] {
                    if let Some(url) = images.get(&dir.join(name)) {
                        *slot = Some(url.clone());
                    }
                }
                row
            })
            .collect();
        drop(running);
        let needed = shown
            .iter()
            .filter(|r| {
                r.state == DownloadState::Waiting && r.reason == Some(DownloadReason::Space)
            })
            .filter_map(|r| r.bytes_total.map(|t| t.saturating_sub(r.bytes_done)))
            .max();
        let board = Board {
            rows: shown,
            settings: self.settings(),
            link: LinkWords {
                offline: self.link().offline,
            },
            needed_bytes: needed,
        };
        if self.board.get() != board {
            self.board.set(board);
        }
    }

    /// Adds `videos` of a title for the session's profile, each at the end
    /// of the queue; a video that already has a download keeps it (a failed
    /// or missing one is queued again). The ids, in order.
    pub fn add(
        &self,
        profile: &str,
        kind: &str,
        item_id: &str,
        videos: Vec<AddVideo>,
        request: serde_json::Value,
        pick: Option<Vec<ParsedStream>>,
    ) -> Result<Vec<String>, CoreError> {
        let Some(core) = self.core() else {
            return Ok(Vec::new());
        };
        let mut ids = Vec::new();
        for video in videos {
            let version = pick.as_ref().and_then(|chain| {
                let first = chain.first()?;
                serde_json::to_string(&Version {
                    key: first.key.clone(),
                    chain: chain.iter().map(StoredStream::of).collect(),
                    ext: None,
                    torrent: None,
                    validator: None,
                })
                .ok()
            });
            let record = DownloadRecord {
                id: new_id(),
                added_by: profile.to_owned(),
                kind: kind.to_owned(),
                item_id: item_id.to_owned(),
                video_id: video.video_id.clone(),
                season: video.season.map(i64::from),
                episode: video.episode.map(i64::from),
                facts: serde_json::to_string(&video.facts)
                    .map_err(|e| CoreError::Downloads(e.to_string()))?,
                request: request.to_string(),
                version,
                state: DownloadState::Queued.word().into(),
                reason: None,
                mobile_data: false,
                hand_picked: pick.is_some(),
                bytes_done: 0,
                bytes_total: None,
                moves: 0,
                subtitles: "[]".into(),
                subtitles_state: "pending".into(),
                position: 0,
                created_ms: mq_library::hlc::now_ms() as i64,
                done_ms: None,
                smart: None,
            };
            let mut stored = core.library().add_download(&record)?;
            let state = DownloadState::read(&stored.state);
            if matches!(state, DownloadState::Failed | DownloadState::Missing) {
                self.forget_files(&stored.id);
                stored = DownloadRecord {
                    state: DownloadState::Queued.word().into(),
                    reason: None,
                    bytes_done: 0,
                    moves: 0,
                    version: record.version.clone().or(stored.version),
                    ..stored
                };
                core.library().save_download(&stored)?;
            }
            self.fetch_images(&core, &stored.id, &video.facts);
            ids.push(stored.id);
        }
        self.changed();
        Ok(ids)
    }

    /// Pauses, resumes, retries or removes `ids`.
    pub fn act(&self, ids: &[String], action: Action) -> Result<(), CoreError> {
        let Some(core) = self.core() else {
            return Ok(());
        };
        for id in ids {
            let Some(mut row) = core
                .library()
                .downloads()?
                .into_iter()
                .find(|r| &r.id == id)
            else {
                continue;
            };
            let state = DownloadState::read(&row.state);
            match action {
                Action::Remove => {
                    self.stop(id);
                    let torrent = self.torrent_ref(&row);
                    core.library().delete_download(id)?;
                    self.forget_files(id);
                    #[cfg(feature = "torrent")]
                    if let Some(t) = torrent {
                        let core = core.clone();
                        tokio::spawn(async move {
                            match core.torrent_engine().await {
                                Ok(engine) => {
                                    if let Err(e) = engine.delete_file(&t.hash, t.file).await {
                                        tracing::warn!("download delete: {e}");
                                    }
                                }
                                Err(e) => tracing::warn!("download delete: {e}"),
                            }
                        });
                    }
                    #[cfg(not(feature = "torrent"))]
                    let _ = torrent;
                    continue;
                }
                Action::Pause => {
                    if matches!(
                        state,
                        DownloadState::Queued | DownloadState::Running | DownloadState::Waiting
                    ) {
                        self.stop(id);
                        row.state = DownloadState::Paused.word().into();
                        row.reason = None;
                    }
                }
                Action::Resume => {
                    if state == DownloadState::Paused {
                        row.state = DownloadState::Queued.word().into();
                    }
                }
                Action::Retry { mobile_data } => {
                    if matches!(state, DownloadState::Failed | DownloadState::Missing) {
                        self.forget_files(id);
                        row.state = DownloadState::Queued.word().into();
                        row.reason = None;
                        row.bytes_done = 0;
                        row.moves = 0;
                        if !row.hand_picked {
                            row.version = None;
                        }
                    }
                    if mobile_data {
                        row.mobile_data = true;
                        if state == DownloadState::Waiting {
                            row.state = DownloadState::Queued.word().into();
                            row.reason = None;
                        }
                    }
                }
            }
            core.library().save_download(&row)?;
        }
        self.changed();
        Ok(())
    }

    /// Every download id, for "all".
    pub fn ids(&self) -> Vec<String> {
        self.board.get().rows.into_iter().map(|r| r.id).collect()
    }

    /// A title's download ids.
    pub fn ids_of(&self, item_id: &str) -> Vec<String> {
        let rows = self.board.get().rows;
        rows.into_iter()
            .filter(|r| r.item_id == item_id)
            .map(|r| r.id)
            .collect()
    }

    /// Leases the posters and stills downloads keep on the stream server,
    /// again when the server moved to another port (a resume).
    async fn lease_images(&self, core: &Core) {
        let Ok(server) = core.stream_server().await else {
            return;
        };
        let port = server.addr().port();
        let rows = core.library().downloads().unwrap_or_default();
        let mut added = false;
        for r in &rows {
            for name in ["poster", "still"] {
                let path = self.job_dir(&r.id).join(name);
                let fresh = lock(&self.images)
                    .get(&path)
                    .is_some_and(|(p, _, _)| *p == port);
                if fresh {
                    continue;
                }
                let Ok(meta) = tokio::fs::metadata(&path).await else {
                    continue;
                };
                let lease = server.register(
                    mq_server::Upstream::File {
                        path: path.clone(),
                        len: meta.len(),
                        held: None,
                    },
                    &format!("{name}.jpg"),
                );
                let url = lease.url.clone();
                lock(&self.images).insert(path, (port, url, lease));
                added = true;
            }
        }
        // Gone downloads' leases go.
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        lock(&self.images).retain(|path, _| {
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .is_some_and(|n| ids.contains(&n))
        });
        if added {
            self.publish();
        }
    }

    /// Fetches the poster and the still a download shows offline, once.
    fn fetch_images(&self, core: &Core, id: &str, facts: &Facts) {
        let dir = self.job_dir(id);
        let wanted: Vec<(PathBuf, String)> = [("poster", &facts.poster), ("still", &facts.still)]
            .into_iter()
            .filter_map(|(name, url)| Some((dir.join(name), url.clone()?)))
            .filter(|(path, _)| !path.exists())
            .collect();
        if wanted.is_empty() {
            return;
        }
        let (core, wake) = (core.clone(), self.wake.clone());
        tokio::spawn(async move {
            let opts = mq_http::RequestOptions {
                timeout: std::time::Duration::from_secs(20),
                max_bytes: 8 << 20,
                ..Default::default()
            };
            for (path, url) in wanted {
                let Ok(answer) = core.http().get(&url, &opts).await else {
                    continue;
                };
                if answer.status != 200 || answer.body.is_empty() {
                    continue;
                }
                if let Some(dir) = path.parent() {
                    let _ = tokio::fs::create_dir_all(dir).await;
                }
                let _ = tokio::fs::write(&path, &answer.body).await;
            }
            wake.notify_one();
        });
    }

    fn stop(&self, id: &str) {
        if let Some(run) = lock(&self.running).remove(id) {
            run.cancel.cancel();
        }
    }

    fn forget_files(&self, id: &str) {
        let dir = self.job_dir(id);
        if dir.starts_with(&self.dir) && dir != self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// The download of `video_id` of `item_id` that can play now (4.12):
    /// its file, whole, or still being written with what it holds.
    pub(crate) fn local(&self, item_id: &str, video_id: &str) -> Option<(String, Local)> {
        let core = self.core()?;
        let row = core.library().download_of(item_id, video_id).ok()??;
        let version: Version = serde_json::from_str(row.version.as_deref()?).ok()?;
        // A torrent plays through the engine (`torrent_download`).
        if version.torrent.is_some() {
            return None;
        }
        let state = DownloadState::read(&row.state);
        let path = self.file_of(&row)?;
        let len = u64::try_from(row.bytes_total?).ok()?;
        let held = if state == DownloadState::Done {
            None
        } else {
            if row.bytes_done <= 0 {
                return None;
            }
            let running = lock(&self.running);
            Some(match running.get(&row.id) {
                Some(run) => run.held.subscribe(),
                None => {
                    // Not running: what it holds, and nothing more to come.
                    let (tx, rx) = watch::channel(row.bytes_done as u64);
                    drop(tx);
                    rx
                }
            })
        };
        let ext = version.ext.clone().unwrap_or_else(|| "mkv".into());
        Some((
            version.key,
            Local {
                path,
                len,
                held,
                file_name: format!("{}.{ext}", sanitize(&row.video_id)),
            },
        ))
    }

    /// Tests: job `id`'s folder.
    #[doc(hidden)]
    pub fn job_folder(&self, id: &str) -> PathBuf {
        self.job_dir(id)
    }

    /// Tests: what a launch does to the rows a kill or a reboot left.
    #[doc(hidden)]
    pub fn relaunch(&self) {
        self.recover();
        self.changed();
    }

    /// Tests: the file download `id` keeps, once done.
    #[doc(hidden)]
    pub fn file_path(&self, id: &str) -> Option<PathBuf> {
        let row = self
            .core()?
            .library()
            .downloads()
            .ok()?
            .into_iter()
            .find(|r| r.id == id)?;
        self.file_of(&row).filter(|p| p.exists())
    }

    /// The torrent download of `video_id` of `item_id`, with its version's
    /// key and whether it is done: it plays through the engine, from disk.
    #[cfg(feature = "torrent")]
    pub(crate) fn torrent_download(
        &self,
        item_id: &str,
        video_id: &str,
    ) -> Option<(String, TorrentRef, bool)> {
        let core = self.core()?;
        let row = core.library().download_of(item_id, video_id).ok()??;
        let version: Version = serde_json::from_str(row.version.as_deref()?).ok()?;
        let done = DownloadState::read(&row.state) == DownloadState::Done;
        Some((version.key, version.torrent?, done))
    }

    /// The stored stream of a download, rebuilt for a Versions row with no
    /// addon (4.12), and whether it is done.
    pub(crate) fn kept(
        &self,
        item_id: &str,
        video_id: &str,
    ) -> Option<(ParsedStream, DownloadRow)> {
        let core = self.core()?;
        let row = core.library().download_of(item_id, video_id).ok()??;
        let version: Version = serde_json::from_str(row.version.as_deref()?).ok()?;
        let at = usize::try_from(row.moves)
            .ok()?
            .min(version.chain.len().checked_sub(1)?);
        let mut stream = version.chain.get(at)?.stream();
        stream.key = version.key.clone();
        let shown = row_of(&row, lock(&self.running).get(&row.id));
        Some((stream, shown))
    }

    /// Job `id` met a space error.
    fn ran_out(&self, id: &str) {
        lock(&self.strict).insert(id.to_owned());
    }

    /// Tests: a free-space figure the platform reported `age` ago.
    #[doc(hidden)]
    pub fn report_free(&self, free: u64, age: Duration) {
        let at = std::time::Instant::now()
            .checked_sub(age)
            .unwrap_or_else(std::time::Instant::now);
        lock(&self.allowed).free = Some((free, at));
    }

    /// The device's free space as downloads count it: the disk's figure, or
    /// the platform's fresh report when larger (iOS counts purgeable space),
    /// for the Downloads screen and the download ranking (code review r1
    /// P3-4).
    pub(crate) fn free(&self, core: &Core) -> Option<u64> {
        self.base_free(core, "")
    }

    /// The figure the downloads' space rule starts from for job `id` (4.9).
    fn base_free(&self, core: &Core, id: &str) -> Option<u64> {
        let disk = core.free_bytes();
        if lock(&self.strict).contains(id) {
            return disk;
        }
        let reported = lock(&self.allowed)
            .free
            .filter(|(_, at)| at.elapsed() < REPORT_FRESH)
            .map(|(free, _)| free);
        match (disk, reported) {
            (Some(d), Some(r)) => Some(d.max(r)),
            (d, r) => d.or(r),
        }
    }

    /// Bytes running jobs other than `except` will still write.
    fn committed(&self, except: &str) -> u64 {
        lock(&self.running)
            .iter()
            .filter(|(id, _)| id.as_str() != except)
            .map(|(_, run)| run.total.unwrap_or(0).saturating_sub(*run.held.borrow()))
            .sum()
    }

    /// Whether `remaining` more bytes fit for job `id` (4.9).
    fn fits(&self, core: &Core, id: &str, remaining: u64) -> bool {
        let Some(free) = self.base_free(core, id) else {
            return true;
        };
        let fits = free
            .saturating_sub(self.committed(id))
            .saturating_sub(RESERVE)
            >= remaining;
        if fits {
            lock(&self.strict).remove(id);
        }
        fits
    }

    /// Whether `remaining` more bytes fit for job `id`, the idle stream
    /// cache deleted first when that is what makes them fit (4.9).
    async fn fits_making_room(&self, core: &Core, id: &str, remaining: u64) -> bool {
        if self.fits(core, id, remaining) {
            return true;
        }
        #[cfg(feature = "torrent")]
        if let Some(engine) = core.running_engine() {
            let need = remaining + RESERVE + self.committed(id);
            engine.make_room(need).await;
            return self.fits(core, id, remaining);
        }
        false
    }

    /// Why nothing may run now for a job of `torrent` (its info hash, for
    /// a torrent), if anything holds it.
    fn held_by(&self, mobile_data: bool, torrent: Option<&str>) -> Option<DownloadReason> {
        let link = self.link();
        if link.offline {
            return Some(DownloadReason::Network);
        }
        if !lock(&self.allowed).allowed {
            return Some(DownloadReason::App);
        }
        if self.settings().wifi_only && !mobile_data {
            if link.low_data {
                return Some(DownloadReason::LowData);
            }
            if link.metered {
                return Some(DownloadReason::Wifi);
            }
        }
        let other = lock(&self.streams)
            .values()
            .any(|played| played.is_none() || played.as_deref() != torrent);
        if other {
            return Some(DownloadReason::Playing);
        }
        None
    }

    /// One look at the queue: holds and releases, stops what was removed or
    /// paused, and starts queued jobs up to `RUNNING`.
    fn tick(self: &Arc<Self>) {
        let Some(core) = self.core() else { return };
        let Ok(rows) = core.library().downloads() else {
            return;
        };
        self.settle_smart(&core, &rows);
        // Runs whose row went or stopped being running.
        let gone: Vec<String> = lock(&self.running)
            .keys()
            .filter(|id| {
                !rows.iter().any(|r| {
                    &r.id == *id && DownloadState::read(&r.state) == DownloadState::Running
                })
            })
            .cloned()
            .collect();
        for id in gone {
            self.stop(&id);
        }
        for mut row in rows {
            let state = DownloadState::read(&row.state);
            let hold = self.held_by(row.mobile_data, torrent_of(&row).as_deref());
            match (state, hold) {
                (DownloadState::Running, Some(reason)) => {
                    self.stop(&row.id);
                    row.state = DownloadState::Waiting.word().into();
                    row.reason = Some(reason.word());
                    let _ = core.library().save_download(&row);
                }
                (DownloadState::Queued, Some(reason)) => {
                    row.state = DownloadState::Waiting.word().into();
                    row.reason = Some(reason.word());
                    let _ = core.library().save_download(&row);
                }
                (DownloadState::Waiting, hold) => {
                    let now = hold.map(DownloadReason::word);
                    // A space wait goes on until what it still needs fits, or
                    // with its size unknown for a minute.
                    let short = row.reason.as_deref() == Some("space")
                        && match row.bytes_total {
                            Some(total) => {
                                let remaining = total.saturating_sub(row.bytes_done).max(0) as u64;
                                !self.fits(&core, &row.id, remaining)
                            }
                            None => lock(&self.space_since)
                                .get(&row.id)
                                .is_some_and(|at| at.elapsed() < Duration::from_secs(60)),
                        };
                    if now.is_none() && short {
                        continue;
                    }
                    if now.is_none() {
                        // Released (a space wait looks again at its start).
                        row.state = DownloadState::Queued.word().into();
                        row.reason = None;
                        let _ = core.library().save_download(&row);
                        self.try_start(&core, row);
                    } else if row.reason != now && row.reason.as_deref() != Some("space") {
                        row.reason = now;
                        let _ = core.library().save_download(&row);
                    }
                }
                (DownloadState::Queued, None) => self.try_start(&core, row),
                (DownloadState::Done, _) if row.subtitles_state == "pending" => {
                    self.keep_subtitles(&core, row);
                }
                _ => {}
            }
        }
        self.publish();
    }

    /// Fetches a done download's subtitles once (4.10), and again while no
    /// source answered: after [`SUBTITLE_WAIT`], doubling up to
    /// [`SUBTITLE_WAIT_MAX`], never at every look of the queue.
    fn keep_subtitles(self: &Arc<Self>, core: &Core, row: DownloadRecord) {
        let waiting = lock(&self.subtitle_waits)
            .get(&row.id)
            .is_some_and(|(at, _)| std::time::Instant::now() < *at);
        if waiting || self.link().offline || !lock(&self.subtitling).insert(row.id.clone()) {
            return;
        }
        let me = Arc::downgrade(self);
        let core = core.clone();
        tokio::spawn(async move {
            let Some(d) = me.upgrade() else { return };
            let kept = subtitles::keep(&d, &core, &row).await;
            lock(&d.subtitling).remove(&row.id);
            let subtitles::Kept::Done(kept) = kept else {
                let mut waits = lock(&d.subtitle_waits);
                let step = waits
                    .get(&row.id)
                    .map_or(SUBTITLE_WAIT, |(_, s)| (*s * 2).min(SUBTITLE_WAIT_MAX));
                waits.insert(row.id.clone(), (std::time::Instant::now() + step, step));
                return;
            };
            lock(&d.subtitle_waits).remove(&row.id);
            let Ok(Some(mut row)) = core
                .library()
                .downloads()
                .map(|rows| rows.into_iter().find(|r| r.id == row.id))
            else {
                return;
            };
            row.subtitles_state = if kept.is_empty() { "none" } else { "done" }.into();
            row.subtitles = serde_json::to_string(&kept).unwrap_or_else(|_| "[]".into());
            let _ = core.library().save_download(&row);
            d.changed();
        });
    }

    /// Smart downloads (4.14): `core`'s profile finished `video_id` of
    /// `item_id`. When it is a done download and smart downloads are on, the
    /// next aired episode is added (ranked as the finished one was), and the
    /// finished one goes once the next is done, or at once with no next;
    /// never while another profile, a kids one included, has it started and
    /// unfinished. A kids profile triggers nothing. The step is kept on the
    /// finished row, so a relaunch goes on with it, and a meta that cannot be
    /// read now (offline) is looked up again later (code review r1 P2-2).
    pub(crate) fn finished(self: &Arc<Self>, core: &Core, item_id: &str, video_id: &str) {
        if !self.settings().smart || core.policy().kids() {
            return;
        }
        let Ok(Some(mut row)) = core.library().download_of(item_id, video_id) else {
            return;
        };
        // Each save past the end calls again: once is enough.
        if DownloadState::read(&row.state) != DownloadState::Done
            || row.season.is_none()
            || row.smart.is_some()
        {
            return;
        }
        let step = SmartStep {
            by: core.profile().to_owned(),
            next: None,
        };
        row.smart = serde_json::to_string(&step).ok();
        let _ = core.library().save_download(&row);
        self.changed();
    }

    /// Looks up the next episode of a smart step whose next is not known
    /// yet, once at a time, and keeps what it found on the row.
    fn smart_lookup(self: &Arc<Self>, core: &Core, row: DownloadRecord) {
        if self.link().offline || !lock(&self.smart_looking).insert(row.id.clone()) {
            return;
        }
        let me = Arc::downgrade(self);
        let core = core.clone();
        tokio::spawn(async move {
            let Some(d) = me.upgrade() else { return };
            let next = match smart_next(&core, &row).await {
                Next::Unreadable => None,
                Next::None => Some(String::new()),
                Next::Has(id) => Some(id),
                Next::Add(video) => {
                    let request = serde_json::from_str(&row.request).unwrap_or_default();
                    d.add(
                        &row.added_by,
                        &row.kind,
                        &row.item_id,
                        vec![video],
                        request,
                        None,
                    )
                    .ok()
                    .and_then(|ids| ids.into_iter().next())
                }
            };
            if let Some(next) = next
                && let Ok(Some(mut row)) = core.library().download_of(&row.item_id, &row.video_id)
                && let Some(mut step) = row
                    .smart
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<SmartStep>(s).ok())
            {
                step.next = Some(next);
                row.smart = serde_json::to_string(&step).ok();
                let _ = core.library().save_download(&row);
            }
            lock(&d.smart_looking).remove(&row.id);
            d.changed();
        });
    }

    /// Smart steps: the next episode looked up where it is not known yet;
    /// the finished one deleted once its next is done (or gone, or there is
    /// none), unless another profile is part way in.
    fn settle_smart(self: &Arc<Self>, core: &Core, rows: &[DownloadRecord]) {
        for row in rows {
            let Some(step) = row
                .smart
                .as_deref()
                .and_then(|s| serde_json::from_str::<SmartStep>(s).ok())
            else {
                continue;
            };
            let ready = match step.next.as_deref() {
                None => {
                    self.smart_lookup(core, row.clone());
                    false
                }
                // No next episode.
                Some("") => true,
                Some(next) => rows
                    .iter()
                    .find(|r| r.id == next)
                    .is_none_or(|r| DownloadState::read(&r.state) == DownloadState::Done),
            };
            if ready && !part_way_elsewhere(core, row, &step.by) {
                let _ = self.act(std::slice::from_ref(&row.id), Action::Remove);
            }
        }
    }

    /// The subtitles a download of `key` for `video_id` of `item_id` kept,
    /// with their files.
    pub(crate) fn kept_subtitles(
        &self,
        item_id: &str,
        video_id: &str,
        key: &str,
    ) -> Vec<(subtitles::StoredSubtitle, PathBuf)> {
        let Some(core) = self.core() else {
            return Vec::new();
        };
        let Ok(Some(row)) = core.library().download_of(item_id, video_id) else {
            return Vec::new();
        };
        let same = row
            .version
            .as_deref()
            .and_then(|v| serde_json::from_str::<Version>(v).ok())
            .is_some_and(|v| v.key == key);
        if !same {
            return Vec::new();
        }
        let stored: Vec<subtitles::StoredSubtitle> =
            serde_json::from_str(&row.subtitles).unwrap_or_default();
        stored
            .into_iter()
            .map(|s| {
                let path = subtitles::file_path(self, &row.id, &s);
                (s, path)
            })
            .filter(|(_, path)| path.is_file())
            .collect()
    }

    fn try_start(self: &Arc<Self>, core: &Core, mut row: DownloadRecord) {
        let mut running = lock(&self.running);
        if running.len() >= RUNNING || running.contains_key(&row.id) {
            return;
        }
        let cancel = CancellationToken::new();
        let (held, _) = watch::channel(row.bytes_done.max(0) as u64);
        running.insert(
            row.id.clone(),
            Running {
                cancel: cancel.clone(),
                held,
                total: row.bytes_total.map(|t| t as u64),
                speed: Speed::default(),
                peers: 0,
            },
        );
        drop(running);
        row.state = DownloadState::Running.word().into();
        row.reason = None;
        let _ = core.library().save_download(&row);
        let me = Arc::downgrade(self);
        let core = core.clone();
        tokio::spawn(async move {
            let id = row.id.clone();
            let outcome = cancel
                .run_until_cancelled(run_job(me.clone(), core.clone(), row))
                .await;
            let Some(d) = me.upgrade() else { return };
            if let Some(outcome) = outcome {
                d.finish(&core, &id, outcome);
            }
            // A cancelled run was stopped by whoever cancelled it, which
            // wrote its row.
            d.changed();
        });
    }

    /// Writes how a run ended.
    fn finish(&self, core: &Core, id: &str, outcome: Outcome) {
        lock(&self.running).remove(id);
        let Ok(Some(mut row)) = core
            .library()
            .downloads()
            .map(|rows| rows.into_iter().find(|r| r.id == id))
        else {
            return;
        };
        let (state, reason) = match outcome {
            Outcome::Done => (DownloadState::Done, None),
            Outcome::Wait(reason) => (DownloadState::Waiting, Some(reason)),
            Outcome::Fail(reason) => (DownloadState::Failed, Some(reason)),
        };
        if state == DownloadState::Done {
            row.done_ms = Some(mq_library::hlc::now_ms() as i64);
        }
        if reason == Some(DownloadReason::Space) {
            lock(&self.space_since).insert(id.to_owned(), std::time::Instant::now());
        } else {
            lock(&self.space_since).remove(id);
        }
        row.state = state.word().into();
        row.reason = reason.map(DownloadReason::word);
        let _ = core.library().save_download(&row);
    }

    /// A torrent job's connected peers now.
    #[cfg(feature = "torrent")]
    fn swarm(&self, id: &str, peers: usize) {
        if let Some(run) = lock(&self.running).get_mut(id) {
            run.peers = peers;
        }
    }

    /// Whether the torrent of the running job `id` has peers now: a partial
    /// one is recommended online only then (code review r1, runtime P2-5).
    pub(crate) fn has_peers(&self, id: &str) -> bool {
        lock(&self.running).get(id).is_some_and(|run| run.peers > 0)
    }

    /// A run's progress: `held` bytes on disk of `total`, and `received`
    /// more since the last call.
    fn progress(&self, id: &str, held: u64, total: Option<u64>, received: u64) {
        let mut running = lock(&self.running);
        if let Some(run) = running.get_mut(id) {
            run.held.send_replace(held);
            run.total = total.or(run.total);
            run.speed.add(received);
        }
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Done,
    Wait(DownloadReason),
    Fail(DownloadReason),
}

/// What the user did to a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pause,
    Resume,
    Retry { mobile_data: bool },
    Remove,
}

/// One video of an add.
#[derive(Debug, Clone, Default)]
pub struct AddVideo {
    pub video_id: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub facts: Facts,
}

fn row_of(r: &DownloadRecord, live: Option<&Running>) -> DownloadRow {
    let facts: Facts = serde_json::from_str(&r.facts).unwrap_or_default();
    let version: Option<Version> = r
        .version
        .as_deref()
        .and_then(|v| serde_json::from_str(v).ok());
    let quality = version.as_ref().and_then(|v| v.chain.first()).map(|s| {
        let release = &s.stream.release;
        crate::views::versions::quality_word(release.resolution.height(), release.quality)
    });
    let held = live.map(|l| *l.held.borrow());
    let bytes_done = held.unwrap_or(r.bytes_done.max(0) as u64);
    let bytes_total = live
        .and_then(|l| l.total)
        .or(r.bytes_total.map(|t| t.max(0) as u64));
    let speed = live.and_then(|l| l.speed.bps());
    let eta = match (speed, bytes_total) {
        (Some(bps), Some(total)) if bps > 0 => Some(total.saturating_sub(bytes_done) / bps),
        _ => None,
    };
    DownloadRow {
        id: r.id.clone(),
        kind: r.kind.clone(),
        item_id: r.item_id.clone(),
        video_id: r.video_id.clone(),
        season: r.season.and_then(|s| u32::try_from(s).ok()),
        episode: r.episode.and_then(|e| u32::try_from(e).ok()),
        name: facts.name,
        episode_title: facts.episode_title,
        poster: facts.poster,
        still: facts.still,
        runtime: facts.runtime,
        released: facts.released,
        state: DownloadState::read(&r.state),
        reason: r.reason.as_deref().and_then(DownloadReason::read),
        mobile_data: r.mobile_data,
        bytes_done,
        bytes_total,
        speed_bps: speed,
        eta_secs: eta,
        quality,
        subtitles: {
            let kept: Vec<subtitles::StoredSubtitle> =
                serde_json::from_str(&r.subtitles).unwrap_or_default();
            let mut tags: Vec<String> = Vec::new();
            for s in kept {
                let tag = crate::views::playback::listed_track(&s.wire).language;
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
            tags
        },
    }
}

/// The queue's task: a look at every wake, every minute (a space wait
/// looks again), and the board twice a second while bytes move.
async fn run_queue(me: std::sync::Weak<Downloads>) {
    loop {
        let Some(d) = me.upgrade() else { return };
        d.tick();
        if let Some(core) = d.core() {
            d.lease_images(&core).await;
        }
        let busy = !lock(&d.running).is_empty();
        let wait = if busy {
            PUBLISH_EVERY
        } else {
            Duration::from_secs(60)
        };
        // Only a weak handle waits: a dropped core ends the loop.
        let wake = d.wake.clone();
        drop(d);
        tokio::select! {
            () = wake.notified() => {}
            () = tokio::time::sleep(wait) => {}
        }
    }
}

/// One run of a job: picks its version when it has none, then fetches
/// from where it stopped.
async fn run_job(me: std::sync::Weak<Downloads>, core: Core, mut row: DownloadRecord) -> Outcome {
    if row.version.is_none() {
        match pick_version(&core, &row).await {
            Some(chain) => {
                row.version = serde_json::to_string(&Version {
                    key: chain[0].key.clone(),
                    chain: chain.iter().map(StoredStream::of).collect(),
                    ext: None,
                    torrent: None,
                    validator: None,
                })
                .ok();
                let _ = core.library().save_download(&row);
            }
            None => return Outcome::Fail(DownloadReason::NoVersion),
        }
    }
    loop {
        let Some(version) = row
            .version
            .as_deref()
            .and_then(|v| serde_json::from_str::<Version>(v).ok())
        else {
            return Outcome::Fail(DownloadReason::NoVersion);
        };
        let Some(source) = usize::try_from(row.moves)
            .ok()
            .and_then(|at| version.chain.get(at))
        else {
            return Outcome::Fail(DownloadReason::Retries);
        };
        let Some(d) = me.upgrade() else {
            return Outcome::Wait(DownloadReason::App);
        };
        let stream = source.stream();
        #[cfg(feature = "torrent")]
        let ended = if crate::views::playback::by_engine(&stream, core.debrid().is_some(), true) {
            torrent::fetch(&d, &core, &mut row, &stream).await
        } else {
            http::fetch(&d, &core, &mut row, &stream).await
        };
        #[cfg(not(feature = "torrent"))]
        let ended = http::fetch(&d, &core, &mut row, &stream).await;
        drop(d);
        match ended {
            http::Ended::Done => return Outcome::Done,
            http::Ended::Wait(reason) => return Outcome::Wait(reason),
            http::Ended::Move(reason) => {
                // A hand-picked version never moves; a pick moves twice at
                // most, and fails with its source's reason when no other
                // source is left.
                let next = usize::try_from(row.moves + 1).unwrap_or(usize::MAX);
                if row.hand_picked || next >= version.chain.len() {
                    return Outcome::Fail(reason);
                }
                if row.moves + 1 > MAX_MOVES {
                    return Outcome::Fail(DownloadReason::Retries);
                }
                // The next source of the pick, from its start: what the last
                // one's answers named (its extension, validator, torrent
                // file) is not this one's.
                row.moves += 1;
                row.bytes_done = 0;
                row.bytes_total = None;
                if let Some(mut moved) = row
                    .version
                    .as_deref()
                    .and_then(|v| serde_json::from_str::<Version>(v).ok())
                {
                    moved.ext = None;
                    moved.validator = None;
                    // A torrent left for another source is no longer kept.
                    #[cfg(feature = "torrent")]
                    if let Some(t) = moved.torrent.take() {
                        let core = core.clone();
                        tokio::spawn(async move {
                            if let Ok(engine) = core.torrent_engine().await
                                && let Err(e) = engine.delete_file(&t.hash, t.file).await
                            {
                                tracing::warn!("download move: {e}");
                            }
                        });
                    }
                    moved.torrent = None;
                    row.version = serde_json::to_string(&moved).ok();
                }
                if let Some(d) = me.upgrade() {
                    d.forget_files(&row.id);
                }
                let _ = core.library().save_download(&row);
            }
        }
    }
}

/// The version a job keeps, picked as Play would pick it with the device
/// the add described: a Versions view run with no screen, its pick and the
/// pick's failover chain, sources this build cannot download left out.
async fn pick_version(core: &Core, row: &DownloadRecord) -> Option<Vec<ParsedStream>> {
    let request: serde_json::Value = serde_json::from_str(&row.request).ok()?;
    let facts: Facts = serde_json::from_str(&row.facts).unwrap_or_default();
    let mut params = request.clone();
    if let Some(map) = params.as_object_mut() {
        map.insert("kind".into(), row.kind.clone().into());
        map.insert("videoId".into(), row.video_id.clone().into());
        map.insert("title".into(), facts.name.clone().into());
        if let Some(s) = row.season {
            map.insert("season".into(), s.into());
        }
        if let Some(e) = row.episode {
            map.insert("episode".into(), e.into());
        }
        for (key, value) in [
            ("runtime", facts.runtime.clone()),
            ("releaseInfo", facts.release_info.clone()),
            ("released", facts.released.clone()),
        ] {
            if let Some(value) = value {
                map.entry(key).or_insert(value.into());
            }
        }
        if let Some(n) = facts.episodes_in_season {
            map.entry("episodesInSeason").or_insert(n.into());
        }
    }
    let core = core.for_profile(&row.added_by);
    let params =
        crate::views::versions::VersionsParams::from_json(&params, core.prefs().streams).ok()?;
    let view =
        crate::views::versions::VersionsView::open_to_download(&core, params, Some(&row.item_id));
    let (_, mut updates) = view.subscribe();
    let settle = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(settle);
    loop {
        let snapshot = view.snapshot();
        if snapshot.done && snapshot.recommended.is_some()
            || snapshot.done && snapshot.rows.is_empty()
        {
            break;
        }
        tokio::select! {
            next = futures_util::StreamExt::next(&mut updates) => if next.is_none() { break },
            () = &mut settle => break,
        }
    }
    let key = view.snapshot().recommended?.key;
    let chain: Vec<ParsedStream> = view
        .pick(&key)
        .candidates
        .into_iter()
        .filter(|s| http::downloadable(&core, s))
        .collect();
    (!chain.is_empty()).then_some(chain)
}

/// The downloads, for an op that changes them: a kids profile changes
/// nothing (plans/offline-downloads.md 4.16).
pub fn owner(core: &Core) -> Result<Arc<Downloads>, CoreError> {
    if core.policy().kids() {
        return Err(CoreError::Downloads(
            "a kids profile cannot change downloads".into(),
        ));
    }
    Ok(core.downloads())
}

/// What `downloadAdd` names (4.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Video,
    Season,
    Series,
}

/// A `downloadAdd` (4.15): one video, the aired episodes of a season, or of
/// every regular season of a title. `params` is what the app sends with
/// `versions` (the device's caps and language), kept so each job ranks as
/// Play would when it starts; `pick` is a version the user chose, for a
/// single video.
pub struct AddRequest {
    pub kind: String,
    pub item_id: String,
    pub scope: Scope,
    pub video_id: Option<String>,
    pub season: Option<u32>,
    pub params: serde_json::Value,
    pub pick: Option<Vec<ParsedStream>>,
}

/// Adds the downloads `req` names for `core`'s profile, read from the
/// title's meta (the page that asks has just read it). A kids profile adds
/// nothing.
pub async fn add(core: &Core, req: AddRequest) -> Result<Vec<String>, CoreError> {
    if core.policy().kids() {
        return Err(CoreError::Downloads(
            "a kids profile cannot download".into(),
        ));
    }
    let (meta, _) = core
        .fetch_meta(&req.kind, &req.item_id)
        .await
        .map_err(CoreError::Downloads)?;
    let now = mq_library::hlc::now_ms() as i64;
    let videos = expand(&meta, &req, now);
    if videos.is_empty() {
        return Err(CoreError::Downloads("nothing to download".into()));
    }
    let pick = req.pick.filter(|_| videos.len() == 1);
    core.downloads().add(
        core.profile(),
        &req.kind,
        &req.item_id,
        videos,
        req.params,
        pick,
    )
}

/// The videos `scope` names in `meta`, with what each needs offline.
fn expand(meta: &mq_protocol::MetaItem, req: &AddRequest, now_ms: i64) -> Vec<AddVideo> {
    let (scope, season, video_id) = (req.scope, req.season, req.video_id.as_deref());
    let p = &meta.preview;
    let title = Facts {
        name: p.name.clone(),
        poster: p.poster.as_ref().map(|u| u.to_string()),
        runtime: p.runtime.clone(),
        release_info: p.release_info.clone(),
        ..Facts::default()
    };
    let episodic = req.kind != "movie" && !meta.videos.is_empty();
    if !episodic {
        return vec![AddVideo {
            video_id: video_id.unwrap_or(&p.id).to_owned(),
            facts: title,
            ..AddVideo::default()
        }];
    }
    let in_season = |s: u32| {
        meta.videos
            .iter()
            .filter(|v| v.series_info.as_ref().is_some_and(|i| i.season == s))
            .count() as u32
    };
    meta.videos
        .iter()
        .filter(|v| match scope {
            Scope::Video => Some(v.id.as_str()) == video_id,
            Scope::Season => v.series_info.as_ref().map(|i| i.season) == season,
            Scope::Series => v.series_info.as_ref().is_some_and(|i| i.season > 0),
        })
        .filter(|v| scope == Scope::Video || crate::views::playback::aired(v, now_ms))
        .map(|v| {
            let info = v.series_info.as_ref();
            AddVideo {
                video_id: v.id.clone(),
                season: info.map(|i| i.season),
                episode: info.map(|i| i.episode),
                facts: Facts {
                    episode_title: Some(v.title.clone()).filter(|t| !t.is_empty()),
                    still: v.thumbnail.clone(),
                    released: v.released.map(|d| d.date_naive().to_string()),
                    episodes_in_season: info.map(|i| in_season(i.season)),
                    ..title.clone()
                },
            }
        })
        .collect()
}

fn new_id() -> String {
    let mut bytes = [0u8; 16];
    let _ = getrandom::fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// What a smart step's lookup found.
enum Next {
    /// The meta cannot be read now (offline): look again later.
    Unreadable,
    /// No aired episode follows.
    None,
    /// The next episode already has this download: wait for it.
    Has(String),
    Add(AddVideo),
}

/// The aired episode after `row`'s, as a download wants it.
async fn smart_next(core: &Core, row: &DownloadRecord) -> Next {
    let Ok((meta, _)) = core.fetch_meta(&row.kind, &row.item_id).await else {
        return Next::Unreadable;
    };
    let Some((next, _)) = crate::views::playback::following(&meta, &row.video_id) else {
        return Next::None;
    };
    let now = mq_library::hlc::now_ms() as i64;
    if !crate::views::playback::aired(next, now) {
        return Next::None;
    }
    match core.library().download_of(&row.item_id, &next.id) {
        Ok(Some(existing)) => return Next::Has(existing.id),
        Ok(None) => {}
        Err(_) => return Next::Unreadable,
    }
    let req = AddRequest {
        kind: row.kind.clone(),
        item_id: row.item_id.clone(),
        scope: Scope::Video,
        video_id: Some(next.id.clone()),
        season: None,
        params: serde_json::Value::Null,
        pick: None,
    };
    expand(&meta, &req, now)
        .into_iter()
        .next()
        .map_or(Next::None, Next::Add)
}

/// Another profile on the device has `row`'s video started and not
/// finished: a smart download keeps it.
fn part_way_elsewhere(core: &Core, row: &DownloadRecord, by: &str) -> bool {
    let profiles = core.library().profiles().unwrap_or_default();
    profiles.iter().filter(|p| p.id != by).any(|p| {
        core.library()
            .progress(&p.id, &row.video_id)
            .ok()
            .flatten()
            .is_some_and(|(pos, dur)| {
                pos > 0 && (dur <= 0 || (pos as f64) < dur as f64 * mq_library::FINISHED_RATIO)
            })
    })
}

/// The info hash of a job's torrent, chosen or listed.
fn torrent_of(row: &DownloadRecord) -> Option<String> {
    let version: Version = serde_json::from_str(row.version.as_deref()?).ok()?;
    if let Some(t) = version.torrent {
        return Some(t.hash);
    }
    match &version.chain.first()?.stream().source {
        mq_protocol::Source::Torrent { info_hash, .. } => Some(info_hash.to_ascii_lowercase()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream from the network holds downloads (4.4), but a torrent's own
    /// download goes on while that torrent plays: its scheduler puts the
    /// reader first (engine r1 P1-3).
    #[tokio::test]
    async fn a_stream_holds_every_download_but_its_own_torrents() {
        let core = Core::temporary("default").unwrap();
        let d = core.downloads();
        d.set_link(Link::default());
        let (mine, other) = ("a".repeat(40), "b".repeat(40));
        assert_eq!(d.held_by(false, Some(&mine)), None);
        let playing = d.streaming(Some(&mine));
        assert_eq!(d.held_by(false, Some(&mine)), None);
        assert_eq!(
            d.held_by(false, Some(&other)),
            Some(DownloadReason::Playing)
        );
        assert_eq!(d.held_by(false, None), Some(DownloadReason::Playing));
        drop(playing);
        let http = d.streaming(None);
        assert_eq!(d.held_by(false, Some(&mine)), Some(DownloadReason::Playing));
        drop(http);
        assert_eq!(d.held_by(false, None), None);
    }
}
