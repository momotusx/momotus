//! One round (plans/m7-trakt.md 4.7): what moved on Trakt comes into the
//! library through `import_plays` and `import_resume_points`, then the
//! outbox goes out. Cursors move in the transaction that applies their rows,
//! and only once every fetch they depend on worked. Every call of a round
//! is bound to the connection the round started under.

use super::map::{self, Look, Mapper, Miss, PlaceMiss, TraktIds};
use super::{Bind, Conn, Trakt};
use crate::Core;
use crate::core::MetaMiss;
use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use mq_library::hlc::now_ms;
use mq_library::{ImportedPlay, ImportedResume, Intent, IntentOp, TrackerKey};
use mq_tracking::trakt::call::{self, CallError};
use mq_tracking::trakt::{Api, AuthError, Bound, Gate};
use serde::Serialize;
use serde_json::{Value, json};

/// Trakt's largest page.
const PAGE: u32 = 250;
/// History writes per request.
const BATCH: usize = 100;
/// The reconcile runs at least this often.
const RECONCILE_EVERY_MS: i64 = 24 * 60 * 60 * 1000;
/// An ambiguous stop is a play already on Trakt when one lies this close.
const CHECK_WINDOW_MS: i64 = 3 * 60 * 1000;
/// Own sends this close to a pulled resume point make it an echo.
const ECHO_MS: i64 = 2 * 60 * 1000;
const ECHO_POINTS: f64 = 1.0;
/// Own sends are kept this long.
const SENT_KEPT_MS: i64 = 24 * 60 * 60 * 1000;
/// A pulled resume point under this percent is skipped.
const MIN_PERCENT: f64 = 2.0;
/// Rows dated further ahead are left for a later round.
const FUTURE_SLACK_MS: i64 = 10 * 60 * 1000;

/// What one round did (`traktSync`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
pub struct TraktReport {
    /// Rows read from Trakt.
    pub pulled: u32,
    /// Marks and resume points that changed the library.
    pub applied: u32,
    /// Writes Trakt took.
    pub pushed: u32,
    /// Writes still waiting.
    pub pending: u32,
    /// Items listed as unmapped in this round.
    pub unmapped: u32,
}

/// Runs one round for the profile. One at a time; a sign-out or a new
/// sign-in meanwhile drops what it would write, and none of its calls goes
/// out with the new connection's tokens.
pub(super) async fn round(
    core: &Core,
    trakt: &Trakt,
    conn: &Conn,
) -> Result<TraktReport, CallError> {
    let _turn = conn.round.lock().await;
    let Some((api, _)) = trakt.api() else {
        return Err(CallError::Transient(
            "Trakt is not set up in this build".into(),
        ));
    };
    let bound = conn
        .auth
        .bound()
        .await
        .ok_or(CallError::Auth(AuthError::Off))?;
    let gate = trakt.gate(&bound.account);
    let key = conn.key(&bound.account);
    let mut round = Round {
        core,
        conn,
        api: &api,
        gate: &gate,
        key,
        bound: &bound,
        mapper: Mapper::new(
            core,
            Bind {
                conn,
                bound: &bound,
            },
        ),
        report: TraktReport::default(),
        now: now_ms() as i64,
        left: None,
        reached: false,
    };
    let result = round.run().await;
    match &result {
        Ok(()) | Err(CallError::Auth(AuthError::Off)) => {}
        Err(e) => conn.failed(&super::reason(e)),
    }
    conn.set_importing(None);
    // The app is in use and online: failed looks not seen again over two
    // days of such rounds start over (`map::failed`). A round Trakt never
    // answered (offline) is not time passing for them.
    if round.reached {
        round
            .at()
            .set_state(core, map::LAST_ROUND, &now_ms().to_string());
    }
    let pending = core.library().tracker_pending(round.key).map_or(0, |p| p.0);
    round.report.pending = pending as u32;
    result.map(|()| round.report)
}

struct Round<'a> {
    core: &'a Core,
    conn: &'a Conn,
    api: &'a Api,
    gate: &'a Gate,
    key: TrackerKey<'a>,
    bound: &'a Bound,
    mapper: Mapper<'a>,
    report: TraktReport,
    now: i64,
    /// Why a row was left for a later round, if one was: the activities
    /// stay as they were, so the next round reads it again.
    left: Option<String>,
    /// Trakt answered one of its calls: the device is online.
    reached: bool,
}

/// Rows placed, and the earliest date of a row left for later.
struct Plays {
    taken: Vec<ImportedPlay>,
    left_at: Option<i64>,
}

impl<'a> Round<'a> {
    async fn run(&mut self) -> Result<(), CallError> {
        self.prune_sent();
        let pulled = self.pull().await;
        // Writes go out whatever the reads did: a show that fails, or a
        // list Trakt refuses, never holds them back.
        self.drain().await;
        pulled
    }

    async fn pull(&mut self) -> Result<(), CallError> {
        let acts = self.get("/sync/last_activities").await?;
        self.reached = true;
        let stored = self.state_json("activities");
        let first = stored.is_null();
        let moved = |kind: &str, field: &str| acts[kind][field] != stored[kind][field];
        let reconciled = self
            .state("reconciled_ms")
            .and_then(|v| v.parse::<i64>().ok());
        let mut reconcile = reconciled.is_none_or(|at| self.now - at > RECONCILE_EVERY_MS);
        if !first && (moved("movies", "watched_at") || moved("episodes", "watched_at")) {
            let previous = ["movies", "episodes"]
                .iter()
                .filter_map(|k| ms(&stored[k]["watched_at"]))
                .max();
            if !self.history(previous).await? {
                // Moved with no row after the previous mark: a play dated in
                // the past, or a removal.
                reconcile = true;
            }
        }
        if reconcile {
            self.reconcile().await?;
        }
        if moved("movies", "paused_at") || moved("episodes", "paused_at") {
            self.playback().await?;
        }
        let _fence = self.fence()?;
        if let Some(reason) = self.left.take() {
            // Everything else is in; what waits is read again next round.
            self.conn.failed(&reason);
            return Ok(());
        }
        self.core
            .library()
            .set_tracker_state(self.key, "activities", &acts.to_string())
            .map_err(store)?;
        // The reads worked; a write that fails next says so itself.
        self.conn.ok();
        Ok(())
    }

    /// History from the cursor's day, every page. Returns whether a row lies
    /// after `previous` (the day is read again, so the row that set it does
    /// not count).
    async fn history(&mut self, previous: Option<i64>) -> Result<bool, CallError> {
        let cursor = self.state("history_day").unwrap_or_else(|| day(self.now));
        let path = format!("/sync/history?start_at={cursor}T00:00:00.000Z");
        let rows = self.get_all(&path).await?;
        let reached = rows
            .iter()
            .any(|r| ms(&r["watched_at"]).is_some_and(|at| Some(at) > previous));
        let plays = self.plays(&rows, |r| ms(&r["watched_at"])).await;
        // The cursor moves to the newest day taken, never back, and never
        // past a row left for later (dated ahead, or its show's details
        // failing).
        let taken = plays
            .taken
            .iter()
            .map(|p| p.watched_ms)
            .filter(|&at| at <= self.now + FUTURE_SLACK_MS);
        let mut new_day = taken.max().map(day).unwrap_or_else(|| cursor.clone());
        if let Some(at) = plays.left_at {
            new_day = new_day.min(day(at));
        }
        let new_day = new_day.max(cursor);
        self.import(&plays.taken, &[("history_day", new_day.as_str())])?;
        Ok(reached)
    }

    /// Everything Trakt has watched, adds only: movies from the watched
    /// list, shows from their own history when their `last_updated_at`
    /// moved. A show's checkpoint commits with its plays, so an
    /// interrupted first import goes on where it stopped.
    async fn reconcile(&mut self) -> Result<(), CallError> {
        let movies = self.get_all("/sync/watched/movies").await?;
        let plays = self.plays(&movies, |r| ms(&r["last_watched_at"])).await;
        self.import(&plays.taken, &[])?;
        let mut left = false;
        let shows = self
            .get_all("/sync/watched/shows?extended=noseasons")
            .await?;
        let todo: Vec<(u64, String)> = shows
            .iter()
            .filter_map(|s| {
                let id = s["show"]["ids"]["trakt"].as_u64()?;
                let updated = s["last_updated_at"].as_str()?.to_owned();
                let seen = self.state(&format!("show:{id}"));
                (seen.as_deref() != Some(updated.as_str())).then_some((id, updated))
            })
            .collect();
        let total = todo.len() as u32;
        for (done, (id, updated)) in todo.into_iter().enumerate() {
            self.importing(done as u32, total);
            let rows = self.get_all(&format!("/sync/history/shows/{id}")).await?;
            let plays = self.plays(&rows, |r| ms(&r["watched_at"])).await;
            let later = plays
                .taken
                .iter()
                .any(|p| p.watched_ms > self.now + FUTURE_SLACK_MS);
            left |= plays.left_at.is_some();
            let checkpoint = format!("show:{id}");
            // A play dated ahead, or a row left for later, keeps the show
            // open, to be read again; the shows after it go on.
            let state: &[(&str, &str)] = if later || plays.left_at.is_some() {
                &[]
            } else {
                &[(checkpoint.as_str(), updated.as_str())]
            };
            self.import(&plays.taken, state)?;
        }
        let now = self.now.to_string();
        let today = day(self.now);
        // With a show left open, the reconcile runs again next round (only
        // the shows without their checkpoint are read again).
        let mut state = Vec::new();
        if !left {
            state.push(("reconciled_ms", now.as_str()));
        }
        if self.state("history_day").is_none() {
            state.push(("history_day", today.as_str()));
        }
        self.import(&[], &state)
    }

    /// Resume points: an item newer than the local progress becomes one,
    /// unless it is this device's own pause or stop coming back.
    async fn playback(&mut self) -> Result<(), CallError> {
        let items = self.get_all("/sync/playback").await?;
        let mut points = Vec::new();
        for item in &items {
            let percent = item["progress"].as_f64().unwrap_or(0.0);
            let Some(paused_ms) = ms(&item["paused_at"]) else {
                continue;
            };
            if percent < MIN_PERCENT {
                continue;
            }
            let placed = match self.mapper.place(item).await {
                Ok(placed) => placed,
                Err(PlaceMiss::Permanent(u)) => {
                    self.unplaced(&u);
                    continue;
                }
                Err(PlaceMiss::Transient(reason)) => {
                    self.left = Some(reason);
                    continue;
                }
            };
            if self.echo(&placed.video_id, percent, paused_ms) {
                continue;
            }
            let (position_ms, duration_ms) = match self.position(&placed, percent).await {
                Ok(Some(position)) => position,
                // No runtime anywhere: nothing to place it on.
                Ok(None) => continue,
                // No addon answered at all (offline): read again, not a look.
                Err(miss) if miss.unreached => {
                    self.left = Some(miss.reason);
                    continue;
                }
                Err(miss) => {
                    let (at, item) = (self.at(), &placed.item_id);
                    match map::failed(self.core, at, Look::Runtime, item, miss.reason) {
                        Miss::Transient(reason) => self.left = Some(reason),
                        Miss::Permanent(_) => {}
                    }
                    continue;
                }
            };
            points.push(ImportedResume {
                item_id: placed.item_id,
                video_id: placed.video_id,
                kind: placed.kind,
                name: placed.name,
                position_ms,
                duration_ms,
                paused_ms,
            });
        }
        let _fence = self.fence()?;
        let taken = self
            .core
            .library()
            .import_resume_points(self.key.profile, &points, self.core.clock().device())
            .map_err(store)?;
        self.report.applied += taken as u32;
        Ok(())
    }

    /// One of this device's own sends: within a point and two minutes.
    fn echo(&self, video_id: &str, percent: f64, at: i64) -> bool {
        let Some(sent) = self.state(&format!("sent:{video_id}")) else {
            return false;
        };
        let sent: Value = serde_json::from_str(&sent).unwrap_or_default();
        let (Some(p), Some(t)) = (sent["percent"].as_f64(), sent["at"].as_i64()) else {
            return false;
        };
        (p - percent).abs() <= ECHO_POINTS && (t - at).abs() <= ECHO_MS
    }

    /// Own sends older than a day are forgotten.
    fn prune_sent(&self) {
        let library = self.core.library();
        for (name, value) in library
            .tracker_states(self.key, "sent:")
            .unwrap_or_default()
        {
            let at = serde_json::from_str::<Value>(&value)
                .ok()
                .and_then(|v| v["at"].as_i64())
                .unwrap_or(0);
            if self.now - at > SENT_KEPT_MS {
                let _ = library.delete_tracker_state(self.key, &name);
            }
        }
    }

    /// Where a percent lands: on the local duration, else 97 percent of the
    /// meta's runtime (players count credits differently). `Err` when the
    /// meta could not be read now.
    async fn position(
        &self,
        placed: &map::Mapped,
        percent: f64,
    ) -> Result<Option<(i64, i64)>, MetaMiss> {
        let local = self
            .core
            .library()
            .progress(self.key.profile, &placed.video_id)
            .ok()
            .flatten()
            .filter(|&(_, duration)| duration > 0);
        if let Some((_, duration)) = local {
            return Ok(Some(((percent / 100.0 * duration as f64) as i64, duration)));
        }
        let meta = self
            .core
            .fetch_meta_or_miss(&placed.kind, &placed.item_id)
            .await?;
        map::succeeded(self.core, self.at(), Look::Runtime, &placed.item_id);
        let runtime = meta.preview.runtime.as_deref().and_then(runtime_ms);
        Ok(runtime.map(|runtime| {
            (
                (percent / 100.0 * runtime as f64 * 0.97).floor() as i64,
                runtime,
            )
        }))
    }

    /// Places rows and turns them into plays; rows Trakt cannot name are
    /// listed as unmapped. A row whose show's details cannot be read now is
    /// left for later (its date bounds what may move past it); the others
    /// go on.
    async fn plays(&mut self, rows: &[Value], at: impl Fn(&Value) -> Option<i64>) -> Plays {
        self.report.pulled += rows.len() as u32;
        let mut plays = Plays {
            taken: Vec::new(),
            left_at: None,
        };
        for row in rows {
            // Signed out meanwhile: nothing more is looked up.
            if self.fence().is_err() {
                break;
            }
            // Unknown dates sort before any real edit.
            let when = at(row).unwrap_or(1);
            match self.mapper.place(row).await {
                Ok(placed) => plays.taken.push(ImportedPlay {
                    item_id: placed.item_id,
                    video_id: placed.video_id,
                    kind: placed.kind,
                    name: placed.name,
                    watched_ms: when,
                }),
                Err(PlaceMiss::Permanent(u)) => self.unplaced(&u),
                Err(PlaceMiss::Transient(reason)) => {
                    self.left = Some(reason);
                    plays.left_at = Some(plays.left_at.map_or(when, |l| l.min(when)));
                }
            }
        }
        plays
    }

    fn import(&mut self, plays: &[ImportedPlay], state: &[(&str, &str)]) -> Result<(), CallError> {
        let _fence = self.fence()?;
        let state: Vec<_> = state.iter().map(|(k, v)| (self.key, *k, *v)).collect();
        let report = self
            .core
            .library()
            .import_plays(self.key.profile, plays, self.now, &state)
            .map_err(store)?;
        self.report.applied += report.applied as u32;
        Ok(())
    }

    /// Sends the writes that may go now, in batches. Writes a show's
    /// details cannot place now wait for a later try; writes Trakt cannot
    /// name are dropped.
    async fn drain(&mut self) {
        'batches: for _ in 0..50 {
            let intents = self
                .core
                .library()
                .ready_tracker_intents(self.key, now_ms() as i64, BATCH)
                .unwrap_or_default();
            // With no usable sign-in, the writes wait for one without
            // spending their backoff.
            let signed_in = futures_util::FutureExt::now_or_never(self.conn.auth.status())
                .is_some_and(|status| status.is_ok());
            if intents.is_empty() || !signed_in || self.fence().is_err() {
                break;
            }
            let mut adds = Vec::new();
            let mut removes = Vec::new();
            for intent in intents {
                if self.fence().is_err() {
                    break 'batches;
                }
                let kind = if intent.video_id == intent.item_id {
                    "movie"
                } else {
                    "series"
                };
                // From the round's memory after a show's first look.
                let ids = self
                    .mapper
                    .ids(kind, &intent.item_id, &intent.video_id)
                    .await;
                match ids {
                    Ok(ids) if intent.op == IntentOp::Add => {
                        if self.already_there(&intent, &ids).await {
                            self.ack(&intent);
                        } else {
                            adds.push((intent, ids));
                        }
                    }
                    Ok(ids) => removes.push((intent, ids)),
                    Err(Miss::Permanent(reason)) => {
                        self.unplaced(&map::Unplaced {
                            id: intent.item_id.clone(),
                            name: intent.item_id.clone(),
                            reason,
                        });
                        self.ack(&intent);
                    }
                    Err(Miss::Transient(reason)) => {
                        self.conn.failed(&reason);
                        self.retry(&intent);
                    }
                }
            }
            for (path, batch) in [("/sync/history", adds), ("/sync/history/remove", removes)] {
                if !batch.is_empty() && !self.send(path, batch).await {
                    break 'batches;
                }
            }
        }
        self.plan_retry();
    }

    /// An add released by a stop that got no answer: Trakt may have taken
    /// the play already. One look at the minutes around the stop.
    async fn already_there(&mut self, intent: &Intent, ids: &TraktIds) -> bool {
        let name = format!("check:{}", intent.video_id);
        let Some(stop) = self.state(&name).and_then(|v| v.parse::<i64>().ok()) else {
            return false;
        };
        let path = format!(
            "/sync/history?start_at={}&end_at={}",
            iso(stop - CHECK_WINDOW_MS),
            iso(stop + CHECK_WINDOW_MS)
        );
        let Ok(rows) = self.get_all(&path).await else {
            // Not known: sent, as a missed play is worse than a second one.
            return false;
        };
        let _ = self.core.library().delete_tracker_state(self.key, &name);
        rows.iter().any(|row| ids.resolved(row))
    }

    /// Sends one batch. `false` when the sign-in failed: nothing more goes.
    async fn send(&mut self, path: &str, batch: Vec<(Intent, TraktIds)>) -> bool {
        let mut body = json!({"movies": [], "episodes": []});
        for (intent, ids) in &batch {
            let at = (intent.op == IntentOp::Add).then(|| iso(intent.watched_ms));
            let (list, entry) = ids.history(at);
            body[list].as_array_mut().expect("a list").push(entry);
        }
        // Scrobbles first: a running pause or stop finishes before this
        // write, and the worker never waits for the outbox.
        drop(self.conn.lane.lock().await);
        if self.live().is_err() {
            return false;
        }
        let answer = call::post(
            self.api,
            self.gate,
            &self.conn.auth,
            self.bound,
            path,
            &body,
            None,
        );
        let answer = call::body(answer.await).and_then(|answer| {
            let removes = path.ends_with("/remove");
            acknowledged(&answer, &body, removes)
                .map(|()| answer)
                .map_err(CallError::Transient)
        });
        match answer {
            Ok(answer) => {
                self.conn.ok();
                self.reached = true;
                let missing = &answer["not_found"];
                for (intent, ids) in &batch {
                    let (list, entry) = ids.history(None);
                    let lost = missing[list]
                        .as_array()
                        .is_some_and(|l| l.iter().any(|m| m["ids"] == entry["ids"]));
                    if lost {
                        self.unplaced(&map::Unplaced {
                            id: intent.item_id.clone(),
                            name: intent.item_id.clone(),
                            reason: "Trakt does not know it".into(),
                        });
                    } else {
                        self.report.pushed += 1;
                    }
                    self.ack(intent);
                }
                true
            }
            // The sign-in failed, not the writes: they wait as they are,
            // without spending their backoff.
            Err(CallError::Auth(e)) => {
                if e != AuthError::Off {
                    self.conn.failed(&super::reason(&CallError::Auth(e)));
                }
                false
            }
            Err(e) => {
                self.conn.failed(&super::reason(&e));
                for (intent, _) in &batch {
                    self.retry(intent);
                }
                true
            }
        }
    }

    fn ack(&mut self, intent: &Intent) {
        let _ = self
            .core
            .library()
            .ack_tracker_intent(self.key, &intent.video_id, intent.rev);
    }

    /// Not sent now: tried again after its backoff.
    fn retry(&self, intent: &Intent) {
        let _ = self.core.library().fail_tracker_intent(
            self.key,
            &intent.video_id,
            intent.rev,
            now_ms() as i64,
        );
    }

    /// When the scheduler should come back for writes waiting on a retry.
    fn plan_retry(&self) {
        let next = self
            .core
            .library()
            .tracker_next_retry(self.key)
            .ok()
            .flatten();
        *super::lock(&self.conn.retry_at) = next.map(|n| n as u64);
    }

    fn unplaced(&mut self, u: &map::Unplaced) {
        self.report.unmapped += 1;
        map::record(self.core, self.at(), &u.id, &u.name, &u.reason);
    }

    fn importing(&self, done: u32, total: u32) {
        self.conn
            .set_importing(Some(super::TraktImporting { done, total }));
        self.core.trakt_refresh(self.conn);
    }

    /// The connection this round started under, for its account's writes.
    fn at(&self) -> Bind<'a> {
        Bind {
            conn: self.conn,
            bound: self.bound,
        }
    }

    /// The connection this round started under is still the one in use,
    /// and stays so while the guard is held (a sign-out waits for it).
    fn fence(&self) -> Result<std::sync::MutexGuard<'a, ()>, CallError> {
        self.at()
            .fence(self.core)
            .ok_or(CallError::Auth(AuthError::Off))
    }

    /// No call goes for a profile that turned kids while the round ran.
    fn live(&self) -> Result<(), CallError> {
        if super::kids(self.core) {
            return Err(CallError::Auth(AuthError::Off));
        }
        Ok(())
    }

    async fn get(&self, path: &str) -> Result<Value, CallError> {
        self.live()?;
        call::body(call::get(self.api, self.gate, &self.conn.auth, self.bound, path).await)
    }

    async fn get_all(&self, path: &str) -> Result<Vec<Value>, CallError> {
        self.live()?;
        call::get_all(self.api, self.gate, &self.conn.auth, self.bound, path, PAGE).await
    }

    fn state(&self, name: &str) -> Option<String> {
        self.core
            .library()
            .tracker_state(self.key, name)
            .ok()
            .flatten()
    }

    fn state_json(&self, name: &str) -> Value {
        self.state(name)
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or(Value::Null)
    }
}

/// Whether a history answer acknowledges `sent`: its counts and its
/// `not_found` lists are there, every item it lists as not found is one
/// that was sent, and for adds every item sent is either counted as added
/// or updated (a play Trakt had already, sent again) or listed as not
/// found. Anything else keeps the batch.
fn acknowledged(answer: &Value, sent: &Value, removes: bool) -> Result<(), String> {
    let counts = &answer[if removes { "deleted" } else { "added" }];
    let missing = &answer["not_found"];
    for list in ["movies", "episodes"] {
        let (Some(counted), Some(lost)) = (counts[list].as_u64(), missing[list].as_array()) else {
            return Err("Trakt's answer did not acknowledge the writes".into());
        };
        let sent = sent[list].as_array().map(Vec::as_slice).unwrap_or_default();
        let named = |m: &Value| m["ids"].is_object() && sent.iter().any(|s| s["ids"] == m["ids"]);
        if !lost.iter().all(named) {
            return Err("Trakt's answer named an item that was not sent".into());
        }
        // Each item once: a repeated entry would stand for one never named.
        let distinct: std::collections::HashSet<String> =
            lost.iter().map(|m| m["ids"].to_string()).collect();
        if distinct.len() != lost.len() {
            return Err("Trakt's answer named an item twice".into());
        }
        // Absent in older answers: none updated.
        let updated = answer["updated"][list].as_u64().unwrap_or(0);
        if !removes && counted + updated + lost.len() as u64 != sent.len() as u64 {
            return Err("Trakt's answer did not count the writes".into());
        }
    }
    Ok(())
}

fn store(e: mq_library::LibraryError) -> CallError {
    CallError::Transient(format!("library: {e}"))
}

pub(super) fn ms(value: &Value) -> Option<i64> {
    DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|d| d.timestamp_millis())
}

pub(super) fn iso(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|d| d.to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn day(ms: i64) -> String {
    iso(ms).get(..10).unwrap_or("1970-01-01").to_owned()
}

/// A meta's runtime ("47 min", "1h 32min", "2 h") in ms.
fn runtime_ms(text: &str) -> Option<i64> {
    let mut minutes = 0i64;
    let mut number = String::new();
    let mut seen = false;
    for c in text.chars().chain([' ']) {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        if !number.is_empty() {
            let n: i64 = number.parse().ok()?;
            number.clear();
            seen = true;
            minutes += if c == 'h' { n * 60 } else { n };
        }
    }
    (seen && minutes > 0).then_some(minutes * 60_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtimes_and_dates_read() {
        assert_eq!(runtime_ms("47 min"), Some(47 * 60_000));
        assert_eq!(runtime_ms("1h 32min"), Some(92 * 60_000));
        assert_eq!(runtime_ms("2h"), Some(120 * 60_000));
        assert_eq!(runtime_ms("N/A"), None);
        let at = ms(&json!("2026-10-19T10:20:30.000Z")).unwrap();
        assert_eq!(iso(at), "2026-10-19T10:20:30.000Z");
        assert_eq!(day(at), "2026-10-19");
    }

    #[test]
    fn history_answers_are_acknowledgements_only_when_they_count() {
        let sent = json!({"movies": [], "episodes": [{"ids": {"tvdb": 1}}, {"ids": {"tvdb": 2}}]});
        let full = json!({"added": {"movies": 0, "episodes": 1},
                          "not_found": {"movies": [], "episodes": [{"ids": {"tvdb": 2}}]}});
        assert_eq!(acknowledged(&full, &sent, false), Ok(()));
        assert!(acknowledged(&json!({}), &sent, false).is_err());
        assert!(acknowledged(&json!(null), &sent, false).is_err());
        let short = json!({"added": {"movies": 0, "episodes": 0},
                           "not_found": {"movies": [], "episodes": []}});
        assert!(acknowledged(&short, &sent, false).is_err());
        // A removal deletes however many plays there were.
        let removed = json!({"deleted": {"movies": 0, "episodes": 5},
                             "not_found": {"movies": [], "episodes": []}});
        assert_eq!(acknowledged(&removed, &sent, true), Ok(()));
        assert!(
            acknowledged(&full, &sent, true).is_err(),
            "no deleted counts"
        );
    }

    #[test]
    fn a_not_found_entry_must_name_an_item_that_was_sent() {
        let sent = json!({"movies": [], "episodes": [{"ids": {"tvdb": 1}}]});
        for stranger in [json!({}), json!({"ids": {"tvdb": 9}}), json!("x")] {
            let answer = json!({"added": {"movies": 0, "episodes": 0},
                                "not_found": {"movies": [], "episodes": [stranger]}});
            assert!(acknowledged(&answer, &sent, false).is_err(), "{answer}");
        }
    }

    #[test]
    fn a_not_found_entry_named_twice_acknowledges_nothing() {
        let sent = json!({"movies": [], "episodes": [{"ids": {"tvdb": 1}}, {"ids": {"tvdb": 2}}]});
        // Two entries for A: the count adds up, but B was never named.
        let twice = json!({"added": {"movies": 0, "episodes": 0},
                           "not_found": {"movies": [],
                                         "episodes": [{"ids": {"tvdb": 1}}, {"ids": {"tvdb": 1}}]}});
        assert!(acknowledged(&twice, &sent, false).is_err());
    }

    /// A play Trakt had already, sent again, is counted as updated (the
    /// live answer's shape, 2026-10-19): the batch is acknowledged.
    #[test]
    fn a_play_trakt_counts_as_updated_acknowledges_its_write() {
        let sent = json!({"movies": [{"ids": {"imdb": "tt1"}, "watched_at": "2026-10-19T00:00:00.000Z"}],
                          "episodes": [{"ids": {"tvdb": 1}}, {"ids": {"tvdb": 2}}]});
        let answer = json!({"added": {"movies": 0, "episodes": 1},
                            "updated": {"movies": 1, "episodes": 0},
                            "not_found": {"movies": [], "shows": [], "seasons": [],
                                          "episodes": [{"ids": {"tvdb": 2}}],
                                          "people": [], "users": []}});
        assert_eq!(acknowledged(&answer, &sent, false), Ok(()));
        // Still every item: an updated count too high counts one never sent.
        let over = json!({"added": {"movies": 0, "episodes": 1},
                          "updated": {"movies": 2, "episodes": 0},
                          "not_found": {"movies": [], "episodes": [{"ids": {"tvdb": 2}}]}});
        assert!(acknowledged(&over, &sent, false).is_err());
    }
}
