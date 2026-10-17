//! Development sync flow. The keystore record lives in a 0600 file in the
//! data dir; apps keep it in the OS keystore instead. It never goes to the
//! output: it holds the device's keys and token.

use mq_http::{ClientConfig, HttpClient};
use mq_runtime::Core;
use mq_sync::roster::Id;
use mq_sync::{SyncConfig, SyncError};
use serde_json::json;
use std::path::Path;
use std::time::Instant;

const KEY_FILE: &str = "sync-dev.json";

fn http() -> Result<HttpClient, String> {
    HttpClient::new(ClientConfig::default()).map_err(|e| e.to_string())
}

fn id(text: &str) -> Result<Id, String> {
    serde_json::from_value(json!(text)).map_err(|_| format!("not a member id: {text}"))
}

/// A sync error as printed; a device the account dropped also forgets its
/// record (the library stays, as an app's would).
fn failed(data: &Path) -> impl Fn(SyncError) -> String + '_ {
    move |e| {
        if matches!(e, SyncError::Dropped) {
            let _ = std::fs::remove_file(data.join(KEY_FILE));
        }
        match e.code() {
            Some(code) => format!("{e} ({code})"),
            None => e.to_string(),
        }
    }
}

pub async fn init(
    data: &Path,
    server: &str,
    invite: Option<&str>,
    name: &str,
) -> Result<(), String> {
    let record = mq_sync::create_account(&http()?, server, invite, name)
        .await
        .map_err(failed(data))?;
    save(data, &record)?;
    super::emit(json!({ "event": "sync_account", "account": record.keys.account }));
    Ok(())
}

/// Sync rounds until the record settles: a round that changed it (a new key
/// or roster) stores it and runs again.
pub async fn now(core: &Core, data: &Path) -> Result<(), String> {
    let mut record = read(data)?;
    for _ in 0..3 {
        let started = Instant::now();
        let mut report = core.sync(&record).await.map_err(failed(data))?;
        let next = report.record.take();
        super::emit(json!({
            "event": "synced",
            "report": report,
            "recordChanged": next.is_some(),
            "ms": started.elapsed().as_millis() as u64,
        }));
        let Some(next) = next else {
            return Ok(());
        };
        save(data, &next)?;
        record = next;
    }
    Ok(())
}

fn read(data: &Path) -> Result<SyncConfig, String> {
    let path = data.join(KEY_FILE);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("no sync setup at {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Written to a fresh 0600 file and renamed over the old one, so the record
/// is never readable by others and never half written.
fn save(data: &Path, record: &SyncConfig) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let path = data.join(KEY_FILE);
    let temp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&temp);
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    out.write_all(
        serde_json::to_string_pretty(record)
            .expect("plain struct")
            .as_bytes(),
    )
    .and_then(|()| out.sync_all())
    .map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| e.to_string())
}

/// New device: prints the pairing code, waits for approval, joins after the
/// user confirms the codes match.
pub async fn pair(data: &Path, server: &str, name: &str) -> Result<(), String> {
    let http = http()?;
    let request = mq_sync::pair::start_pairing(&http, server)
        .await
        .map_err(failed(data))?;
    super::emit(json!({ "event": "pairing", "qr": request.qr, "expiresMs": request.expires_ms }));
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if mq_library::hlc::now_ms() >= request.expires_ms {
            return Err("the pairing code expired".into());
        }
        if let Some(offer) = request.poll(&http).await.map_err(failed(data))? {
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
            let record = request
                .claim(&http, offer, name)
                .await
                .map_err(failed(data))?;
            save(data, &record)?;
            super::emit(json!({ "event": "paired" }));
            return Ok(());
        }
    }
}

/// Device that syncs: vouches for the device showing `qr`.
pub async fn approve(core: &Core, data: &Path, qr: &str) -> Result<(), String> {
    let record = read(data)?;
    let (code, record) =
        mq_sync::pair::approve_pairing(core.library(), core.http(), &record, core.clock(), qr)
            .await
            .map_err(failed(data))?;
    save(data, &record)?;
    super::emit(json!({ "event": "approved", "code": code }));
    Ok(())
}

/// Makes a recovery kit and prints it once.
pub async fn kit(core: &Core, data: &Path) -> Result<(), String> {
    let record = read(data)?;
    let (kit, record) = mq_sync::pair::make_kit(core.library(), core.http(), &record, core.clock())
        .await
        .map_err(failed(data))?;
    save(data, &record)?;
    super::emit(json!({ "event": "recovery_kit", "kit": kit }));
    Ok(())
}

/// Joins the account of a recovery kit as a new device.
pub async fn recover(data: &Path, server: &str, kit: &str, name: &str) -> Result<(), String> {
    let record = mq_sync::pair::recover(&http()?, server, kit, name)
        .await
        .map_err(failed(data))?;
    save(data, &record)?;
    super::emit(json!({ "event": "recovered" }));
    Ok(())
}

pub async fn devices(core: &Core, data: &Path) -> Result<(), String> {
    let record = read(data)?;
    let mut devices = mq_sync::devices(core.library(), core.http(), &record, core.clock())
        .await
        .map_err(failed(data))?;
    if let Some(record) = devices.record.take() {
        save(data, &record)?;
    }
    super::emit(json!({ "event": "devices", "devices": devices }));
    Ok(())
}

pub async fn remove(core: &Core, data: &Path, member: &str) -> Result<(), String> {
    let record = read(data)?;
    let record = mq_sync::remove_devices(
        core.library(),
        core.http(),
        &record,
        core.clock(),
        vec![id(member)?],
    )
    .await
    .map_err(failed(data))?;
    save(data, &record)?;
    super::emit(json!({ "event": "removed", "epoch": record.keys.epoch }));
    Ok(())
}

pub async fn lost(core: &Core, data: &Path, member: &str, keep: &[String]) -> Result<(), String> {
    let record = read(data)?;
    let lost = id(member)?;
    let keep = keep.iter().map(|k| id(k)).collect::<Result<Vec<_>, _>>()?;
    let preview = mq_sync::lost_preview(core.library(), core.http(), &record, core.clock(), lost)
        .await
        .map_err(failed(data))?;
    let record = mq_sync::report_lost(
        core.library(),
        core.http(),
        &record,
        core.clock(),
        lost,
        keep,
    )
    .await
    .map_err(failed(data))?;
    save(data, &record)?;
    super::emit(json!({ "event": "lost", "listed": preview, "epoch": record.keys.epoch }));
    Ok(())
}

pub async fn sign_out(core: &Core, data: &Path, force: bool) -> Result<(), String> {
    let record = read(data)?;
    mq_sync::sign_out(core.library(), core.http(), &record, core.clock(), force)
        .await
        .map_err(failed(data))?;
    std::fs::remove_file(data.join(KEY_FILE)).map_err(|e| e.to_string())?;
    super::emit(json!({ "event": "signed_out" }));
    Ok(())
}

pub async fn use_server_roster(core: &Core, data: &Path, drop: &[String]) -> Result<(), String> {
    let record = read(data)?;
    let drop = drop.iter().map(|d| id(d)).collect::<Result<Vec<_>, _>>()?;
    let record =
        mq_sync::use_server_roster(core.library(), core.http(), &record, core.clock(), drop)
            .await
            .map_err(failed(data))?;
    save(data, &record)?;
    super::emit(json!({ "event": "adopted", "epoch": record.keys.epoch }));
    Ok(())
}
