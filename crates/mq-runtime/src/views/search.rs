//! Search: one row per addon catalog that supports `search`, in addon order.
//! A title shown in an earlier row is not repeated further down, so the same
//! film from two metadata addons appears once.
//!
//! The query is debounced here, so every UI types into the same behaviour.

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::views::addons_changed;
use crate::views::home::{HomeRow, RowState, Tile, tile_from_preview, type_label};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::Table;
use mq_protocol::{ResourcePath, ResourceResponse};
use serde::Serialize;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use stremio_core::types::addon::ExtraValue;
use tokio::sync::broadcast::Receiver;
use tokio::task::{JoinHandle, JoinSet};

const DEBOUNCE: Duration = Duration::from_millis(250);
const SOFT_DEADLINE: Duration = Duration::from_millis(2500);
const HARD_DEADLINE: Duration = Duration::from_secs(12);
const RECENT: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct SearchSnapshot {
    pub query: String,
    /// Rows with results, loading or failed; catalogs with no match are left out.
    pub rows: Vec<HomeRow>,
    /// Recent searches, newest first, for the empty state.
    pub recent: Vec<String>,
}

pub struct SearchView {
    core: Core,
    snapshot: SharedObservable<SearchSnapshot>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    watch: JoinHandle<()>,
}

#[derive(Clone)]
struct Slot {
    row_id: String,
    title: String,
    addon: InstalledAddon,
    path: ResourcePath,
}

impl SearchView {
    pub fn open(core: &Core) -> Self {
        let snapshot = SharedObservable::new(SearchSnapshot {
            query: String::new(),
            rows: Vec::new(),
            recent: recent(core),
        });
        let task = Arc::default();
        let changes = core.library().subscribe();
        let watch = tokio::spawn(watch_addons(
            core.clone(),
            snapshot.clone(),
            Arc::clone(&task),
            changes,
        ));
        Self {
            core: core.clone(),
            snapshot,
            task,
            watch,
        }
    }

    pub fn snapshot(&self) -> SearchSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        SearchSnapshot,
        Pin<Box<dyn Stream<Item = Vec<SearchSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }

    /// New text from the search field. Work for the previous text stops.
    pub fn set_query(&self, query: &str) {
        let query = query.trim().to_owned();
        if let Some(old) = lock(&self.task).take() {
            old.abort();
        }
        if query.is_empty() {
            self.snapshot.set(SearchSnapshot {
                query,
                rows: Vec::new(),
                recent: recent(&self.core),
            });
            return;
        }
        let task = tokio::spawn(run(self.core.clone(), query, self.snapshot.clone()));
        *lock(&self.task) = Some(task);
    }

    /// The user opened a result: keep the query in recent searches.
    pub fn remember(&self) -> Result<(), crate::CoreError> {
        let query = self.snapshot.get().query;
        if !query.is_empty() {
            let now = mq_library::hlc::now_ms() as i64;
            self.core
                .library()
                .record_search(self.core.profile(), &query, now)?;
        }
        Ok(())
    }
}

impl Drop for SearchView {
    fn drop(&mut self) {
        self.watch.abort();
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
    }
}

/// Runs the query on screen again when the installed addons change, so a
/// newly installed search catalog shows up without retyping.
async fn watch_addons(
    core: Core,
    snapshot: SharedObservable<SearchSnapshot>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    mut changes: Receiver<Vec<Table>>,
) {
    while addons_changed(&mut changes).await {
        let query = snapshot.get().query;
        if query.is_empty() {
            continue;
        }
        let again = tokio::spawn(run(core.clone(), query, snapshot.clone()));
        if let Some(old) = lock(&task).replace(again) {
            old.abort();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn recent(core: &Core) -> Vec<String> {
    core.library()
        .recent_searches(core.profile(), RECENT)
        .unwrap_or_default()
}

fn search_slots(addons: &[InstalledAddon], query: &str) -> Vec<Slot> {
    let extra = vec![ExtraValue {
        name: "search".into(),
        value: query.to_owned(),
    }];
    addons
        .iter()
        .flat_map(|addon| {
            addon
                .manifest
                .catalogs
                .iter()
                .filter(|c| c.extra.iter().any(|e| e.name == "search"))
                .filter(|c| c.is_extra_supported(&extra))
                .map(|c| Slot {
                    row_id: format!("{}:{}:{}", addon.id(), c.r#type, c.id),
                    title: type_label(&c.r#type).to_owned(),
                    addon: addon.clone(),
                    path: ResourcePath {
                        resource: "catalog".into(),
                        r#type: c.r#type.clone(),
                        id: c.id.clone(),
                        extra: extra.clone(),
                    },
                })
        })
        .collect()
}

async fn run(core: Core, query: String, snapshot: SharedObservable<SearchSnapshot>) {
    tokio::time::sleep(DEBOUNCE).await;
    let slots = search_slots(&core.installed_addons().unwrap_or_default(), &query);
    let mut rows: Vec<(RowState, Vec<Tile>)> = vec![(RowState::Loading, Vec::new()); slots.len()];
    let publish = |rows: &[(RowState, Vec<Tile>)]| {
        snapshot.set(SearchSnapshot {
            query: query.clone(),
            rows: render(&slots, rows),
            recent: Vec::new(),
        })
    };
    publish(&rows);
    let mut calls = JoinSet::new();
    for (index, slot) in slots.iter().cloned().enumerate() {
        let client = core.addons().clone();
        calls.spawn(async move {
            let result = client.fetch(&slot.addon, &slot.path, HARD_DEADLINE).await;
            (index, result)
        });
    }
    let soft = tokio::time::sleep(SOFT_DEADLINE);
    tokio::pin!(soft);
    let mut soft_fired = false;
    loop {
        tokio::select! {
            joined = calls.join_next() => {
                let Some(joined) = joined else { break };
                let Ok((index, result)) = joined else { continue };
                rows[index] = outcome(result, &query);
                publish(&rows);
            }
            _ = &mut soft, if !soft_fired => {
                soft_fired = true;
                for row in rows.iter_mut().filter(|r| r.0 == RowState::Loading) {
                    row.0 = RowState::Slow;
                }
                publish(&rows);
            }
        }
    }
}

fn outcome(
    result: Result<mq_addons::Fetched, mq_addons::AddonError>,
    query: &str,
) -> (RowState, Vec<Tile>) {
    let parsed = result.map_err(|e| e.to_string()).and_then(|f| {
        serde_json::from_slice::<ResourceResponse>(&f.body)
            .map_err(|e| format!("unreadable results: {e}"))
    });
    match parsed {
        Ok(ResourceResponse::Metas { metas }) => {
            let mut tiles: Vec<Tile> = metas.into_iter().map(tile_from_preview).collect();
            // Addon order is kept, except that a title named exactly what was
            // typed goes first (Cinemeta ranks Pixar's "Up" sixth for "up").
            let wanted = fold(query);
            tiles.sort_by_key(|t| fold(&t.name) != wanted);
            (RowState::Ready, tiles)
        }
        Ok(_) => (
            RowState::Failed {
                reason: "addon returned no search results".into(),
            },
            Vec::new(),
        ),
        Err(reason) => (RowState::Failed { reason }, Vec::new()),
    }
}

/// Rows in slot order with titles already shown above removed; rows left
/// with nothing are dropped.
fn render(slots: &[Slot], rows: &[(RowState, Vec<Tile>)]) -> Vec<HomeRow> {
    let mut seen = HashSet::new();
    slots
        .iter()
        .zip(rows)
        .filter_map(|(slot, (state, tiles))| {
            let tiles: Vec<Tile> = tiles
                .iter()
                .filter(|t| seen.insert(t.id.clone()))
                .cloned()
                .collect();
            if *state == RowState::Ready && tiles.is_empty() {
                return None;
            }
            Some(HomeRow {
                row_id: slot.row_id.clone(),
                title: slot.title.clone(),
                source: slot.addon.manifest.name.clone(),
                state: state.clone(),
                tiles,
            })
        })
        .collect()
}

/// Lowercase letters and digits only, words joined by single spaces.
fn fold(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
