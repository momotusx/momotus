//! Offline downloads (plans/offline-downloads.md 4.1): one row per video
//! kept on this device. Device-local: no sync collection, no outbox, and no
//! change notification (the runtime's downloads manager publishes its own
//! state; a checkpoint write every 8 MiB would wake every view).

use crate::store::{Library, LibraryError};
use rusqlite::{OptionalExtension, Row, params};

/// One download as stored. The JSON columns (`facts`, `request`, `version`,
/// `subtitles`) belong to the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRecord {
    pub id: String,
    pub added_by: String,
    pub kind: String,
    pub item_id: String,
    pub video_id: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub facts: String,
    pub request: String,
    pub version: Option<String>,
    pub state: String,
    pub reason: Option<String>,
    pub mobile_data: bool,
    pub hand_picked: bool,
    pub bytes_done: i64,
    pub bytes_total: Option<i64>,
    pub moves: i64,
    pub subtitles: String,
    pub subtitles_state: String,
    pub position: i64,
    pub created_ms: i64,
    pub done_ms: Option<i64>,
}

const COLUMNS: &str = "id, added_by, kind, item_id, video_id, season, episode, facts, request, \
     version, state, reason, mobile_data, hand_picked, bytes_done, bytes_total, moves, \
     subtitles, subtitles_state, position, created_ms, done_ms";

fn record(r: &Row<'_>) -> rusqlite::Result<DownloadRecord> {
    Ok(DownloadRecord {
        id: r.get(0)?,
        added_by: r.get(1)?,
        kind: r.get(2)?,
        item_id: r.get(3)?,
        video_id: r.get(4)?,
        season: r.get(5)?,
        episode: r.get(6)?,
        facts: r.get(7)?,
        request: r.get(8)?,
        version: r.get(9)?,
        state: r.get(10)?,
        reason: r.get(11)?,
        mobile_data: r.get(12)?,
        hand_picked: r.get(13)?,
        bytes_done: r.get(14)?,
        bytes_total: r.get(15)?,
        moves: r.get(16)?,
        subtitles: r.get(17)?,
        subtitles_state: r.get(18)?,
        position: r.get(19)?,
        created_ms: r.get(20)?,
        done_ms: r.get(21)?,
    })
}

impl Library {
    /// Every download, in queue order.
    pub fn downloads(&self) -> Result<Vec<DownloadRecord>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(&format!(
                "SELECT {COLUMNS} FROM downloads ORDER BY position"
            ))?;
            stmt.query_map([], record)?.collect()
        })
    }

    /// The download of `video_id` of `item_id`, if any.
    pub fn download_of(
        &self,
        item_id: &str,
        video_id: &str,
    ) -> Result<Option<DownloadRecord>, LibraryError> {
        self.read(|c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM downloads WHERE item_id = ?1 AND video_id = ?2"),
                params![item_id, video_id],
                record,
            )
            .optional()
        })
    }

    /// Adds `row` at the end of the queue (its `position` is set here), or,
    /// when its video already has a download, changes nothing and gives
    /// that one: one row per video.
    pub fn add_download(&self, row: &DownloadRecord) -> Result<DownloadRecord, LibraryError> {
        self.write(|c| {
            let existing = c
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM downloads WHERE item_id = ?1 AND video_id = ?2"
                    ),
                    params![row.item_id, row.video_id],
                    record,
                )
                .optional()?;
            if let Some(existing) = existing {
                return Ok(existing);
            }
            let position: i64 = c.query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM downloads",
                [],
                |r| r.get(0),
            )?;
            let row = DownloadRecord {
                position,
                ..row.clone()
            };
            put(c, &row)?;
            Ok(row)
        })
    }

    /// Writes `row` over the stored one with its id (the manager is the
    /// one writer of these rows). False when it was deleted meanwhile: a
    /// cancel is never undone by a late checkpoint.
    pub fn save_download(&self, row: &DownloadRecord) -> Result<bool, LibraryError> {
        self.write(|c| {
            let there: Option<i64> = c
                .query_row(
                    "SELECT 1 FROM downloads WHERE id = ?1",
                    params![row.id],
                    |r| r.get(0),
                )
                .optional()?;
            if there.is_none() {
                return Ok(false);
            }
            put(c, row)?;
            Ok(true)
        })
    }

    /// Removes the download `id`; true when there was one.
    pub fn delete_download(&self, id: &str) -> Result<bool, LibraryError> {
        self.write(|c| Ok(c.execute("DELETE FROM downloads WHERE id = ?1", params![id])? > 0))
    }
}

fn put(c: &rusqlite::Connection, r: &DownloadRecord) -> rusqlite::Result<()> {
    c.execute(
        &format!(
            "INSERT OR REPLACE INTO downloads ({COLUMNS}) VALUES \
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)"
        ),
        params![
            r.id,
            r.added_by,
            r.kind,
            r.item_id,
            r.video_id,
            r.season,
            r.episode,
            r.facts,
            r.request,
            r.version,
            r.state,
            r.reason,
            r.mobile_data,
            r.hand_picked,
            r.bytes_done,
            r.bytes_total,
            r.moves,
            r.subtitles,
            r.subtitles_state,
            r.position,
            r.created_ms,
            r.done_ms,
        ],
    )?;
    Ok(())
}
