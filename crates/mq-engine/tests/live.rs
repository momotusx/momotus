//! Real swarms (legal content from webtorrent.io's free torrents). Ignored by
//! default; run with
//! `cargo test -p mq-engine --test live -- --ignored --test-threads 1`.

use std::time::Duration;

use mq_engine::{Engine, EngineConfig, FileHint};
use tokio::io::AsyncReadExt;

const TRACKERS: [&str; 3] = [
    "udp://tracker.opentrackr.org:1337",
    "udp://explodie.org:6969",
    "wss://tracker.btorrent.xyz",
];

/// Metadata, Ready, then 1 MiB at the head and at the middle of the
/// largest video, all within two minutes.
async fn stream(info_hash: &str, video: &str) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::start(EngineConfig::new(dir.path())).await.unwrap();
    let trackers: Vec<String> = TRACKERS.iter().map(|t| t.to_string()).collect();
    let run = async {
        let t = engine.add_magnet(info_hash, &trackers, &[]).await.unwrap();
        let file = engine.select_file(t, &FileHint::Largest).await.unwrap();
        let reader = engine.open_reader(t, file).await.unwrap();
        assert!(
            reader.file().path.ends_with(video),
            "{}",
            reader.file().path
        );
        for start in [0, reader.len() / 2] {
            let mut range = reader.range(start, Some(start + (1 << 20))).await.unwrap();
            range.ready().await.unwrap();
            let mut buf = Vec::new();
            range.read_to_end(&mut buf).await.unwrap();
            assert_eq!(buf.len(), 1 << 20);
        }
        let stats = engine.stats(t).await.unwrap();
        assert!(stats.dwh && stats.peers > 0 && stats.ready_ms.is_some());
        engine.remove(t, false).await.unwrap();
    };
    tokio::time::timeout(Duration::from_secs(120), run)
        .await
        .expect("streamed within two minutes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "real swarm"]
async fn sintel() {
    stream("08ada5a7a6183aae1e09d831df6748d566095a10", "Sintel.mp4").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "real swarm"]
async fn big_buck_bunny() {
    stream(
        "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
        "Big Buck Bunny.mp4",
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "real swarm"]
async fn tears_of_steel() {
    stream(
        "209c8226b299b308beaf2b9cd3fb49212dbd13ec",
        "Tears of Steel.webm",
    )
    .await;
}
