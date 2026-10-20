//! Two engines on loopback: a seeder with the data and a leecher that gets
//! metadata and pieces from it, read back through range readers.

mod support;

use std::net::Ipv4Addr;
use std::path::Path;
use std::time::{Duration, Instant};

use mq_engine::{Engine, FileHint, TorrentHandle};
use tokio::io::AsyncReadExt;

async fn read_range(
    engine: &Engine,
    t: TorrentHandle,
    file: usize,
    start: u64,
    end: u64,
) -> Vec<u8> {
    let reader = engine.open_reader(t, "default", file).await.unwrap();
    let mut range = reader.range(start, Some(end)).await.unwrap();
    range.ready().await.unwrap();
    let mut out = Vec::new();
    range.read_to_end(&mut out).await.unwrap();
    out
}

async fn leech_and_compare(s: support::Swarm, dwh: bool) {
    let run = async {
        let t = s
            .leecher
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let files = s.leecher.files(t).unwrap();
        assert_eq!(files.len(), 2);
        let upper = s.info_hash.to_uppercase();
        let again = s.leecher.add_magnet(&upper, &[], &[]).await.unwrap();
        assert_eq!(again, t, "a known torrent is not resolved again");
        let file = s
            .leecher
            .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        assert_eq!(files[file].len, s.movie.len() as u64);
        // A seek into the middle first, then the whole file.
        let (a, b) = (3 * 1024 * 1024 + 7, 4 * 1024 * 1024 + 1000);
        let mid = read_range(&s.leecher, t, file, a, b).await;
        assert!(mid == s.movie[a as usize..b as usize], "mid range differs");
        let all = read_range(&s.leecher, t, file, 0, s.movie.len() as u64).await;
        assert!(all == s.movie, "file differs");
        let stats = s.leecher.stats(t).await.unwrap();
        assert_eq!(stats.dwh, dwh);
        assert!(stats.ready_ms.is_some());
        if dwh {
            assert!(stats.requests > 0, "DWH sent the requests");
        }
        s.leecher.remove(t, false).await.unwrap();
    };
    tokio::time::timeout(Duration::from_secs(90), run)
        .await
        .expect("leecher finished in time");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_from_a_loopback_seeder_with_dwh() {
    leech_and_compare(support::swarm(true).await, true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_from_a_loopback_seeder_with_the_stock_picker() {
    leech_and_compare(support::swarm(false).await, false).await;
}

/// 136 KiB pieces, as YTS makes them: 8.5 blocks, so every piece ends in a
/// short block. DWH drives them and the bytes match.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_unaligned_pieces_with_dwh() {
    let ip = Ipv4Addr::LOCALHOST.into();
    let s = support::swarm_with(ip, &[ip], &[ip], 136 * 1024, true).await;
    leech_and_compare(s, true).await;
}

/// Two playbacks of one pack: the second one's file choice adds its file,
/// so the first file stays wanted and its verified pieces count as the
/// torrent's progress.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_file_choice_keeps_the_first_file_wanted() {
    let s = support::swarm(true).await;
    let run = async {
        let engine = &s.leecher;
        let t = engine
            .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
            .await
            .unwrap();
        let movie = engine
            .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
            .await
            .unwrap();
        let nfo = engine
            .select_file(t, "default", &FileHint::Name("a.nfo".into()))
            .await
            .unwrap();
        assert_ne!(nfo, movie);
        let (a, b) = (3 << 20, 4 << 20);
        let read = read_range(engine, t, movie, a as u64, b as u64).await;
        assert!(read == s.movie[a..b], "range differs");
        let stats = engine.stats(t).await.unwrap();
        assert!(
            stats.downloaded_bytes >= 1 << 20,
            "the movie's pieces are not wanted: {stats:?}"
        );
    };
    tokio::time::timeout(Duration::from_secs(90), run)
        .await
        .expect("finished in time");
}

/// With upload off a finished file is never announced as complete (engine
/// review r2, P3-3): its `left` stays above 0, so trackers never list us as
/// a seeder that serves nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_finished_file_with_upload_off_is_never_announced_complete() {
    let s = support::swarm(true).await;
    // Every announce gets an interval of 1 s and no peers: the engine
    // announces again every second.
    let body = b"d8:intervali1e5:peers0:e";
    let mut answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend(body);
    let tracker = support::HttpTracker::start(answer, false);
    let dir = tempfile::tempdir().unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let local = [
        Ipv4Addr::LOCALHOST.into(),
        std::net::Ipv6Addr::LOCALHOST.into(),
    ];
    let engine = Engine::start(mq_engine::EngineConfig {
        discovery: false,
        upload: false,
        ..mq_engine::EngineConfig::new(dir.path(), http.torrent_scope(&local))
    })
    .await
    .unwrap();
    let t = engine
        .add_magnet(
            &s.info_hash,
            std::slice::from_ref(&tracker.url),
            &[s.seeder_addr],
        )
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let whole = read_range(&engine, t, movie, 0, s.movie.len() as u64).await;
    assert_eq!(whole, s.movie, "the whole file");
    let seen = tracker.requests();
    for _ in 0..60 {
        if tracker.requests() >= seen + 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let lines = tracker.request_lines();
    assert!(
        lines.len() >= seen + 2,
        "no announce after the file finished: {lines:?}"
    );
    let last = lines.last().unwrap();
    let left: u64 = last
        .split(['?', '&', ' '])
        .find_map(|kv| kv.strip_prefix("left="))
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("no left in {last}"));
    assert!(
        left >= 1,
        "a finished file announced left=0 with upload off: {last}"
    );
    assert!(!last.contains("event=completed"), "{last}");
}

/// An engine on `dir` whose torrent of `s` announces to an HTTP tracker
/// (an interval of 60 s, no peers), its movie read: the tracker, the
/// engine, the torrent. Every request of the metadata fetch is in.
async fn tracked(
    s: &support::Swarm,
    dir: &Path,
) -> (support::HttpTracker, Engine, mq_engine::TorrentHandle) {
    let body = b"d8:intervali60e5:peers0:e";
    let mut answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend(body);
    let tracker = support::HttpTracker::start(answer, false);
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let local = [
        Ipv4Addr::LOCALHOST.into(),
        std::net::Ipv6Addr::LOCALHOST.into(),
    ];
    let engine = Engine::start(mq_engine::EngineConfig {
        discovery: false,
        upload: false,
        ..mq_engine::EngineConfig::new(dir, http.torrent_scope(&local))
    })
    .await
    .unwrap();
    let t = engine
        .add_magnet(
            &s.info_hash,
            std::slice::from_ref(&tracker.url),
            &[s.seeder_addr],
        )
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    read_range(&engine, t, movie, 0, 1 << 20).await;
    // The metadata fetch announced and may have stopped too.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(tracker.requests() >= 1, "never announced");
    (tracker, engine, t)
}

/// `stopped` announces the tracker took.
fn stopped(tracker: &support::HttpTracker) -> usize {
    tracker
        .request_lines()
        .iter()
        .filter(|l| l.contains("event=stopped"))
        .count()
}

/// Waits up to 3 s for more than `before` `stopped` announces.
async fn stopped_after(tracker: &support::HttpTracker, before: usize) -> bool {
    for _ in 0..30 {
        if stopped(tracker) > before {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// A torrent that goes tells every tracker that took its announce
/// (`stopped`), so a viewer who left is no longer in the swarm's counts
/// (engine review r3, P3-6). Its have-bitfield goes with its data.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_removed_torrent_announces_stopped() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let (tracker, engine, t) = tracked(&s, dir.path()).await;
    let have = dir.path().join(format!("{}.have", s.info_hash));
    assert!(have.exists(), "no have-bitfield beside the data");
    let before = stopped(&tracker);
    engine.remove(t, false).await.unwrap();
    assert!(
        stopped_after(&tracker, before).await,
        "no stopped announce: {:?}",
        tracker.request_lines()
    );
    assert!(!have.exists(), "the have-bitfield outlived the data");
}

/// A dropped engine stops its session: its torrents end with it (their
/// trackers hear `stopped`), not go on writing into a folder the next
/// engine owns (engine review r4, P3-3: the tracker stream held the session
/// alive).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dropped_engine_stops_its_torrents() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let (tracker, engine, _t) = tracked(&s, dir.path()).await;
    let before = stopped(&tracker);
    drop(engine);
    assert!(
        stopped_after(&tracker, before).await,
        "the dropped engine's torrent went on: {:?}",
        tracker.request_lines()
    );
}

/// An engine on `dir` that may reach 127.0.0.1, once the one before let go
/// of the folder.
async fn engine_on(dir: &Path) -> Engine {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let config = || mq_engine::EngineConfig {
        discovery: false,
        ..mq_engine::EngineConfig::new(dir, http.torrent_scope(&[Ipv4Addr::LOCALHOST.into()]))
    };
    for _ in 0..40 {
        if let Ok(engine) = Engine::start(config()).await {
            return engine;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Engine::start(config()).await.unwrap()
}

/// A relaunch whose have-bitfield claims a piece the file no longer holds
/// (a write the disk lost to a power cut reads back as zeros) checks every
/// piece and fetches that one again: the player never gets the zeros
/// (engine review r4, P1-1: the sample counted read errors, not a wrong
/// hash).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_relaunch_fetches_a_piece_the_disk_lost() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let head = 64 << 10;
    let first = engine_on(dir.path()).await;
    let t = first
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = first
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    read_range(&first, t, movie, 0, head as u64).await;
    // Kept, with the bitfield that claims the head.
    first.remove(t, true).await.unwrap();
    drop(first);
    let file = find(&dir.path().join(&s.info_hash), "movie.mkv").expect("the movie file");
    let lost = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
    std::os::unix::fs::FileExt::write_all_at(&lost, &vec![0u8; head], 0).unwrap();
    drop(lost);
    let again = engine_on(dir.path()).await;
    let t = again
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = again
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let got = read_range(&again, t, movie, 0, head as u64).await;
    assert!(
        got == s.movie[..head],
        "the relaunch served the bytes the disk lost"
    );
}

/// The file named `name` under `dir`, at any depth.
fn find(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let path = e.path();
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        } else if e.file_name() == name {
            return Some(path);
        }
    }
    None
}

/// The time from an engine's start to a torrent's first range, which waits
/// for the torrent's check. The engine before on `dir` may still be going
/// (its last maintenance round holds the folder's lock).
async fn first_range(dir: &Path, torrent: &[u8]) -> (Engine, Duration) {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let config = || mq_engine::EngineConfig {
        discovery: false,
        ..mq_engine::EngineConfig::new(dir, http.torrent_scope(&[]))
    };
    let mut engine = Engine::start(config()).await;
    for _ in 0..40 {
        if engine.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        engine = Engine::start(config()).await;
    }
    let engine = engine.unwrap();
    let started = Instant::now();
    let t = engine
        .add_torrent_bytes(torrent.to_vec(), &[])
        .await
        .unwrap();
    let file = engine
        .select_file(t, "default", &FileHint::Largest)
        .await
        .unwrap();
    let reader = engine.open_reader(t, "default", file).await.unwrap();
    reader.range(0, Some(1)).await.unwrap();
    (engine, started.elapsed())
}

/// A relaunch knows a torrent's pieces from its have-bitfield (the fork's
/// fast resume) and checks only a sample of those it had, never every byte
/// again: here 1 GiB of holes, about 2 s to hash in a release build and
/// half a minute in a debug one (engine review r3, P2-2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_relaunch_skips_the_full_check() {
    const PIECE: u64 = 4 << 20;
    const LEN: u64 = 1 << 30;
    let hashes = (LEN / PIECE * 20) as usize;
    let mut torrent =
        format!("d4:infod6:lengthi{LEN}e4:name7:big.mkv12:piece lengthi{PIECE}e6:pieces{hashes}:")
            .into_bytes();
    torrent.extend(vec![0xab; hashes]);
    torrent.extend(b"ee");
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = first_range(dir.path(), &torrent).await;
    drop(first);
    let (_again, took) = first_range(dir.path(), &torrent).await;
    assert!(
        took < Duration::from_millis(1500),
        "the relaunch checked every byte: {took:?}"
    );
}

/// A torrent that goes leaves its have-bitfield on disk at once: a process
/// that ends right after (the CLI) never runs the bitfield's flusher again,
/// and the next launch would fetch those pieces anew (engine review r3,
/// P2-2). One thread, so nothing runs between the removal and the read; a
/// small budget keeps readahead from finishing the file, and under 16 MiB
/// arrives (each of which flushes on its own).
#[tokio::test(flavor = "current_thread")]
async fn a_removed_torrent_leaves_its_pieces_on_disk() {
    let movie: Vec<u8> = (0..24u32 << 20)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let s = support::swarm_movie("movie.mkv", movie).await;
    let dir = tempfile::tempdir().unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let engine = Engine::start(mq_engine::EngineConfig {
        discovery: false,
        // A 2 MiB readahead.
        cache_budget_bytes: 8 << 20,
        ..mq_engine::EngineConfig::new(
            dir.path(),
            http.torrent_scope(&[Ipv4Addr::LOCALHOST.into()]),
        )
    })
    .await
    .unwrap();
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    read_range(&engine, t, movie, 0, 1 << 20).await;
    let stats = engine.stats(t).await.unwrap();
    assert!(
        stats.pieces_have < stats.pieces_total && stats.downloaded_bytes < 16 << 20,
        "the file finished or 16 MiB came, which flushes on its own: {stats:?}"
    );
    let from = log_len(dir.path());
    engine.remove(t, true).await.unwrap();
    let have = std::fs::read(dir.path().join(format!("{}.have", s.info_hash))).unwrap();
    assert!(
        have.iter().any(|b| *b != 0),
        "no piece on disk after the removal: {have:?}"
    );
    // Its data first (engine review r4, P1-1), in this folder's own log
    // (r5, P3-3).
    assert_synced_before_claimed(dir.path(), from);
}

/// A magnet whose peers all failed says why the last one did (the fork
/// carries its error): here a peer that drops every connection unanswered,
/// as the kids_play seeder once did ("Connection reset by peer" while the
/// handshake was read).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_magnet_whose_peers_all_failed_says_why() {
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let peer = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            drop(stream);
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let engine = Engine::start(mq_engine::EngineConfig {
        discovery: false,
        ..mq_engine::EngineConfig::new(
            dir.path(),
            http.torrent_scope(&[Ipv4Addr::LOCALHOST.into()]),
        )
    })
    .await
    .unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    let err = engine.add_magnet(hash, &[], &[peer]).await.unwrap_err();
    let err = format!("{err:?}");
    assert!(
        err.contains(&format!("error reading metainfo from {peer}")),
        "the cause is missing: {err}"
    );
}

/// The flusher syncs a finished file's data before its bitfield claims it
/// (engine review r4, P1-1: the bitfield was fsynced, the data never),
/// read in this folder's own log (r5, P3-3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_finished_file_is_claimed_after_its_data_synced() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_on(dir.path()).await;
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let from = log_len(dir.path());
    let whole = read_range(&engine, t, movie, 0, s.movie.len() as u64).await;
    assert_eq!(whole, s.movie);
    let path = dir.path().join(format!("{}.have", s.info_hash));
    let mut claimed = false;
    for _ in 0..50 {
        claimed = std::fs::read(&path).is_ok_and(|have| have.first() == Some(&0xff));
        if claimed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(claimed, "the finished file was never claimed on disk");
    assert_synced_before_claimed(dir.path(), from);
}

/// A have-bitfield the data contradicts is cleared, by info hash, before
/// the full check: a check cut short (a pause, a removal) must not leave
/// it for the next launch (engine review r4, P1-1: the clear went by the
/// torrent's id, which the folder store ignores). Here every piece is
/// claimed over 256 MiB of holes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_contradicted_have_bitfield_is_cleared_at_once() {
    const PIECE: u64 = 4 << 20;
    const LEN: u64 = 256 << 20;
    let hashes = (LEN / PIECE * 20) as usize;
    let mut torrent =
        format!("d4:infod6:lengthi{LEN}e4:name7:big.mkv12:piece lengthi{PIECE}e6:pieces{hashes}:")
            .into_bytes();
    torrent.extend(vec![0xab; hashes]);
    torrent.extend(b"ee");
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = first_range(dir.path(), &torrent).await;
    drop(first);
    let have = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "have"))
        .expect("a have-bitfield");
    // A new file in its place: the first engine's flusher may still write
    // its last state into the old one.
    let forged = dir.path().join("forged");
    std::fs::write(&forged, vec![0xff; (LEN / PIECE / 8) as usize]).unwrap();
    std::fs::rename(&forged, &have).unwrap();
    // Dense enough that the start runs the full check (a sparse folder
    // with no bitfield would store an empty one at once).
    let data = find(dir.path(), "big.mkv").expect("the data file");
    let file = std::fs::OpenOptions::new().write(true).open(&data).unwrap();
    std::os::unix::fs::FileExt::write_all_at(&file, &vec![0xab; 40 << 20], 0).unwrap();
    drop(file);
    let engine = engine_on(dir.path()).await;
    let t = engine.add_torrent_bytes(torrent, &[]).await.unwrap();
    engine
        .select_file(t, "default", &FileHint::Largest)
        .await
        .unwrap();
    let mut cleared = false;
    for _ in 0..1500 {
        if !have.exists() {
            cleared = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    engine.remove(t, false).await.unwrap();
    assert!(
        cleared,
        "the contradicted bitfield stayed through the check"
    );
}

/// Events in the fork's sync log for files under `dir` so far (none
/// without the `test-hooks` feature, which turns the log on).
fn log_len(dir: &Path) -> usize {
    #[cfg(feature = "test-hooks")]
    return mq_engine::librqbit::sync_log::under(dir).len();
    #[cfg(not(feature = "test-hooks"))]
    {
        let _ = dir;
        0
    }
}

/// In the log for `dir` after its first `from` events, the movie's data
/// synced before the first bitfield write that claims a piece. Checked only
/// with the `test-hooks` feature.
fn assert_synced_before_claimed(dir: &Path, from: usize) {
    #[cfg(feature = "test-hooks")]
    {
        use mq_engine::librqbit::sync_log::Event;
        let events = mq_engine::librqbit::sync_log::under(dir);
        let events = &events[from.min(events.len())..];
        let synced = events
            .iter()
            .position(|e| matches!(e, Event::DataSynced(p) if p.ends_with("movie.mkv")));
        let claimed = events
            .iter()
            .position(|e| matches!(e, Event::HaveWritten { claimed, .. } if *claimed > 0));
        assert!(
            matches!((synced, claimed), (Some(s), Some(c)) if s < c),
            "no data sync before the first claim: {events:?}"
        );
    }
    #[cfg(not(feature = "test-hooks"))]
    let _ = (dir, from);
}

/// An engine on `dir` with a 2 MiB readahead (an 8 MiB budget), the 24 MiB
/// movie of `s` selected and its first MiB read: under 16 MiB arrives and
/// the file does not finish, so nothing flushes on its own.
async fn head_read(s: &support::Swarm, dir: &Path) -> (Engine, mq_engine::TorrentHandle) {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let engine = Engine::start(mq_engine::EngineConfig {
        discovery: false,
        cache_budget_bytes: 8 << 20,
        ..mq_engine::EngineConfig::new(dir, http.torrent_scope(&[Ipv4Addr::LOCALHOST.into()]))
    })
    .await
    .unwrap();
    let t = engine
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = engine
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    read_range(&engine, t, movie, 0, 1 << 20).await;
    let stats = engine.stats(t).await.unwrap();
    assert!(
        stats.pieces_have < stats.pieces_total && stats.downloaded_bytes < 16 << 20,
        "the file finished or 16 MiB came, which flushes on its own: {stats:?}"
    );
    (engine, t)
}

/// A 24 MiB movie swarm.
async fn big_swarm() -> support::Swarm {
    let movie: Vec<u8> = (0..24u32 << 20)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    support::swarm_movie("movie.mkv", movie).await
}

/// A removal sets the data aside before the session drops the torrent, so
/// the bitfield finds no file to sync: the caller never waits for the disk
/// to write what it deletes a moment later (engine review r5, P3-1).
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_removal_syncs_no_data() {
    use mq_engine::librqbit::sync_log::{Event, under};
    let s = big_swarm().await;
    let dir = tempfile::tempdir().unwrap();
    let (engine, t) = head_read(&s, dir.path()).await;
    let from = log_len(dir.path());
    engine.remove(t, false).await.unwrap();
    // The pause's flush runs on the flusher: give it time.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let synced: Vec<Event> = under(dir.path())[from..]
        .iter()
        .filter(|e| matches!(e, Event::DataSynced(_)))
        .cloned()
        .collect();
    assert!(
        synced.is_empty(),
        "a removal synced data it deletes: {synced:?}"
    );
    assert!(!dir.path().join(&s.info_hash).exists(), "the data stayed");
}

/// A full check's result is stored only after the data it claims is synced
/// (engine review r5, P3-2): here a relaunch with no bitfield checks the
/// head the first run fetched.
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_check_syncs_what_it_claims_before_storing_it() {
    let s = support::swarm(true).await;
    let dir = tempfile::tempdir().unwrap();
    let first = engine_on(dir.path()).await;
    let t = first
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = first
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    read_range(&first, t, movie, 0, 64 << 10).await;
    first.remove(t, true).await.unwrap();
    drop(first);
    // The first engine's flusher writes its last state meanwhile.
    tokio::time::sleep(Duration::from_millis(500)).await;
    std::fs::remove_file(dir.path().join(format!("{}.have", s.info_hash))).unwrap();
    let from = log_len(dir.path());
    let again = engine_on(dir.path()).await;
    let t = again
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let movie = again
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    // The range waits for the check.
    let reader = again.open_reader(t, "default", movie).await.unwrap();
    reader.range(0, Some(1)).await.unwrap();
    assert_synced_before_claimed(dir.path(), from);
}

/// The data sync opens each file with write access: Windows flushes only a
/// writable handle, so a read-only one failed every sync and no bitfield
/// was written there (engine review r5, P3-4). A file this process may
/// write but not read stands in for it on Unix.
#[cfg(feature = "test-hooks")]
#[tokio::test(flavor = "current_thread")]
async fn a_data_sync_opens_the_file_for_writing() {
    use std::os::unix::fs::PermissionsExt;
    let s = big_swarm().await;
    let dir = tempfile::tempdir().unwrap();
    let (engine, t) = head_read(&s, dir.path()).await;
    let file = find(&dir.path().join(&s.info_hash), "movie.mkv").expect("the movie file");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o200)).unwrap();
    let from = log_len(dir.path());
    engine.remove(t, true).await.unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_synced_before_claimed(dir.path(), from);
}

/// A pause flushes the bitfield: the apps are killed, never closed, after
/// their idle torrents pause, so claims waiting for the next 16 MiB were
/// lost (engine review r5, P3-5). A hold pauses the torrent and the
/// `.have` claims the head while the engine lives.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_torrent_flushes_its_bitfield() {
    let s = big_swarm().await;
    let dir = tempfile::tempdir().unwrap();
    let (engine, t) = head_read(&s, dir.path()).await;
    let path = dir.path().join(format!("{}.have", s.info_hash));
    let claims = || std::fs::read(&path).is_ok_and(|have| have.iter().any(|b| *b != 0));
    assert!(!claims(), "claimed before the pause");
    assert!(engine.pause_now(t, "kid", 1).await.unwrap());
    let mut claimed = false;
    for _ in 0..30 {
        claimed = claims();
        if claimed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(claimed, "the pause left the claims in memory");
}
