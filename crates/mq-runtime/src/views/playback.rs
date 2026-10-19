//! Playback: turns one picked version into a loopback URL the player opens,
//! falls over to the same release from another source when one fails, and
//! saves watch progress.
//!
//! The candidates are the picked Versions row followed by its failover chain
//! (the same release from other addons or services).

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::segments::{self, Segment, Target};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::{AddonEvent, Deadlines};
use mq_debrid::{ResolveRequest, Resolver, StoreId};
use mq_library::{Authorization, FINISHED_RATIO, LibraryItemRow, Policy, ProgressRow};
use mq_protocol::{ResourcePath, Source, decode_subtitles};
use mq_server::{Format, Lease, SubtitleSlot, Upstream};
use mq_streams::ParsedStream;
use serde::Serialize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use stremio_core::types::addon::ExtraValue;
use tokio::task::JoinHandle;
use url::Url;

/// Progress is written at most this often while playing.
const SAVE_EVERY: Duration = Duration::from_secs(5);
/// A source must answer its first byte within this to count as alive.
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// The subtitle hash reads two 64 KiB ranges; past this, ask without it.
const HASH_TIMEOUT: Duration = Duration::from_secs(4);
/// One step of a skip segment lookup (the release-matched source, or the gap
/// filler), queueing and mapping included.
const SEGMENTS_DEADLINE: Duration = Duration::from_secs(6);
/// A torrent's swarm must send its metadata within this...
#[cfg(feature = "torrent")]
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
/// ...and the head plus the container tail within this, before the player
/// gets the URL (players give up on a silent body after a few seconds).
#[cfg(feature = "torrent")]
const TORRENT_READY_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshot {
    pub phase: PlaybackPhase,
    /// Loopback URL for the player, once `ready`.
    pub url: Option<String>,
    /// The version that is playing: the picked key or one of its failovers.
    pub key: Option<String>,
    /// The source the view adopted last: one more each time it moves to a
    /// new source, never reused in the view's life (0 before the first).
    /// Two sources of one `key` (two installs listing the same link) differ
    /// here; `failOver` names it.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub attempt: u64,
    /// `url` is a manifest the core rewrote, not a file: HLS (its name ends
    /// in `.m3u8`) or DASH (`.mpd`).
    pub manifest: bool,
    pub filename: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size_bytes: Option<u64>,
    /// Where to start, from saved progress.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub resume_ms: Option<u64>,
    /// Sources that failed, in order, each with a plain reason.
    pub attempts: Vec<Attempt>,
    /// External subtitles: the stream's own, then each subtitles addon's, as
    /// they arrive.
    pub subtitles: Vec<SubtitleTrack>,
    /// Subtitles addons are still being asked.
    pub subtitles_pending: bool,
    /// The next released episode, for an Up Next card.
    pub next: Option<NextEpisode>,
    /// The playing stream's binge group; pass it as `bingeGroup` when opening
    /// Versions for the next episode so the same release line is picked.
    pub binge_group: Option<String>,
    /// Skip segments of the playing version, once its duration is reported.
    pub segments: Vec<Segment>,
    /// When to offer the Up Next card: the credits start from a source that
    /// matched the release; null keeps the app's own timing.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub next_at_ms: Option<u64>,
    /// Trakt's side: `on` scrobbles, `unmapped` (Trakt cannot name it),
    /// `off`, `thisDeviceOff` (the profile uses Trakt, this device is not
    /// signed in).
    pub trakt: crate::trakt::TraktPlayback,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct NextEpisode {
    pub video_id: String,
    pub title: String,
    pub season: u32,
    pub episode: u32,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub url: String,
    /// As the addon gives it, usually ISO 639-2 ("eng").
    pub lang: String,
    pub label: Option<String>,
    /// Addon name.
    pub source: String,
    pub forced: bool,
    pub hearing_impaired: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackPhase {
    Resolving,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub key: String,
    pub via: String,
    pub error: String,
}

/// What is being watched, for progress and Continue Watching.
#[derive(Debug, Clone, Default)]
pub struct PlayTarget {
    pub kind: String,
    pub item_id: String,
    pub video_id: String,
    pub name: Option<String>,
    pub poster: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

struct State {
    snapshot: PlaybackSnapshot,
    candidates: Vec<ParsedStream>,
    next: usize,
    /// The playing source's token: its URL and its subtitles' URLs live as
    /// long as it is kept here.
    lease: Option<Lease>,
    /// Bumped under this lock by every new resolve and every failover; a
    /// resolve task adopts a lease, and publishes tracks, only while its own
    /// number is the newest.
    run: u64,
    /// The view is gone: no lease is ever adopted again.
    closed: bool,
    last: Option<(u64, u64)>,
    saved_at: Option<Instant>,
    dirty: bool,
    /// Bumped whenever the playing source changes, so a skip lookup for an
    /// earlier source never lands on this one.
    source: u64,
    /// The source and duration the current skip lookup is for.
    lookup: Option<(u64, u64)>,
    /// The running skip lookup; started and replaced under the state lock,
    /// so an older report can never abort a newer lookup.
    segments_task: Option<JoinHandle<()>>,
    /// Every torrent this view started, from the moment `add_magnet`
    /// answered: a policy closure pauses them all, a lease adopted or not.
    #[cfg(feature = "torrent")]
    torrents: Vec<mq_engine::TorrentHandle>,
}

pub struct PlaybackView {
    core: Core,
    target: Arc<PlayTarget>,
    /// The playlist formats the player plays (its caps).
    plays: Arc<[Format]>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    state: Arc<Mutex<State>>,
    /// The resolve task and its run.
    task: Mutex<Option<(u64, JoinHandle<()>)>>,
    /// The Trakt session; dropped after the last save, which sends the stop.
    trakt: Option<crate::trakt::Scrobble>,
    /// A kids policy refused the target: the view does no work.
    denied: bool,
}

impl State {
    fn new(candidates: Vec<ParsedStream>, resume_ms: Option<u64>) -> Self {
        Self {
            snapshot: PlaybackSnapshot {
                phase: PlaybackPhase::Resolving,
                url: None,
                key: None,
                attempt: 0,
                manifest: false,
                filename: None,
                size_bytes: None,
                resume_ms,
                attempts: Vec::new(),
                subtitles: Vec::new(),
                subtitles_pending: false,
                next: None,
                binge_group: None,
                segments: Vec::new(),
                next_at_ms: None,
                trakt: crate::trakt::TraktPlayback::Off,
            },
            candidates,
            next: 0,
            lease: None,
            run: 0,
            closed: false,
            last: None,
            saved_at: None,
            dirty: false,
            source: 0,
            lookup: None,
            segments_task: None,
            #[cfg(feature = "torrent")]
            torrents: Vec::new(),
        }
    }
}

impl PlaybackView {
    /// `open_for` a player that plays HLS.
    pub fn open(core: &Core, candidates: Vec<ParsedStream>, target: PlayTarget) -> Self {
        Self::open_for(core, candidates, target, &[Format::Hls])
    }

    /// Opens the candidates in turn for a player that plays the playlist
    /// formats `plays` names (`VersionsView::plays`): a playlist it cannot
    /// play fails over like any source that does not open. Under a kids
    /// policy a target off the list, or a video not of its title, opens
    /// denied: no work, an empty snapshot, and `denied()` says so.
    pub fn open_for(
        core: &Core,
        candidates: Vec<ParsedStream>,
        target: PlayTarget,
        plays: &[Format],
    ) -> Self {
        let denied = core.policy().kids()
            && !matches!(
                core.library().authorize(
                    core.profile(),
                    &target.kind,
                    &target.item_id,
                    Some(&target.video_id),
                ),
                Ok(Authorization::Authorized { .. })
            );
        if denied {
            let state = State::new(Vec::new(), None);
            return Self {
                core: core.clone(),
                snapshot: SharedObservable::new(state.snapshot.clone()),
                state: Arc::new(Mutex::new(state)),
                target: Arc::new(target),
                plays: plays.into(),
                task: Mutex::new(None),
                trakt: None,
                denied: true,
            };
        }
        let resume_ms = core
            .library()
            .progress(core.profile(), &target.video_id)
            .ok()
            .flatten()
            .filter(|&(pos, dur)| {
                pos > 0 && (dur == 0 || (pos as f64) < dur as f64 * FINISHED_RATIO)
            })
            .map(|(pos, _)| pos as u64);
        let state = State::new(candidates, resume_ms);
        let snapshot = SharedObservable::new(state.snapshot.clone());
        let shared = Arc::new(Mutex::new(state));
        let report = {
            let (shared, snapshot) = (Arc::downgrade(&shared), snapshot.clone());
            move |trakt| {
                if let Some(state) = shared.upgrade() {
                    let mut s = lock(&state);
                    s.snapshot.trakt = trakt;
                    snapshot.set(s.snapshot.clone());
                }
            }
        };
        let session = crate::trakt::ScrobbleTarget {
            kind: target.kind.clone(),
            item_id: target.item_id.clone(),
            video_id: target.video_id.clone(),
            name: target.name.clone().unwrap_or_default(),
        };
        let trakt = core.trakt_scrobble(session, Box::new(report));
        let view = Self {
            core: core.clone(),
            target: Arc::new(target),
            plays: plays.into(),
            snapshot,
            state: shared,
            task: Mutex::new(None),
            trakt,
            denied: false,
        };
        view.spawn_resolve();
        view
    }

    /// A kids policy refused the target at open.
    pub fn denied(&self) -> bool {
        self.denied
    }

    /// The title left the kids profile's list (plans/m11-kids.md 4.4, 8):
    /// the resolve stops (the subtitle hash reader with it), the position is
    /// saved, the lease drops (its URLs end and bodies being served stop),
    /// no lease is adopted again, and every torrent the view started is
    /// held for this profile with the `generation` the denial was read at
    /// (paused at once unless another profile is playing it). A failover
    /// after this does nothing.
    pub async fn stop_forbidden(&self, generation: u64) {
        if let Some((_, task)) = lock(&self.task).take() {
            task.abort();
        }
        self.save();
        let started = {
            let mut s = lock(&self.state);
            if let Some(task) = s.segments_task.take() {
                task.abort();
            }
            // A resolve still running reads this under the same lock: it
            // adopts no lease and starts no torrent after this.
            s.closed = true;
            release_all(&mut s);
            self.snapshot.set(s.snapshot.clone());
            #[cfg(feature = "torrent")]
            let started = std::mem::take(&mut s.torrents);
            #[cfg(not(feature = "torrent"))]
            let started: Vec<()> = Vec::new();
            started
        };
        #[cfg(feature = "torrent")]
        if !started.is_empty()
            && let Ok(engine) = self.core.torrent_engine().await
        {
            for torrent in started {
                if let Err(e) = engine
                    .pause_now(torrent, self.core.profile(), generation)
                    .await
                {
                    tracing::warn!("pausing a forbidden torrent: {e}");
                }
            }
        }
        #[cfg(not(feature = "torrent"))]
        let _ = (started, generation);
    }

    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        PlaybackSnapshot,
        Pin<Box<dyn Stream<Item = Vec<PlaybackSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }

    /// The player could not open or keep playing the source `failed` (the
    /// snapshot's `attempt`; `None` means the current one): drop it and move
    /// to the next candidate. A late report about a source already given up
    /// on is ignored, so it never drops the one playing now, even when both
    /// have one `key`.
    ///
    /// `kind`, `status` and `error` are what the player reports; the attempt
    /// list gets the core's words for them (`mq_server::failure_words`).
    pub fn fail_over(
        &self,
        failed: Option<u64>,
        kind: Option<&str>,
        status: Option<u16>,
        error: &str,
    ) {
        let error = mq_server::failure_words(kind, status, error);
        let error = error.as_str();
        {
            let mut s = lock(&self.state);
            let other = failed.is_some_and(|failed| failed != s.snapshot.attempt);
            // A stopped view opens no other source (a debrid one would
            // reach the account for a title off the list).
            if s.snapshot.phase != PlaybackPhase::Ready || other || s.closed {
                return;
            }
            // The running task's work for this source ends here.
            s.run += 1;
            release_all(&mut s);
            if let Some(stream) = s.next.checked_sub(1).and_then(|i| s.candidates.get(i)) {
                let attempt = attempt(stream, error);
                s.snapshot.attempts.push(attempt);
            }
            s.snapshot.phase = PlaybackPhase::Resolving;
            s.snapshot.key = None;
            s.snapshot.subtitles_pending = false;
            clear_segments(&mut s);
            self.snapshot.set(s.snapshot.clone());
        }
        self.spawn_resolve();
    }

    /// The player knows the duration of the version `key`: look its skip
    /// segments up. Reports for another version, durations under a minute or
    /// over six hours, and repeats within 2 percent are ignored; a larger
    /// correction looks up again.
    pub fn report_duration(&self, key: &str, duration_ms: u64) {
        if !(60_000..=6 * 3_600_000).contains(&duration_ms) {
            return;
        }
        let t = &self.target;
        let Some(target) = Target::from_ids(&t.kind, &t.item_id, &t.video_id) else {
            return;
        };
        let mut s = lock(&self.state);
        {
            if s.snapshot.phase != PlaybackPhase::Ready || s.snapshot.key.as_deref() != Some(key) {
                return;
            }
            if let Some((source, known)) = s.lookup
                && source == s.source
                && known.abs_diff(duration_ms) * 50 <= known
            {
                return;
            }
        }
        let lookup = (s.source, duration_ms);
        s.lookup = Some(lookup);
        if !s.snapshot.segments.is_empty() || s.snapshot.next_at_ms.is_some() {
            s.snapshot.segments.clear();
            s.snapshot.next_at_ms = None;
            self.snapshot.set(s.snapshot.clone());
        }
        // Spawning does not block, so it happens under the lock.
        let task = tokio::spawn(find_segments(
            self.core.clone(),
            self.state.clone(),
            self.snapshot.clone(),
            target,
            lookup,
        ));
        if let Some(old) = s.segments_task.replace(task) {
            old.abort();
        }
    }

    /// Player position report, `paused` when the player is paused. Saved at
    /// most every few seconds; the last report is always saved when the
    /// view closes. Trakt gets start and pause from it.
    pub fn report_progress(&self, position_ms: u64, duration_ms: u64, paused: bool) {
        // No progress row for a title off the kids list.
        if self.denied {
            return;
        }
        if let Some(trakt) = &self.trakt {
            trakt.progress(position_ms, duration_ms, paused);
        }
        let due = {
            let mut s = lock(&self.state);
            s.last = Some((position_ms, duration_ms));
            s.dirty = true;
            s.saved_at.is_none_or(|t| t.elapsed() >= SAVE_EVERY)
        };
        if due {
            self.save();
        }
    }

    fn save(&self) {
        let (position_ms, duration_ms, first) = {
            let mut s = lock(&self.state);
            let Some((pos, dur)) = s.last.filter(|_| s.dirty) else {
                return;
            };
            let first = s.saved_at.is_none();
            s.dirty = false;
            s.saved_at = Some(Instant::now());
            (pos, dur, first)
        };
        let saved = save_progress(
            &self.core,
            &self.target,
            position_ms,
            duration_ms,
            first,
            self.trakt.as_ref(),
        );
        if let Err(e) = saved {
            tracing::warn!("progress save failed: {e}");
        }
    }

    fn spawn_resolve(&self) {
        let run = {
            let mut s = lock(&self.state);
            s.run += 1;
            s.run
        };
        let task = tokio::spawn(resolve_next(
            self.core.clone(),
            self.target.clone(),
            self.plays.clone(),
            self.state.clone(),
            self.snapshot.clone(),
            run,
        ));
        install(&self.task, run, task);
    }
}

/// Keeps `task` as the view's resolve task unless a newer run's task is
/// already there: two callers racing between spawning and installing never
/// leave the older task running in place of the newer.
fn install(slot: &Mutex<Option<(u64, JoinHandle<()>)>>, run: u64, task: JoinHandle<()>) {
    let mut slot = lock(slot);
    if slot.as_ref().is_some_and(|(newer, _)| *newer > run) {
        task.abort();
    } else if let Some((_, old)) = slot.replace((run, task)) {
        old.abort();
    }
}

impl Drop for PlaybackView {
    fn drop(&mut self) {
        if let Some((_, task)) = lock(&self.task).take() {
            task.abort();
        }
        if let Some(task) = lock(&self.state).segments_task.take() {
            task.abort();
        }
        self.save();
        // A task still running after the abort can never adopt a lease now.
        let mut s = lock(&self.state);
        s.closed = true;
        release_all(&mut s);
    }
}

/// A new source: the old one's skip segments do not apply to it.
fn clear_segments(s: &mut State) {
    s.source += 1;
    s.lookup = None;
    if let Some(task) = s.segments_task.take() {
        task.abort();
    }
    s.snapshot.segments.clear();
    s.snapshot.next_at_ms = None;
}

/// Finds skip segments for one source and duration: the release-matched
/// source first, published at once, then the gap filler. When the
/// release-matched source cannot be asked (a timeout, a refusal), the filler
/// goes ahead and that source is asked again after each of the finder's
/// waits, so one slow answer does not cost the whole playback its Skip; what
/// it finds later takes precedence over the filler's. Results for a source or
/// duration that is no longer current are dropped, and the task ends with the
/// source.
async fn find_segments(
    core: Core,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    target: Target,
    lookup: (u64, u64),
) {
    let publish = |change: &dyn Fn(&mut PlaybackSnapshot)| {
        let mut s = lock(&state);
        if s.lookup == Some(lookup) {
            change(&mut s.snapshot);
            snapshot.set(s.snapshot.clone());
        }
    };
    let duration_ms = lookup.1;
    let finder = core.segments();
    let fill = |have: Vec<Segment>| {
        let finder = &finder;
        let target = &target;
        async move {
            let extra =
                tokio::time::timeout(SEGMENTS_DEADLINE, finder.fill(target, duration_ms, &have))
                    .await
                    .unwrap_or_default();
            if !extra.is_empty() {
                publish(&|s| s.segments = segments::merge(&s.segments, extra.clone()));
            }
        }
    };
    let mut filled = false;
    let waits = std::iter::once(Duration::ZERO).chain(finder.retries());
    for wait in waits {
        tokio::time::sleep(wait).await;
        let asked =
            tokio::time::timeout(SEGMENTS_DEADLINE, finder.primary(&target, duration_ms)).await;
        match asked {
            Ok(Some(found)) => {
                if !found.segments.is_empty() {
                    publish(&|s| {
                        s.segments = segments::merge(&found.segments, s.segments.clone());
                        s.next_at_ms = found.next_at_ms;
                    });
                }
                if !filled {
                    fill(found.segments).await;
                }
                return;
            }
            _ if !filled => {
                filled = true;
                fill(Vec::new()).await;
            }
            _ => {}
        }
    }
}

/// Ends the stream's and the subtitles' loopback URLs; they stop working.
/// Under the view's lock, which the lease's drop follows with the server's:
/// never the reverse.
fn release_all(s: &mut State) {
    s.snapshot.url = None;
    s.snapshot.manifest = false;
    s.snapshot.subtitles.clear();
    s.lease = None;
}

/// Whether `run` still owns the view's source and tracks.
fn current(s: &State, run: u64) -> bool {
    !s.closed && s.run == run
}

/// Keeps `lease` as the playing source's, only while the view is open and
/// `run` is still the newest; else it is dropped here, which ends its token.
fn adopt(s: &mut State, run: u64, lease: Lease) -> bool {
    if !current(s, run) {
        return false;
    }
    s.snapshot.url = Some(lease.url.clone());
    s.snapshot.manifest = lease.format.is_some();
    s.snapshot.attempt += 1;
    s.lease = Some(lease);
    true
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn attempt(stream: &ParsedStream, error: &str) -> Attempt {
    Attempt {
        key: stream.key.clone(),
        via: stream
            .service
            .clone()
            .unwrap_or_else(|| stream.addon_name.clone()),
        error: error.to_owned(),
    }
}

/// Saves a position. At the end it marks the video watched, with the write
/// a Trakt session holds for it in the same transaction.
fn save_progress(
    core: &Core,
    target: &PlayTarget,
    position_ms: u64,
    duration_ms: u64,
    first: bool,
    trakt: Option<&crate::trakt::Scrobble>,
) -> Result<(), crate::CoreError> {
    let (library, profile) = (core.library(), core.profile());
    // Continue Watching needs a name and poster for titles outside the library.
    if first
        && let Some(name) = &target.name
        && !library.is_in_library(profile, &target.item_id)?
    {
        let row = LibraryItemRow {
            item_id: target.item_id.clone(),
            kind: target.kind.clone(),
            name: name.clone(),
            poster: target.poster.clone(),
            poster_shape: None,
            added_ms: mq_library::hlc::now_ms() as i64,
        };
        // Only a cache of what was played: any real library edit wins.
        library.upsert_item(profile, &row, false, &core.zero_hlc())?;
    }
    library.set_progress(
        profile,
        ProgressRow {
            video_id: &target.video_id,
            item_id: &target.item_id,
            position_ms: position_ms as i64,
            duration_ms: duration_ms as i64,
            updated_ms: mq_library::hlc::now_ms() as i64,
        },
        core.clock(),
    )?;
    if duration_ms > 0 && position_ms as f64 >= duration_ms as f64 * FINISHED_RATIO {
        let account = trakt.and_then(|t| t.to_hold(core));
        let hold = trakt
            .zip(account.as_deref())
            .map(|(t, a)| t.intent(profile, a));
        let holds = hold.is_some();
        library.set_watched_holding(
            profile,
            &target.item_id,
            &target.video_id,
            core.clock(),
            hold,
        )?;
        if let Some(trakt) = trakt.filter(|_| holds) {
            trakt.held();
        }
    }
    Ok(())
}

/// Tries candidates from `state.next` until one answers, then publishes its
/// loopback URL; publishes `Failed` when none is left.
async fn resolve_next(
    core: Core,
    target: Arc<PlayTarget>,
    plays: Arc<[Format]>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    run: u64,
) {
    let server = match core.stream_server().await {
        Ok(server) => server,
        Err(e) => {
            let mut s = lock(&state);
            s.snapshot.phase = PlaybackPhase::Failed;
            s.snapshot.attempts.push(Attempt {
                key: String::new(),
                via: "Marquee".into(),
                error: format!("local stream server did not start: {e}"),
            });
            snapshot.set(s.snapshot.clone());
            return;
        }
    };
    loop {
        let stream = {
            let mut s = lock(&state);
            if s.closed {
                return;
            }
            let Some(stream) = s.candidates.get(s.next).cloned() else {
                s.snapshot.phase = PlaybackPhase::Failed;
                snapshot.set(s.snapshot.clone());
                return;
            };
            s.next += 1;
            stream
        };
        let opened = match open_source(&core, &stream, &target, &state).await {
            Ok((upstream, filename, size)) => {
                let name = filename
                    .clone()
                    .unwrap_or_else(|| format!("{}.mkv", target.video_id));
                server
                    .open_for(upstream.clone(), &name, PROBE_TIMEOUT, &plays)
                    .await
                    .map(|lease| (upstream, filename, size, lease))
            }
            Err(e) => Err(e),
        };
        match opened {
            Ok((upstream, filename, size, lease)) => {
                let wanted = core.prefs().subtitle_languages;
                // The stream's own tracks come from its addon.
                let addon_url = stream.addon_url.as_ref();
                let slot = lease.subtitles();
                let reader = lease.reader();
                {
                    let mut s = lock(&state);
                    if !adopt(&mut s, run, lease) {
                        return;
                    }
                    clear_segments(&mut s);
                    s.snapshot.phase = PlaybackPhase::Ready;
                    s.snapshot.key = Some(stream.key.clone());
                    s.snapshot.filename = filename.clone();
                    s.snapshot.size_bytes = size.or(stream.size_bytes);
                    s.snapshot.subtitles = stream
                        .subtitles
                        .iter()
                        .filter_map(|w| track(w, &stream.addon_name, &slot, addon_url))
                        .collect();
                    order_subtitles(&mut s.snapshot.subtitles, &wanted);
                    s.snapshot.subtitles_pending = true;
                    s.snapshot.binge_group = stream.binge_group.clone();
                    snapshot.set(s.snapshot.clone());
                }
                remember_stream(&core, &target, &stream);
                let subtitles = async {
                    let extra = video_extra(&upstream, reader, filename.as_deref()).await;
                    find_subtitles(&core, &target, extra, &slot, run, &state, &snapshot).await
                };
                let next = async {
                    if let Some(next) = next_episode(&core, &target).await {
                        let mut s = lock(&state);
                        // Checked as it is set: the title may have left a
                        // kids list while its meta was on the way.
                        let policy = core.policy();
                        let allowed = policy.allows(&target.kind, &target.item_id)
                            && (!policy.kids() || Policy::belongs(&target.item_id, &next.video_id));
                        if allowed && !s.closed {
                            s.snapshot.next = Some(next);
                            snapshot.set(s.snapshot.clone());
                        }
                    }
                };
                tokio::join!(subtitles, next);
                return;
            }
            Err(error) => {
                let mut s = lock(&state);
                s.snapshot.attempts.push(attempt(&stream, &error));
                snapshot.set(s.snapshot.clone());
            }
        }
    }
}

type Opened = (Upstream, Option<String>, Option<u64>);

/// Resolves one stream to an upstream. Its scope is the install that listed
/// it (the candidate's `addon_url`); neither the installed row's bit, which
/// sync may set, nor the manifest id, which any addon may declare, is read.
async fn open_source(
    core: &Core,
    stream: &ParsedStream,
    target: &PlayTarget,
    state: &Mutex<State>,
) -> Result<Opened, String> {
    #[cfg(not(feature = "torrent"))]
    let _ = state;
    let (url, headers, filename, size, addon_url) = match &stream.source {
        Source::Http { url } => (
            url.to_string(),
            stream.headers.clone(),
            stream.filename.clone(),
            stream.size_bytes,
            stream.addon_url.clone(),
        ),
        Source::Torrent {
            info_hash,
            file_idx,
            trackers,
            ..
        } if core.debrid().is_none() => {
            // Full builds play torrents with the built-in engine; store
            // builds have none and need a debrid account.
            #[cfg(feature = "torrent")]
            return open_torrent(core, stream, target, state, info_hash, *file_idx, trackers).await;
            #[cfg(not(feature = "torrent"))]
            {
                let _ = (info_hash, file_idx, trackers);
                return Err("Torrents need a debrid account in this version".into());
            }
        }
        Source::ClientResolve(_) | Source::Torrent { .. } => {
            let (service, request) = debrid_request(stream, target.season, target.episode)
                .ok_or("The addon did not say which torrent to play")?;
            let resolver = core
                .debrid()
                .ok_or("This version needs a debrid account. Add one in Settings")?;
            let resolved = resolver
                .resolve(with_store(&resolver, service.as_deref(), request))
                .await
                .map_err(|e| e.to_string());
            // Hybrid: a torrent the debrid store cannot give plays peer to
            // peer in full builds.
            #[cfg(feature = "torrent")]
            if let (
                Err(e),
                Source::Torrent {
                    info_hash,
                    file_idx,
                    trackers,
                    ..
                },
            ) = (&resolved, &stream.source)
            {
                return open_torrent(core, stream, target, state, info_hash, *file_idx, trackers)
                    .await
                    .map_err(|p2p| format!("{e}; peer to peer: {p2p}"));
            }
            let link = resolved?;
            // The provider's link, not the addon's: no install scope.
            (link.url, Vec::new(), Some(link.filename), link.size, None)
        }
        other => return Err(format!("{} streams cannot play here", source_word(other))),
    };
    // Whether it answers is checked by the stream server's first read.
    Ok((
        Upstream::Http {
            url,
            headers,
            addon_url,
        },
        filename,
        size,
    ))
}

/// Plays a torrent with the built-in engine: metadata, file choice (the
/// addon's `fileIdx`, else its filename, else the episode, else the largest
/// video), then the head and container tail verified before the player gets
/// the URL.
#[cfg(feature = "torrent")]
async fn open_torrent(
    core: &Core,
    stream: &ParsedStream,
    target: &PlayTarget,
    state: &Mutex<State>,
    info_hash: &str,
    file_idx: Option<u32>,
    trackers: &[String],
) -> Result<Opened, String> {
    use mq_engine::FileHint;
    let engine = core.torrent_engine().await.map_err(|e| e.to_string())?;
    // This playback's own check, read before the torrent is touched: a
    // hold placed after it (the title left a kids list) is never lifted by
    // it (plans/m11-kids.md 4.4).
    let generation = match core.library().authorize(
        core.profile(),
        &target.kind,
        &target.item_id,
        Some(&target.video_id),
    ) {
        Ok(Authorization::Authorized { generation }) => generation,
        Ok(Authorization::Denied { .. }) => return Err("This title is not on the list".into()),
        Err(e) => return Err(e.to_string()),
    };
    let torrent = tokio::time::timeout(
        METADATA_TIMEOUT,
        engine.add_magnet(info_hash, trackers, &[]),
    )
    .await
    .map_err(|_| "No peer sent the torrent's details in time".to_owned())?
    .map_err(|e| e.to_string())?;
    // This profile's use of it, for as long as the playback lives: a hold
    // for another profile leaves the torrent running for it.
    let profile = core.profile();
    let claim = engine.claim(torrent, profile).map_err(|e| e.to_string())?;
    {
        // Kept from now on, so a closure pauses it even before its head is
        // ready; a view already stopped starts nothing.
        let mut s = lock(state);
        if s.closed {
            return Err("The playback was stopped".into());
        }
        s.torrents.push(torrent);
    }
    // Only a newer authorization lifts a hold; an older one leaves it, and
    // the file choice below then answers that the torrent is held.
    if let Err(e) = engine.release_hold(torrent, profile, generation).await {
        tracing::debug!("torrent hold: {e}");
    }
    let hint = match (file_idx, &stream.filename, target.season, target.episode) {
        (Some(i), ..) => FileHint::Index(i as usize),
        (None, Some(name), ..) => FileHint::Name(name.clone()),
        (None, None, Some(season), Some(episode)) => FileHint::Episode { season, episode },
        _ => FileHint::Largest,
    };
    let started = async {
        let file = engine.select_file(torrent, profile, &hint).await?;
        let reader = claim.open_reader(file).await?;
        let mut head = reader.range(0, None).await?;
        let ready = tokio::time::timeout(TORRENT_READY_TIMEOUT, head.ready()).await;
        Ok::<_, mq_engine::EngineError>((file, reader.file().clone(), ready))
    };
    let failed = match started.await {
        Ok((file, info, Ok(Ok(())))) => {
            let name = info
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&info.path)
                .to_owned();
            let upstream = Upstream::Torrent { claim, file };
            return Ok((upstream, Some(name), Some(info.len)));
        }
        // Held for this profile (its check is older than the hold): the
        // torrent, its hold and its files stay, since another profile may
        // be playing it, and the next candidate plays instead.
        Err(mq_engine::EngineError::Held) => return Err("This title is not on the list".into()),
        Ok((_, _, Err(_))) => "the swarm did not send the start of the video in time".to_owned(),
        Ok((_, _, Ok(Err(e)))) | Err(e) => e.to_string(),
    };
    // The next candidate plays instead: stop downloading this one.
    let _ = engine.remove(torrent, false).await;
    Err(failed)
}

/// Verified torrent bytes for the subtitle hash.
#[cfg(feature = "torrent")]
struct TorrentBytes(mq_engine::StreamReader);

#[cfg(feature = "torrent")]
impl mq_media::RangeReader for TorrentBytes {
    fn len(&self) -> futures_util::future::BoxFuture<'_, Result<u64, mq_media::ProbeError>> {
        let len = self.0.len();
        Box::pin(async move { Ok(len) })
    }

    fn read_at(
        &self,
        offset: u64,
        len: u64,
    ) -> futures_util::future::BoxFuture<'_, Result<bytes::Bytes, mq_media::ProbeError>> {
        Box::pin(async move {
            use tokio::io::AsyncReadExt;
            let err = |e: String| mq_media::ProbeError::Http(e);
            let mut range = self
                .0
                .range(offset, Some(offset.saturating_add(len)))
                .await
                .map_err(|e| err(e.to_string()))?;
            let mut out = Vec::new();
            range
                .read_to_end(&mut out)
                .await
                .map_err(|e| err(e.to_string()))?;
            Ok(out.into())
        })
    }
}

/// What to ask a debrid store for this stream, with the service the addon
/// named; `None` for sources that need no debrid.
pub(crate) fn debrid_request(
    stream: &ParsedStream,
    season: Option<u32>,
    episode: Option<u32>,
) -> Option<(Option<String>, ResolveRequest)> {
    match &stream.source {
        Source::ClientResolve(c) => Some((
            c.service.clone().or_else(|| stream.service.clone()),
            ResolveRequest {
                store: None,
                info_hash: c.info_hash.clone()?,
                file_idx: c.file_idx,
                filename_hint: c.filename.clone(),
                season: c.season.or(season),
                episode: c.episode.or(episode),
            },
        )),
        Source::Torrent {
            info_hash,
            file_idx,
            ..
        } => Some((
            stream.service.clone(),
            ResolveRequest {
                store: None,
                info_hash: info_hash.clone(),
                file_idx: *file_idx,
                filename_hint: stream.filename.clone(),
                season,
                episode,
            },
        )),
        _ => None,
    }
}

/// Uses the store the addon named when that account is configured, else the
/// default store.
fn with_store(
    resolver: &Resolver,
    service: Option<&str>,
    mut request: ResolveRequest,
) -> ResolveRequest {
    request.store = service
        .and_then(StoreId::from_service)
        .filter(|id| resolver.stores().contains(id));
    request
}

/// Warms the debrid link of a version the user is likely to play, so Play
/// starts from the resolver cache. Only versions the addon reports cached:
/// asking a store for an uncached torrent adds it to the user's account.
pub(crate) fn prefetch(
    core: &Core,
    stream: &ParsedStream,
    season: Option<u32>,
    episode: Option<u32>,
) {
    if stream.cached != Some(true) {
        return;
    }
    let (Some(resolver), Some((service, request))) =
        (core.debrid(), debrid_request(stream, season, episode))
    else {
        return;
    };
    let request = with_store(&resolver, service.as_deref(), request);
    tokio::spawn(async move {
        if let Err(e) = resolver.prefetch(request).await {
            tracing::debug!("debrid prefetch failed: {e}");
        }
    });
}

fn source_word(source: &Source) -> &'static str {
    match source {
        Source::YouTube { .. } => "YouTube",
        Source::PlayerFrame { .. } => "Embedded player",
        Source::External { .. } => "External",
        Source::Usenet { .. } => "Usenet",
        Source::Archive { .. } => "Archive",
        _ => "These",
    }
}

/// A track served through the loopback server as UTF-8 WebVTT, whatever
/// format and charset the addon's file has; `None` once its playback is
/// released.
fn track(
    w: &mq_protocol::WireSubtitle,
    source: &str,
    slot: &SubtitleSlot,
    addon_url: Option<&Url>,
) -> Option<SubtitleTrack> {
    Some(SubtitleTrack {
        url: slot.add(w.url.as_str(), &w.lang, addon_url)?,
        lang: w.lang.clone(),
        label: w.label.clone(),
        source: source.to_owned(),
        forced: w.forced,
        hearing_impaired: w.hearing_impaired,
    })
}

/// The OpenSubtitles hash and size when they can be had (first and last
/// 64 KiB, read in process for files), and the filename.
async fn video_extra(
    upstream: &Upstream,
    reader: Option<Box<dyn mq_media::RangeReader>>,
    filename: Option<&str>,
) -> Vec<ExtraValue> {
    let reader = match upstream {
        // Over the playback's own cache and cursors; none for passthrough.
        Upstream::Http { .. } => reader,
        #[cfg(feature = "torrent")]
        Upstream::Torrent { claim, file } => match claim.open_reader(*file).await {
            Ok(reader) => Some(Box::new(TorrentBytes(reader)) as Box<dyn mq_media::RangeReader>),
            Err(e) => {
                tracing::warn!("torrent reader for the subtitle hash: {e}");
                None
            }
        },
    };
    let mut extra = Vec::new();
    // Origins that ignore Range give no hash; filename and id still match.
    if let Some(reader) = reader
        && let Ok(Ok((hash, size))) =
            tokio::time::timeout(HASH_TIMEOUT, mq_media::opensubtitles_hash(&*reader)).await
    {
        extra.push(extra_value("videoHash", hash));
        extra.push(extra_value("videoSize", size.to_string()));
    }
    if let Some(name) = filename {
        extra.push(extra_value("filename", name.to_owned()));
    }
    extra
}

/// Asks every subtitles addon for this video with `extra`, and adds tracks
/// to the snapshot as each addon answers, while `run` is still the view's.
async fn find_subtitles(
    core: &Core,
    target: &PlayTarget,
    extra: Vec<ExtraValue>,
    slot: &SubtitleSlot,
    run: u64,
    state: &Mutex<State>,
    snapshot: &SharedObservable<PlaybackSnapshot>,
) {
    let path = ResourcePath {
        resource: "subtitles".into(),
        r#type: target.kind.clone(),
        id: target.video_id.clone(),
        extra,
    };
    let addons: Vec<_> = core
        .installed_addons()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.supports(&path))
        .collect();
    let wanted = core.prefs().subtitle_languages;
    let mut fanout = core.addons().fan_out(&addons, &path, Deadlines::default());
    while let Some(event) = fanout.next().await {
        let mut s = lock(state);
        if !current(&s, run) {
            return;
        }
        match event {
            AddonEvent::Arrived {
                addon_id,
                transport_url,
                body,
                ..
            } => {
                let addon = addons.iter().find(|a| a.transport_url == transport_url);
                let source = addon.map_or(addon_id.clone(), |a| a.manifest.name.clone());
                let found = decode_subtitles(&body)
                    .map(|r| r.subtitles)
                    .unwrap_or_default();
                for w in &found {
                    // One file, one loopback address: compare by it. The
                    // fetch has the scope of the install that answered.
                    let Some(t) = track(w, &source, slot, Some(&transport_url)) else {
                        return;
                    };
                    if !s.snapshot.subtitles.iter().any(|known| known.url == t.url) {
                        s.snapshot.subtitles.push(t);
                    }
                }
                order_subtitles(&mut s.snapshot.subtitles, &wanted);
            }
            AddonEvent::Done => s.snapshot.subtitles_pending = false,
            _ => continue,
        }
        snapshot.set(s.snapshot.clone());
    }
    let mut s = lock(state);
    if current(&s, run) && s.snapshot.subtitles_pending {
        s.snapshot.subtitles_pending = false;
        snapshot.set(s.snapshot.clone());
    }
}

fn extra_value(name: &str, value: String) -> ExtraValue {
    ExtraValue {
        name: name.into(),
        value,
    }
}

/// Preferred languages first, in preference order; the rest keep their order.
fn order_subtitles(tracks: &mut [SubtitleTrack], wanted: &[String]) {
    tracks.sort_by_key(|t| {
        wanted
            .iter()
            .position(|w| w.eq_ignore_ascii_case(&t.lang))
            .unwrap_or(wanted.len())
    });
}

/// The episode after this one in season and episode order, skipping
/// specials (season 0) unless a special is playing and episodes not out yet.
async fn next_episode(core: &Core, target: &PlayTarget) -> Option<NextEpisode> {
    target.season?;
    let (meta, _) = core.fetch_meta(&target.kind, &target.item_id).await.ok()?;
    next_after(&meta, &target.video_id)
}

/// The released episode after `video_id` in `meta`, shared with Home's
/// Next Up.
pub(crate) fn next_after(meta: &mq_protocol::MetaItem, video_id: &str) -> Option<NextEpisode> {
    let current = meta
        .videos
        .iter()
        .find(|v| v.id == video_id)?
        .series_info
        .clone()?;
    let now = mq_library::hlc::now_ms() as i64;
    let mut episodes: Vec<_> = meta
        .videos
        .iter()
        .filter_map(|v| Some((v, v.series_info.clone()?)))
        .filter(|(_, s)| s.season > 0 || current.season == 0)
        .collect();
    episodes.sort_by_key(|(_, s)| (s.season, s.episode));
    episodes
        .into_iter()
        .find(|(v, s)| {
            (s.season, s.episode) > (current.season, current.episode)
                && v.released.is_none_or(|r| r.timestamp_millis() <= now)
        })
        .map(|(v, s)| NextEpisode {
            video_id: v.id.clone(),
            title: v.title.clone(),
            season: s.season,
            episode: s.episode,
            thumbnail: v.thumbnail.clone(),
        })
}

/// Keeps the stream that started playing for this title, so Versions for its
/// next episode prefers the same release line.
fn remember_stream(core: &Core, target: &PlayTarget, stream: &ParsedStream) {
    let memory = serde_json::json!({
        "key": stream.key,
        "addonId": stream.addon_id,
        "release": stream.filename.as_deref().unwrap_or(&stream.raw_name),
    });
    if let Err(e) = core.library().set_stream_memory(
        core.profile(),
        &target.item_id,
        stream.binge_group.as_deref(),
        &memory.to_string(),
        core.clock(),
    ) {
        tracing::warn!("stream memory write failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(json: &str) -> ParsedStream {
        let body = format!(r#"{{"streams":[{json}]}}"#);
        let wire = mq_protocol::decode_streams(body.as_bytes()).unwrap();
        mq_streams::normalize(&wire.streams[0], "a", "A", 0, 0)
    }

    #[test]
    fn debrid_requests_follow_the_stream() {
        let resolve = stream(
            r#"{"clientResolve":{"type":"debrid","service":"torbox","infoHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","fileIdx":3,"filename":"Show.S01E02.mkv","episode":2}}"#,
        );
        let (service, req) = debrid_request(&resolve, Some(1), Some(9)).unwrap();
        assert_eq!(service.as_deref(), Some("torbox"));
        assert_eq!(req.file_idx, Some(3));
        assert_eq!(req.filename_hint.as_deref(), Some("Show.S01E02.mkv"));
        // The addon's own episode wins; the season falls back to the target.
        assert_eq!((req.season, req.episode), (Some(1), Some(2)));

        let torrent = stream(
            r#"{"infoHash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","fileIdx":1,"behaviorHints":{"service":"realdebrid"}}"#,
        );
        let (service, req) = debrid_request(&torrent, Some(4), Some(5)).unwrap();
        assert_eq!(service.as_deref(), Some("realdebrid"));
        assert_eq!(req.info_hash, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!((req.season, req.episode), (Some(4), Some(5)));

        assert!(
            debrid_request(&stream(r#"{"url":"https://x.example/a.mkv"}"#), None, None).is_none()
        );
        let no_hash = stream(r#"{"clientResolve":{"type":"debrid","service":"torbox"}}"#);
        assert!(debrid_request(&no_hash, None, None).is_none());
    }

    #[tokio::test]
    async fn a_stale_resolve_never_replaces_a_newer_one() {
        let slot = Mutex::new(None);
        let task = || tokio::spawn(std::future::pending::<()>());
        let (older, newer, newest) = (task(), task(), task());
        let ends = [&older, &newer, &newest].map(|t| t.abort_handle());
        // Caller B installs run 2 before caller A, paused after spawning,
        // installs run 1.
        install(&slot, 2, newer);
        install(&slot, 1, older);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            ends.each_ref().map(|t| t.is_finished()),
            [true, false, false]
        );
        // A newer run still replaces it.
        install(&slot, 3, newest);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            ends.each_ref().map(|t| t.is_finished()),
            [true, true, false]
        );
    }

    #[tokio::test]
    async fn a_late_lease_is_never_adopted() {
        let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
        let server = mq_server::StreamServer::start(http).await.unwrap();
        let lease = || {
            let upstream = Upstream::Http {
                url: "http://127.0.0.1:9/v.mkv".into(),
                headers: Vec::new(),
                addon_url: None,
            };
            server.register(upstream, "v.mkv")
        };
        let mut s = State::new(Vec::new(), None);
        // A failover moved the view on while run 1 was opening.
        s.run = 2;
        assert!(!adopt(&mut s, 1, lease()), "a run that moved on");
        assert_eq!(server.playbacks(), 0);
        assert!(adopt(&mut s, 2, lease()));
        assert_eq!(server.playbacks(), 1);
        // The view closed while run 2's task still ran.
        s.closed = true;
        release_all(&mut s);
        assert!(!adopt(&mut s, 2, lease()), "a closed view");
        assert_eq!(server.playbacks(), 0);
        assert_eq!(s.snapshot.url, None);
    }

    /// A resolve that runs on after its view closed (one the abort did not
    /// reach in time) takes no other candidate.
    #[tokio::test]
    async fn a_closed_view_opens_no_source() {
        let core = Core::temporary("default").unwrap();
        let mut s = State::new(vec![stream(r#"{"url":"http://127.0.0.1:9/v.mkv"}"#)], None);
        s.closed = true;
        let snapshot = SharedObservable::new(s.snapshot.clone());
        let state = Arc::new(Mutex::new(s));
        let target = Arc::new(PlayTarget::default());
        resolve_next(core, target, Arc::from([]), state.clone(), snapshot, 0).await;
        let s = lock(&state);
        assert_eq!(s.next, 0, "no candidate taken");
        assert_eq!(s.snapshot.phase, PlaybackPhase::Resolving);
    }
}
