//! Scrobbles (plans/m7-trakt.md 4.4 and 4.5): one worker per profile turns
//! the playback views' reports into start, pause and stop. Only the newest
//! state of each session is sent, one request at a time; a view's close
//! sends its stop from the worker, after the view is gone.

use super::map::{self, TraktIds};
use super::{ACCOUNT_SETTING, Conn, TRACKER};
use crate::Core;
use mq_library::hlc::now_ms;
use mq_library::{IntentOp, NewIntent, TrackerKey};
use mq_tracking::trakt::call;
use mq_tracking::trakt::{Answer, AuthError};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::time::Instant;

/// Scrobbles wait this long for a record when the profile uses Trakt and
/// the app has not handed one over yet (it does right after `open`).
const RECORD_WAIT: Duration = Duration::from_secs(5);
/// A stop is tried this many times before it counts as unanswered.
const STOP_TRIES: u32 = 4;
/// Trakt counts a stop at or past this percent as a play.
const PLAY_PERCENT: f64 = 80.0;
/// Under this percent a closed session sends nothing.
const MIN_PERCENT: f64 = 1.0;

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

/// A playback view's end of its session. Dropping it closes the session:
/// the worker sends the stop.
pub(crate) struct Scrobble {
    id: u64,
    session: String,
    target: Target,
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

    /// The playback reached its end: one write per session waits in the
    /// outbox until the session's stop says whether Trakt took the play
    /// (then it goes), so a play survives a stop that never gets out.
    pub(crate) fn finished(&self, core: &Core) {
        if self.held.swap(true, Ordering::SeqCst) {
            return;
        }
        let library = core.library();
        let Ok(Some((account, _))) = library.tracker_account(core.profile(), TRACKER) else {
            return;
        };
        let intent = NewIntent {
            key: TrackerKey {
                profile: core.profile(),
                tracker: TRACKER,
                account: &account,
            },
            op: IntentOp::Add,
            watched_ms: now_ms() as i64,
            session: Some(&self.session),
        };
        if let Err(e) =
            library.put_tracker_intent(&self.target.item_id, &self.target.video_id, intent)
        {
            tracing::warn!("trakt: held write not queued: {e}");
        }
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
        report(
            match futures_util::FutureExt::now_or_never(conn.auth.account()).flatten() {
                Some(_) => TraktPlayback::On,
                None if super::setting(self, ACCOUNT_SETTING).is_some() => {
                    TraktPlayback::ThisDeviceOff
                }
                None => TraktPlayback::Off,
            },
        );
        let events = conn
            .events
            .get_or_init(|| {
                let (tx, rx) = unbounded_channel();
                tokio::spawn(run(self.clone(), conn.clone(), rx));
                tx
            })
            .clone();
        let id = trakt.sessions.fetch_add(1, Ordering::SeqCst) + 1;
        let session = format!("{}-{id}", trakt.boot);
        let held = Arc::new(AtomicBool::new(false));
        let _ = events.send(Event::Open {
            id,
            target: target.clone(),
            session: session.clone(),
            held: held.clone(),
            report,
        });
        Some(Scrobble {
            id,
            session,
            target,
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
    /// The clock when it opened: an unmark after it is the user's word.
    since: String,
    percent: f64,
    want: Want,
    sent: Sent,
    closed: bool,
    ids: Option<Result<TraktIds, String>>,
    tries: u32,
}

/// The profile's worker: folds every queued event, then sends what the
/// sessions want now.
async fn run(core: Core, conn: Arc<Conn>, mut events: UnboundedReceiver<Event>) {
    let mut sessions: BTreeMap<u64, Sess> = BTreeMap::new();
    let mut retry: Option<Instant> = None;
    loop {
        let first = match retry {
            Some(at) => tokio::select! {
                event = events.recv() => match event {
                    Some(event) => Some(event),
                    None => return,
                },
                () = tokio::time::sleep_until(at) => None,
            },
            None => match events.recv().await {
                Some(event) => Some(event),
                None => return,
            },
        };
        let mut waiting = Vec::new();
        let mut batch: Vec<Event> = first.into_iter().collect();
        while let Ok(event) = events.try_recv() {
            batch.push(event);
        }
        for event in batch {
            fold(&core, &mut sessions, &mut waiting, event);
        }
        retry = act(&core, &conn, &mut sessions).await;
        for done in waiting {
            let _ = done.send(());
        }
    }
}

fn fold(
    core: &Core,
    sessions: &mut BTreeMap<u64, Sess>,
    waiting: &mut Vec<oneshot::Sender<()>>,
    event: Event,
) {
    match event {
        Event::Open {
            id,
            target,
            session,
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
                    since: core.hlc(),
                    percent: 0.0,
                    want: Want::Nothing,
                    sent: Sent::Nothing,
                    closed: false,
                    ids: None,
                    tries: 0,
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
    Off,
    Transient(String),
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
        Ok(Answer::Transient { reason, .. }) => Reply::Transient(reason),
        Err(AuthError::Transient(reason)) => Reply::Transient(reason),
        Err(_) => Reply::Off,
    }
}

/// Sends what the sessions want, oldest session first, so an episode's
/// stop goes before the next one's start.
async fn act(core: &Core, conn: &Arc<Conn>, sessions: &mut BTreeMap<u64, Sess>) -> Option<Instant> {
    let trakt = core.trakt();
    wait_for_record(core, conn).await;
    let account = conn.auth.account().await;
    let (Some((api, _)), Some(account)) = (trakt.api(), account) else {
        // Nothing can be sent: closed sessions end, their writes released.
        let closed: Vec<u64> = sessions
            .iter()
            .filter(|(_, s)| s.closed)
            .map(|(id, _)| *id)
            .collect();
        for id in closed {
            if let Some(s) = sessions.remove(&id) {
                end(core, &s, false);
            }
        }
        return None;
    };
    let _lane = conn.lane.lock().await;
    let gate = trakt.gate(&account);
    let key = conn.key(&account);
    let newest_playing = sessions
        .iter()
        .filter(|(_, s)| !s.closed && s.want == Want::Playing)
        .map(|(id, _)| *id)
        .max();
    let mut retry: Option<Instant> = None;
    let ids: Vec<u64> = sessions.keys().copied().collect();
    let mut stopped_any = false;
    for id in ids {
        let s = sessions.get_mut(&id).expect("listed");
        if s.ids.is_none() {
            let resolved = resolve(core, key, &s.target).await;
            (s.report)(if resolved.is_ok() {
                TraktPlayback::On
            } else {
                TraktPlayback::Unmapped
            });
            s.ids = Some(resolved);
        }
        let Some(Ok(ids)) = s.ids.clone() else {
            if s.closed {
                let s = sessions.remove(&id).expect("listed");
                end(core, &s, false);
            }
            continue;
        };
        let superseded = newest_playing.is_some_and(|n| n > id);
        if s.closed || (superseded && matches!(s.sent, Sent::Playing | Sent::Paused)) {
            let sends = s.sent != Sent::Stopped && s.percent >= MIN_PERCENT;
            let done = if sends {
                let body = ids.scrobble(s.percent);
                let answer =
                    read(call::post(&api, &gate, &conn.auth, "/scrobble/stop", &body).await);
                stopped(core, conn, key, s, &ids, answer)
            } else {
                true
            };
            if !done {
                retry = earliest(retry, backoff(s.tries));
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
        match read(call::post(&api, &gate, &conn.auth, &path, &body).await) {
            Reply::Ok(answer) => {
                conn.ok();
                s.tries = 0;
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
            Reply::Transient(reason) => {
                conn.failed(&reason);
                s.tries += 1;
                retry = earliest(retry, backoff(s.tries));
            }
        }
    }
    if stopped_any {
        // A finished playback is a good moment for a round.
        core.trakt_schedule(conn);
    }
    core.trakt_refresh(conn);
    retry
}

/// Reads a stop's answer. Returns whether the stop is over (`false`: try
/// again later).
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
        Reply::Transient(reason) => {
            conn.failed(&reason);
            s.tries += 1;
            if s.tries < STOP_TRIES {
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

async fn resolve(core: &Core, key: TrackerKey<'_>, target: &Target) -> Result<TraktIds, String> {
    if let Some(reason) = map::known(core, key, &target.item_id) {
        return Err(reason);
    }
    let resolved = map::ids(core, &target.kind, &target.item_id, &target.video_id).await;
    if let Err(reason) = &resolved {
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

/// The wait before trying a failed send again: 1 s, doubling, at most a minute.
fn backoff(tries: u32) -> Duration {
    Duration::from_millis(500u64 << tries.clamp(1, 7))
}

fn earliest(current: Option<Instant>, wait: Duration) -> Option<Instant> {
    let at = Instant::now() + wait;
    Some(current.map_or(at, |c| c.min(at)))
}
