//! Blind relay for Marquee sync. It stores sealed registers under blind ids
//! and hands them out by cursor; it can read none of them.
//!
//! One row per `(account, coll, key id)`, replaced only by a newer HLC; each
//! replacement takes the account's next `seq`. Every database call runs under
//! one lock, so a reader never sees seq N+1 before N is committed.

use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use mq_library::hlc::{Hlc, now_ms};
use mq_sync::wire::{
    AccountResponse, MAX_BODY, MAX_CLOCK_AHEAD_MS, PAGE, PullResponse, PushRequest, PushResponse,
    Rejected, WireRecord,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{Arc, Mutex};

const MAX_REQUEST: usize = 2 * 1024 * 1024;
/// Bytes charged per stored record on top of its body.
const RECORD_OVERHEAD: i64 = 96;

#[derive(Debug, Clone)]
pub struct Config {
    /// When set, creating an account needs this value in `x-invite`.
    pub invite: Option<String>,
    /// Sealed bytes one account may store.
    pub quota_bytes: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            invite: None,
            quota_bytes: 50 * 1024 * 1024,
        }
    }
}

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    config: Arc<Config>,
}

pub fn router(db: &Path, config: Config) -> rusqlite::Result<Router> {
    let conn = Connection::open(db)?;
    conn.execute_batch(SCHEMA)?;
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        config: Arc::new(config),
    };
    Ok(Router::new()
        .route("/v1/accounts", post(create_account))
        .route("/v1/push", post(push))
        .route("/v1/pull", get(pull))
        .route("/v1/devices", post(add_device))
        .layer(DefaultBodyLimit::max(MAX_REQUEST))
        .with_state(state))
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS account (
    id          TEXT PRIMARY KEY,
    seq         INTEGER NOT NULL DEFAULT 0,
    bytes       INTEGER NOT NULL DEFAULT 0,
    created_ms  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS device (
    token_hash  BLOB PRIMARY KEY,
    account     TEXT NOT NULL REFERENCES account(id),
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

fn random_hex(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).expect("os randomness");
    hex::encode(bytes)
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

async fn create_account(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(invite) = &state.config.invite {
        let given = headers.get("x-invite").and_then(|v| v.to_str().ok());
        if given != Some(invite.as_str()) {
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
        Err(e) => {
            tracing::error!("create account: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "storage error")
        }
    }
}

/// A new token for another device of the caller's account, so each device
/// can be revoked on its own.
async fn add_device(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let db = lock(&state);
    let Some(account) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let token = random_hex(32);
    match db.execute(
        "INSERT INTO device (token_hash, account, created_ms) VALUES (?1, ?2, ?3)",
        params![token_hash(&token), account, now_ms() as i64],
    ) {
        Ok(_) => Json(AccountResponse {
            account,
            device_token: token,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("add device: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "storage error")
        }
    }
}

/// The account of a valid, unrevoked bearer token.
fn authorize(db: &Connection, headers: &HeaderMap) -> Option<String> {
    let token = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    db.query_row(
        "SELECT account FROM device WHERE token_hash = ?1 AND revoked = 0",
        params![token_hash(token)],
        |r| r.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

/// A record as stored: checked fields, decoded body.
struct Checked {
    coll: u8,
    key_id: Vec<u8>,
    hlc: String,
    epoch: u32,
    body: Vec<u8>,
}

fn check(record: &WireRecord, server_ms: u64) -> Result<Checked, String> {
    let key_id = hex::decode(&record.key_id)
        .ok()
        .filter(|k| k.len() == 16)
        .ok_or("key id must be 16 bytes of hex")?;
    let hlc = Hlc::decode(&record.hlc).ok_or("hlc must be 32 hex digits")?;
    if hlc.wall_ms > server_ms + MAX_CLOCK_AHEAD_MS {
        return Err(format!(
            "clock is {} ms ahead of the relay",
            hlc.wall_ms - server_ms
        ));
    }
    let body = B64
        .decode(&record.body)
        .map_err(|_| "body must be base64")?;
    if body.len() > MAX_BODY {
        return Err(format!("body is larger than {MAX_BODY} bytes"));
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
    let Some(account) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown or revoked device");
    };
    let result = (|| -> rusqlite::Result<Result<PushResponse, Response>> {
        let tx = db.transaction()?;
        let (mut seq, mut bytes): (i64, i64) = tx.query_row(
            "SELECT seq, bytes FROM account WHERE id = ?1",
            params![account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut response = PushResponse {
            accepted: 0,
            stale: 0,
            rejected: Vec::new(),
            server_ms,
        };
        for record in &request.records {
            let r = match check(record, server_ms) {
                Ok(r) => r,
                Err(reason) => {
                    response.rejected.push(Rejected {
                        key_id: record.key_id.clone(),
                        reason,
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
            let changed = tx.execute(
                "INSERT INTO record (account, coll, key_id, hlc, epoch, seq, body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account, coll, key_id) DO UPDATE SET
                   hlc = excluded.hlc, epoch = excluded.epoch, seq = excluded.seq, body = excluded.body
                 WHERE (excluded.hlc, excluded.epoch) > (record.hlc, record.epoch)",
                params![account, r.coll, r.key_id, r.hlc, r.epoch, seq + 1, r.body],
            )?;
            if changed == 1 {
                seq += 1;
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
        Err(e) => {
            tracing::error!("push: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "storage error")
        }
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
    let Some(account) = authorize(&db, &headers) else {
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
                more,
                server_ms,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!("pull: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "storage error")
        }
    }
}
