//! The CLI's sync flow against a real relay: the keystore record never
//! reaches the output, and a device the account dropped forgets it.

use mq_http::{ClientConfig, HttpClient};
use mq_library::Library;
use mq_library::hlc::HlcClock;
use mq_sync::SyncConfig;
use std::path::Path;

async fn mq(data: &Path, args: &[&str]) -> (bool, String) {
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_mq"))
        .arg("--data")
        .arg(data)
        .args(args)
        .output()
        .await
        .unwrap();
    (out.status.success(), String::from_utf8(out.stdout).unwrap())
}

fn record(data: &Path) -> SyncConfig {
    serde_json::from_str(&std::fs::read_to_string(data.join("sync-dev.json")).unwrap()).unwrap()
}

/// Every secret of a record, as it is written out.
fn secrets(record: &SyncConfig) -> Vec<String> {
    let text = |value: serde_json::Value| value.as_str().unwrap().to_owned();
    vec![
        record.device_token.clone(),
        text(serde_json::to_value(record.member).unwrap()),
        text(serde_json::to_value(record.keys.ak).unwrap()),
        text(serde_json::to_value(record.keys.id_key).unwrap()),
    ]
}

#[tokio::test]
async fn sync_output_holds_no_secrets_and_a_dropped_device_forgets_its_record() {
    let dir = std::env::temp_dir().join(format!("mq-cli-sync-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let data = dir.join("cli");
    let (ok, out) = mq(&data, &["sync", "init", &server, "--name", "CLI"]).await;
    assert!(ok, "{out}");
    let first = record(&data);

    // A TV joins (the CLI device vouches for it) and rotates the key: the
    // CLI's next sync takes the new key, a changed record it must not print.
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let (scratch, clock) = (Library::temporary().unwrap(), HlcClock::new(9));
    let joining = mq_sync::pair::start_pairing(&http, &server).await.unwrap();
    mq_sync::pair::approve_pairing(&scratch, &http, &first, &clock, &joining.qr)
        .await
        .unwrap();
    let offer = joining.poll(&http).await.unwrap().expect("approved");
    let tv = joining.claim(&http, offer, "TV").await.unwrap();
    let tv_library = Library::temporary().unwrap();
    let tv = mq_sync::remove_devices(&tv_library, &http, &tv, &clock, vec![])
        .await
        .unwrap();
    let (ok, out) = mq(&data, &["sync", "now"]).await;
    assert!(ok, "{out}");
    assert!(out.contains("\"recordChanged\":true"), "{out}");
    let adopted = record(&data);
    assert_ne!(adopted.keys.ak, first.keys.ak);
    for secret in secrets(&first).into_iter().chain(secrets(&adopted)) {
        assert!(!out.contains(&secret), "a secret in the output: {out}");
    }

    // The TV removes the CLI device: its next sync says so and forgets the
    // record.
    let cli = first.secret().id();
    mq_sync::remove_devices(&tv_library, &http, &tv, &clock, vec![cli])
        .await
        .unwrap();
    let (ok, out) = mq(&data, &["sync", "now"]).await;
    assert!(!ok && out.contains("(dropped)"), "{out}");
    assert!(!data.join("sync-dev.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
