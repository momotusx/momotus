//! Real swarms (legal content from webtorrent.io's free torrents). Ignored by
//! default; run with
//! `cargo test -p mq-engine --test live -- --ignored --test-threads 1 --nocapture`.
//! Every engine runs under the production address policy (trackers, peers
//! and the DHT filtered), uploads nothing, and deletes its files.

use std::str::FromStr;
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

/// The DHT alone, with no tracker, finds Sintel's swarm under the
/// production policy, from a fresh table (bootstrap names resolved by the
/// system, every datagram checked). Upload off. Prints one JSON line.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "real swarm"]
async fn dht_alone_finds_the_swarm() {
    let dir = tempfile::tempdir().unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let config = EngineConfig {
        upload: false,
        ..EngineConfig::new(dir.path(), http.torrent_scope(&[]))
    };
    let engine = Engine::start(config).await.unwrap();
    let started = std::time::Instant::now();
    let run = async {
        let t = engine
            .add_magnet("08ada5a7a6183aae1e09d831df6748d566095a10", &[], &[])
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
        let peers = engine.stats(t).await.unwrap().peers;
        engine.remove(t, false).await.unwrap();
        (metadata_ms, peers)
    };
    let (metadata_ms, peers) = tokio::time::timeout(Duration::from_secs(180), run)
        .await
        .expect("the DHT alone found the swarm within three minutes");
    let refused = engine.refused();
    println!(
        "{{\"sources\":\"dht\",\"metadata_ms\":{metadata_ms},\"first_mib_ms\":{},\"peers\":{peers},\"refused\":{{\"trackers\":{},\"peers\":{},\"dht\":{}}}}}",
        started.elapsed().as_millis(),
        refused.trackers,
        refused.peers,
        refused.dht
    );
}

/// Whether public UDP trackers answer from the address they were asked
/// (C27 r3 P3: the policy takes an answer only from there). Reads one
/// tracker URL per line from the file `MQ_UDP_TRACKERS` names, announces
/// Sintel to each through a client with the policy and one without, and
/// prints one JSON line per tracker and a summary. A measurement, not a
/// gate: public trackers drop requests, so a tracker that answered only
/// one client is checked by hand (bench/c27/live-stack.jsonl).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "real trackers; set MQ_UDP_TRACKERS"]
async fn udp_trackers_answer_from_the_address_asked() {
    use mq_engine::librqbit::dht::Id20;
    use mq_engine::librqbit::tracker_comms::{AnnounceFields, Policy, UdpTrackerClient};
    let list = std::fs::read_to_string(std::env::var("MQ_UDP_TRACKERS").unwrap()).unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let scope = http.torrent_scope(&[]);
    let permits = scope.clone();
    let policy = Policy {
        permits: std::sync::Arc::new(move |ip| permits.permits(ip)),
        resolver: scope.resolver(),
    };
    let with = UdpTrackerClient::new(Default::default(), None, Some(policy))
        .await
        .unwrap();
    let without = UdpTrackerClient::new(Default::default(), None, None)
        .await
        .unwrap();
    let fields = || AnnounceFields {
        info_hash: Id20::from_str("08ada5a7a6183aae1e09d831df6748d566095a10").unwrap(),
        peer_id: Id20::new(*b"-MQ0100-c27probe0001"),
        downloaded: 0,
        left: 1,
        uploaded: 0,
        event: 0,
        key: 7,
        port: 6881,
    };
    let mut only_without = Vec::new();
    for url in list.lines().map(str::trim) {
        // `udp://host:port/announce`, the host bracketed when IPv6.
        let Some(rest) = url.strip_prefix("udp://") else {
            continue;
        };
        let authority = rest.split('/').next().unwrap_or_default();
        let Some((host, port)) = authority.rsplit_once(':') else {
            continue;
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let Ok(Some(addr)) = tokio::net::lookup_host((host, port))
            .await
            .map(|mut a| a.next())
        else {
            println!("{{\"tracker\":\"{url}\",\"resolved\":false}}");
            continue;
        };
        // Public trackers drop requests now and then: up to three tries each,
        // the two clients taking turns.
        let ask = |client: &UdpTrackerClient| {
            let client = client.clone();
            async move {
                tokio::time::timeout(Duration::from_secs(5), client.announce(addr, fields()))
                    .await
                    .is_ok_and(|r| r.is_ok())
            }
        };
        let (mut a, mut b) = (false, false);
        for _ in 0..3 {
            a = a || ask(&with).await;
            b = b || ask(&without).await;
            if a && b {
                break;
            }
        }
        println!(
            "{{\"tracker\":\"{url}\",\"addr\":\"{addr}\",\"with_policy\":{a},\"without\":{b}}}"
        );
        if b && !a {
            only_without.push(url.to_owned());
        }
    }
    println!("{{\"answered_only_without_the_policy\":{only_without:?}}}");
}
