//! One sync round: push local registers past the push cursor, then pull and
//! apply everything past the pull cursor. Cursors live in the library's
//! `sync_state`, so a crash at any point only repeats work: pushes are
//! upserts and applies are idempotent.

use crate::seal::{AccountKey, SealError};
use crate::wire::{AccountResponse, PAGE, PullResponse, PushRequest, PushResponse, WireRecord};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use mq_http::{HttpClient, HttpError, Priority, RequestOptions};
use mq_library::hlc::{Hlc, HlcClock, now_ms};
use mq_library::{Library, LibraryError, SyncCursor, SyncRecord};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const PUSH_CURSOR: &str = "push_cursor";
const PULL_SEQ: &str = "pull_seq";

/// Where and as whom to sync. The token and key come from the platform
/// keystore and are never logged.
#[derive(Clone)]
pub struct SyncConfig {
    pub server: String,
    pub device_token: String,
    pub account_key: [u8; 32],
}

impl std::fmt::Debug for SyncConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncConfig")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub pushed: u32,
    /// The relay already had these or newer.
    pub stale: u32,
    pub pulled: u32,
    /// Pulled registers that changed local state.
    pub applied: u32,
    /// Pulled records that did not open with this account key.
    pub unreadable: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error("the relay refused a record: {0}")]
    Rejected(String),
}

/// The plaintext inside a sealed body. The key travels inside so the reader
/// can check it against the blind id the body was filed under.
#[derive(Serialize, Deserialize)]
struct Payload {
    k: String,
    v: serde_json::Value,
}

pub async fn sync_once(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<SyncReport, SyncError> {
    let key = AccountKey::from_bytes(&config.account_key);
    let mut report = SyncReport::default();
    push(library, http, config, &key, clock, &mut report).await?;
    pull(library, http, config, &key, clock, &mut report).await?;
    Ok(report)
}

/// Creates an account on the relay and returns this device's token. The
/// caller generates the account key and keeps both in the keystore.
pub async fn create_account(
    http: &HttpClient,
    server: &str,
    invite: Option<&str>,
) -> Result<AccountResponse, SyncError> {
    let mut options = options(&SyncConfig {
        server: server.to_owned(),
        device_token: String::new(),
        account_key: [0; 32],
    });
    options.headers = invite
        .map(|i| vec![("x-invite".to_owned(), i.to_owned())])
        .unwrap_or_default();
    Ok(http
        .post_json(
            &format!("{server}/v1/accounts"),
            &serde_json::json!({}),
            &options,
        )
        .await?
        .json()?)
}

/// Only the push half, for when the app goes to the background.
pub async fn push_changes(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    let key = AccountKey::from_bytes(&config.account_key);
    push(library, http, config, &key, clock, &mut report).await?;
    Ok(report)
}

/// Only the pull half, for when the relay says something changed.
pub async fn pull_changes(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    let key = AccountKey::from_bytes(&config.account_key);
    pull(library, http, config, &key, clock, &mut report).await?;
    Ok(report)
}

fn options(config: &SyncConfig) -> RequestOptions {
    let allow_private = url::Url::parse(&config.server)
        .ok()
        .and_then(|u| u.host_str().map(mq_http::is_private_host))
        .unwrap_or(false);
    RequestOptions {
        priority: Priority::Addon,
        timeout: Duration::from_secs(30),
        max_bytes: 16 * 1024 * 1024,
        allow_private,
        headers: vec![(
            "authorization".into(),
            format!("Bearer {}", config.device_token),
        )],
    }
}

async fn push(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    key: &AccountKey,
    clock: &HlcClock,
    report: &mut SyncReport,
) -> Result<(), SyncError> {
    let mut cursor: SyncCursor = library
        .sync_state(PUSH_CURSOR)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    loop {
        let page = library.changed_since(&cursor, PAGE)?;
        let Some(last) = page.last().map(SyncRecord::cursor) else {
            return Ok(());
        };
        let records = page.iter().map(|r| seal(key, r)).collect();
        let response: PushResponse = http
            .post_json(
                &format!("{}/v1/push", config.server),
                &PushRequest { records },
                &options(config),
            )
            .await?
            .json()?;
        observe_server(clock, response.server_ms);
        if let Some(refused) = response.rejected.first() {
            // Keep the cursor before the refused record so it is pushed
            // again once the cause (usually this device's clock) is fixed.
            let blind = |r: &SyncRecord| hex::encode(key.key_id(r.coll, &r.key));
            if let Some(stop) = page.iter().position(|r| blind(r) == refused.key_id)
                && stop > 0
            {
                save_push_cursor(library, &page[stop - 1].cursor())?;
            }
            return Err(SyncError::Rejected(refused.reason.clone()));
        }
        report.pushed += response.accepted;
        report.stale += response.stale;
        save_push_cursor(library, &last)?;
        cursor = last;
    }
}

fn save_push_cursor(library: &Library, cursor: &SyncCursor) -> Result<(), SyncError> {
    let json = serde_json::to_string(cursor).expect("cursor serializes");
    Ok(library.set_sync_state(PUSH_CURSOR, &json)?)
}

async fn pull(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    key: &AccountKey,
    clock: &HlcClock,
    report: &mut SyncReport,
) -> Result<(), SyncError> {
    let mut after: u64 = library
        .sync_state(PULL_SEQ)?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    loop {
        let page: PullResponse = http
            .get(
                &format!("{}/v1/pull?after={after}&limit={PAGE}", config.server),
                &options(config),
            )
            .await?
            .json()?;
        observe_server(clock, page.server_ms);
        let mut records = Vec::with_capacity(page.records.len());
        for wire in &page.records {
            match open(key, wire) {
                Ok(record) => records.push(record),
                Err(e) => {
                    tracing::warn!("sync record {} did not open: {e}", wire.seq);
                    report.unreadable += 1;
                }
            }
        }
        if let Some(newest) = records.iter().filter_map(|r| Hlc::decode(&r.hlc)).max() {
            clock.observe(newest, now_ms());
        }
        report.pulled += page.records.len() as u32;
        report.applied += library.apply(&records)? as u32;
        library.set_sync_state(PULL_SEQ, &page.last.to_string())?;
        after = page.last;
        if !page.more {
            return Ok(());
        }
    }
}

/// Lifts a slow local clock to the relay's time so new writes sort after
/// what other devices already pushed.
fn observe_server(clock: &HlcClock, server_ms: u64) {
    clock.observe(
        Hlc {
            wall_ms: server_ms,
            counter: 0,
            device: 0,
        },
        now_ms(),
    );
}

fn seal(key: &AccountKey, record: &SyncRecord) -> WireRecord {
    let key_id = key.key_id(record.coll, &record.key);
    let payload = serde_json::to_vec(&Payload {
        k: record.key.clone(),
        v: record.value.clone(),
    })
    .expect("json values serialize");
    WireRecord {
        coll: record.coll,
        key_id: hex::encode(key_id),
        hlc: record.hlc.clone(),
        epoch: 0,
        body: B64.encode(key.seal(record.coll, &key_id, &record.hlc, 0, &payload)),
        seq: 0,
    }
}

fn open(key: &AccountKey, wire: &WireRecord) -> Result<SyncRecord, SealError> {
    let key_id: [u8; 16] = hex::decode(&wire.key_id)
        .ok()
        .and_then(|k| k.try_into().ok())
        .ok_or(SealError::Malformed)?;
    let body = B64.decode(&wire.body).map_err(|_| SealError::Malformed)?;
    let plain = key.open(wire.coll, &key_id, &wire.hlc, wire.epoch, &body)?;
    let payload: Payload = serde_json::from_slice(&plain).map_err(|_| SealError::Malformed)?;
    // The body must belong to the register it was filed under.
    if key.key_id(wire.coll, &payload.k) != key_id {
        return Err(SealError::Open);
    }
    Ok(SyncRecord {
        coll: wire.coll,
        key: payload.k,
        value: payload.v,
        hlc: wire.hlc.clone(),
    })
}
