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

/// b and c. Three seeders writing, three readers and a round every 100 ms
/// for 30 s: no deadlock, and every read gets the seeder's bytes (or
/// waits for them), never a hole's zeros.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn rounds_under_readers_and_writers_never_serve_a_hole() {
    let l = std::sync::Arc::new(chosen(3).await);
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut readers = Vec::new();
    for seed in 1..=3u64 {
        let l = l.clone();
        readers.push(tokio::spawn(async move {
            let mut x = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let mut reads = 0;
            while Instant::now() < deadline {
                x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let a = (x >> 33) % l.len();
                let b = (a + 1 + (x >> 11) % (3 * PIECE)).min(l.len());
                let got = within(60, l.read((a, b))).await;
                assert!(got == l.source((a, b)), "read {a}..{b} differs");
                reads += 1;
            }
            reads
        }));
    }
    let (mut rounds, mut evicted) = (0, 0);
    let mut x = 7u64;
    while Instant::now() < deadline {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let first = ((x >> 33) % 18) as u32;
        let r = within(30, l.evict(&[first, first + 1, first + 2, first + 3])).await;
        rounds += 1;
        evicted += r.evicted.len();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut reads = 0;
    for r in readers {
        reads += within(90, r).await.unwrap();
    }
    eprintln!("{rounds} rounds, {evicted} pieces evicted, {reads} reads");
    assert!(
        rounds > 100 && evicted > 50 && reads > 30,
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
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_child", "--nocapture"])
            .env(CRASH, spec)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
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

/// The child of test f; a passing no-op in a normal run.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn crash_child() {
    let Ok(spec) = std::env::var(CRASH) else {
        return;
    };
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

/// j. A punch that fails (here after freeing its first block, as neither
/// call promises all or nothing): the probe says no, and a round's pieces
/// leave unclaimed and are fetched again, never claimed over a hole.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_punch_never_claims_its_pieces_again() {
    let ok = tempfile::tempdir().unwrap();
    assert!(mq_engine::librqbit::punch_supported(ok.path()));
    let refused = tempfile::tempdir().unwrap();
    sync_log::fail(refused.path(), Fail::Punch, usize::MAX);
    assert!(!mq_engine::librqbit::punch_supported(refused.path()));

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
