//! Playback: turns one picked version into a loopback URL the player opens,
//! falls over to the same release from another source when one fails, and
//! saves watch progress.
//!
//! The candidates are the picked Versions row followed by its failover chain
//! (the same release from other addons or services).

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::segments::{self, Segment, Target};
use crate::turkcealtyazi::{self, NAME as BUILT_IN};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::{AddonEvent, Deadlines, InstalledAddon};
use mq_debrid::{ResolveRequest, Resolver, StoreId};
use mq_library::{Authorization, FINISHED_RATIO, LibraryItemRow, Policy, ProgressRow};
use mq_protocol::{ResourcePath, Source, decode_subtitles};
use mq_server::{Format, Lease, SubtitleSlot, SubtitleTrouble, Upstream};
use mq_streams::ParsedStream;
use serde::Serialize;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, Weak};
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
/// ...but a swarm that sends almost nothing is cut early when another
/// candidate can play instead (plans/m13-stream-choice.md 7, dead swarms).
/// Measured: a healthy bare-hash swarm found and unchoked its peers for
/// most of its first 19 s (Android, bench/c27/android-bare-lto.jsonl), and
/// a Torrentio swarm with 22 listed seeders sent nothing for 150 s after
/// its metadata (the Apple walk).
#[cfg(any(test, feature = "torrent"))]
const STALL: Stall = Stall {
    first: Duration::from_secs(30),
    later: Duration::from_secs(15),
    floor: 256 << 10,
};
/// A capacity sample is read this long after its source started
/// (plans/m13-stream-choice.md 5).
const SAMPLE_AFTER: Duration = Duration::from_secs(30);

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
    /// Subtitle sources that gave nothing this time, each once, shown under
    /// the subtitle list (plans/m13-turkcealtyazi.md 4.2).
    pub subtitle_notes: Vec<SubtitleNote>,
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
    /// As the addon gives it, usually ISO 639-2 ("eng"), sometimes an
    /// OpenSubtitles special ("pob").
    pub lang: String,
    /// The language as one tag for every spelling of it: ISO 639-1 where
    /// there is one, with its region or script ("en", "pt-BR", "zh-Hant",
    /// "es-419"); tracks of one tag sit together in `subtitles`.
    pub language: String,
    /// The language's English name ("Portuguese (Brazil)"): the heading
    /// of its group.
    pub language_name: String,
    /// What tells this track from the others of its language: the release
    /// it was made for when the addon names it, else its source; numbered
    /// when two would read the same.
    pub name: String,
    pub label: Option<String>,
    /// Addon name.
    pub source: String,
    pub forced: bool,
    pub hearing_impaired: bool,
    /// The release text `name` starts from, when there is one.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub(crate) release: Option<String>,
}

impl SubtitleTrack {
    /// A track in `lang` from `source`, its language read and its name set
    /// to the release text, else the source, until `reorder` tells
    /// duplicates apart.
    fn new(
        url: String,
        lang: &str,
        label: Option<String>,
        source: &str,
        release: Option<String>,
    ) -> Self {
        let language = crate::languages::language(lang);
        // A label that only names the language says nothing more.
        let release = release.map(|r| r.trim().to_owned()).filter(|r| {
            !r.is_empty()
                && !r.eq_ignore_ascii_case(&language.name)
                && !r.eq_ignore_ascii_case(lang)
        });
        Self {
            url,
            lang: lang.to_owned(),
            language: language.tag,
            name: release.clone().unwrap_or_else(|| source.to_owned()),
            language_name: language.name,
            label,
            source: source.to_owned(),
            forced: false,
            hearing_impaired: false,
            release,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleNote {
    /// As `SubtitleTrack::source`: the addon name.
    pub source: String,
    pub problem: SubtitleProblem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleProblem {
    /// The source did not answer; its tracks stay and may load later.
    Unreachable,
    /// The source turns requests away for a few minutes.
    Blocked,
    /// The source's pages changed and cannot be read until an update.
    Changed,
    /// A file of the source could not be read; its track left the list.
    TrackFailed,
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
    /// The Versions pick this playback came from: a failure is reported
    /// to it (plans/m13-stream-choice.md 3.9).
    pick: Option<crate::views::versions::PickHandle>,
    /// "Try again" was asked: each torrent gets the full start deadline,
    /// never cut early as a dead swarm.
    patient: bool,
    /// Dead swarms waiting for proof the connection works (a peer or a
    /// source reached in this chain), so an outage marks none of them.
    dead: Dead,
    /// The HTTP capacity sample was taken (once per view: at 30 s, or when
    /// the view closes first, as a short trailer's does).
    sampled: bool,
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
    /// Each subtitle track's place, by URL: its source's position in the
    /// installed list (the stream's own first), then its rank there. Off
    /// the wire; `subtitles` is sorted by it within a language.
    order: HashMap<String, (usize, usize)>,
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
                subtitle_notes: Vec::new(),
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
            pick: None,
            patient: false,
            dead: Dead::default(),
            sampled: false,
            last: None,
            saved_at: None,
            dirty: false,
            source: 0,
            lookup: None,
            segments_task: None,
            order: HashMap::new(),
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
        Self::open_with(core, candidates, None, target, plays)
    }

    fn open_with(
        core: &Core,
        candidates: Vec<ParsedStream>,
        pick: Option<crate::views::versions::PickHandle>,
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
        let mut state = State::new(candidates, resume_ms);
        state.pick = pick;
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

    /// `open_for` the row the user picked in Versions: a source of it that
    /// fails is reported back to the pick.
    pub fn open_picked(
        core: &Core,
        pick: crate::views::versions::Pick,
        target: PlayTarget,
        plays: &[Format],
    ) -> Self {
        Self::open_with(core, pick.candidates, Some(pick.handle), target, plays)
    }

    /// "Try again" once every source failed: the same versions again from
    /// the first, each torrent with the full start deadline, so a swarm
    /// that is only slow to start gets its chance. Ignored unless the
    /// playback failed.
    pub fn retry(&self) {
        {
            let mut s = lock(&self.state);
            if s.snapshot.phase != PlaybackPhase::Failed || s.closed {
                return;
            }
            s.next = 0;
            s.patient = true;
            s.snapshot.phase = PlaybackPhase::Resolving;
            s.snapshot.attempts.clear();
            self.snapshot.set(s.snapshot.clone());
        }
        self.spawn_resolve();
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
            if let Some(pick) = &s.pick {
                pick.failed();
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
        let sample = {
            let mut s = lock(&self.state);
            let sample = take_http_sample(&mut s);
            s.closed = true;
            release_all(&mut s);
            sample
        };
        if let Some(sample) = sample {
            record_http_sample(&self.core, sample);
        }
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
    s.snapshot.subtitle_notes.clear();
    s.order.clear();
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

/// Swarms that failed to start in one playback's chain, remembered as dead
/// only once something in the chain proved the connection works (code
/// review r1 P2-5, P3): a swarm that unchoked us and sent nothing proves
/// it itself; a metadata timeout waits for another candidate's peer or
/// source.
#[derive(Debug, Default)]
struct Dead {
    reached: bool,
    pending: Vec<String>,
}

impl Dead {
    /// `hash` did not start; `proven`: a peer of its own unchoked us.
    /// The hashes to remember now.
    #[cfg_attr(not(feature = "torrent"), allow(dead_code))]
    fn failed(&mut self, hash: &str, proven: bool) -> Vec<String> {
        self.pending.push(hash.to_ascii_lowercase());
        if proven {
            self.reached = true;
        }
        self.flush()
    }

    /// Something in the chain reached a peer or a source.
    fn reached(&mut self) -> Vec<String> {
        self.reached = true;
        self.flush()
    }

    fn flush(&mut self) -> Vec<String> {
        if self.reached {
            std::mem::take(&mut self.pending)
        } else {
            Vec::new()
        }
    }
}

/// Remembers `hashes` as stalled on this device for a day.
fn remember_dead(core: &Core, hashes: Vec<String>) {
    let now = mq_library::hlc::now_ms();
    for hash in hashes {
        crate::speed::record_dead(core.library(), &hash, now);
    }
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
    let hvc1_only = lock(&state).pick.as_ref().is_some_and(|p| p.hvc1_only);
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
                match server
                    .open_for(upstream.clone(), &name, PROBE_TIMEOUT, &plays)
                    .await
                {
                    Ok(lease) => shows_picture(&core, &stream, &lease, hvc1_only)
                        .await
                        .map(|()| (upstream, filename, size, lease)),
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        };
        match opened {
            Ok((upstream, filename, size, lease)) => {
                let wanted = preferred_languages(&core);
                // The stream's own tracks come from its addon.
                let addon_url = stream.addon_url.as_ref();
                let slot = lease.subtitles();
                listen_for_trouble(&slot, Arc::downgrade(&state), snapshot.clone(), run);
                let reader = lease.reader();
                let proven = {
                    let mut s = lock(&state);
                    if !adopt(&mut s, run, lease) {
                        return;
                    }
                    let proven = s.dead.reached();
                    if let Some(pick) = &s.pick {
                        pick.ready();
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
                    let own: Vec<String> =
                        s.snapshot.subtitles.iter().map(|t| t.url.clone()).collect();
                    for (rank, url) in own.into_iter().enumerate() {
                        s.order.insert(url, (0, rank));
                    }
                    reorder(&mut s, &wanted);
                    s.snapshot.subtitles_pending = true;
                    s.snapshot.binge_group = stream.binge_group.clone();
                    snapshot.set(s.snapshot.clone());
                    proven
                };
                remember_dead(&core, proven);
                if matches!(upstream, Upstream::Http { .. }) {
                    let attempt = lock(&state).snapshot.attempt;
                    spawn_http_sample(&core, &state, attempt);
                }
                remember_stream(&core, &target, &stream);
                // The addons ask with the hash; the built-in source lists at
                // once and ranks with the frame rate. Pending ends with both.
                let subtitles = async {
                    let installed = core.installed_addons().unwrap_or_default();
                    let reader = video_reader(&upstream, reader).await;
                    let reader = reader.as_deref();
                    let addons = async {
                        let extra = video_extra(reader, filename.as_deref()).await;
                        let found = (&slot, run, &*state, &snapshot);
                        find_subtitles(&core, &target, &installed, extra, found).await
                    };
                    let built_in = built_in_tracks(
                        &core, &target, &stream, &installed, reader, &slot, run, &state, &snapshot,
                    );
                    tokio::join!(addons, built_in);
                    let mut s = lock(&state);
                    if current(&s, run) && s.snapshot.subtitles_pending {
                        s.snapshot.subtitles_pending = false;
                        snapshot.set(s.snapshot.clone());
                    }
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
                if let Some(pick) = &s.pick {
                    pick.failed();
                }
                snapshot.set(s.snapshot.clone());
            }
        }
    }
}

/// On an engine that shows HEVC only from `hvc1` entries (Apple), a file
/// with an `hev1` entry that has no parameter sets plays as sound alone,
/// even relabelled: the version fails over, and the ranking remembers it.
async fn shows_picture(
    core: &Core,
    stream: &ParsedStream,
    lease: &Lease,
    hvc1_only: bool,
) -> Result<(), String> {
    if !hvc1_only || !lease.picture_missing_on_apple().await {
        return Ok(());
    }
    let now = mq_library::hlc::now_ms();
    crate::speed::record_no_picture(core.library(), &stream.key, now);
    Err("This file's video shows no picture on this device".into())
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
        } if by_engine(stream, core.debrid().is_some(), core.has_engine()) => {
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
    // Addresses the policy refused while this torrent tried to start, as
    // counts only (plans/c27-torrent-policy.md 3.8). The window is this
    // open's; another torrent opening at the same time adds its own.
    let refused_before = engine.refused();
    let did_not_start = || {
        let d = engine.refused().since(refused_before);
        tracing::info!(
            trackers = d.trackers,
            peers = d.peers,
            "torrent did not start"
        );
    };
    let torrent = tokio::time::timeout(
        METADATA_TIMEOUT,
        engine.add_magnet(info_hash, trackers, &[]),
    )
    .await
    .map_err(|_| {
        did_not_start();
        // Stale seeder counts fail this way: dead, once the chain shows
        // the connection works.
        let now = lock(state).dead.failed(info_hash, false);
        remember_dead(core, now);
        "No peer sent the torrent's details in time".to_owned()
    })?
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
    let by_episode = matches!(hint, FileHint::Episode { .. });
    let started = async {
        let file = engine.select_file(torrent, profile, &hint).await?;
        let reader = claim.open_reader(file).await?;
        let info = reader.file().clone();
        // The file the addon pointed at must be the one listed (3.4).
        if let Some(why) = wrong_file(&info.path, info.len, stream.size_bytes, target, by_episode) {
            return Ok((file, info, Started::Wrong(why)));
        }
        let mut head = reader.range(0, None).await?;
        // Received payload and the first unchoke (code review r1 P1): a
        // piece verifies only whole, long after its first blocks.
        let progress = || async {
            let s = engine.stats(torrent).await.ok()?;
            Some(Progress {
                received: s.received_bytes,
                unchoked: s.unchoked_ms.is_some(),
            })
        };
        // Cut early only when another candidate can play instead, and never
        // after "Try again".
        let others = {
            let s = lock(state);
            !s.patient && s.next < s.candidates.len()
        };
        let ready = watch_start(head.ready(), progress, STALL, others, TORRENT_READY_TIMEOUT).await;
        Ok::<_, mq_engine::EngineError>((file, info, ready))
    };
    let failed = match started.await {
        Ok((file, info, Started::Ready(Ok(())))) => {
            let name = info
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&info.path)
                .to_owned();
            spawn_sample(core, torrent, info_hash, stream.seeders);
            let upstream = Upstream::Torrent { claim, file };
            return Ok((upstream, Some(name), Some(info.len)));
        }
        // Held for this profile (its check is older than the hold): the
        // torrent, its hold and its files stay, since another profile may
        // be playing it, and the next candidate plays instead.
        Err(mq_engine::EngineError::Held) => return Err("This title is not on the list".into()),
        Ok((_, _, Started::Late { unchoked })) => {
            did_not_start();
            if unchoked {
                let now = lock(state).dead.reached();
                remember_dead(core, now);
            }
            "the swarm did not send the start of the video in time".to_owned()
        }
        Ok((_, _, Started::Stalled { nothing })) => {
            did_not_start();
            // Ranked last here for a day only when it sent nothing at all
            // in the full window it got (the listed seeders were wrong);
            // its unchoke proves the connection either way.
            let now = {
                let mut s = lock(state);
                if nothing {
                    s.dead.failed(info_hash, true)
                } else {
                    s.dead.reached()
                }
            };
            remember_dead(core, now);
            "the swarm sent almost nothing".to_owned()
        }
        Ok((_, _, Started::Wrong(why))) => why.to_owned(),
        Ok((_, _, Started::Ready(Err(e)))) | Err(e) => e.to_string(),
    };
    // The next candidate plays instead: stop downloading this one, unless
    // another playback still claims it (the engine then keeps it).
    drop(claim);
    let _ = engine.remove(torrent, false).await;
    Err(failed)
}

/// How a torrent's start went.
#[cfg(any(test, feature = "torrent"))]
#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
enum Started<T> {
    Ready(T),
    /// Verified bytes grew by less than the floor in a window, and another
    /// candidate can play; `nothing`: not one byte came in it.
    Stalled {
        nothing: bool,
    },
    /// The deadline passed; `unchoked`: a peer had unchoked us.
    Late {
        unchoked: bool,
    },
    /// The file is not the one the version listed.
    #[cfg_attr(test, allow(dead_code))]
    Wrong(&'static str),
}

/// How long a swarm may take to send each `floor` of verified bytes: the
/// first window, from its first connected peer, is longer, since a swarm
/// spends its first seconds finding and unchoking peers.
#[cfg(any(test, feature = "torrent"))]
#[derive(Clone, Copy)]
struct Stall {
    first: Duration,
    later: Duration,
    floor: u64,
}

/// What the engine says of a starting torrent, each second.
#[cfg(any(test, feature = "torrent"))]
#[derive(Clone, Copy)]
struct Progress {
    /// Payload received (code review r1 P1: a 16 MiB piece verifies long
    /// after its first blocks arrive).
    received: u64,
    /// A peer unchoked us.
    unchoked: bool,
}

/// Waits for `ready` while the swarm keeps sending: a window opens at its
/// first unchoke (the metadata is already in), and each `floor` of
/// received payload must come within it (`first`, then `later`). A swarm
/// that falls short is cut when `others` can play instead; otherwise, and
/// with no unchoke at all, it gets until `deadline`. Never cut while it
/// progresses.
#[cfg(any(test, feature = "torrent"))]
async fn watch_start<T, P, F>(
    ready: impl std::future::Future<Output = T>,
    mut progress: P,
    stall: Stall,
    others: bool,
    deadline: Duration,
) -> Started<T>
where
    P: FnMut() -> F,
    F: std::future::Future<Output = Option<Progress>>,
{
    let start = tokio::time::Instant::now();
    tokio::pin!(ready);
    let first = progress().await.map_or(0, |p| p.received);
    // The bytes and time the current window began with, and its length.
    let mut window: Option<(u64, tokio::time::Instant, Duration)> = None;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            out = &mut ready => return Started::Ready(out),
            _ = tick.tick() => {
                let now = tokio::time::Instant::now();
                if now - start >= deadline {
                    return Started::Late { unchoked: window.is_some() };
                }
                let Some(p) = progress().await else { continue };
                match window {
                    None if p.unchoked => window = Some((p.received, now, stall.first)),
                    None => {}
                    Some((mark, _, _)) if p.received >= mark.saturating_add(stall.floor) => {
                        window = Some((p.received, now, stall.later));
                    }
                    Some((_, since, length)) if others && now - since >= length => {
                        return Started::Stalled { nothing: p.received == first };
                    }
                    Some(_) => {}
                }
            }
        }
    }
}

/// Why the file a torrent opened is not the version listed (3.4): larger
/// than the listed size by more than 10% (smaller is a pack's file), or
/// named for another episode. `by_episode`: the file was found by the
/// episode already.
#[cfg(any(test, feature = "torrent"))]
fn wrong_file(
    path: &str,
    len: u64,
    listed: Option<u64>,
    target: &PlayTarget,
    by_episode: bool,
) -> Option<&'static str> {
    if listed.is_some_and(|l| len as f64 > l as f64 * 1.1) {
        return Some("the torrent's file is larger than the version listed");
    }
    if by_episode {
        return None;
    }
    let r = mq_relparse::parse(path.rsplit('/').next().unwrap_or(path));
    let other = |named: &[u32], want: Option<u32>| {
        want.is_some_and(|w| !named.is_empty() && !named.contains(&w))
    };
    // A name with no season numbers its episodes across the show (anime:
    // "One Piece - 1071"), which a per-season episode cannot judge (code
    // review r1 P2-3).
    let seasoned = !r.seasons.is_empty();
    (other(&r.seasons, target.season) || (seasoned && other(&r.episodes, target.episode)))
        .then_some("the torrent's file is another episode")
}

/// Reads the torrent's capacity sample once its first 30 s are over and
/// counts it into the estimate; a torrent removed by then (it failed over)
/// gives none. Once per torrent and network: a pack's next episode reads
/// the same first 30 s again (code review r1 P2-1).
#[cfg(feature = "torrent")]
fn spawn_sample(
    core: &Core,
    torrent: mq_engine::TorrentHandle,
    info_hash: &str,
    seeders: Option<u32>,
) {
    if !first_sample(info_hash, core.network()) {
        return;
    }
    let core = core.clone();
    tokio::spawn(async move {
        tokio::time::sleep(SAMPLE_AFTER).await;
        let Ok(engine) = core.torrent_engine().await else {
            return;
        };
        let Ok(stats) = engine.stats(torrent).await else {
            return;
        };
        let sample = crate::speed::Sample {
            kind: crate::speed::Kind::P2p,
            bytes: stats.sample_bytes,
            busy: Duration::from_millis(stats.sample_busy_ms),
            seeders,
        };
        let min = core.prefs().streams.min_seeders;
        crate::speed::record(
            core.library(),
            core.network(),
            &sample,
            min,
            mq_library::hlc::now_ms(),
        );
    });
}

/// Reads an HTTP playback's capacity sample once its first 30 s are over,
/// while it is still the playing source and the view lives, and counts it
/// into the estimate.
fn spawn_http_sample(core: &Core, state: &Arc<Mutex<State>>, attempt: u64) {
    let (core, state) = (core.clone(), Arc::downgrade(state));
    tokio::spawn(async move {
        tokio::time::sleep(SAMPLE_AFTER).await;
        let Some(state) = state.upgrade() else {
            return;
        };
        let sample = {
            let mut s = lock(&state);
            if s.closed || s.snapshot.attempt != attempt {
                return;
            }
            take_http_sample(&mut s)
        };
        if let Some(sample) = sample {
            record_http_sample(&core, sample);
        }
    });
}

/// The playing HTTP source's capacity sample, once per view.
fn take_http_sample(s: &mut State) -> Option<(u64, Duration)> {
    if s.sampled {
        return None;
    }
    let sample = s.lease.as_ref().and_then(Lease::sample)?;
    s.sampled = true;
    Some(sample)
}

fn record_http_sample(core: &Core, (bytes, busy): (u64, Duration)) {
    let sample = crate::speed::Sample {
        kind: crate::speed::Kind::Http,
        bytes,
        busy,
        seeders: None,
    };
    let min = core.prefs().streams.min_seeders;
    crate::speed::record(
        core.library(),
        core.network(),
        &sample,
        min,
        mq_library::hlc::now_ms(),
    );
}

/// True the first time this process samples `info_hash` on `network`.
// ponytail: per process; a restart may sample a torrent once more.
#[cfg(any(test, feature = "torrent"))]
fn first_sample(info_hash: &str, network: crate::speed::Network) -> bool {
    static SAMPLED: Mutex<Vec<(String, crate::speed::Network)>> = Mutex::new(Vec::new());
    let key = (info_hash.to_ascii_lowercase(), network);
    let mut sampled = lock(&SAMPLED);
    if sampled.contains(&key) {
        return false;
    }
    sampled.push(key);
    true
}

/// A torrent row plays through the engine when there is no account, or
/// when our cache check found the service does not have it: the ranking
/// scored it by torrent speed (3.2), and asking the service would add it
/// to the account (code review r1 P2-2).
fn by_engine(stream: &ParsedStream, account: bool, engine: bool) -> bool {
    matches!(stream.source, Source::Torrent { .. })
        && (!account || (engine && stream.cached == Some(false)))
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

/// Hears the subtitle route's failures for this run's tracks. Weak: the
/// state owns the lease, which owns this reporter.
fn listen_for_trouble(
    slot: &SubtitleSlot,
    state: Weak<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    run: u64,
) {
    slot.on_trouble(move |url, trouble| {
        let Some(state) = state.upgrade() else {
            return;
        };
        let mut s = lock(&state);
        if current(&s, run) && note_trouble(&mut s.snapshot, url, trouble) {
            snapshot.set(s.snapshot.clone());
        }
    });
}

/// The note for a track the route could not serve; a file that cannot be
/// read also leaves the list. False when nothing changed.
fn note_trouble(snapshot: &mut PlaybackSnapshot, url: &str, trouble: SubtitleTrouble) -> bool {
    let Some(at) = snapshot.subtitles.iter().position(|t| t.url == url) else {
        return false;
    };
    let (source, problem) = match trouble {
        SubtitleTrouble::Unreachable => (
            snapshot.subtitles[at].source.clone(),
            SubtitleProblem::Unreachable,
        ),
        SubtitleTrouble::Unreadable => (
            snapshot.subtitles.remove(at).source,
            SubtitleProblem::TrackFailed,
        ),
    };
    let removed = trouble == SubtitleTrouble::Unreadable;
    add_note(snapshot, source, problem) || removed
}

/// Adds the note once; false when it was there.
fn add_note(snapshot: &mut PlaybackSnapshot, source: String, problem: SubtitleProblem) -> bool {
    let note = SubtitleNote { source, problem };
    let new = !snapshot.subtitle_notes.contains(&note);
    if new {
        snapshot.subtitle_notes.push(note);
    }
    new
}

fn is_built_in(addon: &InstalledAddon) -> bool {
    turkcealtyazi::is_builtin(addon.transport_url.as_str(), Some(&addon.manifest.id))
}

/// The built-in TurkceAltyazi.org tracks (plans/m13-turkcealtyazi.md 3.8):
/// listed at once, ranked when the frame rate is read (the probe waits at
/// most 1.5 s), placed where its row sits among the addons. Once, however
/// many rows stand for it.
#[allow(clippy::too_many_arguments)]
async fn built_in_tracks(
    core: &Core,
    target: &PlayTarget,
    stream: &ParsedStream,
    installed: &[InstalledAddon],
    reader: Option<&dyn mq_media::RangeReader>,
    slot: &SubtitleSlot,
    run: u64,
    state: &Arc<Mutex<State>>,
    snapshot: &SharedObservable<PlaybackSnapshot>,
) {
    let Some(position) = installed.iter().position(is_built_in) else {
        return;
    };
    let (Some(digits), Some(want)) = (
        turkcealtyazi::digits(&target.item_id),
        turkcealtyazi::target(target),
    ) else {
        return;
    };
    let site = core.turkcealtyazi();
    let release = &stream.release;
    let mut playing = mq_turkcealtyazi::Playing {
        group: release.group.as_deref(),
        source: release.quality,
        cut: release.cut,
        fps: None,
    };
    let probe = async {
        let budget = mq_media::ProbeBudget::default();
        let found = mq_media::probe(reader?, budget, &mq_media::CancellationToken::new()).await;
        found.video?.fps.value
    };
    let listed = tokio::time::timeout(Deadlines::default().hard, async {
        // The request goes now; the ranking, from the cache, waits for the
        // frame rate.
        let (fps, _) = tokio::join!(probe, site.list(digits, want, &playing));
        playing.fps = fps.map(|fps| fps as f32);
        site.list(digits, want, &playing).await
    })
    .await;
    let listing = listed.unwrap_or(mq_turkcealtyazi::Listing::Problem(
        mq_turkcealtyazi::Problem::Unreachable,
    ));
    let wanted = preferred_languages(core);
    let owned = OwnedPlaying {
        group: playing.group.map(str::to_owned),
        source: playing.source,
        cut: playing.cut,
        fps: playing.fps,
    };
    let mut s = lock(state);
    if !current(&s, run) {
        return;
    }
    match listing {
        mq_turkcealtyazi::Listing::Tracks(tracks) => {
            for (rank, found) in tracks.into_iter().enumerate() {
                let url_cell = Arc::new(OnceLock::new());
                let fetch = BuiltInFetch {
                    site: site.clone(),
                    row: found.row.clone(),
                    target: want,
                    playing: owned.clone(),
                    url: url_cell.clone(),
                    state: Arc::downgrade(state),
                    snapshot: snapshot.clone(),
                    run,
                };
                let key = format!("turkcealtyazi:{}", found.row.altid);
                let Some(url) = slot.add_fetched(&key, "tur", fetch.into_fetch()) else {
                    return;
                };
                let _ = url_cell.set(url.clone());
                s.order.insert(url.clone(), (position + 1, rank));
                if !s.snapshot.subtitles.iter().any(|known| known.url == url) {
                    let label = Some(found.label.clone());
                    s.snapshot.subtitles.push(SubtitleTrack {
                        hearing_impaired: found.row.hearing_impaired,
                        ..SubtitleTrack::new(url, "tur", label, BUILT_IN, Some(found.label))
                    });
                }
            }
        }
        mq_turkcealtyazi::Listing::Nothing => return,
        mq_turkcealtyazi::Listing::Problem(p) => {
            add_note(&mut s.snapshot, BUILT_IN.into(), problem(p));
        }
    }
    reorder(&mut s, &wanted);
    snapshot.set(s.snapshot.clone());
}

/// What the playing release says, owned by a track's fetch.
#[derive(Clone)]
struct OwnedPlaying {
    group: Option<String>,
    source: mq_relparse::Quality,
    cut: Option<mq_relparse::Cut>,
    fps: Option<f32>,
}

/// One built-in track's fetch, run by the subtitle route each time the
/// track loads. `state` is weak: the state owns the lease, which owns this.
#[derive(Clone)]
struct BuiltInFetch {
    site: Arc<mq_turkcealtyazi::Site>,
    row: mq_turkcealtyazi::Row,
    target: mq_turkcealtyazi::Target,
    playing: OwnedPlaying,
    /// The track's URL, known once the slot gave it.
    url: Arc<OnceLock<String>>,
    state: Weak<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    run: u64,
}

impl BuiltInFetch {
    fn into_fetch(self) -> mq_server::Fetch {
        let this = Arc::new(self);
        Arc::new(move || {
            let this = this.clone();
            Box::pin(async move { this.fetch().await })
        })
    }

    /// The file's bytes; else the note (and a faulty file leaves the list,
    /// for 6 h) and nothing.
    async fn fetch(&self) -> Option<Vec<u8>> {
        let playing = mq_turkcealtyazi::Playing {
            group: self.playing.group.as_deref(),
            source: self.playing.source,
            cut: self.playing.cut,
            fps: self.playing.fps,
        };
        let fetched = self.site.fetch(&self.row, self.target, &playing).await;
        if let mq_turkcealtyazi::Fetched::File(bytes) = fetched {
            return Some(bytes);
        }
        let state = self.state.upgrade()?;
        let mut s = lock(&state);
        if current(&s, self.run) && note_fetched(&mut s.snapshot, self.url.get(), &fetched) {
            self.snapshot.set(s.snapshot.clone());
        }
        None
    }
}

/// The note for a built-in fetch that gave no file: the site's problem
/// keeps the track; a fault of the file takes it off the list.
fn note_fetched(
    snapshot: &mut PlaybackSnapshot,
    url: Option<&String>,
    fetched: &mq_turkcealtyazi::Fetched,
) -> bool {
    use mq_turkcealtyazi::Fetched;
    let note = match fetched {
        Fetched::File(_) => return false,
        Fetched::Problem(p) => problem(*p),
        Fetched::Fault {
            downloads_changed: true,
            ..
        } => SubtitleProblem::Changed,
        Fetched::Fault { .. } => SubtitleProblem::TrackFailed,
    };
    let before = snapshot.subtitles.len();
    if let (Fetched::Fault { .. }, Some(url)) = (fetched, url) {
        snapshot.subtitles.retain(|t| &t.url != url);
    }
    let removed = snapshot.subtitles.len() != before;
    add_note(snapshot, BUILT_IN.into(), note) || removed
}

fn problem(p: mq_turkcealtyazi::Problem) -> SubtitleProblem {
    match p {
        mq_turkcealtyazi::Problem::Unreachable => SubtitleProblem::Unreachable,
        mq_turkcealtyazi::Problem::Blocked => SubtitleProblem::Blocked,
        mq_turkcealtyazi::Problem::Changed => SubtitleProblem::Changed,
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
    let url = slot.add(w.url.as_str(), &w.lang, addon_url)?;
    Some(SubtitleTrack {
        forced: w.forced,
        hearing_impaired: w.hearing_impaired,
        ..SubtitleTrack::new(url, &w.lang, w.label.clone(), source, release(w))
    })
}

/// The release a file was made for, as the addon names it: its label, or
/// OpenSubtitles' file name (without its extension) or release name.
fn release(w: &mq_protocol::WireSubtitle) -> Option<String> {
    let extra = |key: &str| {
        w.extra
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let file = extra("subtitleFileName").map(|f| {
        let stem = f
            .rsplit_once('.')
            .map_or(f, |(stem, ext)| if ext.len() <= 4 { stem } else { f });
        stem.to_owned()
    });
    let text = w
        .label
        .clone()
        .filter(|l| !l.trim().is_empty())
        .or(file)
        .or_else(|| extra("movieReleaseName").map(str::to_owned))?;
    // At most 80 characters: a line in a menu.
    Some(text.chars().take(80).collect())
}

/// The video's bytes in process, for the subtitle hash and the frame rate:
/// over the playback's own cache and cursors; none for passthrough.
async fn video_reader(
    upstream: &Upstream,
    reader: Option<Box<dyn mq_media::RangeReader>>,
) -> Option<Box<dyn mq_media::RangeReader>> {
    match upstream {
        Upstream::Http { .. } => reader,
        #[cfg(feature = "torrent")]
        Upstream::Torrent { claim, file } => match claim.open_reader(*file).await {
            Ok(reader) => Some(Box::new(TorrentBytes(reader)) as Box<dyn mq_media::RangeReader>),
            Err(e) => {
                tracing::warn!("torrent reader for the subtitle hash: {e}");
                None
            }
        },
    }
}

/// The OpenSubtitles hash and size when they can be had (first and last
/// 64 KiB, read in process for files), and the filename.
async fn video_extra(
    reader: Option<&dyn mq_media::RangeReader>,
    filename: Option<&str>,
) -> Vec<ExtraValue> {
    let mut extra = Vec::new();
    // Origins that ignore Range give no hash; filename and id still match.
    if let Some(reader) = reader
        && let Ok(Ok((hash, size))) =
            tokio::time::timeout(HASH_TIMEOUT, mq_media::opensubtitles_hash(reader)).await
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
/// The built-in source's rows are never asked (`built_in_tracks`).
async fn find_subtitles(
    core: &Core,
    target: &PlayTarget,
    installed: &[InstalledAddon],
    extra: Vec<ExtraValue>,
    (slot, run, state, snapshot): (
        &SubtitleSlot,
        u64,
        &Mutex<State>,
        &SharedObservable<PlaybackSnapshot>,
    ),
) {
    let path = ResourcePath {
        resource: "subtitles".into(),
        r#type: target.kind.clone(),
        id: target.video_id.clone(),
        extra,
    };
    let addons: Vec<_> = installed
        .iter()
        .filter(|a| a.supports(&path) && !is_built_in(a))
        .cloned()
        .collect();
    let wanted = preferred_languages(core);
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
                let position = installed
                    .iter()
                    .position(|a| a.transport_url == transport_url)
                    .map_or(usize::MAX, |at| at + 1);
                let found = decode_subtitles(&body)
                    .map(|r| r.subtitles)
                    .unwrap_or_default();
                for (rank, w) in found.iter().enumerate() {
                    // One file, one loopback address: compare by it. The
                    // fetch has the scope of the install that answered.
                    let Some(t) = track(w, &source, slot, Some(&transport_url)) else {
                        return;
                    };
                    s.order.entry(t.url.clone()).or_insert((position, rank));
                    if !s.snapshot.subtitles.iter().any(|known| known.url == t.url) {
                        s.snapshot.subtitles.push(t);
                    }
                }
                reorder(&mut s, &wanted);
            }
            _ => continue,
        }
        snapshot.set(s.snapshot.clone());
    }
}

fn extra_value(name: &str, value: String) -> ExtraValue {
    ExtraValue {
        name: name.into(),
        value,
    }
}

/// The profile's languages, most wanted first, as language tags: its
/// subtitle languages, then its audio languages.
fn preferred_languages(core: &Core) -> Vec<String> {
    let prefs = core.prefs();
    let mut tags: Vec<String> = Vec::new();
    for code in prefs
        .subtitle_languages
        .iter()
        .chain(&prefs.streams.languages)
    {
        let tag = crate::languages::language(code).tag;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// Tracks grouped by language: the preferred languages first in their
/// order (a regional one, "pt-BR", counts with its language), then the
/// others by English name. Within a language, by the source's place in the
/// installed list (the stream's own first), then the source's own rank,
/// whenever each answered. Then each track's `name`: two that would read
/// the same in one language are numbered in that order.
fn reorder(s: &mut State, preferred: &[String]) {
    let State {
        snapshot, order, ..
    } = s;
    let rank = |tag: &str| {
        let primary = crate::languages::primary(tag);
        preferred.iter().position(|p| p == tag).or_else(|| {
            preferred
                .iter()
                .position(|p| crate::languages::primary(p) == primary)
        })
    };
    snapshot.subtitles.sort_by_cached_key(|t| {
        (
            rank(&t.language).unwrap_or(usize::MAX),
            t.language_name.to_lowercase(),
            t.language.clone(),
            order.get(&t.url).copied().unwrap_or((usize::MAX, 0)),
        )
    });
    let base = |t: &SubtitleTrack| t.release.clone().unwrap_or_else(|| t.source.clone());
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut total: HashMap<(String, String), usize> = HashMap::new();
    for t in &snapshot.subtitles {
        *total.entry((t.language.clone(), base(t))).or_default() += 1;
    }
    for t in &mut snapshot.subtitles {
        let key = (t.language.clone(), base(t));
        let n = seen.entry(key.clone()).or_default();
        *n += 1;
        t.name = if total[&key] > 1 {
            format!("{} {}", key.1, n)
        } else {
            key.1
        };
    }
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

    /// Code review r1 P1: the stall rule reads received payload, so a
    /// swarm whose 16 MiB pieces verify late (the first at 20 s) while
    /// blocks arrive every second is never cut.
    #[tokio::test(start_paused = true)]
    async fn received_payload_not_verified_pieces_keeps_a_swarm_alive() {
        // `at` gives received bytes: 128 KiB a second from the unchoke.
        let (started, _) = watch(Some(Duration::from_secs(25)), |t| (t * 128 * KIB, 3), true).await;
        assert!(matches!(started, Started::Ready(())));
    }

    /// Code review r1 P2-5, P3: a metadata timeout is remembered once the
    /// chain proves the connection works, and an outage marks nothing.
    #[test]
    fn dead_swarms_wait_for_proof_the_connection_works() {
        let mut dead = Dead::default();
        assert!(dead.failed("AAAA", false).is_empty(), "no proof yet");
        assert!(dead.failed("bbbb", false).is_empty());
        assert_eq!(dead.reached(), ["aaaa", "bbbb"], "a source opened");
        assert_eq!(dead.failed("cccc", false), ["cccc"], "proof already in");
        let mut outage = Dead::default();
        assert!(outage.failed("dddd", false).is_empty());
        assert!(
            outage.pending.len() == 1 && !outage.reached,
            "dropped with the view"
        );
        let mut own = Dead::default();
        assert_eq!(
            own.failed("eeee", true),
            ["eeee"],
            "its own unchoke proves it"
        );
    }

    /// Code review r1 P2-1: one capacity sample per torrent and network.
    #[test]
    fn a_torrent_is_sampled_once_per_network() {
        use crate::speed::Network;
        let hash = "f".repeat(40);
        assert!(first_sample(&hash, Network::Wifi));
        assert!(!first_sample(&hash.to_uppercase(), Network::Wifi));
        assert!(first_sample(&hash, Network::Cellular));
    }

    /// Code review r1 P2-2: a torrent our check found uncached plays
    /// through the engine, never the service first.
    #[test]
    fn an_uncached_swarm_row_goes_to_the_engine() {
        let mut torrent = stream(&format!(r#"{{"infoHash":"{}"}}"#, "a".repeat(40)));
        assert!(by_engine(&torrent, false, true), "no account");
        assert!(
            !by_engine(&torrent, true, true),
            "unknown: the service first"
        );
        torrent.cached = Some(false);
        assert!(by_engine(&torrent, true, true));
        assert!(!by_engine(&torrent, true, false), "no engine: the service");
        torrent.cached = Some(true);
        assert!(!by_engine(&torrent, true, true));
    }

    /// "Try again" restarts the same versions from the first, patiently
    /// (no early cut), and only once the playback failed.
    #[tokio::test]
    async fn try_again_restarts_the_same_versions_patiently() {
        let core = Core::temporary("default").unwrap();
        // A loopback link from no install is refused at once.
        let dead = stream(r#"{"url":"http://127.0.0.1:9/v.mkv"}"#);
        let view = PlaybackView::open(&core, vec![dead], PlayTarget::default());
        view.retry();
        assert!(!lock(&view.state).patient, "not failed yet: ignored");
        let failed = || async {
            for _ in 0..3000 {
                if view.snapshot().phase == PlaybackPhase::Failed {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("never failed: {:?}", view.snapshot());
        };
        failed().await;
        assert_eq!(view.snapshot().attempts.len(), 1);
        view.retry();
        {
            let s = lock(&view.state);
            assert!(s.patient);
            assert_eq!(s.snapshot.phase, PlaybackPhase::Resolving);
            assert!(s.snapshot.attempts.is_empty());
        }
        failed().await;
        assert_eq!(
            view.snapshot().attempts.len(),
            1,
            "the same version tried again"
        );
    }

    /// The engine's verified bytes and connected peers, one per second
    /// (`at(t)` gives second `t`'s).
    fn fake(
        at: impl Fn(u64) -> (u64, usize),
    ) -> impl FnMut() -> std::future::Ready<Option<Progress>> {
        let mut t = 0;
        move || {
            let (bytes, peers) = at(t);
            t += 1;
            std::future::ready(Some(Progress {
                received: bytes,
                unchoked: peers > 0,
            }))
        }
    }

    const KIB: u64 = 1 << 10;
    const DEADLINE: Duration = Duration::from_secs(60);

    async fn watch(
        ready_after: Option<Duration>,
        at: impl Fn(u64) -> (u64, usize),
        others: bool,
    ) -> (Started<()>, Duration) {
        let begun = tokio::time::Instant::now();
        let ready = async move {
            match ready_after {
                Some(d) => tokio::time::sleep(d).await,
                None => std::future::pending::<()>().await,
            }
        };
        let started = watch_start(ready, fake(at), STALL, others, DEADLINE).await;
        (started, begun.elapsed())
    }

    /// A swarm with peers that sends nothing is cut once its first window
    /// (30 s from the first peer) ends, when another candidate can play,
    /// and is remembered: nothing came.
    #[tokio::test(start_paused = true)]
    async fn a_dead_swarm_is_cut_after_its_first_window() {
        let (started, took) = watch(None, |_| (0, 3), true).await;
        assert!(matches!(started, Started::Stalled { nothing: true }));
        assert!((30..33).contains(&took.as_secs()), "{took:?}");
    }

    /// A healthy swarm that is slow to start (the Android bare-hash runs
    /// spent most of their first 19 s finding peers): no peer for 15 s, the
    /// first bytes 18 s after it, Ready at 42 s. Never cut: its first window
    /// starts at its first peer and lasts 30 s.
    #[tokio::test(start_paused = true)]
    async fn a_slow_starting_healthy_swarm_is_never_cut() {
        let at = |t: u64| {
            let peers = if t < 15 { 0 } else { 4 };
            (t.saturating_sub(33) * 300 * KIB, peers)
        };
        let (started, took) = watch(Some(Duration::from_secs(42)), at, true).await;
        assert!(matches!(started, Started::Ready(())), "{took:?}");
    }

    /// With no other candidate a dead swarm gets the whole deadline; so does
    /// one where no peer ever connects.
    #[tokio::test(start_paused = true)]
    async fn the_deadline_holds_with_no_other_candidate_or_no_peer() {
        let (started, took) = watch(None, |_| (0, 3), false).await;
        assert!(matches!(started, Started::Late { .. }));
        assert!(took >= DEADLINE, "{took:?}");
        let (started, took) = watch(None, |_| (0, 0), true).await;
        assert!(matches!(started, Started::Late { .. }));
        assert!(took >= DEADLINE, "{took:?}");
    }

    /// After its first 256 KiB a swarm gets 15 s for each next 256 KiB: one
    /// that stops is cut, but not remembered, since it sent something.
    #[tokio::test(start_paused = true)]
    async fn a_swarm_that_stops_is_cut_but_not_remembered() {
        let (started, took) = watch(None, |t| ((t.min(6)) * 60 * KIB, 2), true).await;
        assert!(matches!(started, Started::Stalled { nothing: false }));
        assert!((19..23).contains(&took.as_secs()), "{took:?}");
    }

    /// A swarm that keeps sending is never cut early: it gets the deadline,
    /// and a ready head wins at once.
    #[tokio::test(start_paused = true)]
    async fn a_swarm_that_keeps_sending_is_never_cut_early() {
        let (started, _) = watch(None, |t| (t * 300 * KIB, 5), true).await;
        assert!(matches!(started, Started::Late { .. }));
        let (started, _) = watch(Some(Duration::from_secs(20)), |t| (t * 300 * KIB, 5), true).await;
        assert!(matches!(started, Started::Ready(())));
    }

    /// 3.4: the engine's file must be the version listed; only larger
    /// fails (a pack's file is smaller), and another episode fails.
    #[test]
    fn the_engines_file_must_be_the_version_listed() {
        let episode = PlayTarget {
            season: Some(1),
            episode: Some(5),
            ..PlayTarget::default()
        };
        let gb = 1u64 << 30;
        let file = "Show/Show.S01E05.1080p.WEB.mkv";
        assert_eq!(
            wrong_file(file, 2 * gb, Some(2 * gb), &episode, false),
            None
        );
        assert_eq!(
            wrong_file(file, 2 * gb, Some(40 * gb), &episode, false),
            None,
            "a pack's size listed"
        );
        assert!(
            wrong_file(file, 3 * gb, Some(2 * gb), &episode, false).is_some(),
            "larger"
        );
        // Code review r1 P2-3: an absolute episode number with no season
        // is not judged against the per-season episode.
        let anime = PlayTarget {
            season: Some(21),
            episode: Some(1),
            ..PlayTarget::default()
        };
        assert_eq!(
            wrong_file(
                "[SubsPlease] One Piece - 1071 (1080p).mkv",
                gb,
                Some(gb),
                &anime,
                false
            ),
            None
        );
        let other = "Show/Show.S01E06.1080p.WEB.mkv";
        assert!(wrong_file(other, gb, Some(gb), &episode, false).is_some());
        assert_eq!(
            wrong_file(other, gb, Some(gb), &episode, true),
            None,
            "found by the episode"
        );
        assert_eq!(
            wrong_file(
                "Movie.2020.1080p.mkv",
                gb,
                None,
                &PlayTarget::default(),
                false
            ),
            None
        );
    }
}
