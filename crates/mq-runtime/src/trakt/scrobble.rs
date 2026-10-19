//! Scrobbles (plans/m7-trakt.md 4.4 and 4.5): one worker per profile turns
//! the playback views' reports into start, pause and stop. Only the newest
//! state of each session is sent, one request at a time; a view's close
//! sends its stop from the worker, after the view is gone. A session keeps
//! the connection it started under: its calls never go out with another
//! account's tokens.

use super::map::{self, Miss, TraktIds};
use super::{ACCOUNT_SETTING, Conn, TRACKER, lock};
use crate::Core;
use crate::core::WeakCore;
use mq_library::hlc::now_ms;
use mq_library::{IntentOp, NewIntent, TrackerKey};
use mq_tracking::trakt::call;
use mq_tracking::trakt::{Answer, AuthError, Bound};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// Scrobbles wait this long for a record when the profile uses Trakt and
/// the app has not handed one over yet (it does right after `open`).
const RECORD_WAIT: Duration = Duration::from_secs(5);
/// A stop is tried this many times before it counts as unanswered.
const STOP_TRIES: u32 = 4;
/// Trakt counts a stop at or past this percent as a play.
const PLAY_PERCENT: f64 = 80.0;
/// Under this percent a closed session sends nothing.
const MIN_PERCENT: f64 = 1.0;
/// Trakt's `Retry-After` is honored up to this, and a failing stop holds
/// newer sessions no longer than this before it is given up.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(60 * 60);

/// Trakt's side of a playback (`PlaybackSnapshot.trakt`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum TraktPlayback {
    /// Scrobbled.
    On,
    /// Trakt cannot name this title or episode; listed in the `trakt` view.
    Unmapped,
    #[default]
    Off,
    /// The profile uses Trakt; this device is not signed in.
    ThisDeviceOff,
}

pub(crate) type Report = Box<dyn Fn(TraktPlayback) + Send>;

/// What a session plays.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub kind: String,
    pub item_id: String,
    pub video_id: String,
    pub name: String,
}

pub(crate) enum Event {
    Open {
        id: u64,
        target: Target,
        session: String,
        /// The clock when the view opened: an unmark after it is the
        /// user's word.
        since: String,
        /// The connection held when the view opened, if any.
        bound: Option<Bound>,
        owner: Owner,
        held: Arc<AtomicBool>,
        report: Report,
    },
    Progress {
        id: u64,
        percent: f64,
        paused: bool,
    },
    Close {
        id: u64,
    },
    /// Pause what plays; answer once sent.
    Background(oneshot::Sender<()>),
}

/// The account a session is bound to, shared by the view and the worker.
type Owner = Arc<Mutex<Option<String>>>;

/// A playback view's end of its session. Dropping it closes the session:
/// the worker sends the stop.
pub(crate) struct Scrobble {
    id: u64,
    session: String,
    owner: Owner,
    held: Arc<AtomicBool>,
    events: UnboundedSender<Event>,
}

impl Scrobble {
    /// A position report; `paused` when the player is paused.
    pub(crate) fn progress(&self, position_ms: u64, duration_ms: u64, paused: bool) {
        if duration_ms == 0 {
            return;
        }
        let percent = (position_ms as f64 / duration_ms as f64 * 100.0).clamp(0.0, 100.0);
        let _ = self.events.send(Event::Progress {
            id: self.id,
            percent,
            paused,
        });
    }

    /// The playback reached its end: the account its session holds one
    /// write for, when it holds none yet. That is the session's own
    /// account, and only while the library still keeps that account's work:
    /// a session started under another account holds nothing. The caller
    /// writes it with the mark, in one transaction (`set_watched_holding`),
    /// then calls `held`.
    pub(crate) fn to_hold(&self, core: &Core) -> Option<String> {
        if self.held.load(Ordering::SeqCst) {
            return None;
        }
        let owner = lock(&self.owner).clone()?;
        let current = core.library().tracker_account(core.profile(), TRACKER);
        let current = current.ok().flatten().map(|(account, _)| account);
        (current.as_deref() == Some(owner.as_str())).then_some(owner)
    }

    /// The write `to_hold` named, for `set_watched_holding`.
    pub(crate) fn intent<'a>(&'a self, profile: &'a str, account: &'a str) -> NewIntent<'a> {
        NewIntent {
            key: TrackerKey {
                profile,
                tracker: TRACKER,
                account,
            },
            op: IntentOp::Add,
            watched_ms: now_ms() as i64,
            session: Some(&self.session),
        }
    }

    pub(crate) fn held(&self) {
        self.held.store(true, Ordering::SeqCst);
    }
}

impl Drop for Scrobble {
    fn drop(&mut self) {
        let _ = self.events.send(Event::Close { id: self.id });
    }
}

impl Core {
    /// A scrobble session for a playback view, or `None` when this build has
    /// no Trakt app. `report` gets Trakt's side now and as it changes.
    pub(crate) fn trakt_scrobble(&self, target: Target, report: Report) -> Option<Scrobble> {
        let trakt = self.trakt();
        trakt.config()?;
        let conn = self.trakt_conn();
        let bound = futures_util::FutureExt::now_or_never(conn.auth.bound()).flatten();
        report(match bound {
            Some(_) => TraktPlayback::On,
            None if super::setting(self, ACCOUNT_SETTING).is_some() => TraktPlayback::ThisDeviceOff,
            None => TraktPlayback::Off,
        });
        let events = conn
            .events
            .get_or_init(|| {
                let (tx, rx) = unbounded_channel();
                let stop = conn.stop.clone();
                tokio::spawn(run(self.downgrade(), conn.clone(), rx, stop));
                tx
            })
            .clone();
        let id = trakt.sessions.fetch_add(1, Ordering::SeqCst) + 1;
        let session = format!("{}-{id}", trakt.boot);
        let held = Arc::new(AtomicBool::new(false));
        let owner: Owner = Arc::new(Mutex::new(bound.as_ref().map(|b| b.account.clone())));
        let _ = events.send(Event::Open {
            id,
            target,
            session: session.clone(),
            since: self.hlc(),
            bound,
            owner: owner.clone(),
            held: held.clone(),
            report,
        });
        Some(Scrobble {
            id,
            session,
            owner,
            held,
            events,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    Nothing,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sent {
    Nothing,
    Playing,
    Paused,
    Stopped,
}

struct Sess {
    target: Target,
    report: Report,
    session: String,
    held: Arc<AtomicBool>,
    since: String,
    /// The connection its calls use: the one at open, else the first one
    /// seen. Never another.
    bound: Option<Bound>,
    /// `bound`'s account, for the view's held write.
    owner: Owner,
    percent: f64,
    want: Want,
    sent: Sent,
    closed: bool,
    /// Kept once known for good; a miss that may pass is looked up again.
    ids: Option<Result<TraktIds, String>>,
    tries: u32,
    /// After a failed send, nothing goes before this (backoff, or Trakt's
    /// `Retry-After`).
    not_before: Option<Instant>,
    /// When its stop first failed.
    stop_failing_since: Option<Instant>,
}

/// The profile's worker: folds every queued event, then sends what the
/// sessions want now. It holds the core weakly and ends with it.
async fn run(
    core: WeakCore,
    conn: Arc<Conn>,
    mut events: UnboundedReceiver<Event>,
    stop: CancellationToken,
) {
    let mut sessions: BTreeMap<u64, Sess> = BTreeMap::new();
    let mut retry: Option<Instant> = None;
    loop {
        let sleep = async {
            match retry {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        let first = tokio::select! {
            () = stop.cancelled() => return,
            event = events.recv() => match event {
                Some(event) => Some(event),
                None => return,
            },
            () = sleep => None,
        };
        let Some(core) = core.upgrade() else { return };
        let mut waiting = Vec::new();
        let mut batch: Vec<Event> = first.into_iter().collect();
        while let Ok(event) = events.try_recv() {
            batch.push(event);
        }
        for event in batch {
            fold(&mut sessions, &mut waiting, event);
        }
        retry = act(&core, &conn, &mut sessions).await;
        for done in waiting {
            let _ = done.send(());
        }
    }
}

fn fold(sessions: &mut BTreeMap<u64, Sess>, waiting: &mut Vec<oneshot::Sender<()>>, event: Event) {
    match event {
        Event::Open {
            id,
            target,
            session,
            since,
            bound,
            owner,
            held,
            report,
        } => {
            sessions.insert(
                id,
                Sess {
                    target,
                    report,
                    session,
                    held,
                    since,
                    bound,
                    owner,
                    percent: 0.0,
                    want: Want::Nothing,
                    sent: Sent::Nothing,
                    closed: false,
                    ids: None,
                    tries: 0,
                    not_before: None,
                    stop_failing_since: None,
                },
            );
        }
        Event::Progress {
            id,
            percent,
            paused,
        } => {
            if let Some(s) = sessions.get_mut(&id) {
                s.percent = percent;
                s.want = if paused { Want::Paused } else { Want::Playing };
            }
        }
        Event::Close { id } => {
            if let Some(s) = sessions.get_mut(&id) {
                s.closed = true;
            }
        }
        Event::Background(done) => {
            // Playback may go on in picture in picture: a pause, never a stop.
            for s in sessions.values_mut() {
                if s.want == Want::Playing {
                    s.want = Want::Paused;
                }
            }
            waiting.push(done);
        }
    }
}

/// One answer, read for scrobbles.
enum Reply {
    Ok(serde_json::Value),
    /// 409: Trakt has this play already.
    Duplicate,
    /// 404 or 422: Trakt does not know the item.
    Unmapped(String),
    /// 420, 423 or another refusal: retrying does not help.
    Refused(String),
    /// No connection, or not this session's any more.
    Off,
    /// Try again, not before Trakt's `Retry-After` when it sent one.
    Transient(String, Option<Duration>),
}

fn read(answer: Result<Answer, AuthError>) -> Reply {
    match answer {
        Ok(Answer::Ok { body, .. }) => Reply::Ok(body),
        Ok(Answer::Refused { status: 409, .. }) => Reply::Duplicate,
        Ok(Answer::Refused {
            status: status @ (404 | 422),
            error,
        }) => Reply::Unmapped(match error {
            Some(error) => format!("Trakt does not know it ({status}, {error})"),
            None => format!("Trakt does not know it ({status})"),
        }),
        Ok(Answer::Refused { status, error }) => Reply::Refused(match (status, error) {
            (420, _) => "status 420 (account limit)".into(),
            (423, _) => "status 423 (account locked)".into(),
            (status, Some(error)) => format!("status {status} ({error})"),
            (status, None) => format!("status {status}"),
        }),
        Ok(Answer::Transient {
            reason,
            retry_after,
        }) => Reply::Transient(reason, retry_after.map(|after| after.min(RETRY_AFTER_CAP))),
        Err(AuthError::Transient(reason)) => Reply::Transient(reason, None),
        Err(_) => Reply::Off,
    }
}

/// Sends what the sessions want, oldest session first, so an episode's
/// stop goes before the next one's start: while an older stop waits for a
/// retry, newer sessions send nothing.
async fn act(core: &Core, conn: &Arc<Conn>, sessions: &mut BTreeMap<u64, Sess>) -> Option<Instant> {
    let trakt = core.trakt();
    if super::gone(core) {
        // Deleted by sync while it played: nothing more goes out for it.
        trakt.forget(core.profile());
        return None;
    }
    wait_for_record(core, conn).await;
    let current = conn.auth.bound().await;
    let (api, _) = trakt.api()?;
    let _lane = conn.lane.lock().await;
    let newest_playing = sessions
        .iter()
        .filter(|(_, s)| !s.closed && s.want == Want::Playing)
        .map(|(id, _)| *id)
        .max();
    let mut retry: Option<Instant> = None;
    let mut blocked = false;
    let mut changed = false;
    let ids: Vec<u64> = sessions.keys().copied().collect();
    let mut stopped_any = false;
    for id in ids {
        let s = sessions.get_mut(&id).expect("listed");
        if s.bound.is_none() && current.is_some() {
            s.bound = current.clone();
            *lock(&s.owner) = s.bound.as_ref().map(|b| b.account.clone());
        }
        let Some(bound) = s.bound.clone() else {
            // Never connected: nothing to send; a close ends it.
            if s.closed {
                let s = sessions.remove(&id).expect("listed");
                end(core, &s, false);
            }
            continue;
        };
        if blocked {
            continue;
        }
        if let Some(at) = s.not_before.filter(|at| *at > Instant::now()) {
            retry = Some(retry.map_or(at, |r| r.min(at)));
            if s.closed || matches!(s.sent, Sent::Playing | Sent::Paused) {
                // An older session still owes its stop or state: newer
                // sessions wait for it.
                blocked = true;
            }
            continue;
        }
        let key = conn.key(&bound.account);
        if s.ids.is_none() {
            match resolve(core, key, &s.target).await {
                Ok(ids) => {
                    (s.report)(TraktPlayback::On);
                    s.ids = Some(Ok(ids));
                }
                Err(Miss::Permanent(reason)) => {
                    (s.report)(TraktPlayback::Unmapped);
                    s.ids = Some(Err(reason));
                    changed = true;
                }
                Err(Miss::Transient(reason)) => {
                    // Looked up again after a while; never remembered.
                    conn.failed(&reason);
                    s.tries += 1;
                    s.not_before = Some(Instant::now() + backoff(s.tries));
                    retry = earliest(retry, backoff(s.tries));
                    changed = true;
                    if s.closed {
                        // A close with no ids: the stop cannot go; its held
                        // write is released for the outbox to try.
                        let s = sessions.remove(&id).expect("listed");
                        end(core, &s, false);
                    }
                    continue;
                }
            }
        }
        let Some(Ok(ids)) = s.ids.clone() else {
            if s.closed {
                let s = sessions.remove(&id).expect("listed");
                end(core, &s, false);
            }
            continue;
        };
        let gate = trakt.gate(&bound.account);
        let superseded = newest_playing.is_some_and(|n| n > id);
        if s.closed || (superseded && matches!(s.sent, Sent::Playing | Sent::Paused)) {
            let sends = s.sent != Sent::Stopped && s.percent >= MIN_PERCENT;
            let done = if sends {
                let body = ids.scrobble(s.percent);
                let answer = call::post(&api, &gate, &conn.auth, &bound, "/scrobble/stop", &body);
                changed = true;
                stopped(core, conn, key, s, &ids, read(answer.await))
            } else {
                true
            };
            if !done {
                retry = earliest(
                    retry,
                    s.not_before
                        .map_or(Duration::ZERO, |at| at - Instant::now()),
                );
                blocked = true;
                continue;
            }
            stopped_any |= sends;
            s.sent = Sent::Stopped;
            if s.closed {
                let s = sessions.remove(&id).expect("listed");
                if !sends {
                    end(core, &s, false);
                }
            }
            continue;
        }
        let (action, next) = match (s.want, s.sent) {
            (Want::Playing, Sent::Nothing | Sent::Paused) => ("start", Sent::Playing),
            (Want::Paused, Sent::Playing) => ("pause", Sent::Paused),
            _ => continue,
        };
        let body = ids.scrobble(s.percent);
        let path = format!("/scrobble/{action}");
        changed = true;
        match read(call::post(&api, &gate, &conn.auth, &bound, &path, &body).await) {
            Reply::Ok(answer) => {
                conn.ok();
                s.tries = 0;
                s.not_before = None;
                s.sent = next;
                if action == "pause" {
                    remember(core, key, &s.target.video_id, s.percent);
                }
                if !ids.resolved(&answer) {
                    map::wrong_show(core, key, &s.target.item_id, &s.target.name);
                    (s.report)(TraktPlayback::Unmapped);
                    s.ids = Some(Err("Trakt matched it to another title".into()));
                }
            }
            Reply::Duplicate => s.sent = next,
            Reply::Unmapped(reason) => {
                map::record(core, key, &s.target.item_id, &s.target.name, &reason);
                (s.report)(TraktPlayback::Unmapped);
                s.ids = Some(Err(reason));
            }
            // Trying again does not help until something changes.
            Reply::Refused(reason) => {
                conn.failed(&reason);
                s.sent = next;
            }
            Reply::Off => {}
            Reply::Transient(reason, after) => {
                conn.failed(&reason);
                s.tries += 1;
                let wait = backoff(s.tries).max(after.unwrap_or_default());
                s.not_before = Some(Instant::now() + wait);
                retry = earliest(retry, wait);
            }
        }
    }
    if stopped_any {
        // A finished playback is a good moment for a round.
        core.trakt_schedule(conn);
    }
    if changed {
        core.trakt_refresh(conn);
    }
    retry
}

/// Reads a stop's answer. Returns whether the stop is over (`false`: try
/// again at `not_before`).
fn stopped(
    core: &Core,
    conn: &Conn,
    key: TrackerKey<'_>,
    s: &mut Sess,
    ids: &TraktIds,
    answer: Reply,
) -> bool {
    let confirm = |s: &Sess| {
        let t = &s.target;
        if let Err(e) = core.library().confirm_watched(
            key.profile,
            &t.item_id,
            &t.video_id,
            &s.since,
            core.clock(),
        ) {
            tracing::warn!("trakt: confirmed mark not written: {e}");
        }
    };
    match answer {
        Reply::Ok(body) => {
            conn.ok();
            remember(core, key, &s.target.video_id, s.percent);
            if !ids.resolved(&body) {
                // A play went to another title: none follows it there.
                map::wrong_show(core, key, &s.target.item_id, &s.target.name);
                (s.report)(TraktPlayback::Unmapped);
                end(core, s, true);
            } else if body["action"].as_str() == Some("scrobble") {
                confirm(s);
                end(core, s, true);
            } else {
                end(core, s, false);
            }
            true
        }
        Reply::Duplicate => {
            confirm(s);
            end(core, s, true);
            true
        }
        Reply::Unmapped(reason) => {
            map::record(core, key, &s.target.item_id, &s.target.name, &reason);
            (s.report)(TraktPlayback::Unmapped);
            end(core, s, false);
            true
        }
        Reply::Refused(reason) => {
            conn.failed(&reason);
            end(core, s, false);
            true
        }
        Reply::Off => {
            end(core, s, false);
            true
        }
        Reply::Transient(reason, after) => {
            conn.failed(&reason);
            s.tries += 1;
            let failing_since = *s.stop_failing_since.get_or_insert_with(Instant::now);
            if let Some(wait) = stop_wait(s.tries, after, failing_since.elapsed()) {
                s.not_before = Some(Instant::now() + wait);
                return false;
            }
            // Unanswered: Trakt may have the play or not. The held write
            // goes out, after a look at the minutes around the stop.
            if s.percent >= PLAY_PERCENT && s.held.load(Ordering::SeqCst) {
                let at = now_ms().to_string();
                let name = format!("check:{}", s.target.video_id);
                let _ = core.library().set_tracker_state(key, &name, &at);
            }
            end(core, s, false);
            true
        }
    }
}

/// When a failed stop is tried again (`None`: given up as unanswered):
/// after its backoff or Trakt's `Retry-After`, and never later than
/// `RETRY_AFTER_CAP` after its first failure, since newer sessions wait
/// for it.
fn stop_wait(tries: u32, after: Option<Duration>, failing_for: Duration) -> Option<Duration> {
    let left = RETRY_AFTER_CAP
        .checked_sub(failing_for)
        .filter(|left| !left.is_zero())?;
    (tries < STOP_TRIES).then(|| backoff(tries).max(after.unwrap_or_default()).min(left))
}

/// The session is over: its held write goes (`delivered`: Trakt has the
/// play) or is released to the outbox.
fn end(core: &Core, s: &Sess, delivered: bool) {
    if let Err(e) = core.library().end_tracker_session(&s.session, delivered) {
        tracing::warn!("trakt: session end not written: {e}");
    }
}

/// This device's own pause or stop, so a pull does not take it back as
/// another device's.
fn remember(core: &Core, key: TrackerKey<'_>, video_id: &str, percent: f64) {
    let value = json!({"percent": percent, "at": now_ms()}).to_string();
    let _ = core
        .library()
        .set_tracker_state(key, &format!("sent:{video_id}"), &value);
}

/// The session's ids; a permanent miss is listed as unmapped.
async fn resolve(core: &Core, key: TrackerKey<'_>, target: &Target) -> Result<TraktIds, Miss> {
    let resolved = map::ids(core, key, &target.kind, &target.item_id, &target.video_id).await;
    if let Err(Miss::Permanent(reason)) = &resolved {
        map::record(core, key, &target.item_id, &target.name, reason);
    }
    resolved
}

/// The profile uses Trakt but no record arrived yet: wait for one, at
/// most 5 s after the connection was first used.
async fn wait_for_record(core: &Core, conn: &Conn) {
    // Listening before looking, so a record that comes in between wakes it.
    let arrived = conn.record_in.notified();
    tokio::pin!(arrived);
    arrived.as_mut().enable();
    if conn.auth.account().await.is_some() || super::setting(core, ACCOUNT_SETTING).is_none() {
        return;
    }
    tokio::select! {
        () = arrived => {}
        () = tokio::time::sleep_until(conn.created + RECORD_WAIT) => {}
    }
}

/// The wait before trying a failed send again: 1 s, doubling, about a
/// minute at most.
fn backoff(tries: u32) -> Duration {
    Duration::from_millis(500u64 << tries.clamp(1, 7))
}

fn earliest(current: Option<Instant>, wait: Duration) -> Option<Instant> {
    let at = Instant::now() + wait;
    Some(current.map_or(at, |c| c.min(at)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_is_capped_and_a_stuck_stop_gives_up() {
        let hours = |h: u64| Duration::from_secs(h * 60 * 60);
        let answer = Ok(Answer::Transient {
            reason: "busy".into(),
            retry_after: Some(hours(6)),
        });
        assert!(
            matches!(read(answer), Reply::Transient(_, Some(after)) if after == RETRY_AFTER_CAP),
            "Retry-After is honored up to the cap"
        );
        // A stop failing for 59 minutes waits at most the minute left.
        let minute = Duration::from_secs(60);
        assert_eq!(
            stop_wait(1, Some(hours(1)), hours(1) - minute),
            Some(minute)
        );
        // At the cap it is given up, whatever Trakt asks for.
        assert_eq!(stop_wait(1, Some(minute), RETRY_AFTER_CAP), None);
        // Before it: the backoff, and four tries at most.
        assert_eq!(stop_wait(1, None, Duration::ZERO), Some(backoff(1)));
        assert_eq!(stop_wait(STOP_TRIES, None, Duration::ZERO), None);
    }
}
