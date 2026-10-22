//! Home: Continue Watching first, then one row per addon catalog in the
//! user's addon order. Row positions are fixed at open so focus never jumps
//! while catalogs arrive; empty and failed rows stay as explained states.
//!
//! A kids profile's Home asks no catalog and has no hero: Continue Watching
//! of allowed titles, then its list's 20 newest (plans/m11-kids.md 5).

use crate::live::{FRAME, ListDiff, coalesce};
use crate::ratings::{self, Ratings, Title};
use crate::views::playback::{NextEpisode, next_after};
use crate::views::settings::HomeRows;
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use eyeball_im::ObservableVector;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::{AllowedRow, ContinueItem, Policy, Table};
use mq_protocol::{ResourcePath, ResourceResponse, decode_response};
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
    /// The IMDb and Rotten Tomatoes scores to show side by side: MDBList's,
    /// with the addon's own IMDb rating when MDBList has none, and "Fresh"
    /// when the Rotten Tomatoes addon lists the title and MDBList gave no
    /// Tomatometer.
    pub ratings: Ratings,
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
        let listed = core.ratings().listed();
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
        tasks.spawn(ratings::on_listed(listed, {
            let (core, rows, header) = (core.clone(), rows.clone(), header.clone());
            move || {
                rerate(&core, &rows, &header);
            }
        }));
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
    /// `new`: the ids made from the old link take the new one's tag.
    pub(crate) fn move_home_rows(&self, old: &str, new: &str) -> Result<(), CoreError> {
        let (from, to) = (
            format!("@{}:", install_tag(old)),
            format!("@{}:", install_tag(new)),
        );
        let rows = self.prefs().home_rows;
        let moved = |ids: &[String]| -> Vec<String> {
            ids.iter().map(|id| id.replacen(&from, &to, 1)).collect()
        };
        let next = HomeRows {
            hidden: moved(&rows.hidden),
            order: moved(&rows.order),
            shown: moved(&rows.shown),
        };
        if next != rows {
            self.set_prefs(&serde_json::json!({ "homeRows": next }))?;
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
/// follows the addon order. The swap is one batch of diffs. The ratings
/// shown are put on again then (and, by `on_listed`, when a read of the
/// Rotten Tomatoes addon's lists ends).
async fn watch_addons(
    core: Core,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    mut ids: Vec<String>,
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
    header: SharedObservable<HomeHeader>,
) {
    let mut loads = JoinSet::new();
    while home_changed(&mut changes).await {
        while loads.try_join_next().is_some() {}
        // The Rotten Tomatoes addon's mark comes and goes with the addon.
        let titles = rerate(&core, &rows, &header);
        loads.spawn(rate(core.clone(), rows.clone(), header.clone(), titles));
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
/// says, or a custom kind that only repeats the addon's name (Trakt Tv's type "trakt"), is not added. A
/// known kind stays even when the addon's name holds it (The Movie Database Addon's "Popular Movies").
fn catalog_title(name: &str, kind: &str, source: &str) -> String {
    let name = without_source(
        &name.split_whitespace().collect::<Vec<_>>().join(" "),
        source,
    );
    let label = type_label(kind);
    let custom = label == kind;
    if has_words(&name, label) || (custom && has_words(source, label)) {
        name
    } else {
        format!("{name} {label}")
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
    let mut lookups = JoinSet::new();
    loop {
        tokio::select! {
            joined = calls.join_next() => {
                let Some(joined) = joined else { break };
                let Ok((row_id, result)) = joined else { continue };
                let titles = apply_catalog(&core, &rows, &row_id, result);
                maybe_pick_hero(&rows, &header);
                lookups.spawn(rate(core.clone(), rows.clone(), header.clone(), titles));
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
    // The ratings still on their way.
    while lookups.join_next().await.is_some() {}
}

/// Looks up the ratings of `titles`, then puts what holds for them now on
/// every tile that shows one of them, the hero's too: not what the lookup
/// saw, since the addon may have gone off meanwhile, and the tiles' fill
/// may be older still.
async fn rate(
    core: Core,
    rows: Arc<Mutex<ObservableVector<HomeRow>>>,
    header: SharedObservable<HomeHeader>,
    titles: Vec<Title>,
) {
    core.ratings().lookup(&titles).await;
    let rater = core.ratings();
    let _turn = rater.turn();
    put(&rows, &header, &rater.shown(&titles));
}

/// Puts on every tile shown, the hero's too, what the ratings cache and
/// the Rotten Tomatoes addon's lists hold now. The titles shown, for a
/// lookup. The rows are locked only to read them and to put the answer on.
fn rerate(
    core: &Core,
    rows: &Mutex<ObservableVector<HomeRow>>,
    header: &SharedObservable<HomeHeader>,
) -> Vec<Title> {
    let rater = core.ratings();
    let _turn = rater.turn();
    let hero = header.get().hero;
    let titles = ratings::titles(
        lock(rows)
            .iter()
            .flat_map(|r| &r.tiles)
            .chain(hero.iter().map(|h| &h.tile)),
    );
    put(rows, header, &rater.shown(&titles));
    titles
}

/// Gives every tile of a title `known` holds, the hero's too, its row.
fn put(
    rows: &Mutex<ObservableVector<HomeRow>>,
    header: &SharedObservable<HomeHeader>,
    known: &HashMap<Title, ratings::Scores>,
) {
    {
        let mut rows = lock(rows);
        for index in 0..rows.len() {
            let mut row = rows[index].clone();
            if ratings::apply(&mut row.tiles, known) {
                rows.set(index, row);
            }
        }
    }
    header.update_if(|h| {
        h.hero
            .as_mut()
            .is_some_and(|hero| ratings::apply([&mut hero.tile], known))
    });
}

/// Fills the row from the catalog's answer; the titles it may show, for
/// their ratings.
fn apply_catalog(
    core: &Core,
    rows: &Mutex<ObservableVector<HomeRow>>,
    row_id: &str,
    result: Result<mq_addons::Fetched, mq_addons::AddonError>,
) -> Vec<Title> {
    let outcome = result
        .map_err(|e| e.to_string())
        .and_then(|f| {
            decode_response(&f.body, "catalog").map_err(|e| format!("unreadable catalog: {e}"))
        })
        .and_then(|response| match response {
            ResourceResponse::Metas { metas } => {
                let mut tiles: Vec<Tile> = metas
                    .into_iter()
                    .take(ROW_TILES)
                    .map(tile_from_preview)
                    .collect();
                // What the ratings cache holds, before the rows lock.
                core.ratings().fill(&mut tiles);
                Ok(tiles)
            }
            _ => Err("addon returned no catalog items".to_owned()),
        });
    // Only the titles the row takes: an answer it refuses asks nothing.
    let mut titles = Vec::new();
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
                titles = ratings::titles(&tiles);
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
    titles
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
        ratings: Ratings::of(None, rating.as_deref()),
        rating,
        blurb: Some(blurb),
    }
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
    let mut lookups = JoinSet::new();
    let titles = refresh_continue(&core, &rows).await;
    maybe_pick_hero(&rows, &header);
    lookups.spawn(rate(core.clone(), rows.clone(), header.clone(), titles));
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables.iter().any(|t| {
            matches!(t, Table::Progress | Table::Watched | Table::LibraryItems)
                || (kids && *t == Table::Access)
        }) {
            let titles = refresh_continue(&core, &rows).await;
            maybe_pick_hero(&rows, &header);
            while lookups.try_join_next().is_some() {}
            lookups.spawn(rate(core.clone(), rows.clone(), header.clone(), titles));
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
        ratings: Ratings::default(),
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
// newer plays of titles off the list never crowd out allowed ones. Answers
// the titles the row shows, for their ratings.
async fn refresh_continue(core: &Core, rows: &Mutex<ObservableVector<HomeRow>>) -> Vec<Title> {
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
        let mut quick: Vec<(i64, Tile)> = unfinished
            .iter()
            .map(|item| (item.updated_ms, tile_from_continue(item.clone(), None)))
            .collect();
        core.ratings().fill(quick.iter_mut().map(|(_, t)| t));
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
            let mut tile = tile_from_continue(item, meta.ok().as_ref().map(|m| &m.0));
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
        Some((done.updated_ms, tile_for_next(&meta, next)))
    }))
    .await;
    items.extend(next_up.into_iter().flatten());
    core.ratings().fill(items.iter_mut().map(|(_, t)| t));
    let titles = ratings::titles(items.iter().map(|(_, t)| t));
    apply_continue(core, rows, items);
    titles
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
        ratings: Ratings::of(None, imdb_rating(p).as_deref()),
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
        ratings: Ratings::of(None, preview.and_then(imdb_rating).as_deref()),
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

#[cfg(test)]
mod tests {
    use super::{catalog_title, without_source};

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
