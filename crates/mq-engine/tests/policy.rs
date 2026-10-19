//! The engine's address policy (plans/c27-torrent-policy.md 4.2, phase 2).
//! Canaries on loopback stand for addresses the policy refuses: each test
//! runs the protected case under the production rule (or `[::1]` when it
//! needs a seeder, which is addressed as `[::1]`) and a control that also
//! allows the canary's address, which shows the canary can be reached.

// Only the scoped swarms are used here.
#[allow(dead_code)]
mod support;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mq_engine::librqbit::{Session, SessionOptions};
use mq_engine::{Engine, EngineConfig, FileHint};
use mq_http::test_dns::{self, Canary, Interface};
use tokio::io::AsyncReadExt;
use tokio::time::timeout;

const V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);
/// An info hash no swarm has.
const NOWHERE: &str = "0123456789abcdef0123456789abcdef01234567";

/// The mapped form (`[::ffff:127.0.0.1]`) of an IPv4 canary's address.
fn mapped(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(v4) => SocketAddr::new(IpAddr::V6(v4.ip().to_ipv6_mapped()), v4.port()),
        v6 => v6,
    }
}

/// An engine of its own, discovery off, allowed `scope`.
async fn engine(scope: &[IpAddr]) -> (Engine, mq_http::HttpClient, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let config = EngineConfig {
        discovery: false,
        ..EngineConfig::new(dir.path(), http.torrent_scope(scope))
    };
    (Engine::start(config).await.unwrap(), http, dir)
}

/// A canary counts after it accepts: a moment for the count to settle.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// Metadata, the movie and its first MiB from the swarm, checked.
async fn first_mib(s: &support::Swarm, peers: &[SocketAddr], trackers: &[String]) {
    let t = s
        .leecher
        .add_magnet(&s.info_hash, trackers, peers)
        .await
        .unwrap();
    let file = s
        .leecher
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let reader = s.leecher.open_reader(t, "default", file).await.unwrap();
    let mut range = reader.range(0, Some(1 << 20)).await.unwrap();
    range.ready().await.unwrap();
    let mut bytes = Vec::new();
    range.read_to_end(&mut bytes).await.unwrap();
    assert!(bytes == s.movie[..1 << 20], "the seeder's bytes");
}

fn assert_canaries(control: bool, canaries: &[&Canary]) {
    let hits: Vec<usize> = canaries.iter().map(|c| c.hits()).collect();
    if control {
        assert!(
            hits.iter().all(|&h| h >= 1),
            "control: each canary reached {hits:?}"
        );
    } else {
        assert!(
            hits.iter().all(|&h| h == 0),
            "a canary was dialed: {hits:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_canary_peer_is_never_dialed() {
    // Metadata phase: the canaries are the only peers.
    for control in [false, true] {
        let scope = if control { vec![V4] } else { vec![] };
        let (literal, mapped_canary) = (Canary::tcp(V4, 0), Canary::tcp(V4, 0));
        let (engine, _http, _dir) = engine(&scope).await;
        let before = engine.refused();
        let peers = [literal.addr(), mapped(mapped_canary.addr())];
        let added = timeout(
            Duration::from_secs(5),
            engine.add_magnet(NOWHERE, &[], &peers),
        )
        .await
        .expect("the metadata round ends within 5 s");
        assert!(added.is_err(), "no metadata from a canary");
        settle().await;
        assert_canaries(control, &[&literal, &mapped_canary]);
        if !control {
            let refused = engine.refused().since(before);
            assert!(refused.peers >= 2, "{refused:?}");
        }
    }
    // Live phase: the seeder on [::1] with the canaries beside it.
    for control in [false, true] {
        let leecher = if control { vec![V6, V4] } else { vec![V6] };
        let s = support::swarm_scoped(V6, &[V6], &leecher, true).await;
        let (literal, mapped_canary) = (Canary::tcp(V4, 0), Canary::tcp(V4, 0));
        let peers = [s.seeder_addr, literal.addr(), mapped(mapped_canary.addr())];
        let before = s.leecher.refused();
        timeout(Duration::from_secs(60), first_mib(&s, &peers, &[]))
            .await
            .expect("played from the seeder");
        settle().await;
        assert_canaries(control, &[&literal, &mapped_canary]);
        assert_refused(control, s.leecher.refused().since(before).peers);
    }
}

/// Protected runs count each canary they refused (at least the two forms);
/// controls refuse nothing.
fn assert_refused(control: bool, peers: u64) {
    if control {
        assert_eq!(peers, 0, "control: nothing refused");
    } else {
        assert!(peers >= 2, "each canary refused and counted: {peers}");
    }
}

/// An HTTP tracker on `[::1]` that answers every announce with `peers` in
/// the compact forms (`peers` for IPv4, `peers6` for IPv6). Returns its
/// announce URL and how many announces it got.
fn tracker(peers: &[SocketAddr]) -> (String, Arc<AtomicUsize>) {
    use std::io::{Read, Write};
    let (mut v4, mut v6) = (Vec::new(), Vec::new());
    for peer in peers {
        match peer {
            SocketAddr::V4(a) => {
                v4.extend(a.ip().octets());
                v4.extend(a.port().to_be_bytes());
            }
            SocketAddr::V6(a) => {
                v6.extend(a.ip().octets());
                v6.extend(a.port().to_be_bytes());
            }
        }
    }
    let mut body = format!("d8:intervali1800e5:peers{}:", v4.len()).into_bytes();
    body.extend(&v4);
    body.extend(format!("6:peers6{}:", v6.len()).bytes());
    body.extend(&v6);
    body.push(b'e');
    let listener = std::net::TcpListener::bind((V6, 0)).unwrap();
    let url = format!("http://{}/announce", listener.local_addr().unwrap());
    let announces = Arc::new(AtomicUsize::new(0));
    let counted = announces.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut head = Vec::new();
            let mut buf = [0u8; 1024];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => head.extend_from_slice(&buf[..n]),
                }
            }
            counted.fetch_add(1, Ordering::SeqCst);
            let mut answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            answer.extend(&body);
            let _ = stream.write_all(&answer);
        }
    });
    (url, announces)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn peers_a_tracker_returns_stay_inside_the_policy() {
    for control in [false, true] {
        let leecher = if control { vec![V6, V4] } else { vec![V6] };
        let s = support::swarm_scoped(V6, &[V6], &leecher, true).await;
        let (literal, mapped_canary) = (Canary::tcp(V4, 0), Canary::tcp(V4, 0));
        let (url, announces) =
            tracker(&[literal.addr(), s.seeder_addr, mapped(mapped_canary.addr())]);
        // No peers given: the tracker is the only way to the seeder.
        let before = s.leecher.refused();
        timeout(Duration::from_secs(60), first_mib(&s, &[], &[url]))
            .await
            .expect("played through the tracker");
        assert!(announces.load(Ordering::SeqCst) >= 1);
        settle().await;
        assert_canaries(control, &[&literal, &mapped_canary]);
        assert_refused(control, s.leecher.refused().since(before).peers);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silent_peer_outside_the_policy_is_refused_before_any_read() {
    // A peer that connects and says nothing: refused at once, never waited
    // on for a handshake.
    let (refusing, _http, _dir) = engine(&[]).await;
    let port = refusing.listen_addr().unwrap().port();
    let before = refusing.refused();
    let mut silent = tokio::net::TcpStream::connect((V6, port)).await.unwrap();
    let mut byte = [0u8; 1];
    let read = timeout(Duration::from_secs(2), silent.read(&mut byte))
        .await
        .expect("closed without waiting for a handshake");
    assert!(matches!(read, Ok(0) | Err(_)), "{read:?}");
    let refused = refusing.refused().since(before);
    assert!(refused.peers >= 1, "{refused:?}");
    // Control: a silent peer the policy allows is waited on (the handshake
    // read times out after 10 s), so the close above is the policy's.
    let (open, _http, _dir) = engine(&[V6]).await;
    let port = open.listen_addr().unwrap().port();
    let mut kept = tokio::net::TcpStream::connect((V6, port)).await.unwrap();
    let read = timeout(Duration::from_secs(2), kept.read(&mut byte)).await;
    assert!(read.is_err(), "control: still waiting for its handshake");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_peer_outside_the_policy_cannot_connect_in() {
    // The seeder allows nobody; the leecher may dial it.
    let s = support::swarm_scoped(V6, &[], &[V6], true).await;
    let before = s._seeder.refused();
    let added = timeout(
        Duration::from_secs(5),
        s.leecher.add_magnet(&s.info_hash, &[], &[s.seeder_addr]),
    )
    .await
    .expect("the metadata round ends within 5 s");
    assert!(added.is_err(), "the seeder answered nothing");
    let refused = s._seeder.refused().since(before);
    assert!(refused.peers >= 1, "{refused:?}");
    // Control: a seeder that allows the leecher's address serves it.
    let s = support::swarm_on(V6, true).await;
    timeout(
        Duration::from_secs(60),
        first_mib(&s, &[s.seeder_addr], &[]),
    )
    .await
    .expect("served");
}

/// A uTP SYN (BEP 29): type 4, version 1, connection id 0x1234, seq 1.
fn utp_syn() -> [u8; 20] {
    let mut syn = [0u8; 20];
    syn[0] = 0x41;
    syn[2..4].copy_from_slice(&0x1234u16.to_be_bytes());
    syn[4..8].copy_from_slice(&1u32.to_be_bytes());
    syn[12..16].copy_from_slice(&0x0010_0000u32.to_be_bytes());
    syn[16..18].copy_from_slice(&1u16.to_be_bytes());
    syn
}

/// What comes back within a second of a SYN from `127.0.0.1` to `port`.
async fn answer_to_a_syn(port: u16) -> Option<Vec<u8>> {
    let socket = tokio::net::UdpSocket::bind((V4, 0)).await.unwrap();
    socket.send_to(&utp_syn(), (V4, port)).await.unwrap();
    let mut buf = [0u8; 1500];
    match timeout(Duration::from_secs(1), socket.recv_from(&mut buf)).await {
        Ok(Ok((n, _))) => Some(buf[..n].to_vec()),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn incoming_utp_from_a_refused_source_gets_no_answer() {
    // The production rule refuses loopback: no SYN-ACK, no RESET.
    let (engine, _http, _dir) = engine(&[]).await;
    let port = engine.listen_addr().unwrap().port();
    let before = engine.refused().peers;
    assert_eq!(
        answer_to_a_syn(port).await,
        None,
        "answered a refused source"
    );
    assert!(engine.refused().peers > before, "the refusal is counted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn incoming_utp_from_an_allowed_source_connects() {
    // Control: with loopback allowed, the same SYN gets its SYN-ACK.
    let (engine, _http, _dir) = engine(&[V4]).await;
    let port = engine.listen_addr().unwrap().port();
    let answer = answer_to_a_syn(port)
        .await
        .expect("no answer to an allowed source");
    assert_eq!(answer[0], 0x21, "an ST_STATE, version 1");
    assert_eq!(answer[18..20], 1u16.to_be_bytes(), "acknowledges seq 1");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_add_rereads_the_interfaces() {
    let (engine, http, _dir) = engine(&[]).await;
    let b: IpAddr = "2001:db8:1:3::abcd".parse().unwrap();
    test_dns::set_interfaces(
        &http,
        Ok(vec![Interface {
            addr: b,
            mask: Some("ffff:ffff:ffff:ffff::".parse().unwrap()),
            loopback: false,
            point_to_point: false,
        }]),
    );
    assert_ne!(test_dns::own_addresses(&http), [b], "not read yet");
    let _ = timeout(
        Duration::from_millis(500),
        engine.add_magnet(NOWHERE, &[], &[]),
    )
    .await;
    assert_eq!(test_dns::own_addresses(&http), [b], "read by the add");
}

// Phase 3: trackers.

/// A torrent with only `trackers`, under `scope`, for `wait`: its trackers'
/// refusals in that time.
async fn announce_for(scope: &[IpAddr], trackers: &[String], wait: Duration) -> u64 {
    let (engine, _http, _dir) = engine(scope).await;
    let before = engine.refused();
    let _ = timeout(wait, engine.add_magnet(NOWHERE, trackers, &[])).await;
    settle().await;
    engine.refused().since(before).trackers
}

/// Protected runs refuse every form (at least one each); controls none.
fn assert_trackers_refused(control: bool, trackers: u64, forms: u64) {
    if control {
        assert_eq!(trackers, 0, "control: nothing refused");
    } else {
        assert!(
            trackers >= forms,
            "each form refused and counted: {trackers}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_canary_http_tracker_is_never_contacted() {
    for control in [false, true] {
        let scope = if control { vec![V4, V6] } else { vec![] };
        // One canary per form: the IPv4 literal, its mapped form, a test
        // name, `localhost` (the system resolver), the `[::1]` literal.
        let p = [V4, V4, V4, V4, V6].map(|ip| Canary::tcp(ip, 0));
        test_dns::set("canary-http.c27.test", &[V4]);
        let port = |i: usize| p[i].addr().port();
        let trackers = [
            format!("http://{}/announce", p[0].addr()),
            format!("http://[::ffff:127.0.0.1]:{}/announce", port(1)),
            format!("http://canary-http.c27.test:{}/announce", port(2)),
            format!("http://localhost:{}/announce", port(3)),
            format!("http://{}/announce", p[4].addr()),
        ];
        let refused = announce_for(&scope, &trackers, Duration::from_secs(3)).await;
        assert_canaries(control, &p.each_ref());
        assert_trackers_refused(control, refused, 5);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tracker_redirect_is_not_followed() {
    let target = Canary::tcp(V4, 0);
    let redirect = format!(
        "HTTP/1.1 302 Found\r\nLocation: http://{}/announce\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        target.addr()
    );
    let tracker = support::HttpTracker::start(redirect.into_bytes(), false);
    announce_for(
        &[V6],
        std::slice::from_ref(&tracker.url),
        Duration::from_secs(3),
    )
    .await;
    assert!(tracker.requests() >= 1, "the tracker got the announce");
    assert_eq!(target.hits(), 0, "its redirect was not followed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_http_announce_opens_a_new_connection() {
    // An answer that asks for the next announce in a second, on a
    // connection the tracker keeps open.
    let body = b"d8:intervali1e12:min intervali1e5:peers0:e";
    let mut answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend(body);
    let tracker = support::HttpTracker::start(answer, true);
    // Three announces need two seconds; five leave room on a loaded machine.
    announce_for(
        &[V6],
        std::slice::from_ref(&tracker.url),
        Duration::from_secs(5),
    )
    .await;
    let (requests, connections) = (tracker.requests(), tracker.connections());
    assert!(requests >= 3, "announces: {requests}");
    assert_eq!(connections, requests, "one connection per announce");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_canary_udp_tracker_is_never_contacted() {
    for control in [false, true] {
        let scope = if control { vec![V4, V6] } else { vec![] };
        // `udp://` is not a special scheme: an IPv4 host is a name and goes
        // through the resolver; the bracketed forms are literals.
        let q = [V4, V4, V4, V4, V6].map(|ip| Canary::udp(ip, 0));
        test_dns::set("canary-udp.c27.test", &[V4]);
        let port = |i: usize| q[i].addr().port();
        let trackers = [
            format!("udp://{}", q[0].addr()),
            format!("udp://[::ffff:127.0.0.1]:{}", port(1)),
            format!("udp://canary-udp.c27.test:{}", port(2)),
            format!("udp://localhost:{}", port(3)),
            format!("udp://{}", q[4].addr()),
        ];
        let refused = announce_for(&scope, &trackers, Duration::from_secs(3)).await;
        assert_canaries(control, &q.each_ref());
        assert_trackers_refused(control, refused, 5);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cached_udp_tracker_address_meets_the_current_snapshot() {
    use mq_engine::librqbit::dht::Id20;
    use mq_engine::librqbit::tracker_comms::{AnnounceFields, Policy, UdpTrackerClient};
    let on_link = |addr: &str| {
        Ok(vec![Interface {
            addr: addr.parse().unwrap(),
            mask: Some("ffff:ffff:ffff:ffff::".parse().unwrap()),
            loopback: false,
            point_to_point: false,
        }])
    };
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    test_dns::set_interfaces(&http, on_link("2001:db8:1:2::abcd"));
    let scope = http.torrent_scope(&[]);
    let permits = scope.clone();
    let policy = Policy {
        permits: Arc::new(move |ip| permits.permits(ip)),
        resolver: scope.resolver(),
    };
    let client = UdpTrackerClient::new(Default::default(), None, Some(policy))
        .await
        .unwrap();
    // Public by class and off-link under the first snapshot.
    let tracker: SocketAddr = "[2001:db8:1:3::1]:6969".parse().unwrap();
    let fields = || AnnounceFields {
        info_hash: Id20::new([1; 20]),
        peer_id: Id20::new([2; 20]),
        downloaded: 0,
        left: 0,
        uploaded: 0,
        event: 0,
        key: 0,
        port: 6881,
    };
    let refused = scope.refused();
    let first = timeout(
        Duration::from_millis(300),
        client.announce(tracker, fields()),
    )
    .await;
    if let Ok(answer) = first {
        let error = answer.expect_err("nothing answers there").to_string();
        assert!(!error.contains("address policy"), "{error}");
    }
    assert_eq!(scope.refused(), refused, "sent, or failed to send");
    // The tracker's prefix is on a link now: the same call is refused at
    // once, before anything is sent.
    test_dns::set_interfaces(&http, on_link("2001:db8:1:3::abcd"));
    scope.refresh();
    let second = timeout(
        Duration::from_millis(300),
        client.announce(tracker, fields()),
    )
    .await
    .expect("refused at once");
    let error = second.expect_err("refused").to_string();
    assert!(error.contains("address policy"), "{error}");
    assert_eq!(scope.refused(), refused + 1);
}

fn announce_fields() -> mq_engine::librqbit::tracker_comms::AnnounceFields {
    use mq_engine::librqbit::dht::Id20;
    mq_engine::librqbit::tracker_comms::AnnounceFields {
        info_hash: Id20::new([1; 20]),
        peer_id: Id20::new([2; 20]),
        downloaded: 0,
        left: 0,
        uploaded: 0,
        event: 0,
        key: 0,
        port: 6881,
    }
}

/// A UDP tracker client whose policy is `permits`.
async fn udp_client(
    permits: impl Fn(IpAddr) -> bool + Send + Sync + 'static,
) -> mq_engine::librqbit::tracker_comms::UdpTrackerClient {
    use mq_engine::librqbit::tracker_comms::{Policy, UdpTrackerClient};
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let policy = Policy {
        permits: Arc::new(permits),
        resolver: http.torrent_scope(&[]).resolver(),
    };
    UdpTrackerClient::new(Default::default(), None, Some(policy))
        .await
        .unwrap()
}

/// A UDP tracker on `127.0.0.1` that answers Connect and Announce (no
/// peers), from its own socket or, given one, from `answer_from`. Counts
/// both kinds of request.
struct UdpTracker {
    addr: SocketAddr,
    connects: Arc<AtomicUsize>,
    announces: Arc<AtomicUsize>,
}

fn udp_tracker(answer_from: Option<std::net::UdpSocket>) -> UdpTracker {
    let socket = std::net::UdpSocket::bind((V4, 0)).unwrap();
    let addr = socket.local_addr().unwrap();
    let (connects, announces) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (c, a) = (connects.clone(), announces.clone());
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            if n < 16 {
                continue;
            }
            // Action and transaction id sit at the same place in both.
            let (action, tid) = (&buf[8..12], &buf[12..16]);
            let mut answer = Vec::from(action);
            answer.extend(tid);
            if action == 0u32.to_be_bytes() {
                c.fetch_add(1, Ordering::SeqCst);
                answer.extend(7u64.to_be_bytes());
            } else {
                a.fetch_add(1, Ordering::SeqCst);
                answer.extend([0, 0, 7, 8]); // interval
                answer.extend([0; 8]); // leechers, seeders
            }
            let _ = answer_from
                .as_ref()
                .unwrap_or(&socket)
                .send_to(&answer, from);
        }
    });
    UdpTracker {
        addr,
        connects,
        announces,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cached_udp_connection_id_meets_the_current_policy() {
    let allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let tracker = udp_tracker(None);
    let client = udp_client({
        let allowed = allowed.clone();
        move |_| allowed.load(Ordering::SeqCst)
    })
    .await;
    client
        .announce(tracker.addr, announce_fields())
        .await
        .expect("connected and announced");
    // Refused now (a network change), with its connection id still cached.
    allowed.store(false, Ordering::SeqCst);
    let second = timeout(
        Duration::from_millis(300),
        client.announce(tracker.addr, announce_fields()),
    )
    .await
    .expect("refused at once");
    let error = second.expect_err("refused").to_string();
    assert!(error.contains("address policy"), "{error}");
    settle().await;
    let seen = (
        tracker.connects.load(Ordering::SeqCst),
        tracker.announces.load(Ordering::SeqCst),
    );
    assert_eq!(seen, (1, 1), "nothing sent on the cached id");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_udp_answer_from_another_address_is_ignored() {
    let other = std::net::UdpSocket::bind((V4, 0)).unwrap();
    let tracker = udp_tracker(Some(other));
    let client = udp_client(|_| true).await;
    let answer = timeout(
        Duration::from_millis(1500),
        client.announce(tracker.addr, announce_fields()),
    )
    .await;
    assert!(answer.is_err(), "took an answer from another address");
    assert_eq!(tracker.connects.load(Ordering::SeqCst), 1);
}

/// An HTTP tracker on `[::1]` that reads each request and then, by `answer`,
/// says nothing or streams an endless body. Records when the client hung up.
fn misbehaving_tracker(answer: bool) -> (String, Arc<std::sync::Mutex<Option<Instant>>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind((V6, 0)).unwrap();
    let url = format!("http://{}/announce", listener.local_addr().unwrap());
    let closed = Arc::new(std::sync::Mutex::new(None));
    let seen = closed.clone();
    std::thread::spawn(move || {
        let Some(Ok(mut stream)) = listener.incoming().next() else {
            return;
        };
        let mut head = Vec::new();
        let mut buf = [0u8; 1024];
        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => head.extend_from_slice(&buf[..n]),
            }
        }
        if answer {
            // Chunked with no end: no length the client could stop at.
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
            let mut chunk = b"10000\r\n".to_vec();
            chunk.extend(vec![b'x'; 0x10000]);
            chunk.extend(b"\r\n");
            while stream.write_all(&chunk).is_ok() {}
        } else {
            while matches!(stream.read(&mut buf), Ok(n) if n > 0) {}
        }
        *seen.lock().unwrap() = Some(Instant::now());
    });
    (url, closed)
}

/// Runs an add of a torrent with only `tracker` until the tracker sees the
/// client hang up or `limit` passes; how long that took.
async fn hang_up_after(
    url: String,
    closed: &std::sync::Mutex<Option<Instant>>,
    limit: Duration,
) -> Option<Duration> {
    let (engine, _http, _dir) = engine(&[V6]).await;
    let started = Instant::now();
    let add = engine.add_magnet(NOWHERE, std::slice::from_ref(&url), &[]);
    let hung_up = async {
        loop {
            if let Some(at) = *closed.lock().unwrap() {
                return at - started;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    // The add stays alive meanwhile: dropping it would end the announce.
    timeout(limit, async {
        tokio::select! {
            _ = add => None,
            took = hung_up => Some(took),
        }
    })
    .await
    .ok()
    .flatten()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stalling_tracker_is_given_up() {
    let (url, closed) = misbehaving_tracker(false);
    let took = hang_up_after(url, &closed, Duration::from_secs(40))
        .await
        .expect("the announce gave up within 40 s");
    assert!(took >= Duration::from_secs(25), "{took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_endless_tracker_answer_is_cut_off() {
    let (url, closed) = misbehaving_tracker(true);
    let took = hang_up_after(url, &closed, Duration::from_secs(10))
        .await
        .expect("the answer was cut off");
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_url_with_a_refused_literal_is_never_fetched() {
    use mq_engine::librqbit::AddTorrent;
    let canary = Canary::tcp(V4, 0);
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let scope = http.torrent_scope(&[]);
    let dir = tempfile::tempdir().unwrap();
    let opts = SessionOptions {
        dht: None,
        disable_local_service_discovery: true,
        listen: None,
        destinations: Some(mq_engine::destinations(&scope, &scope, &scope)),
        ..Default::default()
    };
    let session = Session::new_with_opts(dir.path().into(), opts)
        .await
        .unwrap();
    let url = format!("http://{}/movie.torrent", canary.addr());
    let added = session.add_torrent(AddTorrent::from_url(url), None).await;
    let error = format!("{:#}", added.err().expect("refused"));
    assert!(error.contains("address policy"), "{error}");
    settle().await;
    assert_eq!(canary.hits(), 0);
    session.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_proxy_cannot_be_combined_with_the_policy() {
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let scope = http.torrent_scope(&[]);
    let dir = tempfile::tempdir().unwrap();
    let opts = SessionOptions {
        dht: None,
        disable_local_service_discovery: true,
        listen: None,
        connect: Some(mq_engine::librqbit::ConnectionOptions {
            proxy_url: Some("socks5://127.0.0.1:1".into()),
            ..Default::default()
        }),
        destinations: Some(mq_engine::destinations(&scope, &scope, &scope)),
        ..Default::default()
    };
    let started = Session::new_with_opts(dir.path().into(), opts).await;
    let error = format!("{:#}", started.err().expect("refused"));
    assert!(error.contains("proxy"), "{error}");
}

// Phase 4: the DHT.

/// A session with the DHT only, persisted in `file` (dumped every 200 ms),
/// bootstrapping from `bootstrap`, allowed `allowed`; and its DHT scope.
async fn dht_session(
    allowed: &[IpAddr],
    bootstrap: Vec<String>,
    file: &std::path::Path,
) -> (Arc<Session>, mq_http::AddressScope) {
    use mq_engine::librqbit::DhtSessionConfig;
    use mq_engine::librqbit::dht::DhtPersistenceConfig;
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let scope = http.torrent_scope(allowed);
    let dht = scope.with_own_count();
    // The DHT as Marquee runs it: persisted, through the session.
    let opts = SessionOptions {
        dht: Some(DhtSessionConfig {
            bootstrap_addrs: Some(bootstrap),
            port: None,
            persistence: Some(DhtPersistenceConfig {
                config_filename: Some(file.into()),
                dump_interval: Some(Duration::from_millis(200)),
            }),
        }),
        disable_local_service_discovery: true,
        listen: None,
        destinations: Some(mq_engine::destinations(&scope, &scope, &dht)),
        ..Default::default()
    };
    let session = Session::new_with_opts(file.parent().unwrap().into(), opts)
        .await
        .unwrap();
    (session, dht)
}

/// The byte string under `key` in the bencoded dictionary `msg`.
fn bencode_get<'a>(msg: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    fn string(msg: &[u8], at: usize) -> Option<(&[u8], usize)> {
        let colon = at + msg.get(at..)?.iter().position(|&b| b == b':')?;
        let len: usize = std::str::from_utf8(&msg[at..colon]).ok()?.parse().ok()?;
        Some((msg.get(colon + 1..colon + 1 + len)?, colon + 1 + len))
    }
    fn skip(msg: &[u8], at: usize) -> Option<usize> {
        match *msg.get(at)? {
            b'i' => Some(at + msg[at..].iter().position(|&b| b == b'e')? + 1),
            b'l' | b'd' => {
                let mut i = at + 1;
                while *msg.get(i)? != b'e' {
                    i = skip(msg, i)?;
                }
                Some(i + 1)
            }
            _ => string(msg, at).map(|(_, end)| end),
        }
    }
    let mut i = 1;
    while *msg.get(i)? != b'e' {
        let (k, value) = string(msg, i)?;
        if k == key {
            return string(msg, value).map(|(v, _)| v);
        }
        i = skip(msg, value)?;
    }
    None
}

/// A DHT node on `[::1]` that answers every query with its id and, given
/// one, `contact` in `nodes6`. Counts the queries it got.
struct FakeNode {
    addr: SocketAddr,
    queries: Arc<AtomicUsize>,
}

fn fake_node(contact: Option<SocketAddr>) -> FakeNode {
    // Answers carry nodes of the asker's family, so the node sits in the
    // contact's: `nodes` for IPv4, `nodes6` for IPv6.
    let ip = if contact.is_some_and(|c| c.is_ipv4()) {
        V4
    } else {
        V6
    };
    let socket = std::net::UdpSocket::bind((ip, 0)).unwrap();
    let addr = socket.local_addr().unwrap();
    let queries = Arc::new(AtomicUsize::new(0));
    let counted = queries.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = socket.recv_from(&mut buf) {
            let msg = &buf[..n];
            let Some(tid) = bencode_get(msg, b"t").map(<[u8]>::to_vec) else {
                continue;
            };
            if bencode_get(msg, b"y") != Some(b"q") {
                continue;
            }
            counted.fetch_add(1, Ordering::SeqCst);
            let mut answer = b"d1:rd2:id20:".to_vec();
            answer.extend([0x11; 20]);
            match contact {
                Some(SocketAddr::V4(c)) => {
                    answer.extend(b"5:nodes26:");
                    answer.extend([0x22; 20]);
                    answer.extend(c.ip().octets());
                    answer.extend(c.port().to_be_bytes());
                }
                Some(SocketAddr::V6(c)) => {
                    answer.extend(b"6:nodes638:");
                    answer.extend([0x22; 20]);
                    answer.extend(c.ip().octets());
                    answer.extend(c.port().to_be_bytes());
                }
                None => {}
            }
            answer.extend(format!("e1:t{}:", tid.len()).bytes());
            answer.extend(&tid);
            answer.extend(b"1:y1:re");
            let _ = socket.send_to(&answer, from);
        }
    });
    FakeNode { addr, queries }
}

/// A node that never answers: an allowed bootstrap that keeps a DHT up.
fn silent_node() -> (std::net::UdpSocket, String) {
    let socket = std::net::UdpSocket::bind((V6, 0)).unwrap();
    let addr = socket.local_addr().unwrap().to_string();
    (socket, addr)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dht_never_sends_to_the_canary() {
    for control in [false, true] {
        let allowed = if control { vec![V4, V6] } else { vec![] };
        // Bootstrap nodes by IPv4 literal, mapped literal and `[::1]`.
        let canaries = [V4, V4, V6].map(|ip| Canary::udp(ip, 0));
        let bootstrap = vec![
            canaries[0].addr().to_string(),
            mapped(canaries[1].addr()).to_string(),
            canaries[2].addr().to_string(),
        ];
        let dir = tempfile::tempdir().unwrap();
        let (session, dht) = dht_session(&allowed, bootstrap, &dir.path().join("dht.json")).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_canaries(control, &canaries.each_ref());
        if control {
            assert_eq!(dht.refused(), 0, "control: nothing refused");
        } else {
            assert!(dht.refused() >= 3, "each node refused: {}", dht.refused());
        }
        session.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_source_gets_no_dht_reply() {
    for control in [false, true] {
        let allowed = if control { vec![V4, V6] } else { vec![V6] };
        let (_silent, bootstrap) = silent_node();
        let dir = tempfile::tempdir().unwrap();
        let (session, dht) =
            dht_session(&allowed, vec![bootstrap], &dir.path().join("dht.json")).await;
        let port = session.get_dht().unwrap().listen_addr().port();
        let before = dht.refused();
        let asker = tokio::net::UdpSocket::bind((V4, 0)).await.unwrap();
        let mut ping = b"d1:ad2:id20:".to_vec();
        ping.extend([0x33; 20]);
        ping.extend(b"e1:q4:ping1:t2:aa1:y1:qe");
        asker.send_to(&ping, (V4, port)).await.unwrap();
        let mut buf = [0u8; 1500];
        let answered = timeout(Duration::from_secs(1), asker.recv_from(&mut buf))
            .await
            .is_ok();
        assert_eq!(answered, control, "a reply only to an allowed source");
        if !control {
            assert!(dht.refused() > before, "the reply was refused and counted");
        }
        session.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persisted_dht_nodes_meet_the_policy() {
    // A table that holds the canary: a first session, allowed everything,
    // learns it from a node's answer (a node enters the table when it is
    // queried) and saves the table. Only the IPv4 table is loaded again.
    let canary = Canary::udp(V4, 0);
    let node = fake_node(Some(canary.addr()));
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dht.json");
    let (first, _) = dht_session(&[V4, V6], vec![node.addr.to_string()], &file).await;
    let seeded = timeout(Duration::from_secs(5), async {
        loop {
            match std::fs::read_to_string(&file) {
                Ok(json) if json.contains(&canary.addr().to_string()) => return json,
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    })
    .await
    .expect("the learned node saved");
    first.stop().await;
    for control in [false, true] {
        std::fs::write(&file, &seeded).unwrap();
        let allowed = if control { vec![V4, V6] } else { vec![V6] };
        let (_silent, bootstrap) = silent_node();
        let before = canary.hits();
        let (session, dht) = dht_session(&allowed, vec![bootstrap], &file).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        session.stop().await;
        settle().await;
        let pinged = canary.hits() - before;
        if control {
            assert!(pinged >= 1, "control: the saved node was pinged");
        } else {
            assert_eq!(pinged, 0, "the saved node was pinged");
            assert!(dht.refused() >= 1, "the saved node's ping was refused");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nodes_learned_from_answers_meet_the_policy() {
    for control in [false, true] {
        let canary = Canary::udp(V4, 0);
        // An allowed node that names the canary in its answers.
        let node = fake_node(Some(mapped(canary.addr())));
        let allowed = if control { vec![V4, V6] } else { vec![V6] };
        let dir = tempfile::tempdir().unwrap();
        let (session, dht) = dht_session(
            &allowed,
            vec![node.addr.to_string()],
            &dir.path().join("dht.json"),
        )
        .await;
        let tried = timeout(Duration::from_secs(3), async {
            while canary.hits() == 0 && dht.refused() == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        assert!(tried.is_ok(), "the learned node was tried");
        assert!(node.queries.load(Ordering::SeqCst) >= 1);
        if !control {
            // A refused query fails at once: none waits out its 10 s
            // timeout, so the lookup moves on.
            let drained = timeout(Duration::from_secs(1), async {
                while session.get_dht().unwrap().stats().outstanding_requests > 0 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
            assert!(drained.is_ok(), "a refused query stayed outstanding");
        }
        settle().await;
        assert_canaries(control, &[&canary]);
        session.stop().await;
    }
}
