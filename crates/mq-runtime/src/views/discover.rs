//! Discover: Movies, Series and Anime, filtered by streaming service, genre
//! and decade, in an order the user picks (plans/discover-v2.md). An order
//! an addon's catalog gives ("Popular", "Trending", a user's own lists) is
//! paged through its `skip` extra; an order the core makes (IMDb rating,
//! dates, A to Z, a service's list) works on a complete set of lists and
//! shows it a window at a time.

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::services::{self, Service};
use crate::views::discover_sets::{
    DECADES, Item, Order, arrange, genre_label, genres_of, tile_in_decade, today,
};
use crate::views::home::{
    OTHER_KIND, Tile, catalog_title, home_changed, kind_label, row_id, tile_from_preview,
    type_label, without_source,
};
use crate::views::settings::{DiscoverPrefs, HomeRows};
use eyeball::SharedObservable;
use futures_util::stream::FuturesUnordered;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::Table;
use mq_protocol::{ResourcePath, ResourceResponse, decode_catalog};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use stremio_core::types::addon::ExtraValue;
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinHandle;

/// One try at a page or a list; a failed page is tried once more.
const TIMEOUT: Duration = Duration::from_secs(12);
/// A complete set publishes what has come by then; late lists merge in once.
const SOFT_DEADLINE: Duration = Duration::from_millis(3500);
/// How long a decade looks through a paged list per load.
const DECADE_BUDGET: Duration = Duration::from_secs(8);
/// Pages of a paged list asked at once while a decade looks through it.
const IN_FLIGHT: usize = 3;
/// Matches a decade's look-through stops at, per load.
const DECADE_FILL: usize = 24;
/// Tiles of a complete set published at a time.
const WINDOW: usize = 80;
/// The remembered choice is written this long after the last change.
const REMEMBER_AFTER: Duration = Duration::from_millis(500);
/// Kitsu's Most Popular, as listed: ranks 1 to 20, then 41 to 120 (the
/// addon answers page one again for a skip of 1 to 39).
const KITSU_SKIPS: [usize; 5] = [0, 40, 60, 80, 100];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct CatalogChoice {
    /// `addon id|type|catalog id`, the value `select` takes.
    pub id: String,
    pub name: String,
    pub source: String,
}

/// A streaming service the rail offers: `id` is the stable key apps map to
/// their tile, `name` the name shown.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    pub id: String,
    pub name: String,
}

/// An order the sort menu offers. `id` is what `select {sort}` takes:
/// `popular`, `trending`, `imdb`, `fresh`, `highest`, `airing`, `newest`,
/// `oldest`, `az`, or `list:<catalog id>` for one of `lists`. `mark` names
/// the rating mark drawn beside the label (`imdb`, `rt`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct SortChoice {
    pub id: String,
    pub label: String,
    pub mark: Option<String>,
}

/// A genre the Genre menu offers; `count` is how many listed titles give it
/// when the list is a complete set, null for a catalog's own options.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct GenreChoice {
    pub name: String,
    pub count: Option<u32>,
}

/// What the list on screen covers, for the footer: `titles` in a complete
/// set (null for a paged list), the `services` it comes from, how many of
/// them `failed` or are still `pending`, how many titles a decade `looked`
/// through, and the scope `note` that goes with the sort's label ("on your
/// services").
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub titles: Option<u32>,
    pub services: u32,
    pub failed: u32,
    pub pending: u32,
    pub looked: Option<u32>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum DiscoverState {
    Loading,
    /// More pages can be loaded.
    Ready,
    /// The catalog has nothing more.
    End,
    /// The last page asked failed; `items` keeps what came before (none
    /// when the first page failed). `loadMore` asks that page again.
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverSnapshot {
    /// Kinds with at least one catalog: ("movie", "Movies"), Movies, Series
    /// and Anime first. A catalog of a type Marquee does not know (Trakt
    /// Tv's "trakt") is filed under the kind its name says ("Popular
    /// movies"), else ("other", "Other"); the raw type is never a kind here.
    pub kinds: Vec<(String, String)>,
    pub kind: Option<String>,
    /// Catalogs of the selected type, in addon order (the screen before
    /// Discover v2; kept until every app reads `sorts`).
    pub catalogs: Vec<CatalogChoice>,
    /// The catalog a paged list comes from; null for a complete set.
    pub catalog: Option<String>,
    /// Genre options: the catalog's own, or the genres a complete set
    /// gives (`genreChoices` with counts).
    pub genres: Vec<String>,
    pub genre: Option<String>,
    /// The catalog only answers with a genre picked.
    pub genre_required: bool,
    /// A genre asked with a kind and no catalog (a title page's genre
    /// chip) that no catalog of that kind lists: Discover shows the kind
    /// without it, on its first catalog that needs no genre (else its first,
    /// with that catalog's first genre), and says so (core backlog C40).
    /// None otherwise.
    pub missing_genre: Option<String>,
    /// The services the rail offers for the kind ("All" is null).
    pub platforms: Vec<Platform>,
    pub platform: Option<String>,
    /// The orders the sort menu offers for the kind and service.
    pub sorts: Vec<SortChoice>,
    pub sort: Option<String>,
    /// Every other catalog of the kind, by its own name ("More lists").
    pub lists: Vec<CatalogChoice>,
    pub genre_choices: Vec<GenreChoice>,
    /// What the genre menu is called: "Genre", or "Year" or "Period" when
    /// the list's genre extra holds those.
    pub genre_label: String,
    /// The Year menu's choices; empty when the list cannot be dated.
    pub decades: Vec<String>,
    pub decade: Option<String>,
    pub scope: Scope,
    /// A sort asked for (or remembered) that the kind or service does not
    /// offer: the list shows the kind's first sort instead.
    pub missing_sort: Option<String>,
    pub items: Vec<Tile>,
    /// The item `preview {id}` named, with what its catalog said of it, so
    /// a preview shows its synopsis without opening a title view (core
    /// backlog C44a). Null until one is named and listed: an id named
    /// before its page came is filled in when it comes. A `None` or a new
    /// list (another kind, catalog or genre) clears it.
    pub preview: Option<ItemDetails>,
    pub state: DiscoverState,
}

/// A catalog item's synopsis, up to three genres and runtime, as its
/// catalog gave them; any may be missing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetails {
    pub id: String,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub runtime: Option<String>,
    /// In the Library, as it stands now: `setInLibrary {id, value}` on
    /// Discover changes it with no title view (core backlog C44b).
    pub in_library: bool,
    /// Add to Library can be offered: false on a kids profile, as the
    /// title page's `librarySettable`.
    pub library_settable: bool,
}

/// What `select` changes; `None` keeps, `Some(None)` clears.
#[derive(Debug, Clone, Default)]
pub struct DiscoverChoice {
    pub kind: Option<String>,
    pub platform: Option<Option<String>>,
    pub sort: Option<String>,
    pub genre: Option<Option<String>>,
    pub decade: Option<Option<String>>,
    /// The screen before Discover v2 picked a catalog: a link to that list.
    pub catalog: Option<String>,
}

#[derive(Clone)]
pub(crate) struct Catalog {
    pub(crate) choice: CatalogChoice,
    pub(crate) addon: InstalledAddon,
    /// The type it is asked with.
    pub(crate) kind: String,
    /// The kind it is filed under (`shelf`).
    pub(crate) shelf: String,
    pub(crate) id: String,
    pub(crate) genres: Vec<String>,
    genre_required: bool,
    pages: bool,
    /// The streaming service its name names.
    service: Option<&'static Service>,
    /// The sort it backs on its kind, for the catalogs Discover knows.
    backs: Option<&'static str>,
}

/// The catalogs whose order a sort is, by addon manifest id and catalog id.
/// Names stay inside the core: the screen shows the sort's own label.
const BACKS: [(&str, &str, &str); 9] = [
    ("com.linvo.cinemeta", "top", "popular"),
    ("community.trakt-tv", "trakt_trending_movies", "trending"),
    ("community.trakt-tv", "trakt_trending_series", "trending"),
    ("community.anime.kitsu", "kitsu-anime-popular", "popular"),
    ("community.anime.kitsu", "kitsu-anime-trending", "trending"),
    ("community.anime.kitsu", "kitsu-anime-rating", "highest"),
    ("community.anime.kitsu", "kitsu-anime-airing", "airing"),
    ("pw.ers.rottentomatoes", "rtfresh_movie", "fresh"),
    ("pw.ers.rottentomatoes", "rtfresh_series", "fresh"),
];

/// The genre that makes Rotten Tomatoes' movie list the Anime tab's.
const ANIME_GENRE: &str = "Anime";

/// Where a list comes from.
#[derive(Clone)]
enum Source {
    /// An addon's catalog, paged.
    Paged {
        catalog: Box<Catalog>,
        /// A genre asked always and never shown (Anime's Certified Fresh).
        fixed_genre: Option<String>,
    },
    /// Lists fetched whole and ordered by the core.
    Set {
        lists: Vec<Fetch>,
        order: Order,
        /// Only titles this list holds stay (Certified Fresh on a service).
        keep: Option<Fetch>,
        /// Asked with the set only to learn from (a service's Certified
        /// Fresh list, so its sort menu is right before the user opens it).
        also: Option<Fetch>,
        note: Option<&'static str>,
    },
}

/// One list of a complete set: a catalog at one `skip`, with a genre.
#[derive(Clone)]
struct Fetch {
    catalog: Box<Catalog>,
    skip: usize,
    genre: Option<String>,
}

impl Fetch {
    fn key(&self) -> String {
        format!(
            "{}#{}#{}",
            self.catalog.choice.id,
            self.skip,
            self.genre.as_deref().unwrap_or_default()
        )
    }
}

impl Source {
    /// Equal keys load the same list.
    fn key(&self) -> String {
        match self {
            Source::Paged {
                catalog,
                fixed_genre,
            } => format!("paged {} {fixed_genre:?}", catalog.choice.id),
            Source::Set {
                lists,
                order,
                keep,
                also,
                ..
            } => format!(
                "set {:?} {:?} {:?} {:?}",
                lists.iter().map(Fetch::key).collect::<Vec<_>>(),
                order,
                keep.as_ref().map(Fetch::key),
                also.as_ref().map(Fetch::key)
            ),
        }
    }
}

struct State {
    catalogs: Vec<Catalog>,
    snapshot: DiscoverSnapshot,
    source: Option<Source>,
    /// Raised by every select: a load writes only while it is current.
    generation: u64,
    seen: HashSet<String>,
    /// Added to the next page's `skip`: the items an addon skipped past
    /// when a page repeated an earlier one (`load_page`).
    skip_ahead: usize,
    /// A complete set in its order, of which `items` shows the first
    /// `shown`.
    set: Vec<Tile>,
    shown: usize,
    /// Decoded lists of complete sets, kept while the view lives, so another
    /// order, genre or decade asks nothing.
    lists: HashMap<String, Arc<Vec<Item>>>,
    /// Services whose Certified Fresh titles are none (kind|service).
    fresh_empty: HashSet<String>,
    /// The id `preview` named: kept until a select or a `None`, so an item
    /// named before its page came is filled in when it does.
    preview: Option<String>,
    /// Bumped by every select; the remembered choice is written when it
    /// has not moved for `REMEMBER_AFTER`.
    remember: u64,
}

pub struct DiscoverView {
    inner: Arc<Inner>,
    watch: JoinHandle<()>,
}

struct Inner {
    core: Core,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<DiscoverSnapshot>,
    task: Mutex<Option<JoinHandle<()>>>,
}

/// The kinds the catalogs are filed under, Movies, Series and Anime first,
/// then in catalog order, with labels: ("movie", "Movies").
pub(crate) fn kinds(catalogs: &[Catalog]) -> Vec<(String, String)> {
    let mut kinds: Vec<(String, String)> = Vec::new();
    for c in catalogs {
        if !kinds.iter().any(|(k, _)| *k == c.shelf) {
            kinds.push((c.shelf.clone(), type_label(&c.shelf).to_owned()));
        }
    }
    let rank = |k: &str| ["movie", "series", "anime"].iter().position(|x| *x == k);
    kinds.sort_by_key(|(k, _)| rank(k).unwrap_or(usize::MAX));
    kinds
}

fn empty_snapshot(catalogs: &[Catalog]) -> DiscoverSnapshot {
    DiscoverSnapshot {
        kinds: kinds(catalogs),
        kind: None,
        catalogs: Vec::new(),
        catalog: None,
        genres: Vec::new(),
        genre: None,
        genre_required: false,
        missing_genre: None,
        platforms: Vec::new(),
        platform: None,
        sorts: Vec::new(),
        sort: None,
        lists: Vec::new(),
        genre_choices: Vec::new(),
        genre_label: "Genre".into(),
        decades: Vec::new(),
        decade: None,
        scope: Scope::default(),
        missing_sort: None,
        items: Vec::new(),
        preview: None,
        state: DiscoverState::End,
    }
}

impl DiscoverView {
    /// Opens on the choice the profile last made (its kind, service, sort,
    /// genre and decade, where they still exist), else on the first kind's
    /// first sort, and follows the installed addons from then on.
    pub fn open(core: &Core) -> Self {
        // Subscribed before reading, so no addon change falls in between.
        let changes = core.library().subscribe();
        let catalogs = browsable(
            &core.installed_addons().unwrap_or_default(),
            &core.prefs().home_rows,
        );
        let snapshot = empty_snapshot(&catalogs);
        let inner = Arc::new(Inner {
            core: core.clone(),
            state: Arc::new(Mutex::new(State {
                catalogs,
                snapshot: snapshot.clone(),
                source: None,
                generation: 0,
                seen: HashSet::new(),
                skip_ahead: 0,
                set: Vec::new(),
                shown: 0,
                lists: HashMap::new(),
                fresh_empty: HashSet::new(),
                preview: None,
                remember: 0,
            })),
            snapshot: SharedObservable::new(snapshot),
            task: Mutex::new(None),
        });
        let last = core.prefs().discover;
        inner.choose(
            DiscoverChoice {
                kind: last.kind,
                platform: Some(last.platform),
                sort: last.sort,
                genre: Some(last.genre),
                decade: Some(last.decade),
                catalog: None,
            },
            false,
        );
        let (watched, library_watch) = (inner.clone(), inner.clone());
        let library = core.library().subscribe();
        let watch = tokio::spawn(async move {
            futures_util::join!(
                watch_addons(watched, changes),
                watch_library(library_watch, library)
            );
        });
        Self { inner, watch }
    }

    pub fn snapshot(&self) -> DiscoverSnapshot {
        self.inner.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        DiscoverSnapshot,
        Pin<Box<dyn Stream<Item = Vec<DiscoverSnapshot>> + Send>>,
    ) {
        let updates = self.inner.snapshot.subscribe().map(|s| vec![s]);
        (
            self.inner.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }

    /// The screen before Discover v2: changes the type, the catalog or the
    /// genre. A catalog, or a kind and a genre with nothing else (a title
    /// page's genre chip, Search's genre grid), is a link and starts from a
    /// clean slate.
    pub fn select(&self, kind: Option<&str>, catalog: Option<&str>, genre: Option<Option<&str>>) {
        self.choose(DiscoverChoice {
            kind: kind.map(str::to_owned),
            genre: genre.map(|g| g.map(str::to_owned)),
            catalog: catalog.map(str::to_owned),
            ..DiscoverChoice::default()
        });
    }

    /// Changes the kind, service, sort, genre or decade; whatever is not
    /// given is kept when it still fits. Starts the list again, and the
    /// choice is remembered for the profile.
    pub fn choose(&self, choice: DiscoverChoice) {
        self.inner.choose(choice, true);
    }

    /// Shows the next window of a complete set, loads the next page of a
    /// paged list when it has more and none is loading, or asks a failed
    /// page again.
    pub fn load_more(&self) {
        self.inner.load_more();
    }

    /// Names the item a preview shows (`None` clears): `preview` carries
    /// its details from the catalog's own answer, with no request, once the
    /// item is listed.
    pub fn preview(&self, id: Option<&str>) {
        let mut s = lock(&self.inner.state);
        s.preview = id.map(str::to_owned);
        if fill_preview(&self.inner.core, &mut s) {
            self.inner.snapshot.set(s.snapshot.clone());
        }
    }

    /// Adds a listed item to the Library, or takes it out, from what its
    /// catalog gave (no title view, no meta fetch, no swarm warm-up): the
    /// preview's Add to Library (core backlog C44b). An id not listed does
    /// nothing.
    pub fn set_in_library(&self, id: &str, in_library: bool) -> Result<(), crate::CoreError> {
        let row = {
            let s = lock(&self.inner.state);
            let Some(t) = s.snapshot.items.iter().find(|t| t.id == id) else {
                return Ok(());
            };
            mq_library::LibraryItemRow {
                item_id: t.id.clone(),
                kind: t.kind.clone(),
                name: t.name.clone(),
                // The addon's own poster, as the title view keeps it.
                poster: t.poster_fallback.clone().or_else(|| t.poster.clone()),
                poster_shape: None,
                added_ms: mq_library::hlc::now_ms() as i64,
            }
        };
        self.inner.core.set_in_library(&row, in_library)
    }
}

/// Keeps the preview's Library state as it stands: an add or a removal
/// here, on the title page or from another device.
async fn watch_library(inner: Arc<Inner>, mut changes: Receiver<Vec<Table>>) {
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables.contains(&Table::LibraryItems) {
            let mut s = lock(&inner.state);
            if fill_preview(&inner.core, &mut s) {
                inner.snapshot.set(s.snapshot.clone());
            }
        }
    }
}

/// Follows the installed addons and the twins turned on in Customize Home:
/// the list on screen stays when it still comes from the same catalogs (no
/// reload); otherwise the choice is fitted to what is installed again.
async fn watch_addons(inner: Arc<Inner>, mut changes: Receiver<Vec<Table>>) {
    let mut key = inner.core.rpdb_key();
    while home_changed(&mut changes).await {
        let now = inner.core.rpdb_key();
        if now != key {
            // The covers carry the key's band: the lists are made again.
            let mut s = lock(&inner.state);
            crate::views::home::recover_tiles(
                &inner.core,
                key.take(),
                now.clone(),
                s.snapshot.items.iter_mut(),
            );
            s.lists.clear();
            inner.snapshot.set(s.snapshot.clone());
            key = now;
        }
        let catalogs = browsable(
            &inner.core.installed_addons().unwrap_or_default(),
            &inner.core.prefs().home_rows,
        );
        {
            let mut s = lock(&inner.state);
            let ids = |c: &[Catalog]| c.iter().map(|c| c.choice.id.clone()).collect::<Vec<_>>();
            if ids(&catalogs) == ids(&s.catalogs) {
                continue;
            }
            s.snapshot.kinds = kinds(&catalogs);
            s.catalogs = catalogs;
        }
        inner.choose(DiscoverChoice::default(), false);
    }
}

/// The sorts a kind and service offer, in menu order.
fn sorts(
    catalogs: &[Catalog],
    kind: &str,
    platform: Option<&str>,
    genre: Option<&str>,
    fresh_empty: &HashSet<String>,
) -> Vec<SortChoice> {
    let backed = |sort: &str| backing(catalogs, kind, sort).is_some();
    let choice = |id: &str| {
        let (label, mark) = match id {
            "popular" => ("Popular", None),
            "trending" => ("Trending", None),
            "imdb" => ("IMDb rating", Some("imdb")),
            "fresh" => ("Certified Fresh", Some("rt")),
            "highest" => ("Highest rated", None),
            "airing" => ("Airing now", None),
            "newest" => ("Newest", None),
            "oldest" => ("Oldest", None),
            _ => ("A to Z", None),
        };
        SortChoice {
            id: id.to_owned(),
            label: label.to_owned(),
            mark: mark.map(str::to_owned),
        }
    };
    let mut ids: Vec<&str> = Vec::new();
    if let Some(p) = platform {
        ids.push("trending");
        ids.push("imdb");
        if backed("fresh") && !fresh_empty.contains(&format!("{kind}|{p}")) {
            ids.push("fresh");
        }
        ids.extend(["newest", "oldest", "az"]);
        return ids.into_iter().map(choice).collect();
    }
    let anime = kind == "anime";
    if backed("popular") {
        ids.push("popular");
    }
    // Kitsu's Trending takes no genre.
    let trending = backing(catalogs, kind, "trending");
    if trending.is_some_and(|c| genre.is_none() || !c.genres.is_empty()) {
        ids.push("trending");
    }
    let sets = !service_lists(catalogs, kind, None).is_empty();
    if sets && !anime {
        ids.push("imdb");
    }
    if backed("fresh") {
        ids.push("fresh");
    }
    for own in ["highest", "airing"] {
        if backed(own) {
            ids.push(own);
        }
    }
    let kitsu_dates = anime && backed("popular");
    if (sets && !anime) || kitsu_dates {
        ids.extend(["newest", "oldest", "az"]);
    }
    ids.into_iter().map(choice).collect()
}

/// The catalog a sort reads on a kind: one the table names, of that kind,
/// first in addon order; Anime's Certified Fresh is Rotten Tomatoes' movie
/// list when it offers the Anime genre.
fn backing<'a>(catalogs: &'a [Catalog], kind: &str, sort: &str) -> Option<&'a Catalog> {
    let of = |c: &&Catalog| c.backs == Some(sort);
    catalogs
        .iter()
        .filter(of)
        .find(|c| c.shelf == kind)
        .or_else(|| {
            (kind == "anime" && sort == "fresh").then(|| {
                catalogs
                    .iter()
                    .filter(of)
                    .find(|c| c.kind == "movie" && c.genres.iter().any(|g| g == ANIME_GENRE))
            })?
        })
}

/// The service lists of a kind, of one service or of all.
fn service_lists<'a>(
    catalogs: &'a [Catalog],
    kind: &str,
    platform: Option<&str>,
) -> Vec<&'a Catalog> {
    catalogs
        .iter()
        .filter(|c| c.shelf == kind)
        .filter(|c| {
            c.service
                .is_some_and(|s| platform.is_none_or(|p| s.id == p))
        })
        .collect()
}

/// The services the rail offers for a kind, in addon order.
fn platforms(catalogs: &[Catalog], kind: &str) -> Vec<Platform> {
    let mut out: Vec<Platform> = Vec::new();
    for s in service_lists(catalogs, kind, None)
        .into_iter()
        .filter_map(|c| c.service)
    {
        if !out.iter().any(|p| p.id == s.id) {
            out.push(Platform {
                id: s.id.to_owned(),
                name: s.name.to_owned(),
            });
        }
    }
    out
}

/// The catalogs of a kind no sort reads and no service names: "More lists".
fn more_lists(catalogs: &[Catalog], kind: &str) -> Vec<CatalogChoice> {
    catalogs
        .iter()
        .filter(|c| c.shelf == kind && c.service.is_none())
        .filter(|c| {
            !c.backs.is_some_and(|b| {
                backing(catalogs, kind, b).is_some_and(|x| x.choice.id == c.choice.id)
            })
        })
        .map(|c| c.choice.clone())
        .collect()
}

/// The sort a catalog stands for on its kind: the one it backs, else its
/// own list.
fn sort_of(catalogs: &[Catalog], catalog: &Catalog) -> String {
    catalog
        .backs
        .filter(|b| {
            backing(catalogs, &catalog.shelf, b).is_some_and(|x| x.choice.id == catalog.choice.id)
        })
        .map_or_else(|| format!("list:{}", catalog.choice.id), str::to_owned)
}

/// Where a sort's list comes from.
fn source(catalogs: &[Catalog], kind: &str, platform: Option<&str>, sort: &str) -> Option<Source> {
    let order = |s: &str| match s {
        "imdb" => Some(Order::Imdb),
        "newest" => Some(Order::Newest),
        "oldest" => Some(Order::Oldest),
        "az" => Some(Order::Az),
        "trending" => Some(Order::Source),
        _ => None,
    };
    let whole = |c: &Catalog| Fetch {
        catalog: Box::new(c.clone()),
        skip: 0,
        genre: None,
    };
    let fresh = || {
        backing(catalogs, kind, "fresh").map(|c| Fetch {
            genre: (c.shelf != kind).then(|| ANIME_GENRE.to_owned()),
            ..whole(c)
        })
    };
    if let Some(list) = sort.strip_prefix("list:") {
        let catalog = catalogs.iter().find(|c| c.choice.id == list)?;
        return Some(Source::Paged {
            catalog: Box::new(catalog.clone()),
            fixed_genre: None,
        });
    }
    if let Some(p) = platform {
        let lists: Vec<Fetch> = service_lists(catalogs, kind, Some(p))
            .into_iter()
            .map(whole)
            .collect();
        if lists.is_empty() {
            return None;
        }
        let (order, keep) = match sort {
            "fresh" => (Order::Source, Some(fresh()?)),
            s => (order(s)?, None),
        };
        return Some(Source::Set {
            lists,
            order,
            keep,
            also: fresh(),
            note: None,
        });
    }
    match sort {
        "imdb" | "newest" | "oldest" | "az" if kind == "anime" => {
            let popular = backing(catalogs, kind, "popular")?;
            Some(Source::Set {
                lists: KITSU_SKIPS
                    .iter()
                    .map(|&skip| Fetch {
                        skip,
                        ..whole(popular)
                    })
                    .collect(),
                order: order(sort)?,
                keep: None,
                also: None,
                note: Some("of 100 popular titles, not a complete top 100"),
            })
        }
        "imdb" | "newest" | "oldest" | "az" => {
            let lists: Vec<Fetch> = service_lists(catalogs, kind, None)
                .into_iter()
                .map(whole)
                .collect();
            (!lists.is_empty()).then(|| Source::Set {
                lists,
                order: order(sort).unwrap_or(Order::Source),
                keep: None,
                also: None,
                note: Some("on your services"),
            })
        }
        "fresh" => {
            let f = fresh()?;
            Some(Source::Paged {
                catalog: f.catalog,
                fixed_genre: f.genre,
            })
        }
        s => backing(catalogs, kind, s).map(|c| Source::Paged {
            catalog: Box::new(c.clone()),
            fixed_genre: None,
        }),
    }
}

/// The C40 pick for a link: the first catalog of the kind, in addon order,
/// whose genres include `genre` (any case), with the genre in its own
/// spelling; with none, the kind's first catalog that needs no genre (else
/// its first) and the genre it could not show.
fn link_pick<'a>(
    catalogs: &'a [Catalog],
    kind: &str,
    genre: &str,
) -> Option<(&'a Catalog, Option<String>, Option<String>)> {
    let of_kind = |c: &&Catalog| c.shelf == kind && c.service.is_none();
    let listed = catalogs.iter().filter(of_kind).find_map(|c| {
        let own = c.genres.iter().find(|x| x.eq_ignore_ascii_case(genre))?;
        Some((c, own.clone()))
    });
    match listed {
        Some((c, own)) => Some((c, Some(own), None)),
        None => {
            let c = (catalogs.iter().filter(of_kind))
                .find(|c| !c.genre_required)
                .or_else(|| catalogs.iter().find(of_kind))?;
            Some((c, None, Some(genre.to_owned())))
        }
    }
}

impl Inner {
    /// Fits `choice` to what is installed, publishes the new list's
    /// controls and loads it; `remember` writes the result for the profile.
    fn choose(&self, choice: DiscoverChoice, remember: bool) {
        let reload = {
            let mut s = lock(&self.state);
            let s = &mut *s;
            let current = (
                s.snapshot.kind.clone(),
                s.snapshot.platform.clone(),
                s.snapshot.sort.clone(),
                s.snapshot.genre.clone(),
                s.snapshot.decade.clone(),
            );
            let link = choice.catalog.is_some()
                || (choice.kind.is_some()
                    && matches!(choice.genre, Some(Some(_)))
                    && choice.platform.is_none()
                    && choice.sort.is_none()
                    && choice.decade.is_none());
            let mut missing_genre = None;
            let mut missing_sort = None;
            let kind;
            let platform;
            let sort;
            let mut genre;
            let decade;
            if link {
                // A link starts from a clean slate (review r2 N1).
                let picked = match &choice.catalog {
                    Some(id) => s
                        .catalogs
                        .iter()
                        .find(|c| c.choice.id == *id)
                        .map(|c| (c, choice.genre.clone().flatten(), None)),
                    None => link_pick(
                        &s.catalogs,
                        choice.kind.as_deref().unwrap_or_default(),
                        choice
                            .genre
                            .clone()
                            .flatten()
                            .as_deref()
                            .unwrap_or_default(),
                    ),
                };
                let Some((c, g, missing)) = picked else {
                    return;
                };
                kind = c.shelf.clone();
                platform = None;
                sort = sort_of(&s.catalogs, c);
                genre = g;
                missing_genre = missing;
                decade = None;
            } else {
                let kinds = kinds(&s.catalogs);
                let Some(k) = choice
                    .kind
                    .clone()
                    .or(current.0.clone())
                    .filter(|k| kinds.iter().any(|(x, _)| x == k))
                    .or_else(|| kinds.first().map(|(k, _)| k.clone()))
                else {
                    self.empty(s);
                    return;
                };
                kind = k;
                platform = match choice.platform.clone() {
                    Some(p) => p,
                    None => current.1.clone(),
                }
                .filter(|p| platforms(&s.catalogs, &kind).iter().any(|x| x.id == *p));
                genre = match choice.genre.clone() {
                    Some(g) => g,
                    None => current.3.clone(),
                };
                let asked = choice.sort.clone().or(current.2.clone());
                let offered = sorts(
                    &s.catalogs,
                    &kind,
                    platform.as_deref(),
                    genre.as_deref(),
                    &s.fresh_empty,
                );
                let lists = more_lists(&s.catalogs, &kind);
                let fits = |id: &str| {
                    offered.iter().any(|o| o.id == id)
                        || id
                            .strip_prefix("list:")
                            .is_some_and(|l| lists.iter().any(|c| c.id == l))
                };
                sort = match asked.clone().filter(|a| fits(a)) {
                    Some(a) => a,
                    None => {
                        missing_sort = asked;
                        offered
                            .first()
                            .map(|o| o.id.clone())
                            .or_else(|| lists.first().map(|c| format!("list:{}", c.id)))
                            .unwrap_or_default()
                    }
                };
                decade = match choice.decade.clone() {
                    Some(d) => d,
                    None => current.4.clone(),
                }
                .filter(|d| DECADES.contains(&d.as_str()));
            }
            let Some(source) = source(&s.catalogs, &kind, platform.as_deref(), &sort) else {
                self.empty(s);
                return;
            };
            // A paged list shows only genres it lists, in its own spelling.
            if let Source::Paged {
                catalog,
                fixed_genre,
            } = &source
            {
                genre = if fixed_genre.is_some() {
                    None
                } else {
                    genre
                        .and_then(|g| {
                            catalog
                                .genres
                                .iter()
                                .find(|x| x.eq_ignore_ascii_case(&g))
                                .cloned()
                        })
                        .or_else(|| {
                            catalog
                                .genre_required
                                .then(|| catalog.genres.first().cloned())
                                .flatten()
                        })
                };
            }
            let decade = decade.filter(|_| sort != "fresh");
            let key = source.key();
            let same = s.source.as_ref().map(Source::key) == Some(key);
            let unchanged = same
                && current
                    == (
                        Some(kind.clone()),
                        platform.clone(),
                        Some(sort.clone()),
                        genre.clone(),
                        decade.clone(),
                    );
            self.publish_controls(s, &kind, platform, sort, genre, decade, &source);
            s.snapshot.missing_genre = missing_genre;
            s.snapshot.missing_sort = missing_sort;
            if remember {
                s.remember += 1;
            }
            if unchanged {
                // The same list with the same choice: nothing to load.
                self.snapshot.set(s.snapshot.clone());
                false
            } else {
                s.source = Some(source);
                s.generation += 1;
                s.snapshot.items.clear();
                s.set.clear();
                s.shown = 0;
                s.snapshot.preview = None;
                s.preview = None;
                s.seen.clear();
                s.skip_ahead = 0;
                s.snapshot.scope.looked = None;
                s.snapshot.state = DiscoverState::Loading;
                self.snapshot.set(s.snapshot.clone());
                true
            }
        };
        if reload {
            self.spawn();
        }
        if remember {
            self.remember_later();
        }
    }

    /// No addon gives anything to browse.
    fn empty(&self, s: &mut State) {
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        s.snapshot = empty_snapshot(&s.catalogs);
        s.source = None;
        s.generation += 1;
        s.seen.clear();
        s.skip_ahead = 0;
        s.set.clear();
        self.snapshot.set(s.snapshot.clone());
    }

    /// The controls for a list: kind, service, sort, genre and decade, what
    /// each menu offers, and the screen before Discover v2's fields.
    #[allow(clippy::too_many_arguments)]
    fn publish_controls(
        &self,
        s: &mut State,
        kind: &str,
        platform: Option<String>,
        sort: String,
        genre: Option<String>,
        decade: Option<String>,
        source: &Source,
    ) {
        let snap = &mut s.snapshot;
        snap.kind = Some(kind.to_owned());
        snap.catalogs = s
            .catalogs
            .iter()
            .filter(|c| c.shelf == kind)
            .map(|c| c.choice.clone())
            .collect();
        snap.platforms = platforms(&s.catalogs, kind);
        snap.sorts = sorts(
            &s.catalogs,
            kind,
            platform.as_deref(),
            genre.as_deref(),
            &s.fresh_empty,
        );
        snap.lists = more_lists(&s.catalogs, kind);
        snap.decades = if sort == "fresh" {
            Vec::new()
        } else {
            DECADES.iter().map(|d| (*d).to_owned()).collect()
        };
        match source {
            Source::Paged {
                catalog,
                fixed_genre,
            } => {
                snap.catalog = Some(catalog.choice.id.clone());
                let own = if fixed_genre.is_some() {
                    Vec::new()
                } else {
                    catalog.genres.clone()
                };
                snap.genre_label = genre_label(&own).to_owned();
                snap.genre_choices = own
                    .iter()
                    .map(|g| GenreChoice {
                        name: g.clone(),
                        count: None,
                    })
                    .collect();
                snap.genres = own;
                snap.genre_required = catalog.genre_required && fixed_genre.is_none();
                snap.scope = Scope::default();
            }
            Source::Set { lists, note, .. } => {
                snap.catalog = None;
                snap.genres = Vec::new();
                snap.genre_choices = Vec::new();
                snap.genre_label = "Genre".into();
                snap.genre_required = false;
                let services: HashSet<&str> = lists
                    .iter()
                    .map(|f| {
                        f.catalog
                            .service
                            .map_or(f.catalog.choice.id.as_str(), |s| s.id)
                    })
                    .collect();
                snap.scope = Scope {
                    services: services.len() as u32,
                    note: note.map(str::to_owned),
                    ..Scope::default()
                };
            }
        }
        snap.platform = platform;
        snap.sort = Some(sort);
        snap.genre = genre;
        snap.decade = decade;
    }

    fn load_more(&self) {
        {
            let mut s = lock(&self.state);
            let s = &mut *s;
            // A complete set shows its next window; a failed one is asked
            // again below.
            if matches!(s.source, Some(Source::Set { .. }))
                && s.snapshot.state == DiscoverState::Ready
            {
                s.shown = (s.shown + WINDOW).min(s.set.len());
                s.snapshot.items = s.set[..s.shown].to_vec();
                if s.shown == s.set.len() {
                    s.snapshot.state = DiscoverState::End;
                }
                self.snapshot.set(s.snapshot.clone());
                return;
            }
            // A failed page is asked again, the catalog itself when its
            // first page failed (web journey review r3 P3-4).
            if !matches!(
                s.snapshot.state,
                DiscoverState::Ready | DiscoverState::Failed { .. }
            ) {
                return;
            }
            s.snapshot.state = DiscoverState::Loading;
            self.snapshot.set(s.snapshot.clone());
        }
        self.spawn();
    }

    fn spawn(&self) {
        let (source, generation) = {
            let s = lock(&self.state);
            (s.source.clone(), s.generation)
        };
        let Some(source) = source else { return };
        let (core, state, snapshot) =
            (self.core.clone(), self.state.clone(), self.snapshot.clone());
        // An older load is not aborted: it finishes its lists into the
        // cache, and its generation keeps it from writing the screen.
        let task = tokio::spawn(async move {
            match source {
                Source::Paged {
                    catalog,
                    fixed_genre,
                } => load_page(core, state, snapshot, generation, *catalog, fixed_genre).await,
                Source::Set {
                    lists,
                    order,
                    keep,
                    also,
                    ..
                } => load_set(core, state, snapshot, generation, lists, order, keep, also).await,
            }
        });
        *lock(&self.task) = Some(task);
    }

    /// Writes the choice for the profile once it has settled.
    fn remember_later(&self) {
        let at = lock(&self.state).remember;
        let (state, core) = (self.state.clone(), self.core.clone());
        tokio::spawn(async move {
            tokio::time::sleep(REMEMBER_AFTER).await;
            let s = lock(&state);
            if s.remember == at {
                remember(&core, &s.snapshot);
            }
        });
    }
}

/// Writes the leaves of the remembered choice that changed.
fn remember(core: &Core, snap: &DiscoverSnapshot) {
    let now = DiscoverPrefs {
        kind: snap.kind.clone(),
        platform: snap.platform.clone(),
        sort: snap.sort.clone(),
        genre: snap.genre.clone(),
        decade: snap.decade.clone(),
    };
    let was = core.prefs().discover;
    let mut patch = serde_json::Map::new();
    let mut leaf = |name: &str, a: &Option<String>, b: &Option<String>| {
        if a != b {
            patch.insert(name.to_owned(), serde_json::json!(a));
        }
    };
    leaf("kind", &now.kind, &was.kind);
    leaf("platform", &now.platform, &was.platform);
    leaf("sort", &now.sort, &was.sort);
    leaf("genre", &now.genre, &was.genre);
    leaf("decade", &now.decade, &was.decade);
    if !patch.is_empty() && !core.policy().kids() {
        let _ = core.set_prefs(&serde_json::json!({ "discover": patch }));
    }
}

impl Drop for DiscoverView {
    fn drop(&mut self) {
        self.watch.abort();
        if let Some(task) = lock(&self.inner.task).take() {
            task.abort();
        }
        // A choice made just before leaving is not lost to the debounce.
        let s = lock(&self.inner.state);
        if s.remember > 0 {
            remember(&self.inner.core, &s.snapshot);
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Catalogs a user can browse: every required extra has options to pick
/// from (so search-only catalogs and id-list feeds are left out). A twin,
/// titled as a catalog before it in addon order, is left out as on Home
/// unless the user turned its row on in Customize Home: no addon's name
/// tells the two apart (the owner, 2026-10-22). Catalogs of a known type
/// come before those filed by their name (`shelf`), so a kind picked with a
/// genre lands on a catalog that offers genres of that kind.
pub(crate) fn browsable(addons: &[InstalledAddon], rows: &HomeRows) -> Vec<Catalog> {
    let mut titles = HashSet::new();
    let mut catalogs: Vec<Catalog> = addons
        .iter()
        .flat_map(|addon| addon.manifest.catalogs.iter().map(move |c| (addon, c)))
        .filter_map(|(addon, c)| {
            c.default_required_extra()?;
            let name = c.name.as_deref().unwrap_or(&c.id);
            let title = catalog_title(name, &c.r#type, &addon.manifest.name).to_lowercase();
            // Inserted first, so the first of a title counts even when a later one is shown.
            if !titles.insert(title) && !rows.shown.contains(&row_id(addon, &c.r#type, &c.id)) {
                return None;
            }
            let genre = c.extra.iter().find(|e| e.name == "genre");
            let name = without_source(name, &addon.manifest.name);
            let service = services::of_catalog(&name).filter(|_| kind_label(&c.r#type).is_some());
            let backs = BACKS
                .iter()
                .find(|(addon_id, id, _)| *addon_id == addon.manifest.id && *id == c.id)
                .map(|(_, _, sort)| *sort);
            Some(Catalog {
                shelf: match service {
                    Some(s) if s.anime => "anime".to_owned(),
                    _ => shelf(&c.r#type, &name),
                },
                choice: CatalogChoice {
                    id: format!("{}|{}|{}", addon.id(), c.r#type, c.id),
                    name,
                    source: addon.manifest.name.clone(),
                },
                addon: addon.clone(),
                kind: c.r#type.clone(),
                id: c.id.clone(),
                genres: genre
                    .as_ref()
                    .map(|g| g.options.clone())
                    .unwrap_or_default(),
                genre_required: genre.is_some_and(|g| g.is_required),
                pages: c.extra.iter().any(|e| e.name == "skip"),
                service,
                backs,
            })
        })
        .collect();
    // Stable: addon order holds within each part.
    catalogs.sort_by_key(|c| kind_label(&c.kind).is_none());
    catalogs
}

/// The kind a catalog is filed under: its type when Marquee knows it, else
/// the one kind its name says ("Popular movies" is a movie list), else
/// "other". `name` is the catalog's name without its addon's (from Anime
/// Lists, "Anime Lists - Best" is "Best"), so an addon's own name never
/// files it. Its items cannot say: none is fetched before a kind is picked.
fn shelf(kind: &str, name: &str) -> String {
    if kind_label(kind).is_some() {
        return kind.to_owned();
    }
    const NAMED: [(&str, &[&str]); 3] = [
        ("movie", &["movie", "movies", "film", "films"]),
        ("series", &["series", "show", "shows"]),
        ("anime", &["anime"]),
    ];
    let words: Vec<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .collect();
    let mut said = NAMED
        .iter()
        .filter(|(_, named)| words.iter().any(|w| named.contains(&w.as_str())));
    match (said.next(), said.next()) {
        (Some((kind, _)), None) => (*kind).to_owned(),
        _ => OTHER_KIND.0.to_owned(),
    }
}

/// Loads the next page of a paged list. With a decade picked, pages are
/// asked `IN_FLIGHT` at a time while they bring too few matches, within
/// `DECADE_BUDGET`, and each page's matches are published as they come.
async fn load_page(
    core: Core,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<DiscoverSnapshot>,
    generation: u64,
    catalog: Catalog,
    fixed_genre: Option<String>,
) {
    let (genre, decade, listed) = {
        let s = lock(&state);
        (
            fixed_genre.or_else(|| s.snapshot.genre.clone()),
            s.snapshot.decade.clone(),
            s.seen.len() + s.skip_ahead,
        )
    };
    let Some(decade) = decade else {
        return load_one(
            &core, &state, &snapshot, generation, &catalog, genre, listed,
        )
        .await;
    };
    // A decade looks through the list: the pages after what was read.
    let started = Instant::now();
    let mut next = listed;
    let mut step = 0;
    let mut found = 0;
    let mut retried = false;
    loop {
        let skips: Vec<usize> = if step == 0 || !catalog.pages {
            vec![next]
        } else {
            (0..IN_FLIGHT).map(|i| next + i * step).collect()
        };
        let pages = futures_util::future::join_all(
            skips
                .iter()
                .map(|&skip| fetch_page(&core, &catalog, genre.as_deref(), skip)),
        )
        .await;
        let mut s = lock(&state);
        if s.generation != generation {
            return;
        }
        let mut new = 0;
        let mut answered = 0;
        let mut failed = None;
        for (skip, page) in skips.iter().zip(pages) {
            match page {
                Ok(ResourceResponse::Metas { metas }) => {
                    answered += metas.len();
                    step = step.max(metas.len());
                    next = next.max(skip + metas.len());
                    let covers = core.covers(metas.iter().map(|m| m.id.as_str()));
                    for meta in metas {
                        if s.seen.insert(meta.id.clone()) {
                            new += 1;
                            let tile = tile_from_preview(meta, &covers);
                            if tile_in_decade(&tile, &decade) {
                                found += 1;
                                s.snapshot.items.push(tile);
                            }
                        }
                    }
                }
                Ok(_) => failed = Some("addon returned no catalog items".to_owned()),
                Err(reason) => failed = Some(reason),
            }
        }
        s.snapshot.scope.looked = Some(s.seen.len() as u32);
        // A page that repeats an earlier one is passed once (Kitsu).
        if new == 0 && answered > 0 && catalog.pages && !retried {
            retried = true;
            s.skip_ahead += step;
            next += step;
            continue;
        }
        let end = new == 0 || !catalog.pages;
        let done = end || found >= DECADE_FILL || started.elapsed() >= DECADE_BUDGET;
        s.snapshot.state = match failed {
            Some(reason) if new == 0 => DiscoverState::Failed { reason },
            _ if end => DiscoverState::End,
            _ if done => DiscoverState::Ready,
            _ => DiscoverState::Loading,
        };
        fill_preview(&core, &mut s);
        snapshot.set(s.snapshot.clone());
        if done || matches!(s.snapshot.state, DiscoverState::Failed { .. }) {
            return;
        }
    }
}

/// One page of a paged list, as before Discover v2: a page that brings
/// nothing new is asked once more past itself before the list ends.
async fn load_one(
    core: &Core,
    state: &Arc<Mutex<State>>,
    snapshot: &SharedObservable<DiscoverSnapshot>,
    generation: u64,
    catalog: &Catalog,
    genre: Option<String>,
    listed: usize,
) {
    let mut skip = listed;
    // A page that brings nothing new is asked once more past itself before
    // the list ends: Anime Kitsu answers page one again for a skip of 1 to
    // 39 (its own code calls Kitsu's offset paging unreliable), so without
    // this its lists end after 20.
    let mut retried = false;
    loop {
        let parsed = fetch_page(core, catalog, genre.as_deref(), skip).await;
        let mut s = lock(state);
        if s.generation != generation {
            return;
        }
        s.snapshot.state = match parsed {
            Ok(ResourceResponse::Metas { metas }) => {
                let (before, len) = (s.snapshot.items.len(), metas.len());
                let covers = core.covers(metas.iter().map(|m| m.id.as_str()));
                for meta in metas {
                    if s.seen.insert(meta.id.clone()) {
                        s.snapshot.items.push(tile_from_preview(meta, &covers));
                    }
                }
                let new = s.snapshot.items.len() > before;
                if catalog.pages && !new && len > 0 && !retried {
                    retried = true;
                    skip += len;
                    continue;
                }
                if new {
                    // Later pages start past the page the addon repeated.
                    s.skip_ahead += skip - listed;
                }
                // No new titles, or no paging: the catalog is done.
                if catalog.pages && new {
                    DiscoverState::Ready
                } else {
                    DiscoverState::End
                }
            }
            Ok(_) => DiscoverState::Failed {
                reason: "addon returned no catalog items".into(),
            },
            Err(reason) => DiscoverState::Failed { reason },
        };
        // A preview named before its item came.
        fill_preview(core, &mut s);
        snapshot.set(s.snapshot.clone());
        return;
    }
}

/// Loads a complete set: the lists not decoded yet are asked at once; what
/// has come by `SOFT_DEADLINE` is published, and the rest merges in once,
/// when every list has answered or failed.
#[allow(clippy::too_many_arguments)]
async fn load_set(
    core: Core,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<DiscoverSnapshot>,
    generation: u64,
    lists: Vec<Fetch>,
    order: Order,
    keep: Option<Fetch>,
    also: Option<Fetch>,
) {
    let wanted: Vec<Fetch> = lists
        .iter()
        .chain(keep.iter())
        .chain(also.iter())
        .cloned()
        .collect();
    let missing: Vec<Fetch> = {
        let s = lock(&state);
        wanted
            .iter()
            .filter(|f| !s.lists.contains_key(&f.key()))
            .cloned()
            .collect()
    };
    let mut asked: FuturesUnordered<_> = missing
        .into_iter()
        .map(|f| {
            let core = core.clone();
            async move {
                let page = fetch_page(&core, &f.catalog, f.genre.as_deref(), f.skip).await;
                (f, page)
            }
        })
        .collect();
    let mut failed = 0u32;
    let deadline = tokio::time::sleep(SOFT_DEADLINE);
    tokio::pin!(deadline);
    let mut published = false;
    let mut stale = false;
    loop {
        tokio::select! {
            next = asked.next() => match next {
                Some((f, page)) => {
                    let items = match page {
                        Ok(ResourceResponse::Metas { metas }) => {
                            let covers = core.covers(metas.iter().map(|m| m.id.as_str()));
                            metas.into_iter().map(|m| Item::of(m, &covers)).collect()
                        }
                        _ => {
                            failed += 1;
                            continue;
                        }
                    };
                    lock(&state).lists.insert(f.key(), Arc::new(items));
                }
                None => break,
            },
            _ = &mut deadline, if !published => {
                published = true;
                stale = !compose(&core, &state, &snapshot, generation, &lists, order, keep.as_ref(), failed, asked.len());
            }
        }
    }
    if !stale {
        compose(
            &core,
            &state,
            &snapshot,
            generation,
            &lists,
            order,
            keep.as_ref(),
            failed,
            0,
        );
    }
}

/// Orders the decoded lists into the set and publishes its first window,
/// keeping as many tiles shown as before; false when the load is stale.
#[allow(clippy::too_many_arguments)]
fn compose(
    core: &Core,
    state: &Arc<Mutex<State>>,
    snapshot: &SharedObservable<DiscoverSnapshot>,
    generation: u64,
    lists: &[Fetch],
    order: Order,
    keep: Option<&Fetch>,
    failed: u32,
    pending: usize,
) -> bool {
    let mut s = lock(state);
    let s = &mut *s;
    if s.generation != generation {
        return false;
    }
    let mut seen = HashSet::new();
    let union: Vec<Item> = lists
        .iter()
        .filter_map(|f| s.lists.get(&f.key()))
        .flat_map(|l| l.iter())
        .filter(|i| seen.insert(i.id().to_owned()))
        .cloned()
        .collect();
    let kept: Option<HashSet<&str>> = keep
        .and_then(|k| s.lists.get(&k.key()))
        .map(|l| l.iter().map(Item::id).collect());
    // Picking a service learns whether it has Certified Fresh titles.
    if let (Some(platform), Some(kind)) = (s.snapshot.platform.clone(), s.snapshot.kind.clone()) {
        let fresh = backing(&s.catalogs, &kind, "fresh").map(|c| Fetch {
            catalog: Box::new(c.clone()),
            skip: 0,
            genre: (c.shelf != kind).then(|| ANIME_GENRE.to_owned()),
        });
        if let Some(rt) = fresh.and_then(|f| s.lists.get(&f.key()).cloned()) {
            let ids: HashSet<&str> = rt.iter().map(Item::id).collect();
            if !union.iter().any(|i| ids.contains(i.id()))
                && s.fresh_empty.insert(format!("{kind}|{platform}"))
            {
                s.snapshot.sorts = sorts(
                    &s.catalogs,
                    &kind,
                    Some(&platform),
                    s.snapshot.genre.as_deref(),
                    &s.fresh_empty,
                );
            }
        }
    }
    let pool: Vec<&Item> = union
        .iter()
        .filter(|i| kept.as_ref().is_none_or(|k| k.contains(i.id())))
        .collect();
    s.snapshot.genre_choices = genres_of(pool.iter().copied())
        .into_iter()
        .map(|(name, n)| GenreChoice {
            name,
            count: Some(n),
        })
        .collect();
    s.snapshot.genres = s
        .snapshot
        .genre_choices
        .iter()
        .map(|g| g.name.clone())
        .collect();
    let set: Vec<Tile> = arrange(
        pool,
        order,
        s.snapshot.genre.as_deref(),
        s.snapshot.decade.as_deref(),
        today(),
    )
    .into_iter()
    .map(|i| i.tile.clone())
    .collect();
    s.shown = s.shown.max(WINDOW).min(set.len());
    s.snapshot.items = set[..s.shown].to_vec();
    s.snapshot.scope.titles = Some(set.len() as u32);
    s.snapshot.scope.failed = failed;
    s.snapshot.scope.pending = pending as u32;
    let answered = lists
        .iter()
        .filter(|f| s.lists.contains_key(&f.key()))
        .count();
    s.snapshot.state = if answered == 0 && pending == 0 {
        DiscoverState::Failed {
            reason: "no list answered".into(),
        }
    } else if s.shown < set.len() {
        DiscoverState::Ready
    } else {
        DiscoverState::End
    };
    s.set = set;
    fill_preview(core, s);
    snapshot.set(s.snapshot.clone());
    true
}

/// One page of a catalog, `skip` items in; a failed page is asked once
/// more.
async fn fetch_page(
    core: &Core,
    catalog: &Catalog,
    genre: Option<&str>,
    skip: usize,
) -> Result<ResourceResponse, String> {
    let mut extra = Vec::new();
    if let Some(genre) = genre {
        extra.push(ExtraValue {
            name: "genre".into(),
            value: genre.to_owned(),
        });
    }
    if skip > 0 {
        extra.push(ExtraValue {
            name: "skip".into(),
            value: skip.to_string(),
        });
    }
    let path = ResourcePath {
        resource: "catalog".into(),
        r#type: catalog.kind.clone(),
        id: catalog.id.clone(),
        extra,
    };
    let mut last = String::new();
    for _ in 0..2 {
        match core.addons().fetch(&catalog.addon, &path, TIMEOUT).await {
            Ok(f) => {
                core.learn_imdb_ids(&f.body);
                return decode_catalog(&f.body).map_err(|e| format!("unreadable catalog: {e}"));
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

/// The named item's details, when it is listed; true when they changed.
fn fill_preview(core: &Core, s: &mut State) -> bool {
    let details = s.preview.as_deref().and_then(|id| {
        let tile = s.snapshot.items.iter().find(|t| t.id == id)?;
        let blurb = tile.blurb.clone().unwrap_or_default();
        Some(ItemDetails {
            id: tile.id.clone(),
            description: blurb.description,
            genres: blurb.genres,
            runtime: blurb.runtime,
            in_library: core
                .library()
                .is_in_library(core.profile(), &tile.id)
                .unwrap_or(false),
            library_settable: !core.policy().kids(),
        })
    });
    let changed = s.snapshot.preview != details;
    s.snapshot.preview = details;
    changed
}
