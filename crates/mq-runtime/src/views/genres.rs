//! Genres per kind with art, for the genre grids of Search and Discover
//! (core backlog C43): every genre the installed catalogs offer, by kind,
//! each with the backdrop of the first item of the first catalog that
//! lists it. The art is fetched in the background once per catalog and
//! genre and kept for a day, so a grid opened again asks nothing.

use crate::Core;
use crate::live::{FRAME, coalesce};
use crate::views::discover::{Catalog, browsable, kinds};
use crate::views::home::home_changed;
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_library::Table;
use mq_protocol::{ResourcePath, ResourceResponse, decode_catalog};
use serde::Serialize;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use stremio_core::types::addon::ExtraValue;
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinHandle;
use tokio::time::Instant;

const TIMEOUT: Duration = Duration::from_secs(12);
/// Art lookups at once: Cinemeta's 50 or so genres fill in within a few
/// seconds without a burst at the addon.
const AT_ONCE: usize = 6;
/// How long a genre's art holds before it is looked up again (a catalog's
/// first item changes from day to day).
const KEEP: Duration = Duration::from_secs(24 * 60 * 60);
/// How soon a lookup that failed is tried again: a grid reopened at once
/// does not ask a failing addon twice.
const RETRY: Duration = Duration::from_secs(60);

/// What the core knows of one genre's art (`Core::genre_art`): the art a
/// lookup found (`None`: the item had no backdrop), or a failed lookup, and
/// when.
#[derive(Debug, Clone)]
pub(crate) struct Looked {
    art: Option<String>,
    ok: bool,
    at: Instant,
}

/// Whether a genre's art is to be looked up now: never looked up, a found
/// one older than `KEEP`, or a failed one older than `RETRY`.
fn wants_lookup(looked: Option<&Looked>, now: Instant) -> bool {
    looked.is_none_or(|l| now.duration_since(l.at) >= if l.ok { KEEP } else { RETRY })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct GenresSnapshot {
    /// Kinds in Discover's order, each with its genres; empty on a kids
    /// profile, which has no Discover.
    pub kinds: Vec<KindGenres>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct KindGenres {
    /// As Discover's `kinds`: the kind to `select` and its label.
    pub kind: String,
    pub label: String,
    /// In catalog order, each genre once whatever its case (the first
    /// catalog's spelling); years (a catalog's filter by year) left out.
    pub genres: Vec<GenreTile>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct GenreTile {
    /// Discover's `select {kind, genre}` opens it.
    pub genre: String,
    /// The backdrop of the first item of the first catalog listing the
    /// genre; null until it is fetched, and when that item has none.
    pub art: Option<String>,
}

/// Where a genre's art comes from: its first catalog. Named, not placed:
/// a result that lands after the list changed goes where its kind and
/// genre are now, or nowhere.
struct Source {
    kind: String,
    genre: String,
    catalog: Catalog,
}

impl Source {
    fn key(&self) -> String {
        format!("{}|{}", self.catalog.choice.id, self.genre)
    }
}

/// Puts `art` on `kind`'s `genre` when the list has it; false when it does
/// not (the list changed since the lookup started).
fn put_art(snapshot: &mut GenresSnapshot, kind: &str, genre: &str, art: Option<String>) -> bool {
    let Some(tile) = snapshot
        .kinds
        .iter_mut()
        .filter(|k| k.kind == kind)
        .flat_map(|k| k.genres.iter_mut())
        .find(|t| t.genre == genre)
    else {
        return false;
    };
    tile.art = art;
    true
}

pub struct GenresView {
    inner: Arc<Inner>,
    watch: JoinHandle<()>,
}

struct Inner {
    core: Core,
    snapshot: SharedObservable<GenresSnapshot>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl GenresView {
    /// Lists the genres at once, art from earlier lookups included, fetches
    /// the art still missing, and follows the installed addons.
    pub fn open(core: &Core) -> Self {
        let changes = core.library().subscribe();
        let inner = Arc::new(Inner {
            core: core.clone(),
            snapshot: SharedObservable::new(GenresSnapshot { kinds: Vec::new() }),
            task: Mutex::new(None),
        });
        inner.refresh();
        let watched = inner.clone();
        let watch = tokio::spawn(watch_addons(watched, changes));
        Self { inner, watch }
    }

    pub fn snapshot(&self) -> GenresSnapshot {
        self.inner.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        GenresSnapshot,
        Pin<Box<dyn Stream<Item = Vec<GenresSnapshot>> + Send>>,
    ) {
        let updates = self.inner.snapshot.subscribe().map(|s| vec![s]);
        (
            self.inner.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for GenresView {
    fn drop(&mut self) {
        self.watch.abort();
        if let Some(task) = lock(&self.inner.task).take() {
            task.abort();
        }
    }
}

async fn watch_addons(inner: Arc<Inner>, mut changes: Receiver<Vec<Table>>) {
    while home_changed(&mut changes).await {
        inner.refresh();
    }
}

impl Inner {
    fn refresh(self: &Arc<Self>) {
        let core = &self.core;
        let catalogs = if core.policy().kids() {
            Vec::new()
        } else {
            browsable(
                &core.installed_addons().unwrap_or_default(),
                &core.prefs().home_rows,
            )
        };
        let (mut snapshot, sources) = genres(&catalogs);
        let now = Instant::now();
        let missing: Vec<Source> = {
            let known = lock(core.genre_art());
            sources
                .into_iter()
                .filter(|s| {
                    let looked = known.get(&s.key());
                    // Art past its day still shows while it is looked up again.
                    if let Some(l) = looked.filter(|l| l.ok) {
                        put_art(&mut snapshot, &s.kind, &s.genre, l.art.clone());
                    }
                    wants_lookup(looked, now)
                })
                .collect()
        };
        let mut task = lock(&self.task);
        if let Some(old) = task.take() {
            old.abort();
        }
        self.snapshot.set(snapshot);
        let this = self.clone();
        *task = Some(tokio::spawn(async move { this.fetch_art(missing).await }));
    }

    /// Fetches each genre's art, `AT_ONCE` at a time, and shows each as it
    /// lands. A failed lookup is kept as one, so it is tried again only
    /// after `RETRY`.
    async fn fetch_art(&self, missing: Vec<Source>) {
        let mut found = futures_util::stream::iter(missing)
            .map(|s| async move {
                let art = first_backdrop(&self.core, &s.catalog, &s.genre).await;
                (s, art)
            })
            .buffer_unordered(AT_ONCE);
        while let Some((s, art)) = found.next().await {
            let looked = Looked {
                ok: art.is_ok(),
                art: art.clone().unwrap_or(None),
                at: Instant::now(),
            };
            lock(self.core.genre_art()).insert(s.key(), looked);
            let Ok(art) = art else { continue };
            let mut snapshot = self.snapshot.get();
            if put_art(&mut snapshot, &s.kind, &s.genre, art) {
                self.snapshot.set(snapshot);
            }
        }
    }
}

/// Each kind's genres in catalog order, each once whatever its case, and
/// for each the first catalog that lists it.
fn genres(catalogs: &[Catalog]) -> (GenresSnapshot, Vec<Source>) {
    let mut sources = Vec::new();
    let snapshot = GenresSnapshot {
        kinds: kinds(catalogs)
            .into_iter()
            .map(|(kind, label)| {
                let mut genres: Vec<GenreTile> = Vec::new();
                for c in catalogs.iter().filter(|c| c.shelf == kind) {
                    for g in &c.genres {
                        if is_year(g) || genres.iter().any(|t| t.genre.eq_ignore_ascii_case(g)) {
                            continue;
                        }
                        sources.push(Source {
                            kind: kind.clone(),
                            genre: g.clone(),
                            catalog: c.clone(),
                        });
                        genres.push(GenreTile {
                            genre: g.clone(),
                            art: None,
                        });
                    }
                }
                KindGenres {
                    kind,
                    label,
                    genres,
                }
            })
            .collect(),
    };
    (snapshot, sources)
}

/// A year ("2026", Cinemeta's New by year) or a decade ("1980s"): a
/// catalog's filter, not a genre to browse.
fn is_year(option: &str) -> bool {
    let digits = option.strip_suffix('s').unwrap_or(option);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// The backdrop of the first item `catalog` lists for `genre`; `Ok(None)`
/// when it answers with no item or no backdrop, `Err` when it does not
/// answer.
async fn first_backdrop(core: &Core, catalog: &Catalog, genre: &str) -> Result<Option<String>, ()> {
    let path = ResourcePath {
        resource: "catalog".into(),
        r#type: catalog.kind.clone(),
        id: catalog.id.clone(),
        extra: vec![ExtraValue {
            name: "genre".into(),
            value: genre.to_owned(),
        }],
    };
    let fetched = core
        .addons()
        .fetch(&catalog.addon, &path, TIMEOUT)
        .await
        .map_err(|_| ())?;
    match decode_catalog(&fetched.body).map_err(|_| ())? {
        ResourceResponse::Metas { metas } => Ok(metas
            .into_iter()
            .next()
            .and_then(|m| m.background.map(|u| u.to_string()))),
        _ => Ok(None),
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(genres: &[&str]) -> GenresSnapshot {
        GenresSnapshot {
            kinds: vec![KindGenres {
                kind: "movie".into(),
                label: "Movies".into(),
                genres: genres
                    .iter()
                    .map(|g| GenreTile {
                        genre: (*g).into(),
                        art: None,
                    })
                    .collect(),
            }],
        }
    }

    /// Review P2: a result that lands after the list changed goes to its
    /// genre wherever it is now, and a genre the new list lacks takes
    /// nothing; never to whatever sits at its old place.
    #[test]
    fn a_late_result_goes_by_name_or_nowhere() {
        // The lookup started when Drama was second; Action is gone now.
        let mut now = grid(&["Drama", "Comedy"]);
        assert!(put_art(&mut now, "movie", "Drama", Some("d.jpg".into())));
        assert_eq!(now.kinds[0].genres[0].art.as_deref(), Some("d.jpg"));
        assert!(!put_art(&mut now, "movie", "Action", Some("a.jpg".into())));
        assert!(!put_art(&mut now, "series", "Comedy", Some("c.jpg".into())));
        assert_eq!(now, {
            let mut want = grid(&["Drama", "Comedy"]);
            want.kinds[0].genres[0].art = Some("d.jpg".into());
            want
        });
    }

    /// Review P3: found art holds a day, a failed lookup a minute.
    #[test]
    fn found_art_holds_a_day_and_a_failure_a_minute() {
        let at = Instant::now();
        let found = Looked {
            art: None,
            ok: true,
            at,
        };
        let failed = Looked {
            art: None,
            ok: false,
            at,
        };
        assert!(wants_lookup(None, at));
        assert!(!wants_lookup(
            Some(&found),
            at + Duration::from_secs(23 * 60 * 60)
        ));
        assert!(wants_lookup(Some(&found), at + KEEP));
        assert!(!wants_lookup(Some(&failed), at + Duration::from_secs(59)));
        assert!(wants_lookup(Some(&failed), at + RETRY));
    }

    #[test]
    fn years_and_decades_are_no_genres() {
        for year in ["2026", "1980s", "90s"] {
            assert!(is_year(year), "{year}");
        }
        for genre in ["Drama", "s", "", "Sci-Fi", "80s Action"] {
            assert!(!is_year(genre), "{genre}");
        }
    }
}
