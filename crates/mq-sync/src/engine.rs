//! One sync round: push queued local registers, then pull and apply what
//! other devices wrote. Push problems never stop the pull, so one bad record
//! cannot cut a device off from its other devices.
//!
//! State lives in the library's `sync_state` and outbox, so a crash at any
//! point only repeats work: pushes are upserts and applies are idempotent.

use crate::seal::{AccountKey, KeyBundle, SealError};
use crate::wire::{
    AccountResponse, MAX_CLOCK_AHEAD_MS, PAGE, PullResponse, PushRequest, PushResponse, Refusal,
    WireRecord,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use mq_http::{HttpClient, HttpError, Priority, RequestOptions};
use mq_library::hlc::{Hlc, HlcClock, MAX_WALL_MS};
use mq_library::{Library, LibraryError, SyncRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::Duration;

const PULL_SEQ: &str = "pull_seq";
const READER_SEEN: &str = "pull_reader";
const QUEUED_ALL: &str = "queued_all";
const TARGET: &str = "sync_target";
const GENERATION: &str = "relay_generation";

/// Shape version of register values this build writes; newer ones are
/// skipped until the app updates.
pub const SHAPES: u16 = 1;
/// What this build can read. Raising it makes every device pull everything
/// again once, to apply records an older build skipped.
const READER: &str = "2";
/// Sealed bytes sent in one push request.
const PUSH_BYTES: usize = 4 * 1024 * 1024;

/// Where and as whom to sync. The token and keys come from the platform
/// keystore and are never logged.
#[derive(Clone)]
pub struct SyncConfig {
    pub server: String,
    pub device_token: String,
    pub keys: KeyBundle,
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
    /// Registers the relay will never take (too large, malformed); dropped
    /// from the outbox so they do not block the rest.
    pub refused: u32,
    pub pulled: u32,
    /// Pulled registers that changed local state.
    pub applied: u32,
    /// Pulled records that did not open with this account key.
    pub unreadable: u32,
    /// Pulled records written by a newer app version, kept for after the
    /// update.
    pub newer: u32,
    /// This device's clock minus the relay's. Beyond 10 minutes either way
    /// one of them is wrong: the UI should say so (pushes get stamped at
    /// the relay's time, and last-writer-wins falls back to push order).
    pub clock_gap_ms: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error("the relay refused a record: {0}")]
    Rejected(String),
    #[error("the relay must use https unless it is on this network")]
    Insecure,
    #[error("pairing: {0}")]
    Pairing(String),
}

/// The plaintext inside a sealed body. The key travels inside so the reader
/// can check it against the blind id the body was filed under.
#[derive(Serialize, Deserialize)]
struct Payload {
    k: String,
    v: serde_json::Value,
    /// Shape version of `v`.
    #[serde(default)]
    s: u16,
}

pub async fn sync_once(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    prepare(library, config)?;
    let key = AccountKey::from_bundle(&config.keys);
    let pushed = push(library, http, config, &key, clock, &mut report).await;
    pull(library, http, config, &key, clock, &mut report).await?;
    pushed?;
    Ok(report)
}

/// Only the push half, for when the app goes to the background.
pub async fn push_changes(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    clock: &HlcClock,
) -> Result<SyncReport, SyncError> {
    let mut report = SyncReport::default();
    prepare(library, config)?;
    let key = AccountKey::from_bundle(&config.keys);
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
    prepare(library, config)?;
    let key = AccountKey::from_bundle(&config.keys);
    pull(library, http, config, &key, clock, &mut report).await?;
    Ok(report)
}

/// Creates an account on the relay and returns this device's token. The
/// caller generates the key bundle and keeps both in the keystore.
pub async fn create_account(
    http: &HttpClient,
    server: &str,
    invite: Option<&str>,
) -> Result<AccountResponse, SyncError> {
    require_secure(server)?;
    let mut options = options(server, "");
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

/// A token for a new device of an account, asked for by a device that
/// already belongs to it. Each device then gets revoked on its own.
pub async fn add_device(
    http: &HttpClient,
    server: &str,
    device_token: &str,
) -> Result<AccountResponse, SyncError> {
    require_secure(server)?;
    Ok(http
        .post_json(
            &format!("{server}/v1/devices"),
            &serde_json::json!({}),
            &options(server, device_token),
        )
        .await?
        .json()?)
}

/// Bearer tokens never cross the internet in clear text.
pub(crate) fn require_secure(server: &str) -> Result<(), SyncError> {
    let url = url::Url::parse(server).map_err(|_| SyncError::Insecure)?;
    // Loopback, private ranges and link-local only: carrier NAT (100.64/10)
    // and names that may resolve anywhere still cross other networks.
    let local = match url.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Some(url::Host::Ipv6(ip)) => {
            ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
        }
        None => false,
    };
    if url.scheme() == "https" || (url.scheme() == "http" && local) {
        Ok(())
    } else {
        Err(SyncError::Insecure)
    }
}

/// Cursors belong to one relay and one device token: syncing somewhere else
/// starts over, queueing everything and pulling from the start.
fn prepare(library: &Library, config: &SyncConfig) -> Result<(), SyncError> {
    require_secure(&config.server)?;
    let token = hex::encode(&Sha256::digest(config.device_token.as_bytes())[..8]);
    let target = format!("{}|{token}", config.server);
    if library.sync_state(TARGET)?.as_deref() != Some(target.as_str()) {
        for key in [PULL_SEQ, QUEUED_ALL, READER_SEEN, GENERATION] {
            library.set_sync_state(key, "")?;
        }
        library.set_sync_state(TARGET, &target)?;
    }
    if library.sync_state(QUEUED_ALL)?.as_deref() != Some("1") {
        // Registers written before this device synced here are not queued.
        library.queue_all()?;
        library.set_sync_state(QUEUED_ALL, "1")?;
    }
    Ok(())
}

fn options(server: &str, device_token: &str) -> RequestOptions {
    let allow_private = url::Url::parse(server)
        .ok()
        .and_then(|u| u.host_str().map(mq_http::is_private_host))
        .unwrap_or(false);
    RequestOptions {
        priority: Priority::Addon,
        timeout: Duration::from_secs(30),
        max_bytes: 16 * 1024 * 1024,
        allow_private,
        headers: vec![("authorization".into(), format!("Bearer {device_token}"))],
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
    let mut restamped = HashSet::new();
    loop {
        let page = library.outbox(PAGE)?;
        let Some(last_id) = page.last_id else {
            return Ok(());
        };
        // As many records as fit one request; the rest stay queued.
        let mut records = Vec::new();
        let mut sent: Vec<&SyncRecord> = Vec::new();
        let mut bytes = 0;
        let mut through = last_id;
        for (id, record) in &page.records {
            let wire = seal(key, config.keys.epoch, record);
            if !records.is_empty() && bytes + wire.body.len() > PUSH_BYTES {
                through = id - 1;
                break;
            }
            bytes += wire.body.len();
            records.push(wire);
            sent.push(record);
        }
        if records.is_empty() {
            library.outbox_done(last_id)?;
            continue;
        }
        let response: PushResponse = http
            .post_json(
                &format!("{}/v1/push", config.server),
                &PushRequest { records },
                &options(&config.server, &config.device_token),
            )
            .await?
            .json()?;
        observe_server(clock, response.server_ms);
        report.clock_gap_ms = clock.wall_now() as i64 - response.server_ms as i64;
        report.pushed += response.accepted;
        report.stale += response.stale;
        // The relay holds a newer version of these, which this device may
        // have passed already: read from just before the oldest of them.
        if let Some(oldest) = response.stale_at.iter().min() {
            let cursor: u64 = library
                .sync_state(PULL_SEQ)?
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if oldest.saturating_sub(1) < cursor {
                library.set_sync_state(PULL_SEQ, &oldest.saturating_sub(1).to_string())?;
            }
        }
        let mut stuck = None;
        for refusal in &response.rejected {
            let Some(record) = sent
                .iter()
                .find(|r| hex::encode(key.key_id(r.coll, &r.key)) == refusal.key_id)
            else {
                continue;
            };
            match refusal.code {
                // Written while this device's clock ran ahead. Nobody else
                // ever accepted that version, so it is stamped again at the
                // relay's time, once per round.
                // A version stamped by another device (an import, or one queued
                // after moving to a new relay) is never re-stamped: it must not
                // jump above a real edit. It waits, queued, until the relay's
                // clock reaches it.
                Refusal::ClockAhead
                    if Hlc::decode(&record.hlc).is_none_or(|h| h.device != clock.device()) =>
                {
                    library.requeue(record.coll, &record.key)?;
                    stuck = Some(refusal.reason.clone());
                }
                Refusal::ClockAhead if restamped.insert((record.coll, record.key.clone())) => {
                    // Just above what the relay holds for this register (and no
                    // earlier than its time), so the edit still wins without
                    // pushing every device's clock to the 10-minute edge.
                    let device = clock.device();
                    let floor = Hlc {
                        wall_ms: response.server_ms.min(MAX_WALL_MS),
                        counter: 0,
                        device,
                    };
                    let above = refusal
                        .current
                        .as_deref()
                        .and_then(Hlc::decode)
                        .map(|current| current.next_for(device));
                    let at = above.map_or(floor, |above| above.max(floor));
                    clock.observe(at, clock.wall_now());
                    library.restamp(record.coll, &record.key, &at.encode())?;
                }
                Refusal::ClockAhead => {
                    // Still ahead after a restamp: keep it queued for a later round.
                    library.requeue(record.coll, &record.key)?;
                    stuck = Some(refusal.reason.clone());
                }
                _ => {
                    tracing::warn!("relay refused a register for good: {}", refusal.reason);
                    report.refused += 1;
                }
            }
        }
        library.outbox_done(through)?;
        if let Some(reason) = stuck {
            return Err(SyncError::Rejected(reason));
        }
    }
}

async fn pull(
    library: &Library,
    http: &HttpClient,
    config: &SyncConfig,
    key: &AccountKey,
    clock: &HlcClock,
    report: &mut SyncReport,
) -> Result<(), SyncError> {
    let reader = format!("{READER}.{SHAPES}");
    let mut after: u64 = match library.sync_state(READER_SEEN)? {
        Some(seen) if seen == reader => library
            .sync_state(PULL_SEQ)?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        _ => 0,
    };
    let mut started_over = false;
    loop {
        let page: PullResponse = http
            .get(
                &format!("{}/v1/pull?after={after}&limit={PAGE}", config.server),
                &options(&config.server, &config.device_token),
            )
            .await?
            .json()?;
        observe_server(clock, page.server_ms);
        report.clock_gap_ms = clock.wall_now() as i64 - page.server_ms as i64;
        let known = library.sync_state(GENERATION)?.unwrap_or_default();
        let restored = page.head < after || (!known.is_empty() && known != page.generation);
        if restored && !started_over {
            // The relay lost records (restored from a backup): send it
            // everything this device holds and read it all again. Once per
            // round, so a relay that keeps saying so cannot loop us.
            started_over = true;
            library.queue_all()?;
            library.set_sync_state(GENERATION, &page.generation)?;
            library.set_sync_state(PULL_SEQ, "0")?;
            after = 0;
            continue;
        }
        library.set_sync_state(GENERATION, &page.generation)?;
        let mut records = Vec::with_capacity(page.records.len());
        for wire in &page.records {
            match open(key, wire) {
                Ok((_, shape)) if shape > SHAPES => report.newer += 1,
                // The relay refuses clocks this far ahead, so such a record
                // comes from a key holder that should not be believed.
                Ok((record, _)) => records.push(record),
                Err(e) => {
                    tracing::warn!("sync record {} did not open: {e}", wire.seq);
                    report.unreadable += 1;
                }
            }
        }
        // The relay refused anything more than 10 minutes ahead of its own
        // time when it was pushed; the clock's absolute ceiling covers a relay
        // that lies about both.
        if let Some(newest) = records.iter().filter_map(|r| Hlc::decode(&r.hlc)).max() {
            let capped = Hlc {
                wall_ms: newest.wall_ms.min(page.server_ms + MAX_CLOCK_AHEAD_MS),
                ..newest
            };
            clock.observe(capped, clock.wall_now());
        }
        report.pulled += page.records.len() as u32;
        report.applied += library.apply(&records)? as u32;
        library.set_sync_state(PULL_SEQ, &page.last.to_string())?;
        // A relay that says "more" without moving forward is not followed.
        let stalled = page.last <= after;
        after = page.last;
        if !page.more || stalled {
            library.set_sync_state(READER_SEEN, &reader)?;
            return Ok(());
        }
    }
}

/// Lifts a slow local clock to the relay's time (like NTP) so new writes
/// sort after what other devices already pushed; a TV box that booted with
/// its clock in 1970 needs exactly this. The clock's absolute ceiling keeps
/// a lying relay from breaking the encoding.
fn observe_server(clock: &HlcClock, server_ms: u64) {
    let now = clock.wall_now();
    clock.observe(
        Hlc {
            wall_ms: server_ms,
            counter: 0,
            device: 0,
        },
        now,
    );
}

fn seal(key: &AccountKey, epoch: u32, record: &SyncRecord) -> WireRecord {
    let key_id = key.key_id(record.coll, &record.key);
    let payload = serde_json::to_vec(&Payload {
        k: record.key.clone(),
        v: record.value.clone(),
        s: SHAPES,
    })
    .expect("json values serialize");
    WireRecord {
        coll: record.coll,
        key_id: hex::encode(key_id),
        hlc: record.hlc.clone(),
        epoch,
        body: B64.encode(key.seal(record.coll, &key_id, &record.hlc, epoch, &payload)),
        seq: 0,
    }
}

/// Opens a pulled record; returns it with the shape version it was written in.
fn open(key: &AccountKey, wire: &WireRecord) -> Result<(SyncRecord, u16), SealError> {
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
    let record = SyncRecord {
        coll: wire.coll,
        key: payload.k,
        value: payload.v,
        hlc: wire.hlc.clone(),
    };
    Ok((record, payload.s))
}
