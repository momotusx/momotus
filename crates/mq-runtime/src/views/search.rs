//! Search: one row per addon catalog that supports `search`, in addon order.
//! A title shown in an earlier row is not repeated further down, so the same
//! film from two metadata addons appears once.
//!
//! The query is debounced here, so every UI types into the same behaviour.
//!
//! A kids profile's search asks no addon: it matches the query over its
//! list's names, in one row (plans/m11-kids.md 4.6).
//!
//! Above the addon rows, "Top results" holds the titles Trakt names for the
//! query, read through the installed meta addons (plans/search-quality.md):
//! it finds what an addon's own search misses (Cinemeta leaves out shorts
//! such as Sintel).

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::views::home::{
    HomeRow, KIDS_ROW_ID, RowState, Tile, kids_tile, tile_from_preview, type_label,
};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::Table;
use mq_protocol::{ResourcePath, ResourceResponse};
use mq_tracking::trakt::search::Found;
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
/// Trakt is asked once the text has rested this long (`DEBOUNCE` included).
const TRAKT_SETTLE: Duration = Duration::from_millis(400);
/// The shortest folded query Trakt is asked for.
const TRAKT_MIN_CHARS: usize = 2;
/// The most titles "Top results" leads with.
const TOP_TILES: usize = 5;
pub const TOP_ROW_ID: &str = "top";

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
    /// The installed search catalogs a query goes to ("Top results" not
    /// counted). With none, empty `rows` mean no installed addon can
    /// search, not that nothing matched (web journey review P3-10). Always
    /// 0 for a kids profile, whose search reads its own list.
    pub searchable: u32,
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
            searchable: searchable(core),
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
                searchable: searchable(&self.core),
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
/// newly installed search catalog shows up without retyping, and when a
/// kids list changes.
async fn watch_addons(
    core: Core,
    snapshot: SharedObservable<SearchSnapshot>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    mut changes: Receiver<Vec<Table>>,
) {
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        let again = tables.iter().any(|t| match t {
            Table::Addons => true,
            Table::Access => core.policy().kids(),
            _ => false,
        });
        if !again {
            continue;
        }
        let shown = snapshot.get();
        let query = shown.query.clone();
        if query.is_empty() {
            let now = searchable(&core);
            if shown.searchable != now {
                snapshot.set(SearchSnapshot {
                    searchable: now,
                    ..shown
                });
            }
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

/// Recent searches, none for a kids profile: an old query can name a title
/// since taken off its list.
fn recent(core: &Core) -> Vec<String> {
    if core.policy().kids() {
        return Vec::new();
    }
    core.library()
        .recent_searches(core.profile(), RECENT)
        .unwrap_or_default()
}

/// How many installed search catalogs a query would go to; 0 for a kids
/// profile.
fn searchable(core: &Core) -> u32 {
    if core.policy().kids() {
        return 0;
    }
    // Any text: whether a catalog takes `search` does not hang on it.
    search_slots(&core.installed_addons().unwrap_or_default(), "query").len() as u32
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
    if core.policy().kids() {
        snapshot.set(SearchSnapshot {
            rows: kids_matches(&core, &query),
            query,
            recent: Vec::new(),
            searchable: 0,
        });
        return;
    }
    let slots = search_slots(&core.installed_addons().unwrap_or_default(), &query);
    let mut rows: Vec<(RowState, Vec<Tile>)> = vec![(RowState::Loading, Vec::new()); slots.len()];
    let key = fold(&query);
    let mut top_pending = key.chars().count() >= TRAKT_MIN_CHARS && core.trakt_search_ready();
    let mut top: Option<(RowState, Vec<Tile>)> =
        top_pending.then(|| (RowState::Loading, Vec::new()));
    let publish = |top: &Option<(RowState, Vec<Tile>)>, rows: &[(RowState, Vec<Tile>)]| {
        snapshot.set(SearchSnapshot {
            query: query.clone(),
            rows: render(top.as_ref(), &slots, rows),
            recent: Vec::new(),
            searchable: slots.len() as u32,
        })
    };
    publish(&top, &rows);
    let top_work = tokio::time::timeout(HARD_DEADLINE, top_results(&core, &query, &key));
    tokio::pin!(top_work);
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
        if calls.is_empty() && !top_pending {
            break;
        }
        tokio::select! {
            Some(joined) = calls.join_next(), if !calls.is_empty() => {
                let Ok((index, result)) = joined else { continue };
                rows[index] = outcome(result, &query);
                publish(&top, &rows);
            }
            found = &mut top_work, if top_pending => {
                top_pending = false;
                // A source that failed leaves the row out: search then
                // looks as it does without it.
                top = found.ok().flatten().map(|tiles| (RowState::Ready, tiles));
                publish(&top, &rows);
            }
            _ = &mut soft, if !soft_fired => {
                soft_fired = true;
                for row in rows.iter_mut().chain(top.as_mut()).filter(|r| r.0 == RowState::Loading) {
                    row.0 = RowState::Slow;
                }
                publish(&top, &rows);
            }
        }
    }
}

/// "Top results": the titles Trakt names for the query, led as `lead`
/// picks, each read through the installed meta addons (an id none of them
/// knows is left out). None when Trakt gave nothing. Trakt is asked only
/// once the text has rested `TRAKT_SETTLE`, unless memory holds a fresh
/// answer.
async fn top_results(core: &Core, query: &str, key: &str) -> Option<Vec<Tile>> {
    let hits = match core.trakt_titles_held(key) {
        Some(hits) => hits,
        None => {
            tokio::time::sleep(TRAKT_SETTLE.saturating_sub(DEBOUNCE)).await;
            core.trakt_titles(query, key).await?
        }
    };
    let metas = futures_util::future::join_all(
        lead(&hits, key)
            .into_iter()
            .map(|hit| async move { core.fetch_meta(hit.kind, &hit.imdb_id).await }),
    )
    .await;
    Some(
        metas
            .into_iter()
            .filter_map(Result::ok)
            .map(|(meta, _)| tile_from_preview(meta.preview))
            .collect(),
    )
}

/// What "Top results" leads with: the titles named exactly the query, then
/// those whose name starts with it, in Trakt's order, at most `TOP_TILES`;
/// with none of them, Trakt's first hit (a title typed in another language
/// or misspelt).
fn lead(hits: &[Found], key: &str) -> Vec<Found> {
    let named = |exact: bool| {
        hits.iter().filter(move |hit| {
            let name = fold(&hit.title);
            if exact {
                name == key
            } else {
                name != key && name.starts_with(key)
            }
        })
    };
    let led: Vec<Found> = named(true)
        .chain(named(false))
        .take(TOP_TILES)
        .cloned()
        .collect();
    if led.is_empty() {
        hits.iter().take(1).cloned().collect()
    } else {
        led
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

/// "Top results" first, then the rows in slot order, with titles already
/// shown above removed; rows left with nothing are dropped.
fn render(
    top: Option<&(RowState, Vec<Tile>)>,
    slots: &[Slot],
    rows: &[(RowState, Vec<Tile>)],
) -> Vec<HomeRow> {
    let mut seen = HashSet::new();
    let top = top
        .map(|(state, tiles)| HomeRow {
            row_id: TOP_ROW_ID.into(),
            title: "Top results".into(),
            source: "Trakt".into(),
            state: state.clone(),
            tiles: tiles
                .iter()
                .filter(|t| seen.insert(t.id.clone()))
                .cloned()
                .collect(),
        })
        .filter(|row| !(row.state == RowState::Ready && row.tiles.is_empty()));
    let addons: Vec<HomeRow> = slots
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
        .collect();
    top.into_iter().chain(addons).collect()
}

/// The counted titles whose name or other names hold the folded query, a
/// title named exactly that first; no row when none does.
fn kids_matches(core: &Core, query: &str) -> Vec<HomeRow> {
    let wanted = fold(query);
    let list = core.library().allowed_list(core.profile());
    let mut found: Vec<(bool, Tile)> = list
        .map(|l| l.counted)
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            let names: Vec<String> = std::iter::once(&row.name)
                .chain(&row.names)
                .map(|n| fold(n))
                .collect();
            names
                .iter()
                .any(|n| n.contains(&wanted))
                .then(|| (!names.contains(&wanted), kids_tile(row)))
        })
        .collect();
    found.sort_by_key(|(inexact, _)| *inexact);
    if found.is_empty() {
        return Vec::new();
    }
    vec![HomeRow {
        row_id: KIDS_ROW_ID.into(),
        title: "Your shows and movies".into(),
        source: "Your list".into(),
        state: RowState::Ready,
        tiles: found.into_iter().map(|(_, t)| t).collect(),
    }]
}

/// Lowercase letters and digits only, words joined by single spaces.
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(title: &str) -> Found {
        Found {
            kind: "movie",
            imdb_id: format!("tt-{title}"),
            title: title.into(),
        }
    }

    fn titles(found: Vec<Found>) -> Vec<String> {
        found.into_iter().map(|f| f.title).collect()
    }

    /// Exact names first, then names that start with the query, in Trakt's
    /// order, at most five; with neither, Trakt's first hit alone.
    #[test]
    fn the_lead_is_exact_names_then_prefixes_else_the_first_hit() {
        let hits = [
            "Sintel Rising",
            "Interstellar",
            "Sintel",
            "Sintel: 2",
            "The Sintel",
        ]
        .map(hit);
        assert_eq!(
            titles(lead(&hits, "sintel")),
            ["Sintel", "Sintel Rising", "Sintel: 2"]
        );
        assert_eq!(titles(lead(&hits, "money heist")), ["Sintel Rising"]);
        assert!(lead(&[], "sintel").is_empty());
        let many: Vec<Found> = (0..8).map(|i| hit(&format!("Dune {i}"))).collect();
        assert_eq!(lead(&many, "dune").len(), TOP_TILES);
    }
}
