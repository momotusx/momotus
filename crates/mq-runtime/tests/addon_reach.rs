//! An addon's private reach is its own host, read from its link, as for its
//! playbacks (C28): a bit a synced row carries grants nothing.

use mq_addons::AddonError;
use mq_http::test_dns;
use mq_library::AddonRow;
use mq_protocol::ResourcePath;
use mq_runtime::Core;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const MANIFEST: &str = r#"{"id":"reach","version":"1.0.0","name":"Reach","types":["movie"],"resources":["meta"],"catalogs":[],"idPrefixes":["tt"]}"#;
const META: &str = r#"{"meta":{"id":"tt1","type":"movie","name":"Film"}}"#;

/// A server on `bind` that counts its connections; `/manifest.json` and the
/// meta answer, or a redirect to `redirect` for the meta when given.
async fn server(bind: &str, redirect: Option<String>) -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind(bind).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = accepted.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let redirect = redirect.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let reply = match (head.contains("/manifest.json"), redirect) {
                    (true, _) => ok(MANIFEST),
                    (false, Some(to)) => format!(
                        "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    ),
                    (false, None) => ok(META),
                };
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (port, accepted)
}

fn ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn meta_path() -> ResourcePath {
    ResourcePath {
        resource: "meta".into(),
        r#type: "movie".into(),
        id: "tt1".into(),
        extra: vec![],
    }
}

#[tokio::test]
async fn a_synced_bit_grants_an_addon_no_private_reach() {
    // The canary sits on loopback; a public-looking name leads to it.
    let (port, canary) = server("127.0.0.1:0", None).await;
    test_dns::set("forged-addon.test", &["127.0.0.1".parse().unwrap()]);
    let core = Core::temporary("p").unwrap();
    // As sync could deliver it: a public link with the private bit set.
    let row = AddonRow {
        transport_url: format!("http://forged-addon.test:{port}/manifest.json"),
        manifest_json: MANIFEST.into(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    core.library()
        .set_addons("p", &[row], core.clock())
        .unwrap();
    let addons = core.installed_addons().unwrap();
    let fetched = core
        .addons()
        .fetch(&addons[0], &meta_path(), Duration::from_secs(5))
        .await;
    let Err(refused) = fetched else {
        panic!("fetched")
    };
    match refused {
        AddonError::Http(e) => assert!(
            e.to_string().contains("resolves only to private addresses"),
            "{e}"
        ),
        other => panic!("{other}"),
    }
    assert_eq!(
        canary.load(Ordering::SeqCst),
        0,
        "nothing reached the canary"
    );
    assert!(core.fetch_meta("movie", "tt1").await.is_err());
    assert_eq!(canary.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_local_install_still_reaches_its_own_host() {
    let (port, served) = server("127.0.0.1:0", None).await;
    let core = Core::temporary("p").unwrap();
    core.install_addon(&format!("http://127.0.0.1:{port}/manifest.json"))
        .await
        .unwrap();
    let (meta, _) = core.fetch_meta("movie", "tt1").await.unwrap();
    assert_eq!(meta.preview.name, "Film");
    assert_eq!(
        served.load(Ordering::SeqCst),
        2,
        "the manifest and the meta"
    );
}

#[tokio::test]
async fn a_local_install_reaches_no_other_private_host() {
    // The local addon sends its meta on to another loopback address.
    let (canary_port, canary) = server("[::1]:0", None).await;
    let to = format!("http://[::1]:{canary_port}/meta/movie/tt1.json");
    let (port, _) = server("127.0.0.1:0", Some(to)).await;
    let core = Core::temporary("p").unwrap();
    core.install_addon(&format!("http://127.0.0.1:{port}/manifest.json"))
        .await
        .unwrap();
    let addons = core.installed_addons().unwrap();
    let fetched = core
        .addons()
        .fetch(&addons[0], &meta_path(), Duration::from_secs(5))
        .await;
    let Err(refused) = fetched else {
        panic!("fetched")
    };
    match refused {
        AddonError::Http(e) => assert_eq!(e.kind(), "blocked address", "{e}"),
        other => panic!("{other}"),
    }
    assert_eq!(
        canary.load(Ordering::SeqCst),
        0,
        "nothing reached the canary"
    );
}
