//! The window keeper (plans/streaming-window.md 2.1 to 2.4, phase 2): a
//! file plays while the torrent holds only its window. The swarm is a
//! 37,000-byte `.nfo` then a video of a few dozen MiB in 1 MiB pieces on
//! loopback; the leecher runs upload off with DWH driving, and every window
//! here has a small plan of its own, so the bounds do not depend on this
//! machine's free space.

#![cfg(feature = "test-hooks")]

mod support;

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mq_engine::librqbit::sync_log::{self, Fail, Point};
use mq_engine::{
    APFS_HEAD_BYTES, Claim, Engine, EngineError, FileHint, RESERVE_BYTES, TorrentHandle, Window,
    WindowPlan, test_hooks,
};
use tokio::io::AsyncReadExt;

const MIB: u64 = 1 << 20;
const PIECE: u32 = 1 << 20;
/// 64 KiB/s: a 4 MiB critical window, a 1.9 MiB playback window.
const RATE: u64 = 64 * 1024;

/// The scheduler's budget in these tests: the plan's `W` less what APFS
/// keeps of a file's head (on Apple targets), which the plan holds apart.
const W: u64 = 24 * MIB;

fn plan() -> WindowPlan {
    WindowPlan {
        budget: W + APFS_HEAD_BYTES,
        behind: 2 * MIB,
        slack: 4 * MIB,
        ahead: 4 * MIB,
        open_floor: 2 * MIB,
        open_cap: 4 * MIB,
        floor: 12 * MIB,
        playing: 6 * MIB,
    }
}

struct Windowed {
    s: support::Swarm,
    root: tempfile::TempDir,
    engine: Engine,
    t: TorrentHandle,
    movie: usize,
    claim: Option<Claim>,
}

impl Windowed {
    fn dir(&self) -> std::path::PathBuf {
        self.root.path().join(&self.s.info_hash)
    }

    async fn read(&self, from: u64, to: u64) -> Vec<u8> {
        let reader = self
            .engine
            .open_reader(self.t, "default", self.movie)
            .await
            .unwrap();
        let mut range = reader.range(from, Some(to)).await.unwrap();
        let mut got = Vec::new();
        within(60, range.read_to_end(&mut got)).await.unwrap();
        got
    }

    fn movie(&self, from: u64, to: u64) -> &[u8] {
        &self.s.movie[from as usize..to as usize]
    }

    async fn stats(&self) -> mq_engine::TorrentStats {
        self.engine.stats(self.t).await.unwrap()
    }
}

/// A leecher on a fresh folder with the video selected, claimed and
/// windowed with `plan`.
async fn windowed(len: usize, plan: WindowPlan, resume_at: Option<u64>) -> Windowed {
    let s = support::swarm_big(len, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    windowed_on(s, root, plan, resume_at).await
}

async fn windowed_on(
    s: support::Swarm,
    root: tempfile::TempDir,
    plan: WindowPlan,
    resume_at: Option<u64>,
) -> Windowed {
    let engine = engine_on(root.path()).await;
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let claim = Some(engine.claim(t, "default").unwrap());
    let w = Window {
        plan,
        bitrate: RATE,
        metered: false,
        resume_at,
    };
    engine.window(t, movie, w).await.unwrap();
    Windowed {
        s,
        root,
        engine,
        t,
        movie,
        claim,
    }
}

/// The folder's allocated bytes, sampled every 2 ms until dropped.
struct Sampler {
    max: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Sampler {
    fn start(dir: std::path::PathBuf) -> Self {
        let (max, stop) = (
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicBool::new(false)),
        );
        std::thread::spawn({
            let (max, stop) = (max.clone(), stop.clone());
            move || {
                while !stop.load(Ordering::Relaxed) {
                    max.fetch_max(allocated(&dir), Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        });
        Self { max, stop }
    }

    fn max(&self) -> u64 {
        self.max.load(Ordering::Relaxed)
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// a, j, m. A linear read of 48 MiB in a 24 MiB window: the folder never
/// holds more than `W`, the head and the index stay, the keeper ran at
/// most once per `h / 2` read, and every byte is right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_window_never_holds_more_than_its_budget() {
    let w = windowed(48 << 20, plan(), None).await;
    let sampler = Sampler::start(w.dir());
    let len = w.s.movie.len() as u64;
    let got = w.read(0, len).await;
    assert!(got == w.s.movie, "bytes differ");
    let max = sampler.max();
    assert!(max <= W, "allocated {max} over {}", W);
    let s = w.stats().await;
    assert!(s.held_bytes.unwrap() <= W);
    assert_eq!(
        s.readahead_bytes,
        Some(plan().ahead),
        "the window's readahead"
    );
    assert!(s.evict_rounds > 0, "the keeper's rounds are counted");
    // The head's piece and the tail (the index) are still there.
    let last = s.pieces_total - 1;
    for p in [0, last - 1, last] {
        let state = test_hooks::piece_state(&w.engine, w.t, p).await;
        assert!(state.contains("Some((true,"), "{state}");
    }
    let rounds = sync_log::under(&w.dir())
        .iter()
        .filter(|e| matches!(e, sync_log::Event::Punched { .. }))
        .count() as u64;
    assert!(rounds > 0, "nothing was evicted");
    assert!(
        rounds <= len / (plan().slack / 2),
        "{rounds} rounds for {len} bytes"
    );
}

/// m. The bitfield's fsync held (a round can free nothing until it ends),
/// and a seek meanwhile: the cap leaves a seek's urgent windows (blocking
/// and critical, 5 MiB here, 6 pieces rounded out) room under `W`, and
/// nothing else starts (the grace keeps back the readahead), so the
/// folder stays within `W` (review P2-3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_seek_while_a_round_waits_fetches_only_its_urgent_windows() {
    let w = windowed(48 << 20, plan(), None).await;
    let fsync = sync_log::hold(w.root.path(), Point::Fsync);
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut buf = vec![0u8; 64 * 1024];
    let started = Instant::now();
    while !fsync.reached() {
        within(30, range.read(&mut buf)).await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(30), "no round");
    }
    drop(range);
    let before = allocated(&w.dir());
    let sampler = Sampler::start(w.dir());
    let at = 36 * MIB;
    let mut seek = reader.range(at, None).await.unwrap();
    within(30, seek.ready()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let max = sampler.max();
    fsync.open();
    drop(seek);
    // A half piece over for the file system's own blocks.
    let bound = W + MIB / 2;
    assert!(
        max <= bound,
        "allocated {max} over {bound} ({before} before the seek)"
    );
}

/// b. Scrub: 30 seeks across a 256 MiB file, each a player read of 256 KiB:
/// once the keeper's next looks have run the folder is within `W`, and
/// every byte is right. On this Mac's APFS the data sync after a round's
/// punch fills the holes between nearby small extents (the seeks' orphans):
/// 14 MiB allocated became 67 MiB with the scheduler holding 20; the keeper
/// punches the filled pieces again within about 100 ms, so the peak the
/// 2 ms sampler sees is printed, not bounded.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scrub_stays_within_the_budget() {
    let w = windowed(256 << 20, plan(), None).await;
    let sampler = Sampler::start(w.dir());
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let len = w.s.movie.len() as u64;
    let mut x = 7u64;
    let mut buf = vec![0u8; 256 * 1024];
    for _ in 0..30 {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let at = (x >> 33) % (len - 256 * 1024);
        let mut range = reader.range(at, None).await.unwrap();
        within(30, range.read_exact(&mut buf)).await.unwrap();
        assert!(buf == w.movie(at, at + 256 * 1024));
    }
    let max = sampler.max();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let settled = allocated(&w.dir());
    eprintln!("scrub: peak {max}, settled {settled}, budget {}", W);
    assert!(settled <= W, "allocated {settled} once settled");
    assert!(w.stats().await.evict_rounds > 0);
}

/// c. A seek back within `B` reads from disk (no piece verified again);
/// further back the pieces are fetched again, and the bytes are right.
/// The playing read stays open at 30 MiB with 8 MiB of readahead, so its
/// kept set fills the window and the keeper takes every piece behind it
/// but `B`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_seek_back_within_b_asks_nothing() {
    let plan = WindowPlan {
        ahead: 8 * MIB,
        ..plan()
    };
    let w = windowed(64 << 20, plan, None).await;
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut playing = reader.range(0, None).await.unwrap();
    let mut head = vec![0u8; (30 * MIB) as usize];
    within(60, playing.read_exact(&mut head)).await.unwrap();
    assert!(head == w.movie(0, 30 * MIB));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let back = 30 * MIB - MIB;
    assert!(w.read(back, back + 512 * 1024).await == w.movie(back, back + 512 * 1024));
    assert_eq!(w.stats().await.refetched_bytes, 0, "within B: from disk");
    let far = 10 * MIB;
    assert!(w.read(far, far + MIB).await == w.movie(far, far + MIB));
    assert!(
        w.stats().await.refetched_bytes > 0,
        "beyond B: fetched again"
    );
    drop(playing);
}

/// d. With no reader the anchor keeps `[position - B, playback end)`: a
/// read 3 MiB back from where the last one stopped fetches nothing again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_reader_then_a_read_behind_the_anchor_asks_nothing() {
    let plan = WindowPlan {
        behind: 4 * MIB,
        ..plan()
    };
    let w = windowed(48 << 20, plan, None).await;
    assert!(w.read(0, 30 * MIB).await == w.movie(0, 30 * MIB));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let back = 27 * MIB;
    assert!(w.read(back, back + MIB).await == w.movie(back, back + MIB));
    assert_eq!(w.stats().await.refetched_bytes, 0);
}

/// e and p. When the last claim drops the keeper trims the torrent to the
/// head, the index and `[position - B, position + 180 s × r]`; after a
/// relaunch a window resumed there opens the head (a probe), two hash
/// ranges, a player read at the head and one at the index, and nothing
/// near the resume point is fetched again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_trimmed_window_resumes_warm() {
    let w = windowed(48 << 20, plan(), None).await;
    let at = 30 * MIB;
    assert!(w.read(0, at).await == w.movie(0, at));
    let Windowed {
        s,
        root,
        engine,
        t,
        claim,
        ..
    } = w;
    drop(claim);
    until("the trim", || async {
        engine.stats(t).await.unwrap().window_bytes.is_none()
    })
    .await;
    // The head (pieces 0 and 1), the index (46 to 48) and, of 28 to 41
    // (180 s at 64 KiB/s is 11.25 MiB), what was there stay.
    let have = |p: u32| {
        let engine = engine.clone();
        async move {
            test_hooks::piece_state(&engine, t, p)
                .await
                .contains("Some((true,")
        }
    };
    let mut held = Vec::new();
    for p in 0..49 {
        if have(p).await {
            held.push(p);
        }
    }
    let kept = |p: &u32| [0, 1].contains(p) || (28..=41).contains(p) || (46..=48).contains(p);
    assert!(
        held.iter().all(kept),
        "trimmed to the kept pieces: {held:?}"
    );
    assert!(held.contains(&29) && held.contains(&47), "{held:?}");
    drop(engine);
    let len = s.movie.len() as u64;
    // A 10 MiB window, so the reads at the head and the index fill it past
    // its trigger by more than the head pieces they leave behind, and the
    // reader waits out the index read's grace.
    let small = WindowPlan {
        budget: 10 * MIB + APFS_HEAD_BYTES,
        ..plan()
    };
    let w = windowed_on(s, root, small, Some(at)).await;
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut probes = Vec::new();
    for (from, to) in [(0, MIB), (0, 64 * 1024), (len - 64 * 1024, len)] {
        let mut r = reader.range(from, Some(to)).await.unwrap();
        within(30, r.ready()).await.unwrap();
        probes.push(r);
    }
    // The player's header read: nothing behind it to evict, so a round
    // past the trigger takes the resume window unless it is kept.
    assert!(w.read(0, 2 * MIB).await == w.movie(0, 2 * MIB));
    let mut index = reader.range(len - 2 * MIB, None).await.unwrap();
    within(30, index.ready()).await.unwrap();
    drop(index);
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(w.read(at, at + 2 * MIB).await == w.movie(at, at + 2 * MIB));
    assert_eq!(
        w.stats().await.refetched_bytes,
        0,
        "fetched again near {at}"
    );
}

/// p, a playback stopped before the keeper's first look (well under a
/// second): its window is trimmed and ends all the same.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_playback_stopped_at_once_is_trimmed() {
    let mut w = windowed(48 << 20, plan(), None).await;
    drop(w.claim.take());
    until("the trim", || async {
        w.stats().await.window_bytes.is_none()
    })
    .await;
}

/// p, a reopen (review P2-1): the same file opens again while the trim's
/// round runs. The new window stays, and its keeper evicts as the read
/// goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_window_set_during_the_trim_stays_and_is_kept() {
    let mut w = windowed(48 << 20, plan(), None).await;
    assert!(w.read(0, 16 * MIB).await == w.movie(0, 16 * MIB));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let gated = sync_log::hold(w.root.path(), Point::Gated);
    drop(w.claim.take());
    let started = Instant::now();
    while !gated.reached() {
        assert!(started.elapsed() < Duration::from_secs(10), "no trim");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let claim = w.engine.claim(w.t, "default").unwrap();
    let again = WindowPlan {
        budget: plan().budget + 4 * MIB,
        ..plan()
    };
    let window = Window {
        plan: again,
        bitrate: RATE,
        metered: false,
        resume_at: None,
    };
    w.engine.window(w.t, w.movie, window).await.unwrap();
    gated.open();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(w.stats().await.window_bytes, Some(again.budget));
    let rounds = w.stats().await.evict_rounds;
    let len = w.s.movie.len() as u64;
    assert!(w.read(16 * MIB, len).await == w.movie(16 * MIB, len));
    let s = w.stats().await;
    assert!(s.evict_rounds > rounds, "the new window's keeper: {s:?}");
    assert!(s.held_bytes.unwrap() <= W + 4 * MIB, "{s:?}");
    drop(claim);
}

/// Up Next in a pack (review P2-3): a file newly chosen in a live torrent
/// that fits only by counting `R` gets the whole-file round (the idle
/// torrent goes), and playing it whole ends the window its other file had.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_next_file_of_a_live_torrent_gets_its_round_and_ends_the_window() {
    let w = windowed(32 << 20, plan(), None).await;
    assert!(w.read(0, 4 * MIB).await == w.movie(0, 4 * MIB));
    let idle = support::swarm(true).await;
    let a = w
        .engine
        .add_magnet(&idle.info_hash, &[], &[idle.seeder_addr])
        .await
        .unwrap();
    let movie = w
        .engine
        .select_file(a, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let reader = w.engine.open_reader(a, "default", movie).await.unwrap();
    let mut r = reader.range(0, None).await.unwrap();
    within(30, r.read_to_end(&mut Vec::new())).await.unwrap();
    drop((r, reader));
    // The `.nfo` with free space short of it plus the reserve, past what
    // the torrent holds, by less than the idle torrent holds.
    let held = allocated(&w.dir());
    test_hooks::set_free(&w.engine, Some(37_000 + RESERVE_BYTES - held - 1));
    let nfo = w
        .engine
        .select_file(w.t, "default", &FileHint::Name("a.nfo".into()))
        .await
        .unwrap();
    assert!(matches!(
        w.engine.stats(a).await,
        Err(EngineError::UnknownTorrent)
    ));
    w.engine.play_whole(w.t, nfo).await.unwrap();
    let s = w.stats().await;
    assert_eq!(s.window_bytes, None);
    until("no window in the scheduler", || async {
        w.stats().await.held_bytes.is_none()
    })
    .await;
    test_hooks::set_free(&w.engine, None);
}

/// f. Every punch refused: after three failed rounds the playback ends,
/// before any write fails, and ranges fail with the reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refused_punches_end_the_playback_cleanly() {
    let s = support::swarm_big(48 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join(&s.info_hash);
    std::fs::create_dir_all(&folder).unwrap();
    sync_log::fail(&folder, Fail::PunchRefused, usize::MAX);
    let w = windowed_on(s, root, plan(), None).await;
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut got = Vec::new();
    let err = within(60, range.read_to_end(&mut got)).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "this storage cannot free space while playing"
    );
    assert!(
        got[..] == w.s.movie[..got.len()],
        "every byte read is right"
    );
    match reader.range(0, None).await {
        Err(EngineError::Storage(why)) => {
            assert_eq!(why, "this storage cannot free space while playing")
        }
        other => panic!("{:?}", other.err()),
    }
    let max = allocated(&w.dir());
    assert!(max <= W, "no fetch past the budget: {max}");
}

/// f, second half (review P1-1). Free space forced under the reserve while
/// the player's one range stays open, as mpv's and ExoPlayer's do: the
/// window gives back the shortfall and its readahead, the keeper goes on
/// evicting (the read is still the player's), the torrent holds no more
/// than its budget less the shortfall, and the same range reads on to the
/// end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shortfall_shrinks_the_window_and_the_read_finishes() {
    let w = windowed(48 << 20, plan(), None).await;
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut head = vec![0u8; 10 * MIB as usize];
    within(60, range.read_exact(&mut head)).await.unwrap();
    assert!(head == w.movie(0, 10 * MIB));
    test_hooks::set_free(&w.engine, Some(RESERVE_BYTES - 6 * MIB));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let rounds = w.stats().await.evict_rounds;
    let len = w.s.movie.len() as u64;
    let sampler = Sampler::start(w.dir());
    let mut rest = Vec::new();
    within(60, range.read_to_end(&mut rest)).await.unwrap();
    assert!(rest == w.movie(10 * MIB, len));
    let max = sampler.max();
    assert!(max <= W, "{max}");
    let s = w.stats().await;
    assert_eq!(s.readahead_bytes, Some(0));
    assert!(s.evict_rounds > rounds, "the keeper went on: {s:?}");
    assert!(s.held_bytes.unwrap() <= W - 6 * MIB, "{s:?}");
    test_hooks::set_free(&w.engine, None);
}

/// q. Under the terminal floor (a quarter of the reserve) the playback ends
/// with "storage full" before any write fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn under_the_floor_the_playback_ends_with_storage_full() {
    let w = windowed(48 << 20, plan(), None).await;
    assert!(w.read(0, 4 * MIB).await == w.movie(0, 4 * MIB));
    test_hooks::set_free(&w.engine, Some(RESERVE_BYTES / 4 - 1));
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    until("storage full", || async {
        matches!(reader.range(0, None).await, Err(EngineError::Storage(ref why)) if why == "storage full")
    })
    .await;
    assert!(w.stats().await.paused, "fetching stopped");
    // The scheduler's window and cap end too (review P2-5).
    until("no window in the scheduler", || async {
        w.stats().await.held_bytes.is_none()
    })
    .await;
}

/// q for a whole file: under the floor a playback with no window ends
/// with "storage full" too (the maintenance tick).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn under_the_floor_a_whole_file_ends_with_storage_full() {
    let s = support::swarm(true).await;
    let root = tempfile::tempdir().unwrap();
    let engine = engine_on(root.path()).await;
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let _claim = engine.claim(t, "default").unwrap();
    let reader = engine.open_reader(t, "default", movie).await.unwrap();
    within(30, reader.range(0, Some(MIB)).await.unwrap().ready())
        .await
        .unwrap();
    test_hooks::set_free(&engine, Some(RESERVE_BYTES / 4 - 1));
    until("storage full", || async {
        matches!(reader.range(0, None).await, Err(EngineError::Storage(ref why)) if why == "storage full")
    })
    .await;
}

/// g. A window's budget round deletes idle torrents, oldest first, until
/// free space is `W - held + reserve`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_torrents_go_before_a_window_grows() {
    let idle = support::swarm(true).await;
    let s = support::swarm_big(32 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let engine = engine_on(root.path()).await;
    let a = engine
        .add_magnet(&idle.info_hash, &[], &[idle.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(a, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let reader = engine.open_reader(a, "default", movie).await.unwrap();
    let mut r = reader.range(0, None).await.unwrap();
    within(30, r.read_to_end(&mut Vec::new())).await.unwrap();
    drop((r, reader));
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let _claim = engine.claim(t, "default").unwrap();
    test_hooks::set_free(&engine, Some(plan().budget + RESERVE_BYTES - MIB));
    let w = Window {
        plan: plan(),
        bitrate: RATE,
        metered: false,
        resume_at: None,
    };
    engine.window(t, movie, w).await.unwrap();
    assert!(matches!(
        engine.stats(a).await,
        Err(EngineError::UnknownTorrent)
    ));
    assert!(!root.path().join(&idle.info_hash).exists());
}

/// n. A whole file that fits only by counting `R`: its file choice deletes
/// the idle torrents first, and the read finishes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_whole_file_that_fits_only_through_r_deletes_idle_torrents_first() {
    let idle = support::swarm(true).await;
    let s = support::swarm_big(16 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    let engine = engine_on(root.path()).await;
    let a = engine
        .add_magnet(&idle.info_hash, &[], &[idle.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(a, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let reader = engine.open_reader(a, "default", movie).await.unwrap();
    let mut r = reader.range(0, None).await.unwrap();
    within(30, r.read_to_end(&mut Vec::new())).await.unwrap();
    drop((r, reader));
    // Free space short of the file plus the reserve by 1 MiB, which the
    // idle 5 MiB covers.
    let size = s.movie.len() as u64;
    test_hooks::set_free(&engine, Some(size + RESERVE_BYTES - MIB));
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    assert!(matches!(
        engine.stats(a).await,
        Err(EngineError::UnknownTorrent)
    ));
    let reader = engine.open_reader(t, "default", movie).await.unwrap();
    let mut r = reader.range(0, None).await.unwrap();
    let mut got = Vec::new();
    within(30, r.read_to_end(&mut got)).await.unwrap();
    assert!(got == s.movie);
}

/// i. Two windows at once: each counts what the other will still take.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_window_counts_what_the_other_will_take() {
    let a = windowed(24 << 20, plan(), None).await;
    assert!(a.read(0, 8 * MIB).await == a.movie(0, 8 * MIB));
    let held = a.stats().await.held_bytes.unwrap();
    assert!(held > 0);
    assert_eq!(a.engine.committed(Some(a.t)).await, 0);
    assert_eq!(a.engine.committed(None).await, plan().budget - held);
}

/// h. A round held in its bitfield fsync for a second: the adapter's loop
/// answers within two ticks all the while (the keeper runs beside it, so
/// no request waits on a disk write), and the read goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_fsync_never_stalls_the_adapter() {
    let w = windowed(48 << 20, plan(), None).await;
    let fsync = sync_log::hold(w.root.path(), Point::Fsync);
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut buf = vec![0u8; 64 * 1024];
    let started = Instant::now();
    while !fsync.reached() {
        within(30, range.read(&mut buf)).await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(30), "no round");
    }
    let held = Instant::now();
    let mut slowest = Duration::ZERO;
    while held.elapsed() < Duration::from_secs(1) {
        let asked = Instant::now();
        w.engine.peer_report(w.t).await.unwrap();
        slowest = slowest.max(asked.elapsed());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    fsync.open();
    assert!(
        slowest < Duration::from_millis(200),
        "the adapter waited {slowest:?}"
    );
    within(30, range.read_to_end(&mut Vec::new()))
        .await
        .unwrap();
}

/// o. Room under `W_min + 64 MiB`: `DeviceFull`, before any write.
#[test]
fn a_device_under_the_window_floor_is_full() {
    let inputs = mq_engine::WindowInputs {
        free: RESERVE_BYTES + 32 * MIB,
        reclaimable: 0,
        committed: 0,
        bitrate: 4_775_000,
        piece_len: 4 * MIB,
        fill: 5_000_000,
    };
    assert!(matches!(
        mq_engine::window_plan(mq_engine::DeviceClass::Standard, inputs),
        Err(EngineError::DeviceFull)
    ));
}

/// 2.1's worked numbers: a 40 GiB remux at 4.775 MB/s on a phone with
/// 10 GB free needs about 3.6 GB; Tears of Steel with 1.05 GB free gets
/// about 0.51 GB.
#[test]
fn the_window_plan_matches_the_worked_numbers() {
    let plan = |free: u64, bitrate: u64, piece_len: u64| {
        mq_engine::window_plan(
            mq_engine::DeviceClass::Standard,
            mq_engine::WindowInputs {
                free,
                reclaimable: 0,
                committed: 0,
                bitrate,
                piece_len,
                fill: 5_000_000,
            },
        )
        .unwrap()
    };
    let remux = plan(10_000_000_000, 4_775_000, 16 * MIB);
    assert!(
        (3_300_000_000..3_900_000_000).contains(&remux.budget),
        "{remux:?}"
    );
    let tos = plan(1_050_000_000, 778_400, 512 * 1024);
    assert!((480_000_000..540_000_000).contains(&tos.budget), "{tos:?}");
    assert!(tos.ahead > 0 && tos.ahead <= 600 * 778_400);
    // The walk override lowers the budget, the readahead first and then
    // `B` by what is left (review P2-6), so the kept set still fits; it
    // never goes under the floor.
    let walk = tos.within(200_000_000).unwrap();
    assert_eq!(walk.budget, 200_000_000);
    assert_eq!(
        tos.budget - walk.budget,
        (tos.ahead - walk.ahead) + (tos.behind - walk.behind)
    );
    let tight = tos.within(tos.floor + 10 * MIB).unwrap();
    assert_eq!(tight.ahead, 0);
    assert!(
        tight.floor + tight.behind <= tight.budget,
        "the kept set fits: {tight:?}"
    );
    assert!(tos.within(tos.floor - 1).is_none());
    assert!(tos.ahead_secs(778_400) >= 120);
}

async fn engine_on(dir: &Path) -> Engine {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let config = mq_engine::EngineConfig {
        discovery: false,
        upload: false,
        ..mq_engine::EngineConfig::new(
            dir,
            http.torrent_scope(&[std::net::Ipv4Addr::LOCALHOST.into()]),
        )
    };
    for _ in 0..40 {
        if let Ok(engine) = Engine::start(config.clone()).await {
            return engine;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Engine::start(config).await.unwrap()
}

/// Allocated bytes under `dir`.
fn allocated(dir: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => allocated(&e.path()),
            Ok(m) => m.blocks() * 512,
            Err(_) => 0,
        })
        .sum()
}

async fn within<T>(secs: u64, f: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(secs), f)
        .await
        .expect("in time")
}

async fn until<F: Future<Output = bool>>(what: &str, mut f: impl FnMut() -> F) {
    let started = Instant::now();
    while !f().await {
        assert!(started.elapsed() < Duration::from_secs(30), "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
