//! `POST /v1/reports` (plans/error-reports.md 7, 8): error and crash reports
//! from the apps, kept 30 days in their own database beside the relay's.
//!
//! Nothing here can reach sync: its own file, its own connection and lock,
//! its own budget slot. No client address is written anywhere: the limits
//! live in memory, as the rest of the relay's do. A report carries no
//! identifier; `batch` is a number this relay draws per request, so the
//! nightly summary can bound how many devices a group touched.

use crate::{AppState, client, identity, limited};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, params};
use serde_json::json;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Requests a minute per address and relay-wide.
pub(crate) const REPORTS: (usize, u32, u32) = (8, 6, 600);
/// Reports one address may have stored in a day (the honest app sends far
/// fewer: its queue holds 30).
pub(crate) const PER_ADDRESS_DAY: u32 = 100;
/// Reports stored in a day across the relay.
pub(crate) const RELAY_DAY: i64 = 20_000;
/// Live pages of the database, in bytes.
pub(crate) const MAX_LIVE_BYTES: i64 = 512 * 1024 * 1024;
/// Days a report is kept.
pub(crate) const KEEP_DAYS: i64 = 30;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const SCHEMA_VERSION: i64 = 1;

pub(crate) struct Reports {
    db: Mutex<Connection>,
    /// Reports stored per address in the current day window: (start, count).
    daily: Mutex<HashMap<IpAddr, (u64, u32)>>,
    accepted: AtomicU64,
    refused: AtomicU64,
    capped: AtomicU64,
    /// When the relay-wide cap was last logged, so a flood logs once an hour.
    full_logged_ms: AtomicU64,
}

impl Reports {
    /// Opens or creates the database. `auto_vacuum` only takes effect on a
    /// file with no table yet, so it is set first; `secure_delete` overwrites
    /// what retention deletes.
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA auto_vacuum = INCREMENTAL;
             PRAGMA journal_mode = WAL;
             PRAGMA secure_delete = ON;
             CREATE TABLE IF NOT EXISTS report (
                 id       INTEGER PRIMARY KEY,
                 day      INTEGER NOT NULL,
                 hour     INTEGER NOT NULL,
                 batch    INTEGER NOT NULL,
                 app      TEXT NOT NULL,
                 version  TEXT NOT NULL,
                 kind     TEXT NOT NULL,
                 grp      TEXT NOT NULL,
                 count    INTEGER NOT NULL,
                 body     TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS report_day ON report (day);
             CREATE INDEX IF NOT EXISTS report_grp ON report (grp);",
        )?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match version {
            0 => db.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?,
            SCHEMA_VERSION => {}
            other => {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "reports database version {other}, this build writes {SCHEMA_VERSION}"
                )));
            }
        }
        Ok(Self {
            db: Mutex::new(db),
            daily: Mutex::default(),
            accepted: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            capped: AtomicU64::new(0),
            full_logged_ms: AtomicU64::new(0),
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The relay-wide caps: reports stored today, and live bytes.
    fn full(&self, now: u64) -> bool {
        let db = self.db();
        let today: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM report WHERE day = ?1",
                params![(now / DAY_MS) as i64],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let live: i64 = db
            .query_row(
                "SELECT (page_count - freelist_count) * page_size
                 FROM pragma_page_count(), pragma_freelist_count(), pragma_page_size()",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        today >= RELAY_DAY || live >= MAX_LIVE_BYTES
    }

    /// How many of `wanted` reports `address` may still store today; the
    /// count is spent here.
    fn take(&self, address: IpAddr, wanted: usize, now: u64) -> usize {
        let mut daily = self.daily.lock().unwrap_or_else(|p| p.into_inner());
        if daily.len() > 50_000 {
            daily.retain(|_, (start, _)| now < *start + DAY_MS);
        }
        let slot = daily.entry(address).or_insert((now, 0));
        if now >= slot.0 + DAY_MS {
            *slot = (now, 0);
        }
        let left = PER_ADDRESS_DAY.saturating_sub(slot.1) as usize;
        let taken = wanted.min(left);
        slot.1 += taken as u32;
        taken
    }

    fn store(&self, batch: &mq_reports::Batch, take: usize, now: u64) -> rusqlite::Result<()> {
        let mut number = [0u8; 4];
        getrandom::fill(&mut number)
            .map_err(|e| rusqlite::Error::InvalidParameterName(format!("no random number: {e}")))?;
        let number = u32::from_le_bytes(number);
        let app = serde_json::to_value(batch.device.app).unwrap_or_default();
        let mut db = self.db();
        let tx = db.transaction()?;
        for report in batch.reports.iter().take(take) {
            let kind = serde_json::to_value(report.kind).unwrap_or_default();
            let body = json!({ "device": &batch.device, "report": report });
            tx.execute(
                "INSERT INTO report (day, hour, batch, app, version, kind, grp, count, body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    (now / DAY_MS) as i64,
                    ((now % DAY_MS) / (60 * 60 * 1000)) as i64,
                    number,
                    app.as_str().unwrap_or(""),
                    batch.device.version,
                    kind.as_str().unwrap_or(""),
                    report.group,
                    report.count,
                    body.to_string(),
                ],
            )?;
        }
        tx.commit()
    }

    /// Hourly: rows past `KEEP_DAYS` go, their pages are overwritten
    /// (`secure_delete`) and given back, and the write-ahead log is emptied.
    /// Logs the counts since the last run, never an address.
    pub(crate) fn housekeeping(&self, now: u64) {
        let cutoff = (now / DAY_MS) as i64 - KEEP_DAYS;
        let db = self.db();
        match db.execute("DELETE FROM report WHERE day < ?1", params![cutoff]) {
            Ok(0) => {}
            Ok(gone) => {
                tracing::info!("{gone} reports past {KEEP_DAYS} days deleted");
                if let Err(e) = db.execute_batch("PRAGMA incremental_vacuum;") {
                    tracing::warn!("reports vacuum: {e}");
                }
            }
            Err(e) => tracing::error!("reports retention: {e}"),
        }
        if let Err(e) = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())) {
            tracing::warn!("reports checkpoint: {e}");
        }
        let counts =
            [&self.accepted, &self.refused, &self.capped].map(|c| c.swap(0, Ordering::Relaxed));
        if counts.iter().any(|&c| c > 0) {
            tracing::info!(
                "reports: {} accepted, {} refused, {} over a cap",
                counts[0],
                counts[1],
                counts[2]
            );
        }
    }
}

fn retry_after(mut response: Response, seconds: u32) -> Response {
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
    response
}

pub(crate) async fn receive(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(reports) = state.reports.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let address = match client(&state, peer, &headers) {
        Ok(address) => address,
        Err(refused) => return refused,
    };
    if limited(&state, address, REPORTS) {
        let busy = identity::coded(StatusCode::TOO_MANY_REQUESTS, "busy", "too many reports");
        return retry_after(busy, 60);
    }
    let batch = match mq_reports::parse_batch(&body) {
        Ok(batch) => batch,
        Err(refusal) => {
            reports.refused.fetch_add(1, Ordering::Relaxed);
            let why = match refusal {
                mq_reports::Refusal::BadVersion => "a report version this relay does not take",
                mq_reports::Refusal::BadReport(why) => why,
            };
            return identity::coded(StatusCode::BAD_REQUEST, refusal.code(), why);
        }
    };
    let now = state.now_ms();
    if reports.full(now) {
        reports.capped.fetch_add(1, Ordering::Relaxed);
        let last = reports.full_logged_ms.load(Ordering::Relaxed);
        if now >= last + 60 * 60 * 1000 {
            reports.full_logged_ms.store(now, Ordering::Relaxed);
            tracing::warn!("reports: the relay-wide cap is reached; new reports wait a day");
        }
        let full = identity::coded(
            StatusCode::SERVICE_UNAVAILABLE,
            "reportsFull",
            "the report store is full",
        );
        return retry_after(full, 86_400);
    }
    let take = reports.take(address, batch.reports.len(), now);
    if take < batch.reports.len() {
        reports.capped.fetch_add(1, Ordering::Relaxed);
    }
    if take > 0
        && let Err(e) = reports.store(&batch, take, now)
    {
        tracing::error!("reports store: {e}");
        return identity::coded(
            StatusCode::SERVICE_UNAVAILABLE,
            "reportsDown",
            "reports cannot be stored now",
        );
    }
    reports.accepted.fetch_add(take as u64, Ordering::Relaxed);
    axum::Json(json!({ "accepted": take })).into_response()
}
