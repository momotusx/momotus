//! Tracker announces never go through a proxy the environment names
//! (plans/c27-torrent-policy.md 4.2, phase 3): a proxy would resolve the
//! tracker's name itself, outside the address policy. A binary of its own,
//! since it sets the process environment.

#[allow(dead_code)]
mod support;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use mq_engine::{Engine, EngineConfig};
use mq_http::test_dns::Canary;

#[test]
fn trackers_ignore_proxy_variables() {
    let proxy = Canary::tcp(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let url = format!("http://{}", proxy.addr());
    // SAFETY: no other thread runs yet; the runtime starts below.
    unsafe {
        for key in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
            std::env::set_var(key, &url);
        }
        for key in ["NO_PROXY", "no_proxy"] {
            std::env::remove_var(key);
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let tracker = support::HttpTracker::start(answer.to_vec(), false);
        let dir = tempfile::tempdir().unwrap();
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let config = EngineConfig {
            discovery: false,
            ..EngineConfig::new(
                dir.path(),
                http.torrent_scope(&[Ipv6Addr::LOCALHOST.into()]),
            )
        };
        let engine = Engine::start(config).await.unwrap();
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let _ = tokio::time::timeout(
            Duration::from_secs(3),
            engine.add_magnet(hash, std::slice::from_ref(&tracker.url), &[]),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(tracker.requests() >= 1, "the tracker got the announce");
        assert_eq!(proxy.hits(), 0, "never through the proxy");
    });
}
