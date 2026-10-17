//! Home: Continue Watching first, then one row per addon catalog in the
//! user's addon order. Row positions are fixed at open so focus never jumps
//! while catalogs arrive; empty and failed rows stay as explained states.

use crate::Core;
use crate::live::{FRAME, ListDiff, coalesce};
use eyeball::SharedObservable;
use eyeball_im::ObservableVector;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::{ContinueItem, Table};
use mq_protocol::{ResourcePath, ResourceResponse};
use serde::Serialize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;

pub const CONTINUE_ROW_ID: &str = "continue-watching";
const ROW_TILES: usize = 20;
const SOFT_DEADLINE: Duration = Duration::from_millis(2500);
const HARD_DEADLINE: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tile {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub poster: Option<String>,
    pub background: Option<String>,
    pub logo: Option<String>,
    pub poster_shape: String,
    pub release_info: Option<String>,
    /// 0.0 to 1.0 for items in progress.
    pub progress: Option<f32>,
    /// Video to resume for Continue Watching tiles.
    pub video_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RowState {
    Loading,
    Slow,
    Ready,
    Empty,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HomeRow {
    pub row_id: String,
    pub title: String,
    pub source: String,
    pub state: RowState,
    pub tiles: Vec<Tile>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HomeHeader {
    pub hero: Option<Hero>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hero {
    pub tile: Tile,
    /// One line telling the user why this title is here.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HomeUpdate {
    Header { header: Box<HomeHeader> },
    Rows { diffs: Vec<ListDiff<HomeRow>> },
}

struct CatalogSlot {
    row_id: String,
    addon: InstalledAddon,
    path: ResourcePath,
}

/// A live Home screen. Dropping it cancels all of its work.
pub struct HomeView {
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
    header: SharedObservable<HomeHeader>,
    _tasks: JoinSet<()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HomeSnapshot {
    pub header: HomeHeader,
    pub rows: Vec<HomeRow>,
}

impl HomeView {
    pub fn open(core: &Core) -> Self {
        let addons = core.installed_addons().unwrap_or_default();
        let slots = catalog_slots(&addons);
        let mut initial = ObservableVector::new();
        for slot in &slots {
            initial.push_back(HomeRow {
                row_id: slot.row_id.clone(),
                title: row_title(slot),
                source: slot.addon.manifest.name.clone(),
                state: RowState::Loading,
                tiles: Vec::new(),
            });
        }
        let rows = Arc::new(Mutex::new(initial));
        let header = SharedObservable::new(HomeHeader::default());
        let mut tasks = JoinSet::new();
        tasks.spawn(watch_continue(core.clone(), rows.clone(), header.clone()));
        tasks.spawn(load_catalogs(
            core.clone(),
            slots,
            rows.clone(),
            header.clone(),
        ));
        Self {
            rows,
            header,
            _tasks: tasks,
        }
    }

    pub fn snapshot(&self) -> HomeSnapshot {
        HomeSnapshot {
            header: self.header.get(),
            rows: lock(&self.rows).iter().cloned().collect(),
        }
    }

    /// Snapshot plus a stream of coalesced updates (at most one per frame).
    pub fn subscribe(
        &self,
    ) -> (
        HomeSnapshot,
        Pin<Box<dyn Stream<Item = Vec<HomeUpdate>> + Send>>,
    ) {
        let (values, row_stream) = lock(&self.rows)
            .subscribe()
            .into_values_and_batched_stream();
        let header_stream = self.header.subscribe();
        let snapshot = HomeSnapshot {
            header: self.header.get(),
            rows: values.into_iter().collect(),
        };
        let rows = row_stream.map(|batch| {
            vec![HomeUpdate::Rows {
                diffs: batch.into_iter().map(ListDiff::from).collect(),
            }]
        });
        let headers = header_stream.map(|header| {
            vec![HomeUpdate::Header {
                header: Box::new(header),
            }]
        });
        let merged = futures_util::stream::select(rows, headers);
        (snapshot, Box::pin(coalesce(Box::pin(merged), FRAME)))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Board catalogs: those whose required extras all have defaults (search-only
/// catalogs are skipped), in addon then catalog order.
fn catalog_slots(addons: &[InstalledAddon]) -> Vec<CatalogSlot> {
    let mut slots = Vec::new();
    for addon in addons {
        for catalog in &addon.manifest.catalogs {
            let Some(extra) = catalog.default_required_extra() else {
                continue;
            };
            slots.push(CatalogSlot {
                row_id: format!("{}:{}:{}", addon.id(), catalog.r#type, catalog.id),
                addon: addon.clone(),
                path: ResourcePath {
                    resource: "catalog".into(),
                    r#type: catalog.r#type.clone(),
                    id: catalog.id.clone(),
                    extra,
                },
            });
        }
    }
    slots
}

fn row_title(slot: &CatalogSlot) -> String {
    let name = slot
        .addon
        .manifest
        .catalogs
        .iter()
        .find(|c| c.id == slot.path.id && c.r#type == slot.path.r#type)
        .and_then(|c| c.name.clone())
        .unwrap_or_else(|| slot.path.id.clone());
    format!("{name} {}", type_label(&slot.path.r#type))
}

fn type_label(kind: &str) -> &str {
    match kind {
        "movie" => "Movies",
        "series" => "Series",
        "channel" => "Channels",
        "tv" => "Live TV",
        "anime" => "Anime",
        other => other,
    }
}

fn find_row(rows: &ObservableVector<HomeRow>, row_id: &str) -> Option<usize> {
    rows.iter().position(|r| r.row_id == row_id)
}

fn update_row(rows: &Mutex<ObservableVector<HomeRow>>, row_id: &str, f: impl FnOnce(&mut HomeRow)) {
    let mut rows = lock(rows);
    if let Some(index) = find_row(&rows, row_id) {
        let mut row = rows[index].clone();
        f(&mut row);
        if row != rows[index] {
            rows.set(index, row);
        }
    }
}

async fn load_catalogs(
    core: Core,
    slots: Vec<CatalogSlot>,
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
    header: SharedObservable<HomeHeader>,
) {
    let mut calls = JoinSet::new();
    for slot in slots {
        let client = core.addons().clone();
        calls.spawn(async move {
            let result = client.fetch(&slot.addon, &slot.path, HARD_DEADLINE).await;
            (slot.row_id, result)
        });
    }
    let soft = tokio::time::sleep(SOFT_DEADLINE);
    tokio::pin!(soft);
    let mut soft_fired = false;
    loop {
        tokio::select! {
            joined = calls.join_next() => {
                let Some(joined) = joined else { break };
                let Ok((row_id, result)) = joined else { continue };
                apply_catalog(&rows, &row_id, result);
                maybe_pick_hero(&rows, &header);
            }
            _ = &mut soft, if !soft_fired => {
                soft_fired = true;
                let mut rows = lock(&rows);
                let slow: Vec<usize> = rows.iter().enumerate()
                    .filter(|(_, r)| r.state == RowState::Loading)
                    .map(|(i, _)| i)
                    .collect();
                for index in slow {
                    let mut row = rows[index].clone();
                    row.state = RowState::Slow;
                    rows.set(index, row);
                }
            }
        }
    }
}

fn apply_catalog(
    rows: &Mutex<ObservableVector<HomeRow>>,
    row_id: &str,
    result: Result<mq_addons::Fetched, mq_addons::AddonError>,
) {
    let outcome = result.map_err(|e| e.to_string()).and_then(|f| {
        serde_json::from_slice::<ResourceResponse>(&f.body)
            .map_err(|e| format!("unreadable catalog: {e}"))
    });
    update_row(rows, row_id, |row| match outcome {
        Ok(ResourceResponse::Metas { metas }) => {
            row.tiles = metas
                .into_iter()
                .take(ROW_TILES)
                .map(tile_from_preview)
                .collect();
            row.state = if row.tiles.is_empty() {
                RowState::Empty
            } else {
                RowState::Ready
            };
        }
        Ok(_) => {
            row.state = RowState::Failed {
                reason: "addon returned no catalog items".into(),
            }
        }
        Err(reason) => row.state = RowState::Failed { reason },
    });
}

fn tile_from_preview(m: mq_protocol::MetaItemPreview) -> Tile {
    Tile {
        id: m.id,
        kind: m.r#type,
        name: m.name,
        poster: m.poster.map(|u| u.to_string()),
        background: m.background.map(|u| u.to_string()),
        logo: m.logo.map(|u| u.to_string()),
        poster_shape: serde_json::to_value(&m.poster_shape)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "poster".into()),
        release_info: m.release_info,
        progress: None,
        video_id: None,
    }
}

/// The hero is the newest Continue Watching item with artwork, else the first
/// catalog item with a background. Once a catalog hero is shown it stays put
/// while later rows arrive, so the screen does not shift under the user; only a
/// Continue Watching item may replace it.
fn maybe_pick_hero(rows: &Mutex<ObservableVector<HomeRow>>, header: &SharedObservable<HomeHeader>) {
    let rows = lock(rows);
    // Walk rows in display order and stop at the first one still loading, so
    // the hero is deterministic and never taken from a row further down just
    // because it answered first. Slow, failed and empty rows are skipped.
    let candidate = rows
        .iter()
        .take_while(|row| row.state != RowState::Loading)
        .find_map(|row| {
            let tile = row.tiles.iter().find(|t| t.background.is_some())?;
            let from_continue = row.row_id == CONTINUE_ROW_ID;
            let reason = if from_continue {
                "Pick up where you left off".to_owned()
            } else {
                format!("#1 in {} on {}", row.title, row.source)
            };
            Some((
                from_continue,
                Hero {
                    tile: tile.clone(),
                    reason,
                },
            ))
        });
    let current = header.get().hero;
    let next = match (candidate, &current) {
        (Some((true, hero)), _) => Some(hero),
        (Some((false, _)), Some(existing)) => Some(existing.clone()),
        (Some((false, hero)), None) => Some(hero),
        (None, _) => None,
    };
    if current != next {
        header.set(HomeHeader { hero: next });
    }
}

async fn watch_continue(
    core: Core,
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
    header: SharedObservable<HomeHeader>,
) {
    let mut changes = core.library().subscribe();
    refresh_continue(&core, &rows);
    maybe_pick_hero(&rows, &header);
    while let Ok(tables) = changes.recv().await {
        if tables
            .iter()
            .any(|t| matches!(t, Table::Progress | Table::Watched | Table::LibraryItems))
        {
            refresh_continue(&core, &rows);
            maybe_pick_hero(&rows, &header);
        }
    }
}

// ponytail: runs the indexed query on the async thread; it is a single
// LIMIT 20 read, move to spawn_blocking if traces show it on the hot path.
fn refresh_continue(core: &Core, rows: &Mutex<ObservableVector<HomeRow>>) {
    let items = core
        .library()
        .continue_watching(core.profile(), ROW_TILES)
        .unwrap_or_default();
    let tiles: Vec<Tile> = items.into_iter().map(tile_from_continue).collect();
    let mut rows = lock(rows);
    let existing = find_row(&rows, CONTINUE_ROW_ID);
    match (existing, tiles.is_empty()) {
        (Some(index), true) => {
            rows.remove(index);
        }
        (Some(index), false) => {
            if rows[index].tiles != tiles {
                let mut row = rows[index].clone();
                row.tiles = tiles;
                rows.set(index, row);
            }
        }
        (None, false) => rows.push_front(HomeRow {
            row_id: CONTINUE_ROW_ID.into(),
            title: "Continue Watching".into(),
            source: "Library".into(),
            state: RowState::Ready,
            tiles,
        }),
        (None, true) => {}
    }
}

fn tile_from_continue(item: ContinueItem) -> Tile {
    let progress = (item.duration_ms > 0)
        .then(|| (item.position_ms as f32 / item.duration_ms as f32).clamp(0.0, 1.0));
    Tile {
        id: item.item_id,
        kind: item.kind,
        name: item.name,
        poster: item.poster,
        background: None,
        logo: None,
        poster_shape: "poster".into(),
        release_info: None,
        progress,
        video_id: Some(item.video_id),
    }
}
