//! Registers as sync records. Every synced row is a last-writer-wins register
//! with an HLC; export walks them in `(hlc, coll, key)` order so a cursor
//! never skips rows that share an HLC (an import writes thousands with one),
//! and apply runs the same upsert-if-newer SQL as local writes.

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

fn key(parts: &[&str]) -> String {
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
                out.extend(export(c, coll, after, limit)?);
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
fn export(
    c: &Connection,
    coll: u8,
    after: &SyncCursor,
    limit: usize,
) -> rusqlite::Result<Vec<SyncRecord>> {
    let sep = SEP.to_string();
    let (sql, key_expr) = match coll {
        coll::PROFILES => ("SELECT id, name, hlc FROM profiles", "id"),
        coll::ADDONS => ("SELECT profile_id, hlc FROM addon_list", "profile_id"),
        coll::ITEMS => (
            "SELECT profile_id, item_id, kind, name, poster, poster_shape, in_library, added_ms, hlc FROM library_items",
            "profile_id || ?5 || item_id",
        ),
        coll::PROGRESS => (
            "SELECT profile_id, video_id, item_id, position_ms, duration_ms, updated_ms, hlc FROM progress",
            "profile_id || ?5 || video_id",
        ),
        coll::WATCHED => (
            "SELECT profile_id, video_id, item_id, watched, hlc FROM watched",
            "profile_id || ?5 || video_id",
        ),
        coll::SETTINGS => (
            "SELECT profile_id, key, value_json, hlc FROM settings",
            "profile_id || ?5 || key",
        ),
        coll::STREAM_MEMORY => (
            "SELECT profile_id, item_id, binge_group, stream_json, hlc FROM stream_memory",
            "profile_id || ?5 || item_id",
        ),
        _ => return Ok(Vec::new()),
    };
    let query = format!(
        "{sql} WHERE (hlc, ?6, {key_expr}) > (?1, ?2, ?3) ORDER BY hlc, {key_expr} LIMIT ?4"
    );
    let mut stmt = c.prepare(&query)?;
    let args = params![after.hlc, after.coll, after.key, limit as i64, sep, coll];
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
    Shape(serde_json::Error),
    Sql(rusqlite::Error),
}

impl From<serde_json::Error> for ApplyError {
    fn from(e: serde_json::Error) -> Self {
        Self::Shape(e)
    }
}

impl From<rusqlite::Error> for ApplyError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}

fn apply_one(c: &Connection, r: &SyncRecord) -> Result<usize, ApplyError> {
    let value = r.value.clone();
    let hlc = r.hlc.as_str();
    Ok(match r.coll {
        coll::PROFILES => {
            let v: ProfileValue = serde_json::from_value(value)?;
            c.execute(
                "INSERT INTO profiles (id, name, hlc) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, hlc = excluded.hlc
                 WHERE excluded.hlc > profiles.hlc",
                params![v.profile, v.name, hlc],
            )?
        }
        coll::ADDONS => {
            let v: AddonsValue = serde_json::from_value(value)?;
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
            sql::set_addons(c, &v.profile, &rows, hlc, true)?
        }
        coll::ITEMS => {
            let v: ItemValue = serde_json::from_value(value)?;
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
            sql::set_watched(c, &v.profile, &v.item_id, &v.video_id, v.watched, hlc)?
        }
        coll::SETTINGS => {
            let v: SettingValue = serde_json::from_value(value)?;
            sql::set_setting(c, &v.profile, &v.key, &v.value_json, hlc)?
        }
        coll::STREAM_MEMORY => {
            let v: MemoryValue = serde_json::from_value(value)?;
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

/// The one copy of each register write, shared by local writes and sync.
pub(crate) mod sql {
    use super::*;

    /// `only_if_newer` guards the list by its register HLC (sync apply); a
    /// local edit replaces it outright. Returns 1 when the list changed.
    pub fn set_addons(
        c: &Connection,
        profile: &str,
        addons: &[AddonRow],
        hlc: &str,
        only_if_newer: bool,
    ) -> rusqlite::Result<usize> {
        let guard = if only_if_newer {
            " WHERE excluded.hlc > addon_list.hlc"
        } else {
            ""
        };
        let won = c.execute(
            &format!(
                "INSERT INTO addon_list (profile_id, hlc) VALUES (?1, ?2)
                 ON CONFLICT(profile_id) DO UPDATE SET hlc = excluded.hlc{guard}"
            ),
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
