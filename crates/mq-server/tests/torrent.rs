//! A torrent played through the loopback server: metadata and pieces from a
//! seeder engine on loopback, ranges and HEAD answered from verified bytes.
#![cfg(feature = "torrent")]

#[path = "../../mq-engine/tests/support/mod.rs"]
mod support;

use std::time::Duration;

use mq_engine::FileHint;
use mq_http::{ClientConfig, HttpClient, RequestOptions};
use mq_server::{StreamServer, Upstream};

fn local() -> RequestOptions {
    RequestOptions {
        allow_private: true,
        max_bytes: 16 * 1024 * 1024,
        timeout: Duration::from_secs(60),
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serves_torrent_ranges_byte_for_byte() {
    let s = support::swarm(true).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let file = s
        .leecher
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = server.register(
        Upstream::Torrent {
            claim: s.leecher.claim(t, "default").unwrap(),
            file,
        },
        "movie.mkv",
    );
    let url = lease.url.clone();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    let len = s.movie.len() as u64;

    let head = http
        .request(mq_http::Method::HEAD, &url, None, &local())
        .await
        .unwrap();
    assert_eq!(head.status, 200);
    assert_eq!(
        head.header("content-length"),
        Some(len.to_string().as_str())
    );
    assert_eq!(head.header("content-type"), Some("video/x-matroska"));
    assert_eq!(head.header("accept-ranges"), Some("bytes"));

    // A seek, the container tail, then the whole file.
    let (a, n) = (2 * 1024 * 1024 + 99, 300_000u64);
    let mid = http.get_range(&url, a, n, &local()).await.unwrap();
    assert_eq!(mid.status, 206);
    assert_eq!(
        mid.header("content-range"),
        Some(format!("bytes {a}-{}/{len}", a + n - 1).as_str())
    );
    assert!(mid.body[..] == s.movie[a as usize..(a + n) as usize]);
    let tail = http
        .get_range(&url, len - 1000, 1000, &local())
        .await
        .unwrap();
    assert!(tail.body[..] == s.movie[s.movie.len() - 1000..]);
    let all = http.get(&url, &local()).await.unwrap();
    assert_eq!(all.status, 200);
    assert!(all.body[..] == s.movie[..], "whole file differs");
}

/// A YTS-style hev1 MP4 from a torrent (mq-media's synthetic fixture, `moov`
/// at the tail): the engine's bytes go out with the sample entry as hvc1,
/// in a range that cuts the four bytes and in a whole read.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_torrent_hev1_mp4_is_served_as_hvc1() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mp4 = std::fs::read(format!(
        "{dir}/../mq-media/tests/fixtures/hev1-moov-tail.mp4"
    ))
    .unwrap();
    let fourcc = mp4.windows(4).position(|w| w == b"hev1").unwrap();
    let mut want = mp4.clone();
    want[fourcc..fourcc + 4].copy_from_slice(b"hvc1");
    let s = support::swarm_movie("movie.mp4", mp4).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let file = s
        .leecher
        .select_file(t, "default", &FileHint::Name("movie.mp4".into()))
        .await
        .unwrap();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = server.register(
        Upstream::Torrent {
            claim: s.leecher.claim(t, "default").unwrap(),
            file,
        },
        "movie.mp4",
    );
    let at = fourcc as u64 + 2;
    let cut = http.get_range(&lease.url, at, 6, &local()).await.unwrap();
    assert!(
        cut.body[..] == want[fourcc + 2..fourcc + 8],
        "the cut range"
    );
    let all = http.get(&lease.url, &local()).await.unwrap();
    assert!(all.body[..] == want[..], "the whole file");
}

/// The scan for hev1 entries waits for pieces inside the body: an MP4
/// whose swarm left still gets its headers at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_mp4_torrent_answers_its_headers_before_the_scan() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mp4 = std::fs::read(format!(
        "{dir}/../mq-media/tests/fixtures/hev1-moov-tail.mp4"
    ))
    .unwrap();
    let s = support::swarm_movie("movie.mp4", mp4).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let seeded = s._seeder.add_magnet(&s.info_hash, &[], &[]).await.unwrap();
    s._seeder.remove(seeded, true).await.unwrap();
    let file = s
        .leecher
        .select_file(t, "default", &FileHint::Name("movie.mp4".into()))
        .await
        .unwrap();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = server.register(
        Upstream::Torrent {
            claim: s.leecher.claim(t, "default").unwrap(),
            file,
        },
        "movie.mp4",
    );
    let headers = tokio::time::timeout(
        Duration::from_secs(2),
        http.stream(&lease.url, Some("bytes=0-".into()), &local()),
    )
    .await;
    let body = headers.expect("the headers waited for the scan").unwrap();
    assert_eq!(body.status, 206);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn release_ends_work_in_flight() {
    let s = support::swarm(true).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    // The seeder leaves before a piece is asked for, so reads wait.
    let seeded = s._seeder.add_magnet(&s.info_hash, &[], &[]).await.unwrap();
    s._seeder.remove(seeded, true).await.unwrap();
    let file = s
        .leecher
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let lease = server.register(
        Upstream::Torrent {
            claim: s.leecher.claim(t, "default").unwrap(),
            file,
        },
        "movie.mkv",
    );
    let mut body = http
        .stream(&lease.url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    // A player that leaves ends its range's demand too.
    let readers = || async { s.leecher.stats(t).await.unwrap().readers };
    let left = http
        .stream(&lease.url, Some("bytes=0-".into()), &local())
        .await
        .unwrap();
    assert_eq!(readers().await, 2);
    drop(left);
    let one = tokio::time::timeout(Duration::from_secs(1), async {
        while readers().await > 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(one.is_ok(), "a range outlived its player");
    drop(lease);
    let ended = tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(Ok(_)) = body.next_chunk().await {}
    })
    .await;
    assert!(ended.is_ok(), "the body kept waiting for pieces");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stalled_player_holds_no_demand_after_release() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let s = support::swarm(true).await;
    let t = s
        .leecher
        .add_magnet(&s.info_hash, &[], &[s.seeder_addr])
        .await
        .unwrap();
    let file = s
        .leecher
        .select_file(t, "default", &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let lease = server.register(
        Upstream::Torrent {
            claim: s.leecher.claim(t, "default").unwrap(),
            file,
        },
        "movie.mkv",
    );
    // A player with a small receive buffer that reads the head and stops.
    let path = lease.url.split_once("/s/").unwrap().1;
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.set_recv_buffer_size(16 * 1024).unwrap();
    let mut player = socket.connect(server.addr()).await.unwrap();
    let request = format!(
        "GET /s/{path} HTTP/1.1\r\nHost: {}\r\nRange: bytes=0-\r\n\r\n",
        server.addr()
    );
    player.write_all(request.as_bytes()).await.unwrap();
    let mut head = [0u8; 1024];
    let n = player.read(&mut head).await.unwrap();
    assert!(head[..n].starts_with(b"HTTP/1.1 206"));
    let readers = || async { s.leecher.stats(t).await.unwrap().readers };
    // The range stays open, held back by the player.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(readers().await, 1, "the body did not stall");
    drop(lease);
    let gone = tokio::time::timeout(Duration::from_secs(1), async {
        while readers().await > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(gone.is_ok(), "the range's demand outlived its lease");
}
