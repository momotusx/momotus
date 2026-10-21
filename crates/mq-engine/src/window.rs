//! The streaming window (plans/streaming-window.md 2.1 to 2.4): a file
//! larger than the free space plays while the torrent holds only part of
//! it. [`window_plan`] sizes the window, the scheduler keeps to it, and a
//! keeper task per windowed torrent evicts what the scheduler says it may.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::{
    DeviceClass, Engine, EngineError, Inner, Result, Torrent, TorrentHandle, adapter, lock,
};

pub use mq_engine_sched::{APFS_HEAD_BYTES, HeldBytes, RESERVE_BYTES, WindowInputs, WindowPlan};

/// Under a quarter of the reserve the playback ends with "storage full"
/// before any write fails (2.4).
const FLOOR_BYTES: u64 = RESERVE_BYTES / 4;

/// How often the keeper reads the free space when nothing wakes it.
const GUARD_EVERY: Duration = Duration::from_secs(1);

/// After a round, how often and how long the keeper looks for holes APFS
/// filled at the next data sync.
const HOLES_EVERY: Duration = Duration::from_millis(100);
const HOLES_FOR: Duration = Duration::from_secs(2);

/// Failed rounds in a row that end the playback (2.4).
const FAILED_ROUNDS: u32 = 3;

/// Sizes a window with `class`'s windows (2.1, 2.2), or refuses with
/// [`EngineError::DeviceFull`] when `room` is under `W_min + 64 MiB`.
pub fn window_plan(class: DeviceClass, i: WindowInputs) -> Result<WindowPlan> {
    mq_engine_sched::window_plan(&class.sched(), i).ok_or(EngineError::DeviceFull)
}

/// `f_bavail × f_frsize` of the volume holding `dir`.
#[cfg(unix)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is NUL terminated and `stat` is a buffer statvfs
    // fills when it returns 0.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it wrote `stat`.
    let stat = unsafe { stat.assume_init() };
    #[allow(clippy::unnecessary_cast)]
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(not(unix))]
pub fn free_space(_dir: &Path) -> Option<u64> {
    None
}

/// `R`, `C` and each torrent folder's bytes (by info hash), as maintenance
/// counts them for a ranking that cannot wait (3.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counted {
    pub reclaimable: u64,
    pub committed: u64,
    pub held: std::collections::HashMap<String, HeldBytes>,
}

/// [`Counted`] before the engine starts: every torrent folder in
/// `data_dir`, none in use. Blocking: walks the disk.
pub fn counted_on_disk(data_dir: &Path) -> Counted {
    let left = crate::leftovers(data_dir, &[]).0;
    Counted {
        reclaimable: left.iter().map(|l| l.2).sum(),
        committed: 0,
        held: crate::counted_on_disk_except(data_dir, &[]),
    }
}

/// A windowed playback's state on its torrent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WindowState {
    pub file: usize,
    pub plan: WindowPlan,
    pub bitrate: u64,
    pub metered: bool,
    /// What the space guard took off the budget (2.4).
    pub shortfall: u64,
    /// Which window of the torrent this is: a trim or an end clears only
    /// its own.
    pub generation: u64,
    /// The playback's claim came before its window (the app's order): a
    /// stop before the keeper's first look still trims.
    pub claimed: bool,
}

impl WindowState {
    /// The scheduler's window for this state.
    pub fn sched(&self, ramp_floor: u64) -> mq_engine_sched::Window {
        let p = &self.plan;
        mq_engine_sched::Window {
            file: self.file,
            budget: self.budget(),
            slack: p.slack,
            behind: p.behind,
            ramp_floor: ramp_floor.min(p.ahead),
            open_floor: p.open_floor,
            open_cap: p.open_cap,
        }
    }

    /// The scheduler's budget: `W` less the space guard's shortfall and
    /// what APFS keeps of the file's head (2.6), so the folder stays
    /// within `W`.
    pub fn budget(&self) -> u64 {
        self.plan
            .budget
            .saturating_sub(self.shortfall + mq_engine_sched::APFS_HEAD_BYTES)
    }

    /// The readahead: the device preset on a metered network, else the
    /// window's. A shrunk budget's cap holds it back; it is never turned
    /// off, so what it fetched stays kept (review r2 P2-2: off, it was
    /// evicted and fetched again).
    pub fn ahead(&self, preset: u64) -> u64 {
        if self.metered {
            preset.min(self.plan.ahead)
        } else {
            self.plan.ahead
        }
    }
}

impl Engine {
    /// The space guard for whole files (2.4), on the maintenance tick:
    /// under the terminal floor a claimed playback ends with "storage
    /// full"; under the reserve its readahead stops, and comes back over
    /// the reserve plus 8 MiB.
    pub(crate) async fn guard_whole_files(&self) {
        let Some(free) = self.free() else {
            return;
        };
        let all: Vec<Arc<Torrent>> = lock(&self.inner.torrents).values().cloned().collect();
        for t in all {
            if !t.claimed() || lock(&t.window).is_some() || lock(&t.ended).is_some() {
                continue;
            }
            if free < FLOOR_BYTES {
                self.end_window(&t, "storage full", None).await;
                continue;
            }
            let Some((file, bytes_per_s, readahead)) = *lock(&t.rate) else {
                continue;
            };
            let starved = t.starved.load(Ordering::Relaxed);
            let now = match () {
                _ if free < RESERVE_BYTES => true,
                _ if free > RESERVE_BYTES + 8 * (1 << 20) => false,
                _ => starved,
            };
            if now == starved {
                continue;
            }
            t.starved.store(now, Ordering::Relaxed);
            if let Some(l) = t.live.lock().await.as_ref() {
                let _ = l.msgs.send(adapter::Msg::Rate {
                    file,
                    bytes_per_s,
                    readahead: if now { 0 } else { readahead },
                });
            }
        }
    }
}

impl Engine {
    /// APFS fills the holes between small extents at the data sync after a
    /// punch: pieces with no block written read back as data. When the file
    /// holds more than the scheduler counts plus `h`, the pieces the file
    /// holds that have nothing of ours are punched again.
    pub(crate) async fn clear_filled_holes(&self, t: &Torrent, w: WindowState) {
        let Some(info) = t.files.get(w.file).cloned() else {
            return;
        };
        let path = t.dir.join(&info.path);
        let held = self.ask(t, adapter::Msg::Held).await.unwrap_or(u64::MAX);
        let offset: u64 = t.files[..w.file].iter().map(|f| f.len).sum();
        let piece_len = u64::from(t.piece_len);
        let found = tokio::task::spawn_blocking(move || {
            let allocated = std::fs::metadata(&path).map_or(0, |m| {
                #[cfg(unix)]
                {
                    std::os::unix::fs::MetadataExt::blocks(&m) * 512
                }
                #[cfg(not(unix))]
                {
                    m.len()
                }
            });
            (allocated, data_pieces(&path, offset, piece_len))
        })
        .await;
        let Ok((allocated, pieces)) = found else {
            return;
        };
        if allocated <= held.saturating_add(w.plan.slack) || pieces.is_empty() {
            return;
        }
        let Some(empty) = self.ask(t, |tx| adapter::Msg::Empty { pieces, tx }).await else {
            return;
        };
        if empty.is_empty() {
            return;
        }
        let managed = t.live.lock().await.as_ref().map(|l| l.managed.clone());
        if let Some(managed) = managed {
            let _round = t.keeper.lock().await;
            if let Err(e) = managed.clear_empty_pieces(&empty).await {
                tracing::warn!("clearing filled holes of {}: {e:#}", t.info_hash);
            }
        }
    }
}

/// The torrent pieces a file's data regions touch (`SEEK_DATA` and
/// `SEEK_HOLE`), the file starting at torrent byte `offset`. Blocking.
#[cfg(unix)]
fn data_pieces(path: &Path, offset: u64, piece_len: u64) -> Vec<u32> {
    use std::os::fd::AsRawFd;
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = f.metadata().map_or(0, |m| m.len());
    let (mut pieces, mut pos) = (Vec::new(), 0u64);
    while pos < len {
        let Ok(at) = i64::try_from(pos) else {
            break;
        };
        // SAFETY: lseek takes plain values.
        let start = unsafe { libc::lseek(f.as_raw_fd(), at, libc::SEEK_DATA) };
        if start < 0 {
            break;
        }
        // SAFETY: as above.
        let end = unsafe { libc::lseek(f.as_raw_fd(), start, libc::SEEK_HOLE) };
        if end <= start {
            break;
        }
        let (first, last) = (
            (offset + start as u64) / piece_len,
            (offset + end as u64 - 1) / piece_len,
        );
        pieces.extend((first..=last).filter_map(|p| u32::try_from(p).ok()));
        pos = end as u64;
    }
    pieces.dedup();
    pieces
}

#[cfg(not(unix))]
fn data_pieces(_path: &Path, _offset: u64, _piece_len: u64) -> Vec<u32> {
    Vec::new()
}

/// Why a window ended the playback (2.4).
fn storage_error(errors: &[String]) -> String {
    if errors
        .iter()
        .any(|e| e.contains("No space left") || e.contains("os error 28"))
    {
        "storage full".into()
    } else {
        "this storage cannot free space while playing".into()
    }
}

/// The keeper of one windowed torrent (2.4): one round at a time, woken by
/// the adapter when the torrent holds more than `W - h / 2` and every
/// second for the space guard. When the last claim drops it trims the
/// torrent and ends.
pub(crate) async fn keep(engine: Weak<Inner>, h: TorrentHandle, t: Arc<Torrent>) {
    loop {
        keep_windows(&engine, h, &t).await;
        t.keeping.store(false, Ordering::Release);
        // A window set while this keeper was ending found it running.
        if lock(&t.window).is_none() || t.keeping.swap(true, Ordering::AcqRel) {
            break;
        }
    }
}

/// The keeper's rounds, window after window, until the torrent has none.
async fn keep_windows(engine: &Weak<Inner>, h: TorrentHandle, t: &Arc<Torrent>) {
    let mut failed: Vec<String> = Vec::new();
    // The playback's claim may come just after its window.
    let mut claimed = false;
    let mut punched: Option<std::time::Instant> = None;
    let mut generation = None;
    loop {
        let every = match punched {
            Some(at) if at.elapsed() < HOLES_FOR => HOLES_EVERY,
            _ => GUARD_EVERY,
        };
        tokio::select! {
            () = t.keeper_wake.notified() => {}
            () = tokio::time::sleep(every) => {}
        }
        let Some(inner) = engine.upgrade() else {
            break;
        };
        let engine = Engine { inner };
        let Some(state) = *lock(&t.window) else {
            break;
        };
        // A new window (a reopen right after a stop) starts afresh.
        if generation != Some(state.generation) {
            generation = Some(state.generation);
            (claimed, punched) = (state.claimed, None);
            failed.clear();
        }
        claimed |= t.claimed();
        if claimed && !t.claimed() {
            engine.trim(h, t, state).await;
            continue;
        }
        let free = engine.free().unwrap_or(u64::MAX);
        if free < FLOOR_BYTES {
            engine
                .end_window(t, "storage full", Some(state.generation))
                .await;
            break;
        }
        // The space guard (2.4): the scheduler may hold what it holds and the
        // free space over the reserve, never more than its full budget, so
        // another app's write shrinks the window and the same app's delete
        // gives it back. The budget moves by more than `h / 2` only, so a
        // round's whole pieces do not flip it back and forth.
        // Bytes written, not pieces in flight at full size: free space has
        // not dropped for those yet (review r3 A R2). No answer, no change.
        let Some(written) = engine.ask(t, adapter::Msg::Written).await else {
            continue;
        };
        let live = (written + free).saturating_sub(RESERVE_BYTES);
        let full = WindowState {
            shortfall: 0,
            ..state
        }
        .budget();
        let shortfall = full.saturating_sub(live);
        let moved = shortfall.abs_diff(state.shortfall) > state.plan.slack / 2
            || (shortfall == 0) != (state.shortfall == 0);
        if moved {
            engine.set_shortfall(t, state.generation, shortfall).await;
        }
        engine.clear_filled_holes(t, state).await;
        let Some(pieces) = engine.ask(t, adapter::Msg::ToEvict).await else {
            continue;
        };
        if pieces.is_empty() {
            continue;
        }
        t.rounds.fetch_add(1, Ordering::Relaxed);
        punched = Some(std::time::Instant::now());
        match engine.evict_pieces(h, &pieces).await {
            Ok(r) if r.error.is_none() => failed.clear(),
            Ok(r) => failed.extend(r.error),
            Err(e) => failed.push(e.to_string()),
        }
        if failed.len() as u32 >= FAILED_ROUNDS {
            engine
                .end_window(t, &storage_error(&failed), Some(state.generation))
                .await;
            break;
        }
    }
}
