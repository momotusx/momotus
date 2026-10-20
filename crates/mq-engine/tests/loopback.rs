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
/// again: here 8 GiB of holes, seconds to hash (engine review r3, P2-2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_relaunch_skips_the_full_check() {
    const PIECE: u64 = 4 << 20;
    const LEN: u64 = 8 << 30;
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
