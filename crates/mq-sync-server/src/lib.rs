//! Blind relay for Marquee sync. It stores sealed registers under blind ids
//! and hands them out by cursor; it can read none of them.
//!
//! One row per `(account, coll, key id)`, replaced only by a newer
//! `(hlc, epoch)`; each replacement takes the account's next `seq`. Every
//! database call runs under one lock, so a reader never sees seq N+1 before
//! N is committed.
//!
//! `seq` follows the clock in microseconds (and never repeats), so records
//! written after a restore from a backup sort above every cursor handed out
//! before it. The relay `generation` changes when the operator starts it with
//! `new_generation` after a restore; devices then send and read everything.
// ponytail: database calls run on the async workers under one lock; move
// them to spawn_blocking when one relay serves many busy accounts.

use axum::extract::Path as UrlPath;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use mq_library::hlc::{Hlc, now_ms};
use mq_sync::wire::{
    AccountResponse, MAX_BODY, MAX_CLOCK_AHEAD_MS, MIN_BODY, PAGE, PullResponse, PushRequest,
    PushResponse, Refusal, Rejected, WireRecord,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};

const MAX_REQUEST: usize = 8 * 1024 * 1024;
/// Bytes charged per stored record on top of its body.
const RECORD_OVERHEAD: i64 = 96;
/// Devices one account may have at once.
const MAX_DEVICES: i64 = 32;
/// Pairing slots open at once across the relay, and how long one lives.
const MAX_PAIRINGS: i64 = 1000;
const PAIRING_MS: u64 = 10 * 60 * 1000;
/// Largest sealed pairing or recovery blob.
const MAX_SMALL_BLOB: usize = 64 * 1024;
/// Recovery kits one account may register.
const MAX_KITS: i64 = 4;

#[derive(Debug, Clone)]
pub struct Config {
    /// When set, creating an account needs this value in `x-invite`.
    pub invite: Option<String>,
    /// Sealed bytes one account may store.
    pub quota_bytes: u64,
    /// Start a new generation: use after restoring the database from a
    /// backup, so devices send back what the backup lost.
    pub new_generation: bool,
    /// Added to the relay's idea of the time (tests of a relay with a
    /// wrong clock).
    pub clock_offset_ms: i64,
    /// Take the client address from `X-Forwarded-For` (behind a trusted proxy).
    pub trust_proxy: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            invite: None,
            quota_bytes: 50 * 1024 * 1024,
            new_generation: false,
            clock_offset_ms: 0,
            trust_proxy: false,
        }
    }
}

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    config: Arc<Config>,
    generation: Arc<String>,
    /// Rate limits for opening pairings and redeeming kits.
    limits: Arc<Mutex<Limits>>,
    clock: Arc<Mutex<ClockWatch>>,
}

pub fn router(db: &Path, config: Config) -> rusqlite::Result<Router> {
    let conn = Connection::open(db)?;
    conn.execute_batch(SCHEMA)?;
    let stored: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'generation'", [], |r| {
            r.get(0)
        })
        .optional()?;
    let generation = match stored {
        Some(g) if !config.new_generation => g,
        _ => {
            let g = random_hex(8);
            conn.execute(
                "INSERT INTO meta (key, value) VALUES ('generation', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![g],
            )?;
            g
        }
    };
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        config: Arc::new(config),
        generation: Arc::new(generation),
        limits: Arc::default(),
        clock: Arc::default(),
    };
    Ok(Router::new()
        .route("/v1/accounts", post(create_account))
        .route("/v1/devices", post(add_device))
        .route("/v1/push", post(push))
        .route("/v1/pull", get(pull))
        .route("/v1/pairings", post(open_pairing))
        .route("/v1/pairings/{id}", put(fill_pairing))
        .route("/v1/pairings/{id}/take", post(take_pairing))
        .route("/v1/recovery", post(add_kit).get(list_kits))
        .route("/v1/recovery/reseal", put(reseal_kit))
        .route("/v1/recovery/redeem", post(redeem_kit))
        .layer(DefaultBodyLimit::max(MAX_REQUEST))
        .with_state(state))
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS meta (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS account (
    id          TEXT PRIMARY KEY,
    seq         INTEGER NOT NULL DEFAULT 0,
    bytes       INTEGER NOT NULL DEFAULT 0,
    epoch       INTEGER NOT NULL DEFAULT 0,
    created_ms  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS device (
    token_hash  BLOB PRIMARY KEY,
    account     TEXT NOT NULL REFERENCES account(id),
    minted_by   BLOB,
    created_ms  INTEGER NOT NULL,
    revoked     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS record (
    account  TEXT NOT NULL,
    coll     INTEGER NOT NULL,
    key_id   BLOB NOT NULL,
    hlc      TEXT NOT NULL,
    epoch    INTEGER NOT NULL,
    seq      INTEGER NOT NULL,
    body     BLOB NOT NULL,
    PRIMARY KEY (account, coll, key_id)
);
CREATE INDEX IF NOT EXISTS record_pull ON record (account, seq);
CREATE TABLE IF NOT EXISTS pairing (
    id          TEXT PRIMARY KEY,
    take_hash   BLOB NOT NULL,
    sealed      BLOB,
    account     TEXT,
    minted_by   BLOB,
    expires_ms  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS recovery (
    lookup_hash  BLOB PRIMARY KEY,
    lookup       TEXT NOT NULL,
    account      TEXT NOT NULL,
    added_by     BLOB NOT NULL,
    id_tag       BLOB NOT NULL,
    auth_hash    BLOB NOT NULL,
    pk           BLOB NOT NULL,
    sealed       BLOB NOT NULL,
    updated_ms   INTEGER NOT NULL
);
";

impl AppState {
    /// The relay's time: what clock rules and pairing expiry go by.
    fn now_ms(&self) -> u64 {
        (now_ms() as i64 + self.config.clock_offset_ms).max(0) as u64
    }
}

fn lock(state: &AppState) -> std::sync::MutexGuard<'_, Connection> {
    state.db.lock().unwrap_or_else(|p| p.into_inner())
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

fn storage_error(what: &str, e: rusqlite::Error) -> Response {
    tracing::error!("{what}: {e}");
    error(StatusCode::INTERNAL_SERVER_ERROR, "storage error")
}

fn random_hex(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).expect("os randomness");
    hex::encode(bytes)
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Compares without stopping at the first difference, so response time
/// does not reveal how much of a guess was right.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Microseconds since the epoch.
fn now_us() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or_default()
}

async fn create_account(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(invite) = &state.config.invite {
        let given = headers
            .get("x-invite")
            .map(|v| v.as_bytes())
            .unwrap_or_default();
        if !same(given, invite.as_bytes()) {
            return error(StatusCode::FORBIDDEN, "an invite is required");
        }
    }
    let account = random_hex(16);
    let token = random_hex(32);
    let now = now_ms() as i64;
    let result = (|| -> rusqlite::Result<()> {
        let mut db = lock(&state);
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO account (id, created_ms) VALUES (?1, ?2)",
            params![account, now],
        )?;
        tx.execute(
            "INSERT INTO device (token_hash, account, created_ms) VALUES (?1, ?2, ?3)",
            params![token_hash(&token), account, now],
        )?;
        tx.commit()
    })();
    match result {
        Ok(()) => Json(AccountResponse {
            account,
            device_token: token,
        })
        .into_response(),
        Err(e) => storage_error("create account", e),
    }
}

/// A new token for another device of the caller's account, so each device
/// can be revoked on its own. The minting device is recorded, so revoking a
/// stolen token can take the tokens it minted with it.
async fn add_device(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let db = lock(&state);
    let Some((account, minted_by)) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let devices: rusqlite::Result<i64> = db.query_row(
        "SELECT COUNT(*) FROM device WHERE account = ?1 AND revoked = 0",
        params![account],
        |r| r.get(0),
    );
    match devices {
        Ok(n) if n >= MAX_DEVICES => {
            return error(StatusCode::FORBIDDEN, "this account has too many devices");
        }
        Ok(_) => {}
        Err(e) => return storage_error("count devices", e),
    }
    let token = random_hex(32);
    match db.execute(
        "INSERT INTO device (token_hash, account, minted_by, created_ms) VALUES (?1, ?2, ?3, ?4)",
        params![token_hash(&token), account, minted_by, now_ms() as i64],
    ) {
        Ok(_) => Json(AccountResponse {
            account,
            device_token: token,
        })
        .into_response(),
        Err(e) => storage_error("add device", e),
    }
}

/// The account and token hash of a valid, unrevoked bearer token.
fn authorize(db: &Connection, headers: &HeaderMap) -> Option<(String, Vec<u8>)> {
    let token = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    let hash = token_hash(token);
    let account = db
        .query_row(
            "SELECT account FROM device WHERE token_hash = ?1 AND revoked = 0",
            params![hash],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()?;
    Some((account, hash))
}

/// A record as stored: checked fields, decoded body.
struct Checked {
    coll: u8,
    key_id: Vec<u8>,
    hlc: String,
    epoch: u32,
    body: Vec<u8>,
}

fn check(
    record: &WireRecord,
    server_ms: u64,
    account_epoch: u32,
) -> Result<Checked, (Refusal, String)> {
    let invalid = |why: &str| (Refusal::Invalid, why.to_owned());
    let key_id = hex::decode(&record.key_id)
        .ok()
        .filter(|k| k.len() == 16)
        .ok_or_else(|| invalid("key id must be 16 bytes of hex"))?;
    let hlc =
        Hlc::decode(&record.hlc).ok_or_else(|| invalid("hlc must be 32 lowercase hex digits"))?;
    if hlc.wall_ms > server_ms + MAX_CLOCK_AHEAD_MS {
        let ahead = hlc.wall_ms - server_ms;
        return Err((
            Refusal::ClockAhead,
            format!("clock is {ahead} ms ahead of the relay"),
        ));
    }
    if record.epoch > account_epoch {
        return Err((Refusal::Epoch, "key epoch is ahead of the account".into()));
    }
    let body = B64
        .decode(&record.body)
        .map_err(|_| invalid("body must be base64"))?;
    if body.len() > MAX_BODY {
        return Err((
            Refusal::TooLarge,
            format!("body is larger than {MAX_BODY} bytes"),
        ));
    }
    if body.len() < MIN_BODY {
        return Err(invalid("body is too short to be sealed"));
    }
    Ok(Checked {
        coll: record.coll,
        key_id,
        hlc: hlc.encode(),
        epoch: record.epoch,
        body,
    })
}

async fn push(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PushRequest>,
) -> Response {
    if request.records.len() > PAGE {
        return error(StatusCode::BAD_REQUEST, "too many records in one push");
    }
    let server_ms = state.now_ms();
    let mut db = lock(&state);
    let Some((account, device)) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let result = (|| -> rusqlite::Result<Result<PushResponse, Response>> {
        let tx = db.transaction()?;
        let (mut seq, mut bytes, epoch): (i64, i64, u32) = tx.query_row(
            "SELECT seq, bytes, epoch FROM account WHERE id = ?1",
            params![account],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut response = PushResponse {
            accepted: 0,
            stale: 0,
            stale_at: Vec::new(),
            rejected: Vec::new(),
            server_ms,
        };
        for record in &request.records {
            let r = match check(record, server_ms, epoch) {
                Ok(r) => r,
                Err((code, reason)) => {
                    // For a clock refusal, the version the relay holds now, so the
                    // device can stamp its edit just above it.
                    let current = match (code, hex::decode(&record.key_id)) {
                        (Refusal::ClockAhead, Ok(key_id)) => tx
                            .query_row(
                                "SELECT hlc FROM record WHERE account = ?1 AND coll = ?2 AND key_id = ?3",
                                params![account, record.coll, key_id],
                                |row| row.get(0),
                            )
                            .optional()?,
                        _ => None,
                    };
                    response.rejected.push(Rejected {
                        key_id: record.key_id.clone(),
                        reason,
                        code,
                        current,
                    });
                    continue;
                }
            };
            let old: Option<i64> = tx
                .query_row(
                    "SELECT length(body) FROM record WHERE account = ?1 AND coll = ?2 AND key_id = ?3",
                    params![account, r.coll, r.key_id],
                    |row| row.get(0),
                )
                .optional()?;
            // Rows cost their key, clock and index entries too, not only the body.
            let grown = bytes - old.unwrap_or(-RECORD_OVERHEAD) + r.body.len() as i64;
            if grown as u64 > state.config.quota_bytes {
                return Ok(Err(error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "account storage quota reached",
                )));
            }
            let next = (seq + 1).max(now_us());
            let changed = tx.execute(
                "INSERT INTO record (account, coll, key_id, hlc, epoch, seq, body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account, coll, key_id) DO UPDATE SET
                   hlc = excluded.hlc, epoch = excluded.epoch, seq = excluded.seq, body = excluded.body
                 WHERE (excluded.hlc, excluded.epoch) > (record.hlc, record.epoch)",
                params![account, r.coll, r.key_id, r.hlc, r.epoch, next, r.body],
            )?;
            if changed == 1 {
                seq = next;
                bytes = grown;
                response.accepted += 1;
            } else {
                response.stale += 1;
                // Only a strictly newer version is worth pulling again; an
                // equal one is this device's own, pushed twice.
                let newer: Option<i64> = tx
                    .query_row(
                        "SELECT seq FROM record WHERE account = ?1 AND coll = ?2 AND key_id = ?3
                         AND (hlc, epoch) > (?4, ?5)",
                        params![account, r.coll, r.key_id, r.hlc, r.epoch],
                        |row| row.get(0),
                    )
                    .optional()?;
                response.stale_at.extend(newer.map(|s| s as u64));
            }
        }
        tx.execute(
            "UPDATE account SET seq = ?2, bytes = ?3 WHERE id = ?1",
            params![account, seq, bytes],
        )?;
        tx.commit()?;
        Ok(Ok(response))
    })();
    match result {
        Ok(Ok(response)) => {
            if response
                .rejected
                .iter()
                .any(|r| r.code == Refusal::ClockAhead)
            {
                let mut clock = state.clock.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(devices) = clock.ahead(device, server_ms) {
                    tracing::warn!(
                        "{devices} devices pushed versions more than 10 minutes ahead of this relay's clock in the last 10 minutes: the relay clock is probably behind; check its time sync"
                    );
                }
            }
            Json(response).into_response()
        }
        Ok(Err(refusal)) => refusal,
        Err(e) => storage_error("push", e),
    }
}

#[derive(Deserialize)]
struct PullQuery {
    #[serde(default)]
    after: u64,
    limit: Option<usize>,
}

async fn pull(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PullQuery>,
) -> Response {
    let server_ms = state.now_ms();
    let db = lock(&state);
    let Some((account, _)) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let limit = query.limit.unwrap_or(PAGE).clamp(1, PAGE);
    let rows = (|| -> rusqlite::Result<(Vec<WireRecord>, u64)> {
        let head: i64 = db.query_row(
            "SELECT seq FROM account WHERE id = ?1",
            params![account],
            |r| r.get(0),
        )?;
        let mut stmt = db.prepare(
            "SELECT coll, key_id, hlc, epoch, body, seq FROM record
             WHERE account = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        stmt.query_map(
            params![account, query.after as i64, limit as i64 + 1],
            |r| {
                Ok(WireRecord {
                    coll: r.get(0)?,
                    key_id: hex::encode(r.get::<_, Vec<u8>>(1)?),
                    hlc: r.get(2)?,
                    epoch: r.get(3)?,
                    body: B64.encode(r.get::<_, Vec<u8>>(4)?),
                    seq: r.get::<_, i64>(5)? as u64,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map(|records| (records, head as u64))
    })();
    match rows {
        Ok((mut records, head)) => {
            let more = records.len() > limit;
            records.truncate(limit);
            let last = records.last().map_or(query.after, |r| r.seq);
            Json(PullResponse {
                records,
                last,
                head,
                generation: state.generation.to_string(),
                more,
                server_ms,
            })
            .into_response()
        }
        Err(e) => storage_error("pull", e),
    }
}

/// Devices whose pushes came back clock-ahead lately. Several at once means
/// this relay's clock is behind, not theirs: their edits get restamped at
/// relay time, and last-writer-wins falls back to push order.
#[derive(Default)]
struct ClockWatch {
    ahead: HashMap<Vec<u8>, u64>,
    warned_ms: Option<u64>,
}

const CLOCK_WATCH_MS: u64 = 10 * 60 * 1000;

impl ClockWatch {
    /// Notes a device refused as clock-ahead; returns how many devices were,
    /// when it is time to warn (at most once per window).
    fn ahead(&mut self, device: Vec<u8>, now: u64) -> Option<usize> {
        self.ahead.insert(device, now);
        self.ahead
            .retain(|_, at| now.saturating_sub(*at) < CLOCK_WATCH_MS);
        let quiet = self
            .warned_ms
            .is_none_or(|at| now.saturating_sub(at) >= CLOCK_WATCH_MS);
        (self.ahead.len() >= 2 && quiet).then(|| {
            self.warned_ms = Some(now);
            self.ahead.len()
        })
    }
}

/// Request budgets for the unauthenticated endpoints, per client address and
/// relay-wide, so one script can neither flood the relay nor block everyone
/// else. Windows are one minute: (window start, requests).
#[derive(Default)]
pub(crate) struct Limits {
    per_client: HashMap<(usize, IpAddr), (u64, u32)>,
    global: [(u64, u32); 2],
}

const OPEN_PAIRING: (usize, u32, u32) = (0, 20, 2000);
const REDEEM_KIT: (usize, u32, u32) = (1, 10, 600);

fn spend(slot: &mut (u64, u32), now: u64, cap: u32) -> bool {
    if now >= slot.0 + 60_000 {
        *slot = (now, 0);
    }
    slot.1 += 1;
    slot.1 > cap
}

/// Whether `client` is over its budget (or the relay over its own).
fn limited(
    state: &AppState,
    client: IpAddr,
    (which, per_client, global): (usize, u32, u32),
) -> bool {
    let now = now_ms();
    let mut limits = state.limits.lock().unwrap_or_else(|p| p.into_inner());
    if limits.per_client.len() > 50_000 {
        limits
            .per_client
            .retain(|_, (start, _)| now < *start + 60_000);
    }
    let mine = spend(
        limits.per_client.entry((which, client)).or_default(),
        now,
        per_client,
    );
    let all = spend(&mut limits.global[which], now, global);
    mine || all
}

/// The caller's address: the peer, or the first `X-Forwarded-For` hop when
/// the relay sits behind a proxy it trusts.
fn client(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> IpAddr {
    let forwarded = || {
        headers
            .get("x-forwarded-for")?
            .to_str()
            .ok()?
            .split(',')
            .next()?
            .trim()
            .parse()
            .ok()
    };
    match state.config.trust_proxy {
        true => forwarded().unwrap_or(peer.ip()),
        false => peer.ip(),
    }
}

/// Serves the relay with peer addresses available to the rate limits.
pub async fn serve(listener: tokio::net::TcpListener, app: Router) -> std::io::Result<()> {
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}

#[derive(Deserialize)]
struct SealedBlob {
    sealed: String,
}

fn small_blob(text: &str) -> Option<Vec<u8>> {
    B64.decode(text)
        .ok()
        .filter(|b| (MIN_BODY..=MAX_SMALL_BLOB).contains(&b.len()))
}

/// Mints a device token for `account`, recording what created it.
fn mint_device(db: &Connection, account: &str, minted_by: &[u8]) -> Result<String, Box<Response>> {
    let devices: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM device WHERE account = ?1 AND revoked = 0",
            params![account],
            |r| r.get(0),
        )
        .map_err(|e| Box::new(storage_error("count devices", e)))?;
    if devices >= MAX_DEVICES {
        return Err(Box::new(error(
            StatusCode::FORBIDDEN,
            "this account has too many devices",
        )));
    }
    let token = random_hex(32);
    db.execute(
        "INSERT INTO device (token_hash, account, minted_by, created_ms) VALUES (?1, ?2, ?3, ?4)",
        params![token_hash(&token), account, minted_by, now_ms() as i64],
    )
    .map_err(|e| Box::new(storage_error("mint device", e)))?;
    Ok(token)
}

/// A one-time slot a new device shows as a QR. The QR carries the slot id
/// (for the device that fills it); collecting needs the `take` secret, which
/// stays on the new device.
async fn open_pairing(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if limited(&state, client(&state, peer, &headers), OPEN_PAIRING) {
        return error(StatusCode::TOO_MANY_REQUESTS, "too many pairings started");
    }
    let now = state.now_ms();
    let db = lock(&state);
    let open = (|| -> rusqlite::Result<i64> {
        db.execute(
            "DELETE FROM pairing WHERE expires_ms <= ?1",
            params![now as i64],
        )?;
        db.query_row("SELECT COUNT(*) FROM pairing", [], |r| r.get(0))
    })();
    match open {
        Ok(n) if n >= MAX_PAIRINGS => {
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "too many pairings in progress",
            );
        }
        Ok(_) => {}
        Err(e) => return storage_error("open pairing", e),
    }
    let (id, take) = (random_hex(16), random_hex(16));
    let expires_ms = now + PAIRING_MS;
    match db.execute(
        "INSERT INTO pairing (id, take_hash, expires_ms) VALUES (?1, ?2, ?3)",
        params![id, token_hash(&take), expires_ms as i64],
    ) {
        Ok(_) => {
            Json(json!({ "pairing": id, "take": take, "expiresMs": expires_ms })).into_response()
        }
        Err(e) => storage_error("open pairing", e),
    }
}

/// A device of the account drops the sealed bundle into the slot, once.
async fn fill_pairing(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
    Json(blob): Json<SealedBlob>,
) -> Response {
    let db = lock(&state);
    let Some((account, filler)) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let Some(sealed) = small_blob(&blob.sealed) else {
        return error(
            StatusCode::BAD_REQUEST,
            "sealed blob has a bad size or encoding",
        );
    };
    match db.execute(
        "UPDATE pairing SET sealed = ?2, account = ?3, minted_by = ?4
         WHERE id = ?1 AND sealed IS NULL AND expires_ms > ?5",
        params![id, sealed, account, filler, state.now_ms() as i64],
    ) {
        Ok(1) => Json(json!({})).into_response(),
        // Tell "someone else answered" apart from "expired": the first means
        // the code on the new device belongs to someone else.
        Ok(_) => match db
            .query_row(
                "SELECT 1 FROM pairing WHERE id = ?1 AND sealed IS NOT NULL",
                params![id],
                |_| Ok(()),
            )
            .optional()
        {
            Ok(Some(())) => error(
                StatusCode::CONFLICT,
                "someone else already answered this code; reject it on the new device",
            ),
            Ok(None) => error(StatusCode::GONE, "this pairing code expired"),
            Err(e) => storage_error("fill pairing", e),
        },
        Err(e) => storage_error("fill pairing", e),
    }
}

#[derive(Deserialize)]
struct Take {
    take: String,
    /// Collect the token too (the user confirmed); otherwise only the blob.
    #[serde(default)]
    claim: bool,
}

/// take hash, sealed blob, account, minting device, expiry
type PairingRow = (
    Vec<u8>,
    Option<Vec<u8>>,
    Option<String>,
    Option<Vec<u8>>,
    i64,
);

/// The new device collects the blob with its take secret: 204 while
/// waiting, then the blob (as often as asked), then, with `claim` once the
/// user confirmed the codes, a token minted for the account that filled
/// the slot; after that 404.
async fn take_pairing(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    Json(claim): Json<Take>,
) -> Response {
    let db = lock(&state);
    let row: rusqlite::Result<Option<PairingRow>> = db
        .query_row(
            "SELECT take_hash, sealed, account, minted_by, expires_ms FROM pairing WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional();
    let (take_hash, sealed, account, filler, expires) = match row {
        Ok(Some(row)) => row,
        Ok(None) => return error(StatusCode::NOT_FOUND, "no pairing with that code"),
        Err(e) => return storage_error("take pairing", e),
    };
    if !same(&take_hash, &token_hash(&claim.take)) {
        return error(StatusCode::NOT_FOUND, "no pairing with that code");
    }
    if expires as u64 <= state.now_ms() {
        return error(StatusCode::NOT_FOUND, "pairing expired");
    }
    let (Some(sealed), Some(account), Some(filler)) = (sealed, account, filler) else {
        return StatusCode::NO_CONTENT.into_response();
    };
    if !claim.claim {
        return Json(json!({ "sealed": B64.encode(sealed) })).into_response();
    }
    // The user confirmed the codes match: now, and only now, the new
    // device gets a token.
    let token = match mint_device(&db, &account, &filler) {
        Ok(token) => token,
        Err(refusal) => return *refusal,
    };
    if let Err(e) = db.execute("DELETE FROM pairing WHERE id = ?1", params![id]) {
        return storage_error("claim pairing", e);
    }
    Json(json!({ "deviceToken": token, "account": account })).into_response()
}

fn lookup_hash(lookup: &str) -> Option<Vec<u8>> {
    hex::decode(lookup)
        .ok()
        .filter(|l| l.len() == 16)
        .map(|l| Sha256::digest(l).to_vec())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewKit {
    lookup: String,
    /// SHA-256 of the kit's redeem secret, hex.
    auth_hash: String,
    /// The kit's X25519 public key, base64; devices seal new bundles to it.
    pk: String,
    /// HMAC of the account's id key under a kit-derived key, hex: lets the
    /// kit holder check a recovered bundle is the account's own.
    id_tag: String,
    sealed: String,
}

/// Registers a recovery kit for the account: its lookup id, the hash of its
/// redeem secret, its public key and the bundle sealed to that key. A kit
/// is written once; later bundles are resealed to the same public key.
async fn add_kit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(kit): Json<NewKit>,
) -> Response {
    let mut db = lock(&state);
    let Some((account, added_by)) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let auth_hash = hex::decode(&kit.auth_hash).ok().filter(|h| h.len() == 32);
    let Some(id_tag) = hex::decode(&kit.id_tag).ok().filter(|t| t.len() == 32) else {
        return error(StatusCode::BAD_REQUEST, "bad recovery kit fields");
    };
    let pk = B64.decode(&kit.pk).ok().filter(|k| k.len() == 32);
    let (Some(hash), Some(auth_hash), Some(pk), Some(sealed)) = (
        lookup_hash(&kit.lookup),
        auth_hash,
        pk,
        small_blob(&kit.sealed),
    ) else {
        return error(StatusCode::BAD_REQUEST, "bad recovery kit fields");
    };
    let added = (|| -> rusqlite::Result<Result<(), Response>> {
        let tx = db.transaction()?;
        let kits: i64 = tx.query_row(
            "SELECT COUNT(*) FROM recovery WHERE account = ?1",
            params![account],
            |r| r.get(0),
        )?;
        if kits >= MAX_KITS {
            return Ok(Err(error(
                StatusCode::FORBIDDEN,
                "this account has enough recovery kits",
            )));
        }
        let inserted = tx.execute(
            "INSERT INTO recovery (lookup_hash, lookup, account, added_by, id_tag, auth_hash, pk, sealed, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(lookup_hash) DO NOTHING",
            params![hash, kit.lookup, account, added_by, id_tag, auth_hash, pk, sealed, now_ms() as i64],
        )?;
        tx.commit()?;
        Ok(if inserted == 1 {
            Ok(())
        } else {
            Err(error(
                StatusCode::CONFLICT,
                "this kit is already registered",
            ))
        })
    })();
    match added {
        Ok(Ok(())) => Json(json!({})).into_response(),
        Ok(Err(refusal)) => refusal,
        Err(e) => storage_error("add kit", e),
    }
}

/// The account's kits as (lookup, public key), so a device can reseal a new
/// bundle to each after a key rotation without knowing any kit.
async fn list_kits(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let db = lock(&state);
    let Some((account, _)) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let kits = (|| -> rusqlite::Result<Vec<serde_json::Value>> {
        let mut stmt = db.prepare("SELECT lookup, pk FROM recovery WHERE account = ?1")?;
        stmt.query_map(params![account], |r| {
            let (lookup, pk): (String, Vec<u8>) = (r.get(0)?, r.get(1)?);
            Ok(json!({ "lookup": lookup, "pk": B64.encode(pk) }))
        })?
        .collect()
    })();
    match kits {
        Ok(kits) => Json(json!({ "kits": kits })).into_response(),
        Err(e) => storage_error("list kits", e),
    }
}

#[derive(Deserialize)]
struct Reseal {
    lookup: String,
    sealed: String,
}

/// Replaces the sealed bundle of one of the account's kits.
async fn reseal_kit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(reseal): Json<Reseal>,
) -> Response {
    let db = lock(&state);
    let Some((account, _)) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let (Some(hash), Some(sealed)) = (lookup_hash(&reseal.lookup), small_blob(&reseal.sealed))
    else {
        return error(StatusCode::BAD_REQUEST, "bad lookup id or sealed blob");
    };
    match db.execute(
        "UPDATE recovery SET sealed = ?3, updated_ms = ?4 WHERE lookup_hash = ?1 AND account = ?2",
        params![hash, account, sealed, now_ms() as i64],
    ) {
        Ok(1) => Json(json!({})).into_response(),
        Ok(_) => error(StatusCode::NOT_FOUND, "no such kit on this account"),
        Err(e) => storage_error("reseal kit", e),
    }
}

#[derive(Deserialize)]
struct Redeem {
    lookup: String,
    /// The kit's redeem secret, hex. Only its hash is stored.
    auth: String,
}

/// account, redeem secret hash, sealed bundle, id tag, device that added it
type KitRow = (String, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);

/// Joins the account of a kit as a new device: needs the kit's redeem
/// secret, which no device keeps, so a revoked device holding the bundle
/// cannot come back this way.
async fn redeem_kit(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(redeem): Json<Redeem>,
) -> Response {
    if limited(&state, client(&state, peer, &headers), REDEEM_KIT) {
        return error(StatusCode::TOO_MANY_REQUESTS, "too many recovery attempts");
    }
    let not_found = || error(StatusCode::NOT_FOUND, "no recovery data for that kit");
    let (Some(hash), Some(auth)) = (lookup_hash(&redeem.lookup), hex::decode(&redeem.auth).ok())
    else {
        return not_found();
    };
    let db = lock(&state);
    let found: rusqlite::Result<Option<KitRow>> = db
        .query_row(
            "SELECT account, auth_hash, sealed, id_tag, added_by FROM recovery WHERE lookup_hash = ?1",
            params![hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional();
    let (account, auth_hash, sealed, id_tag, added_by) = match found {
        Ok(Some(found)) => found,
        Ok(None) => return not_found(),
        Err(e) => return storage_error("redeem kit", e),
    };
    if !same(&auth_hash, &Sha256::digest(&auth)) {
        return not_found();
    }
    // Minted as by the device that added the kit, so revoking that device
    // can take these tokens (and its kits) with it.
    match mint_device(&db, &account, &added_by) {
        Ok(token) => Json(json!({
            "account": account,
            "deviceToken": token,
            "sealed": B64.encode(sealed),
            "idTag": hex::encode(id_tag),
        }))
        .into_response(),
        Err(refusal) => *refusal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_devices_ahead_warn_once_per_window() {
        let mut watch = ClockWatch::default();
        assert_eq!(
            watch.ahead(vec![1], 1_000),
            None,
            "one device: its own clock"
        );
        assert_eq!(watch.ahead(vec![1], 2_000), None);
        assert_eq!(watch.ahead(vec![2], 3_000), Some(2));
        assert_eq!(watch.ahead(vec![3], 4_000), None, "already warned");
        // A device seen long ago no longer counts.
        let later = 3_000 + CLOCK_WATCH_MS;
        assert_eq!(watch.ahead(vec![4], later), Some(2), "devices 3 and 4");
        assert_eq!(watch.ahead(vec![5], later + 2 * CLOCK_WATCH_MS), None);
    }
}
