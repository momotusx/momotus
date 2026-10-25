//! The streaming window (plans/streaming-window.md 2.1 to 2.4): a file
//! larger than the free space plays while the torrent holds only part of
//! it. [`window_plan`] sizes the window, the scheduler keeps to it, and a
//! keeper task per windowed torrent evicts what the scheduler says it may.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::{DeviceClass, Engine, Inner, Torrent, TorrentHandle, adapter, lock};

pub use mq_engine_sched::{
    APFS_HEAD_BYTES, HeldBytes, RESERVE_BYTES, RING_BYTES, WindowInputs, WindowPlan,
};

/// Under a quarter of the reserve a window plays on in a ring, in RAM where
/// the torrent is piece files, and a whole file becomes one; no playback
/// ends for space (plans/no-space-warnings.md).
const FLOOR_BYTES: u64 = RESERVE_BYTES / 4;

/// How often the keeper reads the free space when nothing wakes it.
const GUARD_EVERY: Duration = Duration::from_secs(1);

/// After a round, how often and how long the keeper looks for holes APFS
/// filled at the next data sync.
const HOLES_EVERY: Duration = Duration::from_millis(100);
const HOLES_FOR: Duration = Duration::from_secs(2);

/// Failed rounds in a row after which a window's file plays on whole,
/// freeing nothing (plans/no-space-warnings.md).
const FAILED_ROUNDS: u32 = 3;

/// Sizes a window with `class`'s windows (2.1, 2.2), a ring when `room`
/// is under `W_min + 64 MiB` (plans/streaming-window-chunks.md).
pub fn window_plan(class: DeviceClass, i: WindowInputs) -> WindowPlan {
    mq_engine_sched::window_plan(&class.sched(), i)
}

/// A walk build's window of at most `limit` bytes (plans/streaming-window.md
/// 4): under the window's floor, the ring of `limit`, never a refusal.
pub fn window_within(class: DeviceClass, i: WindowInputs, limit: u64) -> WindowPlan {
    window_plan(class, i).within(limit).unwrap_or_else(|| {
        let ring = WindowInputs {
            free: limit + RESERVE_BYTES,
            reclaimable: 0,
            committed: 0,
            ..i
        };
        window_plan(class, ring)
    })
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
    counted_on_disk_keeping(data_dir, &[])
}

/// [`counted_on_disk`] with the torrents of `kept` (lowercase info hashes
/// of offline downloads) neither reclaimable nor idle
/// (plans/offline-downloads.md 4.6).
pub fn counted_on_disk_keeping(data_dir: &Path, kept: &[String]) -> Counted {
    let kept: Vec<std::path::PathBuf> = kept.iter().map(|h| data_dir.join(h)).collect();
    let held: Vec<&Path> = kept.iter().map(std::path::PathBuf::as_path).collect();
    let left = crate::leftovers(data_dir, &held).0;
    Counted {
        reclaimable: left.iter().map(|l| l.2).sum(),
        committed: 0,
        held: crate::counted_on_disk_except(data_dir, &[], &kept),
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
    /// under the reserve a claimed playback's readahead stops, and comes
    /// back over the reserve plus 8 MiB; under the floor it plays on in a
    /// ring window, which frees what is behind the read
    /// (plans/no-space-warnings.md).
    pub(crate) async fn guard_whole_files(&self) {
        let Some(free) = self.free() else {
            return;
        };
        let all: Vec<(TorrentHandle, Arc<Torrent>)> = lock(&self.inner.torrents)
            .iter()
            .map(|(h, t)| (*h, t.clone()))
            .collect();
        for (h, t) in all {
            if !t.claimed() || lock(&t.window).is_some() {
                continue;
            }
            let Some((file, bytes_per_s, readahead)) = *lock(&t.rate) else {
                continue;
            };
            if free < FLOOR_BYTES && !t.unpunched.load(Ordering::Relaxed) {
                if let Some(w) = self.floor_window(h, file, bytes_per_s, false, free).await
                    && let Err(e) = self.window(h, file, w).await
                {
                    tracing::warn!("a whole file under the floor: {e}");
                }
                continue;
            }
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
    /// `file`'s window under the floor, at `free` and counting no idle
    /// torrent (plans/no-space-warnings.md): a ring, in RAM where the
    /// torrent is piece files.
    async fn floor_window(
        &self,
        h: TorrentHandle,
        file: usize,
        bitrate: u64,
        metered: bool,
        free: u64,
    ) -> Option<crate::Window> {
        let plan = match self.window_inputs(h, file, free, bitrate, 0).await {
            Ok(i) => window_plan(
                self.device_class(),
                WindowInputs {
                    reclaimable: 0,
                    ..i
                },
            ),
            Err(e) => {
                tracing::warn!("planning a window at {free} bytes free: {e}");
                return None;
            }
        };
        tracing::info!(
            budget = plan.budget,
            memory = plan.memory,
            free,
            "window under the floor"
        );
        Some(crate::Window {
            plan,
            bitrate,
            metered,
            resume_at: None,
        })
    }

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
        let start = seek(f.as_raw_fd(), at, libc::SEEK_DATA);
        if start < 0 {
            break;
        }
        let end = seek(f.as_raw_fd(), start, libc::SEEK_HOLE);
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

/// `lseek` with a 64-bit offset on every target: 32-bit Android's `off_t`
/// is 32 bits, so its `lseek` would cut a file past 2 GiB
/// (plans/play-every-version.md Step 0b).
#[cfg(unix)]
fn seek(fd: std::os::fd::RawFd, at: i64, whence: libc::c_int) -> i64 {
    // SAFETY: lseek takes plain values.
    #[cfg(any(target_os = "android", target_os = "linux"))]
    // libc 1.0 makes `off_t` 64 bits on 32-bit Android and drops the
    // `*64` names: then this is plain `lseek` again.
    let to = unsafe { libc::lseek64(fd, at, whence) };
    // SAFETY: as above; `off_t` is 64 bits on Apple's platforms.
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    let to = unsafe { libc::lseek(fd, at, whence) };
    to
}

#[cfg(not(unix))]
fn data_pieces(_path: &Path, _offset: u64, _piece_len: u64) -> Vec<u32> {
    Vec::new()
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
        // A RAM ring needs no disk: no floor, no space guard
        // (plans/streaming-window-chunks.md).
        let free = if t.memory.load(Ordering::Relaxed) {
            u64::MAX
        } else {
            engine.free().unwrap_or(u64::MAX)
        };
        // Under the floor the window plays on in a ring: in RAM where the
        // torrent is piece files, else on disk (plans/no-space-warnings.md).
        let ram = !state.plan.memory && t.chunked.load(Ordering::Relaxed);
        if free < FLOOR_BYTES && (!state.plan.ring() || ram) {
            let w = engine
                .floor_window(h, state.file, state.bitrate, state.metered, free)
                .await;
            if let Some(w) = w
                && let Err(e) = engine.set_window(h, state.file, w).await
            {
                tracing::warn!("a window under the floor: {e}");
            }
            continue;
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
        let live = written.saturating_add(free).saturating_sub(RESERVE_BYTES);
        let full = WindowState {
            shortfall: 0,
            ..state
        }
        .budget();
        // Never under the window's floor: what it needs to play
        // (plans/no-space-warnings.md).
        let shortfall = full.saturating_sub(live.max(state.plan.floor));
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
        // Punches that keep failing free nothing: the file plays on whole,
        // never ended for space (plans/no-space-warnings.md).
        if failed.len() as u32 >= FAILED_ROUNDS {
            tracing::warn!("{}: punches fail, playing whole: {failed:?}", t.info_hash);
            t.unpunched.store(true, Ordering::Relaxed);
            engine
                .put_window(t, None, None, Some(state.generation))
                .await;
            break;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::data_pieces;
    use std::io::{Seek, SeekFrom, Write};

    /// Step 0b: data past 2 GiB in a sparse file is found where it is (the
    /// offset a 32-bit `off_t` would cut). Sparse: it takes no disk space.
    #[test]
    fn data_past_2_gib_is_found_where_it_is() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        // A volume that fills a file in (no holes) would take 3 GiB here,
        // and answers nothing past 2 GiB this test could read: a small
        // probe first, and skip there (review P3-1, P3-3).
        let probe = dir.path().join("probe");
        let mut f = std::fs::File::create(&probe).unwrap();
        f.set_len(64 << 20).unwrap();
        f.seek(SeekFrom::End(-1)).unwrap();
        f.write_all(&[1]).unwrap();
        drop(f);
        let blocks = std::fs::metadata(&probe).unwrap().blocks();
        if blocks * 512 >= 32 << 20 {
            eprintln!("skipped: the volume is not sparse ({blocks} blocks for a 64 MiB probe)");
            return;
        }
        let path = dir.path().join("file");
        let mut f = std::fs::File::create(&path).unwrap();
        let at = 5u64 << 29; // 2.5 GiB
        f.set_len(3 << 30).unwrap();
        f.seek(SeekFrom::Start(at)).unwrap();
        f.write_all(&[7u8; 4096]).unwrap();
        drop(f);
        let piece = 1 << 20;
        let pieces = data_pieces(&path, 0, piece);
        assert!(pieces.contains(&((at / piece) as u32)), "{pieces:?}");
        assert!(
            pieces.iter().all(|&p| u64::from(p) >= (2 << 30) / piece),
            "{pieces:?}"
        );
    }
}
