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

/// The same read where the data folder cannot punch holes (the Fire TV's
/// ext4, plans/streaming-window-chunks.md): the engine keeps the video as
/// piece files, the window evicts by deleting them, the folder never holds
/// more than `W`, and every byte is right.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_window_without_punch_holds_its_budget_in_piece_files() {
    let s = support::swarm_big(48 << 20, PIECE).await;
    let root = tempfile::tempdir().unwrap();
    sync_log::fail(root.path(), Fail::PunchRefused, usize::MAX);
    let w = windowed_on(s, root, plan(), None).await;
    let sampler = Sampler::start(w.dir());
    let len = w.s.movie.len() as u64;
    let got = w.read(0, len).await;
    assert!(got == w.s.movie, "bytes differ");
    let max = sampler.max();
    assert!(max <= W, "allocated {max} over {W}");
    let movie = w.dir().join(&w.engine.files(w.t).unwrap()[w.movie].path);
    assert!(movie.is_dir(), "not piece files: {movie:?}");
    let pieces = std::fs::read_dir(&movie).unwrap().count() as u64;
    assert!(pieces * u64::from(PIECE) <= W, "{pieces} piece files kept");
    let s = w.stats().await;
    assert!(s.evict_rounds > 0 && s.held_bytes.unwrap() <= W);
    let evicted = sync_log::under(&w.dir())
        .iter()
        .filter(|e| matches!(e, sync_log::Event::Punched { .. }))
        .count();
    assert!(evicted > 0, "nothing was evicted");
}

/// m. A parked read whose readahead (12 MiB here) wants more than the
/// window has room for: the torrent fills up to the cap. Then, the
/// bitfield's fsync held (a round can free nothing until it ends), a seek:
/// the cap left its urgent windows (blocking and critical, 5 MiB here, 6
/// pieces rounded out) room under `W`, nothing else starts (the grace keeps
/// back the readahead), and the folder stays within `W` (review P2-3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_seek_while_a_round_waits_fetches_only_its_urgent_windows() {
    let long = WindowPlan {
        ahead: 12 * MIB,
        ..plan()
    };
    let w = windowed(48 << 20, long, None).await;
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut head = vec![0u8; 8 * MIB as usize];
    within(30, range.read_exact(&mut head)).await.unwrap();
    // Parked: the readahead fills to the cap, the keeper takes what is
    // behind the read.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let fsync = sync_log::hold(w.root.path(), Point::Fsync);
    let before = allocated(&w.dir());
    let sampler = Sampler::start(w.dir());
    let at = 36 * MIB;
    let mut seek = reader.range(at, None).await.unwrap();
    within(30, seek.ready()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let max = sampler.max();
    fsync.open();
    drop((seek, range));
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

/// b with 16 MiB pieces (review r2): 20 scattered seeks, each a player
/// read of 256 KiB. The folder's peak over the scheduler's (APFS's refills
/// between a round's sync and the keeper's next clear) is printed for the
/// plan's bound, and the folder settles within the window. Run with
/// `TMPDIR` on a small APFS image to measure it there.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scrub_with_large_pieces_settles_within_the_budget() {
    const BIG: u32 = 16 << 20;
    let wide = WindowPlan {
        budget: 160 * MIB + APFS_HEAD_BYTES,
        behind: 16 * MIB,
        slack: 32 * MIB,
        ahead: 16 * MIB,
        open_floor: 64 * MIB,
        open_cap: 64 * MIB,
        floor: 96 * MIB,
        playing: 16 * MIB,
    };
    let s = support::swarm_big(256 << 20, BIG).await;
    let w = windowed_on(s, tempfile::tempdir().unwrap(), wide, None).await;
    let sampler = Sampler::start(w.dir());
    let held = Arc::new(AtomicU64::new(0));
    let watch = tokio::spawn({
        let (engine, t, held) = (w.engine.clone(), w.t, held.clone());
        async move {
            loop {
                let h = engine.stats(t).await.unwrap().held_bytes.unwrap_or(0);
                held.fetch_max(h, Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }
    });
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let len = w.s.movie.len() as u64;
    let mut x = 11u64;
    let mut buf = vec![0u8; 256 * 1024];
    for _ in 0..20 {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let at = (x >> 33) % (len - 256 * 1024);
        let mut range = reader.range(at, None).await.unwrap();
        within(60, range.read_exact(&mut buf)).await.unwrap();
        assert!(buf == w.movie(at, at + 256 * 1024));
    }
    let (peak, held_peak) = (sampler.max(), held.load(Ordering::Relaxed));
    watch.abort();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let settled = allocated(&w.dir());
    eprintln!(
        "scrub16: folder peak {peak}, scheduler peak {held_peak}, settled {settled}, window {}",
        160 * MIB
    );
    assert!(settled <= 160 * MIB, "allocated {settled} once settled");
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
    // A first window large enough that nothing leaves before the trim, so
    // the trim always has a round to hold.
    let first = WindowPlan {
        budget: 40 * MIB + APFS_HEAD_BYTES,
        ..plan()
    };
    let mut w = windowed(48 << 20, first, None).await;
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
    let again = plan();
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
    assert!(s.held_bytes.unwrap() <= W, "{s:?}");
    drop(claim);
}

/// The rate the player gave (`set_rate`) is the window's from then on
/// (review P1-1): the trim keeps 180 s of it ahead of the position, not
/// 180 s of the plan's guess.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_trim_keeps_180_s_at_the_rate_the_player_gave() {
    let first = WindowPlan {
        budget: 40 * MIB + APFS_HEAD_BYTES,
        ..plan()
    };
    let mut w = windowed(48 << 20, first, None).await;
    assert!(w.read(30 * MIB, 40 * MIB).await == w.movie(30 * MIB, 40 * MIB));
    assert!(w.read(20 * MIB, 22 * MIB).await == w.movie(20 * MIB, 22 * MIB));
    // 180 s at twice the plan's 64 KiB/s reaches 44.5 MiB from 22 MiB; at
    // the plan's rate it would end at 33.25 MiB.
    w.engine
        .set_rate(w.t, w.movie, 2 * RATE, false)
        .await
        .unwrap();
    drop(w.claim.take());
    until("the trim", || async {
        w.stats().await.window_bytes.is_none()
    })
    .await;
    for p in [36, 38] {
        let state = test_hooks::piece_state(&w.engine, w.t, p).await;
        assert!(state.contains("Some((true,"), "piece {p}: {state}");
    }
}

/// Up Next in a pack (reviews P2-3, r2 P1-1): the next episode, newly
/// chosen in a live torrent, fits only by counting `R` (the earlier
/// episode's bytes are no room for it whole): it gets the whole-file round
/// (the idle torrent goes), and playing it whole ends the window the
/// earlier episode had. The app's inputs for it count its two boundary
/// pieces (review r3 A R4).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_next_file_of_a_live_torrent_gets_its_round_and_ends_the_window() {
    let s = support::swarm_pack(&[("movie.mkv", 32 << 20), ("e02.mkv", 8 << 20)], PIECE).await;
    let w = windowed_on(s, tempfile::tempdir().unwrap(), plan(), None).await;
    assert!(w.read(0, 16 * MIB).await == w.movie(0, 16 * MIB));
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
    let inputs = w.engine.window_inputs(w.t, 1, 0, RATE, 0).await.unwrap();
    assert_eq!(inputs.boundary, 2 * u64::from(PIECE));
    // Free space a byte short of the next episode plus the reserve.
    test_hooks::set_free(&w.engine, Some((8 << 20) + RESERVE_BYTES - 1));
    let next = w
        .engine
        .select_file(w.t, "default", &FileHint::Name("e02.mkv".into()))
        .await
        .unwrap();
    assert!(matches!(
        w.engine.stats(a).await,
        Err(EngineError::UnknownTorrent)
    ));
    w.engine.play_whole(w.t, next).await.unwrap();
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
    // What the scheduler holds, sampled through the read (review r2 B).
    let held = Arc::new(AtomicU64::new(0));
    let watch = tokio::spawn({
        let (engine, t, held) = (w.engine.clone(), w.t, held.clone());
        async move {
            loop {
                if let Ok(s) = engine.stats(t).await {
                    held.fetch_max(s.held_bytes.unwrap_or(0), Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    });
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut got = Vec::new();
    let err = within(60, range.read_to_end(&mut got)).await.unwrap_err();
    watch.abort();
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
    // Nothing past the budget but what 2.4's ceiling lets the player's
    // blocking window take while the rounds fail: that window rounded out
    // to pieces, plus a piece (2 MiB here).
    let max = allocated(&w.dir());
    assert!(max <= W + 2 * MIB, "no fetch past the ceiling: {max}");
    let held = held.load(Ordering::Relaxed);
    assert!(
        held > 0 && held <= W + 2 * MIB,
        "held past the ceiling: {held}"
    );
}

/// f, second half (reviews P1-1, r2 P2-2). Free space that follows the
/// data folder, and another app's write taking it 6 MiB under the reserve
/// while the player's one range stays open, as mpv's and ExoPlayer's do:
/// the window gives the 6 MiB back, the keeper goes on evicting (the read
/// is still the player's), the readahead keeps its setting, and the same
/// range reads on to the end. The shrunk budget leaves no room for the
/// whole readahead beside a seek's urgent windows, so its far part leaves.
/// When the other app deletes its file, the window grows back, and what
/// the cut took off the readahead is fetched again once, nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shortfall_shrinks_the_window_and_the_read_finishes() {
    // 40 MiB: a full window holds about 34, its kept set about 14, so it
    // has 6 MiB to give.
    let wide = WindowPlan {
        budget: 40 * MIB + APFS_HEAD_BYTES,
        ..plan()
    };
    let w = windowed(64 << 20, wide, None).await;
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut buf = vec![0u8; 30 * MIB as usize];
    within(60, range.read_exact(&mut buf)).await.unwrap();
    assert!(buf == w.movie(0, 30 * MIB));
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before = w.stats().await;
    let base = RESERVE_BYTES - 6 * MIB + allocated(w.root.path());
    test_hooks::set_free_base(&w.engine, Some(base));
    let (shrunk, rounds) = (before.held_bytes.unwrap() - 6 * MIB, before.evict_rounds);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let sampler = Sampler::start(w.dir());
    let mut buf = vec![0u8; 10 * MIB as usize];
    within(60, range.read_exact(&mut buf)).await.unwrap();
    assert!(buf == w.movie(30 * MIB, 40 * MIB));
    let s = w.stats().await;
    assert!(s.evict_rounds > rounds, "the keeper went on: {s:?}");
    assert!(s.held_bytes.unwrap() <= shrunk, "{s:?}");
    assert!(sampler.max() <= 40 * MIB, "{}", sampler.max());
    assert_eq!(
        s.readahead_bytes,
        Some(plan().ahead),
        "the readahead came back: {s:?}"
    );
    assert!(
        w.engine.free().unwrap() >= RESERVE_BYTES,
        "free space came back"
    );
    // The other app deletes its file: the window grows back to `W`.
    test_hooks::set_free_base(&w.engine, Some(base + 6 * MIB + 32 * MIB));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let held = Arc::new(AtomicU64::new(0));
    let watch = tokio::spawn({
        let (engine, t, held) = (w.engine.clone(), w.t, held.clone());
        async move {
            loop {
                let h = engine.stats(t).await.unwrap().held_bytes.unwrap_or(0);
                held.fetch_max(h, Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    });
    let len = w.s.movie.len() as u64;
    let mut rest = Vec::new();
    within(60, range.read_to_end(&mut rest)).await.unwrap();
    watch.abort();
    assert!(rest == w.movie(40 * MIB, len));
    let max = held.load(Ordering::Relaxed);
    assert!(max > shrunk, "the window grew back: {max} after {shrunk}");
    let again = w.stats().await.refetched_bytes;
    assert!(again <= plan().ahead + MIB, "fetched again: {again}");
    test_hooks::set_free_base(&w.engine, None);
}

/// f, a sustained write (review r3 B P2-1): another app writes 10 MiB/s
/// for 12 s while the player reads at 2.5 MiB/s from a window holding a
/// remux-sized readahead (96 MiB). The window gives its readahead back as
/// the space goes, farthest part first, so free space never falls under
/// the reserve, the read goes on, and nothing is fetched twice.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sustained_write_takes_the_readahead_farthest_first() {
    let remux = WindowPlan {
        budget: 160 * MIB + APFS_HEAD_BYTES,
        behind: 4 * MIB,
        slack: 8 * MIB,
        ahead: 96 * MIB,
        open_floor: 8 * MIB,
        open_cap: 8 * MIB,
        floor: 40 * MIB,
        playing: 6 * MIB,
    };
    let w = windowed(256 << 20, remux, None).await;
    let mut base = RESERVE_BYTES + 170 * MIB + allocated(w.root.path());
    test_hooks::set_free_base(&w.engine, Some(base));
    let reader = w.engine.open_reader(w.t, "default", w.movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut buf = vec![0u8; 50 * MIB as usize];
    within(60, range.read_exact(&mut buf)).await.unwrap();
    assert!(buf == w.movie(0, 50 * MIB));
    // The readahead fills: 96 MiB past the playback window.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let before = w.stats().await.held_bytes.unwrap();
    assert!(before > 120 * MIB, "the window filled: {before}");
    let min_free = Arc::new(AtomicU64::new(u64::MAX));
    let watch = tokio::spawn({
        let (engine, min) = (w.engine.clone(), min_free.clone());
        async move {
            loop {
                min.fetch_min(engine.free().unwrap(), Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    });
    let mut at = 50 * MIB;
    let mut chunk = vec![0u8; 256 * 1024];
    let started = Instant::now();
    for tick in 1..=120u64 {
        // The other app: 1 MiB every 100 ms.
        base -= MIB;
        test_hooks::set_free_base(&w.engine, Some(base));
        within(30, range.read_exact(&mut chunk)).await.unwrap();
        assert!(chunk == w.movie(at, at + 256 * 1024));
        at += 256 * 1024;
        let next = started + Duration::from_millis(100 * tick);
        tokio::time::sleep_until(next.into()).await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    watch.abort();
    let min = min_free.load(Ordering::Relaxed);
    let s = w.stats().await;
    eprintln!(
        "writer: held {before} then {:?}, lowest free {} KiB over the reserve",
        s.held_bytes,
        (min as i64 - RESERVE_BYTES as i64) / 1024
    );
    assert!(min >= RESERVE_BYTES, "free space fell under the reserve");
    assert!(s.held_bytes.unwrap() + 80 * MIB <= before, "{s:?}");
    assert_eq!(s.refetched_bytes, 0, "nothing fetched twice");
    // The readahead that fits the shrunk budget stays (past the playback
    // window, about 7 MiB on), the farthest left.
    let have = |p: u32| {
        let engine = w.engine.clone();
        let t = w.t;
        async move {
            test_hooks::piece_state(&engine, t, p)
                .await
                .contains("Some((true,")
        }
    };
    let pos = (at / MIB) as u32;
    assert!(have(pos + 12).await, "the readahead that fits stays");
    assert!(!have(pos + 60).await, "the farthest readahead left");
    test_hooks::set_free_base(&w.engine, None);
}

/// 2.2: on mobile data a window keeps the device preset ahead, and the
/// network turning metered mid-play takes it there; back on Wi-Fi the
/// plan's readahead returns (review r1 B P3-5).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_window_follows_the_network_to_the_mobile_preset_and_back() {
    let remux = WindowPlan {
        budget: 400 * MIB + APFS_HEAD_BYTES,
        ahead: 300 * MIB,
        ..plan()
    };
    let w = windowed(16 << 20, remux, None).await;
    let readahead = || async { w.stats().await.readahead_bytes };
    // The app gives the rate on Wi-Fi at open.
    w.engine.set_rate(w.t, w.movie, RATE, false).await.unwrap();
    until("the plan's readahead", || async {
        readahead().await == Some(300 * MIB)
    })
    .await;
    w.engine.set_metered(true).await;
    until("the device preset", || async {
        readahead().await.is_some_and(|r| r < 300 * MIB)
    })
    .await;
    w.engine.set_metered(false).await;
    until("the plan's readahead again", || async {
        readahead().await == Some(300 * MIB)
    })
    .await;
}

/// 2.4: a whole file under the reserve keeps no readahead until free space
/// is back; a network or rate change meanwhile does not bring it back
/// (review r2 B P3-1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_starved_whole_file_stays_starved_through_a_network_change() {
    let s = support::swarm_big(16 << 20, PIECE).await;
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
    engine.set_rate(t, movie, RATE, false).await.unwrap();
    let readahead = || async { engine.stats(t).await.unwrap().readahead_bytes };
    until("a readahead", || async {
        readahead().await.is_some_and(|r| r > 0)
    })
    .await;
    test_hooks::set_free(&engine, Some(RESERVE_BYTES - 1));
    until("starved under the reserve", || async {
        readahead().await == Some(0)
    })
    .await;
    engine.set_metered(true).await;
    engine.set_rate(t, movie, RATE, false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(readahead().await, Some(0), "still starved");
    test_hooks::set_free(&engine, None);
    until("fed again", || async {
        readahead().await.is_some_and(|r| r > 0)
    })
    .await;
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
        ..Default::default()
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
                ..Default::default()
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
