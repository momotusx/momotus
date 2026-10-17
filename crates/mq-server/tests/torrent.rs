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
        .select_file(t, &FileHint::Name("movie.mkv".into()))
        .await
        .unwrap();
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http.clone()).await.unwrap();
    let url = server.register(
        Upstream::Torrent {
            engine: s.leecher.clone(),
            torrent: t,
            file,
        },
        "movie.mkv",
    );
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
