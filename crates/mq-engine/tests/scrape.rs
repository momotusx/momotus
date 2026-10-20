//! Live swarm counts by tracker scrape (`Engine::scrape`): loopback UDP and
//! HTTP trackers, the production address policy scoped to them, no DHT (so
//! no default trackers).

#[allow(dead_code)] // the HTTP tracker only
mod support;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mq_engine::{Engine, EngineConfig, SwarmCount};

const HASH: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";
const LOOPBACK: [IpAddr; 2] = [
    IpAddr::V4(Ipv4Addr::LOCALHOST),
    IpAddr::V6(Ipv6Addr::LOCALHOST),
];

/// An engine whose trackers may be reached on `allowed` only.
async fn engine(allowed: &[IpAddr]) -> (Engine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let cfg = EngineConfig {
        discovery: false,
        ..EngineConfig::new(dir.path(), http.torrent_scope(allowed))
    };
    (Engine::start(cfg).await.unwrap(), dir)
}

/// A BEP 15 tracker on 127.0.0.1: every scrape counts `seeders` and
/// `leechers` for each hash asked, sent from `answer_from` when given (an
/// address other than the one asked). Counts the datagrams it got.
fn udp_tracker(
    seeders: u32,
    leechers: u32,
    answer_from: Option<UdpSocket>,
) -> (String, Arc<AtomicUsize>) {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("udp://{}/announce", socket.local_addr().unwrap());
    let seen = Arc::new(AtomicUsize::new(0));
    let count = seen.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            count.fetch_add(1, Ordering::SeqCst);
            if n < 16 {
                continue;
            }
            let (action, tid) = (&buf[8..12], &buf[12..16]);
            let mut answer = Vec::from(action);
            answer.extend(tid);
            if action == 0u32.to_be_bytes() {
                answer.extend(7u64.to_be_bytes());
            } else {
                for _ in 0..(n - 16) / 20 {
                    answer.extend(seeders.to_be_bytes());
                    answer.extend(99u32.to_be_bytes()); // completed
                    answer.extend(leechers.to_be_bytes());
                }
            }
            let _ = answer_from
                .as_ref()
                .unwrap_or(&socket)
                .send_to(&answer, from);
        }
    });
    (url, seen)
}

/// An HTTP tracker on `[::1]` whose scrape answer counts `seeders` (and two
/// leechers) for `HASH`.
fn http_tracker(seeders: u32) -> support::HttpTracker {
    let mut body = b"d5:filesd20:".to_vec();
    body.extend(bytes(HASH));
    body.extend(format!("d8:completei{seeders}e10:downloadedi1e10:incompletei2eeee").as_bytes());
    let mut answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend(body);
    support::HttpTracker::start(answer, false)
}

/// An HTTP tracker on `[::1]` whose scrape answer is `body`.
fn http_tracker_with(body: Vec<u8>) -> support::HttpTracker {
    let mut answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend(body);
    support::HttpTracker::start(answer, false)
}

/// Engine review r1, P1-1: a scrape answer nested 10,000 deep reaches the
/// parser, which refuses it; 100,000 levels (200 KB) is over the answer cap
/// before any parse. Either way no count, and no crash (which would kill the
/// test binary).
#[tokio::test]
async fn a_deeply_nested_scrape_answer_counts_nothing() {
    let (engine, _dir) = engine(&LOOPBACK).await;
    for n in [10_000, 100_000] {
        let mut body = b"d5:filesd20:".to_vec();
        body.extend(bytes(HASH));
        body.extend(vec![b'l'; n]);
        body.extend(vec![b'e'; n]);
        body.extend(b"ee");
        let tracker = http_tracker_with(body);
        let sources = [tracker.url.clone()];
        let hash = if n == 10_000 { HASH } else { OTHER };
        let counts = engine.scrape(&[(hash, sources.as_slice())]).await;
        assert_eq!(counts, [None], "{n} levels");
        assert!(tracker.requests() >= 1, "{n}: never asked");
    }
}

fn bytes(hash: &str) -> Vec<u8> {
    (0..20)
        .map(|i| u8::from_str_radix(&hash[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// UDP and HTTP trackers answer together: each hash takes the tracker with
/// the most seeders, and one UDP scrape carries every hash.
#[tokio::test]
async fn the_tracker_with_the_most_seeders_counts() {
    let (udp, seen) = udp_tracker(1, 4, None);
    let http = http_tracker(12);
    let (engine, _dir) = engine(&LOOPBACK).await;
    let both = vec![format!("tracker:{udp}"), http.url.clone()];
    let one = vec![udp];
    let got = engine
        .scrape(&[(HASH, both.as_slice()), (OTHER, one.as_slice())])
        .await;
    let count = |seeders, leechers| Some(SwarmCount { seeders, leechers });
    assert_eq!(got, vec![count(12, 2), count(1, 4)]);
    assert_eq!(seen.load(Ordering::SeqCst), 2, "one connect, one scrape");
}

/// A tracker that never answers costs the budget, not the count.
#[tokio::test]
async fn a_silent_tracker_does_not_hold_the_count() {
    let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let (udp, _) = udp_tracker(5, 1, None);
    let (engine, _dir) = engine(&LOOPBACK).await;
    let sources = vec![
        format!("udp://{}/announce", silent.local_addr().unwrap()),
        udp,
    ];
    let started = Instant::now();
    let got = engine.scrape(&[(HASH, sources.as_slice())]).await;
    let took = started.elapsed();
    assert_eq!(
        got,
        vec![Some(SwarmCount {
            seeders: 5,
            leechers: 1
        })]
    );
    assert!(took < Duration::from_millis(1500), "{took:?}");
}

/// A tracker on an address outside the policy is never asked, over UDP or
/// HTTP, and the refusals count.
#[tokio::test]
async fn a_tracker_outside_the_policy_is_never_asked() {
    let (udp, seen) = udp_tracker(5, 1, None);
    let http = http_tracker(5);
    let (engine, _dir) = engine(&[]).await;
    let sources = vec![udp, http.url.clone()];
    let got = engine.scrape(&[(HASH, sources.as_slice())]).await;
    assert_eq!(got, vec![None]);
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert_eq!(http.connections(), 0);
    assert!(engine.refused().trackers >= 2);
}

/// An answer from another address than the one asked is ignored, as the
/// session's UDP trackers ignore it.
#[tokio::test]
async fn an_answer_from_another_address_is_ignored() {
    let other = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let (udp, seen) = udp_tracker(9, 1, Some(other));
    let (engine, _dir) = engine(&LOOPBACK).await;
    let sources = vec![udp];
    let got = engine.scrape(&[(HASH, sources.as_slice())]).await;
    assert_eq!(got, vec![None]);
    assert!(seen.load(Ordering::SeqCst) >= 1);
}

/// A count stands for a while: the same hash, in any case, asks nothing.
#[tokio::test]
async fn counts_stand_for_a_while() {
    let (udp, seen) = udp_tracker(3, 0, None);
    let (engine, _dir) = engine(&LOOPBACK).await;
    let sources = vec![udp];
    let first = engine.scrape(&[(HASH, sources.as_slice())]).await;
    assert_eq!(
        first,
        vec![Some(SwarmCount {
            seeders: 3,
            leechers: 0
        })]
    );
    let asked = seen.load(Ordering::SeqCst);
    let upper = HASH.to_ascii_uppercase();
    let second = engine.scrape(&[(upper.as_str(), sources.as_slice())]).await;
    assert_eq!(second, first);
    assert_eq!(seen.load(Ordering::SeqCst), asked, "asked again");
}
