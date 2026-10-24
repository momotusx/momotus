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
//!
//! Who belongs to an account is the account's roster (`mq_sync::roster`):
//! entries members sign, which the relay checks with the same rules devices
//! use before storing them, and copies to a journal file so restoring the
//! database alone never loses one.
//!
//! A deleted account leaves only its id in `gone`, in the database and in the
//! journal, so no crash, restore or replay brings it back: its routes answer
//! 410 `accountGone`.
//!
//! With an identity file (`identity`), people sign in with Apple, Google,
//! Facebook or an OpenID provider; the identity only finds the account, and
//! sign-up then needs a ticket from such a sign-in (or the invite).
// ponytail: database calls run on the async workers under one lock; move
// them to spawn_blocking when one relay serves many busy accounts.

mod identity;
mod jwt;

pub use identity::IdentityFile;

use axum::extract::Path as UrlPath;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use mq_library::hlc::{Hlc, now_ms};
use mq_sync::roster::{Entry, Hex, Id, Kind, Role, Roster, RosterError, TokenRequest};
use mq_sync::wire::{
    AccountRequest, AccountResponse, AppendRequest, Behind, DeleteRequest, MAX_BODY,
    MAX_CLOCK_AHEAD_MS, MIN_BODY, PAGE, PullResponse, PushRequest, PushResponse, ROSTER_PAGE,
    Redeemed, Refusal, Rejected, RelayInfo, ReplayRequest, RosterHead, RosterPage,
    ScheduledDeletion, TokenGrant, WireRecord,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write as _;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_REQUEST: usize = 8 * 1024 * 1024;
/// Bytes charged per stored record on top of its body.
const RECORD_OVERHEAD: i64 = 96;
/// Pairing slots open at once across the relay, and how long one lives.
const MAX_PAIRINGS: i64 = 1000;
/// Pairing slots one client address may hold open at once: a TV holds one,
/// a household behind one address joining a few devices stays far below,
/// and a handful of addresses can no longer fill `MAX_PAIRINGS` and stop
/// every QR join (plans/accounts-social-sync.md, review A P2-9).
const PAIRINGS_PER_CLIENT: usize = 10;
const PAIRING_MS: u64 = 10 * 60 * 1000;
/// Largest sealed pairing blob.
const MAX_SMALL_BLOB: usize = 64 * 1024;
/// A pending token (claimed, not yet joined) lives this long.
const PENDING_MS: u64 = 15 * 60 * 1000;
/// A token request signed this far from the relay's time is refused.
const TOKEN_SKEW_MS: u64 = 5 * 60 * 1000;
/// Tokens a member may hold from signed requests; more revoke the oldest.
const FRESH_TOKENS: i64 = 3;
/// The database layout this build writes; an older one is refused.
const SCHEMA_VERSION: i64 = 4;

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
    /// wrong clock); shared, so a test can fix the clock while it runs.
    pub clock_offset_ms: Arc<AtomicI64>,
    /// Take the client address from the last `X-Forwarded-For` hop, the one
    /// a trusted reverse proxy appends.
    pub trust_proxy: bool,
    /// The host devices reach this relay at (as in their server URL, without
    /// the port). Signed token requests must name it, so a request signed
    /// for another relay's host is refused. `None` takes any host.
    pub public_host: Option<String>,
    /// Sign-in with Apple, Google, Facebook or OpenID providers; needs
    /// `public_host` (nonces and callbacks are bound to it).
    pub identity: Option<IdentityFile>,
    /// How often due deletions run and expired sign-in rows go (an hour;
    /// tests shorten it).
    pub housekeeping_every: std::time::Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            invite: None,
            quota_bytes: 50 * 1024 * 1024,
            new_generation: false,
            clock_offset_ms: Arc::default(),
            trust_proxy: false,
            public_host: None,
            identity: None,
            housekeeping_every: std::time::Duration::from_secs(3600),
        }
    }
}

/// Each client address's open pairing slots: (slot id, when it ends).
type OpenPairings = HashMap<IpAddr, Vec<(String, u64)>>;

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    /// Lines of every accepted roster entry; written after the database
    /// commits, under the database lock, so its order is the table's.
    journal: Arc<Mutex<std::fs::File>>,
    /// Where the journal lives, for the rewrite after a deletion.
    journal_path: Arc<PathBuf>,
    /// A journal write failed: no roster write is taken until a restart
    /// writes the journal again from the table, so it never has a gap.
    journal_failed: Arc<AtomicBool>,
    config: Arc<Config>,
    generation: Arc<String>,
    generation_since_ms: u64,
    relay_id: Id,
    /// Rate limits for the endpoints that need no token.
    limits: Arc<Mutex<Limits>>,
    clock: Arc<Mutex<ClockWatch>>,
    /// Sign-in, when the relay has an identity file.
    identity: Option<Arc<identity::Identity>>,
    /// Accounts each client address created by replay in the grace hour.
    grace_created: Arc<Mutex<HashMap<IpAddr, u32>>>,
    /// Pairing slots each client address opened, with when they end
    /// (`PAIRINGS_PER_CLIENT`). In memory: a restart forgets them, and the
    /// relay-wide cap still holds.
    open_pairings: Arc<Mutex<OpenPairings>>,
    /// Whole addon lists refused since start (older apps), for the log.
    old_lists: Arc<std::sync::atomic::AtomicU64>,
}

/// Opens (or creates) the relay database at `db` and its roster journal
/// next to it, and returns the HTTP routes. With an identity file, its key
/// must be the one the database was made with.
pub fn router(db: &Path, config: Config) -> Result<Router, String> {
    if let Some(host) = &config.public_host {
        check_public_host(host)?;
    }
    let conn = Connection::open(db).map_err(|e| e.to_string())?;
    let (conn, generation, generation_since_ms, relay_id) =
        open_database(conn, &config).map_err(|e| e.to_string())?;
    let identity = match &config.identity {
        Some(file) => {
            let host = config
                .public_host
                .as_deref()
                .ok_or("an identity file needs the relay's public host")?;
            let identity = identity::Identity::new(file.clone(), host)?;
            check_identity_key(&conn, &identity.fingerprint())?;
            Some(Arc::new(identity))
        }
        None => None,
    };
    let journal_path = db.with_extension("roster.jsonl");
    let journal = open_journal(&journal_path, &conn)?;
    let every = config.housekeeping_every;
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        journal: Arc::new(Mutex::new(journal)),
        journal_path: Arc::new(journal_path),
        journal_failed: Arc::default(),
        config: Arc::new(config),
        generation: Arc::new(generation),
        generation_since_ms,
        relay_id,
        limits: Arc::default(),
        clock: Arc::default(),
        identity,
        grace_created: Arc::default(),
        open_pairings: Arc::default(),
        old_lists: Arc::default(),
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::spawn(housekeeping(state.clone(), every));
    }
    let mut routes = Router::new()
        .route("/v1/relay", get(relay_info))
        .route("/v1/accounts", post(create_account))
        .route("/v1/account", delete(delete_account))
        .route("/v1/tokens", post(fresh_token))
        .route("/v1/roster", get(read_roster).post(append_roster))
        .route("/v1/roster/replay", post(replay_roster))
        .route("/v1/push", post(push))
        .route("/v1/pull", get(pull))
        .route("/v1/pairings", post(open_pairing))
        .route("/v1/pairings/{id}", put(fill_pairing))
        .route("/v1/pairings/{id}/take", post(take_pairing))
        .route("/v1/recovery/redeem", post(redeem_kit));
    if state.identity.is_some() {
        use identity::*;
        routes = routes
            .route("/v1/auth/{provider}/start", post(start).options(preflight))
            .route(
                "/v1/auth/{provider}/callback",
                get(callback_get).post(callback_post),
            )
            .route("/v1/auth/take", post(take).options(preflight))
            .route("/v1/auth/token", post(native))
            .route("/v1/auth/apple/notifications", post(apple_notification))
            .route("/v1/identity", get(links).post(link))
            .route("/v1/identity/{link}", delete(unlink))
            .route(
                "/v1/account/delete-request",
                post(delete_request)
                    .delete(identity::cancel_deletion)
                    .options(preflight),
            );
    }
    Ok(routes
        .layer(DefaultBodyLimit::max(MAX_REQUEST))
        .with_state(state))
}

/// `--public-host` as devices compare it with their relay address (the URL's
/// host: lowercase, no scheme, port or path). Anything else would refuse
/// every token, nonce and return without saying why.
fn check_public_host(host: &str) -> Result<(), String> {
    let parsed = url::Url::parse(&format!("https://{host}/")).ok();
    let plain = parsed.as_ref().is_some_and(|u| {
        u.host_str() == Some(host)
            && u.port().is_none()
            && u.path() == "/"
            && u.username().is_empty()
    });
    if plain {
        Ok(())
    } else {
        Err(format!(
            "--public-host {host:?} must be the relay's host as devices write it: lowercase, without scheme, port or path"
        ))
    }
}

/// The database keeps the identity key's fingerprint: another key would
/// turn every user into a new one, so the relay does not start on it.
fn check_identity_key(db: &Connection, fingerprint: &str) -> Result<(), String> {
    let stored: Option<String> = db
        .query_row(
            "SELECT value FROM meta WHERE key = 'identity_key'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match stored {
        Some(stored) if stored != fingerprint => Err(
            "the identity key is not the one this database was made with; restore its identity file"
                .into(),
        ),
        Some(_) => Ok(()),
        None => db
            .execute(
                "INSERT INTO meta (key, value) VALUES ('identity_key', ?1)",
                params![fingerprint],
            )
            .map(drop)
            .map_err(|e| e.to_string()),
    }
}

/// Every `every`: deletions that came due run (one batch, one journal
/// rewrite) and sign-in rows past their time go.
async fn housekeeping(state: AppState, every: std::time::Duration) {
    let mut tick = tokio::time::interval(every);
    tick.tick().await;
    loop {
        tick.tick().await;
        run_due(&state);
    }
}

/// After a deletion: `secure_delete` overwrites the database file only, so
/// the write-ahead log is emptied too.
fn checkpoint(db: &Connection) {
    if let Err(e) = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())) {
        tracing::warn!("checkpoint after a deletion: {e}");
    }
}

fn run_due(state: &AppState) {
    let now = state.now_ms();
    let mut db = lock(state);
    if let Err(e) = identity::expire(&db, now) {
        tracing::warn!("expire sign-in rows: {e}");
    }
    if journal_stopped(state).is_some() {
        return;
    }
    let ran = (|| -> rusqlite::Result<Vec<Id>> {
        let tx = db.transaction()?;
        let due: Vec<String> = {
            let mut stmt = tx.prepare("SELECT account FROM deletion WHERE due_ms <= ?1")?;
            stmt.query_map(params![now as i64], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for account in &due {
            purge(&tx, account, now)?;
        }
        tx.commit()?;
        Ok(due.iter().filter_map(|a| account_id(a)).collect())
    })();
    match ran {
        Ok(due) if due.is_empty() => {}
        Ok(due) => {
            tracing::info!("{} scheduled deletions ran", due.len());
            checkpoint(&db);
            rewrite_after_deletion(state, &db, &due, now);
        }
        Err(e) => tracing::error!("scheduled deletions: {e}"),
    }
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA secure_delete = ON;
CREATE TABLE IF NOT EXISTS meta (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS account (
    id                TEXT PRIMARY KEY,
    seq               INTEGER NOT NULL DEFAULT 0,
    bytes             INTEGER NOT NULL DEFAULT 0,
    epoch             INTEGER NOT NULL DEFAULT 0,
    rotation_pending  INTEGER NOT NULL DEFAULT 0,
    created_ms        INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS device (
    token_hash   BLOB PRIMARY KEY,
    account      TEXT NOT NULL REFERENCES account(id),
    member_id    BLOB,
    state        TEXT NOT NULL,
    revoked_seq  INTEGER,
    fresh        INTEGER NOT NULL DEFAULT 0,
    created_ms   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS device_member ON device (account, member_id);
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
CREATE TABLE IF NOT EXISTS roster (
    account  TEXT NOT NULL,
    seq      INTEGER NOT NULL,
    hash     BLOB NOT NULL,
    body     BLOB NOT NULL,
    sig      BLOB NOT NULL,
    PRIMARY KEY (account, seq)
);
CREATE TABLE IF NOT EXISTS pairing (
    id          TEXT PRIMARY KEY,
    take_hash   BLOB NOT NULL,
    sealed      BLOB,
    account     TEXT,
    expires_ms  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS recovery (
    lookup_hash  BLOB PRIMARY KEY,
    account      TEXT NOT NULL,
    auth_hash    BLOB NOT NULL,
    kit_id       BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS recovery_account ON recovery (account);
CREATE TABLE IF NOT EXISTS token_nonce (
    nonce       BLOB PRIMARY KEY,
    expires_ms  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS gone (
    account     TEXT PRIMARY KEY,
    deleted_ms  INTEGER NOT NULL
);
-- Sign-in (plans/accounts.md 2.1): a keyed hash of each identity, never
-- the provider's subject, email or name.
CREATE TABLE IF NOT EXISTS identity (
    link        INTEGER PRIMARY KEY,
    provider    TEXT NOT NULL,
    subject     BLOB NOT NULL,
    account     TEXT NOT NULL,
    created_ms  INTEGER NOT NULL,
    UNIQUE (provider, subject)
);
CREATE INDEX IF NOT EXISTS identity_account ON identity (account);
CREATE TABLE IF NOT EXISTS signups (
    subject  BLOB NOT NULL,
    at_ms    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS signups_subject ON signups (subject, at_ms);
CREATE TABLE IF NOT EXISTS auth_flow (
    id_hash        BLOB PRIMARY KEY,
    provider       TEXT NOT NULL,
    purpose        TEXT NOT NULL,
    verifier_hash  BLOB NOT NULL,
    nonce          TEXT NOT NULL,
    return_to      TEXT,
    client         TEXT NOT NULL,
    created_ms     INTEGER NOT NULL,
    code_hash      BLOB,
    verified       TEXT
);
CREATE TABLE IF NOT EXISTS ticket (
    hash        BLOB PRIMARY KEY,
    provider    TEXT NOT NULL,
    subject     BLOB NOT NULL,
    expires_ms  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS receipt (
    hash        BLOB PRIMARY KEY,
    link        INTEGER,
    expires_ms  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS deletion (
    account  TEXT PRIMARY KEY,
    due_ms   INTEGER NOT NULL,
    link     INTEGER
);
";

type Opened = (Connection, String, u64, Id);

fn open_database(conn: Connection, config: &Config) -> rusqlite::Result<Opened> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let tables: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
        [],
        |r| r.get(0),
    )?;
    if tables > 0 && version != SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidParameterName(
            "this relay database is from an older version; start a new one".into(),
        ));
    }
    conn.execute_batch(SCHEMA)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    let meta = |key: &str| -> rusqlite::Result<Option<String>> {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get(0)
        })
        .optional()
    };
    let set = |key: &str, value: &str| {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
    };
    let relay_id = match meta("relay_id")?.and_then(|id| hex::decode(id).ok()) {
        Some(id) if id.len() == 16 => Hex(id.try_into().expect("16 bytes")),
        _ => {
            let id = Hex(random());
            set("relay_id", &hex::encode(id.0))?;
            id
        }
    };
    let (generation, since) = match meta("generation")? {
        Some(g) if !config.new_generation => {
            let since = meta("generation_since_ms")?
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            (g, since)
        }
        stored => {
            let g = random_hex(8);
            // A first generation was never restored: no grace period.
            let since = if stored.is_some() { now_ms() } else { 0 };
            set("generation", &g)?;
            set("generation_since_ms", &since.to_string())?;
            (g, since)
        }
    };
    Ok((conn, generation, since, relay_id))
}

/// One journal line: a roster entry, an account deleted for good, or a
/// deletion scheduled (or, with no date, cancelled), so a restore keeps it.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum JournalLine {
    Entry {
        account: Id,
        entry: Entry,
    },
    Gone {
        gone: Id,
        #[serde(rename = "deletedMs")]
        deleted_ms: u64,
    },
    Deletion {
        deletion: Id,
        #[serde(rename = "dueMs")]
        due_ms: Option<u64>,
        #[serde(default)]
        link: Option<i64>,
    },
}

/// Opens the roster journal and makes it and the table agree: accounts a
/// `gone` line names are deleted from the database first (a restored backup
/// may still hold them) and keep their tombstone; entries the database lost
/// are taken from the journal, never for a gone account; then the journal is
/// written again from the tables, whole and in order. So a torn last line,
/// and lines a crash or a failed write skipped (at the end or in the
/// middle), are back before the relay takes a write. A line that conflicts
/// with the table, or does not continue it, stops the relay.
// ponytail: rewrites every account's roster at each start; append only the
// missing lines when journals grow large.
fn open_journal(path: &Path, conn: &Connection) -> Result<std::fs::File, String> {
    let fail = |e: std::io::Error| format!("roster journal: {e}");
    let text = match std::fs::read(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(fail(e)),
    };
    let mut lines = Vec::new();
    for (n, line) in text.split_inclusive(|&b| b == b'\n').enumerate() {
        let parsed = serde_json::from_slice::<JournalLine>(line);
        match (line.ends_with(b"\n"), parsed) {
            (true, Ok(JournalLine::Entry { account, entry })) => lines.push((account, entry)),
            // A tombstone already here came with its purge in one transaction,
            // and nothing creates rows for a gone account: only a database
            // that lacks it (a restored backup) needs the purge.
            (true, Ok(JournalLine::Gone { gone, deleted_ms })) => {
                if !is_gone(conn, &gone).map_err(|e| e.to_string())? {
                    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
                    purge(&tx, &hex::encode(gone.0), deleted_ms).map_err(|e| e.to_string())?;
                    tx.commit().map_err(|e| e.to_string())?;
                }
            }
            // In order, the last line of an account wins: a restored backup
            // gets back what was scheduled or cancelled after it.
            (
                true,
                Ok(JournalLine::Deletion {
                    deletion,
                    due_ms,
                    link,
                }),
            ) => {
                let account = hex::encode(deletion.0);
                let applied = match due_ms {
                    Some(due) => conn.execute(
                        "INSERT INTO deletion (account, due_ms, link) SELECT ?1, ?2, ?3
                         WHERE NOT EXISTS (SELECT 1 FROM gone WHERE account = ?1)
                         ON CONFLICT(account) DO UPDATE SET due_ms = excluded.due_ms, link = excluded.link",
                        params![account, due as i64, link],
                    ),
                    None => conn.execute("DELETE FROM deletion WHERE account = ?1", params![account]),
                };
                applied.map_err(|e| e.to_string())?;
            }
            // Torn while being written: the entry was never acknowledged.
            (false, _) => break,
            (true, Err(_)) => return Err(format!("roster journal line {} is damaged", n + 1)),
        }
    }
    for (account, entry) in &lines {
        if is_gone(conn, account).map_err(|e| e.to_string())? {
            continue;
        }
        let body = entry.parse().map_err(|e| e.to_string())?;
        let stored: Option<Vec<u8>> = conn
            .query_row(
                "SELECT hash FROM roster WHERE account = ?1 AND seq = ?2",
                params![hex::encode(account.0), body.seq as i64],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        match stored {
            Some(hash) if hash == entry.hash().0 => {}
            Some(_) => {
                return Err(format!(
                    "roster journal and database disagree at seq {}",
                    body.seq
                ));
            }
            None => {
                let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
                let entry = std::slice::from_ref(entry);
                if let Err(refused) =
                    append_entries(&tx, account, entry, None, false).map_err(|e| e.to_string())?
                {
                    return Err(format!("roster journal seq {}: {}", body.seq, refused.1));
                }
                tx.commit().map_err(|e| e.to_string())?;
            }
        }
    }
    rewrite_journal(path, conn).map_err(fail)
}

/// Writes the journal again from the tables (every tombstone, every
/// scheduled deletion, then every roster entry in order) and returns a
/// handle that appends to the new file: a temp file, fsync, rename, fsync
/// of the directory.
// ponytail: reads every account's roster under the lock; scheduled
// deletions already run in one batch with one rewrite.
fn rewrite_journal(path: &Path, conn: &Connection) -> std::io::Result<std::fs::File> {
    type Due = (String, i64, Option<i64>);
    let tables = (|| -> rusqlite::Result<_> {
        let mut gone = conn.prepare("SELECT account, deleted_ms FROM gone ORDER BY account")?;
        let gone = gone
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut due =
            conn.prepare("SELECT account, due_ms, link FROM deletion ORDER BY account")?;
        let due = due
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<Due>>>()?;
        let mut stmt =
            conn.prepare("SELECT account, body, sig FROM roster ORDER BY account, seq")?;
        let entries = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, stored_entry(r.get(1)?, r.get(2)?)))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((gone, due, entries))
    })();
    let (gone, due, entries) = tables.map_err(std::io::Error::other)?;
    let id = |account: &str| account_id(account).expect("stored account ids are hex");
    let temp = path.with_extension("jsonl.tmp");
    let mut file = std::fs::File::create(&temp)?;
    for (account, deleted_ms) in gone {
        let line = JournalLine::Gone {
            gone: id(&account),
            deleted_ms: deleted_ms as u64,
        };
        write_line(&mut file, &line)?;
    }
    for (account, due_ms, link) in due {
        let line = JournalLine::Deletion {
            deletion: id(&account),
            due_ms: Some(due_ms as u64),
            link,
        };
        write_line(&mut file, &line)?;
    }
    for (account, entry) in entries {
        let account = id(&account);
        write_line(&mut file, &JournalLine::Entry { account, entry })?;
    }
    file.sync_all()?;
    std::fs::rename(&temp, path)?;
    // Windows opens no directory as a file; its renames need no such step.
    #[cfg(unix)]
    {
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
        std::fs::File::open(dir.unwrap_or(Path::new(".")))?.sync_all()?;
    }
    std::fs::OpenOptions::new().append(true).open(path)
}

fn write_line(file: &mut std::fs::File, line: &JournalLine) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(line).expect("journal lines serialize");
    line.push(b'\n');
    file.write_all(&line)
}

fn is_gone(db: &Connection, account: &Id) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT 1 FROM gone WHERE account = ?1",
        params![hex::encode(account.0)],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
}

/// Deletes everything the relay holds for `account` (records, roster,
/// recovery data, tokens, the pairings it filled, identity links and their
/// receipts, a scheduled deletion) and keeps only its tombstone, inside the
/// caller's transaction. An earlier tombstone keeps its date.
fn purge(tx: &Connection, account: &str, deleted_ms: u64) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO gone (account, deleted_ms) VALUES (?1, ?2) ON CONFLICT(account) DO NOTHING",
        params![account, deleted_ms as i64],
    )?;
    tx.execute(
        "DELETE FROM receipt WHERE link IN (SELECT link FROM identity WHERE account = ?1)",
        [account],
    )?;
    for table in [
        "record", "roster", "recovery", "device", "pairing", "identity", "deletion",
    ] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE account = ?1"),
            [account],
        )?;
    }
    tx.execute("DELETE FROM account WHERE id = ?1", [account])?;
    Ok(())
}

/// Journals a scheduled deletion (`due`), or its cancellation (`None`),
/// durably before the answer; a failure stops roster writes as for entries.
fn journal_deletion(
    state: &AppState,
    account: &Id,
    due: Option<u64>,
    link: Option<i64>,
) -> std::io::Result<()> {
    let mut file = state.journal.lock().unwrap_or_else(|p| p.into_inner());
    let line = JournalLine::Deletion {
        deletion: *account,
        due_ms: due,
        link,
    };
    let written = write_line(&mut file, &line).and_then(|()| file.sync_data());
    if written.is_err() {
        state.journal_failed.store(true, Ordering::Relaxed);
    }
    written
}

/// After deletions committed: the journal written again without their
/// lines, the append handle on the new file. A failed rewrite leaves the
/// deletions standing (the next start writes the journal from the tables):
/// roster writes stop until then, and the old journal gets each `gone`
/// line, so a backup restored before that start cannot bring them back.
fn rewrite_after_deletion(state: &AppState, db: &Connection, gone: &[Id], now: u64) {
    let mut file = state.journal.lock().unwrap_or_else(|p| p.into_inner());
    match rewrite_journal(&state.journal_path, db) {
        Ok(new) => *file = new,
        Err(e) => {
            tracing::error!("roster journal rewrite after a deletion: {e}");
            state.journal_failed.store(true, Ordering::Relaxed);
            for account in gone {
                let line = JournalLine::Gone {
                    gone: *account,
                    deleted_ms: now,
                };
                if let Err(e) = write_line(&mut file, &line).and_then(|()| file.sync_data()) {
                    tracing::error!("roster journal gone line: {e}");
                }
            }
        }
    }
}

/// The answer for an account that was deleted.
fn account_gone() -> Response {
    (
        StatusCode::GONE,
        Json(json!({ "error": "this account was deleted", "code": "accountGone" })),
    )
        .into_response()
}

/// Writes accepted entries to the journal, durably, before anyone is told
/// they were accepted. Runs under the database lock, after the commit; a
/// failure stops roster writes until a restart repairs the journal.
fn journal(state: &AppState, account: &Id, entries: &[Entry]) -> std::io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut file = state.journal.lock().unwrap_or_else(|p| p.into_inner());
    let written = entries
        .iter()
        .try_for_each(|entry| {
            let line = JournalLine::Entry {
                account: *account,
                entry: entry.clone(),
            };
            write_line(&mut file, &line)
        })
        .and_then(|()| file.sync_data());
    if written.is_err() {
        state.journal_failed.store(true, Ordering::Relaxed);
    }
    written
}

/// The refusal for a roster write while the journal is broken.
fn journal_stopped(state: &AppState) -> Option<Response> {
    state.journal_failed.load(Ordering::Relaxed).then(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "the relay cannot record roster changes; its operator must restart it",
        )
    })
}

fn stored_entry(body: Vec<u8>, sig: Vec<u8>) -> Entry {
    Entry {
        body,
        sig: Hex(sig.try_into().unwrap_or([0; 64])),
    }
}

fn account_id(text: &str) -> Option<Id> {
    hex::decode(text)
        .ok()
        .and_then(|b| b.try_into().ok())
        .map(Hex)
}

impl AppState {
    /// The relay's time: what clock rules and pairing expiry go by.
    fn now_ms(&self) -> u64 {
        let offset = self.config.clock_offset_ms.load(Ordering::Relaxed);
        (now_ms() as i64 + offset).max(0) as u64
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

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("os randomness");
    bytes
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

async fn relay_info(State(state): State<AppState>) -> Response {
    Json(RelayInfo {
        relay_id: state.relay_id,
        generation: state.generation.to_string(),
        generation_since_ms: state.generation_since_ms,
        server_ms: state.now_ms(),
        providers: state
            .identity
            .as_deref()
            .map(identity::Identity::providers)
            .unwrap_or_default(),
    })
    .into_response()
}

/// Accounts one client address may create by replay in the grace hour.
const GRACE_CREATIONS: u32 = 20;

/// Whether a replayed genesis may create its account. Without sign-in, as
/// for `/v1/accounts` (the invite, when one is set). With it, sign-up needs
/// a ticket, so a replay creates only with the invite, or within the hour
/// after a restore (`new_generation`), when devices give back what the
/// backup lost, at most 20 per client address.
fn may_replay_create(state: &AppState, headers: &HeaderMap, address: IpAddr) -> bool {
    if state.identity.is_none() {
        return invited(state, headers);
    }
    if invite_matches(state, headers) {
        return true;
    }
    let grace = state.generation_since_ms != 0
        && state.now_ms() < state.generation_since_ms + 60 * 60 * 1000;
    let mut created = state
        .grace_created
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if !grace {
        // The hour is over: its counts go with it.
        created.clear();
        return false;
    }
    let count = created.entry(address).or_default();
    *count += 1;
    *count <= GRACE_CREATIONS
}

/// The invite, configured and given: false when none is configured (unlike
/// `invited`, which then lets anyone in). Relays with sign-in go by this.
fn invite_matches(state: &AppState, headers: &HeaderMap) -> bool {
    state.config.invite.as_ref().is_some_and(|invite| {
        let given = headers
            .get("x-invite")
            .map(|v| v.as_bytes())
            .unwrap_or_default();
        same(given, invite.as_bytes())
    })
}

fn invited(state: &AppState, headers: &HeaderMap) -> bool {
    state.config.invite.as_ref().is_none_or(|invite| {
        let given = headers
            .get("x-invite")
            .map(|v| v.as_bytes())
            .unwrap_or_default();
        same(given, invite.as_bytes())
    })
}

/// A new account from its genesis entry; the token returned is bound to the
/// device the genesis lists. Once only: the genesis is no secret (anyone
/// who saw the roster holds it), so a device that lost the answer signs
/// for a token like any member.
///
/// With sign-in, creating needs a ticket (or the invite itself): the
/// ticket's identity is linked to the new account and counted as a sign-up
/// (at most 3 in 30 days). An identity linked elsewhere is refused unless
/// `move` (Start over): its link moves here and the old account is
/// scheduled for deletion. The ticket is used only once the account is made.
async fn create_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AccountRequest>,
) -> Response {
    let signs_in = state.identity.is_some();
    let ticket = request.ticket.as_deref().filter(|_| signs_in);
    let invite = match signs_in {
        true => invite_matches(&state, &headers),
        false => invited(&state, &headers),
    };
    if !invite && ticket.is_none() {
        let why = match signs_in {
            true => "a sign-in or an invite is required",
            false => "an invite is required",
        };
        return error(StatusCode::FORBIDDEN, why);
    }
    let body = match request.entry.parse() {
        Ok(body) => body,
        Err(e) => return error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let Kind::Genesis(genesis) = &body.kind else {
        return error(
            StatusCode::BAD_REQUEST,
            "an account starts with its genesis",
        );
    };
    let now = state.now_ms();
    let mut db = lock(&state);
    if let Some(stopped) = journal_stopped(&state) {
        return stopped;
    }
    type Made = (String, Option<(Id, u64)>);
    let created = (|| -> rusqlite::Result<Result<Made, Response>> {
        let tx = db.transaction()?;
        if is_gone(&tx, &body.account)? {
            return Ok(Err(account_gone()));
        }
        if account_epoch(&tx, &body.account).optional()?.is_some() {
            return Ok(Err(error(
                StatusCode::CONFLICT,
                "this account already exists",
            )));
        }
        let mut scheduled = None;
        if let Some(ticket) = ticket {
            match signed_up(&tx, ticket, &body.account, request.move_link, now)? {
                Ok(old) => scheduled = old,
                Err(refused) => return Ok(Err(refused)),
            }
        }
        if let Err(refused) = append_entries(
            &tx,
            &body.account,
            std::slice::from_ref(&request.entry),
            None,
            true,
        )? {
            return Ok(Err(refused.into_response()));
        }
        let token = mint(
            &tx,
            &body.account,
            Some(&genesis.keys.id()),
            "active",
            false,
        )?;
        tx.commit()?;
        Ok(Ok((token, scheduled)))
    })();
    match created {
        Ok(Ok((token, scheduled))) => {
            let journaled =
                journal(&state, &body.account, &[request.entry]).and_then(|()| match scheduled {
                    Some((old, due)) => journal_deletion(&state, &old, Some(due), None),
                    None => Ok(()),
                });
            match journaled {
                Ok(()) => Json(AccountResponse {
                    account: hex::encode(body.account.0),
                    device_token: token,
                })
                .into_response(),
                Err(e) => journal_error(e),
            }
        }
        Ok(Err(refused)) => refused,
        Err(e) => storage_error("create account", e),
    }
}

/// A sign-up with a ticket, inside the creating transaction: the identity
/// linked to `account`, the sign-up counted, the ticket used. With `move`,
/// an identity linked elsewhere comes here and the old account waits 7 days
/// for its deletion: that account and its date.
fn signed_up(
    tx: &Connection,
    ticket: &str,
    account: &Id,
    move_link: bool,
    now: u64,
) -> rusqlite::Result<Result<Option<(Id, u64)>, Response>> {
    let Some((provider, subject)) = identity::ticket(tx, ticket, now, false)? else {
        return Ok(Err(identity::ticket_refused()));
    };
    if identity::signups(tx, &subject, now)? >= identity::SIGNUP_CAP {
        return Ok(Err(identity::coded(
            StatusCode::TOO_MANY_REQUESTS,
            "signupLimit",
            "this identity made several accounts lately",
        )));
    }
    let new = hex::encode(account.0);
    let mut scheduled = None;
    match identity::identity_link(tx, &provider, &subject)? {
        Some(_) if !move_link => return Ok(Err(identity::linked_elsewhere())),
        Some((link, old)) => {
            tx.execute(
                "UPDATE identity SET account = ?2, created_ms = ?3 WHERE link = ?1",
                params![link, new, now as i64],
            )?;
            // A deletion that link asked for on the old account no longer
            // names it: "Cancel and unlink" there must not reach the new one.
            tx.execute(
                "UPDATE deletion SET link = NULL WHERE link = ?1",
                params![link],
            )?;
            tx.execute(
                "INSERT INTO deletion (account, due_ms, link) VALUES (?1, ?2, NULL)
                 ON CONFLICT(account) DO NOTHING",
                params![old, (now + identity::DELETION_WAIT_MS) as i64],
            )?;
            let due: i64 = tx.query_row(
                "SELECT due_ms FROM deletion WHERE account = ?1",
                params![old],
                |r| r.get(0),
            )?;
            scheduled = account_id(&old).map(|old| (old, due as u64));
        }
        None => {
            tx.execute(
                "INSERT INTO identity (provider, subject, account, created_ms) VALUES (?1, ?2, ?3, ?4)",
                params![provider, subject, new, now as i64],
            )?;
        }
    }
    tx.execute(
        "INSERT INTO signups (subject, at_ms) VALUES (?1, ?2)",
        params![subject, now as i64],
    )?;
    identity::ticket(tx, ticket, now, true)?;
    Ok(Ok(scheduled))
}

fn journal_error(e: std::io::Error) -> Response {
    tracing::error!("roster journal: {e}");
    error(StatusCode::INTERNAL_SERVER_ERROR, "storage error")
}

/// Deletes the token's account now: everything goes in one transaction that
/// keeps only the tombstone, then the journal is written again without the
/// account's lines and with its `gone` line, and the append handle moves to
/// the new file. Every device of the account then gets 410 `accountGone`.
///
/// Once committed, the deletion stands and the answer is 200, also when the
/// rewrite fails: a restart writes the journal again from the tables (the
/// tombstone included), and until then roster writes stop and the old
/// journal gets the `gone` line, so a backup restored before that restart
/// cannot bring the account back.
///
/// A token alone deletes nothing: the request is signed by the device key
/// of the token's own member, a current device of the account, for this
/// relay and host, near the relay's time, its nonce used once. An Apple
/// revoke's receipt may come with it.
async fn delete_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(asked): Json<DeleteRequest>,
) -> Response {
    let now = state.now_ms();
    let mut db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    if let Some(stopped) = journal_stopped(&state) {
        return stopped;
    }
    let account = hex::encode(auth.account.0);
    let deleted = (|| -> rusqlite::Result<Result<(), Response>> {
        let tx = db.transaction()?;
        if !signed_delete(&tx, &state, &auth, &asked.request, &asked.sig, now)? {
            return Ok(Err(error(
                StatusCode::FORBIDDEN,
                "the deletion is not signed by this device",
            )));
        }
        let apple: Vec<i64> = {
            let mut stmt =
                tx.prepare("SELECT link FROM identity WHERE account = ?1 AND provider = 'apple'")?;
            stmt.query_map(params![account], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        if let Some(refused) = identity::check_receipt(&tx, asked.receipt.as_deref(), &apple, now)?
        {
            return Ok(Err(refused));
        }
        if asked.receipt.is_none() && !apple.is_empty() {
            tracing::info!("an account with an Apple link was deleted without a revoke receipt");
        }
        purge(&tx, &account, now)?;
        tx.commit()?;
        Ok(Ok(()))
    })();
    match deleted {
        Ok(Ok(())) => {}
        Ok(Err(refused)) => return refused,
        Err(e) => return storage_error("delete account", e),
    }
    checkpoint(&db);
    rewrite_after_deletion(&state, &db, &[auth.account], now);
    Json(json!({})).into_response()
}

/// Whether `request` is a deletion signed by the token's own member, a
/// current device of its account, for this relay and its host, near the
/// relay's time, with a nonce not used before (it is used now).
fn signed_delete(
    tx: &Connection,
    state: &AppState,
    auth: &Auth,
    request: &TokenRequest,
    sig: &Hex<64>,
    now: u64,
) -> rusqlite::Result<bool> {
    let host = state.config.public_host.as_ref();
    let member: Option<Option<Vec<u8>>> = tx
        .query_row(
            "SELECT member_id FROM device WHERE token_hash = ?1",
            params![auth.token],
            |r| r.get(0),
        )
        .optional()?;
    if request.relay_id != state.relay_id
        || request.account != auth.account
        || request.at_ms.abs_diff(now) > TOKEN_SKEW_MS
        || host.is_some_and(|host| *host != request.host)
        || member.flatten().as_deref() != Some(&request.member.0[..])
    {
        return Ok(false);
    }
    let roster = load_roster(tx, &auth.account)?;
    let keys = match roster.members().get(&request.member) {
        Some(m) if m.role == Role::Device => m.keys,
        _ => return Ok(false),
    };
    if !request.verifies_delete(&keys, sig) {
        return Ok(false);
    }
    tx.execute(
        "DELETE FROM token_nonce WHERE expires_ms < ?1",
        params![now as i64],
    )?;
    let fresh = tx.execute(
        "INSERT INTO token_nonce (nonce, expires_ms) VALUES (?1, ?2) ON CONFLICT(nonce) DO NOTHING",
        params![request.nonce.0.to_vec(), (now + 2 * TOKEN_SKEW_MS) as i64],
    )? == 1;
    Ok(fresh)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenState {
    Pending,
    Active,
    Revoked,
}

struct Auth {
    account: Id,
    token: Vec<u8>,
    state: TokenState,
    /// For a revoked token: the roster entry that removed its member.
    revoked_seq: Option<u64>,
}

/// The token in `headers`, whatever its state; a pending token past its
/// time (by the machine's clock, like its minting) is not a token.
fn authorize(db: &Connection, headers: &HeaderMap) -> Option<Auth> {
    let token = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    let hash = token_hash(token);
    type Row = (String, String, Option<i64>, i64);
    let (account, state, revoked_seq, created): Row = db
        .query_row(
            "SELECT account, state, revoked_seq, created_ms FROM device WHERE token_hash = ?1",
            params![hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .ok()
        .flatten()?;
    let state = match state.as_str() {
        "active" => TokenState::Active,
        "pending" if now_ms() < created as u64 + PENDING_MS => TokenState::Pending,
        "revoked" => TokenState::Revoked,
        _ => return None,
    };
    Some(Auth {
        account: account_id(&account)?,
        token: hash,
        state,
        revoked_seq: revoked_seq.map(|s| s as u64),
    })
}

/// The active token's account, or the refusal for any other token.
fn active(auth: Option<Auth>) -> Result<Auth, Box<Response>> {
    match auth {
        Some(auth) if auth.state == TokenState::Active => Ok(auth),
        Some(auth) if auth.state == TokenState::Pending => Err(Box::new(error(
            StatusCode::FORBIDDEN,
            "this device has not joined the account yet",
        ))),
        _ => Err(Box::new(error(
            StatusCode::UNAUTHORIZED,
            "unknown or revoked device",
        ))),
    }
}

/// Mints a token for `account`, bound to `member` when known.
fn mint(
    db: &Connection,
    account: &Id,
    member: Option<&Id>,
    state: &str,
    fresh: bool,
) -> rusqlite::Result<String> {
    let token = random_hex(32);
    db.execute(
        "INSERT INTO device (token_hash, account, member_id, state, fresh, created_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            token_hash(&token),
            hex::encode(account.0),
            member.map(|m| m.0.to_vec()),
            state,
            fresh,
            now_ms() as i64
        ],
    )?;
    Ok(token)
}

/// Why an append was refused, as an HTTP answer.
struct Refused(StatusCode, String);

impl Refused {
    fn into_response(self) -> Response {
        match self.0 {
            // Not 409, which a device reads as "the head moved" and retries.
            StatusCode::UNPROCESSABLE_ENTITY => identity::coded(self.0, "kitTaken", &self.1),
            status => error(status, &self.1),
        }
    }
}

/// The account's roster as stored, every entry checked again.
fn load_roster(db: &Connection, account: &Id) -> rusqlite::Result<Roster> {
    let mut roster = Roster::default();
    for entry in account_entries(db, account)? {
        if let Err(e) = roster.apply(&entry) {
            tracing::error!("stored roster entry does not apply: {e}");
            break;
        }
    }
    Ok(roster)
}

fn account_entries(db: &Connection, account: &Id) -> rusqlite::Result<Vec<Entry>> {
    let mut stmt = db.prepare("SELECT body, sig FROM roster WHERE account = ?1 ORDER BY seq")?;
    stmt.query_map(params![hex::encode(account.0)], |r| {
        Ok(stored_entry(r.get(0)?, r.get(1)?))
    })?
    .collect()
}

/// Appends `entries` after the account's head, checking each with the
/// roster rules and applying its effects, inside the caller's transaction.
/// Entries already stored (same seq, same hash) are skipped, so a retry
/// after a lost answer succeeds. A pending token appending its own
/// `addDevice` becomes active, bound to that device. Returns the entries it
/// stored.
fn append_entries(
    tx: &rusqlite::Transaction,
    account: &Id,
    entries: &[Entry],
    pending: Option<&[u8]>,
    live: bool,
) -> rusqlite::Result<Result<Vec<Entry>, Refused>> {
    let mut roster = load_roster(tx, account)?;
    let mut stored = Vec::new();
    let refuse = |status, why: &str| Ok(Err(Refused(status, why.to_owned())));
    for entry in entries {
        let body = match entry.parse() {
            Ok(body) if body.account == *account => body,
            Ok(_) => return refuse(StatusCode::BAD_REQUEST, "entry for another account"),
            Err(e) => return refuse(StatusCode::BAD_REQUEST, &e.to_string()),
        };
        if roster.head().is_some_and(|(seq, _)| body.seq <= seq) {
            let held: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT hash FROM roster WHERE account = ?1 AND seq = ?2",
                    params![hex::encode(account.0), body.seq as i64],
                    |r| r.get(0),
                )
                .optional()?;
            if held.as_deref() == Some(&entry.hash().0[..]) {
                continue;
            }
            return refuse(StatusCode::CONFLICT, "another entry holds that place");
        }
        // A kit's lookup is one account's: a passkey's lookup another
        // account holds would answer redeems for that one. Only for a new
        // entry a device sends (`live`): history (a replay, the journal at
        // start) keeps whoever holds the lookup now, and is never refused
        // for it.
        if let (true, Kind::AddKit(kit)) = (live, &body.kind) {
            let holder: Option<String> = tx
                .query_row(
                    "SELECT account FROM recovery WHERE lookup_hash = ?1",
                    params![kit.lookup_hash.0.to_vec()],
                    |r| r.get(0),
                )
                .optional()?;
            if holder.is_some_and(|h| h != hex::encode(account.0)) {
                return refuse(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "this kit's lookup belongs to another account",
                );
            }
        }
        match roster.apply(entry) {
            Ok(()) => {}
            Err(RosterError::NotNext) => {
                return refuse(StatusCode::CONFLICT, "entry does not follow the head");
            }
            Err(e) => return refuse(StatusCode::BAD_REQUEST, &e.to_string()),
        }
        let account_hex = hex::encode(account.0);
        tx.execute(
            "INSERT INTO roster (account, seq, hash, body, sig) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                account_hex,
                body.seq as i64,
                entry.hash().0.to_vec(),
                entry.body,
                entry.sig.0.to_vec()
            ],
        )?;
        // Every token of the member, also one revoked before (by the cap on
        // fresh tokens), reads nothing past its removal.
        let revoke = |member: &Id| {
            tx.execute(
                "UPDATE device SET state = 'revoked',
                   revoked_seq = MIN(COALESCE(revoked_seq, ?3), ?3)
                 WHERE account = ?1 AND member_id = ?2",
                params![account_hex, member.0.to_vec(), body.seq as i64],
            )
        };
        match &body.kind {
            Kind::Genesis(_) => {
                tx.execute(
                    "INSERT INTO account (id, created_ms) VALUES (?1, ?2)",
                    params![account_hex, now_ms() as i64],
                )?;
            }
            Kind::AddDevice(add) => {
                if let Some(token) = pending {
                    tx.execute(
                        "UPDATE device SET member_id = ?2, state = 'active'
                         WHERE token_hash = ?1 AND state = 'pending'",
                        params![token, add.keys.id().0.to_vec()],
                    )?;
                }
            }
            Kind::AddKit(kit) => {
                tx.execute(
                    "INSERT INTO recovery (lookup_hash, account, auth_hash, kit_id)
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT(lookup_hash) DO NOTHING",
                    params![
                        kit.lookup_hash.0.to_vec(),
                        account_hex,
                        kit.auth_hash.0.to_vec(),
                        kit.keys.id().0.to_vec()
                    ],
                )?;
            }
            Kind::Revoke(r) => {
                for id in &r.dropped {
                    revoke(id)?;
                    tx.execute(
                        "DELETE FROM recovery WHERE account = ?1 AND kit_id = ?2",
                        params![account_hex, id.0.to_vec()],
                    )?;
                }
                tx.execute(
                    "UPDATE account SET epoch = ?2, rotation_pending = 0 WHERE id = ?1",
                    params![account_hex, r.new_epoch],
                )?;
            }
            Kind::Leave(_) => {
                revoke(&body.author)?;
                tx.execute(
                    "UPDATE account SET rotation_pending = 1 WHERE id = ?1",
                    params![account_hex],
                )?;
            }
        }
        stored.push(entry.clone());
    }
    Ok(Ok(stored))
}

fn roster_head(db: &Connection, account: &Id) -> rusqlite::Result<Option<RosterHead>> {
    db.query_row(
        "SELECT seq, hash FROM roster WHERE account = ?1 ORDER BY seq DESC LIMIT 1",
        params![hex::encode(account.0)],
        |r| {
            let hash: Vec<u8> = r.get(1)?;
            Ok(RosterHead {
                seq: r.get::<_, i64>(0)? as u64,
                hash: Hex(hash.try_into().unwrap_or([0; 32])),
            })
        },
    )
    .optional()
}

#[derive(Deserialize)]
struct RosterQuery {
    #[serde(default)]
    from: u64,
}

/// Roster entries from `from`. A revoked token reads only up to the entry
/// that removed its device, so a dropped device learns it was dropped and
/// nothing after.
async fn read_roster(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RosterQuery>,
) -> Response {
    let db = lock(&state);
    let Some(auth) = authorize(&db, &headers) else {
        return error(StatusCode::UNAUTHORIZED, "unknown token");
    };
    let page = (|| -> rusqlite::Result<RosterPage> {
        let last = auth.revoked_seq.map_or(i64::MAX, |s| s as i64);
        let mut stmt = db.prepare(
            "SELECT body, sig FROM roster WHERE account = ?1 AND seq >= ?2 AND seq <= ?3
             ORDER BY seq LIMIT ?4",
        )?;
        let mut entries = stmt
            .query_map(
                params![
                    hex::encode(auth.account.0),
                    query.from as i64,
                    last,
                    ROSTER_PAGE as i64 + 1
                ],
                |r| Ok(stored_entry(r.get(0)?, r.get(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = entries.len() > ROSTER_PAGE;
        entries.truncate(ROSTER_PAGE);
        let head = match auth.revoked_seq {
            Some(_) => None,
            None => roster_head(&db, &auth.account)?,
        };
        Ok(RosterPage {
            from: query.from,
            entries,
            head,
            more,
        })
    })();
    match page {
        Ok(page) => Json(page).into_response(),
        Err(e) => storage_error("read roster", e),
    }
}

/// One entry, checked with the roster rules. An active token may append any
/// entry but a genesis; a pending one only its own device's `addDevice`,
/// which makes it active.
async fn append_roster(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AppendRequest>,
) -> Response {
    let mut db = lock(&state);
    let auth = authorize(&db, &headers);
    let kind = request.entry.parse().map(|b| b.kind);
    let pending = match (&auth, &kind) {
        (Some(a), Ok(Kind::AddDevice(_))) if a.state == TokenState::Pending => {
            Some(a.token.clone())
        }
        _ => None,
    };
    let auth = match pending {
        Some(_) => auth.expect("a pending token"),
        None => match active(auth) {
            Ok(auth) => auth,
            Err(refusal) => return *refusal,
        },
    };
    if matches!(kind, Ok(Kind::Genesis(_))) {
        return error(StatusCode::BAD_REQUEST, "an account has one genesis");
    }
    if let Some(stopped) = journal_stopped(&state) {
        return stopped;
    }
    let appended = (|| {
        let tx = db.transaction()?;
        let stored = append_entries(
            &tx,
            &auth.account,
            &[request.entry],
            pending.as_deref(),
            true,
        )?;
        if stored.is_ok() {
            tx.commit()?;
        }
        Ok::<_, rusqlite::Error>(stored)
    })();
    answer_append(&state, &db, &auth.account, appended)
}

fn answer_append(
    state: &AppState,
    db: &Connection,
    account: &Id,
    appended: rusqlite::Result<Result<Vec<Entry>, Refused>>,
) -> Response {
    match appended {
        Ok(Ok(stored)) => {
            if let Err(e) = journal(state, account, &stored) {
                return journal_error(e);
            }
            match roster_head(db, account) {
                Ok(head) => Json(json!({ "head": head })).into_response(),
                Err(e) => storage_error("roster head", e),
            }
        }
        Ok(Err(refused)) => refused.into_response(),
        Err(e) => storage_error("append roster", e),
    }
}

/// Entries a device holds and this relay lost (it was restored from a
/// backup). Needs no token: every entry carries members' signatures and
/// must continue the head, so this only ever repairs history.
async fn replay_roster(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<ReplayRequest>,
) -> Response {
    let address = match client(&state, peer, &headers) {
        Ok(address) => address,
        Err(refused) => return refused,
    };
    if limited(&state, address, REPLAY) {
        return error(StatusCode::TOO_MANY_REQUESTS, "too many replays");
    }
    let mut db = lock(&state);
    // A read that needs no journal: devices learn the account is gone also
    // while roster writes are stopped.
    match is_gone(&db, &request.account) {
        Ok(true) => return account_gone(),
        Ok(false) => {}
        Err(e) => return storage_error("replay roster", e),
    }
    if let Some(stopped) = journal_stopped(&state) {
        return stopped;
    }
    let exists: rusqlite::Result<Option<i64>> = db
        .query_row(
            "SELECT 1 FROM account WHERE id = ?1",
            params![hex::encode(request.account.0)],
            |r| r.get(0),
        )
        .optional();
    match exists {
        // Replaying an account from its genesis creates it.
        Ok(None) if !may_replay_create(&state, &headers, address) => {
            return error(StatusCode::FORBIDDEN, "an invite is required");
        }
        Ok(_) => {}
        Err(e) => return storage_error("replay roster", e),
    }
    let appended = (|| {
        let tx = db.transaction()?;
        let stored = append_entries(&tx, &request.account, &request.entries, None, false)?;
        if stored.is_ok() {
            tx.commit()?;
        }
        Ok::<_, rusqlite::Error>(stored)
    })();
    answer_append(&state, &db, &request.account, appended)
}

/// A fresh token for a current device of the account, which signs the
/// request with its device key: how a device whose token a restore lost
/// gets back in.
async fn fresh_token(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(grant): Json<TokenGrant>,
) -> Response {
    let address = match client(&state, peer, &headers) {
        Ok(address) => address,
        Err(refused) => return refused,
    };
    if limited(&state, address, TOKENS) {
        return error(StatusCode::TOO_MANY_REQUESTS, "too many token requests");
    }
    let now = state.now_ms();
    let request = &grant.request;
    let refused = || error(StatusCode::FORBIDDEN, "token request refused");
    let host = state.config.public_host.as_ref();
    if request.relay_id != state.relay_id
        || request.at_ms.abs_diff(now) > TOKEN_SKEW_MS
        || host.is_some_and(|host| *host != request.host)
    {
        return refused();
    }
    let mut db = lock(&state);
    match is_gone(&db, &request.account) {
        Ok(true) => return account_gone(),
        Ok(false) => {}
        Err(e) => return storage_error("fresh token", e),
    }
    let granted = (|| -> rusqlite::Result<Option<String>> {
        let tx = db.transaction()?;
        let entries = account_entries(&tx, &request.account)?;
        let mut roster = Roster::default();
        let mut keys = None;
        let mut removed = None;
        for (seq, entry) in entries.iter().enumerate() {
            if roster.apply(entry).is_err() {
                break;
            }
            match roster.members().get(&request.member) {
                Some(m) if m.role == Role::Device => keys = Some(m.keys),
                _ if keys.is_some() && removed.is_none() => removed = Some(seq as i64),
                _ => {}
            }
        }
        if !keys.is_some_and(|k| request.verifies(&k, &grant.sig)) {
            return Ok(None);
        }
        tx.execute(
            "DELETE FROM token_nonce WHERE expires_ms < ?1",
            params![now as i64],
        )?;
        let used = tx.execute(
            "INSERT INTO token_nonce (nonce, expires_ms) VALUES (?1, ?2)
             ON CONFLICT(nonce) DO NOTHING",
            params![request.nonce.0.to_vec(), (now + 2 * TOKEN_SKEW_MS) as i64],
        )? == 0;
        if used {
            return Ok(None);
        }
        let token = mint(&tx, &request.account, Some(&request.member), "active", true)?;
        // A device the roster dropped gets a token that reads up to its
        // removal and nothing else, so it can learn that it was dropped
        // even after a restore lost its old token.
        if let Some(seq) = removed {
            tx.execute(
                "UPDATE device SET state = 'revoked', revoked_seq = ?2 WHERE token_hash = ?1",
                params![token_hash(&token), seq],
            )?;
        }
        // At most a few such tokens per device: a loop that keeps asking
        // does not pile them up. One revoked here reads the roster up to
        // today's head, never what comes later.
        tx.execute(
            "UPDATE device SET state = 'revoked', revoked_seq = ?4 WHERE token_hash IN (
                 SELECT token_hash FROM device
                 WHERE account = ?1 AND member_id = ?2 AND fresh = 1 AND state = 'active'
                 ORDER BY created_ms DESC, rowid DESC LIMIT -1 OFFSET ?3)",
            params![
                hex::encode(request.account.0),
                request.member.0.to_vec(),
                FRESH_TOKENS,
                entries.len() as i64 - 1
            ],
        )?;
        tx.commit()?;
        Ok(Some(token))
    })();
    match granted {
        Ok(Some(token)) => Json(AccountResponse {
            account: hex::encode(request.account.0),
            device_token: token,
        })
        .into_response(),
        Ok(None) => refused(),
        Err(e) => storage_error("fresh token", e),
    }
}

/// A record as stored: checked fields, decoded body.
struct Checked {
    coll: u8,
    key_id: Vec<u8>,
    hlc: String,
    epoch: u32,
    body: Vec<u8>,
}

fn check(record: &WireRecord, server_ms: u64) -> Result<Checked, (Refusal, String)> {
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

/// The account's epoch and whether a rotation is pending.
fn account_epoch(db: &Connection, account: &Id) -> rusqlite::Result<(u32, bool)> {
    db.query_row(
        "SELECT epoch, rotation_pending FROM account WHERE id = ?1",
        params![hex::encode(account.0)],
        |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)),
    )
}

async fn push(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PushRequest>,
) -> Response {
    if request.records.len() > PAGE {
        return error(StatusCode::BAD_REQUEST, "too many records in one push");
    }
    // A whole addon list (coll 1) comes only from a build older than the
    // per-addon merge (plans/accounts-social-sync.md 13); taken, it would
    // undo other devices' addons on any device that read it. That device
    // updates first.
    if request.records.iter().any(|r| r.coll == 1) {
        // The count and the app's own build tag (its user agent), nothing
        // of the person or the address.
        let n = state.old_lists.fetch_add(1, Ordering::Relaxed) + 1;
        let build: String = headers
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .chars()
            .take(64)
            .collect();
        tracing::warn!(
            "refused whole addon list {n} since start, from {build}: an app older than the per-addon merge"
        );
        return identity::coded(
            StatusCode::BAD_REQUEST,
            "oldBuild",
            "this app is older than its sync service: update it to sync",
        );
    }
    let server_ms = state.now_ms();
    let mut db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    let account = hex::encode(auth.account.0);
    let result = (|| -> rusqlite::Result<Result<PushResponse, Response>> {
        let tx = db.transaction()?;
        // Pushes go under the account's current key and roster only: a
        // device behind on either is told so and catches up first, with
        // nothing taken from it.
        let (epoch, rotation_pending) = account_epoch(&tx, &auth.account)?;
        let head = roster_head(&tx, &auth.account)?;
        let behind = if rotation_pending {
            Some("a member left; the next device rotates the key first")
        } else if head.is_none_or(|h| h.hash != request.head) {
            Some("the roster moved; catch up first")
        } else if request.records.iter().any(|r| r.epoch != epoch) {
            Some("records must be sealed under the account's current key")
        } else {
            None
        };
        if let Some(why) = behind {
            let body = Behind {
                error: why.to_owned(),
                epoch,
                roster: head,
            };
            return Ok(Err((StatusCode::CONFLICT, Json(body)).into_response()));
        }
        let (mut seq, mut bytes): (i64, i64) = tx.query_row(
            "SELECT seq, bytes FROM account WHERE id = ?1",
            params![account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut response = PushResponse {
            accepted: 0,
            stale: 0,
            stale_at: Vec::new(),
            rejected: Vec::new(),
            server_ms,
            epoch,
            roster: head,
            deletion: deletion_due(&state, &tx, &auth.account)?,
        };
        for record in &request.records {
            let r = match check(record, server_ms) {
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
                    // A register's own history bounds it too, so a version just
                    // above the stored one lands even when that one is ahead of
                    // the relay (written while the relay's clock ran fast). Not
                    // past the stored wall time: pushes cannot walk a register
                    // further ahead.
                    let stored_wall = current.as_deref().and_then(Hlc::decode).map(|h| h.wall_ms);
                    let wall = Hlc::decode(&record.hlc).map(|h| h.wall_ms);
                    match stored_wall
                        .filter(|&stored| wall.is_some_and(|wall| wall <= stored))
                        .map(|stored| check(record, stored))
                    {
                        Some(Ok(r)) => r,
                        _ => {
                            response.rejected.push(Rejected {
                                key_id: record.key_id.clone(),
                                reason,
                                code,
                                current,
                            });
                            continue;
                        }
                    }
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
                if let Some(devices) = clock.ahead(auth.token, server_ms) {
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
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    let account = hex::encode(auth.account.0);
    let limit = query.limit.unwrap_or(PAGE).clamp(1, PAGE);
    type Rows = (
        Vec<WireRecord>,
        u64,
        u32,
        Option<RosterHead>,
        Option<ScheduledDeletion>,
    );
    let rows = (|| -> rusqlite::Result<Rows> {
        let head: i64 = db.query_row(
            "SELECT seq FROM account WHERE id = ?1",
            params![account],
            |r| r.get(0),
        )?;
        let (epoch, _) = account_epoch(&db, &auth.account)?;
        let roster = roster_head(&db, &auth.account)?;
        let mut stmt = db.prepare(
            "SELECT coll, key_id, hlc, epoch, body, seq FROM record
             WHERE account = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        let records = stmt
            .query_map(
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
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((
            records,
            head as u64,
            epoch,
            roster,
            deletion_due(&state, &db, &auth.account)?,
        ))
    })();
    match rows {
        Ok((mut records, head, epoch, roster, deletion)) => {
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
                epoch,
                roster,
                deletion,
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

/// Request budgets for the endpoints that need no token, per client address
/// and relay-wide, so one script can neither flood the relay nor block
/// everyone else. Windows are one minute: (window start, requests).
#[derive(Default)]
pub(crate) struct Limits {
    per_client: HashMap<(usize, IpAddr), (u64, u32)>,
    global: [(u64, u32); 6],
}

const OPEN_PAIRING: (usize, u32, u32) = (0, 20, 2000);
const REDEEM_KIT: (usize, u32, u32) = (1, 10, 600);
const REPLAY: (usize, u32, u32) = (2, 20, 2000);
const TOKENS: (usize, u32, u32) = (3, 20, 2000);
/// Sign-in `start`, `take`, `token` and the deletion page's request, apart
/// from pairings (a TV polls pairing `take` every 2 s, unlimited).
const AUTH: (usize, u32, u32) = (4, 30, 3000);
/// Apple's server-to-server notifications: Apple sends a few a day. The
/// per-address budget applies first; the relay-wide one counts only notices
/// whose signature checked, so a flood from many addresses cannot crowd out
/// Apple's own.
pub(crate) const APPLE_NOTICES: (usize, u32, u32) = (5, 10, 60);

/// The account's scheduled deletion, if one waits, with the provider of the
/// identity that asked (none for a Start over, or once that link is gone),
/// named as the apps name it.
fn deletion_due(
    state: &AppState,
    db: &Connection,
    account: &Id,
) -> rusqlite::Result<Option<ScheduledDeletion>> {
    db.query_row(
        "SELECT d.due_ms, i.provider FROM deletion d \
         LEFT JOIN identity i ON i.link = d.link AND i.account = d.account \
         WHERE d.account = ?1",
        params![hex::encode(account.0)],
        |r| {
            let provider: Option<String> = r.get(1)?;
            Ok(ScheduledDeletion {
                due_ms: r.get::<_, i64>(0)? as u64,
                provider: provider.map(|p| match state.identity.as_deref() {
                    Some(id) => id.display(&p),
                    None => p,
                }),
            })
        },
    )
    .optional()
}

fn spend(slot: &mut (u64, u32), now: u64, cap: u32) -> bool {
    if now >= slot.0 + 60_000 {
        *slot = (now, 0);
    }
    slot.1 += 1;
    slot.1 > cap
}

/// Whether `client` is over its budget (or the relay over its own). A
/// client over its own budget does not spend the relay's, so one flooding
/// address cannot block everyone else.
fn limited(state: &AppState, client: IpAddr, slot: (usize, u32, u32)) -> bool {
    client_limited(state, client, slot) || relay_limited(state, slot)
}

/// The client's own budget alone.
pub(crate) fn client_limited(
    state: &AppState,
    client: IpAddr,
    (which, per_client, _): (usize, u32, u32),
) -> bool {
    // Relay time, as every other window here (tests move it).
    let now = state.now_ms();
    let mut limits = state.limits.lock().unwrap_or_else(|p| p.into_inner());
    if limits.per_client.len() > 50_000 {
        limits
            .per_client
            .retain(|_, (start, _)| now < *start + 60_000);
    }
    spend(
        limits.per_client.entry((which, client)).or_default(),
        now,
        per_client,
    )
}

/// The relay-wide budget alone: for requests shown genuine first, so
/// strangers cannot spend it (Apple's notices).
pub(crate) fn relay_limited(state: &AppState, (which, _, global): (usize, u32, u32)) -> bool {
    let now = state.now_ms();
    let mut limits = state.limits.lock().unwrap_or_else(|p| p.into_inner());
    spend(&mut limits.global[which], now, global)
}

/// The caller's address for rate limits. Without a proxy, the peer. Behind
/// the trusted proxy (`--trust-proxy`, Cloudflare's tunnel): Cloudflare's
/// own `CF-Connecting-IP`, which a client cannot set, else the last
/// `X-Forwarded-For` hop (the one the proxy appended; earlier hops are
/// whatever the client wrote); with neither, the request is refused, since
/// the peer is then the proxy and every client would share one budget.
/// IPv6 counts per /64, which one client usually holds whole.
// The refusal is the answer itself, as in identity.rs's handlers.
#[allow(clippy::result_large_err)]
fn client(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> Result<IpAddr, Response> {
    let header = |name: &str| headers.get(name)?.to_str().ok();
    let ip = match state.config.trust_proxy {
        true => header("cf-connecting-ip")
            .and_then(|v| v.trim().parse().ok())
            .or_else(|| {
                header("x-forwarded-for")?
                    .rsplit(',')
                    .next()?
                    .trim()
                    .parse()
                    .ok()
            })
            .ok_or_else(|| {
                identity::coded(
                    StatusCode::BAD_REQUEST,
                    "noClientAddress",
                    "the proxy in front of this relay named no client address",
                )
            })?,
        false => peer.ip(),
    };
    Ok(match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6((u128::from(v6) & !u128::from(u64::MAX)).into()),
        },
        v4 => v4,
    })
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

/// A one-time slot a new device shows as a QR. The QR carries the slot id
/// (for the device that fills it); collecting needs the `take` secret, which
/// stays on the new device.
async fn open_pairing(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let address = match client(&state, peer, &headers) {
        Ok(address) => address,
        Err(refused) => return refused,
    };
    // Coded `busy`, so the apps say to try again in a moment.
    if limited(&state, address, OPEN_PAIRING) {
        return identity::coded(
            StatusCode::TOO_MANY_REQUESTS,
            "busy",
            "too many pairings started",
        );
    }
    let now = state.now_ms();
    let db = lock(&state);
    let mut open_pairings = state
        .open_pairings
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if open_pairings.len() > 50_000 {
        open_pairings.retain(|_, held| held.iter().any(|(_, expires_ms)| *expires_ms > now));
    }
    // The address's slots still open: neither ended nor taken.
    let held = open_pairings.entry(address).or_default();
    held.retain(|(id, expires_ms)| {
        *expires_ms > now
            && db
                .query_row("SELECT 1 FROM pairing WHERE id = ?1", params![id], |_| {
                    Ok(())
                })
                .optional()
                .is_ok_and(|row| row.is_some())
    });
    if held.len() >= PAIRINGS_PER_CLIENT {
        return identity::coded(
            StatusCode::TOO_MANY_REQUESTS,
            "busy",
            "too many pairings open from this address",
        );
    }
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
            held.push((id.clone(), expires_ms));
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
    let now = state.now_ms();
    let db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    let Some(sealed) = small_blob(&blob.sealed) else {
        return error(
            StatusCode::BAD_REQUEST,
            "sealed blob has a bad size or encoding",
        );
    };
    match db.execute(
        "UPDATE pairing SET sealed = ?2, account = ?3
         WHERE id = ?1 AND sealed IS NULL AND expires_ms > ?4",
        params![id, sealed, hex::encode(auth.account.0), now as i64],
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
    /// Collect a token too (the user confirmed); otherwise only the blob.
    #[serde(default)]
    claim: bool,
}

/// take hash, sealed blob, account, expiry
type PairingRow = (Vec<u8>, Option<Vec<u8>>, Option<String>, i64);

/// The new device collects the blob with its take secret: 204 while
/// waiting, then the blob (as often as asked), then, with `claim` once the
/// user confirmed the codes, a pending token for the account that filled
/// the slot, which becomes active when the device's own `addDevice` lands;
/// after that 404.
async fn take_pairing(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    Json(claim): Json<Take>,
) -> Response {
    let db = lock(&state);
    let row: rusqlite::Result<Option<PairingRow>> = db
        .query_row(
            "SELECT take_hash, sealed, account, expires_ms FROM pairing WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional();
    let (take_hash, sealed, account, expires) = match row {
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
    let (Some(sealed), Some(account)) = (sealed, account.as_deref().and_then(account_id)) else {
        return StatusCode::NO_CONTENT.into_response();
    };
    if !claim.claim {
        return Json(json!({ "sealed": B64.encode(sealed) })).into_response();
    }
    // The user confirmed the codes match: now, and only now, the new
    // device gets a token, which can do nothing but join.
    let claimed = (|| -> rusqlite::Result<String> {
        let token = mint(&db, &account, None, "pending", false)?;
        db.execute("DELETE FROM pairing WHERE id = ?1", params![id])?;
        Ok(token)
    })();
    match claimed {
        Ok(token) => Json(AccountResponse {
            account: hex::encode(account.0),
            device_token: token,
        })
        .into_response(),
        Err(e) => storage_error("claim pairing", e),
    }
}

fn lookup_hash(lookup: &str) -> Option<Vec<u8>> {
    hex::decode(lookup)
        .ok()
        .filter(|l| l.len() == 16)
        .map(|l| Sha256::digest(l).to_vec())
}

#[derive(Deserialize)]
struct Redeem {
    lookup: String,
    /// The kit's redeem secret, hex. Only its hash is stored.
    auth: String,
}

/// account, redeem secret hash, kit id
type KitRow = (String, Vec<u8>, Vec<u8>);

/// Joins the account of a kit as a new device: needs the kit's redeem
/// secret, which no device keeps. The token is pending until the device's
/// own `addDevice`, vouched for by the kit, lands.
async fn redeem_kit(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(redeem): Json<Redeem>,
) -> Response {
    let address = match client(&state, peer, &headers) {
        Ok(address) => address,
        Err(refused) => return refused,
    };
    if limited(&state, address, REDEEM_KIT) {
        return error(StatusCode::TOO_MANY_REQUESTS, "too many recovery attempts");
    }
    let not_found = || {
        identity::coded(
            StatusCode::NOT_FOUND,
            "kitUnknown",
            "no recovery data for that kit",
        )
    };
    let (Some(hash), Some(auth)) = (lookup_hash(&redeem.lookup), hex::decode(&redeem.auth).ok())
    else {
        return not_found();
    };
    let db = lock(&state);
    let found: rusqlite::Result<Option<KitRow>> = db
        .query_row(
            "SELECT account, auth_hash, kit_id FROM recovery WHERE lookup_hash = ?1",
            params![hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional();
    let (account, auth_hash, kit) = match found {
        Ok(Some(found)) => found,
        Ok(None) => return not_found(),
        Err(e) => return storage_error("redeem kit", e),
    };
    if !same(&auth_hash, &Sha256::digest(&auth)) {
        return not_found();
    }
    let (Some(account), Ok(kit)) = (account_id(&account), <[u8; 16]>::try_from(kit)) else {
        return not_found();
    };
    match mint(&db, &account, None, "pending", false) {
        Ok(token) => Json(Redeemed {
            account,
            device_token: token,
            kit: Hex(kit),
        })
        .into_response(),
        Err(e) => storage_error("redeem kit", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Once a journal write failed, no roster write is stored until a
    /// restart has written the journal again: a gap could never be filled.
    #[tokio::test]
    async fn after_a_failed_journal_write_no_roster_write_is_taken() {
        use mq_sync::roster::MemberSecret;
        let dir = std::env::temp_dir().join(format!("mq-journal-{}", random_hex(8)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay.roster.jsonl");
        std::fs::write(&path, "").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        let (conn, generation, since, relay_id) = open_database(conn, &Config::default()).unwrap();
        let state = AppState {
            db: Arc::new(Mutex::new(conn)),
            // Open for reading only: every journal write fails.
            journal: Arc::new(Mutex::new(std::fs::File::open(&path).unwrap())),
            journal_path: Arc::new(path.clone()),
            journal_failed: Arc::default(),
            config: Arc::new(Config::default()),
            generation: Arc::new(generation),
            generation_since_ms: since,
            relay_id,
            limits: Arc::default(),
            clock: Arc::default(),
            identity: None,
            grace_created: Arc::default(),
            open_pairings: Arc::default(),
            old_lists: Arc::default(),
        };
        let create = || {
            let (entry, _) =
                Roster::genesis_entry(&MemberSecret::generate(), Hex(random()), "a", now_ms());
            create_account(
                State(state.clone()),
                HeaderMap::new(),
                Json(AccountRequest {
                    entry,
                    ticket: None,
                    move_link: false,
                }),
            )
        };
        let first = create().await.status();
        assert_eq!(first, StatusCode::INTERNAL_SERVER_ERROR);
        let second = create().await.status();
        assert_eq!(second, StatusCode::SERVICE_UNAVAILABLE);
        let accounts: i64 = lock(&state)
            .query_row("SELECT COUNT(*) FROM account", [], |r| r.get(0))
            .unwrap();
        assert_eq!(accounts, 1, "the second was never stored");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A deletion whose journal rewrite fails still stands: 200, the
    /// tombstone stored and the `gone` line on the old journal; roster writes
    /// stop, a replay still hears the account is gone, and the next start on
    /// this database and that journal keeps only the `gone` line.
    #[tokio::test]
    async fn a_deletion_whose_journal_rewrite_fails_still_stands() {
        use mq_sync::roster::MemberSecret;
        let dir = std::env::temp_dir().join(format!("mq-journal-{}", random_hex(8)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay.roster.jsonl");
        let conn = Connection::open_in_memory().unwrap();
        let (conn, generation, since, relay_id) = open_database(conn, &Config::default()).unwrap();
        let state = AppState {
            journal: Arc::new(Mutex::new(open_journal(&path, &conn).unwrap())),
            db: Arc::new(Mutex::new(conn)),
            // The rewrite cannot make its temp file in a directory that is not there.
            journal_path: Arc::new(dir.join("missing").join("relay.roster.jsonl")),
            journal_failed: Arc::default(),
            config: Arc::new(Config::default()),
            generation: Arc::new(generation),
            generation_since_ms: since,
            relay_id,
            limits: Arc::default(),
            clock: Arc::default(),
            identity: None,
            grace_created: Arc::default(),
            open_pairings: Arc::default(),
            old_lists: Arc::default(),
        };
        let member = MemberSecret::generate();
        let genesis = || Roster::genesis_entry(&member, Hex(random()), "a", now_ms()).0;
        let create = |entry| {
            create_account(
                State(state.clone()),
                HeaderMap::new(),
                Json(AccountRequest {
                    entry,
                    ticket: None,
                    move_link: false,
                }),
            )
        };
        let entry = genesis();
        let account = entry.parse().unwrap().account;
        let created = create(entry.clone()).await;
        assert_eq!(created.status(), StatusCode::OK);
        let body = axum::body::to_bytes(created.into_body(), usize::MAX)
            .await
            .unwrap();
        let token = serde_json::from_slice::<AccountResponse>(&body)
            .unwrap()
            .device_token;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );

        let request = TokenRequest::new(state.relay_id, account, member.id(), now_ms(), "any");
        let asked = DeleteRequest {
            sig: member.sign_delete(&request),
            request,
            receipt: None,
        };
        assert_eq!(
            delete_account(State(state.clone()), headers, Json(asked))
                .await
                .status(),
            StatusCode::OK
        );
        assert!(is_gone(&lock(&state), &account).unwrap());
        let journal = std::fs::read_to_string(&path).unwrap();
        assert!(
            journal.lines().last().unwrap().contains("\"gone\""),
            "{journal}"
        );
        assert_eq!(
            create(genesis()).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let peer = SocketAddr::from(([127, 0, 0, 1], 1));
        let replay = replay_roster(
            State(state.clone()),
            ConnectInfo(peer),
            HeaderMap::new(),
            Json(ReplayRequest {
                account,
                entries: vec![entry],
            }),
        )
        .await;
        assert_eq!(replay.status(), StatusCode::GONE);

        drop(open_journal(&path, &lock(&state)).unwrap());
        let journal = std::fs::read_to_string(&path).unwrap();
        assert_eq!(journal.lines().count(), 1, "{journal}");
        assert!(journal.contains("\"gone\""), "{journal}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
