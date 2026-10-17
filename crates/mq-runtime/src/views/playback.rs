//! Playback: turns one picked version into a loopback URL the player opens,
//! falls over to the same release from another source when one fails, and
//! saves watch progress.
//!
//! The candidates are the picked Versions row followed by its failover chain
//! (the same release from other addons or services).

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::{AddonEvent, Deadlines};
use mq_debrid::{ResolveRequest, Resolver, StoreId};
use mq_http::{Priority, RequestOptions};
use mq_library::{FINISHED_RATIO, LibraryItemRow, ProgressRow};
use mq_media::{ByteCache, HttpRangeReader};
use mq_protocol::{ResourcePath, Source, decode_subtitles};
use mq_server::{StreamServer, Upstream};
use mq_streams::ParsedStream;
use serde::Serialize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use stremio_core::types::addon::ExtraValue;
use tokio::task::JoinHandle;

/// Progress is written at most this often while playing.
const SAVE_EVERY: Duration = Duration::from_secs(5);
/// A source must answer its first byte within this to count as alive.
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// The subtitle hash reads two 64 KiB ranges; past this, ask without it.
const HASH_TIMEOUT: Duration = Duration::from_secs(4);
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
    server: Option<Arc<StreamServer>>,
    last: Option<(u64, u64)>,
    saved_at: Option<Instant>,
    dirty: bool,
}

pub struct PlaybackView {
    core: Core,
    target: Arc<PlayTarget>,
    snapshot: SharedObservable<PlaybackSnapshot>,
    state: Arc<Mutex<State>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl PlaybackView {
    pub fn open(core: &Core, candidates: Vec<ParsedStream>, target: PlayTarget) -> Self {
        let resume_ms = core
            .library()
            .progress(core.profile(), &target.video_id)
            .ok()
            .flatten()
            .filter(|&(pos, dur)| {
                pos > 0 && (dur == 0 || (pos as f64) < dur as f64 * FINISHED_RATIO)
            })
            .map(|(pos, _)| pos as u64);
        let snapshot = PlaybackSnapshot {
            phase: PlaybackPhase::Resolving,
            url: None,
            key: None,
            filename: None,
            size_bytes: None,
            resume_ms,
            attempts: Vec::new(),
            subtitles: Vec::new(),
            subtitles_pending: false,
            next: None,
            binge_group: None,
        };
        let view = Self {
            core: core.clone(),
            target: Arc::new(target),
            snapshot: SharedObservable::new(snapshot.clone()),
            state: Arc::new(Mutex::new(State {
                snapshot,
                candidates,
                next: 0,
                server: None,
                last: None,
                saved_at: None,
                dirty: false,
            })),
            task: Mutex::new(None),
        };
        view.spawn_resolve();
        view
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

    /// The player could not open or keep playing the current source: drop it
    /// and move to the next candidate.
    pub fn fail_over(&self, error: &str) {
        {
            let mut s = lock(&self.state);
            if s.snapshot.phase != PlaybackPhase::Ready {
                return;
            }
            release_all(&mut s);
            if let Some(stream) = s.next.checked_sub(1).and_then(|i| s.candidates.get(i)) {
                let attempt = attempt(stream, error);
                s.snapshot.attempts.push(attempt);
            }
            s.snapshot.phase = PlaybackPhase::Resolving;
            s.snapshot.key = None;
            s.snapshot.subtitles_pending = false;
            self.snapshot.set(s.snapshot.clone());
        }
        self.spawn_resolve();
    }

    /// Player position report. Saved at most every few seconds; the last
    /// report is always saved when the view closes.
    pub fn report_progress(&self, position_ms: u64, duration_ms: u64) {
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
        if let Err(e) = save_progress(&self.core, &self.target, position_ms, duration_ms, first) {
            tracing::warn!("progress save failed: {e}");
        }
    }

    fn spawn_resolve(&self) {
        let task = tokio::spawn(resolve_next(
            self.core.clone(),
            self.target.clone(),
            self.state.clone(),
            self.snapshot.clone(),
        ));
        if let Some(old) = lock(&self.task).replace(task) {
            old.abort();
        }
    }
}

impl Drop for PlaybackView {
    fn drop(&mut self) {
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        self.save();
        release_all(&mut lock(&self.state));
    }
}

/// Ends the stream's and the subtitles' loopback URLs; they stop working.
fn release_all(s: &mut State) {
    let url = s.snapshot.url.take();
    let subtitles = std::mem::take(&mut s.snapshot.subtitles);
    if let Some(server) = &s.server {
        for url in url.iter().chain(subtitles.iter().map(|t| &t.url)) {
            server.release(url);
        }
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

fn save_progress(
    core: &Core,
    target: &PlayTarget,
    position_ms: u64,
    duration_ms: u64,
    first: bool,
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
        &core.hlc(),
    )?;
    if duration_ms > 0 && position_ms as f64 >= duration_ms as f64 * FINISHED_RATIO {
        library.set_watched(
            profile,
            &target.item_id,
            &target.video_id,
            true,
            &core.hlc(),
        )?;
    }
    Ok(())
}

/// Tries candidates from `state.next` until one answers, then publishes its
/// loopback URL; publishes `Failed` when none is left.
async fn resolve_next(
    core: Core,
    target: Arc<PlayTarget>,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<PlaybackSnapshot>,
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
            s.server = Some(server.clone());
            let Some(stream) = s.candidates.get(s.next).cloned() else {
                s.snapshot.phase = PlaybackPhase::Failed;
                snapshot.set(s.snapshot.clone());
                return;
            };
            s.next += 1;
            stream
        };
        match open_source(&core, &stream, &target).await {
            Ok((upstream, filename, size)) => {
                let name = filename
                    .clone()
                    .unwrap_or_else(|| format!("{}.mkv", target.video_id));
                let url = server.register(upstream.clone(), &name);
                let wanted = core.prefs().subtitle_languages;
                let allow_private = addon_allows_private(&core, &stream.addon_id);
                {
                    let mut s = lock(&state);
                    s.snapshot.phase = PlaybackPhase::Ready;
                    s.snapshot.url = Some(url);
                    s.snapshot.key = Some(stream.key.clone());
                    s.snapshot.filename = filename.clone();
                    s.snapshot.size_bytes = size.or(stream.size_bytes);
                    s.snapshot.subtitles = stream
                        .subtitles
                        .iter()
                        .map(|w| track(w, &stream.addon_name, &server, allow_private))
                        .collect();
                    order_subtitles(&mut s.snapshot.subtitles, &wanted);
                    s.snapshot.subtitles_pending = true;
                    s.snapshot.binge_group = stream.binge_group.clone();
                    snapshot.set(s.snapshot.clone());
                }
                remember_stream(&core, &target, &stream);
                let subtitles = find_subtitles(
                    &core,
                    &server,
                    &target,
                    &upstream,
                    filename.as_deref(),
                    &state,
                    &snapshot,
                );
                let next = async {
                    if let Some(next) = next_episode(&core, &target).await {
                        let mut s = lock(&state);
                        s.snapshot.next = Some(next);
                        snapshot.set(s.snapshot.clone());
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

/// Whether the addon that listed a stream may reach private addresses (a
/// local addon); its stream and subtitle URLs get the same allowance.
fn addon_allows_private(core: &Core, addon_id: &str) -> bool {
    core.installed_addons()
        .unwrap_or_default()
        .iter()
        .any(|a| a.id() == addon_id && a.allow_private)
}

/// Resolves one stream to an upstream and checks that it answers.
async fn open_source(
    core: &Core,
    stream: &ParsedStream,
    target: &PlayTarget,
) -> Result<Opened, String> {
    let allow_private = addon_allows_private(core, &stream.addon_id);
    let (url, headers, filename, size) = match &stream.source {
        Source::Http { url } => (
            url.to_string(),
            stream.headers.clone(),
            stream.filename.clone(),
            stream.size_bytes,
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
            return open_torrent(core, stream, target, info_hash, *file_idx, trackers).await;
            #[cfg(not(feature = "torrent"))]
            {
                let _ = (info_hash, file_idx, trackers);
                return Err("torrents need a debrid account in this version".into());
            }
        }
        Source::ClientResolve(_) | Source::Torrent { .. } => {
            let (service, request) = debrid_request(stream, target.season, target.episode)
                .ok_or("the addon did not say which torrent to resolve")?;
            let resolver = core
                .debrid()
                .ok_or("this version needs a debrid account; add one in Settings")?;
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
                return open_torrent(core, stream, target, info_hash, *file_idx, trackers)
                    .await
                    .map_err(|p2p| format!("{e}; peer to peer: {p2p}"));
            }
            let link = resolved?;
            (link.url, Vec::new(), Some(link.filename), link.size)
        }
        other => return Err(format!("{} streams cannot play here", source_word(other))),
    };
    let options = RequestOptions {
        priority: Priority::Playback,
        timeout: PROBE_TIMEOUT,
        allow_private,
        headers: headers.clone(),
        ..RequestOptions::default()
    };
    // Only the status line is read: origins that ignore Range answer 200
    // with the whole file, and that still counts as alive.
    let probe = core.http().stream(&url, Some("bytes=0-0".into()), &options);
    let status = tokio::time::timeout(PROBE_TIMEOUT, probe)
        .await
        .map_err(|_| "source did not answer in time".to_owned())?
        .map_err(|e| format!("source did not answer: {e}"))?
        .status;
    if !(200..300).contains(&status) {
        return Err(format!("source did not answer: http status {status}"));
    }
    Ok((
        Upstream::Http {
            url,
            headers,
            allow_private,
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
    info_hash: &str,
    file_idx: Option<u32>,
    trackers: &[String],
) -> Result<Opened, String> {
    use mq_engine::FileHint;
    let engine = core.torrent_engine().await.map_err(|e| e.to_string())?;
    let torrent = tokio::time::timeout(
        METADATA_TIMEOUT,
        engine.add_magnet(info_hash, trackers, &[]),
    )
    .await
    .map_err(|_| "no peer sent the torrent's metadata in time".to_owned())?
    .map_err(|e| e.to_string())?;
    let hint = match (file_idx, &stream.filename, target.season, target.episode) {
        (Some(i), ..) => FileHint::Index(i as usize),
        (None, Some(name), ..) => FileHint::Name(name.clone()),
        (None, None, Some(season), Some(episode)) => FileHint::Episode { season, episode },
        _ => FileHint::Largest,
    };
    let started = async {
        let file = engine.select_file(torrent, &hint).await?;
        let reader = engine.open_reader(torrent, file).await?;
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
            let upstream = Upstream::Torrent {
                engine,
                torrent,
                file,
            };
            return Ok((upstream, Some(name), Some(info.len)));
        }
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
/// format and charset the addon's file has.
fn track(
    w: &mq_protocol::WireSubtitle,
    source: &str,
    server: &StreamServer,
    allow_private: bool,
) -> SubtitleTrack {
    SubtitleTrack {
        url: server.register_subtitle(w.url.as_str(), &w.lang, allow_private),
        lang: w.lang.clone(),
        label: w.label.clone(),
        source: source.to_owned(),
        forced: w.forced,
        hearing_impaired: w.hearing_impaired,
    }
}

/// Asks every subtitles addon for this video with the OpenSubtitles hash, size
/// and filename when they can be had (hash: first and last 64 KiB), and adds
/// tracks to the snapshot as each addon answers.
async fn find_subtitles(
    core: &Core,
    server: &StreamServer,
    target: &PlayTarget,
    upstream: &Upstream,
    filename: Option<&str>,
    state: &Mutex<State>,
    snapshot: &SharedObservable<PlaybackSnapshot>,
) {
    let reader: Box<dyn mq_media::RangeReader> = match upstream {
        Upstream::Http {
            url,
            headers,
            allow_private,
        } => Box::new(
            HttpRangeReader::new(
                core.http().clone(),
                url,
                headers.clone(),
                Arc::new(ByteCache::new(512 * 1024)),
            )
            .allow_private(*allow_private),
        ),
        #[cfg(feature = "torrent")]
        Upstream::Torrent {
            engine,
            torrent,
            file,
        } => match engine.open_reader(*torrent, *file).await {
            Ok(reader) => Box::new(TorrentBytes(reader)),
            Err(e) => {
                tracing::warn!("torrent reader for the subtitle hash: {e}");
                return;
            }
        },
    };
    let mut extra = Vec::new();
    // Origins that ignore Range give no hash; filename and id still match.
    if let Ok(Ok((hash, size))) =
        tokio::time::timeout(HASH_TIMEOUT, mq_media::opensubtitles_hash(&*reader)).await
    {
        extra.push(extra_value("videoHash", hash));
        extra.push(extra_value("videoSize", size.to_string()));
    }
    if let Some(name) = filename {
        extra.push(extra_value("filename", name.to_owned()));
    }
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
        match event {
            AddonEvent::Arrived { addon_id, body, .. } => {
                let addon = addons.iter().find(|a| a.id() == addon_id);
                let source = addon.map_or(addon_id.clone(), |a| a.manifest.name.clone());
                let private = addon.is_some_and(|a| a.allow_private);
                let found = decode_subtitles(&body)
                    .map(|r| r.subtitles)
                    .unwrap_or_default();
                for w in &found {
                    // One file, one loopback address: compare by it.
                    let t = track(w, &source, server, private);
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
    if s.snapshot.subtitles_pending {
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
        &core.hlc(),
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
}
