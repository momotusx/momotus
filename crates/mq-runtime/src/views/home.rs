//! Home: Continue Watching first, then one row per addon catalog in the
//! user's addon order. Row positions are fixed at open so focus never jumps
//! while catalogs arrive; empty and failed rows stay as explained states.

use crate::live::{FRAME, ListDiff, coalesce};
use crate::views::playback::{NextEpisode, next_after};
use crate::views::settings::HomeRows;
use crate::{Core, CoreError};
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
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
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
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RowState {
    Loading,
    Slow,
    Ready,
    Empty,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct HomeRow {
    pub row_id: String,
    pub title: String,
    pub source: String,
    pub state: RowState,
    pub tiles: Vec<Tile>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct HomeHeader {
    pub hero: Option<Hero>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Hero {
    pub tile: Tile,
    /// One line telling the user why this title is here.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
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
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct HomeSnapshot {
    pub header: HomeHeader,
    pub rows: Vec<HomeRow>,
}

impl HomeView {
    pub fn open(core: &Core) -> Self {
        // Subscribed before reading, so no addon change falls in between.
        let changes = core.library().subscribe();
        let addons = core.installed_addons().unwrap_or_default();
        let slots = shown(arrange(catalog_slots(&addons), &core.prefs().home_rows));
        let ids = slots.iter().map(|s| s.row_id.clone()).collect();
        let mut initial = ObservableVector::new();
        for slot in &slots {
            initial.push_back(loading_row(slot));
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
        tasks.spawn(watch_addons(
            core.clone(),
            changes,
            ids,
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
                row_id: format!(
                    "{}@{}:{}:{}",
                    addon.id(),
                    install_tag(addon.transport_url.as_str()),
                    catalog.r#type,
                    catalog.id
                ),
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

/// The user's order first (ids that still exist, first mention wins), then
/// every other catalog in addon order; each with whether it is hidden.
fn arrange(slots: Vec<CatalogSlot>, prefs: &HomeRows) -> Vec<(CatalogSlot, bool)> {
    let rank = |id: &str| prefs.order.iter().position(|o| o == id);
    let mut slots: Vec<(usize, CatalogSlot)> = slots.into_iter().enumerate().collect();
    // Stable: unordered rows keep addon order after all ordered ones.
    slots.sort_by_key(|(i, s)| (rank(&s.row_id).unwrap_or(usize::MAX), *i));
    slots
        .into_iter()
        .map(|(_, s)| {
            let hidden = prefs.hidden.contains(&s.row_id);
            (s, hidden)
        })
        .collect()
}

fn shown(arranged: Vec<(CatalogSlot, bool)>) -> Vec<CatalogSlot> {
    arranged
        .into_iter()
        .filter_map(|(s, hidden)| (!hidden).then_some(s))
        .collect()
}

/// One catalog as the "Customize Home" screen lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct HomeCatalog {
    pub row_id: String,
    pub title: String,
    pub source: String,
    pub hidden: bool,
}

impl Core {
    /// Hides or shows one Home row. Only `homeRows.hidden` is written, and
    /// ids of catalogs that are gone stay in it.
    pub fn set_home_row_hidden(
        &self,
        row_id: &str,
        hidden: bool,
    ) -> Result<Vec<HomeCatalog>, CoreError> {
        let mut list = self.prefs().home_rows.hidden;
        list.retain(|id| id != row_id);
        if hidden {
            list.push(row_id.to_owned());
        }
        self.set_prefs(&serde_json::json!({"homeRows": {"hidden": list}}))?;
        Ok(self.home_catalogs())
    }

    /// Moves one Home row `by` places (negative is up) among all Home rows,
    /// hidden ones included. Only `homeRows.order` is written; ids of
    /// catalogs that are gone are kept after the rest. Two devices moving
    /// rows at once: the later order wins as a whole.
    pub fn move_home_row(&self, row_id: &str, by: i64) -> Result<Vec<HomeCatalog>, CoreError> {
        let mut ids: Vec<String> = self.home_catalogs().into_iter().map(|c| c.row_id).collect();
        let Some(from) = ids.iter().position(|id| id == row_id) else {
            return Err(CoreError::BadPrefs(format!("no Home row {row_id}")));
        };
        let to = (from as i64)
            .saturating_add(by)
            .clamp(0, ids.len() as i64 - 1) as usize;
        let moved = ids.remove(from);
        ids.insert(to, moved);
        let gone = self
            .prefs()
            .home_rows
            .order
            .into_iter()
            .filter(|id| !ids.contains(id));
        let order: Vec<String> = ids.iter().cloned().chain(gone).collect();
        self.set_prefs(&serde_json::json!({"homeRows": {"order": order}}))?;
        Ok(self.home_catalogs())
    }

    /// Every Home catalog in the order Home uses, hidden ones included where
    /// they would sit.
    pub fn home_catalogs(&self) -> Vec<HomeCatalog> {
        let slots = catalog_slots(&self.installed_addons().unwrap_or_default());
        arrange(slots, &self.prefs().home_rows)
            .into_iter()
            .map(|(slot, hidden)| HomeCatalog {
                title: row_title(&slot),
                source: slot.addon.manifest.name.clone(),
                row_id: slot.row_id,
                hidden,
            })
            .collect()
    }
}

/// Home follows addon changes and the user's row choices (a preference).
async fn home_changed(changes: &mut tokio::sync::broadcast::Receiver<Vec<Table>>) -> bool {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        match changes.recv().await {
            Ok(tables)
                if tables
                    .iter()
                    .any(|t| matches!(t, Table::Addons | Table::Settings)) =>
            {
                return true;
            }
            Ok(_) => {}
            Err(RecvError::Lagged(_)) => return true,
            Err(RecvError::Closed) => return false,
        }
    }
}

/// Tells installations of the same addon apart (two configurations share a
/// manifest id) without putting the URL, which can hold keys, in the row id.
fn install_tag(transport_url: &str) -> String {
    // FNV-1a, 64 bit: stable across versions and platforms.
    let hash = transport_url
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
    format!("{:08x}", hash >> 32)
}

fn loading_row(slot: &CatalogSlot) -> HomeRow {
    HomeRow {
        row_id: slot.row_id.clone(),
        title: row_title(slot),
        source: slot.addon.manifest.name.clone(),
        state: RowState::Loading,
        tiles: Vec::new(),
    }
}

/// Follows the installed addons: rows of catalogs that stay keep their tiles
/// (so nothing flashes), new catalogs load, removed ones go, and the order
/// follows the addon order. The swap is one batch of diffs.
async fn watch_addons(
    core: Core,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    mut ids: Vec<String>,
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
    header: SharedObservable<HomeHeader>,
) {
    let mut loads = JoinSet::new();
    while home_changed(&mut changes).await {
        let slots = shown(arrange(
            catalog_slots(&core.installed_addons().unwrap_or_default()),
            &core.prefs().home_rows,
        ));
        let next: Vec<String> = slots.iter().map(|s| s.row_id.clone()).collect();
        if next == ids {
            continue;
        }
        ids = next;
        let mut fresh = Vec::new();
        {
            let mut rows = lock(&rows);
            let mut swapped: Vec<HomeRow> = find_row(&rows, CONTINUE_ROW_ID)
                .map(|i| rows[i].clone())
                .into_iter()
                .collect();
            for slot in slots {
                match find_row(&rows, &slot.row_id) {
                    Some(i) => swapped.push(rows[i].clone()),
                    None => {
                        swapped.push(loading_row(&slot));
                        fresh.push(slot);
                    }
                }
            }
            rows.clear();
            rows.append(swapped.into_iter().collect());
        }
        // A hero from a catalog that went away goes with it.
        let hero = header.get().hero.map(|h| h.tile.id);
        let shown = hero.is_some_and(|id| {
            lock(&rows)
                .iter()
                .any(|r| r.tiles.iter().any(|t| t.id == id))
        });
        if !shown {
            header.set(HomeHeader { hero: None });
        }
        maybe_pick_hero(&rows, &header);
        // Hiding, showing or moving rows that kept their tiles loads nothing.
        if !fresh.is_empty() {
            loads.spawn(load_catalogs(
                core.clone(),
                fresh,
                rows.clone(),
                header.clone(),
            ));
        }
        while loads.try_join_next().is_some() {}
    }
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

pub(crate) fn type_label(kind: &str) -> &str {
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
    let mine: Vec<String> = slots.iter().map(|s| s.row_id.clone()).collect();
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
                    .filter(|(_, r)| r.state == RowState::Loading && mine.contains(&r.row_id))
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
    update_row(rows, row_id, |row| {
        // A row shown again loads again. The first answer fills it; a later
        // success may still replace a failure (an older load that failed
        // first), but nothing replaces tiles.
        let open = matches!(row.state, RowState::Loading | RowState::Slow);
        let recovers = matches!(row.state, RowState::Failed { .. })
            && matches!(outcome, Ok(ResourceResponse::Metas { .. }));
        if !open && !recovers {
            return;
        }
        match outcome {
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
        }
    });
}

pub(crate) fn tile_from_preview(m: mq_protocol::MetaItemPreview) -> Tile {
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
        // Continue Watching is the first row already; the hero repeating its
        // first tile reads as a mistake (design audit), so it features a catalog.
        .filter(|row| row.row_id != CONTINUE_ROW_ID)
        .find_map(|row| {
            let tile = row.tiles.iter().find(|t| t.background.is_some())?;
            Some(Hero {
                tile: tile.clone(),
                reason: format!("#1 in {} on {}", row.title, row.source),
            })
        });
    let current = header.get().hero;
    let next = match (candidate, &current) {
        (Some(_), Some(existing)) => Some(existing.clone()),
        (Some(hero), None) => Some(hero),
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
    refresh_continue(&core, &rows).await;
    maybe_pick_hero(&rows, &header);
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables
            .iter()
            .any(|t| matches!(t, Table::Progress | Table::Watched | Table::LibraryItems))
        {
            refresh_continue(&core, &rows).await;
            maybe_pick_hero(&rows, &header);
        }
    }
}

/// Continue Watching: unfinished videos plus Next Up (the released episode
/// after a finished one), newest activity first.
// ponytail: runs the indexed queries on the async thread; each is a single
// LIMIT 20 read, move to spawn_blocking if traces show it on the hot path.
async fn refresh_continue(core: &Core, rows: &Mutex<ObservableVector<HomeRow>>) {
    let library = core.library();
    let unfinished = library
        .continue_watching(core.profile(), ROW_TILES)
        .unwrap_or_default();
    let finished = library
        .finished_series(core.profile(), ROW_TILES)
        .unwrap_or_default();
    // A new row shows at once from the library, so it never pushes the page
    // down late; artwork and Next Up follow when the metas answer.
    if find_row(&lock(rows), CONTINUE_ROW_ID).is_none() {
        let quick = unfinished
            .iter()
            .map(|item| (item.updated_ms, tile_from_continue(item.clone(), None)))
            .collect();
        apply_continue(rows, quick);
    }
    // Metas come from the addon cache after the first look; they give the
    // tiles their artwork (the episode's still, else the backdrop).
    let mut items: Vec<(i64, Tile)> =
        futures_util::future::join_all(unfinished.into_iter().map(|item| async move {
            let meta = core.fetch_meta(&item.kind, &item.item_id).await.ok();
            (
                item.updated_ms,
                tile_from_continue(item, meta.as_ref().map(|m| &m.0)),
            )
        }))
        .await;
    let next_up = futures_util::future::join_all(finished.into_iter().map(|done| async move {
        let (meta, _) = core.fetch_meta("series", &done.item_id).await.ok()?;
        let next = next_after(&meta, &done.video_id)?;
        Some((done.updated_ms, tile_for_next(&meta, next)))
    }))
    .await;
    items.extend(next_up.into_iter().flatten());
    apply_continue(rows, items);
}

/// Puts the newest tiles in the Continue Watching row, adding or removing
/// the row as needed.
fn apply_continue(rows: &Mutex<ObservableVector<HomeRow>>, mut items: Vec<(i64, Tile)>) {
    items.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let tiles: Vec<Tile> = items.into_iter().take(ROW_TILES).map(|(_, t)| t).collect();
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

fn tile_for_next(meta: &mq_protocol::MetaItem, next: NextEpisode) -> Tile {
    let p = &meta.preview;
    Tile {
        id: p.id.clone(),
        kind: p.r#type.clone(),
        name: p.name.clone(),
        poster: p.poster.as_ref().map(|u| u.to_string()),
        background: next
            .thumbnail
            .clone()
            .or_else(|| p.background.as_ref().map(|u| u.to_string())),
        logo: p.logo.as_ref().map(|u| u.to_string()),
        poster_shape: "poster".into(),
        release_info: Some(format!("Next: S{}:E{}", next.season, next.episode)),
        progress: None,
        video_id: Some(next.video_id),
    }
}

/// The one caption every app shows under a Continue Watching tile:
/// "S2:E3 · 36 min left" for an episode, "36 min left" for a movie.
fn continue_caption(item: &ContinueItem) -> String {
    let mut parts = item.video_id.rsplit(':');
    let episode = parts.next().and_then(|e| e.parse::<u32>().ok());
    let season = parts.next().and_then(|s| s.parse::<u32>().ok());
    let left = if item.duration_ms > item.position_ms {
        let minutes = (item.duration_ms - item.position_ms + 59_999) / 60_000;
        format!("{minutes} min left")
    } else {
        "In progress".to_owned()
    };
    match (season, episode) {
        (Some(s), Some(e)) if item.kind == "series" => format!("S{s}:E{e} \u{b7} {left}"),
        _ => left,
    }
}

fn tile_from_continue(item: ContinueItem, meta: Option<&mq_protocol::MetaItem>) -> Tile {
    let progress = (item.duration_ms > 0)
        .then(|| (item.position_ms as f32 / item.duration_ms as f32).clamp(0.0, 1.0));
    let caption = continue_caption(&item);
    let still = meta
        .and_then(|m| m.videos.iter().find(|v| v.id == item.video_id))
        .and_then(|v| v.thumbnail.clone());
    let preview = meta.map(|m| &m.preview);
    Tile {
        id: item.item_id,
        kind: item.kind,
        name: item.name,
        poster: item.poster.or_else(|| {
            preview
                .and_then(|p| p.poster.as_ref())
                .map(|u| u.to_string())
        }),
        background: still.or_else(|| {
            preview
                .and_then(|p| p.background.as_ref())
                .map(|u| u.to_string())
        }),
        logo: preview.and_then(|p| p.logo.as_ref()).map(|u| u.to_string()),
        poster_shape: "poster".into(),
        release_info: Some(caption),
        progress,
        video_id: Some(item.video_id),
    }
}
