//! Development sync flow. The key bundle and device token live in a 0600
//! file in the data dir; apps keep them in the OS keystore instead, and join
//! new devices by QR (phase 2) rather than by reading another data dir.

use mq_http::{ClientConfig, HttpClient};
use mq_runtime::Core;
use mq_sync::SyncConfig;
use mq_sync::seal::KeyBundle;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use std::time::Instant;

const KEY_FILE: &str = "sync-dev.json";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyFile {
    server: String,
    device_token: String,
    keys: KeyBundle,
}

fn http() -> Result<HttpClient, String> {
    HttpClient::new(ClientConfig::default()).map_err(|e| e.to_string())
}

pub async fn init(data: &Path, server: &str, invite: Option<&str>) -> Result<(), String> {
    let account = mq_sync::create_account(&http()?, server, invite)
        .await
        .map_err(|e| e.to_string())?;
    let file = KeyFile {
        server: server.to_owned(),
        device_token: account.device_token,
        keys: KeyBundle::generate(),
    };
    write(&data.join(KEY_FILE), &file)?;
    super::emit(json!({ "event": "sync_account", "account": account.account }));
    Ok(())
}

/// Phase 1 stand-in for QR pairing: reads the other data dir's keys and asks
/// the relay for this device's own token.
pub async fn join(data: &Path, from: &Path) -> Result<(), String> {
    let other = read(&from.join(KEY_FILE))?;
    let device = mq_sync::add_device(&http()?, &other.server, &other.device_token)
        .await
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let file = KeyFile {
        server: other.server,
        device_token: device.device_token,
        keys: other.keys,
    };
    write(&data.join(KEY_FILE), &file)?;
    super::emit(json!({ "event": "sync_joined", "account": device.account }));
    Ok(())
}

pub async fn now(core: &Core, data: &Path) -> Result<(), String> {
    let file = read(&data.join(KEY_FILE))?;
    let config = SyncConfig {
        server: file.server,
        device_token: file.device_token,
        keys: file.keys,
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

/// Written to a fresh 0600 file and renamed over the old one, so the key is
/// never readable by others and never half written.
fn write(path: &Path, file: &KeyFile) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let temp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&temp);
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    out.write_all(
        serde_json::to_string_pretty(file)
            .expect("plain struct")
            .as_bytes(),
    )
    .and_then(|()| out.sync_all())
    .map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| e.to_string())
}

/// New device: prints the pairing code, waits for approval, keeps the keys.
pub async fn pair(data: &Path, server: &str) -> Result<(), String> {
    let http = http()?;
    let request = mq_sync::pair::start_pairing(&http, server)
        .await
        .map_err(|e| e.to_string())?;
    super::emit(json!({ "event": "pairing", "qr": request.qr, "expiresMs": request.expires_ms }));
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if mq_library::hlc::now_ms() >= request.expires_ms {
            return Err("the pairing code expired".into());
        }
        if let Some(offer) = request.poll(&http).await.map_err(|e| e.to_string())? {
            // Nothing is kept until the user confirms the other device shows
            // the same code.
            super::emit(json!({ "event": "confirm", "code": offer.code }));
            let mut answer = String::new();
            std::io::stdin()
                .read_line(&mut answer)
                .map_err(|e| e.to_string())?;
            if !answer.trim().eq_ignore_ascii_case("y") {
                return Err("the codes did not match; nothing was kept".into());
            }
            let joined = offer.joined;
            std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
            write(
                &data.join(KEY_FILE),
                &KeyFile {
                    server: joined.server,
                    device_token: joined.device_token,
                    keys: joined.keys,
                },
            )?;
            super::emit(json!({ "event": "paired" }));
            return Ok(());
        }
    }
}

/// Device that syncs: approves the device showing `qr`.
pub async fn approve(data: &Path, qr: &str) -> Result<(), String> {
    let config = config(data)?;
    let code = mq_sync::pair::approve_pairing(&http()?, &config, qr)
        .await
        .map_err(|e| e.to_string())?;
    super::emit(json!({ "event": "approved", "code": code }));
    Ok(())
}

/// Makes a recovery kit and prints it once.
pub async fn kit(data: &Path) -> Result<(), String> {
    let config = config(data)?;
    let kit = mq_sync::pair::new_recovery_kit();
    mq_sync::pair::store_recovery(&http()?, &config, &kit)
        .await
        .map_err(|e| e.to_string())?;
    super::emit(json!({ "event": "recovery_kit", "kit": kit }));
    Ok(())
}

/// Joins the account of a recovery kit as a new device.
pub async fn recover(data: &Path, server: &str, kit: &str) -> Result<(), String> {
    let joined = mq_sync::pair::recover(&http()?, server, kit)
        .await
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
    write(
        &data.join(KEY_FILE),
        &KeyFile {
            server: joined.server,
            device_token: joined.device_token,
            keys: joined.keys,
        },
    )?;
    super::emit(json!({ "event": "recovered" }));
    Ok(())
}

fn config(data: &Path) -> Result<SyncConfig, String> {
    let file = read(&data.join(KEY_FILE))?;
    Ok(SyncConfig {
        server: file.server,
        device_token: file.device_token,
        keys: file.keys,
    })
}
