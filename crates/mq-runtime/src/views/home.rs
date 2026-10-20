//! Home: Continue Watching first, then one row per addon catalog in the
//! user's addon order. Row positions are fixed at open so focus never jumps
//! while catalogs arrive; empty and failed rows stay as explained states.
//!
//! A kids profile's Home asks no catalog and has no hero: Continue Watching
//! of allowed titles, then its list's 20 newest (plans/m11-kids.md 5).

use crate::live::{FRAME, ListDiff, coalesce};
use crate::views::playback::{NextEpisode, next_after};
use crate::views::settings::HomeRows;
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use eyeball_im::ObservableVector;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::{AllowedRow, ContinueItem, Policy, Table};
use mq_protocol::{ResourcePath, ResourceResponse};
use serde::Serialize;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinSet;

pub const CONTINUE_ROW_ID: &str = "continue-watching";
/// A kids profile's own titles, on Home and in Search.
pub const KIDS_ROW_ID: &str = "kids-titles";
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
    /// The IMDb rating the addon gave, as written ("8.4").
    pub rating: Option<String>,
    /// What the hero shows beyond the tile; kept off the wire for rows.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub(crate) blurb: Option<Blurb>,
}

/// A catalog item's synopsis, genres and runtime, for when it becomes the hero.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Blurb {
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub runtime: Option<String>,
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
    pub description: Option<String>,
    /// At most three, in the addon's order.
    pub genres: Vec<String>,
    pub runtime: Option<String>,
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
    core: Core,
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
    header: SharedObservable<HomeHeader>,
    _tasks: JoinSet<()>,
    /// Rows loading again (`retry_row`).
    retries: Mutex<JoinSet<()>>,
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
        let kids = core.policy().kids();
        let access = core.library().subscribe();
        let slots = board(core);
        let ids = slots.iter().map(|s| s.row_id.clone()).collect();
        let mut initial = ObservableVector::new();
        if kids {
            initial.push_back(kids_row(core));
        }
        for slot in &slots {
            initial.push_back(loading_row(slot));
        }
        let rows = Arc::new(Mutex::new(initial));
        let header = SharedObservable::new(HomeHeader::default());
        let mut tasks = JoinSet::new();
        if kids {
            tasks.spawn(watch_kids_row(core.clone(), access, rows.clone()));
        }
        tasks.spawn(watch_continue(
            core.clone(),
            kids,
            rows.clone(),
            header.clone(),
        ));
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
            core: core.clone(),
            rows,
            header,
            _tasks: tasks,
            retries: Mutex::default(),
        }
    }

    /// "Try again" on one catalog row (Apple journey review r2 P3-8): only
    /// that catalog is asked again. The row shows loading, then the answer;
    /// a retry that fails leaves the row failed. False when no shown
    /// catalog row has this id.
    pub fn retry_row(&self, row_id: &str) -> bool {
        let Some(slot) = board(&self.core).into_iter().find(|s| s.row_id == row_id) else {
            return false;
        };
        update_row(&self.rows, row_id, |row| {
            row.state = RowState::Loading;
            row.tiles.clear();
        });
        let mut retries = lock(&self.retries);
        while retries.try_join_next().is_some() {}
        retries.spawn(load_catalogs(
            self.core.clone(),
            vec![slot],
            self.rows.clone(),
            self.header.clone(),
        ));
        true
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

/// The catalog rows Home shows: none for a kids profile, so it never asks
/// an addon for a catalog (plans/m11-kids.md 14.1).
fn board(core: &Core) -> Vec<CatalogSlot> {
    if core.policy().kids() {
        return Vec::new();
    }
    let addons = core.installed_addons().unwrap_or_default();
    shown(arrange(catalog_slots(&addons), &core.prefs().home_rows))
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
    /// "Remove from Continue Watching": the title leaves Continue Watching
    /// and Next Up on every synced device until it is played again (journey
    /// reviews 2026-10-20, web P2-10, Apple P2-3, Android P2-1).
    pub fn remove_from_continue(&self, item_id: &str) -> Result<(), CoreError> {
        self.library()
            .dismiss(self.profile(), item_id, self.clock())?;
        Ok(())
    }

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
        let slots = board(&core);
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
    let links = |category: &str| -> Vec<String> {
        m.links
            .iter()
            .filter(|l| l.category.eq_ignore_ascii_case(category))
            .map(|l| l.name.clone())
            .collect()
    };
    let rating = links("imdb").into_iter().next();
    let blurb = Blurb {
        description: m.description.clone(),
        genres: links("Genres").into_iter().take(3).collect(),
        runtime: m.runtime.clone(),
    };
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
        rating,
        blurb: Some(blurb),
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
        // first tile reads as a mistake (design audit), so it features a
        // catalog. A kids Home has none, and its own tiles carry no
        // background, so it has no hero.
        .filter(|row| row.row_id != CONTINUE_ROW_ID)
        .find_map(|row| {
            let tile = row.tiles.iter().find(|t| t.background.is_some())?;
            let blurb = tile.blurb.clone().unwrap_or_default();
            Some(Hero {
                tile: tile.clone(),
                reason: format!("#1 in {} on {}", row.title, row.source),
                description: blurb.description,
                genres: blurb.genres,
                runtime: blurb.runtime,
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
    kids: bool,
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
    header: SharedObservable<HomeHeader>,
) {
    let mut changes = core.library().subscribe();
    refresh_continue(&core, &rows).await;
    maybe_pick_hero(&rows, &header);
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables.iter().any(|t| {
            matches!(t, Table::Progress | Table::Watched | Table::LibraryItems)
                || (kids && *t == Table::Access)
        }) {
            refresh_continue(&core, &rows).await;
            maybe_pick_hero(&rows, &header);
        }
    }
}

/// A kids Home follows the list at once, before any network work: its own
/// row is read again, and Continue Watching and Next Up tiles off the list
/// go (a refresh waiting on metas puts back nothing, `apply_continue`).
async fn watch_kids_row(
    core: Core,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
) {
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables.contains(&Table::Access) {
            let next = kids_row(&core);
            update_row(&rows, KIDS_ROW_ID, |row| *row = next);
            let policy = core.policy();
            let mut rows = lock(&rows);
            if let Some(index) = find_row(&rows, CONTINUE_ROW_ID) {
                let mut row = rows[index].clone();
                row.tiles.retain(|t| {
                    let video = t.video_id.as_deref().unwrap_or(&t.id);
                    shows(&policy, &t.kind, &t.id, video)
                });
                if row.tiles.is_empty() {
                    rows.remove(index);
                } else if row != rows[index] {
                    rows.set(index, row);
                }
            }
        }
    }
}

/// "Your shows and movies": the 20 most recently allowed of the counted list.
fn kids_row(core: &Core) -> HomeRow {
    let list = core.library().allowed_list(core.profile());
    let tiles: Vec<Tile> = list
        .map(|l| l.counted)
        .unwrap_or_default()
        .iter()
        .rev()
        .take(ROW_TILES)
        .map(kids_tile)
        .collect();
    HomeRow {
        row_id: KIDS_ROW_ID.into(),
        title: "Your shows and movies".into(),
        source: "Your list".into(),
        state: if tiles.is_empty() {
            RowState::Empty
        } else {
            RowState::Ready
        },
        tiles,
    }
}

pub(crate) fn kids_tile(row: &AllowedRow) -> Tile {
    Tile {
        id: row.item_id.clone(),
        kind: row.kind.clone(),
        name: row.name.clone(),
        poster: row.poster.clone(),
        background: None,
        logo: None,
        poster_shape: "poster".into(),
        release_info: None,
        progress: None,
        video_id: None,
        rating: None,
        blurb: None,
    }
}

/// Whether a Continue Watching tile may show: the title passes the policy,
/// and for kids its video is one of the title's (a row that pairs an
/// allowed id with another title's video does not).
fn shows(policy: &Policy, kind: &str, id: &str, video: &str) -> bool {
    policy.allows(kind, id) && (!policy.kids() || Policy::belongs(id, video))
}

/// Continue Watching: unfinished videos plus Next Up (the released episode
/// after a finished one), newest activity first. For kids, titles off the
/// list are left out before any meta is asked for, and again when the
/// tiles land (plans/m11-kids.md 4.2).
// ponytail: runs the indexed queries on the async thread; each is a single
// LIMIT 20 read, move to spawn_blocking if traces show it on the hot path.
// A kids profile reads every played title and filters before taking 20, so
// newer plays of titles off the list never crowd out allowed ones.
async fn refresh_continue(core: &Core, rows: &Mutex<ObservableVector<HomeRow>>) {
    let library = core.library();
    let policy = core.policy();
    let read = if policy.kids() {
        usize::MAX >> 1
    } else {
        ROW_TILES
    };
    // A kid's tile wears its allowlist entry's name and poster: a library
    // row (an import, a sync) may pair an allowed id with another title's.
    let entries: HashMap<String, AllowedRow> = if policy.kids() {
        library
            .allowed_list(core.profile())
            .map(|l| l.counted)
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.item_id.clone(), r))
            .collect()
    } else {
        HashMap::new()
    };
    let unfinished: Vec<ContinueItem> = library
        .continue_watching(core.profile(), read)
        .unwrap_or_default()
        .into_iter()
        .filter(|item| shows(&policy, &item.kind, &item.item_id, &item.video_id))
        .take(ROW_TILES)
        .map(|mut item| {
            if let Some(entry) = entries.get(&item.item_id) {
                item.name = entry.name.clone();
                item.poster = entry.poster.clone();
            }
            item
        })
        .collect();
    let finished: Vec<_> = library
        .finished_series(core.profile(), read)
        .unwrap_or_default()
        .into_iter()
        .filter(|done| shows(&policy, &done.kind, &done.item_id, &done.video_id))
        .take(ROW_TILES)
        .collect();
    // A new row shows at once from the library, so it never pushes the page
    // down late; artwork and Next Up follow when the metas answer.
    if find_row(&lock(rows), CONTINUE_ROW_ID).is_none() {
        let quick = unfinished
            .iter()
            .map(|item| (item.updated_ms, tile_from_continue(item.clone(), None)))
            .collect();
        apply_continue(core, rows, quick);
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
        // A series or an anime (Kitsu's kind), read where its kind is.
        let (meta, _) = core.fetch_meta(&done.kind, &done.item_id).await.ok()?;
        let next = next_after(&meta, &done.video_id)?;
        Some((done.updated_ms, tile_for_next(&meta, next)))
    }))
    .await;
    items.extend(next_up.into_iter().flatten());
    apply_continue(core, rows, items);
}

/// Puts the newest tiles in the Continue Watching row, adding or removing
/// the row as needed. The policy is read again here: a title taken off a
/// kids list while its meta was on the way never comes back.
fn apply_continue(
    core: &Core,
    rows: &Mutex<ObservableVector<HomeRow>>,
    mut items: Vec<(i64, Tile)>,
) {
    let policy = core.policy();
    items.retain(|(_, t)| {
        let video = t.video_id.as_deref().unwrap_or(&t.id);
        shows(&policy, &t.kind, &t.id, video)
    });
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
        rating: imdb_rating(p),
        blurb: None,
    }
}

/// The one caption every app shows under a Continue Watching tile:
/// "S2:E3 · 36 min left" for an episode, "36 min left" for a movie.
fn continue_caption(item: &ContinueItem) -> String {
    let left = if item.duration_ms > item.position_ms {
        let minutes = (item.duration_ms - item.position_ms + 59_999) / 60_000;
        format!("{minutes} min left")
    } else {
        "In progress".to_owned()
    };
    with_episode(&item.kind, &item.video_id, left)
}

/// "S2:E3 · <rest>" for an episode of a series, else `rest`.
pub(crate) fn with_episode(kind: &str, video_id: &str, rest: String) -> String {
    let mut parts = video_id.rsplit(':');
    let episode = parts.next().and_then(|e| e.parse::<u32>().ok());
    let season = parts.next().and_then(|s| s.parse::<u32>().ok());
    match (season, episode) {
        (Some(s), Some(e)) if kind == "series" => format!("S{s}:E{e} \u{b7} {rest}"),
        _ => rest,
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
        rating: preview.and_then(imdb_rating),
        blurb: None,
    }
}

fn imdb_rating(p: &mq_protocol::MetaItemPreview) -> Option<String> {
    p.links
        .iter()
        .find(|l| l.category.eq_ignore_ascii_case("imdb"))
        .map(|l| l.name.clone())
}
