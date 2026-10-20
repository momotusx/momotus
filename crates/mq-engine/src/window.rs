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

pub use mq_engine_sched::{RESERVE_BYTES, WindowInputs, WindowPlan};

/// Under a quarter of the reserve the playback ends with "storage full"
/// before any write fails (2.4).
const FLOOR_BYTES: u64 = RESERVE_BYTES / 4;

/// How often the keeper reads the free space when nothing wakes it.
const GUARD_EVERY: Duration = Duration::from_secs(1);

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

/// `R` before the engine starts: what every torrent folder in `data_dir`
/// holds (none has a reader yet). Blocking: walks the disk.
pub fn reclaimable(data_dir: &Path) -> u64 {
    crate::leftovers(data_dir, &[]).0.iter().map(|l| l.2).sum()
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
}

impl WindowState {
    /// The scheduler's window for this state.
    pub fn sched(&self, ramp_floor: u64) -> mq_engine_sched::Window {
        let p = &self.plan;
        mq_engine_sched::Window {
            file: self.file,
            budget: p.budget.saturating_sub(self.shortfall),
            slack: p.slack,
            behind: p.behind,
            ramp_floor: ramp_floor.min(p.ahead),
            open_floor: p.open_floor,
            open_cap: p.open_cap,
        }
    }

    /// The readahead: none while the space guard holds, the device preset on
    /// a metered network, else the window's.
    pub fn ahead(&self, preset: u64) -> u64 {
        match () {
            _ if self.shortfall > 0 => 0,
            _ if self.metered => preset.min(self.plan.ahead),
            _ => self.plan.ahead,
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
                self.end_window(&t, "storage full").await;
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
    let mut failed: Vec<String> = Vec::new();
    // The playback's claim may come just after its window.
    let mut claimed = false;
    loop {
        tokio::select! {
            () = t.keeper_wake.notified() => {}
            () = tokio::time::sleep(GUARD_EVERY) => {}
        }
        let Some(inner) = engine.upgrade() else {
            break;
        };
        let engine = Engine { inner };
        let Some(state) = *lock(&t.window) else {
            break;
        };
        claimed |= t.claimed();
        if claimed && !t.claimed() {
            engine.trim(h, &t, state).await;
            break;
        }
        let free = engine.free().unwrap_or(u64::MAX);
        if free < FLOOR_BYTES {
            engine.end_window(&t, "storage full").await;
            break;
        }
        // The space guard (2.4): other apps wrote, so the window gives back
        // the shortfall and its readahead stops, until free space is over
        // the reserve plus `h` again.
        let shortfall = if free < RESERVE_BYTES {
            state.shortfall.max(RESERVE_BYTES - free)
        } else if free > RESERVE_BYTES + state.plan.slack {
            0
        } else {
            state.shortfall
        };
        if shortfall != state.shortfall {
            let state = WindowState { shortfall, ..state };
            *lock(&t.window) = Some(state);
            engine.send_window(&t, Some(state), None).await;
        }
        let Some(pieces) = engine.ask(&t, adapter::Msg::ToEvict).await else {
            continue;
        };
        if pieces.is_empty() {
            continue;
        }
        t.rounds.fetch_add(1, Ordering::Relaxed);
        match engine.evict_pieces(h, &pieces).await {
            Ok(r) if r.error.is_none() => failed.clear(),
            Ok(r) => failed.extend(r.error),
            Err(e) => failed.push(e.to_string()),
        }
        if failed.len() as u32 >= FAILED_ROUNDS {
            engine.end_window(&t, &storage_error(&failed)).await;
            break;
        }
    }
    t.keeping.store(false, Ordering::Release);
}
