//! A session refreshes installed addon manifests when it starts and when
//! the app comes to the foreground (C30).

use mq_bridge::{Output, Session};
use mq_runtime::Core;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves the manifest the test puts in; returns its link.
async fn origin(manifest: Arc<Mutex<String>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/manifest.json", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let body = manifest.lock().unwrap().clone();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    url
}

fn manifest(types: &[&str]) -> String {
    json!({
        "id": "a", "version": "1.0.0", "name": "A",
        "resources": ["stream"], "types": types, "catalogs": []
    })
    .to_string()
}

/// Waits up to five seconds for the stored manifest to name `types`.
async fn stored_types_become(core: &Core, types: &[&str]) -> bool {
    for _ in 0..100 {
        let rows = core.library().addons(core.profile()).unwrap();
        let stored: serde_json::Value = serde_json::from_str(&rows[0].manifest_json).unwrap();
        if stored["types"] == json!(types) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn a_session_refreshes_manifests_at_start_and_on_foreground() {
    let served = Arc::new(Mutex::new(manifest(&["movie"])));
    let url = origin(served.clone()).await;
    let core = Core::temporary("default").unwrap();
    core.install_addon(&url).await.unwrap();

    *served.lock().unwrap() = manifest(&["movie", "series"]);
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(core.clone(), tx);
    assert!(
        stored_types_become(&core, &["movie", "series"]).await,
        "the session start refreshed the manifest"
    );

    // A day has passed since that refresh.
    core.library()
        .set_device_value("addons_refreshed_ms", "0")
        .unwrap();
    *served.lock().unwrap() = manifest(&["series"]);
    session
        .handle_text(&json!({"id": 1, "op": "lifecycle", "event": "foreground"}).to_string())
        .await;
    assert_eq!(rx.recv().await.unwrap().json()["ok"], true);
    assert!(
        stored_types_become(&core, &["series"]).await,
        "coming to the foreground refreshed the manifest"
    );
}
