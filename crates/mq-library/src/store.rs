use crate::schema::migrations;
use crate::sync::{
    Gone, POSITION_STEP, Stamp, coll, local, local_changed, queue, refuse_gone, sql,
};
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
    /// The owner is never a kids profile and has no allowlist.
    #[error("the owner profile cannot be a kids profile")]
    OwnerKids,
    /// An allowlist entry outside its bounds (plans/m11-kids.md 3.1).
    #[error("allowlist entry: {0}")]
    BadEntry(&'static str),
    /// A kids profile is always a new one.
    #[error("profile {0} already exists")]
    ProfileExists(String),
    /// Kids profiles and no owner PIN never stand together (plans/m11-kids.md 6.1).
    #[error("kids profiles need the owner's PIN")]
    KidsNeedPin,
    /// A new allow past the cap, on this device (plans/m11-kids.md 3.4).
    #[error("the kids list is full")]
    ListFull,
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

/// The longest recent search kept, in characters.
const SEARCH_MAX_CHARS: usize = 200;

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
    /// Kids flags and allowlists (plans/m11-kids.md).
    Access,
}

impl Table {
    /// Every table, for listeners that missed changes and must refresh all.
    pub const ALL: [Table; 9] = [
        Table::Profiles,
        Table::Addons,
        Table::LibraryItems,
        Table::Progress,
        Table::Watched,
        Table::StreamMemory,
        Table::Settings,
        Table::SearchHistory,
        Table::Access,
    ];

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            // A profile's PIN and its removal show in its picker entry.
            "profiles" | "profile_lock" | "profile_gone" => Table::Profiles,
            "addons" => Table::Addons,
            "library_items" => Table::LibraryItems,
            // A removal from Continue Watching shows where progress does.
            "progress" | "dismissed" => Table::Progress,
            "watched" => Table::Watched,
            "stream_memory" => Table::StreamMemory,
            "settings" => Table::Settings,
            "search_history" => Table::SearchHistory,
            "profile_access" | "allowed" => Table::Access,
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
    /// tests and the CLI; WAL needs a real file for concurrent readers. The
    /// first one in a process also clears what ended processes left behind.
    pub fn temporary() -> Result<Self, LibraryError> {
        static SWEPT: std::sync::Once = std::sync::Once::new();
        SWEPT.call_once(|| sweep_temporaries(&std::env::temp_dir()));
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("mq-library-{}-{n}.sqlite", std::process::id()));
        Self::temporary_at(path)
    }

    /// A throwaway database at `path`, cleared first: a file left there by
    /// an ended process whose pid this one now has holds that run's rows
    /// (code review 2026-10-20 r3 P3-7).
    #[doc(hidden)]
    pub fn temporary_at(path: PathBuf) -> Result<Self, LibraryError> {
        remove_database(&path);
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

    /// Forgets everything this device held of its account and profiles (a
    /// sign-out that removes them, D13): every table is emptied in one
    /// transaction, this device's id and the hosts relay ids were pinned to
    /// aside, and the files are scrubbed after. Every view hears it. The
    /// caller makes the device fresh again (its owner, the defaults step).
    pub fn wipe(&self) -> Result<(), LibraryError> {
        self.write(|c| {
            self.mark_removal();
            let tables: Vec<String> = {
                let mut stmt = c.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                )?;
                stmt.query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            for table in tables {
                let delete = match table.as_str() {
                    "sync_state" => "DELETE FROM sync_state \
                         WHERE key != 'device_id' AND substr(key, 1, 6) != 'relay:'"
                        .to_owned(),
                    _ => format!("DELETE FROM \"{table}\""),
                };
                c.execute(&delete, [])?;
            }
            // A whole-table delete skips the update hook: announce them all.
            *lock(&self.touched) = Table::ALL.to_vec();
            Ok(())
        })?;
        self.checkpoint();
        Ok(())
    }

    /// Announces `table` with the write transaction open on this thread,
    /// for a change no table hook sees (a device value a view reads).
    pub(crate) fn announce(&self, table: Table) {
        let mut touched = lock(&self.touched);
        if !touched.contains(&table) {
            touched.push(table);
        }
    }

    /// Sets a device value that a view reads through `table`, announcing
    /// `table` with the commit (the defaults marker changes the addons
    /// view's `fresh`).
    pub fn set_device_value_announced(
        &self,
        key: &str,
        value: &str,
        table: Table,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO device_state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            self.announce(table);
            Ok(())
        })
    }

    /// Deletes a device value that a view reads through `table`, announcing
    /// `table` with the commit.
    pub fn delete_device_value_announced(
        &self,
        key: &str,
        table: Table,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute("DELETE FROM device_state WHERE key = ?1", params![key])?;
            self.announce(table);
            Ok(())
        })
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
    /// missing, and its access register (standard) for the owner or a
    /// profile this call creates. The core and test cores pass the zero
    /// hlc, so versions synced from another device win; the importer passes
    /// its import hlc. A profile already here waits for its creator's
    /// access record: guessed, a kids profile whose record is still on its
    /// way would open as standard. Only test cores and the importer's tests
    /// create a profile other than the owner here (plans/m11-kids.md 3.5).
    pub fn ensure_profile(&self, id: &str, name: &str, hlc: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            refuse_gone(c, id)?;
            let created = c.execute(
                "INSERT INTO profiles (id, name, hlc) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO NOTHING",
                params![id, name, hlc],
            )?;
            queue(c, created, coll::PROFILES, &[id])?;
            let added = c.execute(
                "INSERT INTO profile_lock (profile_id, pin_hash, hlc) VALUES (?1, NULL, ?2)
                 ON CONFLICT(profile_id) DO NOTHING",
                params![id, hlc],
            )?;
            queue(c, added, coll::PROFILE_LOCK, &[id])?;
            if id != crate::sync::OWNER && created == 0 {
                return Ok(());
            }
            let added = c.execute(
                "INSERT INTO profile_access (profile_id, kids, hlc) VALUES (?1, 0, ?2)
                 ON CONFLICT(profile_id) DO NOTHING",
                params![id, hlc],
            )?;
            queue(c, added, coll::PROFILE_ACCESS, &[id])
        })
    }

    /// Replaces the profile's addon list with `addons` in one transaction,
    /// keeping order. An edit of the stored list goes through `edit_addons`.
    pub fn set_addons<'a>(
        &self,
        profile: &str,
        addons: &[AddonRow],
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        self.edit_addons(profile, stamp, |rows: &mut Vec<AddonRow>| {
            *rows = addons.to_vec();
            Ok::<_, LibraryError>(())
        })
    }

    /// Edits the profile's addon list in one write transaction: `edit` gets
    /// the list as stored and what it leaves is written, in its order.
    /// Nothing (a pull, another edit) lands between the read and the write,
    /// so the edit never rewrites a list it did not see. An error from
    /// `edit` writes nothing and is returned.
    ///
    /// Each addon is a register of its own (plans/accounts-social-sync.md
    /// 13): only the addons the edit added, removed, changed or moved are
    /// stamped and queued, each in the transaction as every local write is
    /// (`Stamp`), so another device's edits to other addons stay. An edit
    /// that changed nothing stamps nothing.
    pub fn edit_addons<'a, E: From<LibraryError>>(
        &self,
        profile: &str,
        stamp: impl Into<Stamp<'a>>,
        edit: impl FnOnce(&mut Vec<AddonRow>) -> Result<(), E>,
    ) -> Result<(), E> {
        let stamp = stamp.into();
        let mut refused = None;
        self.write(|c| {
            refuse_gone(c, profile)?;
            let before = addon_rows(c, profile)?;
            let mut rows = before.clone();
            if let Err(e) = edit(&mut rows) {
                refused = Some(e);
                return Ok(());
            }
            write_addons(c, profile, &before, rows, &stamp)
        })?;
        refused.map_or(Ok(()), Err)
    }

    /// Puts newer manifests of installed addons in place, by link, and
    /// nothing else: the list's stamp stays and nothing is queued. A refresh
    /// caches what the addons serve on this device; it is never an edit, so
    /// it never changes which device's list wins.
    pub fn refresh_addon_manifests(
        &self,
        profile: &str,
        manifests: &[(String, String)],
    ) -> Result<usize, LibraryError> {
        self.write(|c| {
            let mut changed = 0;
            for (url, json) in manifests {
                changed += c.execute(
                    "UPDATE addons SET manifest_json = ?3
                     WHERE profile_id = ?1 AND transport_url = ?2 AND manifest_json != ?3 AND installed = 1",
                    rusqlite::params![profile, url, json],
                )?;
            }
            Ok(changed)
        })
    }

    pub fn addons(&self, profile: &str) -> Result<Vec<AddonRow>, LibraryError> {
        self.read(|c| addon_rows(c, profile))
    }

    /// The newest hlc among the profile's addon registers, installed or
    /// removed, and the list mark an older build left (`addon_list`, kept
    /// unsynced); `None` when the profile never had a list here or pulled.
    pub fn addon_list_hlc(&self, profile: &str) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT MAX(h) FROM (
                   SELECT MAX(hlc) AS h FROM addons WHERE profile_id = ?1
                   UNION ALL SELECT MAX(hlc) FROM addon_order WHERE profile_id = ?1
                   UNION ALL SELECT hlc FROM addon_list WHERE profile_id = ?1)",
                params![profile],
                |r| r.get(0),
            )
        })
    }

    pub fn upsert_item<'a>(
        &self,
        profile: &str,
        item: &LibraryItemRow,
        in_library: bool,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            local(c, coll::ITEMS, &[profile, &item.item_id], &stamp, |h| {
                sql::upsert_item(c, profile, item, in_library, h)
            })
        })
    }

    /// Records playback progress. Older HLCs never overwrite newer ones.
    pub fn set_progress<'a>(
        &self,
        profile: &str,
        p: ProgressRow<'_>,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            local(c, coll::PROGRESS, &[profile, p.video_id], &stamp, |h| {
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

    pub fn set_watched<'a>(
        &self,
        profile: &str,
        item_id: &str,
        video_id: &str,
        watched: bool,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            local(c, coll::WATCHED, &[profile, video_id], &stamp, |h| {
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

    /// Takes a title off Continue Watching and Next Up until a newer
    /// progress register of it arrives: one played again here or on
    /// another device shows it again.
    pub fn dismiss<'a>(
        &self,
        profile: &str,
        item_id: &str,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            local(c, coll::DISMISSED, &[profile, item_id], &stamp, |h| {
                sql::set_dismissed(c, profile, item_id, h)
            })
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
                "COALESCE(i.kind, 'movie') IN ('series', 'anime')
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
                   AND NOT EXISTS (SELECT 1 FROM dismissed d
                                   WHERE d.profile_id = p.profile_id AND d.item_id = p.item_id
                                     AND d.hlc > p.hlc)
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
pub(crate) fn addon_rows(c: &Connection, profile: &str) -> rusqlite::Result<Vec<AddonRow>> {
    let mut stmt = c.prepare(
        "SELECT a.transport_url, a.manifest_json, COALESCE(o.position, 0) AS place, a.enabled, a.allow_private
         FROM addons a LEFT JOIN addon_order o ON o.profile_id = a.profile_id AND o.transport_url = a.transport_url
         WHERE a.profile_id = ?1 AND a.installed = 1 ORDER BY place, a.transport_url",
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

/// The titles `items` gives (item_id, kind, name, poster, poster_shape,
/// added_ms of profile ?1), each with its newest progress row: what the
/// Library screen shows, for the library or a kids profile's list.
pub(crate) fn entries(
    c: &Connection,
    items: &str,
    args: impl rusqlite::Params,
) -> rusqlite::Result<Vec<LibraryEntry>> {
    let mut stmt = c.prepare(&format!(
        "SELECT i.item_id, i.kind, i.name, i.poster, i.poster_shape, i.added_ms,
                p.updated_ms, COALESCE(p.position_ms, 0), COALESCE(p.duration_ms, 0),
                p.video_id,
                COALESCE(w.watched, 0) = 1 AND (p.hlc IS NULL OR NOT p.hlc > w.hlc)
         FROM ({items}) i
         LEFT JOIN progress p ON p.profile_id = ?1 AND p.item_id = i.item_id
           AND p.video_id = (SELECT q.video_id FROM progress q
                             WHERE q.profile_id = ?1 AND q.item_id = i.item_id
                             ORDER BY q.updated_ms DESC, q.video_id DESC LIMIT 1)
         LEFT JOIN watched w ON w.profile_id = ?1
           AND w.video_id = COALESCE(p.video_id, i.item_id)"
    ))?;
    stmt.query_map(args, |r| {
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

/// Removes the temporary libraries (`mq-library-<pid>-<n>.sqlite` and its
/// `-wal` and `-shm`) in `dir` whose process has ended. A library that
/// is never dropped (a test that forgets a view holding its core, a run
/// that was killed) keeps its files until the process ends; one day of test
/// runs left 6,145 of them, 877 MB. Another live process's files stay.
#[doc(hidden)]
pub fn sweep_temporaries(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let me = std::process::id();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid = name
            .to_str()
            .and_then(|n| n.strip_prefix("mq-library-"))
            .and_then(|rest| rest.split('-').next())
            .and_then(|pid| pid.parse::<u32>().ok());
        if let Some(pid) = pid
            && pid != me
            && !alive(pid)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The process `pid` exists (one we may not signal counts as existing).
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return true;
    };
    // Signal 0 checks the process without signalling it.
    // SAFETY: kill with signal 0 has no effect on the process.
    let gone = unsafe { libc::kill(pid, 0) } == -1
        && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
    !gone
}

/// Elsewhere nothing is swept.
#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    true
}

/// The database at `path` and its `-wal` and `-shm`.
fn remove_database(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut p = path.to_path_buf().into_os_string();
        p.push(suffix);
        let _ = std::fs::remove_file(p);
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        if let Some(path) = self.temp_path.take() {
            remove_database(&path);
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

/// One title's looked-up scores (plans/research/ratings.md): IMDb's average
/// and the Tomatometer, each `None` when the source had none.
#[derive(Debug, Clone, PartialEq)]
pub struct RatingRow {
    pub imdb_id: String,
    /// MDBList's kind: "movie" or "show".
    pub kind: String,
    pub imdb: Option<f64>,
    pub rt: Option<i64>,
    pub fetched_ms: i64,
}

impl Library {
    /// The stored rows of these `(kind, imdb id)` keys; keys without one are
    /// left out.
    pub fn ratings(&self, keys: &[(String, String)]) -> Result<Vec<RatingRow>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT imdb, rt, fetched_ms FROM ratings WHERE imdb_id = ?1 AND kind = ?2",
            )?;
            let mut rows = Vec::new();
            for (kind, imdb_id) in keys {
                let row = stmt
                    .query_row(params![imdb_id, kind], |r| {
                        Ok(RatingRow {
                            imdb_id: imdb_id.clone(),
                            kind: kind.clone(),
                            imdb: r.get(0)?,
                            rt: r.get(1)?,
                            fetched_ms: r.get(2)?,
                        })
                    })
                    .optional()?;
                rows.extend(row);
            }
            Ok(rows)
        })
    }

    /// Stores looked-up rows (replacing the same keys) and drops every row
    /// fetched before `forget_before_ms`. Rows looked up while `removals` was
    /// the count are dropped when a profile removal overtook them, as for
    /// `cache_put`. Returns whether they were stored.
    pub fn put_ratings(
        &self,
        rows: &[RatingRow],
        forget_before_ms: i64,
        removals: u64,
    ) -> Result<bool, LibraryError> {
        self.write(|c| {
            if self.removals() != removals {
                return Ok(false);
            }
            let mut stmt = c.prepare_cached(
                "INSERT OR REPLACE INTO ratings (imdb_id, kind, imdb, rt, fetched_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for row in rows {
                stmt.execute(params![
                    row.imdb_id,
                    row.kind,
                    row.imdb,
                    row.rt,
                    row.fetched_ms
                ])?;
            }
            c.execute(
                "DELETE FROM ratings WHERE fetched_ms < ?1",
                params![forget_before_ms],
            )?;
            Ok(true)
        })
    }
}

impl Library {
    /// Titles the user added (`in_library`), each with its newest progress row.
    pub fn library(&self, profile: &str) -> Result<Vec<LibraryEntry>, LibraryError> {
        self.read(|c| {
            entries(
                c,
                "SELECT item_id, kind, name, poster, poster_shape, added_ms
                 FROM library_items WHERE profile_id = ?1 AND in_library = 1",
                params![profile],
            )
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
    pub fn set_stream_memory<'a>(
        &self,
        profile: &str,
        item_id: &str,
        binge_group: Option<&str>,
        stream_json: &str,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            local(c, coll::STREAM_MEMORY, &[profile, item_id], &stamp, |h| {
                sql::set_stream_memory(c, profile, item_id, binge_group, stream_json, h)
            })
        })
    }

    /// The stream last played for a title, as `set_stream_memory` stored it.
    pub fn stream_memory(
        &self,
        profile: &str,
        item_id: &str,
    ) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT stream_json FROM stream_memory WHERE profile_id = ?1 AND item_id = ?2",
                params![profile, item_id],
                |r| r.get(0),
            )
            .optional()
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
    pub fn set_setting<'a>(
        &self,
        profile: &str,
        key: &str,
        value_json: &str,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            local(c, coll::SETTINGS, &[profile, key], &stamp, |h| {
                sql::set_setting(c, profile, key, value_json, h)
            })
        })
    }

    /// Remembers a search the user acted on; keeps the newest 50, each at
    /// most `SEARCH_MAX_CHARS` characters (a pasted page stays a line).
    pub fn record_search(
        &self,
        profile: &str,
        query: &str,
        used_ms: i64,
    ) -> Result<(), LibraryError> {
        let query: String = query.chars().take(SEARCH_MAX_CHARS).collect();
        let query = query.as_str();
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

/// Writes the addons whose registers an edit changed, from the installed
/// list `before` to `after` (its order is the new one; a link listed twice
/// keeps its first place). An addon added, switched or given another
/// manifest gets a state register, one added or moved an order register,
/// and one left out a removal; nothing else is stamped. A real edit (not the
/// defaults step's zero stamp) also makes the list's untouched defaults its
/// own (`endorse`).
pub(crate) fn write_addons(
    c: &Connection,
    profile: &str,
    before: &[AddonRow],
    after: Vec<AddonRow>,
    stamp: &Stamp<'_>,
) -> rusqlite::Result<()> {
    let mut seen = std::collections::HashSet::new();
    let after: Vec<AddonRow> = after
        .into_iter()
        .filter(|a| seen.insert(a.transport_url.clone()))
        .collect();
    let old = |url: &str| before.iter().find(|b| b.transport_url == url);
    let old_places: Vec<Option<i64>> = after
        .iter()
        .map(|a| old(&a.transport_url).map(|b| b.position))
        .collect();
    let mut changed = false;
    let urls: Vec<&str> = after.iter().map(|a| a.transport_url.as_str()).collect();
    for (a, position) in after.iter().zip(places(&old_places, &urls)) {
        let was = old(&a.transport_url);
        let same_state = was.is_some_and(|b| {
            (&b.manifest_json, b.enabled, b.allow_private)
                == (&a.manifest_json, a.enabled, a.allow_private)
        });
        if !same_state {
            changed |= local_changed(c, coll::ADDON, &[profile, &a.transport_url], stamp, |h| {
                sql::set_addon(c, profile, a, true, h)
            })?;
        }
        if was.map(|b| b.position) != Some(position) {
            changed |= local_changed(
                c,
                coll::ADDON_ORDER,
                &[profile, &a.transport_url],
                stamp,
                |h| sql::set_addon_order(c, profile, &a.transport_url, position, h),
            )?;
        }
    }
    for b in before.iter().filter(|b| !seen.contains(&b.transport_url)) {
        changed |= local_changed(c, coll::ADDON, &[profile, &b.transport_url], stamp, |h| {
            sql::set_addon(c, profile, b, false, h)
        })?;
    }
    if changed && stamp.is_real() {
        endorse(c, profile, stamp)?;
    }
    Ok(())
}

/// Stamps the profile's untouched defaults (registers at wall time 0, from
/// the defaults step) with a real edit's stamp: once a person edits a list,
/// the defaults they kept in it are their list too, and travel as such. An
/// untouched default from another device loses to any real register of the
/// profile (`sql::untouched_rule`), so until then a device's defaults never
/// outrank an account's list (plans/accounts-social-sync.md 13).
fn endorse(c: &Connection, profile: &str, stamp: &Stamp<'_>) -> rusqlite::Result<()> {
    let untouched: Vec<(AddonRow, bool)> = c
        .prepare(
            "SELECT transport_url, manifest_json, enabled, allow_private, installed FROM addons
             WHERE profile_id = ?1 AND substr(hlc, 1, 12) = '000000000000'",
        )?
        .query_map(params![profile], |r| {
            Ok((
                AddonRow {
                    transport_url: r.get(0)?,
                    manifest_json: r.get(1)?,
                    position: 0,
                    enabled: r.get(2)?,
                    allow_private: r.get(3)?,
                },
                r.get(4)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    for (a, installed) in &untouched {
        local(c, coll::ADDON, &[profile, &a.transport_url], stamp, |h| {
            sql::set_addon(c, profile, a, *installed, h)
        })?;
    }
    let places: Vec<(String, i64)> = c
        .prepare(
            "SELECT transport_url, position FROM addon_order
             WHERE profile_id = ?1 AND substr(hlc, 1, 12) = '000000000000'",
        )?
        .query_map(params![profile], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (url, position) in &places {
        local(c, coll::ADDON_ORDER, &[profile, url], stamp, |h| {
            sql::set_addon_order(c, profile, url, *position, h)
        })?;
    }
    Ok(())
}

/// New places for a list whose addons had the places `old` (`None` for a
/// new one), in the list's new order. The longest run of old places already
/// in order keeps them; every other addon gets a place between its
/// neighbours, so a move or an insert changes only the addons that moved or
/// came. When no whole number fits between two neighbours, the list is
/// placed again in steps of `POSITION_STEP`.
pub(crate) fn places(old: &[Option<i64>], urls: &[&str]) -> Vec<i64> {
    // Longest run of old places already in the list's order, (place, link)
    // as the list shows them, so two addons appended at once on two devices
    // (one place, told apart by link) are in order (patience sort).
    let known: Vec<(usize, i64)> = old
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.map(|p| (i, p)))
        .collect();
    let shown = |k: usize| (known[k].1, urls[known[k].0]);
    let mut tails: Vec<usize> = Vec::new(); // index into `known` of each run length's smallest tail
    let mut back: Vec<Option<usize>> = vec![None; known.len()];
    for k in 0..known.len() {
        let at = tails.partition_point(|&t| shown(t) < shown(k));
        back[k] = at.checked_sub(1).map(|a| tails[a]);
        if at == tails.len() {
            tails.push(k);
        } else {
            tails[at] = k;
        }
    }
    let mut kept: Vec<Option<i64>> = vec![None; old.len()];
    let mut k = tails.last().copied();
    while let Some(i) = k {
        kept[known[i].0] = Some(known[i].1);
        k = back[i];
    }
    // Every gap between kept places (or past either end) takes its addons
    // spread evenly; a gap too narrow places the whole list again.
    let mut out = vec![0; old.len()];
    let mut i = 0;
    while i < old.len() {
        if let Some(p) = kept[i] {
            out[i] = p;
            i += 1;
            continue;
        }
        let end = (i..old.len())
            .find(|&j| kept[j].is_some())
            .unwrap_or(old.len());
        let n = (end - i) as i64;
        let low = i.checked_sub(1).map(|j| out[j]);
        let high = kept.get(end).copied().flatten();
        let (low, high) = match (low, high) {
            (Some(l), Some(h)) => (l, h),
            (Some(l), None) => (l, l + (n + 1) * POSITION_STEP),
            (None, Some(h)) => (h - (n + 1) * POSITION_STEP, h),
            (None, None) => (0, (n + 1) * POSITION_STEP),
        };
        if high - low <= n {
            return (1..=old.len() as i64).map(|r| r * POSITION_STEP).collect();
        }
        for (step, slot) in out[i..end].iter_mut().enumerate() {
            *slot = low + (high - low) * (step as i64 + 1) / (n + 1);
        }
        i = end;
    }
    out
}
