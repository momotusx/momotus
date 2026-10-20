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

/// A torrent that goes tells every tracker that took its announce
/// (`stopped`), so a viewer who left is no longer in the swarm's counts
/// (engine review r3, P3-6). Its have-bitfield goes with its data.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_removed_torrent_announces_stopped() {
    let s = support::swarm(true).await;
    let body = b"d8:intervali60e5:peers0:e";
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
    read_range(&engine, t, movie, 0, 1 << 20).await;
    let have = dir.path().join(format!("{}.have", s.info_hash));
    assert!(have.exists(), "no have-bitfield beside the data");
    // The metadata fetch announced and may have stopped too: every request
    // so far is in before the count.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let stopped = |lines: Vec<String>| lines.iter().filter(|l| l.contains("event=stopped")).count();
    let before = stopped(tracker.request_lines());
    assert!(tracker.requests() >= 1, "never announced");
    engine.remove(t, false).await.unwrap();
    for _ in 0..30 {
        if stopped(tracker.request_lines()) > before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        stopped(tracker.request_lines()) > before,
        "no stopped announce: {:?}",
        tracker.request_lines()
    );
    assert!(!have.exists(), "the have-bitfield outlived the data");
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
    engine.remove(t, true).await.unwrap();
    let have = std::fs::read(dir.path().join(format!("{}.have", s.info_hash))).unwrap();
    assert!(
        have.iter().any(|b| *b != 0),
        "no piece on disk after the removal: {have:?}"
    );
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
