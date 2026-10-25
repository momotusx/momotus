//! Discover: browse one addon catalog at a time, filtered by type and genre,
//! with more items loaded on demand through the catalog's `skip` extra.

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::views::home::{
    OTHER_KIND, Tile, catalog_title, home_changed, kind_label, row_id, tile_from_preview,
    type_label, without_source,
};
use crate::views::settings::HomeRows;
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::Table;
use mq_protocol::{ResourcePath, ResourceResponse, decode_catalog};
use serde::Serialize;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use stremio_core::types::addon::ExtraValue;
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinHandle;

const TIMEOUT: Duration = Duration::from_secs(12);

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
    /// Kinds with at least one catalog: ("movie", "Movies"). A catalog of a
    /// type Marquee does not know (Trakt Tv's "trakt") is filed under the
    /// kind its name says ("Popular movies"), else ("other", "Other"); the
    /// raw type is never a kind here.
    pub kinds: Vec<(String, String)>,
    pub kind: Option<String>,
    /// Catalogs of the selected type, in addon order.
    pub catalogs: Vec<CatalogChoice>,
    pub catalog: Option<String>,
    /// Genre options of the selected catalog.
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
}

struct State {
    catalogs: Vec<Catalog>,
    snapshot: DiscoverSnapshot,
    seen: HashSet<String>,
    /// The id `preview` named: kept until a select or a `None`, so an item
    /// named before its page came is filled in when it does.
    preview: Option<String>,
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

/// The kinds the catalogs are filed under, in their order, with labels:
/// ("movie", "Movies").
pub(crate) fn kinds(catalogs: &[Catalog]) -> Vec<(String, String)> {
    let mut kinds: Vec<(String, String)> = Vec::new();
    for c in catalogs {
        if !kinds.iter().any(|(k, _)| *k == c.shelf) {
            kinds.push((c.shelf.clone(), type_label(&c.shelf).to_owned()));
        }
    }
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
        items: Vec::new(),
        preview: None,
        state: DiscoverState::End,
    }
}

impl DiscoverView {
    /// Opens on the first catalog of the first type, and follows the
    /// installed addons from then on.
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
                seen: HashSet::new(),
                preview: None,
            })),
            snapshot: SharedObservable::new(snapshot),
            task: Mutex::new(None),
        });
        inner.select_first();
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

    /// Changes the type, the catalog or the genre; whatever is not given is
    /// kept when it still fits. Starts again from the first page.
    pub fn select(&self, kind: Option<&str>, catalog: Option<&str>, genre: Option<Option<&str>>) {
        self.inner.select(kind, catalog, genre);
    }

    /// Loads the next page when the catalog has more and none is loading,
    /// or asks a failed page again.
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
/// the catalog on screen stays when it is still there (no reload);
/// otherwise Discover starts again on the first.
async fn watch_addons(inner: Arc<Inner>, mut changes: Receiver<Vec<Table>>) {
    let mut key = inner.core.rpdb_key();
    while home_changed(&mut changes).await {
        // The RPDB key comes from an addon: the items already listed take the
        // band as it is now, as Home's do.
        let now = inner.core.rpdb_key();
        if now != key {
            let mut s = lock(&inner.state);
            crate::views::home::recover_tiles(
                &inner.core,
                key.take(),
                now.clone(),
                s.snapshot.items.iter_mut(),
            );
            inner.snapshot.set(s.snapshot.clone());
            key = now;
        }
        let catalogs = browsable(
            &inner.core.installed_addons().unwrap_or_default(),
            &inner.core.prefs().home_rows,
        );
        let kept = {
            let mut s = lock(&inner.state);
            let ids = |c: &[Catalog]| c.iter().map(|c| c.choice.id.clone()).collect::<Vec<_>>();
            if ids(&catalogs) == ids(&s.catalogs) {
                continue;
            }
            s.snapshot.kinds = empty_snapshot(&catalogs).kinds;
            s.catalogs = catalogs;
            let current = s.snapshot.catalog.clone();
            let kept = s
                .catalogs
                .iter()
                .any(|c| current.as_deref() == Some(&c.choice.id));
            if kept {
                let kind = s.snapshot.kind.clone();
                s.snapshot.catalogs = s
                    .catalogs
                    .iter()
                    .filter(|c| kind.as_deref() == Some(&c.shelf))
                    .map(|c| c.choice.clone())
                    .collect();
                inner.snapshot.set(s.snapshot.clone());
            }
            kept
        };
        if !kept {
            inner.select_first();
        }
    }
}

impl Inner {
    /// The first catalog, or an empty screen when no addon has one.
    fn select_first(&self) {
        let first = lock(&self.state)
            .catalogs
            .first()
            .map(|c| c.choice.id.clone());
        match first {
            Some(first) => self.select(None, Some(&first), None),
            None => {
                if let Some(task) = lock(&self.task).take() {
                    task.abort();
                }
                let mut s = lock(&self.state);
                s.snapshot = empty_snapshot(&s.catalogs);
                s.seen.clear();
                self.snapshot.set(s.snapshot.clone());
            }
        }
    }

    fn select(&self, kind: Option<&str>, catalog: Option<&str>, genre: Option<Option<&str>>) {
        {
            let mut s = lock(&self.state);
            let current = s.snapshot.catalog.clone();
            // A kind and a genre with no catalog (a title's genre chip): the
            // first catalog of the kind, in addon order, that lists it (C40).
            let asked = genre
                .flatten()
                .filter(|_| catalog.is_none() && kind.is_some());
            let of_kind = |c: &&Catalog| Some(c.shelf.as_str()) == kind;
            // Matched as the addon spells it ("drama" finds "Drama"), and
            // shown in the catalog's own spelling.
            let lists = asked.and_then(|g| {
                s.catalogs.iter().filter(of_kind).find_map(|c| {
                    let own = c.genres.iter().find(|x| x.eq_ignore_ascii_case(g))?;
                    Some((c, own.clone()))
                })
            });
            let missing_genre = asked.filter(|_| lists.is_none()).map(str::to_owned);
            let listed_genre = lists.as_ref().map(|(_, own)| own.clone());
            // With none, the kind's first catalog that needs no genre (else
            // its first): the one a chip's kind opens on, not whichever was
            // open last, and never another genre in place of the missing one
            // while a catalog can show the kind with none.
            let lists = lists.map(|(c, _)| c).or_else(|| {
                missing_genre.as_ref().and(
                    (s.catalogs.iter().filter(of_kind))
                        .find(|c| !c.genre_required)
                        .or_else(|| s.catalogs.iter().find(of_kind)),
                )
            });
            let chosen = lists
                .or_else(|| {
                    s.catalogs
                        .iter()
                        .find(|c| catalog.is_some_and(|id| c.choice.id == id))
                })
                .or_else(|| {
                    kind.and_then(|k| {
                        s.catalogs
                            .iter()
                            .find(|c| c.shelf == k && current.as_deref() == Some(&c.choice.id))
                            .or_else(|| s.catalogs.iter().find(|c| c.shelf == k))
                    })
                })
                .or_else(|| {
                    s.catalogs
                        .iter()
                        .find(|c| current.as_deref() == Some(&c.choice.id))
                })
                .cloned();
            let Some(chosen) = chosen else { return };
            let changed_catalog = current.as_deref() != Some(&chosen.choice.id);
            let genre = match genre {
                _ if listed_genre.is_some() => listed_genre,
                Some(g) => g.map(str::to_owned),
                None if changed_catalog => None,
                None => s.snapshot.genre.clone(),
            }
            .filter(|g| chosen.genres.contains(g))
            .or_else(|| {
                chosen
                    .genre_required
                    .then(|| chosen.genres.first().cloned())
                    .flatten()
            });
            s.snapshot.kind = Some(chosen.shelf.clone());
            s.snapshot.catalogs = s
                .catalogs
                .iter()
                .filter(|c| c.shelf == chosen.shelf)
                .map(|c| c.choice.clone())
                .collect();
            s.snapshot.catalog = Some(chosen.choice.id.clone());
            s.snapshot.genres = chosen.genres.clone();
            s.snapshot.genre_required = chosen.genre_required;
            s.snapshot.genre = genre;
            s.snapshot.missing_genre = missing_genre;
            s.snapshot.items.clear();
            s.snapshot.preview = None;
            s.preview = None;
            s.seen.clear();
            s.snapshot.state = DiscoverState::Loading;
            self.snapshot.set(s.snapshot.clone());
        }
        self.spawn_page();
    }

    fn load_more(&self) {
        {
            let mut s = lock(&self.state);
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
        self.spawn_page();
    }

    fn spawn_page(&self) {
        let task = tokio::spawn(load_page(
            self.core.clone(),
            self.state.clone(),
            self.snapshot.clone(),
        ));
        if let Some(old) = lock(&self.task).replace(task) {
            old.abort();
        }
    }
}

impl Drop for DiscoverView {
    fn drop(&mut self) {
        self.watch.abort();
        if let Some(task) = lock(&self.inner.task).take() {
            task.abort();
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
            Some(Catalog {
                shelf: shelf(&c.r#type, &name),
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

async fn load_page(
    core: Core,
    state: Arc<Mutex<State>>,
    snapshot: SharedObservable<DiscoverSnapshot>,
) {
    let (catalog, genre, skip) = {
        let s = lock(&state);
        let Some(catalog) = s
            .catalogs
            .iter()
            .find(|c| s.snapshot.catalog.as_deref() == Some(&c.choice.id))
            .cloned()
        else {
            return;
        };
        (catalog, s.snapshot.genre.clone(), s.snapshot.items.len())
    };
    let mut extra = Vec::new();
    if let Some(genre) = genre {
        extra.push(ExtraValue {
            name: "genre".into(),
            value: genre,
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
    let result = core.addons().fetch(&catalog.addon, &path, TIMEOUT).await;
    let parsed = result.map_err(|e| e.to_string()).and_then(|f| {
        core.learn_imdb_ids(&f.body);
        decode_catalog(&f.body).map_err(|e| format!("unreadable catalog: {e}"))
    });
    {
        let mut s = lock(&state);
        s.snapshot.state = match parsed {
            Ok(ResourceResponse::Metas { metas }) => {
                let before = s.snapshot.items.len();
                let covers = core.covers(metas.iter().map(|m| m.id.as_str()));
                for meta in metas {
                    if s.seen.insert(meta.id.clone()) {
                        s.snapshot.items.push(tile_from_preview(meta, &covers));
                    }
                }
                // No new titles, or no paging: the catalog is done.
                if catalog.pages && s.snapshot.items.len() > before {
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
        fill_preview(&core, &mut s);
        snapshot.set(s.snapshot.clone());
    }
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
