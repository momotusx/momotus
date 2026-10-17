//! Watch-tracker state (plans/m7-trakt.md sections 4.5, 4.6 and 5): writes
//! waiting to be sent, cursors, and imports of a tracker's plays into the
//! library. A tracker is named by a short id ("trakt"); everything is keyed by
//! (profile, tracker, account), so another account never sees this one's work.

use crate::hlc::Hlc;
use crate::store::{Library, LibraryError, LibraryItemRow, ProgressRow};
use crate::sync::{coll, local, sql};
use rusqlite::{Connection, OptionalExtension, params};

/// Rows dated further ahead than this are deferred, not imported.
const FUTURE_SLACK_MS: i64 = 10 * 60 * 1000;

/// Which tracker and account a write belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackerKey<'a> {
    pub profile: &'a str,
    pub tracker: &'a str,
    pub account: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentOp {
    Add,
    Remove,
}

impl IntentOp {
    fn as_str(self) -> &'static str {
        match self {
            IntentOp::Add => "add",
            IntentOp::Remove => "remove",
        }
    }
}

/// A write waiting to be sent: add or remove one play of `video_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub video_id: String,
    pub item_id: String,
    pub op: IntentOp,
    pub watched_ms: i64,
    /// Acknowledgements name it, so an older answer never deletes a newer
    /// intent for the same video.
    pub rev: i64,
}

/// A new intent: `session` holds it until that playback session ends.
#[derive(Debug, Clone, Copy)]
pub struct NewIntent<'a> {
    pub key: TrackerKey<'a>,
    pub op: IntentOp,
    pub watched_ms: i64,
    pub session: Option<&'a str>,
}

/// One play as a tracker reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPlay {
    pub item_id: String,
    pub video_id: String,
    /// "movie" or "series".
    pub kind: String,
    pub name: String,
    /// When it was watched; 1 for an unknown date, so any real edit wins.
    pub watched_ms: i64,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ImportReport {
    /// Marks that changed local state.
    pub applied: usize,
    /// Rows dated in the future, to be read again later.
    pub deferred: Vec<String>,
}

impl Library {
    /// The account connected for this profile and tracker, with its generation.
    pub fn tracker_account(
        &self,
        profile: &str,
        tracker: &str,
    ) -> Result<Option<(String, i64)>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT account, generation FROM tracker_account WHERE profile_id = ?1 AND tracker = ?2",
                params![profile, tracker],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })
    }

    /// Connects `account` (or disconnects with `None`). Another account, or
    /// a disconnect, drops the previous account's pending writes and cursors.
    pub fn set_tracker_account(
        &self,
        profile: &str,
        tracker: &str,
        account: Option<&str>,
        generation: i64,
        now_ms: i64,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let old: Option<String> = c
                .query_row(
                    "SELECT account FROM tracker_account WHERE profile_id = ?1 AND tracker = ?2",
                    params![profile, tracker],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = old.filter(|old| Some(old.as_str()) != account) {
                for table in ["tracker_outbox", "tracker_state"] {
                    c.execute(
                        &format!("DELETE FROM {table} WHERE profile_id = ?1 AND tracker = ?2 AND account = ?3"),
                        params![profile, tracker, old],
                    )?;
                }
            }
            match account {
                Some(account) => c.execute(
                    "INSERT INTO tracker_account (profile_id, tracker, account, generation, connected_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(profile_id, tracker) DO UPDATE SET
                       account = excluded.account, generation = excluded.generation,
                       connected_ms = excluded.connected_ms",
                    params![profile, tracker, account, generation, now_ms],
                )?,
                None => c.execute(
                    "DELETE FROM tracker_account WHERE profile_id = ?1 AND tracker = ?2",
                    params![profile, tracker],
                )?,
            };
            Ok(())
        })
    }

    /// Marks or unmarks by hand. The register, its sync entry and, when the
    /// value flips and a tracker is connected, a write for the tracker are
    /// one transaction. Returns whether the value flipped.
    pub fn set_watched_by_hand(
        &self,
        profile: &str,
        item_id: &str,
        video_id: &str,
        watched: bool,
        hlc: &str,
        tracker: Option<(TrackerKey<'_>, i64)>,
    ) -> Result<bool, LibraryError> {
        self.write(|c| {
            let before = watched_now(c, profile, video_id)?;
            local(c, coll::WATCHED, &[profile, video_id], hlc, |h| {
                sql::set_watched(c, profile, item_id, video_id, watched, h)
            })?;
            let flipped = before != watched && watched_now(c, profile, video_id)? == watched;
            if let (true, Some((key, now_ms))) = (flipped, tracker) {
                let op = if watched {
                    IntentOp::Add
                } else {
                    IntentOp::Remove
                };
                put_intent(
                    c,
                    item_id,
                    video_id,
                    NewIntent {
                        key,
                        op,
                        watched_ms: now_ms,
                        session: None,
                    },
                )?;
            }
            Ok(flipped)
        })
    }

    /// Queues a tracker write; a pending write for the same video is
    /// replaced and the revision goes up. Returns the new revision.
    pub fn put_tracker_intent(
        &self,
        item_id: &str,
        video_id: &str,
        intent: NewIntent<'_>,
    ) -> Result<i64, LibraryError> {
        self.write(|c| put_intent(c, item_id, video_id, intent))
    }

    /// Writes that may be sent now (not held by a live session, not waiting
    /// for a retry), oldest first. One sender drains them at a time (the
    /// tracker's round), so rows are not claimed.
    pub fn ready_tracker_intents(
        &self,
        key: TrackerKey<'_>,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<Intent>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT video_id, item_id, op, watched_ms, rev FROM tracker_outbox
                 WHERE profile_id = ?1 AND tracker = ?2 AND account = ?3 AND held = 0 AND next_ms <= ?5
                 ORDER BY watched_ms, video_id LIMIT ?4",
            )?;
            stmt.query_map(
                params![key.profile, key.tracker, key.account, limit as i64, now_ms],
                |r| {
                    Ok(Intent {
                        video_id: r.get(0)?,
                        item_id: r.get(1)?,
                        op: if r.get::<_, String>(2)? == "add" {
                            IntentOp::Add
                        } else {
                            IntentOp::Remove
                        },
                        watched_ms: r.get(3)?,
                        rev: r.get(4)?,
                    })
                },
            )?
            .collect()
        })
    }

    /// Sending the write with this revision failed: try again at `next_ms`.
    /// Returns the number of tries so far.
    pub fn retry_tracker_intent(
        &self,
        key: TrackerKey<'_>,
        video_id: &str,
        rev: i64,
        next_ms: i64,
    ) -> Result<i64, LibraryError> {
        self.write(|c| {
            c.execute(
                "UPDATE tracker_outbox SET tries = tries + 1, next_ms = ?6
                 WHERE profile_id = ?1 AND tracker = ?2 AND account = ?3 AND video_id = ?4 AND rev = ?5",
                params![key.profile, key.tracker, key.account, video_id, rev, next_ms],
            )?;
            Ok(c.query_row(
                "SELECT tries FROM tracker_outbox
                 WHERE profile_id = ?1 AND tracker = ?2 AND account = ?3 AND video_id = ?4 AND rev = ?5",
                params![key.profile, key.tracker, key.account, video_id, rev],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
        })
    }

    /// The tracker took the write with this revision; a newer one stays.
    pub fn ack_tracker_intent(
        &self,
        key: TrackerKey<'_>,
        video_id: &str,
        rev: i64,
    ) -> Result<bool, LibraryError> {
        self.write(|c| {
            Ok(c.execute(
                "DELETE FROM tracker_outbox
                 WHERE profile_id = ?1 AND tracker = ?2 AND account = ?3 AND video_id = ?4 AND rev = ?5",
                params![key.profile, key.tracker, key.account, video_id, rev],
            )? > 0)
        })
    }

    /// A playback session ended. `delivered`: its stop reached the tracker
    /// as a play, so the writes it held are dropped; otherwise they are
    /// released to be sent.
    pub fn end_tracker_session(&self, session: &str, delivered: bool) -> Result<(), LibraryError> {
        self.write(|c| {
            if delivered {
                c.execute("DELETE FROM tracker_outbox WHERE session = ?1 AND held = 1", params![session])?;
            } else {
                c.execute(
                    "UPDATE tracker_outbox SET held = 0, session = NULL WHERE session = ?1 AND held = 1",
                    params![session],
                )?;
            }
            Ok(())
        })
    }

    /// Releases writes held by sessions that are not alive (the process
    /// was killed while they played). Run at start with no live sessions.
    pub fn release_orphan_tracker_intents(&self, live: &[&str]) -> Result<usize, LibraryError> {
        self.write(|c| {
            let mut stmt =
                c.prepare("SELECT DISTINCT session FROM tracker_outbox WHERE held = 1")?;
            let held: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            let mut released = 0;
            for session in held.iter().filter(|s| !live.contains(&s.as_str())) {
                released += c.execute(
                    "UPDATE tracker_outbox SET held = 0, session = NULL WHERE session = ?1",
                    params![session],
                )?;
            }
            Ok(released)
        })
    }

    pub fn tracker_state(
        &self,
        key: TrackerKey<'_>,
        name: &str,
    ) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT value FROM tracker_state WHERE profile_id = ?1 AND tracker = ?2 AND account = ?3 AND key = ?4",
                params![key.profile, key.tracker, key.account, name],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn set_tracker_state(
        &self,
        key: TrackerKey<'_>,
        name: &str,
        value: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO tracker_state (profile_id, tracker, account, key, value) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(profile_id, tracker, account, key) DO UPDATE SET value = excluded.value",
                params![key.profile, key.tracker, key.account, name, value],
            )?;
            Ok(())
        })
    }

    /// Imports a tracker's plays. Marks are stamped from the play's own date
    /// on device 0, so they never beat a newer local edit, are never
    /// re-stamped, and two devices importing the same plays write the same
    /// registers. Titles get a cache row, and each series gets one progress
    /// anchor at its latest watched episode so Next Up sees it, unless the
    /// title already has later progress. `state` (cursor writes) commits in
    /// the same transaction.
    pub fn import_plays(
        &self,
        profile: &str,
        plays: &[ImportedPlay],
        now_ms: i64,
        state: &[(TrackerKey<'_>, &str, &str)],
    ) -> Result<ImportReport, LibraryError> {
        self.write(|c| {
            let mut report = ImportReport::default();
            let zero = Hlc::zero(0).encode();
            let mut latest: Vec<&ImportedPlay> = Vec::new();
            for play in plays {
                if play.watched_ms > now_ms + FUTURE_SLACK_MS {
                    report.deferred.push(play.video_id.clone());
                    continue;
                }
                let item = LibraryItemRow {
                    item_id: play.item_id.clone(),
                    kind: play.kind.clone(),
                    name: play.name.clone(),
                    poster: None,
                    poster_shape: None,
                    added_ms: play.watched_ms,
                };
                // Only fills a gap (the zero stamp never replaces a row), and
                // syncs, so other devices know the title's kind and name.
                local(c, coll::ITEMS, &[profile, &play.item_id], &zero, |h| {
                    sql::upsert_item(c, profile, &item, false, h)
                })?;
                let hlc = stamp(play.watched_ms);
                let before = watched_now(c, profile, &play.video_id)?;
                local(c, coll::WATCHED, &[profile, &play.video_id], &hlc, |h| {
                    sql::set_watched(c, profile, &play.item_id, &play.video_id, true, h)
                })?;
                if !before && watched_now(c, profile, &play.video_id)? {
                    report.applied += 1;
                }
                if play.kind == "series" {
                    latest.push(play);
                }
            }
            // Per series, the latest play whose mark ended up set: a play a
            // newer local unmark beat must not take the anchor from the rest.
            let mut kept = Vec::with_capacity(latest.len());
            for play in latest {
                if watched_now(c, profile, &play.video_id)? {
                    kept.push(play);
                }
            }
            let mut latest = kept;
            // Equal dates (a season marked at once, unknown dates) break by
            // episode number, not by id text, where 1:9 sorts after 1:12.
            latest.sort_by(|a, b| {
                (&a.item_id, std::cmp::Reverse(order(a)))
                    .cmp(&(&b.item_id, std::cmp::Reverse(order(b))))
            });
            latest.dedup_by(|a, b| a.item_id == b.item_id);
            for play in latest {
                anchor(c, profile, play)?;
            }
            for (key, name, value) in state {
                c.execute(
                    "INSERT INTO tracker_state (profile_id, tracker, account, key, value) VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(profile_id, tracker, account, key) DO UPDATE SET value = excluded.value",
                    params![key.profile, key.tracker, key.account, name, value],
                )?;
            }
            Ok(report)
        })
    }
}

/// A play's place in time: its date, then its season and episode.
fn order(play: &ImportedPlay) -> (i64, u32, u32, &str) {
    let mut parts = play.video_id.rsplit(':');
    let episode = parts.next().and_then(|e| e.parse().ok()).unwrap_or(0);
    let season = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (play.watched_ms, season, episode, &play.video_id)
}

/// The device-0 stamp of an imported play.
fn stamp(watched_ms: i64) -> String {
    Hlc {
        wall_ms: watched_ms.max(1) as u64,
        counter: 0,
        device: 0,
    }
    .encode()
}

fn watched_now(c: &Connection, profile: &str, video_id: &str) -> rusqlite::Result<bool> {
    Ok(c.query_row(
        "SELECT watched FROM watched WHERE profile_id = ?1 AND video_id = ?2",
        params![profile, video_id],
        |r| r.get::<_, bool>(0),
    )
    .optional()?
    .unwrap_or(false))
}

/// The progress anchor of a series' latest imported episode: written only
/// when that episode's mark ended up set and the title has no later progress.
fn anchor(c: &Connection, profile: &str, play: &ImportedPlay) -> rusqlite::Result<()> {
    if !watched_now(c, profile, &play.video_id)? {
        return Ok(());
    }
    let later: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM progress WHERE profile_id = ?1 AND item_id = ?2 AND updated_ms > ?3)",
        params![profile, play.item_id, play.watched_ms],
        |r| r.get(0),
    )?;
    if later {
        return Ok(());
    }
    let row = ProgressRow {
        video_id: &play.video_id,
        item_id: &play.item_id,
        position_ms: 0,
        duration_ms: 0,
        updated_ms: play.watched_ms,
    };
    local(
        c,
        coll::PROGRESS,
        &[profile, &play.video_id],
        &stamp(play.watched_ms),
        |h| sql::set_progress(c, profile, &row, h),
    )
}

fn put_intent(
    c: &Connection,
    item_id: &str,
    video_id: &str,
    intent: NewIntent<'_>,
) -> rusqlite::Result<i64> {
    let k = intent.key;
    // One counter per account that never goes down: a deleted row's
    // revision is never handed out again, so a late acknowledgement of it
    // cannot delete a newer write.
    let rev: i64 = c
        .query_row(
            "SELECT value FROM tracker_state WHERE profile_id = ?1 AND tracker = ?2 AND account = ?3 AND key = 'rev'",
            params![k.profile, k.tracker, k.account],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
        + 1;
    c.execute(
        "INSERT INTO tracker_state (profile_id, tracker, account, key, value) VALUES (?1, ?2, ?3, 'rev', ?4)
         ON CONFLICT(profile_id, tracker, account, key) DO UPDATE SET value = excluded.value",
        params![k.profile, k.tracker, k.account, rev.to_string()],
    )?;
    c.execute(
        "INSERT INTO tracker_outbox (profile_id, tracker, account, video_id, item_id, rev, op, watched_ms, session, held)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(profile_id, tracker, account, video_id) DO UPDATE SET
           item_id = excluded.item_id, rev = excluded.rev, op = excluded.op, watched_ms = excluded.watched_ms,
           session = excluded.session, held = excluded.held, tries = 0, next_ms = 0",
        params![
            k.profile,
            k.tracker,
            k.account,
            video_id,
            item_id,
            rev,
            intent.op.as_str(),
            intent.watched_ms,
            intent.session,
            intent.session.is_some()
        ],
    )?;
    Ok(rev)
}
