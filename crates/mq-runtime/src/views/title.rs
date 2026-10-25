//! Title page: metadata from the first addon (in the user's order) that
//! answers for the id, episodes grouped by season with watched and progress
//! state from the library, and library and watched commands.

use crate::Core;
use crate::live::{FRAME, ListDiff, coalesce};
use crate::ratings::Ratings;
use crate::views::home::Tile;
use crate::views::playback::{aired, following};
use eyeball::SharedObservable;
use eyeball_im::ObservableVector;
use futures_util::{Stream, StreamExt};
use mq_library::{LibraryItemRow, Table};
use mq_protocol::MetaItem;
use serde::Serialize;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use stremio_core::types::resource::{SeriesInfo, StreamSource, Video};
use tokio::task::JoinSet;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct TitleDetails {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
    pub poster: Option<String>,
    /// RPDB's cover with the IMDb, Metacritic and Rotten Tomatoes band drawn
    /// in, as the title's tiles show it (`Tile::rated_cover`): for its IMDb
    /// id, or the one its addon gave an anime, with the RPDB key an
    /// installed, enabled addon was configured with. Null with no key or no
    /// IMDb id: show `poster` then, and `poster` too when this one fails to
    /// load.
    pub rated_poster: Option<String>,
    pub background: Option<String>,
    pub logo: Option<String>,
    pub release_info: Option<String>,
    pub runtime: Option<String>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    pub directors: Vec<String>,
    /// From the meta's `Writers` links, as `cast` and `directors`.
    pub writers: Vec<String>,
    /// The IMDb rating the addon gave, as written ("8.4").
    pub rating: Option<String>,
    /// The IMDb score to show, as on tiles.
    pub ratings: Ratings,
    /// The title's IMDb page, for the IMDb score to link to; null for an id
    /// that is not IMDb's, and on a kids profile (no way out to the web).
    pub imdb_url: Option<String>,
    /// TMDB's and Trakt's numbers, asked once the page is visible
    /// (`titleVisible`).
    pub scores: TitleScores,
    pub source: String,
    /// Trailers the metadata names (YouTube), first is the main one.
    pub trailers: Vec<Trailer>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Trailer {
    pub title: String,
    pub youtube_id: String,
    /// The watch page, for apps that hand trailers to a browser or the
    /// YouTube app.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "state", rename_all = "camelCase")]
/// One source's number on the title page (plans/title-page-ratings-genres-similar.md):
/// `pending` while an answer can still come (keep the slot's width),
/// `shown` with the number as written, `none` when there is none or no
/// call will be made (draw nothing, take no space).
pub enum Slot {
    Pending,
    Shown {
        text: String,
        /// The source's page for the title (phone, desktop and web link to
        /// it, never TV), built only from checked ids; null when there is
        /// none.
        url: Option<String>,
    },
    #[serde(rename = "none")]
    Empty,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
/// The title page's numbers beyond the addon's IMDb score, per source:
/// TMDB's user score and Trakt's community score, each as a percentage
/// ("87%", "92%"), shown only with at least 10 votes.
pub struct TitleScores {
    pub tmdb: Slot,
    pub trakt: Slot,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum TitleState {
    Loading,
    Ready,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct TitleHeader {
    pub state: TitleState,
    pub details: Option<TitleDetails>,
    pub in_library: bool,
    /// Add to Library can be offered: false on a kids profile, whose
    /// Library is the owner's list (`setInLibrary` is refused there). Hide
    /// the button when false.
    pub library_settable: bool,
    /// Genres open Discover: false on a kids profile, which has no Discover
    /// (plans/m11-kids.md 14.1). Show the genres as plain text when false.
    pub genres_linked: bool,
    /// Subtitles for the video Play plays (core backlog C34): what the
    /// subtitles addons list for it, for the CC and SDH badges. Null until
    /// they answered, and with no subtitles addon.
    pub captions: Option<Captions>,
    pub seasons: Vec<u32>,
    pub selected_season: Option<u32>,
    /// Episode (or the movie itself) the primary button resumes or starts.
    /// None while the next episode is not out yet (see `next_airs`).
    pub play_target: Option<PlayTarget>,
    /// The day the next episode comes out ("YYYY-MM-DD", as
    /// `Episode::released`) when it is not out yet: there is no play target,
    /// and the page says "Next episode airs <date>".
    pub next_airs: Option<String>,
    /// A movie the user watched, or a title with episodes (a series, an
    /// anime) whose every aired regular episode is watched (specials left
    /// out, as Trakt counts a show complete; an episode with no season
    /// counts; core backlog C41). Toggled with `setWatched` on the title's
    /// id: with episodes that marks or unmarks every such one.
    pub watched: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct PlayTarget {
    pub video_id: String,
    pub label: String,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub resume_ms: Option<i64>,
    /// What is left of the file, from the stored duration (the figure Continue Watching uses);
    /// null when nothing was played or the duration is unknown.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub left_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub video_id: String,
    pub season: u32,
    pub episode: u32,
    pub title: String,
    pub overview: Option<String>,
    pub thumbnail: Option<String>,
    pub released: Option<String>,
    pub watched: bool,
    pub progress: Option<f32>,
    /// As [`PlayTarget::left_ms`], for this episode.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub left_ms: Option<i64>,
}

/// What the subtitles addons list for one video.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Captions {
    /// Languages with at least one track, each once ("en" and "en-US"
    /// apart, as the player's menu groups them): 0 means none, no CC.
    pub languages: u32,
    /// A track is for the hard of hearing (the addon says so, or its file
    /// name: "SDH", ".HI."): the SDH badge.
    pub hearing_impaired: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TitleUpdate {
    Header {
        header: Box<TitleHeader>,
    },
    Episodes {
        diffs: Vec<ListDiff<Episode>>,
    },
    /// "More like this", sent whole when it lands.
    Similar {
        tiles: Vec<Tile>,
    },
}

struct State {
    meta: Option<MetaItem>,
    selected_season: Option<u32>,
    /// The subtitles addons' answer, for the video it was asked for.
    captions: Option<(String, Captions)>,
    /// TMDB's and Trakt's answers, once each landed.
    tmdb: Option<Slot>,
    trakt: Option<Slot>,
    /// RPDB's cover for the title, read when the meta lands and again when
    /// the addons change (the key comes from one), not on every render.
    rated_poster: Option<String>,
}

/// A live title page. Dropping it cancels its work.
pub struct TitleView {
    core: Core,
    kind: String,
    id: String,
    /// A kids profile (plans/m11-kids.md 4.6): no trailers.
    kids: bool,
    /// A kids policy refused the title: the view does no work.
    denied: bool,
    header: SharedObservable<TitleHeader>,
    episodes: Arc<Mutex<ObservableVector<Episode>>>,
    state: Arc<Mutex<State>>,
    similar: SharedObservable<Vec<Tile>>,
    /// The TMDB and Trakt lookup, alive only while the page is visible.
    lookup: Mutex<JoinSet<()>>,
    _tasks: JoinSet<()>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct TitleSnapshot {
    pub header: TitleHeader,
    pub episodes: Vec<Episode>,
    /// "More like this": empty until it lands, and for a kids profile.
    pub similar: Vec<Tile>,
}

impl TitleView {
    /// Opens the page. Under a kids policy a title off the list opens
    /// denied: no work, an empty snapshot, and `denied()` says so.
    pub fn open(core: &Core, kind: &str, id: &str) -> Self {
        let policy = core.policy();
        let header = SharedObservable::new(TitleHeader {
            state: TitleState::Loading,
            details: None,
            in_library: false,
            library_settable: !policy.kids(),
            genres_linked: !policy.kids(),
            captions: None,
            seasons: Vec::new(),
            selected_season: None,
            play_target: None,
            next_airs: None,
            watched: false,
        });
        let mut view = Self {
            core: core.clone(),
            kind: kind.to_owned(),
            id: id.to_owned(),
            kids: policy.kids(),
            denied: !policy.allows(kind, id),
            header,
            episodes: Arc::new(Mutex::new(ObservableVector::new())),
            state: Arc::new(Mutex::new(State {
                meta: None,
                selected_season: None,
                captions: None,
                tmdb: None,
                trakt: None,
                rated_poster: None,
            })),
            similar: SharedObservable::new(Vec::new()),
            lookup: Mutex::new(JoinSet::new()),
            _tasks: JoinSet::new(),
        };
        if !view.denied {
            view._tasks.spawn(load_meta(view.ctx()));
            view._tasks.spawn(watch_library(view.ctx()));
            view._tasks.spawn(watch_captions(view.ctx()));
            // Versions usually come next: their first scrape finds the
            // trackers resolved and connected.
            let core = core.clone();
            view._tasks.spawn(async move { core.warm_swarms().await });
        }
        view
    }

    /// A kids policy refused this title at open.
    pub fn denied(&self) -> bool {
        self.denied
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            core: self.core.clone(),
            kind: self.kind.clone(),
            id: self.id.clone(),
            kids: self.kids,
            header: self.header.clone(),
            episodes: self.episodes.clone(),
            state: self.state.clone(),
            similar: self.similar.clone(),
        }
    }

    pub fn snapshot(&self) -> TitleSnapshot {
        TitleSnapshot {
            header: self.header.get(),
            episodes: lock(&self.episodes).iter().cloned().collect(),
            similar: self.similar.get(),
        }
    }

    pub fn subscribe(
        &self,
    ) -> (
        TitleSnapshot,
        Pin<Box<dyn Stream<Item = Vec<TitleUpdate>> + Send>>,
    ) {
        let (values, episode_stream) = lock(&self.episodes)
            .subscribe()
            .into_values_and_batched_stream();
        let snapshot = TitleSnapshot {
            header: self.header.get(),
            episodes: values.into_iter().collect(),
            similar: self.similar.get(),
        };
        let episodes = episode_stream.map(|batch| {
            vec![TitleUpdate::Episodes {
                diffs: batch.into_iter().map(ListDiff::from).collect(),
            }]
        });
        let headers = self.header.subscribe().map(|header| {
            vec![TitleUpdate::Header {
                header: Box::new(header),
            }]
        });
        let similar = self
            .similar
            .subscribe()
            .map(|tiles| vec![TitleUpdate::Similar { tiles }]);
        let merged =
            futures_util::stream::select(futures_util::stream::select(episodes, headers), similar);
        (snapshot, Box::pin(coalesce(Box::pin(merged), FRAME)))
    }

    /// The page shows or stops showing (`titleVisible`): the apps send it
    /// on appearing and on leaving the foreground (the player over it, the
    /// app in the background). TMDB and Trakt are asked only after the page
    /// has been visible for `VISIBLE_FOR` without a break; hidden stops an
    /// unanswered lookup, and the next visible starts it again. Answers
    /// already in stay.
    pub fn set_visible(&self, visible: bool) {
        let mut lookup = lock(&self.lookup);
        while lookup.try_join_next().is_some() {}
        if !visible {
            // Dropping the set aborts its tasks and empties it at once: an
            // aborted task stays in a set until the runtime reaps it, so a
            // visible right after would see it and start nothing.
            *lookup = tokio::task::JoinSet::new();
        } else if lookup.is_empty() && !self.denied && !self.kids {
            // A repeated visible keeps the running wait and lookup.
            lookup.spawn(look_up(self.ctx()));
        }
    }

    pub fn select_season(&self, season: u32) {
        lock(&self.state).selected_season = Some(season);
        self.ctx().render();
    }

    /// Adds or removes the title from the library; a kids profile is
    /// refused (`KidsLibrary`): its Library is the owner's list.
    pub fn set_in_library(&self, in_library: bool) -> Result<(), crate::CoreError> {
        let Some(details) = self.header.get().details else {
            return Ok(());
        };
        let row = LibraryItemRow {
            item_id: details.id,
            kind: details.kind,
            name: details.name,
            poster: details.poster,
            poster_shape: None,
            added_ms: mq_library::hlc::now_ms() as i64,
        };
        self.core.set_in_library(&row, in_library)
    }

    /// Marks or unmarks by hand. With Trakt connected, a mark that flips
    /// queues the write for Trakt in the same transaction. A title's own id,
    /// when it has episodes, marks every aired regular episode (C41); before
    /// its meta is in, that is refused with `NotLoaded`.
    pub fn set_watched(&self, video_id: &str, watched: bool) -> Result<(), crate::CoreError> {
        // No watched row for a title off the kids list.
        if self.denied {
            return Ok(());
        }
        if self.kind != "movie" && video_id == self.id {
            let episodes = {
                let state = lock(&self.state);
                let meta = state.meta.as_ref().ok_or(crate::CoreError::NotLoaded)?;
                series_like(&self.kind, meta)
                    .then(|| aired_episodes(meta, mq_library::hlc::now_ms() as i64))
            };
            if let Some(episodes) = episodes {
                // ponytail: one transaction (and one Trakt queue row) per
                // episode, so a long anime costs hundreds; batch them in one
                // transaction if that is ever felt.
                for episode in episodes {
                    self.set_watched(&episode, watched)?;
                }
                return Ok(());
            }
        }
        let (library, profile) = (self.core.library(), self.core.profile());
        let account = library.tracker_account(profile, crate::trakt::TRACKER)?;
        let tracker = account.as_ref().map(|(account, _)| {
            let key = mq_library::TrackerKey {
                profile,
                tracker: crate::trakt::TRACKER,
                account,
            };
            (key, mq_library::hlc::now_ms() as i64)
        });
        library.set_watched_by_hand(
            profile,
            &self.id,
            video_id,
            watched,
            self.core.clock(),
            tracker,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct Ctx {
    core: Core,
    kind: String,
    id: String,
    kids: bool,
    header: SharedObservable<TitleHeader>,
    episodes: Arc<Mutex<ObservableVector<Episode>>>,
    state: Arc<Mutex<State>>,
    similar: SharedObservable<Vec<Tile>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// How long the page must stay visible before TMDB and Trakt are asked: a
/// glance while browsing asks nothing.
const VISIBLE_FOR: Duration = Duration::from_secs(1);
/// "More like this" shows at most this many titles.
const SIMILAR: usize = 20;

/// The kind TMDB and Trakt are asked as: a movie or a series as itself; an
/// anime by what TMDB says its IMDb id is (`None` here); other kinds not at
/// all.
enum Asked {
    As(&'static str),
    ByTmdb,
}

fn asked(kind: &str) -> Option<Asked> {
    match kind {
        "movie" => Some(Asked::As("movie")),
        "series" => Some(Asked::As("series")),
        "anime" => Some(Asked::ByTmdb),
        _ => None,
    }
}

/// A source's number as the page writes it, only with enough votes: a 0 to
/// 10 score as a whole percentage (8.74 is "87%", 8.75 "88%"); the
/// apps show the text as written and never round.
fn percent(score: Option<(f64, u64)>, url: Option<String>) -> Slot {
    match score {
        Some((score, votes)) if votes >= crate::trakt::MIN_VOTES => Slot::Shown {
            text: format!("{}%", (score * 10.0).round() as i64),
            url,
        },
        _ => Slot::Empty,
    }
}

/// TMDB's page for the title, from find's numeric id.
fn tmdb_url(found: &mq_tracking::tmdb::Found) -> String {
    let path = if found.kind == "movie" { "movie" } else { "tv" };
    format!("https://www.themoviedb.org/{path}/{}", found.id)
}

/// Trakt's page for the title, by its IMDb id (Trakt opens a movie or a
/// show by it, checked 2026-10-24); none for an id that is not IMDb's.
fn trakt_url(kind: &str, imdb: &str) -> Option<String> {
    let path = if kind == "movie" { "movies" } else { "shows" };
    crate::segments::is_imdb(imdb).then(|| format!("https://trakt.tv/{path}/{imdb}"))
}

/// The TMDB and Trakt lookup (plans/title-page-ratings-genres-similar.md),
/// once the page has been visible `VISIBLE_FOR` and its meta is in: for a
/// movie or a series TMDB and Trakt are asked together, so a TMDB outage
/// never holds Trakt back; an anime asks TMDB first for its IMDb id's kind,
/// and with no answer asks Trakt nothing. Each answer patches the page.
async fn look_up(ctx: Ctx) {
    // From the moment the page shows; answers kept from an earlier visit
    // show at once, with no wait.
    let wait = tokio::time::sleep(VISIBLE_FOR);
    let mut header = ctx.header.subscribe();
    while ctx.header.get().state == TitleState::Loading {
        if header.next().await.is_none() {
            return;
        }
    }
    if lock(&ctx.state).meta.is_none() {
        return;
    }
    let (Some(asked), Some(imdb)) = (asked(&ctx.kind), ctx.imdb()) else {
        return;
    };
    let kind = match asked {
        Asked::As(kind) => Some(kind),
        Asked::ByTmdb => None,
    };
    if !ctx.core.lookups_held(kind, &imdb) {
        wait.await;
    }
    let tmdb = async {
        let found = ctx.core.tmdb_find(&imdb, kind).await;
        let url = found.as_ref().map(tmdb_url);
        ctx.put(|s| s.tmdb = Some(percent(found.map(|f| (f.score, f.votes)), url)));
        found
    };
    match asked {
        Asked::As(kind) => {
            tokio::join!(tmdb, ctx.ask_trakt(kind, &imdb));
        }
        Asked::ByTmdb => match tmdb.await {
            Some(found) => ctx.ask_trakt(found.kind, &imdb).await,
            None => ctx.put(|s| s.trakt = Some(Slot::Empty)),
        },
    }
}

async fn load_meta(ctx: Ctx) {
    match ctx.core.fetch_meta(&ctx.kind, &ctx.id).await {
        Ok((meta, source)) => {
            lock(&ctx.state).meta = Some(meta);
            ctx.reread_cover();
            ctx.render_with_source(&source);
        }
        Err(reason) => {
            let mut header = ctx.header.get();
            header.state = TitleState::Failed { reason };
            ctx.header.set(header);
        }
    }
}

/// Asks the subtitles addons about the video Play plays, each time that
/// video changes (a series moves on) and each time the addons change, for
/// the header's `captions`. The player asks again for itself: it sends the
/// file's hash and name, which OpenSubtitles matches by, so this answer
/// cannot stand in for its own.
async fn watch_captions(ctx: Ctx) {
    let mut headers = ctx.header.subscribe();
    let mut addons = ctx.core.library().subscribe();
    let mut asked: Option<String> = None;
    loop {
        let video = ctx.header.get().play_target.map(|t| t.video_id);
        if let Some(video) = video.filter(|v| asked.as_ref() != Some(v)) {
            asked = Some(video.clone());
            ask_captions(&ctx, &video).await;
        }
        tokio::select! {
            header = headers.next() => {
                if header.is_none() {
                    return;
                }
            }
            changed = crate::views::addons_changed(&mut addons) => {
                if !changed {
                    return;
                }
                // Installed, removed or switched: ask the addons as they are.
                asked = None;
            }
        }
    }
}

/// Shows what the subtitles addons list for `video` as each one answers:
/// the language count only grows and the hard-of-hearing mark only turns
/// on, so a slow addon never holds back a quick one's answer. With no
/// subtitles addon the header says nothing.
async fn ask_captions(ctx: &Ctx, video: &str) {
    let path = mq_protocol::ResourcePath {
        resource: "subtitles".into(),
        r#type: ctx.kind.clone(),
        id: video.to_owned(),
        extra: Vec::new(),
    };
    let addons: Vec<_> = ctx
        .core
        .installed_addons()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| {
            a.supports(&path)
                && !crate::turkcealtyazi::is_builtin(a.transport_url.as_str(), Some(&a.manifest.id))
        })
        .collect();
    let show = |captions: Option<Captions>| {
        lock(&ctx.state).captions = captions.map(|c| (video.to_owned(), c));
        ctx.render();
    };
    if addons.is_empty() {
        show(None);
        return;
    }
    let mut fanout = ctx
        .core
        .addons()
        .fan_out(&addons, &path, mq_addons::Deadlines::default());
    let (mut languages, mut hearing_impaired) = (HashSet::new(), false);
    while let Some(event) = fanout.next().await {
        let mq_addons::AddonEvent::Arrived { body, .. } = event else {
            continue;
        };
        let found = mq_protocol::decode_subtitles(&body)
            .map(|r| r.subtitles)
            .unwrap_or_default();
        for w in &found {
            let t = crate::views::playback::listed_track(w);
            languages.insert(t.language.clone());
            hearing_impaired |= t.hearing_impaired;
        }
        show(Some(Captions {
            languages: languages.len() as u32,
            hearing_impaired,
        }));
    }
}

async fn watch_library(ctx: Ctx) {
    let mut changes = ctx.core.library().subscribe();
    // Catch writes made between open and this subscription.
    ctx.render();
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables.contains(&Table::Addons) {
            ctx.reread_cover();
        }
        if tables.iter().any(|t| {
            matches!(
                t,
                Table::Progress | Table::Watched | Table::LibraryItems | Table::Addons
            )
        }) {
            ctx.render();
        }
    }
}

impl Ctx {
    /// The page's TMDB and Trakt slots: an answer that landed, `pending`
    /// while one can still come, `none` when no call will be made.
    fn scores(&self) -> TitleScores {
        let (tmdb, trakt) = {
            let state = lock(&self.state);
            (state.tmdb.clone(), state.trakt.clone())
        };
        let asked = asked(&self.kind);
        let imdb = asked.is_some() && !self.kids && self.imdb().is_some();
        let tmdb_asked = imdb && self.core.tmdb_ready();
        let trakt_asked = imdb
            && self.core.trakt_lookup_ready()
            && (!matches!(asked, Some(Asked::ByTmdb)) || tmdb_asked);
        let planned = |asked: bool| if asked { Slot::Pending } else { Slot::Empty };
        TitleScores {
            tmdb: tmdb.unwrap_or_else(|| planned(tmdb_asked)),
            trakt: trakt.unwrap_or_else(|| planned(trakt_asked)),
        }
    }

    /// The title's IMDb id, one for every badge and lookup: its own, the one
    /// an addon gave for it (Kitsu's), or the one its meta answered with (a
    /// title opened by another id).
    fn imdb(&self) -> Option<String> {
        self.core.imdb_id_of(&self.id).or_else(|| {
            let state = lock(&self.state);
            let id = &state.meta.as_ref()?.preview.id;
            crate::segments::is_imdb(id).then(|| id.clone())
        })
    }

    /// RPDB's cover for the title with the key the addons give now.
    fn reread_cover(&self) {
        let url = self.core.covers([self.id.as_str()]).url(&self.id);
        lock(&self.state).rated_poster = url;
    }

    /// Puts an answer in and renders the page with it.
    fn put(&self, f: impl FnOnce(&mut State)) {
        f(&mut lock(&self.state));
        self.render();
    }

    /// Trakt's score and related titles for the title as `kind`, together.
    async fn ask_trakt(&self, kind: &'static str, imdb: &str) {
        let score = async {
            let score = self.core.trakt_score(kind, imdb).await;
            self.put(|s| s.trakt = Some(percent(score, trakt_url(kind, imdb))));
        };
        let related = async {
            let found = self.core.trakt_related(kind, imdb).await;
            let tiles = self.similar_tiles(imdb, found);
            if self.similar.get() != tiles {
                self.similar.set(tiles);
            }
        };
        tokio::join!(score, related);
    }

    /// "More like this" from Trakt's related titles: never the title itself,
    /// each once, those the profile's policy allows (kids get none anyway:
    /// defence in depth), each with its rated cover and Stremio's public
    /// poster (the one Cinemeta uses) under it, so a profile with no RPDB
    /// key still sees posters.
    fn similar_tiles(
        &self,
        imdb: &str,
        found: Vec<mq_tracking::trakt::search::Found>,
    ) -> Vec<Tile> {
        let policy = self.core.policy();
        let mut seen = HashSet::new();
        let found: Vec<_> = found
            .into_iter()
            // Trakt's ids become tile ids, poster URLs and title pages: IMDb's
            // shape only.
            .filter(|f| crate::segments::is_imdb(&f.imdb_id))
            .filter(|f| f.imdb_id != imdb && f.imdb_id != self.id)
            .filter(|f| policy.allows(f.kind, &f.imdb_id))
            .filter(|f| seen.insert(f.imdb_id.clone()))
            .take(SIMILAR)
            .collect();
        let covers = self.core.covers(found.iter().map(|f| f.imdb_id.as_str()));
        found
            .into_iter()
            .map(|f| {
                Tile {
                    poster: Some(format!(
                        "https://images.metahub.space/poster/medium/{}/img",
                        f.imdb_id
                    )),
                    id: f.imdb_id,
                    kind: f.kind.to_owned(),
                    name: f.title,
                    poster_fallback: None,
                    background: None,
                    logo: None,
                    poster_shape: "poster".into(),
                    release_info: None,
                    progress: None,
                    video_id: None,
                    rating: None,
                    ratings: Ratings::default(),
                    blurb: None,
                }
                .rated_cover(&covers)
            })
            .collect()
    }

    fn render(&self) {
        let source = self
            .header
            .get()
            .details
            .map(|d| d.source)
            .unwrap_or_default();
        self.render_with_source(&source);
    }

    fn render_with_source(&self, source: &str) {
        let (meta, selected, captions) = {
            let state = lock(&self.state);
            (
                state.meta.clone(),
                state.selected_season,
                state.captions.clone(),
            )
        };
        let Some(meta) = meta else { return };
        let profile = self.core.profile();
        let library = self.core.library();
        let watched: HashSet<String> = library
            .watched_videos(profile, &self.id)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let in_library = library.is_in_library(profile, &self.id).unwrap_or(false);

        let mut seasons: Vec<u32> = meta
            .videos
            .iter()
            .filter_map(|v| v.series_info.as_ref().map(|s| s.season))
            .collect();
        seasons.sort_unstable();
        seasons.dedup();
        let resume = library.latest_progress(profile, &self.id).unwrap_or(None);
        // Default season: the one being watched, else the first regular season.
        let default_season = resume
            .as_ref()
            .and_then(|(video_id, ..)| season_of(&meta, video_id))
            .or_else(|| seasons.iter().copied().find(|s| *s > 0))
            .or_else(|| seasons.first().copied());
        let season = selected.or(default_season);

        let episodes: Vec<Episode> = meta
            .videos
            .iter()
            .filter_map(|v| {
                let info = v
                    .series_info
                    .as_ref()
                    .filter(|i| Some(i.season) == season)?;
                let progress = library.progress(profile, &v.id).ok().flatten();
                Some(Episode {
                    video_id: v.id.clone(),
                    season: info.season,
                    episode: info.episode,
                    title: v.title.clone(),
                    overview: v.overview.clone(),
                    thumbnail: v.thumbnail.clone(),
                    released: v.released.map(|d| d.date_naive().to_string()),
                    watched: watched.contains(&v.id),
                    progress: progress.and_then(|(pos, dur)| {
                        (dur > 0).then(|| (pos as f32 / dur as f32).clamp(0.0, 1.0))
                    }),
                    left_ms: progress.and_then(|(pos, dur)| left(pos, dur)),
                })
            })
            .collect();

        let now = mq_library::hlc::now_ms() as i64;
        let (play_target, next_airs) = play_target(&meta, &self.kind, resume, &watched, now);
        let rated_poster = lock(&self.state).rated_poster.clone();
        let imdb = self.imdb();
        let mut details = details(&meta, source, rated_poster, imdb.as_deref(), self.scores());
        if self.kids {
            // A trailer is a YouTube hand-off and IMDb's page the open web:
            // both leave the app.
            details.trailers.clear();
            details.imdb_url = None;
        }
        let header = TitleHeader {
            state: TitleState::Ready,
            details: Some(details),
            in_library,
            library_settable: !self.kids,
            genres_linked: !self.kids,
            // Only the answer for the video Play plays now.
            captions: captions
                .filter(|(video, _)| play_target.as_ref().is_some_and(|t| t.video_id == *video))
                .map(|(_, c)| c),
            seasons,
            selected_season: season,
            play_target,
            next_airs,
            watched: if series_like(&self.kind, &meta) {
                let aired = aired_episodes(&meta, now);
                !aired.is_empty() && aired.iter().all(|v| watched.contains(v))
            } else {
                watched.contains(&self.id)
            },
        };
        if self.header.get() != header {
            self.header.set(header);
        }
        let mut list = lock(&self.episodes);
        if list.iter().ne(episodes.iter()) {
            list.clear();
            list.append(episodes.into_iter().collect());
        }
    }
}

fn season_of(meta: &MetaItem, video_id: &str) -> Option<u32> {
    meta.videos
        .iter()
        .find(|v| v.id == video_id)
        .and_then(|v| v.series_info.as_ref().map(|s| s.season))
}

/// A title played episode by episode: any kind but a movie (a series, an
/// anime, a channel) with videos. Without videos it plays, and is marked,
/// as one, like a movie.
fn series_like(kind: &str, meta: &MetaItem) -> bool {
    kind != "movie" && !meta.videos.is_empty()
}

/// A title's aired regular episodes (season 0, the specials, left out; an
/// episode with no season counts): what a title-level watched mark counts.
fn aired_episodes(meta: &MetaItem, now_ms: i64) -> Vec<String> {
    meta.videos
        .iter()
        .filter(|v| v.series_info.as_ref().is_none_or(|i| i.season > 0))
        .filter(|v| aired(v, now_ms))
        .map(|v| v.id.clone())
        .collect()
}

/// What the primary button plays, and the day the next episode comes out
/// when it is not out yet. Resume the most recent unfinished video. A movie
/// is the movie itself ("Play again" once watched). A series goes on from
/// the episode after the one played last (Home's Next Up reads the same),
/// else after the furthest one watched (marks from Trakt or by hand), else
/// from its first episode; an episode not out yet is never the target, and
/// a series watched to its last episode starts again from its first.
fn play_target(
    meta: &MetaItem,
    kind: &str,
    resume: Option<(String, i64, i64)>,
    watched: &HashSet<String>,
    now_ms: i64,
) -> (Option<PlayTarget>, Option<String>) {
    let start = |video: &Video, label: String| PlayTarget {
        video_id: video.id.clone(),
        label,
        resume_ms: None,
        left_ms: None,
    };
    let finished = |position: i64, duration: i64| {
        duration > 0 && position as f64 >= duration as f64 * mq_library::FINISHED_RATIO
    };
    if let Some((video_id, position, duration)) = &resume
        && *position > 0
        && !finished(*position, *duration)
    {
        let label =
            episode_label(meta, video_id).map_or("Resume".to_owned(), |e| format!("Resume {e}"));
        let target = PlayTarget {
            video_id: video_id.clone(),
            label,
            resume_ms: Some(*position),
            left_ms: left(*position, *duration),
        };
        return (Some(target), None);
    }
    if !series_like(kind, meta) {
        let again = watched.contains(&meta.preview.id);
        let target = PlayTarget {
            video_id: meta.preview.id.clone(),
            label: if again { "Play again" } else { "Play" }.into(),
            resume_ms: None,
            left_ms: None,
        };
        return (Some(target), None);
    }
    let episodes = in_order(meta);
    let Some(&(first, _)) = episodes.first() else {
        return (None, None);
    };
    let play = |video: &Video| start(video, format!("Play {}", label_of(video)));
    let known = |id: &str| episodes.iter().any(|(v, _)| v.id == id);
    // The one played last, unless it was opened and left at once.
    let played = resume.filter(|(id, position, duration)| {
        known(id) && (watched.contains(id) || finished(*position, *duration))
    });
    let anchor = played.map(|(id, ..)| id).or_else(|| {
        episodes
            .iter()
            .rev()
            .find(|(v, _)| watched.contains(&v.id))
            .map(|(v, _)| v.id.clone())
    });
    let next = match &anchor {
        Some(id) => following(meta, id).map(|(v, _)| v),
        None => Some(first),
    };
    match next {
        Some(video) if aired(video, now_ms) => (Some(play(video)), None),
        Some(video) => (None, video.released.map(|d| d.date_naive().to_string())),
        None => {
            let label = format!("Play again from {}", label_of(first));
            (Some(start(first, label)), None)
        }
    }
}

/// The regular episodes in season and episode order; every episode for a
/// series that has only specials.
fn in_order(meta: &MetaItem) -> Vec<(&Video, &SeriesInfo)> {
    let mut episodes: Vec<_> = meta
        .videos
        .iter()
        .filter_map(|v| Some((v, v.series_info.as_ref()?)))
        .collect();
    if episodes.iter().any(|(_, s)| s.season > 0) {
        episodes.retain(|(_, s)| s.season > 0);
    }
    episodes.sort_by_key(|(_, s)| (s.season, s.episode));
    episodes
}

fn label_of(video: &Video) -> String {
    video
        .series_info
        .as_ref()
        .map_or_else(String::new, |s| format!("S{}:E{}", s.season, s.episode))
}

/// The rest of a started file; `None` before it starts or without a duration.
fn left(position: i64, duration: i64) -> Option<i64> {
    (position > 0 && duration > position).then(|| duration - position)
}

fn episode_label(meta: &MetaItem, video_id: &str) -> Option<String> {
    let video = meta.videos.iter().find(|v| v.id == video_id)?;
    let info = video.series_info.as_ref()?;
    Some(format!("S{}:E{}", info.season, info.episode))
}

fn details(
    meta: &MetaItem,
    source: &str,
    rated_poster: Option<String>,
    imdb: Option<&str>,
    scores: TitleScores,
) -> TitleDetails {
    let p = &meta.preview;
    let links = |category: &str| -> Vec<String> {
        p.links
            .iter()
            .filter(|l| l.category.eq_ignore_ascii_case(category))
            .map(|l| l.name.clone())
            .collect()
    };
    TitleDetails {
        id: p.id.clone(),
        kind: p.r#type.clone(),
        name: p.name.clone(),
        description: p.description.clone(),
        poster: p.poster.as_ref().map(|u| u.to_string()),
        rated_poster,
        background: p.background.as_ref().map(|u| u.to_string()),
        logo: p.logo.as_ref().map(|u| u.to_string()),
        release_info: super::home::year_line(p.release_info.as_deref()),
        runtime: p.runtime.clone(),
        genres: links("Genres"),
        cast: links("Cast"),
        directors: links("Directors"),
        writers: links("Writers"),
        rating: links("imdb").into_iter().next(),
        ratings: Ratings::of(links("imdb").first().map(String::as_str)),
        imdb_url: imdb
            .filter(|id| crate::segments::is_imdb(id))
            .map(|id| format!("https://www.imdb.com/title/{id}/")),
        scores,
        source: source.to_owned(),
        trailers: p
            .trailer_streams
            .iter()
            .filter_map(|s| match &s.source {
                StreamSource::YouTube { yt_id } => Some(Trailer {
                    title: s
                        .description
                        .clone()
                        .or_else(|| s.name.clone())
                        .unwrap_or_else(|| "Trailer".into()),
                    youtube_id: yt_id.clone(),
                    url: format!("https://www.youtube.com/watch?v={yt_id}"),
                }),
                _ => None,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A badge links only from a checked id, whatever an addon wrote, and
    /// rounds half up as the apps show it.
    #[test]
    fn badge_links_come_from_checked_ids() {
        assert_eq!(
            trakt_url("series", "tt0903747").as_deref(),
            Some("https://trakt.tv/shows/tt0903747")
        );
        assert_eq!(trakt_url("movie", "tt1/../../evil"), None);
        assert_eq!(trakt_url("movie", "https://example.com"), None);
        let found = mq_tracking::tmdb::Found {
            kind: "series",
            id: 1396,
            score: 9.0,
            votes: 18925,
        };
        assert_eq!(tmdb_url(&found), "https://www.themoviedb.org/tv/1396");
        let text = |score| match percent(Some((score, 50)), None) {
            Slot::Shown { text, .. } => text,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            (text(8.74), text(8.75), text(8.65)),
            ("87%".into(), "88%".into(), "87%".into())
        );
    }

    /// S1:E1 to E4 out, and E5 out only `later` (else no E5); a special.
    fn show(e5: Option<&str>) -> MetaItem {
        let ep = |s: u32, e: u32, released: &str| {
            serde_json::json!({"id": format!("tt9:{s}:{e}"), "title": format!("E{e}"),
                               "season": s, "episode": e, "released": released})
        };
        let mut videos = vec![
            ep(1, 3, "2020-01-15T00:00:00Z"),
            ep(0, 1, "2019-12-01T00:00:00Z"),
            ep(1, 1, "2020-01-01T00:00:00Z"),
            ep(1, 2, "2020-01-08T00:00:00Z"),
            ep(1, 4, "2020-01-22T00:00:00Z"),
        ];
        videos.extend(e5.map(|d| ep(1, 5, d)));
        serde_json::from_value(serde_json::json!(
            {"id": "tt9", "type": "series", "name": "Show", "videos": videos}
        ))
        .unwrap()
    }

    const NOW: i64 = 1_700_000_000_000;
    const LATER: &str = "2099-01-01T00:00:00Z";

    fn watched(eps: &[u32]) -> HashSet<String> {
        eps.iter().map(|e| format!("tt9:1:{e}")).collect()
    }

    fn played(e: u32, position: i64) -> Option<(String, i64, i64)> {
        Some((format!("tt9:1:{e}"), position, 3000))
    }

    fn target(
        meta: &MetaItem,
        resume: Option<(String, i64, i64)>,
        seen: &[u32],
    ) -> (String, String) {
        let (target, airs) = play_target(meta, "series", resume, &watched(seen), NOW);
        assert_eq!(airs, None);
        let t = target.expect("a target");
        (t.video_id, t.label)
    }

    /// Journey reviews 2026-10-20 (web P2-1 B): the episode after the one
    /// played last, as Next Up says, not the first unwatched.
    #[test]
    fn play_goes_on_after_the_episode_played_last() {
        let meta = show(None);
        assert_eq!(
            target(&meta, played(3, 2900), &[1, 3]),
            ("tt9:1:4".into(), "Play S1:E4".into())
        );
        // Marked watched after it was played: position 0.
        assert_eq!(target(&meta, played(2, 0), &[1, 2]).1, "Play S1:E3");
        // A rewatch goes on from where it is, not after the furthest mark.
        assert_eq!(
            target(&meta, played(2, 3000), &[1, 2, 3, 4]).1,
            "Play S1:E3"
        );
        // Marks alone (Trakt, by hand): after the furthest one.
        assert_eq!(target(&meta, None, &[1, 3]).1, "Play S1:E4");
        assert_eq!(target(&meta, None, &[]).1, "Play S1:E1");
        assert_eq!(target(&meta, played(2, 1000), &[1]).1, "Resume S1:E2");
    }

    /// Web P2-1 A: an episode not out yet is never the target; the page
    /// says when it airs.
    #[test]
    fn an_episode_not_out_yet_is_no_target() {
        let meta = show(Some(LATER));
        let (target, airs) = play_target(&meta, "series", played(4, 3000), &watched(&[4]), NOW);
        assert_eq!(target, None);
        assert_eq!(airs.as_deref(), Some("2099-01-01"));
        // Nothing out yet at all.
        let (target, airs) = play_target(&meta, "series", None, &HashSet::new(), 0);
        assert_eq!((target, airs.as_deref()), (None, Some("2020-01-01")));
    }

    /// Web P2-2, Apple P2-1: a series watched to its end plays again from
    /// its first episode instead of offering nothing; so does one of
    /// specials only.
    #[test]
    fn a_series_watched_to_the_end_plays_again_from_the_start() {
        let meta = show(None);
        assert_eq!(
            target(&meta, None, &[1, 2, 3, 4]),
            ("tt9:1:1".into(), "Play again from S1:E1".into())
        );
        assert_eq!(
            target(&meta, played(4, 3000), &[4]).1,
            "Play again from S1:E1"
        );
        let specials: MetaItem = serde_json::from_value(serde_json::json!({
        "id": "tt8", "type": "series", "name": "Specials", "videos": [
            {"id": "tt8:0:1", "title": "A", "season": 0, "episode": 1},
            {"id": "tt8:0:2", "title": "B", "season": 0, "episode": 2}
        ]}))
        .unwrap();
        let seen: HashSet<String> = ["tt8:0:1", "tt8:0:2"].map(String::from).into();
        let (target, _) = play_target(&specials, "series", None, &seen, NOW);
        assert_eq!(target.unwrap().label, "Play again from S0:E1");
    }

    /// Next Up stops at an episode not out yet: one after it with no date
    /// is not out either.
    #[test]
    fn next_up_stops_at_an_episode_not_out_yet() {
        let mut meta = show(Some(LATER));
        meta.videos.push(
            serde_json::from_value(serde_json::json!(
                {"id": "tt9:1:6", "title": "E6", "season": 1, "episode": 6}
            ))
            .unwrap(),
        );
        assert_eq!(crate::views::playback::next_after(&meta, "tt9:1:4"), None);
        let next = crate::views::playback::next_after(&meta, "tt9:1:3").unwrap();
        assert_eq!(next.video_id, "tt9:1:4");
    }
}
