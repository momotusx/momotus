//! Registers as sync records. Every synced row is a last-writer-wins register
//! with an HLC; export walks them in `(hlc, coll, key)` order so a cursor
//! never skips rows that share an HLC (an import writes thousands with one),
//! and apply runs the same upsert-if-newer SQL as local writes.

use crate::hlc::Hlc;
use crate::store::{AddonRow, Library, LibraryError, LibraryItemRow, ProgressRow};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Collection numbers on the wire. Never renumber.
pub mod coll {
    pub const PROFILES: u8 = 0;
    pub const ADDONS: u8 = 1;
    pub const ITEMS: u8 = 2;
    pub const PROGRESS: u8 = 3;
    pub const WATCHED: u8 = 4;
    pub const SETTINGS: u8 = 5;
    pub const STREAM_MEMORY: u8 = 6;
}

/// Separates the parts of a register key; ids never contain it.
const SEP: char = '\u{1f}';

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
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddonValue {
    url: String,
    manifest: String,
    enabled: bool,
    allow_private: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddonsValue {
    profile: String,
    addons: Vec<AddonValue>,
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
            for coll in [
                coll::PROFILES,
                coll::ADDONS,
                coll::ITEMS,
                coll::PROGRESS,
                coll::WATCHED,
                coll::SETTINGS,
                coll::STREAM_MEMORY,
            ] {
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
        self.write(|c| {
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
        })
    }

    /// The newest HLC stored in any register, so a restarted clock never
    /// issues a timestamp below what this library already holds.
    pub fn newest_hlc(&self) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT MAX(h) FROM (
                   SELECT MAX(hlc) AS h FROM profiles UNION ALL SELECT MAX(hlc) FROM addon_list
                   UNION ALL SELECT MAX(hlc) FROM library_items UNION ALL SELECT MAX(hlc) FROM progress
                   UNION ALL SELECT MAX(hlc) FROM watched UNION ALL SELECT MAX(hlc) FROM settings
                   UNION ALL SELECT MAX(hlc) FROM stream_memory)",
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
                   UNION ALL SELECT 1, profile_id FROM addon_list
                   UNION ALL SELECT 2, profile_id || char(31) || item_id FROM library_items
                   UNION ALL SELECT 3, profile_id || char(31) || video_id FROM progress
                   UNION ALL SELECT 4, profile_id || char(31) || video_id FROM watched
                   UNION ALL SELECT 5, profile_id || char(31) || key FROM settings
                   UNION ALL SELECT 6, profile_id || char(31) || item_id FROM stream_memory
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
        coll::PROFILES => ("SELECT id, name, hlc FROM profiles", "id"),
        coll::ADDONS => ("SELECT profile_id, hlc FROM addon_list", "profile_id"),
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
                };
                Ok(record(coll, key(&[&profile]), &value, r.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        coll::ADDONS => {
            let lists: Vec<(String, String)> = stmt
                .query_map(args, |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let mut rows = c.prepare(
                "SELECT transport_url, manifest_json, enabled, allow_private FROM addons
                 WHERE profile_id = ?1 ORDER BY position",
            )?;
            let mut out = Vec::new();
            for (profile, hlc) in lists {
                let addons = rows
                    .query_map(params![profile], |r| {
                        Ok(AddonValue {
                            url: r.get(0)?,
                            manifest: r.get(1)?,
                            enabled: r.get(2)?,
                            allow_private: r.get(3)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let value = AddonsValue {
                    profile: profile.clone(),
                    addons,
                };
                out.push(record(coll, key(&[&profile]), &value, hlc));
            }
            out
        }
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
    Ok(match r.coll {
        coll::PROFILES => {
            let v: ProfileValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile])?;
            c.execute(
                "INSERT INTO profiles (id, name, hlc) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, hlc = excluded.hlc
                 WHERE excluded.hlc > profiles.hlc",
                params![v.profile, v.name, hlc],
            )?
        }
        coll::ADDONS => {
            let v: AddonsValue = serde_json::from_value(value)?;
            same_key(r, &[&v.profile])?;
            let rows: Vec<AddonRow> = v
                .addons
                .into_iter()
                .enumerate()
                .map(|(i, a)| AddonRow {
                    transport_url: a.url,
                    manifest_json: a.manifest,
                    position: i as i64,
                    enabled: a.enabled,
                    allow_private: a.allow_private,
                })
                .collect();
            sql::set_addons(c, &v.profile, &rows, hlc)?
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
        // A collection from a newer app version.
        _ => 0,
    })
}

/// Tables holding a collection's registers, with the key expression; the
/// first one holds the register's hlc.
fn tables(coll: u8) -> &'static [(&'static str, &'static str)] {
    match coll {
        coll::PROFILES => &[("profiles", "id")],
        coll::ADDONS => &[("addon_list", "profile_id"), ("addons", "profile_id")],
        coll::ITEMS => &[("library_items", "profile_id || char(31) || item_id")],
        coll::PROGRESS => &[("progress", "profile_id || char(31) || video_id")],
        coll::WATCHED => &[("watched", "profile_id || char(31) || video_id")],
        coll::SETTINGS => &[("settings", "profile_id || char(31) || key")],
        coll::STREAM_MEMORY => &[("stream_memory", "profile_id || char(31) || item_id")],
        _ => &[],
    }
}

fn stored_hlc(c: &Connection, coll: u8, key: &str) -> rusqlite::Result<Option<String>> {
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

/// A local register write, queued for push when it changed something.
///
/// When the stored version wins by clock (pulled from a device whose clock
/// runs ahead of this one), the user still acted after seeing it, so the
/// write goes in once more just above it. Writes that only cache what the
/// device saw (a zero hlc) or import another app's state (device 0) never
/// do this, so they never override a real edit.
pub(crate) fn local(
    c: &Connection,
    coll: u8,
    parts: &[&str],
    hlc: &str,
    write: impl Fn(&str) -> rusqlite::Result<usize>,
) -> rusqlite::Result<()> {
    let mut changed = write(hlc)?;
    if changed == 0
        && let Some(mine) = Hlc::decode(hlc).filter(|h| h.wall_ms > 0 && h.device != 0)
        && let Some(stored) = stored_hlc(c, coll, &key(parts))?
            .as_deref()
            .and_then(Hlc::decode)
        && stored > mine
    {
        changed = write(&stored.next_for(mine.device).encode())?;
    }
    queue(c, changed, coll, parts)
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

    /// Replaces the list when `hlc` is newer than the list's register; local
    /// edits are guarded too, since a pull can apply a newer list between
    /// an edit taking its hlc and reaching the writer. Returns 1 on change.
    pub fn set_addons(
        c: &Connection,
        profile: &str,
        addons: &[AddonRow],
        hlc: &str,
    ) -> rusqlite::Result<usize> {
        let won = c.execute(
            "INSERT INTO addon_list (profile_id, hlc) VALUES (?1, ?2)
             ON CONFLICT(profile_id) DO UPDATE SET hlc = excluded.hlc
             WHERE excluded.hlc > addon_list.hlc",
            params![profile, hlc],
        )?;
        if won == 0 {
            return Ok(0);
        }
        c.execute("DELETE FROM addons WHERE profile_id = ?1", params![profile])?;
        let mut insert = c.prepare(
            "INSERT INTO addons (profile_id, transport_url, manifest_json, position, enabled, allow_private, hlc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for (position, a) in addons.iter().enumerate() {
            insert.execute(params![
                profile,
                a.transport_url,
                a.manifest_json,
                position as i64,
                a.enabled,
                a.allow_private,
                hlc
            ])?;
        }
        Ok(1)
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
