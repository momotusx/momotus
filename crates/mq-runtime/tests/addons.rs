//! Installed addon manifests change in place (C30): installing an installed
//! link again, and the daily refresh.

use mq_runtime::Core;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DAY: u64 = 24 * 60 * 60 * 1000;

/// An addon origin whose manifest the test sets (`None` answers 500), and
/// how many requests it got.
struct Origin {
    base: String,
    manifest: Arc<Mutex<Option<String>>>,
    asked: Arc<AtomicUsize>,
}

impl Origin {
    async fn start() -> Origin {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let manifest = Arc::new(Mutex::new(None::<String>));
        let asked = Arc::new(AtomicUsize::new(0));
        let (served, count) = (manifest.clone(), asked.clone());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                count.fetch_add(1, Ordering::SeqCst);
                let body = served.lock().unwrap().clone();
                let reply = match body {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    None => {
                        "HTTP/1.1 500 X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
                    }
                };
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        Origin {
            base,
            manifest,
            asked,
        }
    }

    /// Serves a manifest of `id` with `types`.
    fn serve(&self, id: &str, types: &[&str]) {
        let manifest = serde_json::json!({
            "id": id, "version": "1.0.0", "name": id,
            "resources": ["stream"], "types": types, "catalogs": []
        });
        *self.manifest.lock().unwrap() = Some(manifest.to_string());
    }

    fn fail(&self) {
        *self.manifest.lock().unwrap() = None;
    }

    fn url(&self, name: &str) -> String {
        format!("{}/{name}/manifest.json", self.base)
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

/// The stored manifest's types of the row at `url`, with its position in
/// the list and whether it is on.
fn row(core: &Core, url: &str) -> (usize, bool, Vec<String>) {
    let rows = core.library().addons(core.profile()).unwrap();
    let at = rows.iter().position(|r| r.transport_url == url).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&rows[at].manifest_json).unwrap();
    let types = manifest["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_owned())
        .collect();
    (at, rows[at].enabled, types)
}

#[tokio::test]
async fn installing_an_installed_link_replaces_its_manifest_in_place() {
    let a = Origin::start().await;
    let b = Origin::start().await;
    let core = Core::temporary("p").unwrap();
    a.serve("a", &["movie"]);
    b.serve("b", &["movie"]);
    let (url_a, url_b) = (a.url("a"), b.url("b"));
    core.install_addon(&url_a).await.unwrap();
    core.install_addon(&url_b).await.unwrap();
    core.set_addon_enabled(&url_a, false).unwrap();
    // The addon now serves series too; installing its link again takes them.
    a.serve("a", &["movie", "series"]);
    let again = core.install_addon(&url_a).await.unwrap();
    assert_eq!(again.manifest.types, ["movie", "series"]);
    assert_eq!(core.library().addons("p").unwrap().len(), 2);
    assert_eq!(
        row(&core, &url_a),
        (0, false, vec!["movie".into(), "series".into()])
    );
    assert_eq!(row(&core, &url_b), (1, true, vec!["movie".into()]));
}

#[tokio::test]
async fn installed_manifests_refresh_at_most_once_a_day() {
    let origin = Origin::start().await;
    let core = Core::temporary("p").unwrap();
    let owner = core.for_profile("default");
    origin.serve("a", &["movie"]);
    let url = origin.url("a");
    core.install_addon(&url).await.unwrap();
    owner.install_addon(&url).await.unwrap();
    owner.set_addon_enabled(&url, false).unwrap();
    origin.serve("a", &["movie", "series"]);
    let now = 1_000_000;
    let before = origin.asked();
    // One fetch for the link both profiles installed; each row changes in
    // place.
    assert_eq!(core.refresh_addons(now).await.unwrap(), 2);
    assert_eq!(origin.asked(), before + 1);
    assert_eq!(
        row(&core, &url),
        (0, true, vec!["movie".into(), "series".into()])
    );
    assert_eq!(
        row(&owner, &url),
        (0, false, vec!["movie".into(), "series".into()])
    );
    // Within the day: nothing is asked.
    origin.serve("a", &["series"]);
    assert_eq!(core.refresh_addons(now + DAY - 1).await.unwrap(), 0);
    assert_eq!(origin.asked(), before + 1);
    // A day later it is.
    assert_eq!(core.refresh_addons(now + DAY).await.unwrap(), 2);
    assert_eq!(row(&core, &url).2, ["series"]);
}

#[tokio::test]
async fn a_refresh_that_reaches_no_addon_tries_again() {
    let origin = Origin::start().await;
    let core = Core::temporary("p").unwrap();
    origin.serve("a", &["movie"]);
    let url = origin.url("a");
    core.install_addon(&url).await.unwrap();
    origin.fail();
    assert_eq!(core.refresh_addons(1_000_000).await.unwrap(), 0);
    assert_eq!(
        row(&core, &url).2,
        ["movie"],
        "a failed fetch kept its manifest"
    );
    // Not counted as the day's refresh: an hour later it runs.
    origin.serve("a", &["series"]);
    assert_eq!(core.refresh_addons(1_000_000 + 3_600_000).await.unwrap(), 1);
    assert_eq!(row(&core, &url).2, ["series"]);
}

#[tokio::test]
async fn an_unchanged_manifest_writes_nothing() {
    let origin = Origin::start().await;
    let core = Core::temporary("p").unwrap();
    origin.serve("a", &["movie"]);
    core.install_addon(&origin.url("a")).await.unwrap();
    let queued = core.library().outbox(usize::MAX).unwrap().last_id;
    assert_eq!(core.refresh_addons(1_000_000).await.unwrap(), 0);
    assert_eq!(core.library().outbox(usize::MAX).unwrap().last_id, queued);
}
