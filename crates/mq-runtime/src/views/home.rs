//! Home: Continue Watching first, then one row per addon catalog in the
//! user's addon order. Row positions are fixed at open so focus never jumps
//! while catalogs arrive; empty and failed rows stay as explained states.
//!
//! A kids profile's Home asks no catalog and has no hero: Continue Watching
//! of allowed titles, then its list's 20 newest (plans/m11-kids.md 5).

use crate::covers::{Covers, RPDB};
use crate::live::{FRAME, ListDiff, coalesce};
use crate::ratings::Ratings;
use crate::views::playback::{NextEpisode, next_after};
use crate::views::settings::HomeRows;
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use eyeball_im::ObservableVector;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::{AllowedRow, ContinueItem, Policy, Table};
use mq_protocol::{ResourcePath, ResourceResponse, decode_catalog};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
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
    /// For a poster of an IMDb title, RPDB's cover with the ratings band
    /// drawn in (`Tile::rated_cover`); otherwise the addon's poster.
    pub poster: Option<String>,
    /// The addon's own poster, for when `poster` is RPDB's and that image
    /// does not load (an id RPDB does not know answers 404). `None` when
    /// `poster` is the addon's own already.
    pub poster_fallback: Option<String>,
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
    /// The IMDb score to show, from the addon's own rating (`rating`).
    pub ratings: Ratings,
    /// What the hero shows beyond the tile; kept off the wire for rows.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub(crate) blurb: Option<Blurb>,
    /// The shape the addon gave, kept while `rated_cover` shows RPDB's 2:3
    /// cover in its place, so a tile the key no longer covers gets it back.
    #[serde(skip)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub(crate) own_shape: Option<String>,
}

impl Tile {
    /// The cover every tile shows whose title has an IMDb id (its own, or
    /// the one its addon gave), whatever shape its addon gave (the owner,
    /// 2026-10-22), when an enabled addon gave an RPDB key (`covers`):
    /// RPDB's 2:3 poster with the IMDb, Metacritic and Rotten Tomatoes band
    /// drawn in, so its shape becomes "poster"; the addon's own poster is kept in
    /// `poster_fallback` for when it does not load. `?fallback=true` makes
    /// RPDB answer its plain poster for a title it has no ratings for; an
    /// id it does not know still answers 404. Any other tile, or any tile
    /// without a key, keeps the addon's poster and shape. A poster that is
    /// RPDB's already (an app sent a cover back, as `playback`'s `poster`)
    /// is no fallback. Background and still are never touched (Continue
    /// Watching's 16:9 art). Every tile builder ends here.
    pub(crate) fn rated_cover(mut self, covers: &Covers) -> Self {
        if let Some(cover) = covers.url(&self.id) {
            if self.own_shape.is_none() {
                self.own_shape = Some(std::mem::replace(&mut self.poster_shape, "poster".into()));
            }
            self.poster_fallback = self.poster.take().filter(|p| !p.starts_with(RPDB));
            self.poster = Some(cover);
        }
        self
    }

    /// `rated_cover` for a tile built from a stored row (Continue Watching,
    /// Library, a kids list): an RPDB cover stored before (an older play
    /// record) goes when no key draws one now, so the band never outlives
    /// the key. A catalog tile keeps an RPDB poster its addon serves itself.
    pub(crate) fn rated_stored_cover(mut self, covers: &Covers) -> Self {
        self.poster = self.poster.filter(|p| !p.starts_with(RPDB));
        self.rated_cover(covers)
    }

    /// The tile's cover drawn again after the RPDB key changed (`old` and
    /// `new` hold the keys): a cover `old` drew goes back to the addon's
    /// poster and shape, then `new` draws its band, if it has a key. Nothing
    /// else on the tile changes. ponytail: a catalog load in flight when the
    /// key changes lands with the key it read at its start; the next key
    /// change or reload fixes it.
    pub(crate) fn recovered(mut self, old: &Covers, new: &Covers) -> Self {
        if self.poster.is_some() && old.url(&self.id) == self.poster {
            self.poster = self.poster_fallback.take();
            if let Some(shape) = self.own_shape.take() {
                self.poster_shape = shape;
            }
        }
        self.rated_cover(new)
    }
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
    /// One line telling the user why this title is here: "#1 in Popular
    /// Movies", never with the addon's name.
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
                row_id: row_id(addon, &catalog.r#type, &catalog.id),
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
/// every other catalog in addon order; each with whether it is hidden. A
/// twin, a row titled as one before it in addon order (in any case), is
/// hidden until the user shows it: no addon's name tells the two apart (the
/// owner, 2026-10-22). The user's own choice wins either way.
fn arrange(slots: Vec<CatalogSlot>, prefs: &HomeRows) -> Vec<(CatalogSlot, bool)> {
    let mut titles = HashSet::new();
    let twins: Vec<bool> = slots
        .iter()
        .map(|s| !titles.insert(row_title(s).to_lowercase()))
        .collect();
    let rank = |id: &str| prefs.order.iter().position(|o| o == id);
    let mut slots: Vec<(usize, CatalogSlot)> = slots.into_iter().enumerate().collect();
    // Stable: unordered rows keep addon order after all ordered ones.
    slots.sort_by_key(|(i, s)| (rank(&s.row_id).unwrap_or(usize::MAX), *i));
    slots
        .into_iter()
        .map(|(i, s)| {
            let hidden =
                prefs.hidden.contains(&s.row_id) || (twins[i] && !prefs.shown.contains(&s.row_id));
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

    /// Hides or shows one Home row. Only `homeRows.hidden` and
    /// `homeRows.shown` (a twin shown on purpose) are written, and ids of
    /// catalogs that are gone stay in them.
    pub fn set_home_row_hidden(
        &self,
        row_id: &str,
        hidden: bool,
    ) -> Result<Vec<HomeCatalog>, CoreError> {
        let rows = self.prefs().home_rows;
        let (mut on, mut off) = (rows.shown, rows.hidden);
        on.retain(|id| id != row_id);
        off.retain(|id| id != row_id);
        if hidden { &mut off } else { &mut on }.push(row_id.to_owned());
        self.set_prefs(&serde_json::json!({"homeRows": {"hidden": off, "shown": on}}))?;
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

    /// Home's row choices follow an addon whose link changed from `old` to
    /// `new`: the ids made from the old link take the new one's tag. Only
    /// the `homeRows` leaves that change are written, so another device's
    /// edit of the others stands.
    pub(crate) fn move_home_rows(&self, old: &str, new: &str) -> Result<(), CoreError> {
        let (from, to) = (
            format!("@{}:", install_tag(old)),
            format!("@{}:", install_tag(new)),
        );
        let rows = self.prefs().home_rows;
        let mut patch = serde_json::Map::new();
        for (leaf, ids) in [
            ("hidden", &rows.hidden),
            ("order", &rows.order),
            ("shown", &rows.shown),
        ] {
            let moved: Vec<String> = ids.iter().map(|id| id.replacen(&from, &to, 1)).collect();
            if moved != *ids {
                patch.insert(leaf.into(), moved.into());
            }
        }
        if !patch.is_empty() {
            self.set_prefs(&serde_json::json!({ "homeRows": patch }))?;
        }
        Ok(())
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
pub(crate) async fn home_changed(
    changes: &mut tokio::sync::broadcast::Receiver<Vec<Table>>,
) -> bool {
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

/// A Home row's id: the addon, its installation, the kind and the catalog.
pub(crate) fn row_id(addon: &InstalledAddon, kind: &str, catalog: &str) -> String {
    format!(
        "{}@{}:{kind}:{catalog}",
        addon.id(),
        install_tag(addon.transport_url.as_str())
    )
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
    let mut key = core.rpdb_key();
    while home_changed(&mut changes).await {
        while loads.try_join_next().is_some() {}
        // The RPDB key comes from an addon: added, removed or switched, every
        // tile already drawn takes the band as it is now.
        let now = core.rpdb_key();
        if now != key {
            recover_home(&core, key.take(), now.clone(), &rows, &header);
            key = now;
        }
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
    }
}

/// Draws these tiles again after the RPDB key changed from `old` to `new`
/// (`Tile::recovered`): Home's rows and hero, Discover's items.
pub(crate) fn recover_tiles<'a>(
    core: &Core,
    old: Option<String>,
    new: Option<String>,
    tiles: impl IntoIterator<Item = &'a mut Tile>,
) {
    let tiles: Vec<&mut Tile> = tiles.into_iter().collect();
    let ids: Vec<String> = tiles.iter().map(|t| t.id.clone()).collect();
    let old = core.covers_with(old, ids.iter().map(String::as_str));
    let new = core.covers_with(new, ids.iter().map(String::as_str));
    for tile in tiles {
        *tile = tile.clone().recovered(&old, &new);
    }
}

/// Every tile of Home and the hero drawn again with the key `new` in place
/// of `old`, as one batch of row diffs.
fn recover_home(
    core: &Core,
    old: Option<String>,
    new: Option<String>,
    rows: &Arc<Mutex<ObservableVector<HomeRow>>>,
    header: &SharedObservable<HomeHeader>,
) {
    let mut rows = lock(rows);
    let mut next: Vec<HomeRow> = rows.iter().cloned().collect();
    let mut hero = header.get().hero;
    recover_tiles(
        core,
        old,
        new,
        next.iter_mut()
            .flat_map(|r| r.tiles.iter_mut())
            .chain(hero.iter_mut().map(|h| &mut h.tile)),
    );
    for (i, row) in next.into_iter().enumerate() {
        if row != rows[i] {
            rows.set(i, row);
        }
    }
    if hero != header.get().hero {
        header.set(HomeHeader { hero });
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
    catalog_title(&name, &slot.path.r#type, &slot.addon.manifest.name)
}

/// A row's title: the catalog's name and the kind it lists ("Popular Movies"), with its spaces made
/// single (Streaming Catalogs' "Prime  Video"). No addon's name shows on Home, so a name that starts with
/// the addon's name ("trakt - Popular movies" from Trakt Tv) drops that start, and a kind the name already
/// says is not added. A kind Marquee does not know is never added: it is often the addon's own name
/// (Trakt Tv's type "trakt"), and the name says what the row lists. A known kind stays even when the
/// addon's name holds it (The Movie Database Addon's "Popular Movies").
pub(crate) fn catalog_title(name: &str, kind: &str, source: &str) -> String {
    let name = without_source(
        &name.split_whitespace().collect::<Vec<_>>().join(" "),
        source,
    );
    match kind_label(kind) {
        Some(label) if !has_words(&name, label) => format!("{name} {label}"),
        _ => name,
    }
}

/// The hero's caption: "#1 in Popular Movies", never with the addon's name
/// (the owner, 2026-10-22); a twin row is off Home by default, so the title
/// says enough.
fn hero_reason(title: &str) -> String {
    format!("#1 in {title}")
}

/// Lowercase words of letters and digits.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The words that name an addon: its name less the generic ones ("Trakt Tv" is "trakt").
fn source_words(source: &str) -> Vec<String> {
    const GENERIC: [&str; 7] = [
        "tv", "addon", "catalog", "catalogs", "stremio", "the", "for",
    ];
    words(source)
        .into_iter()
        .filter(|w| !GENERIC.contains(&w.as_str()))
        .collect()
}

/// Whether every word of `part` is in `text`, a plural matching its singular ("Random Movie" says "Movies").
fn has_words(text: &str, part: &str) -> bool {
    let stem = |w: &String| w.strip_suffix('s').unwrap_or(w).to_owned();
    let text: Vec<String> = words(text).iter().map(stem).collect();
    let part = words(part);
    !part.is_empty() && part.iter().all(|w| text.contains(&stem(w)))
}

/// A catalog's `name` without a leading addon name and its separator ("trakt - ", "Trakt: ", "Game Cinema
/// \u{2014} "), for a list that names the addon beside it (Home rows, Discover's catalogs). The addon's full
/// name is tried first ("Trakt Tv - Popular"), then its own words ("trakt - Popular"); the name stays as it
/// is unless what is left starts with a letter or digit ("Trakt's Picks", "Trakt.tv Popular").
pub(crate) fn without_source(name: &str, source: &str) -> String {
    [words(source), source_words(source)]
        .iter()
        .find_map(|own| past_words(name, own))
        .unwrap_or_else(|| name.to_owned())
}

/// What follows `own` and one separator at the start of `name`, when it starts with a letter or digit.
fn past_words(name: &str, own: &[String]) -> Option<String> {
    if own.is_empty() {
        return None;
    }
    let mut rest = name.trim_start();
    for word in own {
        let head = rest.get(..word.len())?;
        let after = &rest[word.len()..];
        if head.to_lowercase() != *word || after.starts_with(char::is_alphanumeric) {
            return None;
        }
        rest = after.trim_start();
    }
    let rest = rest
        .trim_start_matches(['-', ':', '|', '\u{b7}', '\u{2013}', '\u{2014}'])
        .trim();
    rest.starts_with(char::is_alphanumeric)
        .then(|| rest.to_owned())
}

/// The label of a kind Marquee knows; `None` for any other type, which is
/// never shown as is (an addon's own type is often its name: Trakt Tv's
/// "trakt").
pub(crate) fn kind_label(kind: &str) -> Option<&'static str> {
    match kind {
        "movie" => Some("Movies"),
        "series" => Some("Series"),
        "channel" => Some("Channels"),
        "tv" => Some("Live TV"),
        "anime" => Some("Anime"),
        _ => None,
    }
}

/// Where what is of no known kind is shown, in Search and Discover.
pub(crate) const OTHER_KIND: (&str, &str) = ("other", "Other");

/// The label of a known kind, else the neutral one (`OTHER_KIND`).
pub(crate) fn type_label(kind: &str) -> &'static str {
    kind_label(kind).unwrap_or(OTHER_KIND.1)
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
                apply_catalog(&core, &rows, &row_id, result);
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

/// Fills the row from the catalog's answer.
fn apply_catalog(
    core: &Core,
    rows: &Mutex<ObservableVector<HomeRow>>,
    row_id: &str,
    result: Result<mq_addons::Fetched, mq_addons::AddonError>,
) {
    let outcome = result
        .map_err(|e| e.to_string())
        .and_then(|f| {
            core.learn_imdb_ids(&f.body);
            decode_catalog(&f.body).map_err(|e| format!("unreadable catalog: {e}"))
        })
        .and_then(|response| match response {
            ResourceResponse::Metas { metas } => {
                let metas: Vec<_> = metas.into_iter().take(ROW_TILES).collect();
                let covers = core.covers(metas.iter().map(|m| m.id.as_str()));
                Ok(metas
                    .into_iter()
                    .map(|m| tile_from_preview(m, &covers))
                    .collect::<Vec<Tile>>())
            }
            _ => Err("addon returned no catalog items".to_owned()),
        });
    update_row(rows, row_id, |row| {
        // A row shown again loads again. The first answer fills it; a later
        // success may still replace a failure (an older load that failed
        // first), but nothing replaces tiles.
        let open = matches!(row.state, RowState::Loading | RowState::Slow);
        let recovers = matches!(row.state, RowState::Failed { .. }) && outcome.is_ok();
        if !open && !recovers {
            return;
        }
        match outcome {
            Ok(tiles) => {
                row.tiles = tiles;
                row.state = if row.tiles.is_empty() {
                    RowState::Empty
                } else {
                    RowState::Ready
                };
            }
            Err(reason) => row.state = RowState::Failed { reason },
        }
    });
}

/// A title's year line as the apps show it, with no dash (core backlog
/// C37): an open range ("2023" then a dash, as Cinemeta ends a running
/// series) reads "Since 2023", a closed one ("2008-2020", "2008 – 2020")
/// "2008 to 2020", and one year twice that year once. Anything else passes
/// as the addon wrote it; an empty line is none.
pub(crate) fn year_line(raw: Option<&str>) -> Option<String> {
    fn year(s: &str) -> Option<&str> {
        (s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit())).then_some(s)
    }
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let Some((from, to)) = raw.split_once(['-', '\u{2013}', '\u{2014}']) else {
        return Some(raw.to_owned());
    };
    let (from, to) = (from.trim(), to.trim());
    Some(match (year(from), year(to)) {
        (Some(from), None) if to.is_empty() => format!("Since {from}"),
        (Some(from), Some(to)) if from == to => from.to_owned(),
        (Some(from), Some(to)) => format!("{from} to {to}"),
        _ => raw.to_owned(),
    })
}

pub(crate) fn tile_from_preview(m: mq_protocol::MetaItemPreview, covers: &Covers) -> Tile {
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
        release_info: year_line(m.release_info.as_deref()),
        progress: None,
        video_id: None,
        ratings: Ratings::of(rating.as_deref()),
        rating,
        blurb: Some(blurb),
        own_shape: None,
        poster_fallback: None,
    }
    .rated_cover(covers)
}

/// The hero is the first catalog item with a background, never from Continue
/// Watching. Once shown it stays put while its row holds it, so the screen
/// does not shift under the user as later rows arrive; its row hidden or gone,
/// or an answer without it, lets the first candidate take its place.
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
                reason: hero_reason(&row.title),
                description: blurb.description,
                genres: blurb.genres,
                runtime: blurb.runtime,
            })
        });
    let current = header.get().hero;
    // Its row is the one its reason names, and it must still hold the tile.
    let kept = current.clone().filter(|hero| {
        rows.iter().any(|r| {
            r.row_id != CONTINUE_ROW_ID
                && hero_reason(&r.title) == hero.reason
                && r.tiles.iter().any(|t| t.id == hero.tile.id)
        })
    });
    let next = kept.or(candidate);
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
    // A streaming account coming or going changes " · On this device"
    // (`kept_here`): the row is read again then too.
    let (mut accounts, mut open) = (core.account_changes(), true);
    refresh_continue(&core, &rows).await;
    maybe_pick_hero(&rows, &header);
    loop {
        let refresh = tokio::select! {
            tables = crate::views::next_change(&mut changes) => {
                let Some(tables) = tables else { return };
                tables.iter().any(|t| {
                    matches!(t, Table::Progress | Table::Watched | Table::LibraryItems)
                        || (kids && *t == Table::Access)
                })
            }
            moved = accounts.changed(), if open => {
                open = moved.is_ok();
                open
            }
        };
        if refresh {
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
    let counted: Vec<AllowedRow> = list
        .map(|l| l.counted)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .take(ROW_TILES)
        .collect();
    let covers = core.covers(counted.iter().map(|r| r.item_id.as_str()));
    let tiles: Vec<Tile> = counted.iter().map(|row| kids_tile(row, &covers)).collect();
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

pub(crate) fn kids_tile(row: &AllowedRow, covers: &Covers) -> Tile {
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
        ratings: Ratings::default(),
        blurb: None,
        own_shape: None,
        poster_fallback: None,
    }
    .rated_stored_cover(covers)
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
// newer plays of titles off the list never crowd out allowed ones. Answers
// the titles the row shows, for their ratings.
async fn refresh_continue(core: &Core, rows: &Mutex<ObservableVector<HomeRow>>) {
    let library = core.library();
    let policy = core.policy();
    // Read once: the tiles below each read their kept IMDb id after their meta.
    let key = &core.rpdb_key();
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
        let covers = core.covers_with(key.clone(), unfinished.iter().map(|i| i.item_id.as_str()));
        let quick: Vec<(i64, Tile)> = unfinished
            .iter()
            .map(|item| {
                (
                    item.updated_ms,
                    tile_from_continue(item.clone(), None, &covers),
                )
            })
            .collect();
        apply_continue(core, rows, quick);
    }
    // Metas come from the addon cache after the first look; they give the
    // tiles their artwork (the episode's still, else the backdrop).
    let mut items: Vec<(i64, Tile)> =
        futures_util::future::join_all(unfinished.into_iter().map(|item| async move {
            let (meta, kept) = tokio::join!(
                core.fetch_meta(&item.kind, &item.item_id),
                kept_here(core, &item)
            );
            let at = item.updated_ms;
            // After the meta: its answer may have given an anime its IMDb id.
            let covers = core.covers_with(key.clone(), [item.item_id.as_str()]);
            let mut tile = tile_from_continue(item, meta.ok().as_ref().map(|m| &m.0), &covers);
            if kept {
                let words = crate::views::playback::ON_DEVICE;
                tile.release_info = tile.release_info.map(|c| format!("{c} \u{b7} {words}"));
            }
            (at, tile)
        }))
        .await;
    let next_up = futures_util::future::join_all(finished.into_iter().map(|done| async move {
        // A series or an anime (Kitsu's kind), read where its kind is.
        let (meta, _) = core.fetch_meta(&done.kind, &done.item_id).await.ok()?;
        let next = next_after(&meta, &done.video_id)?;
        let covers = core.covers_with(key.clone(), [done.item_id.as_str()]);
        Some((done.updated_ms, tile_for_next(&meta, next, &covers)))
    }))
    .await;
    items.extend(next_up.into_iter().flatten());
    apply_continue(core, rows, items);
}

/// A video in progress whose torrent starts from what this device keeps
/// says so after its caption: "S1:E3 · 12 min left · On this device"
/// (plans/player-polish.md 2b). Asked with its meta, so the row applies once
/// per refresh; with no engine running the data folder answers, so a launch
/// starts none for it.
async fn kept_here(core: &Core, item: &ContinueItem) -> bool {
    // With a streaming account the service may take the title, as a
    // Versions row says (round 2 N5): only the engine starts from here.
    #[cfg(feature = "torrent")]
    return item.duration_ms > 0
        && core.debrid().is_none()
        && crate::views::playback::kept_start(core, &item.item_id, &item.video_id)
            .await
            .is_some();
    #[cfg(not(feature = "torrent"))]
    {
        let _ = (core, item);
        false
    }
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

fn tile_for_next(meta: &mq_protocol::MetaItem, next: NextEpisode, covers: &Covers) -> Tile {
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
        ratings: Ratings::of(imdb_rating(p).as_deref()),
        blurb: None,
        own_shape: None,
        poster_fallback: None,
    }
    .rated_cover(covers)
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

fn tile_from_continue(
    item: ContinueItem,
    meta: Option<&mq_protocol::MetaItem>,
    covers: &Covers,
) -> Tile {
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
        // A stored RPDB cover never stands in for the addon's (`rated_stored_cover`).
        poster: item.poster.filter(|p| !p.starts_with(RPDB)).or_else(|| {
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
        ratings: Ratings::of(preview.and_then(imdb_rating).as_deref()),
        rating: preview.and_then(imdb_rating),
        blurb: None,
        own_shape: None,
        poster_fallback: None,
    }
    .rated_cover(covers)
}

fn imdb_rating(p: &mq_protocol::MetaItemPreview) -> Option<String> {
    p.links
        .iter()
        .find(|l| l.category.eq_ignore_ascii_case("imdb"))
        .map(|l| l.name.clone())
}

#[cfg(test)]
mod tests {
    use super::{catalog_title, without_source, year_line};

    /// Core backlog C37: the year line never carries a dash.
    #[test]
    fn the_year_line_has_no_dash() {
        let line = |raw: &str| year_line(Some(raw));
        assert_eq!(line("2023\u{2013}").as_deref(), Some("Since 2023"));
        assert_eq!(line("2023-").as_deref(), Some("Since 2023"));
        assert_eq!(line("2008\u{2013}2020").as_deref(), Some("2008 to 2020"));
        assert_eq!(line("2008-2020").as_deref(), Some("2008 to 2020"));
        assert_eq!(line("2008 \u{2014} 2020").as_deref(), Some("2008 to 2020"));
        assert_eq!(line("2020-2020").as_deref(), Some("2020"));
        assert_eq!(line("2019").as_deref(), Some("2019"));
        assert_eq!(line("TBA").as_deref(), Some("TBA"));
        // A date is no range: it passes as the addon wrote it.
        assert_eq!(line("2019-05-01").as_deref(), Some("2019-05-01"));
        assert_eq!(line("  "), None);
        assert_eq!(year_line(None), None);
    }

    /// C37: a catalog tile (Home, Discover and Search make theirs here)
    /// carries the year line.
    #[test]
    fn a_tile_carries_the_year_line() {
        let body = br#"{"metas":[{"id":"tt3581920","type":"series","name":"The Last of Us","releaseInfo":"2023\u2013"}]}"#;
        let mq_protocol::ResourceResponse::Metas { metas } =
            mq_protocol::decode_catalog(body).unwrap()
        else {
            panic!("metas");
        };
        let tile =
            super::tile_from_preview(metas.into_iter().next().unwrap(), &super::Covers::default());
        assert_eq!(tile.release_info.as_deref(), Some("Since 2023"));
    }

    #[test]
    fn a_row_title_names_the_catalog_and_its_kind() {
        assert_eq!(
            catalog_title("Popular", "movie", "Cinemeta"),
            "Popular Movies"
        );
        assert_eq!(catalog_title("Top", "series", "Cinemeta"), "Top Series");
    }

    #[test]
    fn streaming_catalogs_rows_read_as_their_services() {
        let d = crate::defaults::list()
            .iter()
            .find(|d| d.key == "streamingcatalogs")
            .unwrap();
        let titles: Vec<String> = d
            .manifest
            .catalogs
            .iter()
            .map(|c| catalog_title(c.name.as_deref().unwrap(), &c.r#type, &d.manifest.name))
            .collect();
        assert_eq!(
            titles,
            [
                "Netflix Movies",
                "Netflix Series",
                "HBO Max Movies",
                "HBO Max Series",
                "Disney+ Movies",
                "Disney+ Series",
                "Prime Video Movies",
                "Prime Video Series",
                "Apple TV+ Movies",
                "Apple TV+ Series"
            ]
        );
    }

    /// A type Marquee does not know is never shown as is: no Home title
    /// adds it, and its label is the neutral one.
    #[test]
    fn a_type_marquee_does_not_know_is_never_shown() {
        assert_eq!(catalog_title("Top picks", "trakt", "Lists"), "Top picks");
        assert_eq!(catalog_title("Top", "podcasts", "Pod"), "Top");
        assert_eq!(super::kind_label("trakt"), None);
        assert_eq!(super::type_label("trakt"), "Other");
        assert_eq!(super::type_label("series"), "Series");
    }

    /// The owner, 2026-10-22: a poster of an IMDb title is RPDB's cover with
    /// the ratings band, the addon's poster kept as its fallback; any other
    /// tile keeps the addon's poster.
    #[test]
    fn an_imdb_poster_is_rpdbs_cover_with_the_addons_as_fallback() {
        let bare = |id: &str, shape: &str, poster: Option<&str>| super::Tile {
            id: id.into(),
            kind: "movie".into(),
            name: "x".into(),
            poster: poster.map(str::to_owned),
            poster_fallback: None,
            background: None,
            logo: None,
            poster_shape: shape.into(),
            release_info: None,
            progress: None,
            video_id: None,
            rating: None,
            ratings: Default::default(),
            blurb: None,
            own_shape: None,
        };
        let covers =
            |rpdb: Option<&str>| super::Covers::with(rpdb, &[("kitsu:50016", "tt12590266")]);
        let keyed = |id: &str, shape: &str, poster: Option<&str>, rpdb: Option<&str>| {
            bare(id, shape, poster).rated_cover(&covers(rpdb))
        };
        let tile = |id: &str, shape: &str, poster: Option<&str>| {
            keyed(id, shape, poster, Some("t0-free-rpdb"))
        };
        let own = "https://images.metahub.space/poster/small/tt0111161/img";
        let cover = tile("tt0111161", "poster", Some(own));
        assert_eq!(
            cover.poster.as_deref(),
            Some(
                "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0111161.jpg?fallback=true"
            )
        );
        assert_eq!(cover.poster_fallback.as_deref(), Some(own));
        // An IMDb title with no poster of its own still gets the cover.
        assert!(tile("tt1", "poster", None).poster.is_some());
        // Whatever the addon's shape, an IMDb title's cover is RPDB's 2:3.
        let wide = tile("tt0111161", "landscape", Some(own));
        assert_eq!(wide.poster, cover.poster);
        assert_eq!(wide.poster_shape, "poster");
        assert_eq!(wide.poster_fallback.as_deref(), Some(own));
        // An anime its addon gave an IMDb id: that id's cover.
        let anime = tile("kitsu:50016", "poster", Some(own));
        assert_eq!(
            anime.poster.as_deref(),
            Some(
                "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt12590266.jpg?fallback=true"
            )
        );
        assert_eq!(anime.poster_fallback.as_deref(), Some(own));
        // No IMDb id at all: the addon's poster and shape, no fallback.
        for (id, shape) in [
            ("kitsu:1", "poster"),
            ("kitsu:2", "landscape"),
            ("tt", "poster"),
        ] {
            let t = tile(id, shape, Some(own));
            assert_eq!(
                (
                    t.poster.as_deref(),
                    t.poster_fallback,
                    t.poster_shape.as_str()
                ),
                (Some(own), None, shape),
                "{id} {shape}"
            );
        }
        // A cover sent back (playback's `poster`) is no fallback.
        let back = tile("tt0111161", "poster", cover.poster.as_deref());
        assert_eq!(back.poster, cover.poster);
        assert_eq!(back.poster_fallback, None);
        // With no key, a catalog tile keeps an RPDB poster its addon serves
        // itself (review 2026-10-23); a tile from a stored row drops one, so
        // the band never outlives the key.
        let served = keyed("tt0111161", "poster", cover.poster.as_deref(), None);
        assert_eq!(served.poster, cover.poster);
        let stored =
            bare("tt0111161", "poster", cover.poster.as_deref()).rated_stored_cover(&covers(None));
        assert_eq!((stored.poster, stored.poster_fallback), (None, None));
        let stored = bare("tt0111161", "poster", cover.poster.as_deref())
            .rated_stored_cover(&covers(Some("t0-free-rpdb")));
        assert_eq!(
            (stored.poster, stored.poster_fallback),
            (cover.poster.clone(), None)
        );
        let own_stored = bare("kitsu:13874", "poster", Some(own)).rated_stored_cover(&covers(None));
        assert_eq!(own_stored.poster.as_deref(), Some(own));
        // The key is the addon's: a paid one goes into the link as given.
        let paid = keyed("tt0111161", "poster", Some(own), Some("t1-abc_9"));
        assert!(
            paid.poster
                .unwrap()
                .contains("/t1-abc_9/imdb/poster-default/")
        );
        // No key (the Rotten Tomatoes addon off, removed or keyless): the
        // addon's poster and shape.
        let plain = keyed("tt0111161", "landscape", Some(own), None);
        assert_eq!(
            (
                plain.poster.as_deref(),
                plain.poster_fallback,
                plain.poster_shape.as_str()
            ),
            (Some(own), None, "landscape")
        );
    }

    #[test]
    fn a_catalog_that_names_its_addon_does_not_say_it_twice() {
        // Trakt Tv's catalogs are "trakt - Popular movies" of the custom type "trakt" (the Fire TV walk's
        // "#1 IN TRAKT - POPULAR MOVIES TRAKT ON TRAKT TV").
        let title = catalog_title("trakt - Popular movies", "trakt", "Trakt Tv");
        assert_eq!(title, "Popular movies");
        assert_eq!(
            catalog_title("Trakt: Trending", "movie", "Trakt Tv"),
            "Trending Movies"
        );
        // Game Cinema's catalogs (community.json) part the addon from the name with an em dash.
        let title = catalog_title("Game Cinema \u{2014} New This Week", "movie", "Game Cinema");
        assert_eq!(title, "New This Week Movies");
        assert_eq!(
            catalog_title("Game Cinema \u{2014} Most Watched", "movie", "Game Cinema"),
            "Most Watched Movies"
        );
        assert_eq!(
            catalog_title("Game Cinema \u{2014} Series", "series", "Game Cinema"),
            "Series"
        );
        assert_eq!(
            catalog_title("Trakt \u{2013} Trending", "movie", "Trakt Tv"),
            "Trending Movies"
        );
        // The addon's full name, generic words and all, goes before its own words do.
        assert_eq!(
            catalog_title("Trakt Tv - Popular", "movie", "Trakt Tv"),
            "Popular Movies"
        );
    }

    #[test]
    fn a_kind_the_name_says_in_the_singular_is_not_added() {
        assert_eq!(
            catalog_title("Random Movie", "movie", "Random Movie"),
            "Random Movie"
        );
        assert_eq!(
            catalog_title("Top Series", "series", "Cinemeta"),
            "Top Series"
        );
        // A known kind is said even when the addon's name holds it.
        assert_eq!(
            catalog_title("Year", "movie", "The Movie Database Addon"),
            "Year Movies"
        );
        assert_eq!(
            catalog_title("Kitsu Trending", "anime", "Anime Kitsu"),
            "Kitsu Trending Anime"
        );
    }

    #[test]
    fn every_fixture_catalog_keeps_its_first_word() {
        // The 160 catalogs of the addon catalog fixtures: a name that starts with a word keeps one at its start.
        for f in ["official", "community"] {
            let path = format!(
                "{}/tests/fixtures/addon_catalog/{f}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            let doc: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            for addon in doc["addons"].as_array().unwrap() {
                let source = addon["manifest"]["name"].as_str().unwrap();
                for c in addon["manifest"]["catalogs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let kind = c["type"].as_str().unwrap();
                    let name = c["name"]
                        .as_str()
                        .unwrap_or_else(|| c["id"].as_str().unwrap());
                    for title in [
                        catalog_title(name, kind, source),
                        without_source(name, source),
                    ] {
                        assert!(
                            !name.starts_with(char::is_alphanumeric)
                                || title.starts_with(char::is_alphanumeric),
                            "{source}: {name:?} became {title:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_name_that_goes_on_past_the_addon_without_a_word_stays_whole() {
        assert_eq!(
            catalog_title("Trakt's Picks", "movie", "Trakt Tv"),
            "Trakt's Picks Movies"
        );
        assert_eq!(
            catalog_title("Trakt.tv Popular", "movie", "Trakt Tv"),
            "Trakt.tv Popular Movies"
        );
        assert_eq!(
            catalog_title("Trakt - ...", "movie", "Trakt Tv"),
            "Trakt - ... Movies"
        );
        // Not only ASCII: the addon's name in other capitals.
        assert_eq!(
            catalog_title(
                "\u{041a}\u{0418}\u{041d}\u{041e} - \u{041d}\u{043e}\u{0432}\u{0438}\u{043d}\u{043a}\u{0438}",
                "movie",
                "\u{041a}\u{0438}\u{043d}\u{043e}"
            ),
            "\u{041d}\u{043e}\u{0432}\u{0438}\u{043d}\u{043a}\u{0438} Movies"
        );
    }

    #[test]
    fn a_name_that_is_only_the_addon_or_shares_a_prefix_stays() {
        assert_eq!(catalog_title("Trakt", "movie", "Trakt Tv"), "Trakt Movies");
        assert_eq!(
            catalog_title("Traktor hits", "movie", "Trakt Tv"),
            "Traktor hits Movies"
        );
        assert_eq!(catalog_title("Top", "movie", "TV"), "Top Movies");
    }
}
