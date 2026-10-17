//! Development sync flow. The account key and device token live in a 0600
//! file in the data dir; apps keep them in the OS keystore instead, and join
//! new devices by QR (phase 2) rather than by copying this file.

use mq_http::{ClientConfig, HttpClient};
use mq_runtime::Core;
use mq_sync::SyncConfig;
use mq_sync::seal::AccountKey;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

const KEY_FILE: &str = "sync-dev.json";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyFile {
    server: String,
    device_token: String,
    account_key: String,
}

pub async fn init(data: &Path, server: &str, invite: Option<&str>) -> Result<(), String> {
    let http = HttpClient::new(ClientConfig::default()).map_err(|e| e.to_string())?;
    let account = mq_sync::create_account(&http, server, invite)
        .await
        .map_err(|e| e.to_string())?;
    let file = KeyFile {
        server: server.to_owned(),
        device_token: account.device_token,
        account_key: hex::encode(AccountKey::generate()),
    };
    write(&data.join(KEY_FILE), &file)?;
    super::emit(json!({ "event": "sync_account", "account": account.account }));
    Ok(())
}

/// Phase 1 only: shares the first device's token and key.
pub fn join(data: &Path, from: &Path) -> Result<(), String> {
    let file = read(&from.join(KEY_FILE))?;
    std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
    write(&data.join(KEY_FILE), &file)?;
    super::emit(json!({ "event": "sync_joined", "server": file.server }));
    Ok(())
}

pub async fn now(core: &Core, data: &Path) -> Result<(), String> {
    let file = read(&data.join(KEY_FILE))?;
    let account_key: [u8; 32] = hex::decode(&file.account_key)
        .ok()
        .and_then(|k| k.try_into().ok())
        .ok_or("the key file holds no valid account key")?;
    let config = SyncConfig {
        server: file.server,
        device_token: file.device_token,
        account_key,
    };
    let started = Instant::now();
    let report = core.sync(&config).await.map_err(|e| e.to_string())?;
    super::emit(
        json!({ "event": "synced", "report": report, "ms": started.elapsed().as_millis() as u64 }),
    );
    Ok(())
}

fn read(path: &Path) -> Result<KeyFile, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("no sync setup at {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn write(path: &PathBuf, file: &KeyFile) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    out.write_all(
        serde_json::to_string_pretty(file)
            .expect("plain struct")
            .as_bytes(),
    )
    .map_err(|e| e.to_string())
}
