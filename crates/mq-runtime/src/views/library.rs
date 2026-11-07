//! Library: the titles the user added, filtered by type and sorted, with the
//! progress of the newest video watched. A kids profile's library is its
//! counted list (plans/m11-kids.md 4.6).

use crate::Core;
use crate::live::{FRAME, snapshots};
use crate::views::home::{OTHER_KIND, Tile, kind_label, type_label};
use eyeball::SharedObservable;
use futures_util::Stream;
use mq_library::{LibraryEntry, Table};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum LibrarySort {
    /// Last watched first; titles never played sort by when they were added.
    #[default]
    Recent,
    Added,
    Name,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct KindCount {
    pub kind: String,
    /// "Movies", "Series", ...
    pub label: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    /// The type filter, `None` for everything.
    pub kind: Option<String>,
    pub sort: LibrarySort,
    /// Kinds present in the library, for the filter chips; every type
    /// Marquee does not know shares one chip, `other` ("Other").
    pub kinds: Vec<KindCount>,
    pub items: Vec<Tile>,
}

#[derive(Clone, Default)]
struct Query {
    kind: Option<String>,
    sort: LibrarySort,
}

pub struct LibraryView {
    core: Core,
    query: Arc<Mutex<Query>>,
    snapshot: SharedObservable<LibrarySnapshot>,
    task: JoinHandle<()>,
}

impl LibraryView {
    pub fn open(core: &Core) -> Self {
        // Subscribe before the first read so no change slips in between.
        let changes = core.library().subscribe();
        let query = Arc::new(Mutex::new(Query::default()));
        let snapshot = SharedObservable::new(render(core, &Query::default()));
        let task = tokio::spawn(watch(
            core.clone(),
            changes,
            query.clone(),
            snapshot.clone(),
        ));
        Self {
            core: core.clone(),
            query,
            snapshot,
            task,
        }
    }

    pub fn snapshot(&self) -> LibrarySnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        LibrarySnapshot,
        Pin<Box<dyn Stream<Item = Vec<LibrarySnapshot>> + Send>>,
    ) {
        (
            self.snapshot.get(),
            snapshots(self.snapshot.subscribe(), FRAME),
        )
    }

    pub fn set_filter(&self, kind: Option<String>) {
        self.update(|q| q.kind = kind);
    }

    pub fn set_sort(&self, sort: LibrarySort) {
        self.update(|q| q.sort = sort);
    }

    fn update(&self, f: impl FnOnce(&mut Query)) {
        let query = {
            let mut q = lock(&self.query);
            f(&mut q);
            q.clone()
        };
        self.snapshot.set(render(&self.core, &query));
    }
}

impl Drop for LibraryView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ponytail: every change re-reads and re-sends the whole library; fine for a
// few hundred titles, move to list diffs if libraries grow into thousands.
/// Follows the library.
async fn watch(
    core: Core,
    mut changes: broadcast::Receiver<Vec<Table>>,
    query: Arc<Mutex<Query>>,
    snapshot: SharedObservable<LibrarySnapshot>,
) {
    let again = |snapshot: &SharedObservable<LibrarySnapshot>| {
        let next = render(&core, &lock(&query).clone());
        if next != snapshot.get() {
            snapshot.set(next);
        }
    };
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables
            .iter()
            // Addons too: the RPDB key for the covers comes from one.
            .any(|t| {
                matches!(
                    t,
                    Table::LibraryItems | Table::Progress | Table::Access | Table::Addons
                )
            })
        {
            again(&snapshot);
        }
    }
}

/// The filter chip a title's type falls under: a kind Marquee knows, or
/// one "Other" chip for every other type, never shown as is.
fn chip(kind: &str) -> &str {
    if kind_label(kind).is_some() {
        kind
    } else {
        OTHER_KIND.0
    }
}

fn render(core: &Core, query: &Query) -> LibrarySnapshot {
    let library = core.library();
    let entries = if core.policy().kids() {
        library.allowed_entries(core.profile())
    } else {
        library.library(core.profile())
    }
    .unwrap_or_default();
    let mut kinds: Vec<KindCount> = Vec::new();
    for e in &entries {
        let kind = chip(&e.item.kind);
        match kinds.iter_mut().find(|k| k.kind == kind) {
            Some(k) => k.count += 1,
            None => kinds.push(KindCount {
                kind: kind.to_owned(),
                label: type_label(kind).to_owned(),
                count: 1,
            }),
        }
    }
    kinds.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.kind.cmp(&b.kind)));
    let mut items: Vec<&LibraryEntry> = entries
        .iter()
        .filter(|e| query.kind.as_ref().is_none_or(|k| k == chip(&e.item.kind)))
        .collect();
    match query.sort {
        LibrarySort::Recent => {
            items.sort_by_key(|e| std::cmp::Reverse(e.last_watched_ms.unwrap_or(e.item.added_ms)))
        }
        LibrarySort::Added => items.sort_by_key(|e| std::cmp::Reverse(e.item.added_ms)),
        LibrarySort::Name => items.sort_by_cached_key(|e| e.item.name.to_lowercase()),
    }
    let covers = core.covers(items.iter().map(|e| e.item.item_id.as_str()));
    let items: Vec<Tile> = items.into_iter().map(|e| tile(e, &covers)).collect();
    LibrarySnapshot {
        kind: query.kind.clone(),
        sort: query.sort,
        kinds,
        items,
    }
}

fn tile(e: &LibraryEntry, covers: &crate::covers::Covers) -> Tile {
    Tile {
        id: e.item.item_id.clone(),
        kind: e.item.kind.clone(),
        name: e.item.name.clone(),
        poster: e.item.poster.clone(),
        background: None,
        logo: None,
        poster_shape: e
            .item
            .poster_shape
            .clone()
            .unwrap_or_else(|| "poster".into()),
        release_info: library_caption(e),
        progress: (e.duration_ms > 0)
            .then(|| (e.position_ms as f32 / e.duration_ms as f32).clamp(0.0, 1.0)),
        video_id: None,
        rating: None,
        ratings: crate::ratings::Ratings::default(),
        blurb: None,
        own_shape: None,
        poster_fallback: None,
        rank: None,
        genre: None,
    }
    .rated_stored_cover(covers)
}

/// Where the user is with a title, in Continue Watching's words: "S2:E3 ·
/// 36 min left", "36 min left", "S2:E3 · Watched", "Watched"; nothing for a
/// title never played.
fn library_caption(e: &LibraryEntry) -> Option<String> {
    let finished = e.duration_ms > 0
        && e.position_ms as f64 >= e.duration_ms as f64 * mq_library::FINISHED_RATIO;
    let rest = if e.watched || finished {
        "Watched".to_owned()
    } else if e.position_ms > 0 && e.duration_ms > e.position_ms {
        let minutes = (e.duration_ms - e.position_ms + 59_999) / 60_000;
        format!("{minutes} min left")
    } else {
        return None;
    };
    let video = e.video_id.as_deref().unwrap_or(&e.item.item_id);
    Some(super::home::with_episode(&e.item.kind, video, rest))
}
