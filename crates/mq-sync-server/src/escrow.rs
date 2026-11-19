//! Escrow mode (`--escrow`): the relay keeps the secret of one recovery kit
//! per account, sealed at rest under the identity file's `escrowKey`, and
//! gives it back after a completed sign-in for that account, so a new device
//! joins with the sign-in alone. The operator can then open the account;
//! that is the trade an app makes by running a relay with this flag.
//! Momotus's relay runs without it.
//!
//! The rules that keep a sign-in working:
//! - no roster entry a device sends (`/v1/roster`, `/v1/roster/replay`)
//!   drops the escrowed kit, directly or as its maker's revoke or leave
//!   (`append_entries`); only the hour after a restore lets a replay of
//!   history drop it, and then the escrow row goes with it;
//! - the account's first sign-in link is never unlinked or moved
//!   (`first_link`);
//! - a deletion asked from an app waits 7 days like the web page's
//!   (`delete_account`);
//! - a device can see when the escrowed kit is gone from the roster or was
//!   put before a rekey (`status`), and puts a new one (`mq_sync::escrow::ensure`).

use crate::IdentityFile;
use crate::identity::{coded, identity_link, ticket, ticket_refused};
use crate::{
    AppState, active, authorize, client, error, limited, load_roster, lock, storage_error,
};
use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use mq_sync::escrow::{EscrowKey, seal_for_taker};
use mq_sync::roster::{Hex, Id, Role};
use mq_sync::wire::{EscrowInfo, EscrowPut, EscrowStatus, EscrowTake, EscrowTaken};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::json;
use std::net::SocketAddr;
use std::path::Path;

/// The escrow key, in escrow mode.
pub(crate) fn key(state: &AppState) -> Option<&EscrowKey> {
    state.identity.as_deref()?.escrow.as_ref()
}

fn no_escrow() -> Response {
    error(StatusCode::NOT_FOUND, "this relay keeps no escrow")
}

/// The relay's escrow public key.
pub(crate) async fn info(State(state): State<AppState>) -> Response {
    match key(&state) {
        Some(key) => Json(EscrowInfo {
            public_key: key.public_key(),
        })
        .into_response(),
        None => no_escrow(),
    }
}

/// The escrow generation: `rekey` raises it, and a kit put before is stale.
fn generation(db: &Connection) -> rusqlite::Result<i64> {
    let stored: Option<String> = db
        .query_row(
            "SELECT value FROM meta WHERE key = 'escrow_generation'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(stored.and_then(|g| g.parse().ok()).unwrap_or(0))
}

/// Whether `kit` is a kit of `account`'s roster now.
fn is_kit(db: &Connection, account: &Id, kit: &Id) -> rusqlite::Result<bool> {
    let roster = load_roster(db, account)?;
    Ok(roster
        .members()
        .get(kit)
        .is_some_and(|m| m.role == Role::Kit))
}

/// A device of the account puts a kit secret in escrow, replacing the one
/// held before. The kit must be a member of the account's roster.
pub(crate) async fn put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(put): Json<EscrowPut>,
) -> Response {
    let Some(key) = key(&state) else {
        return no_escrow();
    };
    let db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    let opened = B64
        .decode(&put.sealed)
        .ok()
        .and_then(|sealed| key.open_put(&auth.account, &sealed));
    let Some((kit, kit_id)) = opened else {
        return error(
            StatusCode::BAD_REQUEST,
            "not a kit sealed to this relay's escrow key",
        );
    };
    let stored = (|| -> rusqlite::Result<bool> {
        if !is_kit(&db, &auth.account, &kit_id)? {
            return Ok(false);
        }
        db.execute(
            "INSERT INTO escrow (account, kit_id, sealed, generation) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account) DO UPDATE SET kit_id = excluded.kit_id, sealed = excluded.sealed,
               generation = excluded.generation",
            params![
                hex::encode(auth.account.0),
                kit_id.0.to_vec(),
                key.seal_at_rest(&auth.account, &kit_id, &kit),
                generation(&db)?
            ],
        )?;
        Ok(true)
    })();
    match stored {
        Ok(true) => Json(json!({})).into_response(),
        Ok(false) => error(
            StatusCode::FORBIDDEN,
            "this kit is not a kit of the account",
        ),
        Err(e) => storage_error("escrow put", e),
    }
}

/// What the token's account holds in escrow: the kit, whether it is still a
/// kit of the roster, and whether it was put before the last rekey.
pub(crate) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if key(&state).is_none() {
        return no_escrow();
    }
    let db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    let read = (|| -> rusqlite::Result<EscrowStatus> {
        let row: Option<(Vec<u8>, i64)> = db
            .query_row(
                "SELECT kit_id, generation FROM escrow WHERE account = ?1",
                params![hex::encode(auth.account.0)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((kit, put_in)) = row else {
            return Ok(EscrowStatus::default());
        };
        let kit_id = kit.try_into().ok().map(Hex);
        Ok(EscrowStatus {
            current: match &kit_id {
                Some(kit) => is_kit(&db, &auth.account, kit)?,
                None => false,
            },
            stale: put_in < generation(&db)?,
            kit_id,
        })
    })();
    match read {
        Ok(status) => Json(status).into_response(),
        Err(e) => storage_error("escrow status", e),
    }
}

/// A completed sign-in's ticket, spent for its account's escrowed kit,
/// sealed to the taker's key. No account, no kit, or a kit no longer in the
/// roster keeps the ticket.
pub(crate) async fn take(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(take): Json<EscrowTake>,
) -> Response {
    let Some(key) = key(&state) else {
        return no_escrow();
    };
    match client(&state, peer, &headers) {
        Ok(address) if limited(&state, address, crate::auth(&state)) => {
            return crate::identity::busy();
        }
        Ok(_) => {}
        Err(refused) => return refused,
    }
    let now = state.now_ms();
    let mut db = lock(&state);
    let taken = (|| -> rusqlite::Result<Result<String, Response>> {
        let tx = db.transaction()?;
        let Some((provider, subject)) = ticket(&tx, &take.ticket, now, false)? else {
            return Ok(Err(ticket_refused()));
        };
        let Some((_, account)) = identity_link(&tx, &provider, &subject)? else {
            return Ok(Err(error(
                StatusCode::NOT_FOUND,
                "this identity signs in to no account",
            )));
        };
        let id = crate::account_id(&account).expect("stored account ids are hex");
        let kit = match escrowed_kit(&tx, &account)? {
            Some(kit) if is_kit(&tx, &id, &kit)? => {
                let sealed: Vec<u8> = tx.query_row(
                    "SELECT sealed FROM escrow WHERE account = ?1",
                    params![account],
                    |r| r.get(0),
                )?;
                key.open_at_rest(&id, &kit, &sealed)
            }
            _ => None,
        };
        let Some(kit) = kit else {
            return Ok(Err(coded(
                StatusCode::NOT_FOUND,
                "noEscrow",
                "this account keeps no kit in escrow",
            )));
        };
        let Some(back) = seal_for_taker(&take.to, &kit) else {
            return Ok(Err(error(StatusCode::BAD_REQUEST, "not a usable key")));
        };
        ticket(&tx, &take.ticket, now, true)?;
        tx.commit()?;
        Ok(Ok(B64.encode(back)))
    })();
    match taken {
        Ok(Ok(sealed)) => Json(EscrowTaken { sealed }).into_response(),
        Ok(Err(refused)) => refused,
        Err(e) => storage_error("escrow take", e),
    }
}

/// The escrowed kit of `account`, if any: what a device's revoke or leave
/// may not drop.
pub(crate) fn escrowed_kit(db: &Connection, account: &str) -> rusqlite::Result<Option<Id>> {
    let kit: Option<Vec<u8>> = db
        .query_row(
            "SELECT kit_id FROM escrow WHERE account = ?1",
            params![account],
            |r| r.get(0),
        )
        .optional()?;
    Ok(kit.and_then(|k| k.try_into().ok()).map(Hex))
}

/// In escrow mode the account's first sign-in link stays: unlinking it, or
/// moving it to another account, would leave the listener's sign-in opening
/// nothing (or another account) and no way back in. The first, not the last:
/// a stolen session could add its own identity as a second link and then
/// remove the listener's. A link added or moved in later is never older
/// (`created_ms` is set then), and stays removable. The refusal, if `link`
/// is that link.
pub(crate) fn first_link(
    db: &Connection,
    escrow: bool,
    account: &str,
    link: i64,
) -> rusqlite::Result<Option<Response>> {
    if !escrow {
        return Ok(None);
    }
    let first: Option<i64> = db
        .query_row(
            "SELECT link FROM identity WHERE account = ?1 ORDER BY created_ms, link LIMIT 1",
            params![account],
            |r| r.get(0),
        )
        .optional()?;
    Ok((first == Some(link)).then(|| {
        coded(
            StatusCode::FORBIDDEN,
            "lastLink",
            "this is the account's first sign-in, which always opens it",
        )
    }))
}

/// `mq-sync-server rekey`: every escrow entry sealed again under `to`'s
/// escrow key, in one transaction, the database bound to it, and the escrow
/// generation raised, so every account's devices replace their kit (its
/// secret may be in the leaked copy) on their next `escrow::ensure`. The
/// relay must be stopped: a database another process has open is refused.
/// It then starts with `to` as its identity file. Returns how many entries
/// moved.
pub fn rekey(db: &Path, from: &IdentityFile, to: &IdentityFile) -> Result<usize, String> {
    let key = |file: &IdentityFile| {
        file.escrow()?
            .ok_or_else(|| "an identity file has no escrowKey".to_owned())
    };
    let (from, to) = (key(from)?, key(to)?);
    let fail = |e: rusqlite::Error| e.to_string();
    let mut conn = Connection::open(db).map_err(fail)?;
    // One holder at a time: a running relay keeps the file open, and an
    // exclusive lock then fails at once rather than racing its puts.
    conn.busy_timeout(std::time::Duration::ZERO).map_err(fail)?;
    conn.pragma_update(None, "locking_mode", "EXCLUSIVE")
        .map_err(fail)?;
    // The old entries are overwritten in the file, and the log emptied after.
    conn.pragma_update(None, "secure_delete", "ON")
        .map_err(fail)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Exclusive)
        .map_err(|e| format!("{e}: stop the relay first"))?;
    crate::check_key(
        &tx,
        "escrow_key",
        from.fingerprint(),
        "the old escrow key is not the one this database holds",
    )?;
    let rows: Vec<(String, Vec<u8>, Vec<u8>)> = {
        let mut stmt = tx
            .prepare("SELECT account, kit_id, sealed FROM escrow")
            .map_err(fail)?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .and_then(|rows| rows.collect())
            .map_err(fail)?
    };
    for (account, kit_id, sealed) in &rows {
        let id = crate::account_id(account).ok_or("an escrow row names no account")?;
        let kit_id = Hex(kit_id
            .clone()
            .try_into()
            .map_err(|_| "an escrow row names no kit")?);
        let kit = from
            .open_at_rest(&id, &kit_id, sealed)
            .ok_or_else(|| format!("the escrow entry of {account} does not open"))?;
        tx.execute(
            "UPDATE escrow SET sealed = ?2 WHERE account = ?1",
            params![account, to.seal_at_rest(&id, &kit_id, &kit)],
        )
        .map_err(fail)?;
    }
    let next = generation(&tx).map_err(fail)? + 1;
    tx.execute(
        "INSERT INTO meta (key, value) VALUES ('escrow_generation', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![next.to_string()],
    )
    .map_err(fail)?;
    tx.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'escrow_key'",
        params![to.fingerprint()],
    )
    .map_err(fail)?;
    tx.commit().map_err(fail)?;
    crate::checkpoint(&conn);
    Ok(rows.len())
}

/// Schedules `account`'s deletion in 7 days, unless one waits already
/// (that date stays). Returns when it is due.
pub(crate) fn schedule_deletion(tx: &Connection, account: &str, now: u64) -> rusqlite::Result<u64> {
    let due: Option<i64> = tx
        .query_row(
            "SELECT due_ms FROM deletion WHERE account = ?1",
            params![account],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(due) = due {
        return Ok(due as u64);
    }
    let due = now + crate::identity::DELETION_WAIT_MS;
    tx.execute(
        "INSERT INTO deletion (account, due_ms, link) VALUES (?1, ?2, NULL)",
        params![account, due as i64],
    )?;
    Ok(due)
}
