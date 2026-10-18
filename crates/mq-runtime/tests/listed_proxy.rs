//! Its own test binary: it sets a proxy in the process environment before
//! any client exists (plans/m12-addon-discovery.md 3.2).

use mq_runtime::{Core, ListedEntry};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

#[tokio::test]
async fn a_listed_install_never_goes_through_a_proxy() {
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
        let body = r#"{"id":"listed","version":"1.0.0","name":"Listed","resources":[],"types":[],"catalogs":[]}"#;
        while let Ok((mut socket, _)) = origin.accept().await {
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    // SAFETY: this binary's only test changes the environment before any
    // client reads it, and the current-thread runtime runs no other thread.
    unsafe {
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
        for name in ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
            std::env::set_var(name, format!("http://{proxy_addr}"));
        }
    }
    mq_http::test_dns::set_public("proxied.m12.test", &[IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    let core = Core::temporary("p").unwrap();
    let entry = ListedEntry {
        key: Url::parse(&format!("http://proxied.m12.test:{port}/manifest.json")).unwrap(),
        id: "listed".into(),
    };
    let installed = core
        .install_listed(&entry)
        .await
        .expect("installs directly");
    assert_eq!(installed.0, "listed");
    assert_eq!(
        proxied.load(Ordering::SeqCst),
        0,
        "the install went through the proxy"
    );
    // The proxy is in effect for other public requests, so the check above
    // means something.
    let url = format!("http://proxied.m12.test:{port}/manifest.json");
    let _ = core.http().get(&url, &Default::default()).await;
    assert_eq!(proxied.load(Ordering::SeqCst), 1);
}
