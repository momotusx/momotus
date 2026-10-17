//! Two engines on loopback: a seeder with the data and a leecher that gets
//! metadata and pieces from it, read back through range readers.

mod support;

use std::time::Duration;

use mq_engine::{Engine, FileHint, TorrentHandle};
use tokio::io::AsyncReadExt;

async fn read_range(
    engine: &Engine,
    t: TorrentHandle,
    file: usize,
    start: u64,
    end: u64,
) -> Vec<u8> {
    let reader = engine.open_reader(t, file).await.unwrap();
    let mut range = reader.range(start, Some(end)).await.unwrap();
    range.ready().await.unwrap();
    let mut out = Vec::new();
    range.read_to_end(&mut out).await.unwrap();
    out
}

async fn leech_and_compare(dwh: bool) {
    let s = support::swarm(dwh).await;
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
            .select_file(t, &FileHint::Name("movie.mkv".into()))
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
    leech_and_compare(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streams_from_a_loopback_seeder_with_the_stock_picker() {
    leech_and_compare(false).await;
}
