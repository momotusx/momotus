//! Eviction rounds (plans/streaming-window.md 2.5, phase 1): pieces leave
//! the disk while readers, writers and pauses go on, and the bitfield on
//! disk never claims a piece whose bytes are gone. The swarm is a 37,000
//! byte `.nfo` then a 5 MiB video in 256 KiB pieces, so every piece edge in
//! the video sits 3,960 bytes into a 4 KiB block. The leecher runs as a
//! window does: upload off, DWH driving.

#![cfg(feature = "test-hooks")]

mod support;

use std::future::Future;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mq_engine::librqbit::sync_log::{self, Event, Fail, Point};
use mq_engine::{Engine, FileHint, TorrentHandle, test_hooks};
use tokio::io::AsyncReadExt;

const PIECE: u64 = 256 * 1024;
const NFO: u64 = 37_000;
const BLOCK: u64 = 16 * 1024;
/// The filesystem block of APFS and ext4.
const FS_BLOCK: u64 = 4096;

struct Leech {
    s: support::Swarm,
    /// The leecher, upload off; it stops before its folder goes.
    engine: Engine,
    root: tempfile::TempDir,
    t: TorrentHandle,
    file: usize,
    _seeders: Vec<support::Seeder>,
}

impl Leech {
    fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The leecher's data folder: the torrent's folder and its bitfield.
    fn root(&self) -> &Path {
        self.root.path()
    }

    /// The torrent's folder, which a round's and a stream's holds are
    /// keyed by.
    fn dir(&self) -> PathBuf {
        self.root().join(&self.s.info_hash)
    }

    fn movie_path(&self) -> PathBuf {
        self.dir().join("movie.mkv")
    }

    fn len(&self) -> u64 {
        self.s.movie.len() as u64
    }

    /// Piece `p`'s bytes in the video file.
    fn span(&self, p: u32) -> (u64, u64) {
        let start = (u64::from(p) * PIECE).saturating_sub(NFO);
        let end = ((u64::from(p) + 1) * PIECE - NFO).min(self.len());
        (start, end)
    }

    fn source(&self, (a, b): (u64, u64)) -> &[u8] {
        &self.s.movie[a as usize..b as usize]
    }

    async fn read(&self, (a, b): (u64, u64)) -> Vec<u8> {
        let reader = self
            .engine()
            .open_reader(self.t, "default", self.file)
            .await
            .unwrap();
        let mut range = reader.range(a, Some(b)).await.unwrap();
        range.ready().await.unwrap();
        let mut out = Vec::new();
        range.read_to_end(&mut out).await.unwrap();
        out
    }

    async fn piece_matches(&self, p: u32) -> bool {
        let got = within(30, self.read(self.span(p))).await;
        got == self.source(self.span(p))
    }

    async fn evict(&self, pieces: &[u32]) -> mq_engine::librqbit::Evicted {
        self.engine().evict_pieces(self.t, pieces).await.unwrap()
    }

    /// Asks the seeder for blocks `blocks` of piece `p` outside the
    /// scheduler, and waits until they arrived, plus a moment for their
    /// writes.
    async fn request(&self, p: u32, blocks: std::ops::Range<u64>) {
        let before = self.received().await;
        let piece_len = self.piece_len(p);
        let mut want = 0;
        for b in blocks {
            let begin = b * BLOCK;
            if begin >= piece_len {
                break;
            }
            let len = BLOCK.min(piece_len - begin);
            let block = (p, begin as u32, len as u32);
            // A seeder dropped as not needed comes back after a moment.
            until("the seeder is connected", || async {
                test_hooks::request_block(self.engine(), self.t, self.s.seeder_addr, block).await
            })
            .await;
            want += len;
        }
        until("the requested blocks", || async {
            self.received().await >= before + want
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    fn piece_len(&self, p: u32) -> u64 {
        let total = NFO + self.len();
        PIECE.min(total - u64::from(p) * PIECE)
    }

    async fn received(&self) -> u64 {
        self.engine().stats(self.t).await.unwrap().received_bytes
    }

    async fn have(&self) -> (u32, u32) {
        let s = self.engine().stats(self.t).await.unwrap();
        (s.pieces_have, s.pieces_total)
    }

    /// The sync log of the leecher's folder (its bitfield beside the
    /// torrent's folder included) from `from` on.
    fn log(&self, from: usize) -> Vec<Event> {
        sync_log::under(self.root()).split_off(from)
    }

    fn log_len(&self) -> usize {
        sync_log::under(self.root()).len()
    }

    /// The movie file's punched ranges from `from` on.
    fn punched(&self, from: usize) -> Vec<std::ops::Range<u64>> {
        self.log(from)
            .into_iter()
            .filter_map(|e| match e {
                Event::Punched { path, range } if path == self.movie_path() => Some(range),
                _ => None,
            })
            .collect()
    }
}

/// The swarm with its video file chosen and nothing read.
async fn chosen(seeders: usize) -> Leech {
    let s = support::swarm(true).await;
    let mut extra = Vec::new();
    for _ in 1..seeders {
        extra.push(s.another_seeder().await);
    }
    let mut peers = vec![s.seeder_addr];
    peers.extend(extra.iter().map(|x| x.addr));
    let root = tempfile::tempdir().unwrap();
    let engine = engine_on(root.path()).await;
    let t = engine.add_magnet(&s.info_hash, &[], &peers).await.unwrap();
    let file = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    Leech {
        s,
        engine,
        root,
        t,
        file,
        _seeders: extra,
    }
}

/// The whole video read and verified, and the bitfield's writes settled.
async fn leeched() -> Leech {
    let l = chosen(1).await;
    let all = within(60, l.read((0, l.len()))).await;
    assert!(all == l.s.movie, "the video read back");
    until("every piece verified", || async {
        let (have, total) = l.have().await;
        total > 0 && have == total
    })
    .await;
    settle(&l).await;
    l
}

/// Waits until the folder's sync log stops growing for 300 ms.
async fn settle(l: &Leech) {
    let mut last = l.log_len();
    let started = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let now = l.log_len();
        if now == last || started.elapsed() > Duration::from_secs(10) {
            return;
        }
        last = now;
    }
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

/// A hold that opens when dropped, so a failing test does not leave a
/// round blocked on a worker thread.
struct Held(std::sync::Arc<sync_log::Hold>);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.open();
    }
}

impl std::ops::Deref for Held {
    type Target = sync_log::Hold;

    fn deref(&self) -> &sync_log::Hold {
        &self.0
    }
}

fn hold(dir: &Path, point: Point) -> Held {
    Held(sync_log::hold(dir, point))
}

async fn reached(hold: &sync_log::Hold) {
    until("the hold was reached", || async { hold.reached() }).await;
}

/// `range` rounded inward to whole filesystem blocks.
fn inward((a, b): (u64, u64)) -> std::ops::Range<u64> {
    a.div_ceil(FS_BLOCK) * FS_BLOCK..b / FS_BLOCK * FS_BLOCK
}

fn claims(have: &[u8], p: u32) -> bool {
    have.get(p as usize / 8)
        .is_some_and(|b| b & (0x80 >> (p % 8)) != 0)
}

/// a. A round skips a piece a reader is in, and the next round takes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_round_skips_a_piece_being_read() {
    let l = std::sync::Arc::new(leeched().await);
    let hold = hold(&l.dir(), Point::Read);
    let reader = {
        let (engine, t, file, span) = (l.engine().clone(), l.t, l.file, l.span(5));
        tokio::spawn(async move {
            let reader = engine.open_reader(t, "default", file).await.unwrap();
            let mut range = reader.range(span.0, Some(span.1)).await.unwrap();
            range.ready().await.unwrap();
            let mut out = Vec::new();
            range.read_to_end(&mut out).await.unwrap();
            out
        })
    };
    reached(&hold).await;
    let round = tokio::spawn({
        let l = l.clone();
        async move { l.evict(&[5, 6]).await }
    });
    // A thread sleep, not a timer: a round that blocks holds the torrent
    // lock, and the workers that drive the timers block behind it.
    let started = Instant::now();
    while !round.is_finished() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the round waits on the reader's piece"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let r = round.await.unwrap();
    assert_eq!((r.skipped, r.evicted), (vec![5], vec![6]));
    hold.open();
    let got = within(30, reader).await.unwrap();
    assert!(got == l.source(l.span(5)), "the parked reader's bytes");
    assert_eq!(l.evict(&[5]).await.evicted, [5]);
    assert!(l.piece_matches(5).await && l.piece_matches(6).await);
}

/// b and c. Three seeders writing, three readers each reading one piece
/// four times, and a round aimed at each piece a reader picks while it
/// reads it, for 30 s: rounds meet readers inside their piece and writers
/// refetching it. No deadlock (a round that does not end in 20 s aborts
/// the test: a deadlocked round blocks the workers that drive the timers),
/// and every read gets the seeder's bytes, never a hole's zeros.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn rounds_under_readers_and_writers_never_serve_a_hole() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let l = std::sync::Arc::new(chosen(3).await);
    let deadline = Instant::now() + Duration::from_secs(30);
    // Each reader's pick: its number of picks so far, then the piece.
    let on: std::sync::Arc<[AtomicU64; 3]> = std::sync::Arc::new(Default::default());
    let mut readers = Vec::new();
    for i in 0..3usize {
        let (l, on) = (l.clone(), on.clone());
        readers.push(tokio::spawn(async move {
            let mut x = (i as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let mut reads = 0u64;
            while Instant::now() < deadline {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let p = 1 + ((x >> 33) % 20) as u32;
                on[i].store((reads + 1) << 32 | u64::from(p), Ordering::Relaxed);
                for _ in 0..4 {
                    let read = tokio::time::timeout(Duration::from_secs(60), l.read(l.span(p)));
                    let Ok(got) = read.await else {
                        let state = test_hooks::piece_state(l.engine(), l.t, p).await;
                        panic!("a read of piece {p} waited 60 s: {state}");
                    };
                    assert!(got == l.source(l.span(p)), "piece {p} differs");
                    reads += 1;
                }
            }
            reads
        }));
    }
    // Readers parked in their read for 5 to 20 ms at a time, so rounds meet
    // a reader between its `have` check and its `pread` (verification b N7).
    let parker = tokio::spawn({
        let dir = l.dir();
        async move {
            let mut x = 11u64;
            loop {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let held = sync_log::hold(&dir, Point::Read);
                while !held.reached() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                tokio::time::sleep(Duration::from_millis(5 + (x >> 33) % 16)).await;
                held.open();
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    });
    let (mut rounds, mut evicted) = (0, 0);
    let mut seen = [0u64; 3];
    while Instant::now() < deadline {
        let mut pieces = Vec::new();
        for (i, on) in on.iter().enumerate() {
            let pick = on.load(Ordering::Relaxed);
            if pick >> 32 != seen[i] >> 32 {
                seen[i] = pick;
                pieces.push(pick as u32);
            }
        }
        if pieces.is_empty() {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        let round = tokio::spawn({
            let l = l.clone();
            async move { l.evict(&pieces).await }
        });
        let started = Instant::now();
        while !round.is_finished() {
            if started.elapsed() > Duration::from_secs(20) {
                eprintln!("deadlock: a round did not end in 20 s");
                std::process::abort();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        rounds += 1;
        evicted += round.await.unwrap().evicted.len();
    }
    let mut reads = 0;
    for r in readers {
        reads += within(90, r).await.unwrap();
    }
    parker.abort();
    // The counts of what is held agree once nothing moves (verification b
    // N8): the claim, its per-file count and `have_bytes` change together.
    settle(&l).await;
    let counts = test_hooks::have_counts(l.engine(), l.t).await.unwrap();
    assert!(
        counts.0 == counts.2 && counts.1 == counts.2,
        "per file, have_bytes, needed: {counts:?}"
    );
    eprintln!("{rounds} rounds, {evicted} pieces evicted, {reads} reads");
    assert!(
        rounds > 50 && evicted > 20 && reads > 100,
        "{rounds} {evicted} {reads}"
    );
}

/// d. A late block for a verified piece, between the bitfield's write and
/// the punch, is dropped: the piece is later fetched and hashed whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_late_block_during_a_round_is_dropped() {
    let l = std::sync::Arc::new(leeched().await);
    let hold = hold(&l.dir(), Point::Acked);
    let round = tokio::spawn({
        let l = l.clone();
        async move { l.evict(&[8]).await }
    });
    reached(&hold).await;
    l.request(8, 0..16).await;
    hold.open();
    assert_eq!(within(30, round).await.unwrap().evicted, [8]);
    assert_eq!(l.have().await.0, 20, "piece 8 claimed over its hole");
    assert!(l.piece_matches(8).await, "piece 8 fetched again, not zeros");
}

/// e. The bitfield without the piece is fsynced before any byte of it is
/// punched.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_unclaim_is_synced_before_the_punch() {
    let l = std::sync::Arc::new(leeched().await);
    let mark = l.log_len();
    let hold = hold(l.root(), Point::Fsync);
    let round = tokio::spawn({
        let l = l.clone();
        async move { l.evict(&[8]).await }
    });
    reached(&hold).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(l.punched(mark).is_empty(), "punched before the fsync");
    hold.open();
    assert_eq!(within(30, round).await.unwrap().evicted, [8]);
    let log = l.log(mark);
    let synced = log
        .iter()
        .position(|e| matches!(e, Event::HaveSynced { pieces, .. } if !claims(pieces, 8)));
    let punched = log.iter().position(|e| matches!(e, Event::Punched { .. }));
    assert!(
        synced.is_some() && synced < punched,
        "unclaim synced before the punch: {log:?}"
    );
}

/// The child of test f: `<step> <folder> <seeder> <info hash>`.
const CRASH: &str = "MQ_WINDOW_CRASH";

/// f. A process killed at each step of a round. The child (this binary,
/// `crash_child`) leeches the video, holds a round of pieces 3, 4, 5 and 9
/// at the step, writes the bitfield its last fsync made durable, and waits
/// to be killed. Then every piece claimed by the bitfield on disk (what a
/// killed process leaves) and by the durable one (what a power cut leaves)
/// has its bytes, and a relaunch reads every claimed piece back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_killed_round_leaves_only_true_claims() {
    for step in ["gated", "acked", "punched"] {
        let s = support::swarm(true).await;
        let root = tempfile::tempdir().unwrap();
        let ready = root.path().join("ready");
        let spec = format!(
            "{step} {} {} {}",
            root.path().display(),
            s.seeder_addr,
            s.info_hash
        );
        let mut child = Reaped(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "crash_child", "--nocapture"])
                .env(CRASH, spec)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let child = &mut child.0;
        let waited = Instant::now();
        while !ready.exists() {
            assert!(
                waited.elapsed() < Duration::from_secs(90),
                "{step}: child ready"
            );
            assert!(
                child.try_wait().unwrap().is_none(),
                "{step}: the child died"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        child.kill().unwrap();
        child.wait().unwrap();

        let folder = root.path().join(&s.info_hash);
        let mut disk = std::fs::read(folder.join("a.nfo")).unwrap();
        disk.extend(std::fs::read(folder.join("movie.mkv")).unwrap());
        let mut source = vec![b'n'; NFO as usize];
        source.extend(&s.movie);
        let have = std::fs::read(root.path().join(format!("{}.have", s.info_hash))).unwrap();
        let durable = std::fs::read(root.path().join("durable.have")).unwrap();
        for (view, bits) in [("killed", &have), ("power cut", &durable)] {
            let claimed: Vec<u32> = (0..21).filter(|&p| claims(bits, p)).collect();
            assert!(claimed.len() >= 17, "{step} {view}: {claimed:?}");
            for p in claimed {
                let a = u64::from(p) * PIECE;
                let b = (a + PIECE).min(source.len() as u64);
                assert!(
                    disk.get(a as usize..b as usize) == Some(&source[a as usize..b as usize]),
                    "{step} {view}: piece {p} claimed over missing bytes"
                );
            }
        }
        // The relaunch, through its fast resume.
        let engine = engine_on(root.path()).await;
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let file = engine
            .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        for p in (1..21).filter(|&p| claims(&have, p)) {
            let a = (u64::from(p) * PIECE - NFO) as usize;
            let b = ((u64::from(p) + 1) * PIECE - NFO).min(s.movie.len() as u64) as usize;
            let reader = engine.open_reader(t, "default", file).await.unwrap();
            let mut range = reader.range(a as u64, Some(b as u64)).await.unwrap();
            within(30, range.ready()).await.unwrap();
            let mut got = Vec::new();
            within(30, range.read_to_end(&mut got)).await.unwrap();
            assert!(got == s.movie[a..b], "{step}: piece {p} after the relaunch");
        }
    }
}

/// A child process killed and reaped when dropped, so a failing test leaves
/// none behind.
struct Reaped(std::process::Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The child of test f; a passing no-op in a normal run.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn crash_child() {
    let Ok(spec) = std::env::var(CRASH) else {
        return;
    };
    // Whatever ends or abandons the parent, the child ends: a held round
    // keeps its runtime from exiting on its own.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(180));
        std::process::exit(3);
    });
    let parts: Vec<&str> = spec.split(' ').collect();
    let (step, root, seeder, hash) = (parts[0], Path::new(parts[1]), parts[2], parts[3]);
    let engine = engine_on(root).await;
    let t = engine
        .add_magnet(hash, &[], &[seeder.parse().unwrap()])
        .await
        .unwrap();
    let file = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let len = engine.files(t).unwrap()[file].len;
    let reader = engine.open_reader(t, "default", file).await.unwrap();
    let mut range = reader.range(0, Some(len)).await.unwrap();
    range.ready().await.unwrap();
    range.read_to_end(&mut Vec::new()).await.unwrap();
    until("every piece verified", || async {
        let s = engine.stats(t).await.unwrap();
        s.pieces_total > 0 && s.pieces_have == s.pieces_total
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let point = match step {
        "gated" => Point::Gated,
        "acked" => Point::Acked,
        _ => Point::Punched,
    };
    let hold = sync_log::hold(root, point);
    let round = tokio::spawn({
        let engine = engine.clone();
        async move { engine.evict_pieces(t, &[3, 4, 5, 9]).await }
    });
    reached(&hold).await;
    let have_path = root.join(format!("{hash}.have"));
    let durable = sync_log::under(root)
        .into_iter()
        .filter_map(|e| match e {
            Event::HaveSynced { path, pieces } if path == have_path => Some(pieces),
            _ => None,
        })
        .next_back()
        .expect("a synced bitfield");
    std::fs::write(root.join("durable.have"), durable).unwrap();
    std::fs::write(root.join("ready"), b"").unwrap();
    // Killed here.
    let _ = round.await;
}

/// g. Each flusher failure (data sync, bitfield write, fsync): nothing is
/// punched, the piece is claimed again, and a reader that began waiting
/// during the round is woken and gets it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_flusher_failure_keeps_the_piece_and_wakes_its_reader() {
    for fail in [Fail::DataSync, Fail::HaveWrite, Fail::HaveFsync] {
        let l = std::sync::Arc::new(leeched().await);
        let mark = l.log_len();
        let hold = hold(&l.dir(), Point::Gated);
        sync_log::fail(l.root(), fail, 1);
        let round = tokio::spawn({
            let l = l.clone();
            async move { l.evict(&[8]).await }
        });
        reached(&hold).await;
        let reader = tokio::spawn({
            let l = l.clone();
            async move { l.read(l.span(8)).await }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!reader.is_finished(), "{fail:?}: the reader waits");
        hold.open();
        let r = within(30, round).await.unwrap();
        assert_eq!(r.kept, [8], "{fail:?}");
        assert!(r.error.is_some(), "{fail:?}");
        let got = within(30, reader).await.unwrap();
        assert!(got == l.source(l.span(8)), "{fail:?}: the reader's bytes");
        assert!(l.punched(mark).is_empty(), "{fail:?}: nothing punched");
    }
}

/// h. A piece evicted and fetched again is claimed only after its data
/// synced.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_piece_fetched_again_is_claimed_after_its_data_synced() {
    let l = leeched().await;
    assert_eq!(l.evict(&[8]).await.evicted, [8]);
    let mark = l.log_len();
    assert!(l.piece_matches(8).await);
    until("a bitfield claiming piece 8 again", || async {
        l.log(mark)
            .iter()
            .any(|e| matches!(e, Event::HaveSynced { pieces, .. } if claims(pieces, 8)))
    })
    .await;
    let log = l.log(mark);
    let claim = log
        .iter()
        .position(|e| matches!(e, Event::HaveWritten { claimed, .. } if *claimed == 21));
    let synced = log
        .iter()
        .position(|e| matches!(e, Event::DataSynced(p) if *p == l.movie_path()));
    assert!(synced.is_some() && synced < claim, "{log:?}");
}

/// i. A partly written piece (an orphan) is punched, and its blocks are
/// asked again when read.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_orphan_is_punched_and_fetched_again() {
    let l = chosen(1).await;
    l.request(8, 0..8).await;
    let mark = l.log_len();
    let r = l.evict(&[8]).await;
    assert_eq!(r.evicted, [8]);
    assert!(r.punched_bytes > 0 && !l.punched(mark).is_empty());
    assert!(l.piece_matches(8).await);
}

/// A folder an earlier launch left counts its one file's bytes, as the app
/// counts them once the torrent loads, so the ranking's whole-file room
/// matches the app's (review r3 A R3); a folder of a pack names none.
#[test]
fn a_leftover_folder_of_one_file_counts_that_file() {
    let dir = tempfile::tempdir().unwrap();
    let single = dir.path().join("a".repeat(40));
    std::fs::create_dir_all(single.join("Film (2014)")).unwrap();
    let film = single.join("Film (2014)/film.mkv");
    std::fs::write(&film, vec![1u8; 1 << 20]).unwrap();
    let pack = dir.path().join("b".repeat(40));
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("e01.mkv"), vec![1u8; 1 << 20]).unwrap();
    std::fs::write(pack.join("e02.mkv"), vec![1u8; 1 << 20]).unwrap();
    let counted = mq_engine::counted_on_disk(dir.path());
    let on_disk = std::fs::metadata(&film).unwrap().blocks() * 512;
    assert_eq!(counted.held[&"a".repeat(40)].files, [on_disk]);
    assert!(counted.held[&"b".repeat(40)].files.is_empty());
}

/// Probes at once in one folder (the core's at open, the engine's at its
/// start) all answer yes where holes punch: each has a scratch file of its
/// own (review r3 A R1).
#[test]
fn probes_at_once_never_share_a_scratch_file() {
    let dir = tempfile::tempdir().unwrap();
    if !mq_engine::librqbit::punch_supported(dir.path()) {
        eprintln!("skipped: this host's temp folder cannot punch holes");
        return;
    }
    let answers: Vec<bool> = std::thread::scope(|s| {
        let probes: Vec<_> = (0..16)
            .map(|_| s.spawn(|| (0..10).all(|_| mq_engine::librqbit::punch_supported(dir.path()))))
            .collect();
        probes.into_iter().map(|p| p.join().unwrap()).collect()
    });
    assert!(answers.iter().all(|&a| a), "{answers:?}");
}

/// j. A punch that fails (here after freeing its first block, as neither
/// call promises all or nothing): the probe says no, and a round's pieces
/// leave unclaimed and are fetched again, never claimed over a hole.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_punch_never_claims_its_pieces_again() {
    let ok = tempfile::tempdir().unwrap();
    if !mq_engine::librqbit::punch_supported(ok.path()) {
        eprintln!("skipped: this host's temp folder cannot punch holes");
        return;
    }
    let refused = tempfile::tempdir().unwrap();
    sync_log::fail(refused.path(), Fail::Punch, usize::MAX);
    assert!(!mq_engine::librqbit::punch_supported(refused.path()));
    // A punch that failed after work says nothing about the volume, so it
    // keeps whole files; only a refusal says it cannot punch (review r1
    // P2-2).
    assert_eq!(mq_engine::librqbit::punch_probe(refused.path()), None);
    let unsupported = tempfile::tempdir().unwrap();
    sync_log::fail(unsupported.path(), Fail::PunchRefused, usize::MAX);
    assert_eq!(
        mq_engine::librqbit::punch_probe(unsupported.path()),
        Some(false)
    );
    assert_eq!(mq_engine::librqbit::punch_probe(ok.path()), Some(true));

    let l = leeched().await;
    sync_log::fail(l.root(), Fail::Punch, usize::MAX);
    let r = l.evict(&[5, 6]).await;
    assert_eq!((r.kept, r.evicted), (vec![], vec![5, 6]));
    assert!(r.error.is_some() && r.punched_bytes == 0);
    assert_eq!(
        l.have().await.0,
        19,
        "pieces 5 and 6 claimed after the punch"
    );
    assert!(l.piece_matches(5).await && l.piece_matches(6).await);

    // Refused before any work (EOPNOTSUPP): nothing was touched, so the
    // pieces stay claimed (verification b N3).
    sync_log::fail(l.root(), Fail::PunchRefused, usize::MAX);
    let r = l.evict(&[9, 10]).await;
    assert_eq!((r.kept, r.evicted), (vec![9, 10], vec![]));
    assert!(r.error.is_some() && r.punched_bytes == 0);
    assert!(l.piece_matches(9).await && l.piece_matches(10).await);
}

/// k. Unaligned piece edges: a block shared with a piece that is not gated
/// in the same round keeps its bytes; pieces gated together are punched as
/// one range.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shared_block_is_punched_only_with_both_pieces_gated() {
    let l = leeched().await;
    let mark = l.log_len();
    l.evict(&[5]).await;
    assert_eq!(l.punched(mark), [inward(l.span(5))]);
    let mark = l.log_len();
    l.evict(&[6]).await;
    assert_eq!(l.punched(mark), [inward(l.span(6))], "piece 5 left before");
    let mark = l.log_len();
    l.evict(&[8, 9]).await;
    assert_eq!(l.punched(mark), [inward((l.span(8).0, l.span(9).1))]);
    for p in [4, 7, 10] {
        assert!(l.piece_matches(p).await, "neighbour {p} kept its bytes");
    }
}

/// l. The file's allocated bytes fall by exactly what was punched.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn allocated_falls_by_the_punched_blocks() {
    let l = leeched().await;
    let allocated = || std::fs::metadata(l.movie_path()).unwrap().blocks() * 512;
    let before = allocated();
    let r = l.evict(&[4, 5, 6, 12]).await;
    assert!(r.punched_bytes > 3 * PIECE);
    assert_eq!(before - allocated(), r.punched_bytes);
}

/// m. Stale blocks for an orphan between its gate and its punch are
/// dropped, so it is never claimed over a hole. An orphan whose punch is
/// refused is still released and fetched again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_orphan_is_never_claimed_over_a_hole() {
    let l = std::sync::Arc::new(chosen(1).await);
    l.request(8, 0..8).await;
    let had = l.have().await.0;
    let hold = hold(&l.dir(), Point::Gated);
    let round = tokio::spawn({
        let l = l.clone();
        async move { l.evict(&[8]).await }
    });
    reached(&hold).await;
    l.request(8, 8..16).await;
    hold.open();
    assert_eq!(within(30, round).await.unwrap().evicted, [8]);
    assert_eq!(l.have().await.0, had, "piece 8 claimed over its hole");
    assert!(l.piece_matches(8).await, "piece 8 is not a hole");

    // Behind the read above, so its readahead left it alone.
    l.request(3, 0..8).await;
    sync_log::fail(l.root(), Fail::Punch, 1);
    let r = l.evict(&[3]).await;
    assert_eq!(r.evicted, [3]);
    assert!(r.error.is_some() && r.punched_bytes == 0);
    assert!(l.piece_matches(3).await, "piece 3 fetched again");

    // An orphan in a round whose bitfield write fails: no punch is tried,
    // the verified piece is claimed again, the orphan still released.
    l.request(2, 0..8).await;
    sync_log::fail(l.root(), Fail::DataSync, 1);
    let r = l.evict(&[2, 8]).await;
    assert_eq!((r.evicted, r.kept), (vec![2], vec![8]));
    assert!(l.piece_matches(2).await, "piece 2 fetched again");
}

/// n. A pause during a round's bitfield write or its punch waits for the
/// round; after the resume each piece reads back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pause_waits_for_the_round() {
    let l = std::sync::Arc::new(leeched().await);
    for (point, piece, generation) in [(Point::Gated, 5, 1), (Point::Acked, 9, 3)] {
        let hold = hold(&l.dir(), point);
        let round = tokio::spawn({
            let l = l.clone();
            async move { l.evict(&[piece]).await }
        });
        reached(&hold).await;
        let pause = tokio::spawn({
            let l = l.clone();
            async move { l.engine().pause_now(l.t, "kid", generation).await }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!pause.is_finished(), "{point:?}: the pause waits");
        hold.open();
        assert_eq!(within(30, round).await.unwrap().evicted, [piece]);
        assert!(within(30, pause).await.unwrap().unwrap());
        let released = l.engine().release_hold(l.t, "kid", generation + 1).await;
        assert!(released.unwrap());
        assert!(l.piece_matches(piece).await, "{point:?}");
    }
}

/// o. Stale blocks that complete a piece just after its round reach the
/// scheduler after its `PieceEvicted`: the adapter counts the piece as
/// fetched again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_scheduler_hears_the_eviction_before_the_new_check() {
    let l = std::sync::Arc::new(leeched().await);
    let hold = hold(&l.dir(), Point::Released);
    let round = tokio::spawn({
        let l = l.clone();
        async move { l.evict(&[8]).await }
    });
    reached(&hold).await;
    l.request(8, 0..16).await;
    until("piece 8 verified again", || async {
        l.have().await.0 == 21
    })
    .await;
    hold.open();
    assert_eq!(within(30, round).await.unwrap().evicted, [8]);
    until("the adapter counted the refetch", || async {
        l.engine().stats(l.t).await.unwrap().refetched_bytes == PIECE
    })
    .await;
    assert!(l.piece_matches(8).await);
}

/// p. A sparse folder with no bitfield starts with nothing claimed, even a
/// piece whose bytes are there, and with no full check. The video is
/// 64 MiB: APFS fills a small sparse file's holes at its first write (a
/// 5 MiB one became whole), a large one keeps them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sparse_folder_starts_with_nothing_claimed() {
    let movie: Vec<u8> = (0..(64u32 << 20) + 12_345)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let s = support::swarm_movie("movie.mkv", movie).await;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join(&s.info_hash);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.nfo"), vec![b'n'; NFO as usize]).unwrap();
    let movie = std::fs::File::create(folder.join("movie.mkv")).unwrap();
    movie.set_len(s.movie.len() as u64).unwrap();
    let (a, b) = (128 * PIECE - NFO, 129 * PIECE - NFO);
    std::os::unix::fs::FileExt::write_all_at(&movie, &s.movie[a as usize..b as usize], a).unwrap();
    drop(movie);
    let engine = engine_on(dir.path()).await;
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let have = dir.path().join(format!("{}.have", s.info_hash));
    until("the start's bitfield", || async {
        std::fs::metadata(&have).is_ok_and(|m| m.len() > 0)
    })
    .await;
    let held = std::fs::metadata(folder.join("movie.mkv"))
        .unwrap()
        .blocks()
        * 512;
    assert!(held < 8 << 20, "the video is sparse: {held} bytes held");
    let bits = std::fs::read(&have).unwrap();
    assert!(!claims(&bits, 128), "piece 128 claimed: a full check ran");
    assert!(bits.iter().all(|b| *b == 0), "{bits:?}");
}

/// A round needs upload off and DWH driving (plan 2.3): otherwise it gates
/// nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_round_needs_upload_off_and_drive_mode() {
    let s = support::swarm(true).await;
    let stock = tempfile::tempdir().unwrap();
    let stock = engine_with(stock.path(), false).await;
    for engine in [&s.leecher, &stock] {
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let file = engine
            .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        let reader = engine.open_reader(t, "default", file).await.unwrap();
        let mut range = reader.range(0, Some(PIECE)).await.unwrap();
        within(30, range.ready()).await.unwrap();
        let have = engine.stats(t).await.unwrap().pieces_have;
        let r = engine.evict_pieces(t, &[0, 1]).await;
        assert!(r.is_err(), "a round ran: {r:?}");
        assert_eq!(engine.stats(t).await.unwrap().pieces_have, have);
    }
}

/// A piece verified just before a round takes it: its `PieceChecked`
/// reaches the scheduler before the round's `PieceEvicted`, so the piece
/// is fetched again when read (review b P2-1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_piece_verified_just_before_a_round_is_fetched_again() {
    let l = std::sync::Arc::new(chosen(1).await);
    let checked = hold(&l.dir(), Point::Checked);
    tokio::spawn({
        let l = l.clone();
        async move { l.request(8, 0..16).await }
    });
    reached(&checked).await;
    // The held writer is piece 8's (verification a P3-4).
    let state = test_hooks::piece_state(l.engine(), l.t, 8).await;
    assert!(state.contains("Some((true,"), "{state}");
    assert_eq!(l.evict(&[8]).await.evicted, [8]);
    checked.open();
    assert!(l.piece_matches(8).await, "piece 8 fetched again");
}

/// A round dropped mid-way (its task aborted while it waits for the
/// bitfield's fsync) releases its pieces: a read gets them again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_round_releases_its_pieces() {
    let l = std::sync::Arc::new(leeched().await);
    let fsync = hold(l.root(), Point::Fsync);
    let gated = hold(&l.dir(), Point::Gated);
    let round = tokio::spawn({
        let l = l.clone();
        async move { l.evict(&[5, 9]).await }
    });
    reached(&gated).await;
    round.abort();
    gated.open();
    until("the round dropped", || async { round.is_finished() }).await;
    fsync.open();
    assert_eq!(l.have().await.0, 19, "pieces 5 and 9 claimed again");
    assert!(l.piece_matches(5).await && l.piece_matches(9).await);
}

/// A pause that skips the keeper (as librqbit's own on a fatal error)
/// during a round releases the gated pieces and tells the scheduler: after
/// the resume they read back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pause_mid_round_releases_the_gated_pieces() {
    let l = std::sync::Arc::new(leeched().await);
    let gated = hold(&l.dir(), Point::Gated);
    let round = tokio::spawn({
        let l = l.clone();
        async move { l.engine().evict_pieces(l.t, &[5, 9]).await }
    });
    reached(&gated).await;
    test_hooks::pause_skipping_the_keeper(l.engine(), l.t).await;
    gated.open();
    let _ = within(30, round).await.unwrap();
    assert!(l.piece_matches(5).await && l.piece_matches(9).await);
}

/// A connection's death acts only on its own peer slot (verification b
/// N1). A torrent with only the `.nfo` chosen finishes and drops its seeder
/// as not needed; choosing the video then queues the seeder again while the
/// old connection's death waits. Upstream took the new connection's slot
/// for the old one's and lost the peer; here the video reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dying_connection_never_takes_a_new_ones_slot() {
    let s = support::swarm(true).await;
    let root = tempfile::tempdir().unwrap();
    let engine = engine_on(root.path()).await;
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let dir = root.path().join(&s.info_hash);
    let died = hold(&dir, Point::PeerDied);
    let nfo = engine
        .select_file(t, "default", &FileHint::Name("a.nfo".into()))
        .await
        .unwrap();
    let reader = engine.open_reader(t, "default", nfo).await.unwrap();
    let mut range = reader.range(0, Some(NFO)).await.unwrap();
    within(30, range.ready()).await.unwrap();
    drop(range);
    reached(&died).await;
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    until("the seeder connected again", || async {
        test_hooks::piece_state(&engine, t, 10)
            .await
            .contains("\"live\"")
    })
    .await;
    died.open();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (a, b) = (10 * PIECE - NFO, 11 * PIECE - NFO);
    let reader = engine.open_reader(t, "default", movie).await.unwrap();
    let mut range = reader.range(a, Some(b)).await.unwrap();
    let read = async {
        range.ready().await.unwrap();
        let mut got = Vec::new();
        range.read_to_end(&mut got).await.unwrap();
        got
    };
    let Ok(got) = tokio::time::timeout(Duration::from_secs(30), read).await else {
        panic!("{}", test_hooks::piece_state(&engine, t, 10).await);
    };
    assert!(got == s.movie[a as usize..b as usize]);
}

/// A torrent whose bitfield lives only in memory (its full check's data
/// sync failed, so nothing was stored) never punches: a stale bitfield on
/// disk may still claim the pieces (verification b N4).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bitfield_only_in_memory_never_punches() {
    let s = support::swarm(true).await;
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join(&s.info_hash);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.nfo"), vec![b'n'; NFO as usize]).unwrap();
    std::fs::write(folder.join("movie.mkv"), &s.movie).unwrap();
    sync_log::fail(&folder, Fail::CheckSync, 1);
    let engine = engine_on(root.path()).await;
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    until("the full check", || async {
        let s = engine.stats(t).await.unwrap();
        s.pieces_total > 0 && s.pieces_have == s.pieces_total
    })
    .await;
    let mark = sync_log::under(&folder).len();
    let r = engine.evict_pieces(t, &[5]).await.unwrap();
    assert_eq!((r.kept, r.evicted), (vec![5], vec![]));
    assert!(r.error.is_some());
    let punched = sync_log::under(&folder)
        .split_off(mark)
        .into_iter()
        .any(|e| matches!(e, Event::Punched { .. }));
    assert!(!punched, "a piece was punched");
}

/// A round that outlives a pause (librqbit's own skips the keeper lock)
/// punches nothing the resumed torrent fetched again: the pause emptied
/// that state's storage (verification b N5).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_round_that_outlives_a_pause_never_punches_the_resumed_torrent() {
    let l = std::sync::Arc::new(leeched().await);
    let acked = hold(&l.dir(), Point::Acked);
    let round = tokio::spawn({
        let l = l.clone();
        async move { l.engine().evict_pieces(l.t, &[5, 9]).await }
    });
    reached(&acked).await;
    test_hooks::pause_skipping_the_keeper(l.engine(), l.t).await;
    assert!(l.piece_matches(5).await && l.piece_matches(9).await);
    acked.open();
    let _ = within(30, round).await.unwrap();
    assert!(l.piece_matches(5).await && l.piece_matches(9).await);
    assert_eq!(l.have().await.0, 21);
}

/// Phase 2 (APFS's filled holes): a clear of empty pieces skips a claimed
/// piece and one with a chunk written, and punches an emptied one, which is
/// fetched again whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_clear_of_empty_pieces_touches_nothing_written() {
    let l = leeched().await;
    let r = test_hooks::clear_empty(l.engine(), l.t, &[5])
        .await
        .unwrap();
    assert_eq!((r.skipped, r.evicted), (vec![5], vec![]));
    assert!(l.piece_matches(5).await);
    assert_eq!(l.evict(&[8, 9]).await.evicted, [8, 9]);
    l.request(8, 0..4).await;
    let r = test_hooks::clear_empty(l.engine(), l.t, &[8, 9])
        .await
        .unwrap();
    assert_eq!((r.skipped, r.evicted), (vec![8], vec![9]));
    assert!(l.piece_matches(9).await && l.piece_matches(8).await);
}

/// Phase 2 (plans/streaming-window.md 2.2): a rate set before the torrent
/// is live reaches its scheduler when it starts; the stats report what the
/// scheduler runs with.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rate_set_before_the_start_reaches_the_scheduler() {
    let s = support::swarm(true).await;
    let root = tempfile::tempdir().unwrap();
    let engine = engine_on(root.path()).await;
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    engine.set_rate(t, 1, 1 << 20, true).await.unwrap();
    assert_eq!(engine.stats(t).await.unwrap().bytes_per_s, None);
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    assert_eq!(movie, 1);
    until("the scheduler's rate", || async {
        engine.stats(t).await.unwrap().bytes_per_s == Some(1 << 20)
    })
    .await;
}

/// The download limit holds DWH's reads (the live rows' line limit): the
/// 5 MiB video at 1 MiB/s takes at least 3 s past the limiter's 1 MiB burst,
/// where the loopback alone takes well under one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_download_limit_holds_dwh_reads() {
    let s = support::swarm(true).await;
    let root = tempfile::tempdir().unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let mut config = mq_engine::EngineConfig {
        discovery: false,
        upload: false,
        ..mq_engine::EngineConfig::new(
            root.path(),
            http.torrent_scope(&[std::net::Ipv4Addr::LOCALHOST.into()]),
        )
    };
    config.limits.download_bps = std::num::NonZeroU32::new(1 << 20);
    let engine = Engine::start(config).await.unwrap();
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let started = Instant::now();
    let reader = engine.open_reader(t, "default", movie).await.unwrap();
    let mut range = reader.range(0, None).await.unwrap();
    let mut got = Vec::new();
    within(60, range.read_to_end(&mut got)).await.unwrap();
    assert!(got == s.movie);
    let took = started.elapsed();
    assert!(took >= Duration::from_secs(3), "read in {took:?}");
}

async fn engine_on(dir: &Path) -> Engine {
    engine_with(dir, true).await
}

/// An engine on `dir` with upload off, DWH driving or not; the one before
/// on `dir` (a killed child) may still hold the folder for a moment.
async fn engine_with(dir: &Path, dwh: bool) -> Engine {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let config = || mq_engine::EngineConfig {
        discovery: false,
        upload: false,
        dwh,
        ..mq_engine::EngineConfig::new(
            dir,
            http.torrent_scope(&[std::net::Ipv4Addr::LOCALHOST.into()]),
        )
    };
    for _ in 0..40 {
        if let Ok(engine) = Engine::start(config()).await {
            return engine;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Engine::start(config()).await.unwrap()
}

/// Piece files (plans/streaming-window-chunks.md), for a volume that cannot
/// punch: three files of a torrent in 1000-byte pieces, none starting on a
/// piece (0..2500, 2500..2700 inside piece 2, 2700..6100). Writes and reads
/// cross piece files; an eviction deletes only the piece files wholly in
/// its range, an evicted piece reads zeros, its neighbours keep their bytes,
/// and the folder's allocated bytes fall.
#[test]
fn piece_files_read_write_and_evict_across_boundaries() {
    use mq_engine::librqbit::storage::filesystem::{Chunked, allocated, is_chunked};
    let td = tempfile::tempdir().unwrap();
    let layout = [(0u64, 2500u64), (2500, 200), (2700, 3400)];
    let dirs: Vec<PathBuf> = (0..3).map(|i| td.path().join(format!("f{i}"))).collect();
    let files: Vec<Chunked> = layout
        .iter()
        .zip(&dirs)
        .map(|(&(start, len), dir)| Chunked::new(dir.clone(), start, 1000, len))
        .collect();
    let data: Vec<Vec<u8>> = layout
        .iter()
        .map(|&(start, len)| (0..len).map(|i| (i * 7 + start) as u8).collect())
        .collect();
    let pieces = |dir: &Path| {
        let mut v: Vec<u64> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_str().unwrap().parse().unwrap())
            .collect();
        v.sort_unstable();
        v
    };
    let read = |c: &Chunked, at: u64, len: u64| {
        let mut buf = vec![0xee; len as usize];
        c.read_exact_at(at, &mut buf).unwrap();
        buf
    };
    for ((c, d), dir) in files.iter().zip(&data).zip(&dirs) {
        std::fs::create_dir_all(dir).unwrap();
        for at in (0..d.len()).step_by(333) {
            let end = (at + 333).min(d.len());
            c.write_all_at(at as u64, &d[at..end]).unwrap();
        }
        assert_eq!(read(c, 0, d.len() as u64), *d);
    }
    assert_eq!(pieces(&dirs[0]), [0, 1, 2]);
    assert_eq!(pieces(&dirs[1]), [2], "inside one piece");
    assert_eq!(pieces(&dirs[2]), [2, 3, 4, 5, 6]);
    assert_eq!(
        read(&files[0], 900, 300),
        data[0][900..1200],
        "across pieces"
    );
    let mut over = [0u8; 2];
    assert!(
        files[0].read_exact_at(2499, &mut over).is_err(),
        "past the end"
    );

    // The third file: piece 3 is its bytes 300..1300, piece 4 1300..2300.
    let (c, d, dir) = (&files[2], &data[2], &dirs[2]);
    let before = allocated(dir);
    assert_eq!(
        c.punch(310, 2000).unwrap(),
        1300..2300,
        "only piece 4 whole"
    );
    assert_eq!(pieces(dir), [2, 3, 5, 6]);
    assert_eq!(c.punch(300, 1000).unwrap(), 300..1300, "piece 3 exactly");
    assert_eq!(c.punch(0, 299).unwrap(), 0..0, "a part of piece 2 stays");
    assert_eq!(pieces(dir), [2, 5, 6]);
    assert!(
        allocated(dir) < before,
        "{} not under {before}",
        allocated(dir)
    );
    assert_eq!(read(c, 0, 300), d[..300], "piece 2's part kept");
    assert_eq!(read(c, 300, 2000), vec![0; 2000], "evicted reads zeros");
    assert_eq!(read(c, 2300, 1100), d[2300..], "the rest kept");
    assert_eq!(
        read(&files[0], 2000, 500),
        data[0][2000..],
        "piece 2 elsewhere"
    );
    assert_eq!(read(&files[1], 0, 200), data[1]);
    // The last piece is short (6000..6100): it goes whole too.
    assert_eq!(c.punch(3300, 100).unwrap(), 3300..3400);
    // Written again after an eviction, it reads back.
    c.write_all_at(1300, &d[1300..2300]).unwrap();
    assert_eq!(read(c, 1300, 1000), d[1300..2300]);
    assert!(is_chunked(dir));
    assert!(!is_chunked(td.path()), "a folder of folders is no file");
    let empty = td.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    assert!(!is_chunked(&empty), "an empty folder is no file");
}

/// A piece half written to its piece file when the RAM switch turns on
/// (review r1 P2-1): its piece file goes as it enters RAM, so a piece file
/// on disk always holds the piece (the bitfield claims it by that), and
/// what was on disk reads as zeros until it is fetched again.
#[test]
fn a_piece_that_enters_ram_leaves_its_piece_file() {
    use mq_engine::librqbit::storage::filesystem::Chunked;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let td = tempfile::tempdir().unwrap();
    let dir = td.path().join("f");
    std::fs::create_dir(&dir).unwrap();
    let memory = Arc::new(AtomicBool::new(false));
    let c = Chunked::new(dir.clone(), 0, 1000, 3000).with_memory(memory.clone());
    c.write_all_at(0, &[1; 500]).unwrap();
    c.write_all_at(1000, &[2; 1000]).unwrap();
    assert!(dir.join("0").exists() && dir.join("1").exists());
    memory.store(true, Ordering::Relaxed);
    c.write_all_at(500, &[3; 500]).unwrap();
    assert!(!dir.join("0").exists(), "piece 0 is in RAM now");
    let mut buf = vec![9; 1000];
    c.read_exact_at(0, &mut buf).unwrap();
    assert_eq!(&buf[..500], &[0; 500][..], "its disk part is gone");
    assert_eq!(&buf[500..], &[3; 500][..]);
    // Piece 1, whole on disk, stays there and reads from it.
    c.read_exact_at(1000, &mut buf).unwrap();
    assert_eq!(buf, [2; 1000]);
    assert!(dir.join("1").exists());
    // A new piece goes to RAM, never to disk; an eviction drops it.
    c.write_all_at(2000, &[4; 1000]).unwrap();
    assert!(!dir.join("2").exists());
    c.read_exact_at(2000, &mut buf).unwrap();
    assert_eq!(buf, [4; 1000]);
    assert_eq!(c.punch(2000, 1000).unwrap(), 2000..3000);
    c.read_exact_at(2000, &mut buf).unwrap();
    assert_eq!(buf, [0; 1000], "evicted from RAM");
}
