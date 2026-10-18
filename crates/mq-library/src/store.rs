use crate::schema::migrations;
use crate::sync::{Gone, coll, local, queue, refuse_gone, sql};
use rusqlite::hooks::Action;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Sqlite(rusqlite::Error),
    #[error(transparent)]
    Migration(#[from] rusqlite_migration::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// A write for a profile removed for good.
    #[error("profile {0} was removed")]
    ProfileGone(String),
    #[error("the owner profile cannot be removed")]
    OwnerProfile,
}

impl From<rusqlite::Error> for LibraryError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::ToSqlConversionFailure(e) => match e.downcast::<Gone>() {
                Ok(gone) => Self::ProfileGone(gone.0),
                Err(e) => Self::Sqlite(rusqlite::Error::ToSqlConversionFailure(e)),
            },
            e => Self::Sqlite(e),
        }
    }
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
    /// Every table, for listeners that missed changes and must refresh all.
    pub const ALL: [Table; 8] = [
        Table::Profiles,
        Table::Addons,
        Table::LibraryItems,
        Table::Progress,
        Table::Watched,
        Table::StreamMemory,
        Table::Settings,
        Table::SearchHistory,
    ];

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            // A profile's PIN and its removal show in its picker entry.
            "profiles" | "profile_lock" | "profile_gone" => Table::Profiles,
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
    /// The video of that write, or `None` when nothing was played.
    pub video_id: Option<String>,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// That video (the title itself when nothing was played) is marked
    /// watched, and no later play started over.
    pub watched: bool,
}

/// Position past which a video counts as finished for Continue Watching.
pub const FINISHED_RATIO: f64 = 0.92;

pub struct Library {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    next_reader: AtomicUsize,
    changes: broadcast::Sender<Vec<Table>>,
    /// Tables the open write transaction touched, announced once it has
    /// committed (a listener reading at once must see the change).
    touched: Arc<Mutex<Vec<Table>>>,
    temp_path: Option<PathBuf>,
    /// Bumped (under the writer) whenever a profile is removed. Cached addon
    /// answers carry the count they were fetched under, and only the
    /// current count may be stored or served: a fetch begun before a
    /// removal never puts the removed profile's URLs back.
    removals: AtomicU64,
    /// The open write transaction removes a profile; the count moves once
    /// it has committed, so no reader sees the new count with the old rows.
    removing: AtomicBool,
    /// A removal's log could not be emptied (a reader was in the middle of
    /// a read); the next read or write tries again.
    scrub_pending: AtomicBool,
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
        let touched = Arc::new(Mutex::new(Vec::new()));
        install_hooks(&writer, touched.clone())?;
        let readers = (0..readers)
            .map(|_| open_connection(&target).map(Mutex::new))
            .collect::<Result<_, _>>()?;
        let library = Self {
            writer: Mutex::new(writer),
            readers,
            next_reader: AtomicUsize::new(0),
            changes,
            touched,
            temp_path: None,
            removals: AtomicU64::new(0),
            removing: AtomicBool::new(false),
            scrub_pending: AtomicBool::new(false),
        };
        // Rows a migration dropped, or a removal whose scrub a crash cut
        // short, leave the files now.
        library.checkpoint();
        Ok(library)
    }

    /// Receives the set of tables touched by each committed write transaction.
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<Table>> {
        self.changes.subscribe()
    }

    pub(crate) fn write<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, LibraryError> {
        let out = {
            let mut conn = self.writer.lock().unwrap_or_else(|p| p.into_inner());
            let committed = commit(&mut conn, f);
            let touched = std::mem::take(&mut *lock(&self.touched));
            let removing = self.removing.swap(false, Ordering::SeqCst);
            // Still under the writer, so announcements keep commit order.
            if committed.is_ok() {
                if removing {
                    self.removals.fetch_add(1, Ordering::SeqCst);
                }
                if !touched.is_empty() {
                    let _ = self.changes.send(touched);
                }
            }
            committed?
        };
        self.scrub_if_pending();
        Ok(out)
    }

    /// Moves the write-ahead log into the database and empties it, so the
    /// old copies of rows deleted since are gone from the files too. A
    /// reader in the middle of a read holds it up: then it is tried again
    /// after the next read or write.
    pub(crate) fn checkpoint(&self) {
        let conn = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        // Wait only briefly for readers; the retry covers the rest.
        let _ = conn.busy_timeout(std::time::Duration::from_millis(50));
        let busy = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            r.get::<_, i64>(0)
        });
        let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
        let done = match busy {
            Ok(busy) => busy == 0,
            Err(e) => {
                tracing::warn!("checkpoint after a profile removal failed: {e}");
                false
            }
        };
        self.scrub_pending.store(!done, Ordering::SeqCst);
    }

    fn scrub_if_pending(&self) {
        if self.scrub_pending.load(Ordering::SeqCst) {
            self.checkpoint();
        }
    }

    /// Marks the open write transaction as removing a profile: the count
    /// moves when it commits.
    pub(crate) fn mark_removal(&self) {
        self.removing.store(true, Ordering::SeqCst);
    }

    /// How many profiles were removed while this library was open: cached
    /// addon answers fetched under another count are not used.
    pub fn removals(&self) -> u64 {
        self.removals.load(Ordering::SeqCst)
    }

    pub(crate) fn read<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, LibraryError> {
        let out = {
            let index = self.next_reader.fetch_add(1, Ordering::Relaxed) % self.readers.len();
            let conn = self.readers[index]
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            f(&conn)?
        };
        self.scrub_if_pending();
        Ok(out)
    }

    /// Creates the profile and its lock register (no PIN), each only when
    /// missing. Called with the zero hlc, so versions synced from another
    /// device win.
    pub fn ensure_profile(&self, id: &str, name: &str, hlc: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            refuse_gone(c, id)?;
            let added = c.execute(
                "INSERT INTO profiles (id, name, hlc) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO NOTHING",
                params![id, name, hlc],
            )?;
            queue(c, added, coll::PROFILES, &[id])?;
            let added = c.execute(
                "INSERT INTO profile_lock (profile_id, pin_hash, hlc) VALUES (?1, NULL, ?2)
                 ON CONFLICT(profile_id) DO NOTHING",
                params![id, hlc],
            )?;
            queue(c, added, coll::PROFILE_LOCK, &[id])
        })
    }

    /// Replaces the profile's addon list with `addons` in one transaction,
    /// keeping order. An edit of the stored list goes through `edit_addons`.
    pub fn set_addons(
        &self,
        profile: &str,
        addons: &[AddonRow],
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            local(c, coll::ADDONS, &[profile], hlc, |h| {
                sql::set_addons(c, profile, addons, h)
            })
        })
    }

    /// Edits the profile's addon list in one write transaction: `edit` gets
    /// the list as stored and what it leaves is written, positions in its
    /// order. Nothing (a pull, another edit) lands between the read and the
    /// write, so the edit never rewrites a list it did not see. An error
    /// from `edit` writes nothing and is returned.
    pub fn edit_addons<E: From<LibraryError>>(
        &self,
        profile: &str,
        hlc: &str,
        edit: impl FnOnce(&mut Vec<AddonRow>) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut refused = None;
        self.write(|c| {
            refuse_gone(c, profile)?;
            let mut rows = addon_rows(c, profile)?;
            if let Err(e) = edit(&mut rows) {
                refused = Some(e);
                return Ok(());
            }
            local(c, coll::ADDONS, &[profile], hlc, |h| {
                sql::set_addons(c, profile, &rows, h)
            })
        })?;
        refused.map_or(Ok(()), Err)
    }

    pub fn addons(&self, profile: &str) -> Result<Vec<AddonRow>, LibraryError> {
        self.read(|c| addon_rows(c, profile))
    }

    pub fn upsert_item(
        &self,
        profile: &str,
        item: &LibraryItemRow,
        in_library: bool,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            local(c, coll::ITEMS, &[profile, &item.item_id], hlc, |h| {
                sql::upsert_item(c, profile, item, in_library, h)
            })
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
            local(c, coll::PROGRESS, &[profile, p.video_id], hlc, |h| {
                sql::set_progress(c, profile, &p, h)
            })
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
            local(c, coll::WATCHED, &[profile, video_id], hlc, |h| {
                sql::set_watched(c, profile, item_id, video_id, watched, h)
            })
        })
    }

    /// Which of `video_ids` are watched, in one read.
    pub fn watched_among(
        &self,
        profile: &str,
        video_ids: &[&str],
    ) -> Result<std::collections::HashSet<String>, LibraryError> {
        if video_ids.is_empty() {
            return Ok(Default::default());
        }
        self.read(|c| {
            let marks = vec!["?"; video_ids.len()].join(",");
            let mut stmt = c.prepare(&format!(
                "SELECT video_id FROM watched WHERE profile_id = ? AND watched = 1 AND video_id IN ({marks})"
            ))?;
            let args = std::iter::once(&profile).chain(video_ids.iter());
            stmt.query_map(rusqlite::params_from_iter(args), |r| r.get(0))?
                .collect()
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
        // Progress written after the watched mark (a newer register) is a
        // rewatch in progress: unfinished again until it is marked again.
        let partial =
            "p.position_ms > 0 AND (p.duration_ms = 0 OR p.position_ms < p.duration_ms * ?2)";
        let condition = if finished {
            format!(
                "COALESCE(i.kind, 'movie') = 'series'
                   AND ((p.duration_ms > 0 AND p.position_ms >= p.duration_ms * ?2)
                        OR (COALESCE(w.watched, 0) = 1 AND NOT (p.hlc > w.hlc AND {partial})))"
            )
        } else {
            format!("{partial} AND (COALESCE(w.watched, 0) = 0 OR p.hlc > w.hlc)")
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
                   AND p.video_id = (SELECT q.video_id FROM progress q
                                     WHERE q.profile_id = p.profile_id AND q.item_id = p.item_id
                                     ORDER BY q.updated_ms DESC, q.video_id DESC LIMIT 1)
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

    /// Keeps another app's raw answer for `profile` (removed with it).
    pub fn store_raw_import(
        &self,
        profile: &str,
        source: &str,
        imported_ms: i64,
        payload: &str,
    ) -> Result<i64, LibraryError> {
        self.write(|c| {
            refuse_gone(c, profile)?;
            c.execute(
                "INSERT INTO import_raw (profile_id, source, imported_ms, payload) VALUES (?1, ?2, ?3, ?4)",
                params![profile, source, imported_ms, payload],
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
    // Deleted rows (a removed profile's) are overwritten, not left in free
    // pages.
    conn.pragma_update(None, "secure_delete", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

/// Collects touched tables during a transaction and broadcasts them on commit.
/// One write transaction: committed when `f` succeeds, rolled back (on
/// drop) otherwise.
/// The profile's addons in their order.
fn addon_rows(c: &Connection, profile: &str) -> rusqlite::Result<Vec<AddonRow>> {
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
}

fn commit<T>(
    conn: &mut Connection,
    f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> Result<T, LibraryError> {
    let tx = conn.transaction()?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// Collects the tables a transaction touches; `Library::write` announces
/// them after the commit. SQLite's commit hook runs before the commit is
/// visible, so a listener woken from it could read the old rows.
fn install_hooks(conn: &Connection, touched: Arc<Mutex<Vec<Table>>>) -> rusqlite::Result<()> {
    let on_update = touched.clone();
    conn.update_hook(Some(move |_: Action, _db: &str, table: &str, _row: i64| {
        if let Some(t) = Table::from_name(table) {
            let mut tables = lock(&on_update);
            if !tables.contains(&t) {
                tables.push(t);
            }
        }
    }))?;
    conn.rollback_hook(Some(move || lock(&touched).clear()))?;
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

    /// Stores an addon answer fetched while `removals` was the count; one
    /// that a profile removal overtook is dropped (checked under the writer,
    /// which every removal holds). Returns whether it was stored.
    pub fn cache_put(
        &self,
        key: &str,
        value: &CachedResponse,
        removals: u64,
    ) -> Result<bool, LibraryError> {
        self.write(|c| {
            if self.removals() != removals {
                return Ok(false);
            }
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
            Ok(true)
        })
    }
}

impl Library {
    /// Titles the user added (`in_library`), each with its newest progress row.
    pub fn library(&self, profile: &str) -> Result<Vec<LibraryEntry>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT i.item_id, i.kind, i.name, i.poster, i.poster_shape, i.added_ms,
                        p.updated_ms, COALESCE(p.position_ms, 0), COALESCE(p.duration_ms, 0),
                        p.video_id,
                        COALESCE(w.watched, 0) = 1 AND (p.hlc IS NULL OR NOT p.hlc > w.hlc)
                 FROM library_items i
                 LEFT JOIN progress p ON p.profile_id = i.profile_id AND p.item_id = i.item_id
                   AND p.video_id = (SELECT q.video_id FROM progress q
                                     WHERE q.profile_id = i.profile_id AND q.item_id = i.item_id
                                     ORDER BY q.updated_ms DESC, q.video_id DESC LIMIT 1)
                 LEFT JOIN watched w ON w.profile_id = i.profile_id
                   AND w.video_id = COALESCE(p.video_id, i.item_id)
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
                    video_id: r.get(9)?,
                    watched: r.get(10)?,
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
    /// (video id, position ms, duration ms). The position is 0 when the
    /// video was marked watched after it, so there is nothing to resume (the
    /// rewatch rule of Continue Watching).
    pub fn latest_progress(
        &self,
        profile: &str,
        item_id: &str,
    ) -> Result<Option<(String, i64, i64)>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT p.video_id,
                        CASE WHEN COALESCE(w.watched, 0) = 1 AND NOT p.hlc > w.hlc
                             THEN 0 ELSE p.position_ms END,
                        p.duration_ms
                 FROM progress p
                 LEFT JOIN watched w ON w.profile_id = p.profile_id AND w.video_id = p.video_id
                 WHERE p.profile_id = ?1 AND p.item_id = ?2
                 ORDER BY p.updated_ms DESC, p.video_id DESC LIMIT 1",
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
            local(c, coll::STREAM_MEMORY, &[profile, item_id], hlc, |h| {
                sql::set_stream_memory(c, profile, item_id, binge_group, stream_json, h)
            })
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
            local(c, coll::SETTINGS, &[profile, key], hlc, |h| {
                sql::set_setting(c, profile, key, value_json, h)
            })
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
            refuse_gone(c, profile)?;
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
