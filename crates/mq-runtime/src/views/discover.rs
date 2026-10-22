//! Discover: browse one addon catalog at a time, filtered by type and genre,
//! with more items loaded on demand through the catalog's `skip` extra.

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::views::addons_changed;
use crate::views::home::{Tile, tile_from_preview, type_label, without_source};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::InstalledAddon;
use mq_library::Table;
use mq_protocol::{ResourcePath, ResourceResponse, decode_response};
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
    /// Types with at least one catalog: ("movie", "Movies").
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
    pub items: Vec<Tile>,
    pub state: DiscoverState,
}

#[derive(Clone)]
struct Catalog {
    choice: CatalogChoice,
    addon: InstalledAddon,
    kind: String,
    id: String,
    genres: Vec<String>,
    genre_required: bool,
    pages: bool,
}

struct State {
    catalogs: Vec<Catalog>,
    snapshot: DiscoverSnapshot,
    seen: HashSet<String>,
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

fn empty_snapshot(catalogs: &[Catalog]) -> DiscoverSnapshot {
    let mut kinds: Vec<(String, String)> = Vec::new();
    for c in catalogs {
        if !kinds.iter().any(|(k, _)| *k == c.kind) {
            kinds.push((c.kind.clone(), type_label(&c.kind).to_owned()));
        }
    }
    DiscoverSnapshot {
        kinds,
        kind: None,
        catalogs: Vec::new(),
        catalog: None,
        genres: Vec::new(),
        genre: None,
        genre_required: false,
        items: Vec::new(),
        state: DiscoverState::End,
    }
}

impl DiscoverView {
    /// Opens on the first catalog of the first type, and follows the
    /// installed addons from then on.
    pub fn open(core: &Core) -> Self {
        // Subscribed before reading, so no addon change falls in between.
        let changes = core.library().subscribe();
        let catalogs = browsable(&core.installed_addons().unwrap_or_default());
        let snapshot = empty_snapshot(&catalogs);
        let inner = Arc::new(Inner {
            core: core.clone(),
            state: Arc::new(Mutex::new(State {
                catalogs,
                snapshot: snapshot.clone(),
                seen: HashSet::new(),
            })),
            snapshot: SharedObservable::new(snapshot),
            task: Mutex::new(None),
        });
        // Taken before the first read of the ratings (`on_listed`).
        let listed = core.ratings().listed();
        inner.select_first();
        let (watched, rated) = (inner.clone(), inner.clone());
        let watch = tokio::spawn(async move {
            let rate = crate::ratings::on_listed(listed, move || {
                rate_items(&rated.core, &rated.state, &rated.snapshot);
            });
            futures_util::join!(watch_addons(watched, changes), rate);
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
}

/// Follows the installed addons: the catalog on screen stays when it is
/// still there (no reload); otherwise Discover starts again on the first.
async fn watch_addons(inner: Arc<Inner>, mut changes: Receiver<Vec<Table>>) {
    while addons_changed(&mut changes).await {
        let catalogs = browsable(&inner.core.installed_addons().unwrap_or_default());
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
                    .filter(|c| kind.as_deref() == Some(&c.kind))
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
            let chosen = s
                .catalogs
                .iter()
                .find(|c| catalog.is_some_and(|id| c.choice.id == id))
                .or_else(|| {
                    kind.and_then(|k| {
                        s.catalogs
                            .iter()
                            .find(|c| c.kind == k && current.as_deref() == Some(&c.choice.id))
                            .or_else(|| s.catalogs.iter().find(|c| c.kind == k))
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
            s.snapshot.kind = Some(chosen.kind.clone());
            s.snapshot.catalogs = s
                .catalogs
                .iter()
                .filter(|c| c.kind == chosen.kind)
                .map(|c| c.choice.clone())
                .collect();
            s.snapshot.catalog = Some(chosen.choice.id.clone());
            s.snapshot.genres = chosen.genres.clone();
            s.snapshot.genre_required = chosen.genre_required;
            s.snapshot.genre = genre;
            s.snapshot.items.clear();
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
/// from (so search-only catalogs and id-list feeds are left out).
fn browsable(addons: &[InstalledAddon]) -> Vec<Catalog> {
    addons
        .iter()
        .flat_map(|addon| {
            addon.manifest.catalogs.iter().filter_map(|c| {
                c.default_required_extra()?;
                let genre = c.extra.iter().find(|e| e.name == "genre");
                Some(Catalog {
                    choice: CatalogChoice {
                        id: format!("{}|{}|{}", addon.id(), c.r#type, c.id),
                        name: without_source(
                            c.name.as_deref().unwrap_or(&c.id),
                            &addon.manifest.name,
                        ),
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
        })
        .collect()
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
        decode_response(&f.body, "catalog").map_err(|e| format!("unreadable catalog: {e}"))
    });
    let titles = {
        let mut s = lock(&state);
        s.snapshot.state = match parsed {
            Ok(ResourceResponse::Metas { metas }) => {
                let before = s.snapshot.items.len();
                for meta in metas {
                    if s.seen.insert(meta.id.clone()) {
                        s.snapshot.items.push(tile_from_preview(meta));
                    }
                }
                core.ratings().fill(&mut s.snapshot.items[before..]);
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
        snapshot.set(s.snapshot.clone());
        // Every item's, not only this page's: a page asked while the last
        // lookup was on its way cancelled it.
        crate::ratings::titles(&s.snapshot.items)
    };
    if core.ratings().lookup(&titles).await.is_some() {
        rate_items(&core, &state, &snapshot);
    }
}

/// Puts what holds now on every item shown (a lookup landed, or a read of
/// the lists ended): not what a lookup saw, which a read may have changed.
fn rate_items(core: &Core, state: &Mutex<State>, snapshot: &SharedObservable<DiscoverSnapshot>) {
    let mut s = lock(state);
    if core.ratings().fill(&mut s.snapshot.items) {
        snapshot.set(s.snapshot.clone());
    }
}
