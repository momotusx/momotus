//! Library: the titles the user added, filtered by type and sorted, with the
//! progress of the newest video watched.

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::views::home::{Tile, type_label};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
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
    /// Types present in the library, for the filter chips.
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
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
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
async fn watch(
    core: Core,
    mut changes: broadcast::Receiver<Vec<Table>>,
    query: Arc<Mutex<Query>>,
    snapshot: SharedObservable<LibrarySnapshot>,
) {
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables
            .iter()
            .any(|t| matches!(t, Table::LibraryItems | Table::Progress))
        {
            let query = lock(&query).clone();
            let next = render(&core, &query);
            if next != snapshot.get() {
                snapshot.set(next);
            }
        }
    }
}

fn render(core: &Core, query: &Query) -> LibrarySnapshot {
    let entries = core.library().library(core.profile()).unwrap_or_default();
    let mut kinds: Vec<KindCount> = Vec::new();
    for e in &entries {
        match kinds.iter_mut().find(|k| k.kind == e.item.kind) {
            Some(k) => k.count += 1,
            None => kinds.push(KindCount {
                kind: e.item.kind.clone(),
                label: type_label(&e.item.kind).to_owned(),
                count: 1,
            }),
        }
    }
    kinds.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.kind.cmp(&b.kind)));
    let mut items: Vec<&LibraryEntry> = entries
        .iter()
        .filter(|e| query.kind.as_ref().is_none_or(|k| *k == e.item.kind))
        .collect();
    match query.sort {
        LibrarySort::Recent => {
            items.sort_by_key(|e| std::cmp::Reverse(e.last_watched_ms.unwrap_or(e.item.added_ms)))
        }
        LibrarySort::Added => items.sort_by_key(|e| std::cmp::Reverse(e.item.added_ms)),
        LibrarySort::Name => items.sort_by_cached_key(|e| e.item.name.to_lowercase()),
    }
    LibrarySnapshot {
        kind: query.kind.clone(),
        sort: query.sort,
        kinds,
        items: items.into_iter().map(tile).collect(),
    }
}

fn tile(e: &LibraryEntry) -> Tile {
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
        release_info: None,
        progress: (e.duration_ms > 0)
            .then(|| (e.position_ms as f32 / e.duration_ms as f32).clamp(0.0, 1.0)),
        video_id: None,
    }
}
