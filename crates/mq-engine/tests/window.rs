//! Eviction rounds (plans/streaming-window.md 2.5, phase 1): pieces leave
//! the disk while readers, writers and pauses go on, and the bitfield on
//! disk never claims a piece whose bytes are gone. The swarm is a 37,000
//! byte `.nfo` then a 5 MiB video in 256 KiB pieces, so every piece edge in
//! the video sits 3,960 bytes into a 4 KiB block.

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
    t: TorrentHandle,
    file: usize,
    _seeders: Vec<support::Seeder>,
}

impl Leech {
    fn engine(&self) -> &Engine {
        &self.s.leecher
    }

    /// The torrent's folder, which every hook of this test is keyed by.
    fn dir(&self) -> PathBuf {
        self.s.leech_dir.join(&self.s.info_hash)
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
        sync_log::under(&self.s.leech_dir).split_off(from)
    }

    fn log_len(&self) -> usize {
        sync_log::under(&self.s.leech_dir).len()
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
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &peers)
        .await
        .unwrap();
    let file = s
        .leecher
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    Leech {
        s,
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
    let hold = hold(&l.s.leech_dir, Point::Fsync);
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

/// f. A crash at each step (the folder copied while the round waits
/// there) leaves a bitfield whose every claimed piece has its bytes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_crash_at_each_step_leaves_only_true_claims() {
    for point in [Point::Gated, Point::Acked, Point::Punched] {
        let l = std::sync::Arc::new(leeched().await);
        let hold = hold(&l.dir(), point);
        let round = tokio::spawn({
            let l = l.clone();
            async move { l.evict(&[3, 4, 5, 9]).await }
        });
        reached(&hold).await;
        let copy = tempfile::tempdir().unwrap();
        let have_path = l.s.leech_dir.join(format!("{}.have", l.s.info_hash));
        let have = std::fs::read(&have_path).unwrap();
        let nfo = std::fs::read(l.dir().join("a.nfo")).unwrap_or_default();
        let movie = std::fs::read(l.movie_path()).unwrap();
        std::fs::write(copy.path().join("have"), &have).unwrap();
        hold.open();
        within(30, round).await.unwrap();
        let mut disk = nfo;
        disk.resize(NFO as usize, 0);
        disk.extend(movie);
        let mut source = vec![b'n'; NFO as usize];
        source.extend(&l.s.movie);
        let claimed: Vec<u32> = (0..21).filter(|&p| claims(&have, p)).collect();
        assert!(!claimed.is_empty(), "{point:?}: the bitfield claims pieces");
        for p in claimed {
            let a = u64::from(p) * PIECE;
            let b = (a + PIECE).min(source.len() as u64);
            assert!(
                disk.get(a as usize..b as usize) == Some(&source[a as usize..b as usize]),
                "{point:?}: piece {p} claimed over missing bytes"
            );
        }
    }
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
        sync_log::fail(&l.s.leech_dir, fail, 1);
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

/// j. A storage that refuses punches: the probe says no, and a round
/// keeps every piece.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_storage_that_refuses_punches_keeps_every_piece() {
    let ok = tempfile::tempdir().unwrap();
    assert!(mq_engine::librqbit::punch_supported(ok.path()));
    let refused = tempfile::tempdir().unwrap();
    sync_log::fail(refused.path(), Fail::Punch, usize::MAX);
    assert!(!mq_engine::librqbit::punch_supported(refused.path()));

    let l = leeched().await;
    sync_log::fail(&l.s.leech_dir, Fail::Punch, usize::MAX);
    let r = l.evict(&[5, 6]).await;
    assert_eq!((r.kept, r.evicted), (vec![5, 6], vec![]));
    assert!(r.error.is_some() && r.punched_bytes == 0);
    assert_eq!(l.have().await.0, 21);
    assert!(l.piece_matches(5).await);
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
    sync_log::fail(&l.s.leech_dir, Fail::Punch, 1);
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
/// piece whose bytes are there, and with no full check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sparse_folder_starts_with_nothing_claimed() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join(&s.info_hash);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.nfo"), vec![b'n'; NFO as usize]).unwrap();
    let movie = std::fs::File::create(folder.join("movie.mkv")).unwrap();
    movie.set_len(s.movie.len() as u64).unwrap();
    let (a, b) = (8 * PIECE - NFO, 9 * PIECE - NFO);
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
    let bits = std::fs::read(&have).unwrap();
    assert!(!claims(&bits, 8), "piece 8 claimed: a full check ran");
    assert!(bits.iter().all(|b| *b == 0), "{bits:?}");
}

async fn engine_on(dir: &Path) -> Engine {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    Engine::start(mq_engine::EngineConfig {
        discovery: false,
        ..mq_engine::EngineConfig::new(
            dir,
            http.torrent_scope(&[std::net::Ipv4Addr::LOCALHOST.into()]),
        )
    })
    .await
    .unwrap()
}
