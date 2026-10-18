//! Its own test binary: it changes the process environment before any client
//! exists.

use mq_http::{ClientConfig, HttpClient, RequestOptions, test_dns};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn a_proxy_from_the_environment_is_never_used() {
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy.local_addr().unwrap();
    let proxied = Arc::new(AtomicUsize::new(0));
    let count = proxied.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = proxy.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            drop(socket);
        }
    });
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = origin.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = origin.accept().await {
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await;
        }
    });
    // SAFETY: this binary's only test changes the environment before any
    // client reads it, and the current-thread runtime runs no other thread.
    unsafe {
        // A NO_PROXY from the shell (often `localhost,127.0.0.1`) would let
        // the request go direct with or without the client's own rule.
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
        for name in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
            std::env::set_var(name, format!("http://{proxy_addr}"));
        }
    }
    // A name, so no loopback exception in the proxy rules can apply.
    test_dns::set("proxied.c23.test", &[IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    let http = HttpClient::new(ClientConfig::default())
        .unwrap()
        .scoped(&[Ipv4Addr::LOCALHOST.into()]);
    let opts = RequestOptions {
        timeout: Duration::from_secs(5),
        ..Default::default()
    };
    let url = format!("http://proxied.c23.test:{port}/");
    let ok = tokio::time::timeout(Duration::from_secs(5), http.get(&url, &opts))
        .await
        .expect("finishes within 5 s")
        .expect("reaches the origin directly");
    assert_eq!(&ok.body[..], b"ok");
    assert_eq!(proxied.load(Ordering::SeqCst), 0);

    // The listed path (`no_proxy`) goes direct too, while an unscoped public
    // request without it takes the proxy from the environment. The name
    // stands for a public origin (`set_public`).
    test_dns::set_public("listed.m12.test", &[IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    let unscoped = HttpClient::new(ClientConfig::default()).unwrap();
    let url = format!("http://listed.m12.test:{port}/");
    let listed = RequestOptions {
        no_proxy: true,
        ..opts.clone()
    };
    let ok = tokio::time::timeout(Duration::from_secs(5), unscoped.get(&url, &listed))
        .await
        .expect("finishes within 5 s")
        .expect("reaches the origin directly");
    assert_eq!(&ok.body[..], b"ok");
    assert_eq!(
        proxied.load(Ordering::SeqCst),
        0,
        "the listed path used the proxy"
    );
    let _ = tokio::time::timeout(Duration::from_secs(5), unscoped.get(&url, &opts)).await;
    assert_eq!(
        proxied.load(Ordering::SeqCst),
        1,
        "the environment's proxy was not in effect, so the check above proves nothing"
    );
}
