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

use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
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
use std::path::Path;
use std::sync::{Arc, Mutex};

const MAX_REQUEST: usize = 8 * 1024 * 1024;
/// Bytes charged per stored record on top of its body.
const RECORD_OVERHEAD: i64 = 96;
/// Devices one account may have at once.
const MAX_DEVICES: i64 = 32;

#[derive(Debug, Clone)]
pub struct Config {
    /// When set, creating an account needs this value in `x-invite`.
    pub invite: Option<String>,
    /// Sealed bytes one account may store.
    pub quota_bytes: u64,
    /// Start a new generation: use after restoring the database from a
    /// backup, so devices send back what the backup lost.
    pub new_generation: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            invite: None,
            quota_bytes: 50 * 1024 * 1024,
            new_generation: false,
        }
    }
}

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    config: Arc<Config>,
    generation: Arc<String>,
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
    };
    Ok(Router::new()
        .route("/v1/accounts", post(create_account))
        .route("/v1/devices", post(add_device))
        .route("/v1/push", post(push))
        .route("/v1/pull", get(pull))
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
";

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
    let server_ms = now_ms();
    let mut db = lock(&state);
    let Some((account, _)) = authorize(&db, &headers) else {
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
            rejected: Vec::new(),
            server_ms,
        };
        for record in &request.records {
            let r = match check(record, server_ms, epoch) {
                Ok(r) => r,
                Err((code, reason)) => {
                    response.rejected.push(Rejected {
                        key_id: record.key_id.clone(),
                        reason,
                        code,
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
        Ok(Ok(response)) => Json(response).into_response(),
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
    let server_ms = now_ms();
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
