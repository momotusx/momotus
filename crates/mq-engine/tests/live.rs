//! Real swarms (legal content from webtorrent.io's free torrents). Ignored by
//! default; run with
//! `cargo test -p mq-engine --test live -- --ignored --test-threads 1 --nocapture`.
//! Every engine runs under the production address policy (trackers, peers
//! and the DHT filtered), uploads nothing, and deletes its files.

use std::time::Duration;

use mq_engine::{Engine, EngineConfig, FileHint};
use tokio::io::AsyncReadExt;

const TRACKERS: [&str; 4] = [
    "udp://tracker.opentrackr.org:1337",
    "udp://explodie.org:6969",
    "http://tracker.opentrackr.org:1337/announce",
    "https://tr.nyacat.pw:443/announce",
];

/// Metadata, Ready, then 1 MiB at the head and at the middle of the
/// largest video, all within two minutes. Prints one JSON line of timings.
async fn stream(info_hash: &str, video: &str) {
    let dir = tempfile::tempdir().unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let config = EngineConfig {
        upload: false,
        ..EngineConfig::new(dir.path(), http.torrent_scope(&[]))
    };
    let engine = Engine::start(config).await.unwrap();
    let trackers: Vec<String> = TRACKERS.iter().map(|t| t.to_string()).collect();
    let started = std::time::Instant::now();
    let run = async {
        let t = engine.add_magnet(info_hash, &trackers, &[]).await.unwrap();
        let metadata_ms = started.elapsed().as_millis();
        let mut mib_ms = Vec::new();
        let file = engine
            .select_file(t, "default", &FileHint::Largest)
            .await
            .unwrap();
        let reader = engine.open_reader(t, "default", file).await.unwrap();
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
            mib_ms.push(started.elapsed().as_millis());
        }
        let stats = engine.stats(t).await.unwrap();
        assert!(stats.dwh && stats.peers > 0 && stats.ready_ms.is_some());
        engine.remove(t, false).await.unwrap();
        let refused = engine.refused();
        println!(
            "{{\"video\":\"{video}\",\"metadata_ms\":{metadata_ms},\"head_mib_ms\":{},\"middle_mib_ms\":{},\"ready_ms\":{},\"peers\":{},\"up_bytes_per_s\":{},\"refused\":{{\"trackers\":{},\"peers\":{},\"dht\":{}}}}}",
            mib_ms[0],
            mib_ms[1],
            stats.ready_ms.unwrap_or_default(),
            stats.peers,
            stats.up_bytes_per_s,
            refused.trackers,
            refused.peers,
            refused.dht
        );
    };
    tokio::time::timeout(Duration::from_secs(120), run)
        .await
        .expect("streamed within two minutes");
}

/// The patched tracker stack against real public trackers (C27 phase 5):
/// with the DHT off, UDP, HTTP and HTTPS trackers alone each find
/// Sintel's swarm under the production policy, and refuse nothing. Upload
/// is off. Prints one JSON line per kind.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "real swarm"]
async fn trackers_alone_find_the_swarm() {
    let kinds: [(&str, &[&str]); 3] = [
        (
            "udp",
            &[
                "udp://tracker.opentrackr.org:1337",
                "udp://explodie.org:6969",
            ],
        ),
        ("http", &["http://tracker.opentrackr.org:1337/announce"]),
        (
            "https",
            &[
                "https://tr.nyacat.pw:443/announce",
                "https://tracker.pmman.tech:443/announce",
                "https://open.ftorrent.com:443/announce",
            ],
        ),
    ];
    for (kind, trackers) in kinds {
        let dir = tempfile::tempdir().unwrap();
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let config = EngineConfig {
            discovery: false,
            upload: false,
            ..EngineConfig::new(dir.path(), http.torrent_scope(&[]))
        };
        let engine = Engine::start(config).await.unwrap();
        let trackers: Vec<String> = trackers.iter().map(|t| t.to_string()).collect();
        let started = std::time::Instant::now();
        let run = async {
            let t = engine
                .add_magnet("08ada5a7a6183aae1e09d831df6748d566095a10", &trackers, &[])
                .await
                .unwrap();
            let metadata_ms = started.elapsed().as_millis();
            let file = engine
                .select_file(t, "default", &FileHint::Largest)
                .await
                .unwrap();
            let reader = engine.open_reader(t, "default", file).await.unwrap();
            let mut range = reader.range(0, Some(1 << 20)).await.unwrap();
            range.ready().await.unwrap();
            let mut buf = Vec::new();
            range.read_to_end(&mut buf).await.unwrap();
            assert_eq!(buf.len(), 1 << 20);
            let stats = engine.stats(t).await.unwrap();
            engine.remove(t, false).await.unwrap();
            (metadata_ms, stats.peers)
        };
        let (metadata_ms, peers) = tokio::time::timeout(Duration::from_secs(120), run)
            .await
            .unwrap_or_else(|_| panic!("{kind} trackers alone found no swarm in two minutes"));
        let refused = engine.refused();
        println!(
            "{{\"trackers\":\"{kind}\",\"metadata_ms\":{metadata_ms},\"peers\":{peers},\"first_mib_ms\":{},\"refused_trackers\":{},\"refused_peers\":{}}}",
            started.elapsed().as_millis(),
            refused.trackers,
            refused.peers
        );
        assert_eq!(refused.trackers, 0, "{kind}: a public tracker was refused");
    }
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
