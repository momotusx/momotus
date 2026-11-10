//! Scrobbles (plans/m7-trakt.md 4.4 and 4.5): one worker per profile turns
//! the playback views' reports into start, pause and stop. Only the newest
//! state of each session is sent, one request at a time; a view's close
//! sends its stop from the worker, after the view is gone. A session keeps
//! the connection it started under: its calls never go out with another
//! account's tokens.

use super::map::{self, Miss, TraktIds};
use super::{ACCOUNT_SETTING, Bind, Conn, TRACKER, lock};
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
/// After a profile check that failed, the next look.
const READ_AGAIN: Duration = Duration::from_secs(1);

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
    /// This device signed in, but Trakt ended its sign-in (a refresh
    /// refused): sign in again. Never shown as "not signed in".
    Reconnect,
    /// In a Momotus account: the account's Trakt sign-in has not reached
    /// this device yet.
    Linking,
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
        // Incognito never reaches Trakt, not even its service.
        if self.incognito() {
            return None;
        }
        let trakt = self.trakt();
        trakt.config()?;
        if super::kids(self) {
            return None;
        }
        let conn = self.trakt_conn();
        let bound = futures_util::FutureExt::now_or_never(conn.auth.bound()).flatten();
        report(match bound {
            Some(_) => shown(&conn),
            None if super::link::in_account(self) => match super::account_state(self) {
                super::TraktState::Linking => TraktPlayback::Linking,
                _ => TraktPlayback::Off,
            },
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
    /// Failed lookups of its ids, apart from its sends.
    misses: u32,
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
                    misses: 0,
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

/// Trakt's refusals in words the Trakt page shows: never a status or
/// Trakt's own `error` code (the owner's rule), which go to the log.
fn read(answer: Result<Answer, AuthError>) -> Reply {
    if let Ok(Answer::Refused { status, error }) = &answer {
        tracing::debug!("trakt scrobble refused: status {status} {error:?}");
    }
    match answer {
        Ok(Answer::Ok { body, .. }) => Reply::Ok(body),
        Ok(Answer::Refused { status: 409, .. }) => Reply::Duplicate,
        Ok(Answer::Refused {
            status: 404 | 422, ..
        }) => Reply::Unmapped("Trakt does not know it".into()),
        Ok(Answer::Refused { status, .. }) => Reply::Refused(match status {
            420 => "The Trakt account reached its limit".into(),
            423 => "The Trakt account is locked".into(),
            _ => "Trakt refused the change".into(),
        }),
        Ok(Answer::Transient {
            reason,
            retry_after,
        }) => {
            tracing::debug!("trakt scrobble: {reason}");
            let after = retry_after.map(|after| after.min(RETRY_AFTER_CAP));
            Reply::Transient(super::TRAKT_SILENT.into(), after)
        }
        Err(AuthError::Transient(reason)) => {
            tracing::debug!("trakt scrobble: {reason}");
            Reply::Transient(super::TRAKT_SILENT.into(), None)
        }
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
        match super::kids_now(core) {
            Some(false) => {}
            // Turned kids while it played: nothing more goes out, its work
            // stops and its tokens go.
            Some(true) => {
                trakt.forget(core.profile());
                return None;
            }
            // Not read: nothing goes now, and it looks again shortly.
            None => return Some(Instant::now() + READ_AGAIN),
        }
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
        let at = Bind {
            conn,
            bound: &bound,
        };
        if s.ids.is_none() {
            match resolve(core, at, &s.target).await {
                Ok(ids) => {
                    (s.report)(shown(conn));
                    s.ids = Some(Ok(ids));
                }
                Err(Miss::Permanent(reason)) => {
                    (s.report)(TraktPlayback::Unmapped);
                    s.ids = Some(Err(reason));
                    changed = true;
                }
                Err(Miss::Transient(reason)) => {
                    // Looked up again after a while; never remembered. Its
                    // own count: a stop keeps every one of its tries.
                    conn.failed(&reason);
                    s.misses += 1;
                    s.not_before = Some(Instant::now() + backoff(s.misses));
                    retry = earliest(retry, backoff(s.misses));
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
            let done = if !sends {
                true
            } else if stop_expired(s.stop_failing_since, Instant::now()) {
                // Its hour is up: given up before another request goes.
                changed = true;
                unanswered(core, at, s);
                true
            } else {
                let body = ids.scrobble(s.percent);
                // Checked again as it leaves: a refresh or its turn may
                // take it past its hour.
                let deadline = s.stop_failing_since.map(|since| since + RETRY_AFTER_CAP);
                let path = "/scrobble/stop";
                let answer = call::post(&api, &gate, &conn.auth, &bound, path, &body, deadline);
                changed = true;
                stopped(core, conn, at, s, &ids, read(answer.await))
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
        match read(call::post(&api, &gate, &conn.auth, &bound, &path, &body, None).await) {
            Reply::Ok(answer) => {
                conn.ok();
                s.tries = 0;
                s.not_before = None;
                s.sent = next;
                if action == "pause" {
                    remember(core, at, &s.target.video_id, s.percent);
                }
                if !ids.resolved(&answer) {
                    map::wrong_show(core, at, &s.target.item_id, &s.target.name);
                    (s.report)(TraktPlayback::Unmapped);
                    s.ids = Some(Err("Trakt matched it to another title".into()));
                }
            }
            Reply::Duplicate => s.sent = next,
            Reply::Unmapped(reason) => {
                map::record(core, at, &s.target.item_id, &s.target.name, &reason);
                (s.report)(TraktPlayback::Unmapped);
                s.ids = Some(Err(reason));
            }
            // Trying again does not help until something changes.
            Reply::Refused(reason) => {
                conn.failed(&reason);
                s.sent = next;
            }
            // The sign-in may have died on this call.
            Reply::Off => (s.report)(shown(conn)),
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
    at: Bind<'_>,
    s: &mut Sess,
    ids: &TraktIds,
    answer: Reply,
) -> bool {
    let confirm = |s: &Sess| {
        let t = &s.target;
        if let Err(e) = core.library().confirm_watched(
            at.key().profile,
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
            remember(core, at, &s.target.video_id, s.percent);
            if !ids.resolved(&body) {
                // A play went to another title: none follows it there.
                map::wrong_show(core, at, &s.target.item_id, &s.target.name);
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
            map::record(core, at, &s.target.item_id, &s.target.name, &reason);
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
            unanswered(core, at, s);
            true
        }
    }
}

/// A stop given up: Trakt may have the play or not. The held write goes
/// out, after a look at the minutes around the stop.
fn unanswered(core: &Core, at: Bind<'_>, s: &Sess) {
    if s.percent >= PLAY_PERCENT && s.held.load(Ordering::SeqCst) {
        let name = format!("check:{}", s.target.video_id);
        at.set_state(core, &name, &now_ms().to_string());
    }
    end(core, s, false);
}

/// Whether a stop failing since `since` is past its hour at `now`.
fn stop_expired(since: Option<Instant>, now: Instant) -> bool {
    since.is_some_and(|since| now.saturating_duration_since(since) >= RETRY_AFTER_CAP)
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
fn remember(core: &Core, at: Bind<'_>, video_id: &str, percent: f64) {
    let value = json!({"percent": percent, "at": now_ms()}).to_string();
    at.set_state(core, &format!("sent:{video_id}"), &value);
}

/// What a playback with a connection shows: `On` while its sign-in works.
/// A dead one records nothing and says to sign in again: this device did
/// sign in, so it never reads as `ThisDeviceOff`.
fn shown(conn: &Conn) -> TraktPlayback {
    match futures_util::FutureExt::now_or_never(conn.auth.status()) {
        Some(Err(AuthError::Reconnect(_))) => TraktPlayback::Reconnect,
        _ => TraktPlayback::On,
    }
}

/// The session's ids; a permanent miss is listed as unmapped.
async fn resolve(core: &Core, at: Bind<'_>, target: &Target) -> Result<TraktIds, Miss> {
    let resolved = map::ids(core, at, &target.kind, &target.item_id, &target.video_id).await;
    if let Err(Miss::Permanent(reason)) = &resolved {
        map::record(core, at, &target.item_id, &target.name, reason);
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

    #[test]
    fn a_stop_past_its_hour_is_not_sent_again() {
        let since = Instant::now();
        let minute = Duration::from_secs(60);
        assert!(!stop_expired(None, since + RETRY_AFTER_CAP), "never failed");
        assert!(!stop_expired(Some(since), since + RETRY_AFTER_CAP - minute));
        assert!(stop_expired(Some(since), since + RETRY_AFTER_CAP));
    }

    /// A core whose Trakt is at `base`, holding a record for account `a`.
    async fn signed_in(base: &str) -> (Core, Arc<Conn>) {
        let core = Core::temporary("p").unwrap();
        core.set_trakt_config(Some(super::super::TraktConfig {
            client_id: "client".into(),
            redirect_uri: "https://marquee.example/trakt".into(),
            hosts: mq_tracking::trakt::Hosts {
                api: base.to_owned(),
                auth: base.to_owned(),
                local: true,
            },
        }));
        let conn = core.trakt_conn();
        let record = serde_json::from_value(json!({
            "accessToken": "access", "refreshToken": "refresh", "createdAtMs": now_ms(),
            "expiresAtMs": now_ms() + 86_400_000, "scope": "public", "account": "a",
            "user": "u", "revision": 0, "generation": 1
        }))
        .unwrap();
        assert!(conn.auth.set(Some(record)).await);
        (core, conn)
    }

    /// A movie session at 50 percent that wants to play.
    fn movie(bound: Option<Bound>) -> Sess {
        Sess {
            target: Target {
                kind: "movie".into(),
                item_id: "tt1".into(),
                video_id: "tt1".into(),
                name: "Film".into(),
            },
            report: Box::new(|_| {}),
            session: "s".into(),
            held: Arc::default(),
            since: String::new(),
            bound,
            owner: Arc::default(),
            percent: 50.0,
            want: Want::Playing,
            sent: Sent::Nothing,
            closed: false,
            ids: Some(Ok(TraktIds::Movie { imdb: "tt1".into() })),
            tries: 0,
            misses: 0,
            not_before: None,
            stop_failing_since: None,
        }
    }

    /// `movie`, closed after a stop that failed first at `since`.
    fn stopping(bound: Option<Bound>, since: Instant) -> Sess {
        Sess {
            want: Want::Paused,
            sent: Sent::Playing,
            closed: true,
            tries: 1,
            stop_failing_since: Some(since),
            ..movie(bound)
        }
    }

    /// The worker gives a stop up at its hour before sending it again.
    #[tokio::test(start_paused = true)]
    async fn the_worker_sends_no_stop_past_its_hour() {
        use std::sync::atomic::AtomicUsize;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let accepted = Arc::new(AtomicUsize::new(0));
        let counted = accepted.clone();
        std::thread::spawn(move || {
            for _ in listener.incoming().flatten() {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        });
        let (core, conn) = signed_in(&base).await;
        let since = Instant::now();
        tokio::time::advance(RETRY_AFTER_CAP).await;
        let mut sessions = BTreeMap::from([(1, stopping(conn.auth.bound().await, since))]);
        act(&core, &conn, &mut sessions).await;
        std::thread::sleep(Duration::from_millis(200));
        assert!(sessions.is_empty(), "given up");
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            0,
            "no request after its hour"
        );
    }

    /// A stop whose hour runs out while it waits its turn is not sent.
    #[tokio::test(start_paused = true)]
    async fn a_stop_whose_hour_ends_in_its_turn_is_not_sent() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let paths = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut head = Vec::new();
                let mut chunk = [0u8; 4096];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => head.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&head).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or_default();
                lock(&paths).push(path.to_owned());
                let body = r#"{"action":"start","movie":{"ids":{"imdb":"tt1"}}}"#;
                let reply = format!(
                    "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        let (core, conn) = signed_in(&base).await;
        let since = Instant::now();
        tokio::time::advance(RETRY_AFTER_CAP - Duration::from_millis(300)).await;
        tokio::time::resume();
        // A start goes: the stop's turn comes a second after it.
        let mut sessions = BTreeMap::from([(1, movie(conn.auth.bound().await))]);
        act(&core, &conn, &mut sessions).await;
        let mut sessions = BTreeMap::from([(2, stopping(conn.auth.bound().await, since))]);
        act(&core, &conn, &mut sessions).await;
        assert!(sessions.is_empty(), "given up");
        assert_eq!(*lock(&seen), ["/scrobble/start"], "no stop past its hour");
    }
}
