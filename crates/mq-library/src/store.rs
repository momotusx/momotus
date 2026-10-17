use crate::schema::migrations;
use crate::sync::{coll, queue, sql};
use rusqlite::hooks::Action;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Migration(#[from] rusqlite_migration::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Tables whose changes live views care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
    Profiles,
    Addons,
    LibraryItems,
    Progress,
    Watched,
    StreamMemory,
    Settings,
    SearchHistory,
}

impl Table {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "profiles" => Table::Profiles,
            "addons" => Table::Addons,
            "library_items" => Table::LibraryItems,
            "progress" => Table::Progress,
            "watched" => Table::Watched,
            "stream_memory" => Table::StreamMemory,
            "settings" => Table::Settings,
            "search_history" => Table::SearchHistory,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonRow {
    pub transport_url: String,
    pub manifest_json: String,
    pub position: i64,
    pub enabled: bool,
    pub allow_private: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryItemRow {
    pub item_id: String,
    pub kind: String,
    pub name: String,
    pub poster: Option<String>,
    pub poster_shape: Option<String>,
    pub added_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressRow<'a> {
    pub video_id: &'a str,
    pub item_id: &'a str,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub updated_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueItem {
    pub item_id: String,
    pub video_id: String,
    pub name: String,
    pub poster: Option<String>,
    pub kind: String,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub updated_ms: i64,
}

/// A title in the library with its latest progress, for the Library screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub item: LibraryItemRow,
    /// Newest progress write for any of its videos.
    pub last_watched_ms: Option<i64>,
    pub position_ms: i64,
    pub duration_ms: i64,
}

/// Position past which a video counts as finished for Continue Watching.
pub const FINISHED_RATIO: f64 = 0.92;

pub struct Library {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    next_reader: AtomicUsize,
    changes: broadcast::Sender<Vec<Table>>,
    temp_path: Option<PathBuf>,
}

impl Library {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LibraryError> {
        Self::open_with(path.as_ref().to_path_buf(), 3)
    }

    /// A throwaway database in the temp directory, removed on drop. Used by
    /// tests and the CLI; WAL needs a real file for concurrent readers.
    pub fn temporary() -> Result<Self, LibraryError> {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("mq-library-{}-{n}.sqlite", std::process::id()));
        let mut lib = Self::open_with(path.clone(), 2)?;
        lib.temp_path = Some(path);
        Ok(lib)
    }

    fn open_with(path: PathBuf, readers: usize) -> Result<Self, LibraryError> {
        let target = path.to_string_lossy().into_owned();
        let mut writer = open_connection(&target)?;
        migrations().to_latest(&mut writer)?;
        let (changes, _) = broadcast::channel(64);
        let pending = Arc::new(Mutex::new(Vec::new()));
        install_hooks(&writer, pending, changes.clone())?;
        let readers = (0..readers)
            .map(|_| open_connection(&target).map(Mutex::new))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            writer: Mutex::new(writer),
            readers,
            next_reader: AtomicUsize::new(0),
            changes,
            temp_path: None,
        })
    }

    /// Receives the set of tables touched by each committed write transaction.
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<Table>> {
        self.changes.subscribe()
    }

    pub(crate) fn write<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, LibraryError> {
        let mut conn = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        let tx = conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    pub(crate) fn read<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, LibraryError> {
        let index = self.next_reader.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        let conn = self.readers[index]
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        Ok(f(&conn)?)
    }

    pub fn ensure_profile(&self, id: &str, name: &str, hlc: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            let added = c.execute(
                "INSERT INTO profiles (id, name, hlc) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO NOTHING",
                params![id, name, hlc],
            )?;
            queue(c, added, coll::PROFILES, &[id])
        })
    }

    /// Replaces the profile's addon list in one transaction, keeping order.
    /// Last writer wins like every register: a newer list pulled meanwhile is
    /// kept.
    pub fn set_addons(
        &self,
        profile: &str,
        addons: &[AddonRow],
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let n = sql::set_addons(c, profile, addons, hlc)?;
            queue(c, n, coll::ADDONS, &[profile])
        })
    }

    pub fn addons(&self, profile: &str) -> Result<Vec<AddonRow>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT transport_url, manifest_json, position, enabled, allow_private
                 FROM addons WHERE profile_id = ?1 ORDER BY position",
            )?;
            stmt.query_map(params![profile], |r| {
                Ok(AddonRow {
                    transport_url: r.get(0)?,
                    manifest_json: r.get(1)?,
                    position: r.get(2)?,
                    enabled: r.get(3)?,
                    allow_private: r.get(4)?,
                })
            })?
            .collect()
        })
    }

    pub fn upsert_item(
        &self,
        profile: &str,
        item: &LibraryItemRow,
        in_library: bool,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let n = sql::upsert_item(c, profile, item, in_library, hlc)?;
            queue(c, n, coll::ITEMS, &[profile, &item.item_id])
        })
    }

    /// Records playback progress. Older HLCs never overwrite newer ones.
    pub fn set_progress(
        &self,
        profile: &str,
        p: ProgressRow<'_>,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let n = sql::set_progress(c, profile, &p, hlc)?;
            queue(c, n, coll::PROGRESS, &[profile, p.video_id])
        })
    }

    pub fn progress(
        &self,
        profile: &str,
        video_id: &str,
    ) -> Result<Option<(i64, i64)>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT position_ms, duration_ms FROM progress WHERE profile_id = ?1 AND video_id = ?2",
                params![profile, video_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })
    }

    pub fn set_watched(
        &self,
        profile: &str,
        item_id: &str,
        video_id: &str,
        watched: bool,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let n = sql::set_watched(c, profile, item_id, video_id, watched, hlc)?;
            queue(c, n, coll::WATCHED, &[profile, video_id])
        })
    }

    pub fn watched_videos(
        &self,
        profile: &str,
        item_id: &str,
    ) -> Result<Vec<String>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT video_id FROM watched WHERE profile_id = ?1 AND item_id = ?2 AND watched = 1 ORDER BY video_id",
            )?;
            stmt.query_map(params![profile, item_id], |r| r.get(0))?.collect()
        })
    }

    /// Most recent unfinished video per title, newest first.
    pub fn continue_watching(
        &self,
        profile: &str,
        limit: usize,
    ) -> Result<Vec<ContinueItem>, LibraryError> {
        self.latest_per_title(profile, limit, false)
    }

    /// Series whose most recent video was finished, newest first: the
    /// candidates for Next Up.
    pub fn finished_series(
        &self,
        profile: &str,
        limit: usize,
    ) -> Result<Vec<ContinueItem>, LibraryError> {
        self.latest_per_title(profile, limit, true)
    }

    fn latest_per_title(
        &self,
        profile: &str,
        limit: usize,
        finished: bool,
    ) -> Result<Vec<ContinueItem>, LibraryError> {
        let condition = if finished {
            "COALESCE(i.kind, 'movie') = 'series'
                   AND ((p.duration_ms > 0 AND p.position_ms >= p.duration_ms * ?2) OR COALESCE(w.watched, 0) = 1)"
        } else {
            "p.position_ms > 0
                   AND (p.duration_ms = 0 OR p.position_ms < p.duration_ms * ?2)
                   AND COALESCE(w.watched, 0) = 0"
        };
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT p.item_id, p.video_id, COALESCE(i.name, p.item_id), i.poster, COALESCE(i.kind, 'movie'),
                        p.position_ms, p.duration_ms, p.updated_ms
                 FROM progress p
                 LEFT JOIN library_items i ON i.profile_id = p.profile_id AND i.item_id = p.item_id
                 LEFT JOIN watched w ON w.profile_id = p.profile_id AND w.video_id = p.video_id
                 WHERE p.profile_id = ?1
                   AND {condition}
                   AND p.updated_ms = (SELECT MAX(q.updated_ms) FROM progress q
                                       WHERE q.profile_id = p.profile_id AND q.item_id = p.item_id)
                 ORDER BY p.updated_ms DESC
                 LIMIT ?3",
            ))?;
            stmt.query_map(params![profile, FINISHED_RATIO, limit as i64], |r| {
                Ok(ContinueItem {
                    item_id: r.get(0)?,
                    video_id: r.get(1)?,
                    name: r.get(2)?,
                    poster: r.get(3)?,
                    kind: r.get(4)?,
                    position_ms: r.get(5)?,
                    duration_ms: r.get(6)?,
                    updated_ms: r.get(7)?,
                })
            })?
            .collect()
        })
    }

    pub fn store_raw_import(
        &self,
        source: &str,
        imported_ms: i64,
        payload: &str,
    ) -> Result<i64, LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO import_raw (source, imported_ms, payload) VALUES (?1, ?2, ?3)",
                params![source, imported_ms, payload],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn raw_import(&self, id: i64) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT payload FROM import_raw WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
        })
    }
}

fn open_connection(target: &str) -> Result<Connection, LibraryError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(target, flags)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

/// Collects touched tables during a transaction and broadcasts them on commit.
fn install_hooks(
    conn: &Connection,
    pending: Arc<Mutex<Vec<Table>>>,
    tx: broadcast::Sender<Vec<Table>>,
) -> rusqlite::Result<()> {
    let on_update = pending.clone();
    conn.update_hook(Some(move |_: Action, _db: &str, table: &str, _row: i64| {
        if let Some(t) = Table::from_name(table) {
            let mut tables = on_update.lock().unwrap_or_else(|p| p.into_inner());
            if !tables.contains(&t) {
                tables.push(t);
            }
        }
    }))?;
    let on_commit = pending.clone();
    conn.commit_hook(Some(move || {
        let tables = std::mem::take(&mut *on_commit.lock().unwrap_or_else(|p| p.into_inner()));
        if !tables.is_empty() {
            let _ = tx.send(tables);
        }
        false
    }))?;
    conn.rollback_hook(Some(move || {
        pending.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }))?;
    Ok(())
}

impl Drop for Library {
    fn drop(&mut self) {
        if let Some(path) = self.temp_path.take() {
            for suffix in ["", "-wal", "-shm"] {
                let mut p = path.clone().into_os_string();
                p.push(suffix);
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

/// One persisted HTTP response; durations in seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedResponse {
    pub body: Vec<u8>,
    pub stored_ms: i64,
    pub max_age_s: i64,
    pub stale_revalidate_s: i64,
    pub stale_error_s: i64,
}

/// Rows kept in the HTTP cache; the oldest are trimmed past this.
const HTTP_CACHE_ROWS: i64 = 2000;

impl Library {
    pub fn cache_get(&self, key: &str) -> Result<Option<CachedResponse>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT body, stored_ms, max_age_s, stale_revalidate_s, stale_error_s FROM http_cache WHERE key = ?1",
                params![key],
                |r| {
                    Ok(CachedResponse {
                        body: r.get(0)?,
                        stored_ms: r.get(1)?,
                        max_age_s: r.get(2)?,
                        stale_revalidate_s: r.get(3)?,
                        stale_error_s: r.get(4)?,
                    })
                },
            )
            .optional()
        })
    }

    pub fn cache_put(&self, key: &str, value: &CachedResponse) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO http_cache (key, body, stored_ms, max_age_s, stale_revalidate_s, stale_error_s)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![key, value.body, value.stored_ms, value.max_age_s, value.stale_revalidate_s, value.stale_error_s],
            )?;
            c.execute(
                "DELETE FROM http_cache WHERE key IN (
                   SELECT key FROM http_cache ORDER BY stored_ms DESC LIMIT -1 OFFSET ?1)",
                params![HTTP_CACHE_ROWS],
            )?;
            Ok(())
        })
    }
}

impl Library {
    /// Titles the user added (`in_library`), each with its newest progress row.
    pub fn library(&self, profile: &str) -> Result<Vec<LibraryEntry>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT i.item_id, i.kind, i.name, i.poster, i.poster_shape, i.added_ms,
                        p.updated_ms, COALESCE(p.position_ms, 0), COALESCE(p.duration_ms, 0)
                 FROM library_items i
                 LEFT JOIN progress p ON p.profile_id = i.profile_id AND p.item_id = i.item_id
                   AND p.updated_ms = (SELECT MAX(q.updated_ms) FROM progress q
                                       WHERE q.profile_id = i.profile_id AND q.item_id = i.item_id)
                 WHERE i.profile_id = ?1 AND i.in_library = 1",
            )?;
            stmt.query_map(params![profile], |r| {
                Ok(LibraryEntry {
                    item: LibraryItemRow {
                        item_id: r.get(0)?,
                        kind: r.get(1)?,
                        name: r.get(2)?,
                        poster: r.get(3)?,
                        poster_shape: r.get(4)?,
                        added_ms: r.get(5)?,
                    },
                    last_watched_ms: r.get(6)?,
                    position_ms: r.get(7)?,
                    duration_ms: r.get(8)?,
                })
            })?
            .collect()
        })
    }

    pub fn is_in_library(&self, profile: &str, item_id: &str) -> Result<bool, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT in_library FROM library_items WHERE profile_id = ?1 AND item_id = ?2",
                params![profile, item_id],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or(false))
        })
    }

    /// The most recently updated progress row for a title:
    /// (video id, position ms, duration ms).
    pub fn latest_progress(
        &self,
        profile: &str,
        item_id: &str,
    ) -> Result<Option<(String, i64, i64)>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT video_id, position_ms, duration_ms FROM progress
                 WHERE profile_id = ?1 AND item_id = ?2 ORDER BY updated_ms DESC LIMIT 1",
                params![profile, item_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
        })
    }

    /// Remembers the stream played for a title, so its next episode can
    /// prefer the same release line.
    pub fn set_stream_memory(
        &self,
        profile: &str,
        item_id: &str,
        binge_group: Option<&str>,
        stream_json: &str,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let n = sql::set_stream_memory(c, profile, item_id, binge_group, stream_json, hlc)?;
            queue(c, n, coll::STREAM_MEMORY, &[profile, item_id])
        })
    }

    /// The binge group of the stream last played for a title.
    pub fn last_binge_group(
        &self,
        profile: &str,
        item_id: &str,
    ) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT binge_group FROM stream_memory WHERE profile_id = ?1 AND item_id = ?2",
                params![profile, item_id],
                |r| r.get(0),
            )
            .optional()
            .map(Option::flatten)
        })
    }

    /// Every setting of the profile as (key, JSON value), oldest write first,
    /// so a reader that layers them ends with the newest.
    pub fn settings(&self, profile: &str) -> Result<Vec<(String, String)>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT key, value_json FROM settings WHERE profile_id = ?1 ORDER BY hlc",
            )?;
            stmt.query_map(params![profile], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
    }

    /// One setting, last writer wins by HLC like every synced row.
    pub fn set_setting(
        &self,
        profile: &str,
        key: &str,
        value_json: &str,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let n = sql::set_setting(c, profile, key, value_json, hlc)?;
            queue(c, n, coll::SETTINGS, &[profile, key])
        })
    }

    /// Remembers a search the user acted on; keeps the newest 50.
    pub fn record_search(
        &self,
        profile: &str,
        query: &str,
        used_ms: i64,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO search_history (profile_id, query, used_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT(profile_id, query) DO UPDATE SET used_ms = excluded.used_ms",
                params![profile, query, used_ms],
            )?;
            c.execute(
                "DELETE FROM search_history WHERE profile_id = ?1 AND query NOT IN
                   (SELECT query FROM search_history WHERE profile_id = ?1 ORDER BY used_ms DESC LIMIT 50)",
                params![profile],
            )
            .map(drop)
        })
    }

    pub fn recent_searches(
        &self,
        profile: &str,
        limit: usize,
    ) -> Result<Vec<String>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT query FROM search_history WHERE profile_id = ?1 ORDER BY used_ms DESC LIMIT ?2",
            )?;
            stmt.query_map(params![profile, limit as i64], |r| r.get(0))?
                .collect()
        })
    }
}
