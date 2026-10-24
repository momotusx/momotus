//! Registers as sync records. Every synced row is a last-writer-wins register
//! with an HLC; export walks them in `(hlc, coll, key)` order so a cursor
//! never skips rows that share an HLC (an import writes thousands with one),
//! and apply runs the same upsert-if-newer SQL as local writes.

use crate::hlc::{Hlc, HlcClock};
use crate::policy::AllowedRow;
use crate::store::{AddonRow, Library, LibraryError, LibraryItemRow, ProgressRow, Table};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Collection numbers on the wire. Never renumber.
pub mod coll {
    pub const PROFILES: u8 = 0;
    /// A profile's whole addon list as one register, from older builds:
    /// never written or read since the list merges per addon (`ADDON`).
    pub const ADDONS: u8 = 1;
    pub const ITEMS: u8 = 2;
    pub const PROGRESS: u8 = 3;
    pub const WATCHED: u8 = 4;
    pub const SETTINGS: u8 = 5;
    pub const STREAM_MEMORY: u8 = 6;
    pub const PROFILE_LOCK: u8 = 7;
    pub const PROFILE_GONE: u8 = 8;
    /// Whether a profile is a kids profile (plans/m11-kids.md 3.1).
    pub const PROFILE_ACCESS: u8 = 9;
    /// One title of a kids profile's allowlist.
    pub const ALLOWED: u8 = 10;
    /// A title taken off Continue Watching and Next Up.
    pub const DISMISSED: u8 = 11;
    /// One addon of a profile's list: installed or not, its switches and
    /// manifest; removals kept as tombstones, so two devices editing the
    /// list each keep their edits (plans/accounts-social-sync.md 13).
    pub const ADDON: u8 = 12;
    /// One addon's place in the list, apart from its state, so a move and a
    /// switch of the same addon on two devices both stand.
    pub const ADDON_ORDER: u8 = 13;
}

/// Every collection this build reads and writes, in export order.
const ALL: [u8; 13] = [
    coll::PROFILES,
    coll::ADDON,
    coll::ADDON_ORDER,
    coll::ITEMS,
    coll::PROGRESS,
    coll::WATCHED,
    coll::SETTINGS,
    coll::STREAM_MEMORY,
    coll::PROFILE_LOCK,
    coll::PROFILE_GONE,
    coll::PROFILE_ACCESS,
    coll::ALLOWED,
    coll::DISMISSED,
];

/// The gap between two neighbouring addons' places (`AddonRow::position`):
/// room for an addon put between them without moving either.
pub(crate) const POSITION_STEP: i64 = 1024;

/// Separates the parts of a register key; ids never contain it.
const SEP: char = '\u{1f}';

/// The owner profile: every device starts with it, it can never be gone.
pub const OWNER: &str = "default";
/// The owner's name until someone renames it (the apps call it the main profile).
pub const OWNER_NAME: &str = "Main";

/// Avatars are an app's own ids (a bundled image and a colour token), and
/// they sync to every device: up to 64 ASCII letters, digits, ':', '-' or
/// '_' (empty means the default).
pub fn valid_avatar(avatar: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_');
    avatar.len() <= 64 && avatar.chars().all(allowed)
}

/// Whether the profile was removed for good.
pub(crate) fn gone(c: &Connection, profile: &str) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM profile_gone WHERE profile_id = ?1)",
        params![profile],
        |r| r.get(0),
    )
}

/// A write for a gone profile, refused (`Library::write` turns it into
/// `LibraryError::ProfileGone`).
#[derive(Debug)]
pub(crate) struct Gone(pub String);

impl std::fmt::Display for Gone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "profile {} was removed", self.0)
    }
}

impl std::error::Error for Gone {}

/// Fails the transaction when `profile` is gone, so no local writer (a
/// view, an import, a tracker round) can bring it back.
pub(crate) fn refuse_gone(c: &Connection, profile: &str) -> rusqlite::Result<()> {
    if gone(c, profile)? {
        return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(Gone(
            profile.to_owned(),
        ))));
    }
    Ok(())
}

/// Every row a profile has on this device, except its gone register.
pub(crate) const PROFILE_TABLES: [&str; 18] = [
    "profile_lock",
    "profile_access",
    "allowed",
    "addons",
    "addon_order",
    "addon_list",
    "library_items",
    "progress",
    "watched",
    "stream_memory",
    "dismissed",
    "settings",
    "search_history",
    "tracker_account",
    "tracker_outbox",
    "tracker_state",
    "pin_attempts",
    "import_raw",
];

/// Tables that hold no profile's rows: sync's own state, the device's, the
/// tombstones, `http_cache`, which is cleared whole when a profile goes
/// (its keys are addon URLs, which can carry a profile's keys and searches),
/// and `ratings`, cleared with it (its ids are titles the profile looked at).
#[cfg(test)]
pub(crate) const DEVICE_TABLES: [&str; 7] = [
    "sync_state",
    "sync_outbox",
    "sync_roster",
    "device_state",
    "profile_gone",
    "http_cache",
    "ratings",
];

/// Deletes every row of a gone profile.
pub(crate) fn purge(c: &Connection, profile: &str) -> rusqlite::Result<()> {
    c.execute("DELETE FROM profiles WHERE id = ?1", params![profile])?;
    for table in PROFILE_TABLES {
        c.execute(
            &format!("DELETE FROM {table} WHERE profile_id = ?1"),
            params![profile],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncRecord {
    pub coll: u8,
    /// Names the register; only its blind id leaves the device.
    pub key: String,
    pub value: Value,
    pub hlc: String,
}

/// Registers waiting to be pushed, with the newest outbox id read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutboxPage {
    /// Current value of each queued register, oldest queue entry first.
    pub records: Vec<(i64, SyncRecord)>,
    /// Highest outbox id in this page; `None` when the outbox is empty.
    pub last_id: Option<i64>,
}

/// Position in export order.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SyncCursor {
    pub hlc: String,
    pub coll: u8,
    pub key: String,
}

impl SyncRecord {
    pub fn cursor(&self) -> SyncCursor {
        SyncCursor {
            hlc: self.hlc.clone(),
            coll: self.coll,
            key: self.key.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileValue {
    profile: String,
    name: String,
    #[serde(default)]
    avatar: String,
    #[serde(default)]
    position: i64,
}

/// Access: `None` is a profile without a PIN.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockValue {
    profile: String,
    /// Required, as a string or an explicit null: serde would read a
    /// missing field as `None`, and a record without it would clear a PIN.
    #[serde(deserialize_with = "Option::deserialize")]
    pin_hash: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoneValue {
    profile: String,
}

/// Kids or not. `kids` is required: a record without it is skipped, never
/// read as a standard profile.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccessValue {
    profile: String,
    kids: bool,
}

/// One allowlist entry. A missing optional field reads as none; a missing
/// `allowed` as a removal.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AllowedValue {
    profile: String,
    item_id: String,
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    poster: Option<String>,
    #[serde(default)]
    allowed: bool,
    #[serde(default)]
    added_ms: i64,
}

/// One addon's state. `installed` is required: a record without it is
/// skipped, never read as an install.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddonEntryValue {
    profile: String,
    url: String,
    manifest: String,
    enabled: bool,
    allow_private: bool,
    installed: bool,
}

/// One addon's place.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddonOrderValue {
    profile: String,
    url: String,
    position: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemValue {
    profile: String,
    item_id: String,
    kind: String,
    name: String,
    poster: Option<String>,
    poster_shape: Option<String>,
    in_library: bool,
    added_ms: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgressValue {
    profile: String,
    video_id: String,
    item_id: String,
    position_ms: i64,
    duration_ms: i64,
    updated_ms: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatchedValue {
    profile: String,
    video_id: String,
    item_id: String,
    watched: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemoryValue {
    profile: String,
    item_id: String,
    binge_group: Option<String>,
    stream_json: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DismissedValue {
    profile: String,
    item_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingValue {
    profile: String,
    key: String,
    value_json: String,
}

pub(crate) fn key(parts: &[&str]) -> String {
    parts.join(&SEP.to_string())
}

fn record<T: Serialize>(coll: u8, key: String, value: &T, hlc: String) -> SyncRecord {
    SyncRecord {
        coll,
        key,
        value: serde_json::to_value(value).expect("plain structs serialize"),
        hlc,
    }
}

impl Library {
    /// Up to `limit` registers after `after`, in `(hlc, coll, key)` order.
    pub fn changed_since(
        &self,
        after: &SyncCursor,
        limit: usize,
    ) -> Result<Vec<SyncRecord>, LibraryError> {
        self.read(|c| {
            let mut out = Vec::new();
            for coll in ALL {
                out.extend(export(c, coll, Filter::After(after, limit))?);
            }
            out.sort_by_cached_key(SyncRecord::cursor);
            out.truncate(limit);
            Ok(out)
        })
    }

    /// Applies pulled registers in one transaction; each replaces local state
    /// only when its HLC is newer. Returns how many registers changed.
    pub fn apply(&self, records: &[SyncRecord]) -> Result<usize, LibraryError> {
        let removal = records.iter().any(|r| r.coll == coll::PROFILE_GONE);
        let changed = self.write(|c| {
            if removal {
                self.mark_removal();
            }
            let mut changed = 0;
            for r in records {
                match apply_one(c, r) {
                    Ok(n) => changed += n,
                    // A record from a newer app version or a damaged one is
                    // skipped, not allowed to block the rest of the page.
                    Err(ApplyError::Shape(e)) => {
                        tracing::warn!("skipping sync record in coll {}: {e}", r.coll)
                    }
                    Err(ApplyError::Sql(e)) => return Err(e),
                }
            }
            Ok(changed)
        })?;
        if removal {
            self.checkpoint();
        }
        Ok(changed)
    }

    /// The newest HLC stored in any register, so a restarted clock never
    /// issues a timestamp below what this library already holds.
    pub fn newest_hlc(&self) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT MAX(h) FROM (
                   SELECT MAX(hlc) AS h FROM profiles UNION ALL SELECT MAX(hlc) FROM addons
                   UNION ALL SELECT MAX(hlc) FROM addon_order
                   UNION ALL SELECT MAX(hlc) FROM library_items UNION ALL SELECT MAX(hlc) FROM progress
                   UNION ALL SELECT MAX(hlc) FROM watched UNION ALL SELECT MAX(hlc) FROM settings
                   UNION ALL SELECT MAX(hlc) FROM stream_memory UNION ALL SELECT MAX(hlc) FROM profile_lock
                   UNION ALL SELECT MAX(hlc) FROM profile_gone UNION ALL SELECT MAX(hlc) FROM profile_access
                   UNION ALL SELECT MAX(hlc) FROM allowed UNION ALL SELECT MAX(hlc) FROM dismissed)",
                [],
                |r| r.get(0),
            )
        })
    }

    /// Up to `limit` queued registers with their current values. A register
    /// queued several times is read once, at its newest entry.
    pub fn outbox(&self, limit: usize) -> Result<OutboxPage, LibraryError> {
        self.read(|c| {
            let mut stmt =
                c.prepare("SELECT id, coll, key FROM sync_outbox ORDER BY id LIMIT ?1")?;
            let queued: Vec<(i64, u8, String)> = stmt
                .query_map(params![limit as i64], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            let last_id = queued.last().map(|q| q.0);
            let mut newest: std::collections::HashMap<(u8, &str), i64> = Default::default();
            for (id, coll, key) in &queued {
                newest.insert((*coll, key), *id);
            }
            let mut records = Vec::new();
            for (id, coll, key) in &queued {
                if newest.get(&(*coll, key.as_str())) != Some(id) {
                    continue;
                }
                for record in export(c, *coll, Filter::Key(key))? {
                    records.push((*id, record));
                }
            }
            Ok(OutboxPage { records, last_id })
        })
    }

    /// Queues every register: on a device's first sync, and when the relay
    /// lost records.
    pub fn queue_all(&self) -> Result<(), LibraryError> {
        self.write(|c| {
            // Registers already queued are skipped, so repeating this cannot
            // grow the outbox.
            c.execute_batch(
                "INSERT INTO sync_outbox (coll, key)
                 SELECT coll, key FROM (
                   SELECT 0 AS coll, id AS key FROM profiles
                   UNION ALL SELECT 12, profile_id || char(31) || transport_url FROM addons
                   UNION ALL SELECT 13, profile_id || char(31) || transport_url FROM addon_order
                   UNION ALL SELECT 2, profile_id || char(31) || item_id FROM library_items
                   UNION ALL SELECT 3, profile_id || char(31) || video_id FROM progress
                   UNION ALL SELECT 4, profile_id || char(31) || video_id FROM watched
                   UNION ALL SELECT 5, profile_id || char(31) || key FROM settings
                   UNION ALL SELECT 6, profile_id || char(31) || item_id FROM stream_memory
                   UNION ALL SELECT 7, profile_id FROM profile_lock
                   UNION ALL SELECT 8, profile_id FROM profile_gone
                   UNION ALL SELECT 9, profile_id FROM profile_access
                   UNION ALL SELECT 10, profile_id || char(31) || item_id FROM allowed
                   UNION ALL SELECT 11, profile_id || char(31) || item_id FROM dismissed
                 ) AS r
                 WHERE NOT EXISTS (SELECT 1 FROM sync_outbox o WHERE o.coll = r.coll AND o.key = r.key)",
            )
        })
    }

    /// Gives a register a new hlc and queues it, even when that is lower: for
    /// a version stamped by a clock that ran ahead, which no other device
    /// ever accepted.
    pub fn restamp(&self, coll: u8, key: &str, hlc: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            for (table, key_expr) in tables(coll) {
                c.execute(
                    &format!("UPDATE {table} SET hlc = ?1 WHERE {key_expr} = ?2"),
                    params![hlc, key],
                )?;
            }
            c.execute(
                "INSERT INTO sync_outbox (coll, key) VALUES (?1, ?2)",
                params![coll, key],
            )
            .map(drop)
        })
    }

    /// Queues a register again (one the relay could not take yet).
    pub fn requeue(&self, coll: u8, key: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO sync_outbox (coll, key) VALUES (?1, ?2)",
                params![coll, key],
            )
            .map(drop)
        })
    }

    /// One register as a sync record.
    pub fn register(&self, coll: u8, key: &str) -> Result<Option<SyncRecord>, LibraryError> {
        self.read(|c| Ok(export(c, coll, Filter::Key(key))?.into_iter().next()))
    }

    /// Drops queue entries up to `id` once the relay has them.
    pub fn outbox_done(&self, id: i64) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute("DELETE FROM sync_outbox WHERE id <= ?1", params![id])
                .map(drop)
        })
    }

    pub fn sync_state(&self, key: &str) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT value FROM sync_state WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn set_sync_state(&self, key: &str, value: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(drop)
        })
    }

    /// Sync state keys starting with `prefix`, with their values.
    pub fn sync_states(&self, prefix: &str) -> Result<Vec<(String, String)>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT key, value FROM sync_state WHERE substr(key, 1, length(?1)) = ?1",
            )?;
            stmt.query_map(params![prefix], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
    }

    pub fn delete_sync_state(&self, key: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute("DELETE FROM sync_state WHERE key = ?1", params![key])
                .map(drop)
        })
    }

    /// Replaces the state at `from` with `value` at `to`, in one transaction,
    /// so a crash keeps one or the other.
    pub fn move_sync_state(&self, from: &str, to: &str, value: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute("DELETE FROM sync_state WHERE key = ?1", params![from])?;
            c.execute(
                "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![to, value],
            )
            .map(drop)
        })
    }

    /// Whether this device belongs to a sync account: it stores an account's
    /// roster from the moment it joins until it leaves (`forget_sync`).
    pub fn in_sync_account(&self) -> Result<bool, LibraryError> {
        self.read(|c| c.query_row("SELECT EXISTS(SELECT 1 FROM sync_roster)", [], |r| r.get(0)))
    }

    /// The genesis key of the roster this device holds (not a fork's kept
    /// copy), if it holds one.
    pub fn roster_genesis(&self) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT genesis FROM sync_roster WHERE substr(genesis, 1, 5) != 'fork:' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
        })
    }

    /// The account's roster entries this device verified, in order.
    pub fn roster_entries(&self, genesis: &str) -> Result<Vec<SignedEntry>, LibraryError> {
        self.read(|c| {
            let mut stmt =
                c.prepare("SELECT body, sig FROM sync_roster WHERE genesis = ?1 ORDER BY seq")?;
            stmt.query_map(params![genesis], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
    }

    /// Stores an entry at `seq`. The same entry again changes nothing;
    /// another entry at a taken seq fails (a second history is never
    /// dropped silently).
    pub fn add_roster_entry(
        &self,
        genesis: &str,
        seq: u64,
        body: &[u8],
        sig: &[u8],
    ) -> Result<(), LibraryError> {
        self.write(|c| insert_roster_entry(c, genesis, seq, body, sig))
    }

    /// Replaces the entries stored under `genesis`, in one transaction.
    pub fn replace_roster(
        &self,
        genesis: &str,
        entries: &[SignedEntry],
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "DELETE FROM sync_roster WHERE genesis = ?1",
                params![genesis],
            )?;
            for (seq, (body, sig)) in entries.iter().enumerate() {
                insert_roster_entry(c, genesis, seq as u64, body, sig)?;
            }
            Ok(())
        })
    }

    pub fn clear_roster(&self, genesis: &str) -> Result<(), LibraryError> {
        self.replace_roster(genesis, &[])
    }

    /// Forgets everything sync knew (this device signed out, left here,
    /// deleted the account or was dropped from it): queue, cursors and
    /// roster. The defaults era that said the device made or joined an
    /// account (`DEFAULTS_ERA`) goes back: a device that made it is new
    /// again ("fresh": a profile without a list keeps its defaults step), one
    /// that joined is from before ("legacy"), announced as an addons change
    /// for an open addons view. The library itself stays, and so do this
    /// device's id and the hosts relay ids were pinned to.
    pub fn forget_sync(&self) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute_batch(
                "DELETE FROM sync_outbox;
                 DELETE FROM sync_state WHERE key != 'device_id' AND substr(key, 1, 6) != 'relay:';
                 DELETE FROM sync_roster;",
            )?;
            let left = c.execute(
                "UPDATE device_state
                 SET value = CASE value WHEN 'created' THEN 'fresh' ELSE 'legacy' END
                 WHERE key = ?1 AND value IN ('joined', 'created')",
                params![DEFAULTS_ERA],
            )?;
            if left > 0 {
                self.announce(Table::Addons);
            }
            Ok(())
        })
    }
}

/// Device state key of the device's defaults era (mq-runtime): "fresh" on a
/// device first opened with default addons, "legacy" on one from before,
/// "joined" or "created" while it belongs to a sync account it joined or
/// made.
pub const DEFAULTS_ERA: &str = "addon_defaults";

/// A roster entry as stored: body and signature.
pub type SignedEntry = (Vec<u8>, Vec<u8>);

/// Inserts unless the same entry is there; another one at `seq` breaks the
/// table's key and fails.
fn insert_roster_entry(
    c: &Connection,
    genesis: &str,
    seq: u64,
    body: &[u8],
    sig: &[u8],
) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO sync_roster (genesis, seq, body, sig) SELECT ?1, ?2, ?3, ?4
         WHERE NOT EXISTS (SELECT 1 FROM sync_roster
                           WHERE genesis = ?1 AND seq = ?2 AND body = ?3 AND sig = ?4)",
        params![genesis, seq as i64, body, sig],
    )
    .map(drop)
}

/// One table's registers after the cursor. The key expression and the
/// ordering match `SyncCursor`, so pages are exact.
/// Which registers of a table to read.
enum Filter<'a> {
    /// Up to n registers after the cursor, in export order.
    After(&'a SyncCursor, usize),
    /// The register with this key.
    Key(&'a str),
}

fn export(c: &Connection, coll: u8, filter: Filter<'_>) -> rusqlite::Result<Vec<SyncRecord>> {
    let (sql, key_expr) = match coll {
        coll::PROFILES => ("SELECT id, name, avatar, position, hlc FROM profiles", "id"),
        coll::PROFILE_LOCK => (
            "SELECT profile_id, pin_hash, hlc FROM profile_lock",
            "profile_id",
        ),
        coll::PROFILE_GONE => ("SELECT profile_id, hlc FROM profile_gone", "profile_id"),
        coll::PROFILE_ACCESS => (
            "SELECT profile_id, kids, hlc FROM profile_access",
            "profile_id",
        ),
        coll::ALLOWED => (
            "SELECT profile_id, item_id, kind, name, names, poster, allowed, added_ms, hlc FROM allowed",
            "profile_id || char(31) || item_id",
        ),
        coll::ADDON => (
            "SELECT profile_id, transport_url, manifest_json, enabled, allow_private, installed, hlc FROM addons",
            "profile_id || char(31) || transport_url",
        ),
        coll::ADDON_ORDER => (
            "SELECT profile_id, transport_url, position, hlc FROM addon_order",
            "profile_id || char(31) || transport_url",
        ),
        coll::ITEMS => (
            "SELECT profile_id, item_id, kind, name, poster, poster_shape, in_library, added_ms, hlc FROM library_items",
            "profile_id || char(31) || item_id",
        ),
        coll::PROGRESS => (
            "SELECT profile_id, video_id, item_id, position_ms, duration_ms, updated_ms, hlc FROM progress",
            "profile_id || char(31) || video_id",
        ),
        coll::WATCHED => (
            "SELECT profile_id, video_id, item_id, watched, hlc FROM watched",
            "profile_id || char(31) || video_id",
        ),
        coll::SETTINGS => (
            "SELECT profile_id, key, value_json, hlc FROM settings",
            "profile_id || char(31) || key",
        ),
        coll::STREAM_MEMORY => (
            "SELECT profile_id, item_id, binge_group, stream_json, hlc FROM stream_memory",
            "profile_id || char(31) || item_id",
        ),
        coll::DISMISSED => (
            "SELECT profile_id, item_id, hlc FROM dismissed",
            "profile_id || char(31) || item_id",
        ),
        _ => return Ok(Vec::new()),
    };
    let (query, values): (String, Vec<Box<dyn rusqlite::ToSql>>) = match filter {
        Filter::After(after, limit) => (
            format!(
                "{sql} WHERE (hlc, {coll}, {key_expr}) > (?1, ?2, ?3) ORDER BY hlc, {key_expr} LIMIT ?4"
            ),
            vec![
                Box::new(after.hlc.clone()),
                Box::new(after.coll),
                Box::new(after.key.clone()),
                Box::new(limit as i64),
            ],
        ),
        Filter::Key(key) => (
            format!("{sql} WHERE {key_expr} = ?1"),
            vec![Box::new(key.to_owned())],
        ),
    };
    let mut stmt = c.prepare(&query)?;
    let args = rusqlite::params_from_iter(values.iter().map(|v| v.as_ref()));
    let rows = match coll {
        coll::PROFILES => stmt
            .query_map(args, |r| {
                let profile: String = r.get(0)?;
                let value = ProfileValue {
                    profile: profile.clone(),
                    name: r.get(1)?,
                    avatar: r.get(2)?,
                    position: r.get(3)?,
                };
                Ok(record(coll, key(&[&profile]), &value, r.get(4)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::PROFILE_LOCK => stmt
            .query_map(args, |r| {
                let value = LockValue {
                    profile: r.get(0)?,
                    pin_hash: r.get(1)?,
                };
                Ok(record(coll, key(&[&value.profile]), &value, r.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::PROFILE_GONE => stmt
            .query_map(args, |r| {
                let value = GoneValue { profile: r.get(0)? };
                Ok(record(coll, key(&[&value.profile]), &value, r.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::PROFILE_ACCESS => stmt
            .query_map(args, |r| {
                let value = AccessValue {
                    profile: r.get(0)?,
                    kids: r.get(1)?,
                };
                Ok(record(coll, key(&[&value.profile]), &value, r.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::ALLOWED => stmt
            .query_map(args, |r| {
                let names: String = r.get(4)?;
                let value = AllowedValue {
                    profile: r.get(0)?,
                    item_id: r.get(1)?,
                    kind: r.get(2)?,
                    name: r.get(3)?,
                    names: serde_json::from_str(&names).unwrap_or_default(),
                    poster: r.get(5)?,
                    allowed: r.get(6)?,
                    added_ms: r.get(7)?,
                };
                let k = key(&[&value.profile, &value.item_id]);
                Ok(record(coll, k, &value, r.get(8)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::ADDON => stmt
            .query_map(args, |r| {
                let value = AddonEntryValue {
                    profile: r.get(0)?,
                    url: r.get(1)?,
                    manifest: r.get(2)?,
                    enabled: r.get(3)?,
                    allow_private: r.get(4)?,
                    installed: r.get(5)?,
                };
                let k = key(&[&value.profile, &value.url]);
                Ok(record(coll, k, &value, r.get(6)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::ADDON_ORDER => stmt
            .query_map(args, |r| {
                let value = AddonOrderValue {
                    profile: r.get(0)?,
                    url: r.get(1)?,
                    position: r.get(2)?,
                };
                let k = key(&[&value.profile, &value.url]);
                Ok(record(coll, k, &value, r.get(3)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::ITEMS => stmt
            .query_map(args, |r| {
                let value = ItemValue {
                    profile: r.get(0)?,
                    item_id: r.get(1)?,
                    kind: r.get(2)?,
                    name: r.get(3)?,
                    poster: r.get(4)?,
                    poster_shape: r.get(5)?,
                    in_library: r.get(6)?,
                    added_ms: r.get(7)?,
                };
                let k = key(&[&value.profile, &value.item_id]);
                Ok(record(coll, k, &value, r.get(8)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::PROGRESS => stmt
            .query_map(args, |r| {
                let value = ProgressValue {
                    profile: r.get(0)?,
                    video_id: r.get(1)?,
                    item_id: r.get(2)?,
                    position_ms: r.get(3)?,
                    duration_ms: r.get(4)?,
                    updated_ms: r.get(5)?,
                };
                let k = key(&[&value.profile, &value.video_id]);
                Ok(record(coll, k, &value, r.get(6)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::WATCHED => stmt
            .query_map(args, |r| {
                let value = WatchedValue {
                    profile: r.get(0)?,
                    video_id: r.get(1)?,
                    item_id: r.get(2)?,
                    watched: r.get(3)?,
                };
                let k = key(&[&value.profile, &value.video_id]);
                Ok(record(coll, k, &value, r.get(4)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::STREAM_MEMORY => stmt
            .query_map(args, |r| {
                let value = MemoryValue {
                    profile: r.get(0)?,
                    item_id: r.get(1)?,
                    binge_group: r.get(2)?,
                    stream_json: r.get(3)?,
                };
                let k = key(&[&value.profile, &value.item_id]);
                Ok(record(coll, k, &value, r.get(4)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::DISMISSED => stmt
            .query_map(args, |r| {
                let value = DismissedValue {
                    profile: r.get(0)?,
                    item_id: r.get(1)?,
                };
                let k = key(&[&value.profile, &value.item_id]);
                Ok(record(coll, k, &value, r.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        _ => stmt
            .query_map(args, |r| {
                let value = SettingValue {
                    profile: r.get(0)?,
                    key: r.get(1)?,
                    value_json: r.get(2)?,
                };
                let k = key(&[&value.profile, &value.key]);
                Ok(record(coll, k, &value, r.get(3)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

enum ApplyError {
    Shape(String),
    Sql(rusqlite::Error),
}

impl From<serde_json::Error> for ApplyError {
    fn from(e: serde_json::Error) -> Self {
        Self::Shape(e.to_string())
    }
}

impl From<rusqlite::Error> for ApplyError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}

/// A value must describe the register its key names: the key is what the
/// reader verified against the blind id, the value is what gets written.
fn same_key(r: &SyncRecord, parts: &[&str]) -> Result<(), ApplyError> {
    if key(parts) == r.key {
        Ok(())
    } else {
        Err(ApplyError::Shape(format!(
            "value does not match key in coll {}",
            r.coll
        )))
    }
}

fn apply_one(c: &Connection, r: &SyncRecord) -> Result<usize, ApplyError> {
    let value = r.value.clone();
    let hlc = r.hlc.as_str();
    // Remove wins: whatever arrives for a gone profile, in any order, is
    // dropped (every key starts with its profile).
    let profile = r.key.split(SEP).next().unwrap_or_default();
    if r.coll != coll::PROFILE_GONE && gone(c, profile)? {
        return Ok(0);
    }
    Ok(match r.coll {
        coll::PROFILES => {
            let v: ProfileValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile])?;
            // Held to what this device would write itself: anything else
            // shows as the default avatar.
            let avatar = if valid_avatar(&v.avatar) {
                v.avatar.as_str()
            } else {
                ""
            };
            sql::set_profile(c, &v.profile, &v.name, avatar, v.position, hlc)?
        }
        coll::PROFILE_LOCK => {
            let v: LockValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile])?;
            sql::set_lock(c, &v.profile, v.pin_hash.as_deref(), hlc)?
        }
        coll::PROFILE_GONE => {
            let v: GoneValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile])?;
            if v.profile == OWNER {
                return Err(ApplyError::Shape("the owner profile cannot be gone".into()));
            }
            sql::set_gone(c, &v.profile, hlc)?
        }
        coll::PROFILE_ACCESS => {
            let v: AccessValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile])?;
            if v.profile == OWNER && v.kids {
                return Err(ApplyError::Shape(
                    "the owner is never a kids profile".into(),
                ));
            }
            sql::set_access(c, &v.profile, v.kids, hlc)?
        }
        coll::ALLOWED => {
            let v: AllowedValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.item_id])?;
            if v.profile == OWNER {
                return Err(ApplyError::Shape("the owner has no allowlist".into()));
            }
            let row = AllowedRow {
                item_id: v.item_id,
                kind: v.kind,
                name: v.name,
                names: v.names,
                poster: v.poster,
                allowed: v.allowed,
                added_ms: v.added_ms,
            }
            .bounded()
            .map_err(|e| ApplyError::Shape(e.into()))?;
            sql::set_allowed(c, &v.profile, &row, hlc)?
        }
        coll::ADDON => {
            let v: AddonEntryValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.url])?;
            if !sql::untouched_rule(c, &v.profile, hlc)? {
                return Ok(0);
            }
            let row = AddonRow {
                transport_url: v.url,
                manifest_json: v.manifest,
                position: 0,
                enabled: v.enabled,
                allow_private: v.allow_private,
            };
            sql::set_addon(c, &v.profile, &row, v.installed, hlc)?
        }
        coll::ADDON_ORDER => {
            let v: AddonOrderValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.url])?;
            if !sql::untouched_rule(c, &v.profile, hlc)? {
                return Ok(0);
            }
            sql::set_addon_order(c, &v.profile, &v.url, v.position, hlc)?
        }
        coll::ITEMS => {
            let v: ItemValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.item_id])?;
            let item = LibraryItemRow {
                item_id: v.item_id,
                kind: v.kind,
                name: v.name,
                poster: v.poster,
                poster_shape: v.poster_shape,
                added_ms: v.added_ms,
            };
            sql::upsert_item(c, &v.profile, &item, v.in_library, hlc)?
        }
        coll::PROGRESS => {
            let v: ProgressValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.video_id])?;
            let p = ProgressRow {
                video_id: &v.video_id,
                item_id: &v.item_id,
                position_ms: v.position_ms,
                duration_ms: v.duration_ms,
                updated_ms: v.updated_ms,
            };
            sql::set_progress(c, &v.profile, &p, hlc)?
        }
        coll::WATCHED => {
            let v: WatchedValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.video_id])?;
            sql::set_watched(c, &v.profile, &v.item_id, &v.video_id, v.watched, hlc)?
        }
        coll::SETTINGS => {
            let v: SettingValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.key])?;
            sql::set_setting(c, &v.profile, &v.key, &v.value_json, hlc)?
        }
        coll::STREAM_MEMORY => {
            let v: MemoryValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.item_id])?;
            sql::set_stream_memory(
                c,
                &v.profile,
                &v.item_id,
                v.binge_group.as_deref(),
                &v.stream_json,
                hlc,
            )?
        }
        coll::DISMISSED => {
            let v: DismissedValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile, &v.item_id])?;
            sql::set_dismissed(c, &v.profile, &v.item_id, hlc)?
        }
        // A collection from a newer app version.
        _ => 0,
    })
}

/// Tables holding a collection's registers, with the key expression; the
/// first one holds the register's hlc.
fn tables(coll: u8) -> &'static [(&'static str, &'static str)] {
    match coll {
        coll::PROFILES => &[("profiles", "id")],
        coll::ADDON => &[("addons", "profile_id || char(31) || transport_url")],
        coll::ADDON_ORDER => &[("addon_order", "profile_id || char(31) || transport_url")],
        coll::ITEMS => &[("library_items", "profile_id || char(31) || item_id")],
        coll::PROGRESS => &[("progress", "profile_id || char(31) || video_id")],
        coll::WATCHED => &[("watched", "profile_id || char(31) || video_id")],
        coll::SETTINGS => &[("settings", "profile_id || char(31) || key")],
        coll::STREAM_MEMORY => &[("stream_memory", "profile_id || char(31) || item_id")],
        coll::PROFILE_LOCK => &[("profile_lock", "profile_id")],
        coll::PROFILE_GONE => &[("profile_gone", "profile_id")],
        coll::PROFILE_ACCESS => &[("profile_access", "profile_id")],
        coll::ALLOWED => &[("allowed", "profile_id || char(31) || item_id")],
        coll::DISMISSED => &[("dismissed", "profile_id || char(31) || item_id")],
        _ => &[],
    }
}

pub(crate) fn stored_hlc(c: &Connection, coll: u8, key: &str) -> rusqlite::Result<Option<String>> {
    let Some((table, key_expr)) = tables(coll).first() else {
        return Ok(None);
    };
    c.query_row(
        &format!("SELECT hlc FROM {table} WHERE {key_expr} = ?1"),
        params![key],
        |r| r.get(0),
    )
    .optional()
}

/// What a local write is stamped with. Every local write takes its hlc
/// inside its write transaction, so writes are stamped in the order they
/// commit (C31). Stamped before it held the writer, a write could commit
/// after a newer one, be stamped again just above it (`local`), and land on
/// the hlc another waiting write already held, which then changed nothing.
#[derive(Clone, Copy)]
pub enum Stamp<'a> {
    /// A real edit: the device clock ticks when the write holds the writer,
    /// and when the write goes in again just above a newer stored version
    /// the clock moves past that hlc too.
    Clock(&'a HlcClock),
    /// A given hlc: the zero one of a write that only caches what the device
    /// saw, another app's import, or a test's.
    Fixed(&'a str),
}

impl Stamp<'_> {
    /// A person's edit: the clock, or a given stamp with a wall time (an
    /// import's too); not the zero stamp of a write that only caches or of
    /// the defaults step.
    pub(crate) fn is_real(&self) -> bool {
        match self {
            Stamp::Clock(_) => true,
            Stamp::Fixed(hlc) => Hlc::decode(hlc).is_some_and(|h| h.wall_ms > 0),
        }
    }

    pub(crate) fn hlc(&self) -> String {
        match self {
            Stamp::Clock(clock) => clock.tick(clock.wall_now()).encode(),
            Stamp::Fixed(hlc) => (*hlc).to_owned(),
        }
    }
}

impl<'a> From<&'a HlcClock> for Stamp<'a> {
    fn from(clock: &'a HlcClock) -> Self {
        Stamp::Clock(clock)
    }
}

impl<'a> From<&'a str> for Stamp<'a> {
    fn from(hlc: &'a str) -> Self {
        Stamp::Fixed(hlc)
    }
}

impl<'a> From<&'a String> for Stamp<'a> {
    fn from(hlc: &'a String) -> Self {
        Stamp::Fixed(hlc)
    }
}

/// A local register write, stamped here (inside the write transaction) and
/// queued for push when it changed something.
///
/// When the stored version wins by clock (pulled from a device whose clock
/// runs ahead of this one), the user still acted after seeing it, so the
/// write goes in once more just above it, and a clock stamp moves past that
/// hlc so no later local write is stamped at or below it. Writes that only
/// cache what the device saw (a zero hlc) or import another app's state
/// (device 0) never do this, so they never override a real edit.
pub(crate) fn local(
    c: &Connection,
    coll: u8,
    parts: &[&str],
    stamp: &Stamp<'_>,
    write: impl Fn(&str) -> rusqlite::Result<usize>,
) -> rusqlite::Result<()> {
    local_changed(c, coll, parts, stamp, write).map(drop)
}

/// `local`, telling whether the write went in (a stored version at or above
/// a fixed stamp keeps it out).
pub(crate) fn local_changed(
    c: &Connection,
    coll: u8,
    parts: &[&str],
    stamp: &Stamp<'_>,
    write: impl Fn(&str) -> rusqlite::Result<usize>,
) -> rusqlite::Result<bool> {
    refuse_gone(c, parts[0])?;
    let hlc = stamp.hlc();
    let mut changed = write(&hlc)?;
    if changed == 0
        && let Some(mine) = Hlc::decode(&hlc).filter(|h| h.wall_ms > 0 && h.device != 0)
        && let Some(stored) = stored_hlc(c, coll, &key(parts))?
            .as_deref()
            .and_then(Hlc::decode)
        && stored > mine
    {
        let again = stored.next_for(mine.device);
        changed = write(&again.encode())?;
        if let Stamp::Clock(clock) = stamp {
            clock.observe(again, clock.wall_now());
        }
    }
    queue(c, changed, coll, parts)?;
    Ok(changed > 0)
}

/// Queues a register for push when a local write changed it.
pub(crate) fn queue(
    c: &Connection,
    changed: usize,
    coll: u8,
    parts: &[&str],
) -> rusqlite::Result<()> {
    if changed > 0 {
        c.execute(
            "INSERT INTO sync_outbox (coll, key) VALUES (?1, ?2)",
            params![coll, key(parts)],
        )?;
    }
    Ok(())
}

/// The one copy of each register write, shared by local writes and sync.
pub(crate) mod sql {
    use super::*;

    pub fn set_profile(
        c: &Connection,
        id: &str,
        name: &str,
        avatar: &str,
        position: i64,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO profiles (id, name, avatar, position, hlc) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name, avatar = excluded.avatar, position = excluded.position,
               hlc = excluded.hlc
             WHERE excluded.hlc > profiles.hlc",
            params![id, name, avatar, position, hlc],
        )
    }

    pub fn set_lock(
        c: &Connection,
        profile: &str,
        pin_hash: Option<&str>,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO profile_lock (profile_id, pin_hash, hlc) VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id) DO UPDATE SET pin_hash = excluded.pin_hash, hlc = excluded.hlc
             WHERE excluded.hlc > profile_lock.hlc",
            params![profile, pin_hash, hlc],
        )
    }

    pub fn set_access(
        c: &Connection,
        profile: &str,
        kids: bool,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO profile_access (profile_id, kids, hlc) VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id) DO UPDATE SET kids = excluded.kids, hlc = excluded.hlc
             WHERE excluded.hlc > profile_access.hlc",
            params![profile, kids, hlc],
        )
    }

    /// One allowlist entry, already held to its bounds (`AllowedRow::bounded`).
    pub fn set_allowed(
        c: &Connection,
        profile: &str,
        row: &AllowedRow,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        let names = serde_json::to_string(&row.names).expect("strings serialize");
        c.execute(
            "INSERT INTO allowed (profile_id, item_id, kind, name, names, poster, allowed, added_ms, hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(profile_id, item_id) DO UPDATE SET
               kind = excluded.kind, name = excluded.name, names = excluded.names,
               poster = excluded.poster, allowed = excluded.allowed,
               added_ms = excluded.added_ms, hlc = excluded.hlc
             WHERE excluded.hlc > allowed.hlc",
            params![
                profile,
                row.item_id,
                row.kind,
                row.name,
                names,
                row.poster,
                row.allowed,
                row.added_ms,
                hlc
            ],
        )
    }

    /// Marks the profile gone and purges it. Never for the owner (callers
    /// check). Returns 1 when the register changed.
    pub fn set_gone(c: &Connection, profile: &str, hlc: &str) -> rusqlite::Result<usize> {
        let first = !gone(c, profile)?;
        let changed = c.execute(
            "INSERT INTO profile_gone (profile_id, hlc) VALUES (?1, ?2)
             ON CONFLICT(profile_id) DO UPDATE SET hlc = excluded.hlc
             WHERE excluded.hlc > profile_gone.hlc",
            params![profile, hlc],
        )?;
        purge(c, profile)?;
        if first {
            // Cached addon answers are keyed by URLs that may carry the
            // profile's addon keys or searches; the device's version lists
            // name streams those addons listed (digests of their keys);
            // looked-up ratings name the titles the profile opened.
            c.execute("DELETE FROM http_cache", [])?;
            c.execute("DELETE FROM ratings", [])?;
            c.execute(
                "DELETE FROM device_state WHERE key IN ('no_picture', 'did_not_play', 'wrong_file')",
                [],
            )?;
        }
        Ok(changed)
    }

    /// One addon's state register, written when `hlc` is newer than the
    /// stored one (local edits are guarded too: a pull can apply a newer
    /// version between an edit taking its hlc and reaching the writer). A
    /// removal keeps the row as a tombstone (`installed` 0) with no
    /// manifest. Returns 1 on change.
    pub fn set_addon(
        c: &Connection,
        profile: &str,
        a: &AddonRow,
        installed: bool,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        let manifest = if installed {
            a.manifest_json.as_str()
        } else {
            ""
        };
        c.execute(
            "INSERT INTO addons (profile_id, transport_url, manifest_json, enabled, allow_private, installed, hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(profile_id, transport_url) DO UPDATE SET
               manifest_json = excluded.manifest_json, enabled = excluded.enabled,
               allow_private = excluded.allow_private, installed = excluded.installed, hlc = excluded.hlc
             WHERE excluded.hlc > addons.hlc",
            params![
                profile,
                a.transport_url,
                manifest,
                a.enabled,
                a.allow_private,
                installed,
                hlc
            ],
        )
    }

    /// Untouched defaults never outrank a list a person made (the defaults
    /// step writes them at wall time 0, `store::endorse`): an incoming
    /// register at wall time 0 is skipped while the profile has a real
    /// register here, and an incoming real one first drops the profile's
    /// untouched ones, which no person here chose. Either arrival order
    /// gives the same list. Answers whether to apply the record.
    pub fn untouched_rule(c: &Connection, profile: &str, hlc: &str) -> rusqlite::Result<bool> {
        const UNTOUCHED: &str = "substr(hlc, 1, 12) = '000000000000'";
        if Hlc::decode(hlc).is_some_and(|h| h.wall_ms == 0) {
            let real: bool = c.query_row(
                &format!(
                    "SELECT EXISTS (SELECT 1 FROM addons WHERE profile_id = ?1 AND NOT {UNTOUCHED})
                         OR EXISTS (SELECT 1 FROM addon_order WHERE profile_id = ?1 AND NOT {UNTOUCHED})"
                ),
                params![profile],
                |r| r.get(0),
            )?;
            return Ok(!real);
        }
        for table in ["addons", "addon_order"] {
            c.execute(
                &format!("DELETE FROM {table} WHERE profile_id = ?1 AND {UNTOUCHED}"),
                params![profile],
            )?;
        }
        Ok(true)
    }

    /// One addon's place register, as `set_addon`. Returns 1 on change.
    pub fn set_addon_order(
        c: &Connection,
        profile: &str,
        url: &str,
        position: i64,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO addon_order (profile_id, transport_url, position, hlc) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(profile_id, transport_url) DO UPDATE SET position = excluded.position, hlc = excluded.hlc
             WHERE excluded.hlc > addon_order.hlc",
            params![profile, url, position, hlc],
        )
    }

    pub fn upsert_item(
        c: &Connection,
        profile: &str,
        item: &LibraryItemRow,
        in_library: bool,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO library_items (profile_id, item_id, kind, name, poster, poster_shape, in_library, added_ms, hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(profile_id, item_id) DO UPDATE SET
               kind = excluded.kind, name = excluded.name, poster = excluded.poster,
               poster_shape = excluded.poster_shape, in_library = excluded.in_library,
               added_ms = excluded.added_ms, hlc = excluded.hlc
             WHERE excluded.hlc > library_items.hlc",
            params![
                profile,
                item.item_id,
                item.kind,
                item.name,
                item.poster,
                item.poster_shape,
                in_library,
                item.added_ms,
                hlc
            ],
        )
    }

    pub fn set_progress(
        c: &Connection,
        profile: &str,
        p: &ProgressRow<'_>,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO progress (profile_id, video_id, item_id, position_ms, duration_ms, updated_ms, hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(profile_id, video_id) DO UPDATE SET
               item_id = excluded.item_id, position_ms = excluded.position_ms,
               duration_ms = excluded.duration_ms, updated_ms = excluded.updated_ms, hlc = excluded.hlc
             WHERE excluded.hlc > progress.hlc",
            params![profile, p.video_id, p.item_id, p.position_ms, p.duration_ms, p.updated_ms, hlc],
        )
    }

    pub fn set_watched(
        c: &Connection,
        profile: &str,
        item_id: &str,
        video_id: &str,
        watched: bool,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO watched (profile_id, video_id, item_id, watched, hlc) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(profile_id, video_id) DO UPDATE SET
               item_id = excluded.item_id, watched = excluded.watched, hlc = excluded.hlc
             WHERE excluded.hlc > watched.hlc",
            params![profile, video_id, item_id, watched, hlc],
        )
    }

    pub fn set_stream_memory(
        c: &Connection,
        profile: &str,
        item_id: &str,
        binge_group: Option<&str>,
        stream_json: &str,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO stream_memory (profile_id, item_id, binge_group, stream_json, hlc) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(profile_id, item_id) DO UPDATE SET
               binge_group = excluded.binge_group, stream_json = excluded.stream_json, hlc = excluded.hlc
             WHERE excluded.hlc > stream_memory.hlc",
            params![profile, item_id, binge_group, stream_json, hlc],
        )
    }

    pub fn set_dismissed(
        c: &Connection,
        profile: &str,
        item_id: &str,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO dismissed (profile_id, item_id, hlc) VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id, item_id) DO UPDATE SET hlc = excluded.hlc
             WHERE excluded.hlc > dismissed.hlc",
            params![profile, item_id, hlc],
        )
    }

    pub fn set_setting(
        c: &Connection,
        profile: &str,
        key: &str,
        value_json: &str,
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        c.execute(
            "INSERT INTO settings (profile_id, key, value_json, hlc) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(profile_id, key) DO UPDATE SET value_json = excluded.value_json, hlc = excluded.hlc
             WHERE excluded.hlc > settings.hlc",
            params![profile, key, value_json, hlc],
        )
    }
}
