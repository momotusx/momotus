//! Title page: metadata from the first addon (in the user's order) that
//! answers for the id, episodes grouped by season with watched and progress
//! state from the library, and library and watched commands.

use crate::Core;
use crate::live::{FRAME, ListDiff, coalesce};
use eyeball::SharedObservable;
use eyeball_im::ObservableVector;
use futures_util::{Stream, StreamExt};
use mq_library::{LibraryItemRow, Table};
use mq_protocol::{MetaItem, ResourcePath, ResourceResponse};
use serde::Serialize;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;

const META_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TitleDetails {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
    pub poster: Option<String>,
    pub background: Option<String>,
    pub logo: Option<String>,
    pub release_info: Option<String>,
    pub runtime: Option<String>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    pub directors: Vec<String>,
    pub rating: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum TitleState {
    Loading,
    Ready,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TitleHeader {
    pub state: TitleState,
    pub details: Option<TitleDetails>,
    pub in_library: bool,
    pub seasons: Vec<u32>,
    pub selected_season: Option<u32>,
    /// Episode (or the movie itself) the primary button resumes or starts.
    pub play_target: Option<PlayTarget>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayTarget {
    pub video_id: String,
    pub label: String,
    pub resume_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TitleUpdate {
    Header { header: Box<TitleHeader> },
    Episodes { diffs: Vec<ListDiff<Episode>> },
}

struct State {
    meta: Option<MetaItem>,
    selected_season: Option<u32>,
}

/// A live title page. Dropping it cancels its work.
pub struct TitleView {
    core: Core,
    kind: String,
    id: String,
    header: SharedObservable<TitleHeader>,
    episodes: Arc<Mutex<ObservableVector<Episode>>>,
    state: Arc<Mutex<State>>,
    _tasks: JoinSet<()>,
}

pub struct TitleSnapshot {
    pub header: TitleHeader,
    pub episodes: Vec<Episode>,
}

impl TitleView {
    pub fn open(core: &Core, kind: &str, id: &str) -> Self {
        let header = SharedObservable::new(TitleHeader {
            state: TitleState::Loading,
            details: None,
            in_library: false,
            seasons: Vec::new(),
            selected_season: None,
            play_target: None,
        });
        let mut view = Self {
            core: core.clone(),
            kind: kind.to_owned(),
            id: id.to_owned(),
            header,
            episodes: Arc::new(Mutex::new(ObservableVector::new())),
            state: Arc::new(Mutex::new(State {
                meta: None,
                selected_season: None,
            })),
            _tasks: JoinSet::new(),
        };
        view._tasks.spawn(load_meta(view.ctx()));
        view._tasks.spawn(watch_library(view.ctx()));
        view
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            core: self.core.clone(),
            kind: self.kind.clone(),
            id: self.id.clone(),
            header: self.header.clone(),
            episodes: self.episodes.clone(),
            state: self.state.clone(),
        }
    }

    pub fn snapshot(&self) -> TitleSnapshot {
        TitleSnapshot {
            header: self.header.get(),
            episodes: lock(&self.episodes).iter().cloned().collect(),
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
        let merged = futures_util::stream::select(episodes, headers);
        (snapshot, Box::pin(coalesce(Box::pin(merged), FRAME)))
    }

    pub fn select_season(&self, season: u32) {
        lock(&self.state).selected_season = Some(season);
        self.ctx().render();
    }

    /// Adds or removes the title from the library.
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
        self.core
            .library()
            .upsert_item(self.core.profile(), &row, in_library, &self.core.hlc())?;
        Ok(())
    }

    pub fn set_watched(&self, video_id: &str, watched: bool) -> Result<(), crate::CoreError> {
        self.core.library().set_watched(
            self.core.profile(),
            &self.id,
            video_id,
            watched,
            &self.core.hlc(),
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct Ctx {
    core: Core,
    kind: String,
    id: String,
    header: SharedObservable<TitleHeader>,
    episodes: Arc<Mutex<ObservableVector<Episode>>>,
    state: Arc<Mutex<State>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

async fn load_meta(ctx: Ctx) {
    let path = ResourcePath {
        resource: "meta".into(),
        r#type: ctx.kind.clone(),
        id: ctx.id.clone(),
        extra: vec![],
    };
    let addons: Vec<_> = ctx
        .core
        .installed_addons()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.supports(&path))
        .collect();
    let mut last_error = "no installed addon provides details for this title".to_owned();
    // First answer in addon order wins, as in Stremio; later addons are only
    // asked when earlier ones fail.
    for addon in addons {
        match ctx.core.addons().fetch(&addon, &path, META_TIMEOUT).await {
            Ok(fetched) => match serde_json::from_slice::<ResourceResponse>(&fetched.body) {
                Ok(ResourceResponse::Meta { meta }) => {
                    lock(&ctx.state).meta = Some(meta);
                    ctx.render_with_source(&addon.manifest.name);
                    return;
                }
                Ok(_) => last_error = format!("{} returned no details", addon.manifest.name),
                Err(e) => {
                    last_error = format!("{} returned unreadable details: {e}", addon.manifest.name)
                }
            },
            Err(e) => last_error = format!("{}: {e}", addon.manifest.name),
        }
    }
    let mut header = ctx.header.get();
    header.state = TitleState::Failed { reason: last_error };
    ctx.header.set(header);
}

async fn watch_library(ctx: Ctx) {
    let mut changes = ctx.core.library().subscribe();
    while let Ok(tables) = changes.recv().await {
        if tables
            .iter()
            .any(|t| matches!(t, Table::Progress | Table::Watched | Table::LibraryItems))
        {
            ctx.render();
        }
    }
}

impl Ctx {
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
        let (meta, selected) = {
            let state = lock(&self.state);
            (state.meta.clone(), state.selected_season)
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
                let info = v.series_info.as_ref()?;
                (Some(info.season) == season).then(|| Episode {
                    video_id: v.id.clone(),
                    season: info.season,
                    episode: info.episode,
                    title: v.title.clone(),
                    overview: v.overview.clone(),
                    thumbnail: v.thumbnail.clone(),
                    released: v.released.map(|d| d.date_naive().to_string()),
                    watched: watched.contains(&v.id),
                    progress: library.progress(profile, &v.id).ok().flatten().and_then(
                        |(pos, dur)| (dur > 0).then(|| (pos as f32 / dur as f32).clamp(0.0, 1.0)),
                    ),
                })
            })
            .collect();

        let play_target = play_target(&meta, &self.kind, resume, &watched);
        let header = TitleHeader {
            state: TitleState::Ready,
            details: Some(details(&meta, source)),
            in_library,
            seasons,
            selected_season: season,
            play_target,
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

/// Resume the most recent unfinished video; otherwise the first unwatched
/// regular episode; for movies, the movie itself.
fn play_target(
    meta: &MetaItem,
    kind: &str,
    resume: Option<(String, i64, i64)>,
    watched: &HashSet<String>,
) -> Option<PlayTarget> {
    if let Some((video_id, position, duration)) = resume
        && position > 0
        && (duration == 0 || (position as f64) < duration as f64 * 0.92)
    {
        let label =
            episode_label(meta, &video_id).map_or("Resume".to_owned(), |e| format!("Resume {e}"));
        return Some(PlayTarget {
            video_id,
            label,
            resume_ms: Some(position),
        });
    }
    if kind == "movie" || meta.videos.is_empty() {
        return Some(PlayTarget {
            video_id: meta.preview.id.clone(),
            label: "Play".into(),
            resume_ms: None,
        });
    }
    let next = meta
        .videos
        .iter()
        .filter(|v| v.series_info.as_ref().is_some_and(|s| s.season > 0))
        .find(|v| !watched.contains(&v.id))?;
    Some(PlayTarget {
        video_id: next.id.clone(),
        label: episode_label(meta, &next.id).map_or("Play".to_owned(), |e| format!("Play {e}")),
        resume_ms: None,
    })
}

fn episode_label(meta: &MetaItem, video_id: &str) -> Option<String> {
    let video = meta.videos.iter().find(|v| v.id == video_id)?;
    let info = video.series_info.as_ref()?;
    Some(format!("S{}:E{}", info.season, info.episode))
}

fn details(meta: &MetaItem, source: &str) -> TitleDetails {
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
        background: p.background.as_ref().map(|u| u.to_string()),
        logo: p.logo.as_ref().map(|u| u.to_string()),
        release_info: p.release_info.clone(),
        runtime: p.runtime.clone(),
        genres: links("Genres"),
        cast: links("Cast"),
        directors: links("Directors"),
        rating: links("imdb").into_iter().next(),
        source: source.to_owned(),
    }
}
