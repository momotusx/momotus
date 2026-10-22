//! IMDb and Rotten Tomatoes scores for every title, from MDBList
//! (plans/research/ratings.md). One developer key is baked in at build time
//! (`MARQUEE_MDBLIST_KEY`); without it nothing is asked, the IMDb score comes
//! from the addon and Rotten Tomatoes shows nothing. Answers are kept in the
//! library: fresh for 3 days, for a day when MDBList had no Tomatometer (a
//! new release gains one), and shown for up to 30 days while MDBList cannot
//! be asked. Views never wait: they show what the cache holds and patch the
//! rest in when a lookup lands.
//!
//! The Rotten Tomatoes addon, when a profile has it on, adds a mark with no
//! number: its lists name the titles it calls Fresh. The mark stays inside
//! (`Scores::fresh`) and is never shown: Rotten Tomatoes shows a number or
//! nothing (the owner, 2026-10-23). Its lists are read once a day, on their
//! own: no lookup waits for them.

use crate::Core;
use crate::views::home::Tile;
use futures_util::StreamExt;
use mq_addons::{AddonClient, InstalledAddon};
use mq_http::{HttpClient, HttpError, Priority, RequestOptions};
use mq_library::{Library, RatingRow};
use mq_protocol::{ResourcePath, ResourceResponse, decode_catalog};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use stremio_core::types::addon::ExtraValue;

/// The project's MDBList key, given at build time
/// (`MARQUEE_MDBLIST_KEY=... cargo build`) and never committed. It ships in
/// every binary and bundle, so it belongs to an MDBList account that holds
/// nothing else. Unset or empty, no MDBList call is made.
pub const KEY: Option<&str> = given(option_env!("MARQUEE_MDBLIST_KEY"));
const HOST: &str = "https://api.mdblist.com";
/// Ids per call: MDBList's limit for a supporter account.
pub const BATCH: usize = 100;
const DAY_MS: i64 = 86_400_000;
/// A row is asked again after this long...
const FRESH_MS: i64 = 3 * DAY_MS;
/// ...or after this long when it has no Tomatometer.
const FRESH_WITHOUT_RT_MS: i64 = DAY_MS;
/// A row is shown while MDBList cannot be asked, until it is this old.
const KEPT_MS: i64 = 30 * DAY_MS;
/// A failed call rests MDBList this long (one of its rate windows), or for
/// the `Retry-After` it sent, at most `MAX_REST`.
const REST: Duration = Duration::from_secs(300);
const MAX_REST: Duration = Duration::from_secs(86_400);
const TIMEOUT: Duration = Duration::from_secs(6);
/// A lookup waits this long before its round, so the catalogs of a Home
/// that answer one by one go to MDBList together.
const GATHER: Duration = Duration::from_millis(200);

/// The Rotten Tomatoes addon (a default the owner picked, 2026-10-21): its
/// catalogs list the titles Rotten Tomatoes calls Fresh, with no number.
pub const RT_ADDON: &str = "pw.ers.rottentomatoes";
/// The addon's lists are read again after this long, or after `REST` when
/// a page of them did not answer.
const LISTS_FRESH: Duration = Duration::from_secs(86_400);
/// Pages asked at once, so the global request budget stays with the
/// catalogs a Home shows.
const LIST_PAGES_AT_ONCE: usize = 4;
const LIST_TIMEOUT: Duration = Duration::from_secs(12);
/// Pages read at most: the manifest names them, and any addon can claim the
/// id (the real one has 40).
const MAX_PAGES: usize = 64;
/// Titles taken from one page (the real ones list at most 60).
const MAX_PAGE_TITLES: usize = 500;
/// A list page is refused past this size (the real ones are about 30 KB).
const LIST_MAX_BYTES: usize = 1024 * 1024;
/// A page that fails, or answers empty after it listed titles, keeps the
/// titles it last listed this long at most.
const PAGE_KEPT: Duration = Duration::from_secs(3 * 86_400);

const fn given(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// A title as MDBList is asked about it: its kind ("movie" or "show") and
/// its IMDb id.
pub type Title = (String, String);

/// A title's scores, written as shown: IMDb "8.8", Rotten Tomatoes and
/// Metacritic a number with "%" ("87%"), never a word. A score nobody gave
/// is null: apps show no slot for it, never a placeholder.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct Ratings {
    pub imdb: Option<String>,
    pub rotten_tomatoes: Option<String>,
    /// Empty until a source gives it; the apps lay out its place now.
    pub metacritic: Option<String>,
}

/// What MDBList answered for one title: IMDb's average and the Tomatometer;
/// and whether the Rotten Tomatoes addon lists it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scores {
    pub imdb: Option<f64>,
    pub rt: Option<i64>,
    pub fresh: bool,
}

impl Ratings {
    /// The row a title shows: MDBList's IMDb score, else the addon's own
    /// (`addon_imdb`, as the addon wrote it); the Tomatometer from MDBList
    /// or nothing (the Rotten Tomatoes addon's mark is never shown). A score
    /// off its scale (null, negative, above 10 for IMDb, above 100 for
    /// Rotten Tomatoes) counts as missing.
    pub fn of(scores: Option<&Scores>, addon_imdb: Option<&str>) -> Self {
        let addon = addon_imdb.and_then(|r| r.trim().parse::<f64>().ok());
        let imdb = scores
            .and_then(|s| s.imdb)
            .filter(|v| imdb_ok(*v))
            .or(addon.filter(|v| imdb_ok(*v)));
        Self {
            imdb: imdb.map(|v| format!("{v:.1}")),
            rotten_tomatoes: scores
                .and_then(|s| s.rt)
                .filter(|v| rt_ok(*v))
                .map(|v| format!("{v}%")),
            metacritic: None,
        }
    }
}

/// IMDb's scale is 1 to 10.
fn imdb_ok(v: f64) -> bool {
    (1.0..=10.0).contains(&v)
}

/// The Tomatometer is 0 to 100 percent.
fn rt_ok(v: i64) -> bool {
    (0..=100).contains(&v)
}

/// The words that credit the scores on a title page ("Ratings from"), each
/// source named for what it gave: MDBList for its numbers, the meta addon
/// (`source`, by name) for the IMDb rating it wrote (`addon_imdb`); the
/// Rotten Tomatoes addon's mark is never shown, so it is never credited.
/// `None` when only the meta addon's IMDb rating shows ("Details from"
/// names it) or nothing does.
pub(crate) fn credit(
    scores: Option<&Scores>,
    addon_imdb: Option<&str>,
    source: &str,
) -> Option<String> {
    let s = scores.copied().unwrap_or_default();
    let mdblist_imdb = s.imdb.is_some_and(imdb_ok);
    let mdblist_rt = s.rt.is_some_and(rt_ok);
    if mdblist_imdb && mdblist_rt {
        return Some("IMDb and Rotten Tomatoes via MDBList".to_owned());
    }
    let addon_imdb = addon_imdb
        .and_then(|r| r.trim().parse::<f64>().ok())
        .is_some_and(imdb_ok);
    let rt = if mdblist_rt {
        "Rotten Tomatoes via MDBList".to_owned()
    } else if mdblist_imdb {
        return Some("IMDb via MDBList".to_owned());
    } else {
        return None;
    };
    let imdb = if mdblist_imdb {
        Some("IMDb via MDBList".to_owned())
    } else {
        addon_imdb.then(|| format!("IMDb from {source}"))
    };
    Some(imdb.into_iter().chain([rt]).collect::<Vec<_>>().join(", "))
}

/// The title MDBList is asked about for a tile's kind and id: movies and
/// series with an IMDb id. Nothing else is asked (`kitsu:` ids, channels).
pub(crate) fn title_of(kind: &str, id: &str) -> Option<Title> {
    let kind = match kind {
        "movie" => "movie",
        "series" => "show",
        _ => return None,
    };
    crate::segments::is_imdb(id).then(|| (kind.to_owned(), id.to_owned()))
}

/// The titles of these tiles MDBList may know, each once.
pub(crate) fn titles<'a>(tiles: impl IntoIterator<Item = &'a Tile>) -> Vec<Title> {
    let mut titles: Vec<Title> = tiles
        .into_iter()
        .filter_map(|t| title_of(&t.kind, &t.id))
        .collect();
    titles.sort();
    titles.dedup();
    titles
}

/// Gives each tile whose title `known` holds its row, from those scores and
/// the addon's own rating. True when any tile changed.
pub(crate) fn apply<'a>(
    tiles: impl IntoIterator<Item = &'a mut Tile>,
    known: &HashMap<Title, Scores>,
) -> bool {
    let mut changed = false;
    for tile in tiles {
        let Some(scores) = title_of(&tile.kind, &tile.id).and_then(|t| known.get(&t)) else {
            continue;
        };
        let ratings = Ratings::of(Some(scores), tile.rating.as_deref());
        if tile.ratings != ratings {
            tile.ratings = ratings;
            changed = true;
        }
    }
    changed
}

/// Where MDBList answers and with which key; tests point it at a local fake.
#[derive(Clone)]
pub struct Host {
    pub url: String,
    pub key: Option<String>,
    /// The host is on this machine (tests).
    pub local: bool,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            url: HOST.into(),
            key: baked(KEY),
            local: false,
        }
    }
}

/// The key as the build gave it, trimmed: a blank one is no key.
fn baked(key: Option<&str>) -> Option<String> {
    key.map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_owned)
}

/// Never prints the key.
impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host")
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|_| "<set>"))
            .field("local", &self.local)
            .finish()
    }
}

/// Looks scores up in batches and keeps them in the library.
pub struct Finder {
    http: HttpClient,
    library: Arc<Library>,
    host: Mutex<Host>,
    /// No call goes out before this instant (a refusal, an outage).
    resting: Mutex<Option<Instant>>,
    /// Titles callers wait on; the next round takes them all.
    queue: Mutex<Vec<Title>>,
    /// One round of calls at a time, so the titles asked for during a round
    /// go together in the next one.
    round: tokio::sync::Mutex<()>,
    lists: Arc<Lists>,
    /// `Rater::turn`.
    turn: Mutex<()>,
}

/// The Rotten Tomatoes addon's lists, by the address they were read from;
/// shared with the read on its way.
struct Lists {
    by_url: Mutex<HashMap<String, Listed>>,
    /// Moves on when a read changes the titles a list holds.
    changed: tokio::sync::watch::Sender<u64>,
}

#[derive(Default)]
struct Listed {
    /// Each page's titles, by the page.
    pages: HashMap<String, Page>,
    /// Every page's titles together.
    titles: Arc<HashSet<Title>>,
    /// Read again from this instant; none until a first read has ended.
    due: Option<Instant>,
    /// A read is on its way.
    reading: bool,
}

/// The titles one page listed, and when it last answered with them.
struct Page {
    titles: HashSet<Title>,
    answered: Instant,
}

impl Finder {
    pub fn new(http: HttpClient, library: Arc<Library>) -> Self {
        Self {
            http,
            library,
            host: Mutex::new(Host::default()),
            resting: Mutex::default(),
            queue: Mutex::default(),
            round: tokio::sync::Mutex::new(()),
            lists: Arc::new(Lists {
                by_url: Mutex::default(),
                changed: tokio::sync::watch::channel(0).0,
            }),
            turn: Mutex::default(),
        }
    }

    /// The cached scores of these titles (rows up to 30 days old). With the
    /// lists of a profile's Rotten Tomatoes addon, `marks`, every title is
    /// in the answer, marked or not, so a title they no longer list loses
    /// its mark wherever the answer lands.
    fn known(&self, titles: &[Title], marks: Option<&HashSet<Title>>) -> HashMap<Title, Scores> {
        let now = now_ms();
        let mut known: HashMap<Title, Scores> = self
            .rows(titles)
            .into_iter()
            .filter(|r| now - r.fetched_ms < KEPT_MS)
            .map(|r| {
                let scores = Scores {
                    imdb: r.imdb,
                    rt: r.rt,
                    fresh: false,
                };
                ((r.kind, r.imdb_id), scores)
            })
            .collect();
        if let Some(marks) = marks {
            for title in titles {
                known.entry(title.clone()).or_default().fresh = marks.contains(title);
            }
        }
        known
    }

    /// Asks MDBList about the titles whose rows are missing or stale. False
    /// when nothing was due or nothing could be asked (no key, MDBList
    /// resting).
    async fn ask_due(&self, titles: &[Title]) -> bool {
        let host = lock(&self.host).clone();
        let Some(key) = host.key.clone() else {
            return false;
        };
        // A development build whose requests go to the e2e recorder: the
        // recorder would keep the key on disk and spend the shared quota.
        if !host.local && mq_http::dev_replaying() {
            return false;
        }
        let mine = self.due(titles, now_ms());
        if mine.is_empty() || self.resting() {
            return false;
        }
        lock(&self.queue).extend(mine.iter().cloned());
        tokio::time::sleep(GATHER).await;
        {
            let _round = self.round.lock().await;
            let mut batch = std::mem::take(&mut *lock(&self.queue));
            // A round cancelled half way took these with it.
            batch.extend(mine);
            batch.sort();
            batch.dedup();
            // The round before this one may have answered some.
            let batch = self.due(&batch, now_ms());
            if !batch.is_empty() && !self.resting() {
                self.ask(&host, &key, batch).await;
            }
        }
        true
    }

    fn rows(&self, titles: &[Title]) -> Vec<RatingRow> {
        self.library.ratings(titles).unwrap_or_else(|e| {
            tracing::warn!("ratings cache unreadable: {e}");
            Vec::new()
        })
    }

    /// The titles without a row, or whose row is stale.
    fn due(&self, titles: &[Title], now: i64) -> Vec<Title> {
        let rows: HashMap<Title, RatingRow> = self
            .rows(titles)
            .into_iter()
            .map(|r| ((r.kind.clone(), r.imdb_id.clone()), r))
            .collect();
        titles
            .iter()
            .filter(|t| rows.get(*t).is_none_or(|r| stale(r, now)))
            .cloned()
            .collect()
    }

    /// One round: per kind, `BATCH` ids a call, an IMDb call and a
    /// Tomatometer call for each batch. A failure rests MDBList and ends the
    /// round; when only one of the two calls failed, the other's scores are
    /// stored, the failed source keeps what the row had, and the row stays
    /// due, so it is asked again once the rest is over.
    async fn ask(&self, host: &Host, key: &str, titles: Vec<Title>) {
        let removals = self.library.removals();
        let had: HashMap<Title, RatingRow> = self
            .rows(&titles)
            .into_iter()
            .map(|r| ((r.kind.clone(), r.imdb_id.clone()), r))
            .collect();
        let mut by_kind: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (kind, id) in titles {
            by_kind.entry(kind).or_default().push(id);
        }
        for (kind, ids) in by_kind {
            for batch in ids.chunks(BATCH) {
                if self.resting() {
                    return;
                }
                let (imdb, rt) = futures_util::join!(
                    self.call(host, key, &kind, "imdb", batch),
                    self.call(host, key, &kind, "tomatoes", batch)
                );
                if imdb.is_none() && rt.is_none() {
                    return;
                }
                let partial = imdb.is_none() || rt.is_none();
                let now = now_ms();
                let rows: Vec<RatingRow> = batch
                    .iter()
                    .map(|id| {
                        let was = had.get(&(kind.clone(), id.clone()));
                        RatingRow {
                            imdb_id: id.clone(),
                            kind: kind.clone(),
                            imdb: match &imdb {
                                Some(found) => found.get(id).copied().filter(|v| imdb_ok(*v)),
                                None => was.and_then(|w| w.imdb),
                            },
                            rt: match &rt {
                                Some(found) => found
                                    .get(id)
                                    .map(|v| v.round() as i64)
                                    .filter(|v| rt_ok(*v)),
                                None => was.and_then(|w| w.rt),
                            },
                            // Due either way: a new row as 3 days old, an old one at its
                            // own age, so the failed source's value still ends at 30 days.
                            fetched_ms: if partial {
                                was.map_or(now - FRESH_MS, |w| w.fetched_ms)
                            } else {
                                now
                            },
                        }
                    })
                    .collect();
                if let Err(e) = self.library.put_ratings(&rows, now - KEPT_MS, removals) {
                    tracing::warn!("ratings not stored: {e}");
                }
                if partial {
                    return;
                }
            }
        }
    }

    /// One batch call (`POST /rating/{kind}/{source}`): the score of each id
    /// MDBList scored. `None` when the call failed; MDBList then rests.
    async fn call(
        &self,
        host: &Host,
        key: &str,
        kind: &str,
        source: &str,
        ids: &[String],
    ) -> Option<HashMap<String, f64>> {
        let url = url::Url::parse_with_params(
            &format!("{}/rating/{kind}/{source}", host.url),
            [("apikey", key)],
        )
        .ok()?;
        let opts = RequestOptions {
            priority: Priority::Prefetch,
            timeout: TIMEOUT,
            max_bytes: 1024 * 1024,
            allow_private: host.local,
            ..Default::default()
        };
        let body = serde_json::json!({"provider": "imdb", "ids": ids});
        // Logs name the status or the error's kind only: the URL holds the key.
        let (wait, why) = match self.http.post_json(url.as_str(), &body, &opts).await {
            Ok(response) => {
                if let Some(left) = response.header("x-ratelimit-remaining") {
                    tracing::debug!(left, "MDBList reads left today");
                }
                match response.json::<Value>().ok().as_ref().and_then(scored) {
                    Some(scores) => return Some(scores),
                    None => (
                        REST,
                        format!("sent a {source} answer this core cannot read"),
                    ),
                }
            }
            // A key that is revoked or refused stays so: once a day is enough.
            Err(HttpError::Status {
                status: status @ (401 | 403),
                ..
            }) => (MAX_REST, format!("refused the key ({status})")),
            Err(HttpError::Status {
                status,
                retry_after,
                ..
            }) => (
                retry_after.map_or(REST, |r| r.max(Duration::from_secs(1))),
                format!("refused the {source} lookup ({status})"),
            ),
            Err(e) => (REST, format!("{source} lookup failed ({})", e.kind())),
        };
        self.rest(wait.min(MAX_REST), &why);
        None
    }

    fn resting(&self) -> bool {
        lock(&self.resting).is_some_and(|until| Instant::now() < until)
    }

    /// Rests MDBList for `wait`; the warning goes out once, when a rest starts.
    fn rest(&self, wait: Duration, why: &str) {
        let Some(until) = Instant::now().checked_add(wait) else {
            return;
        };
        let mut resting = lock(&self.resting);
        let started = resting.is_none_or(|old| old <= Instant::now());
        *resting = Some(resting.map_or(until, |old| old.max(until)));
        if started {
            tracing::warn!("MDBList {why}: no ratings lookups for {} s", wait.as_secs());
        }
    }
}

/// A read on its way: however it ends (a panic, its runtime going away), it
/// clears `reading`, so reads never stop for good.
struct Reading {
    lists: Arc<Lists>,
    url: String,
}

impl Drop for Reading {
    fn drop(&mut self) {
        if let Some(listed) = lock(&self.lists.by_url).get_mut(&self.url) {
            listed.reading = false;
        }
    }
}

/// Runs `put` each time a read of the Rotten Tomatoes lists changes what
/// they hold, until the core goes: how a live view (Home, a title page,
/// Discover) puts the marks on when the read it never waited for ends.
/// `listed` is taken when the view opens, so no read that ends later goes
/// unheard.
pub(crate) async fn on_listed(
    mut listed: tokio::sync::watch::Receiver<u64>,
    mut put: impl FnMut(),
) {
    while listed.changed().await.is_ok() {
        put();
    }
}

impl Lists {
    /// The titles the addon at `url` lists, once a read of them has ended.
    fn marks(&self, url: &str) -> Option<Arc<HashSet<Title>>> {
        let by_url = lock(&self.by_url);
        let listed = by_url.get(url)?;
        listed.due.map(|_| listed.titles.clone())
    }

    /// Starts a read of the addon's lists when they are due (a day after
    /// the last read, `REST` after one that missed a page) and no read is on
    /// its way. The read runs on its own: no caller waits for it, and one
    /// that goes away does not end it.
    fn read_when_due(self: &Arc<Self>, addons: &AddonClient, addon: &InstalledAddon) {
        let url = addon.transport_url.to_string();
        {
            let mut by_url = lock(&self.by_url);
            let listed = by_url.entry(url.clone()).or_default();
            if listed.reading || listed.due.is_some_and(|due| Instant::now() < due) {
                return;
            }
            listed.reading = true;
        }
        let reading = Reading {
            lists: self.clone(),
            url,
        };
        let addons = addons.with_max_bytes(LIST_MAX_BYTES);
        let addon = addon.clone();
        tokio::spawn(async move {
            let answers = read_pages(&addons, &addon).await;
            reading.lists.store(&reading.url, answers);
        });
    }

    /// Keeps a read's answers, `None` for a page that failed. A page that
    /// failed, or answered empty after it listed titles, keeps what it last
    /// listed (for `PAGE_KEPT` at most) and makes the read due again after
    /// `REST`; a page the manifest no longer names goes.
    fn store(&self, url: &str, answers: Vec<(String, Option<HashSet<Title>>)>) {
        let now = Instant::now();
        let mut by_url = lock(&self.by_url);
        let listed = by_url.entry(url.to_owned()).or_default();
        let mut pages = HashMap::new();
        let mut whole = true;
        for (page, answer) in answers {
            let old = listed
                .pages
                .remove(&page)
                .filter(|old| now.saturating_duration_since(old.answered) < PAGE_KEPT);
            let kept = match (answer, old) {
                (Some(titles), Some(old)) if titles.is_empty() && !old.titles.is_empty() => {
                    whole = false;
                    old
                }
                (Some(titles), _) => Page {
                    titles,
                    answered: now,
                },
                (None, old) => {
                    whole = false;
                    let Some(old) = old else { continue };
                    old
                }
            };
            pages.insert(page, kept);
        }
        let titles: HashSet<Title> = pages
            .values()
            .flat_map(|p| p.titles.iter().cloned())
            .collect();
        let changed = listed.due.is_none() || *listed.titles != titles;
        listed.pages = pages;
        listed.titles = Arc::new(titles);
        listed.due = Some(now + if whole { LISTS_FRESH } else { REST });
        listed.reading = false;
        drop(by_url);
        if !whole {
            tracing::warn!(
                "a Rotten Tomatoes list did not answer: read again in {} s",
                REST.as_secs()
            );
        }
        if changed {
            self.changed.send_modify(|n| *n += 1);
        }
    }
}

/// The scores as one profile sees them (`Core::ratings`): MDBList's, and the
/// Rotten Tomatoes addon's mark while the profile has that addon on. A kids
/// profile never reads the addon's lists, as its Home asks no addon for a
/// catalog (plans/m11-kids.md 14.1), and never asks MDBList.
pub struct Rater<'a> {
    core: &'a Core,
    finder: &'a Finder,
}

impl<'a> Rater<'a> {
    pub(crate) fn new(core: &'a Core, finder: &'a Finder) -> Self {
        Self { core, finder }
    }

    pub fn set_host(&self, host: Host) {
        *lock(&self.finder.host) = host;
        *lock(&self.finder.resting) = None;
    }

    /// The Rotten Tomatoes addon, when this profile has it installed and on.
    /// Only the manifest that names its id is read; the id is all it is
    /// known by (plans/research/ratings.md).
    fn addon(&self) -> Option<InstalledAddon> {
        if self.core.policy().kids() {
            return None;
        }
        let rows = self.core.library().addons(self.core.profile()).ok()?;
        rows.into_iter()
            .filter(|r| r.enabled && r.manifest_json.contains(RT_ADDON))
            .find_map(|r| {
                let manifest: mq_protocol::Manifest =
                    serde_json::from_str(&r.manifest_json).ok()?;
                if manifest.id != RT_ADDON {
                    return None;
                }
                Some(InstalledAddon {
                    transport_url: url::Url::parse(&r.transport_url).ok()?,
                    manifest,
                })
            })
    }

    fn marks(&self, addon: Option<&InstalledAddon>) -> Option<Arc<HashSet<Title>>> {
        self.finder.lists.marks(addon?.transport_url.as_str())
    }

    /// What the cache holds for these titles, the addon's mark included.
    pub fn known(&self, titles: &[Title]) -> HashMap<Title, Scores> {
        let marks = self.marks(self.addon().as_ref());
        self.finder.known(titles, marks.as_deref())
    }

    /// `known`, with every title in it: one with neither a score nor the
    /// mark shows the addon's own rating alone, so the answer puts on a
    /// tile exactly what holds now.
    pub(crate) fn shown(&self, titles: &[Title]) -> HashMap<Title, Scores> {
        let mut known = self.known(titles);
        for title in titles {
            known.entry(title.clone()).or_default();
        }
        known
    }

    /// Fills tiles from what holds now, before any lookup lands. True when
    /// any tile changed.
    pub(crate) fn fill<'t>(&self, tiles: impl IntoIterator<Item = &'t mut Tile>) -> bool {
        let mut tiles: Vec<&mut Tile> = tiles.into_iter().collect();
        let known = self.shown(&titles(tiles.iter().map(|t| &**t)));
        apply(tiles.iter_mut().map(|t| &mut **t), &known)
    }

    /// Starts a read of the Rotten Tomatoes addon's lists when due (nobody
    /// waits for it) and asks MDBList about the titles whose rows are
    /// missing or stale, then answers what holds for all of them, the mark
    /// as the addon is now. `None` when there is nothing to show: MDBList
    /// had nothing due or could not be asked (no key, MDBList resting) and
    /// the profile has no list read. A kids profile asks nothing.
    pub async fn lookup(&self, titles: &[Title]) -> Option<HashMap<Title, Scores>> {
        if self.core.policy().kids() {
            return None;
        }
        if let Some(addon) = self.addon() {
            self.finder.lists.read_when_due(self.core.addons(), &addon);
        }
        let asked = self.finder.ask_due(titles).await;
        // The addon as it is now: one turned off meanwhile gives no mark.
        let marks = self.marks(self.addon().as_ref());
        (asked || marks.is_some()).then(|| self.finder.known(titles, marks.as_deref()))
    }

    /// One working out and putting on of ratings at a time (Home's `rate`
    /// and `rerate`), so an answer worked out before the addon went off
    /// never lands after one worked out since.
    // ponytail: one lock for the device; per view if two live views ever
    // contend on it.
    pub(crate) fn turn(&self) -> std::sync::MutexGuard<'a, ()> {
        lock(&self.finder.turn)
    }

    /// Moves on when a read of the Rotten Tomatoes lists changes what they
    /// hold, for a live view to put the marks on again (`on_listed`).
    pub(crate) fn listed(&self) -> tokio::sync::watch::Receiver<u64> {
        self.finder.lists.changed.subscribe()
    }

    /// Tests: how long MDBList still rests.
    #[doc(hidden)]
    pub fn rest_left(&self) -> Option<Duration> {
        let until = (*lock(&self.finder.resting))?;
        until.checked_duration_since(Instant::now())
    }

    /// Tests: the Rotten Tomatoes lists as if read `by` earlier.
    #[doc(hidden)]
    pub fn age_lists(&self, by: Duration) {
        let back = |at: Instant| at.checked_sub(by).unwrap_or(at);
        for listed in lock(&self.finder.lists.by_url).values_mut() {
            listed.due = listed.due.map(back);
            for page in listed.pages.values_mut() {
                page.answered = back(page.answered);
            }
        }
    }

    /// Tests: whether a read of the Rotten Tomatoes lists is on its way.
    #[doc(hidden)]
    pub fn reading_lists(&self) -> bool {
        lock(&self.finder.lists.by_url).values().any(|l| l.reading)
    }
}

/// The pages of the Rotten Tomatoes addon's lists, each with the titles it
/// listed or `None` when it failed: each catalog of its that needs no extra,
/// as is and by each genre it offers (`MAX_PAGES` at most), through the
/// addon cache, `LIST_PAGES_AT_ONCE` at a time.
async fn read_pages(
    addons: &AddonClient,
    addon: &InstalledAddon,
) -> Vec<(String, Option<HashSet<Title>>)> {
    let pages: Vec<ResourcePath> = addon
        .manifest
        .catalogs
        .iter()
        .filter(|c| c.extra.iter().all(|e| !e.is_required))
        .flat_map(|c| {
            let genres: Vec<String> = c
                .extra
                .iter()
                .filter(|e| e.name == "genre")
                .flat_map(|e| e.options.clone())
                .collect();
            std::iter::once(None)
                .chain(genres.into_iter().map(Some))
                .map(|genre| ResourcePath {
                    resource: "catalog".into(),
                    r#type: c.r#type.clone(),
                    id: c.id.clone(),
                    extra: genre
                        .map(|value| ExtraValue {
                            name: "genre".into(),
                            value,
                        })
                        .into_iter()
                        .collect(),
                })
        })
        .take(MAX_PAGES)
        .collect();
    futures_util::stream::iter(pages)
        .map(|path| async move {
            let genre = path.extra.first().map_or("", |e| e.value.as_str());
            let page = format!("{}/{}/{genre}", path.r#type, path.id);
            let titles = async {
                let fetched = addons.fetch(addon, &path, LIST_TIMEOUT).await.ok()?;
                match decode_catalog(&fetched.body).ok()? {
                    ResourceResponse::Metas { metas } => Some(
                        metas
                            .iter()
                            .take(MAX_PAGE_TITLES)
                            .filter_map(|m| title_of(&m.r#type, &m.id))
                            .collect(),
                    ),
                    _ => None,
                }
            };
            (page, titles.await)
        })
        .buffer_unordered(LIST_PAGES_AT_ONCE)
        .collect()
        .await
}

/// Asked again: older than 3 days, or than 1 day without a Tomatometer, or
/// dated ahead of this device's clock.
fn stale(row: &RatingRow, now: i64) -> bool {
    let age = now - row.fetched_ms;
    !(0..FRESH_MS).contains(&age) || (row.rt.is_none() && age >= FRESH_WITHOUT_RT_MS)
}

/// `{"ratings": [{"id": "tt…", "rating": 87}, …]}`: the ids with a number.
/// `None` for another shape: no `ratings` array, or items none of which has
/// a readable `id` (an empty array is an answer).
fn scored(body: &Value) -> Option<HashMap<String, f64>> {
    let items = body.get("ratings")?.as_array()?;
    let named: Vec<(&str, &Value)> = items
        .iter()
        .filter_map(|r| Some((r.get("id")?.as_str()?, r)))
        .collect();
    if named.is_empty() && !items.is_empty() {
        return None;
    }
    Some(
        named
            .into_iter()
            .filter_map(|(id, r)| Some((id.to_owned(), r.get("rating")?.as_f64()?)))
            .collect(),
    )
}

fn now_ms() -> i64 {
    mq_library::hlc::now_ms() as i64
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scores(imdb: Option<f64>, rt: Option<i64>) -> Scores {
        Scores {
            imdb,
            rt,
            fresh: false,
        }
    }

    #[test]
    fn a_row_shows_only_scores_on_their_scale() {
        let both = Ratings::of(Some(&scores(Some(8.8), Some(87))), Some("8.7"));
        assert_eq!(both.imdb.as_deref(), Some("8.8"));
        assert_eq!(both.rotten_tomatoes.as_deref(), Some("87%"));
        // MDBList without an IMDb score: the addon's; Rotten Tomatoes never comes from it.
        let addon = Ratings::of(Some(&scores(None, None)), Some(" 8.4 "));
        assert_eq!(
            addon,
            Ratings {
                imdb: Some("8.4".into()),
                rotten_tomatoes: None,
                metacritic: None
            }
        );
        assert_eq!(Ratings::of(None, Some("7")).imdb.as_deref(), Some("7.0"));
        // Off the scale or not a number: no slot, never a placeholder.
        for (imdb, rt, addon) in [
            (Some(-1.0), Some(-1), Some("N/A")),
            (Some(10.5), Some(101), Some("")),
            (Some(0.0), None, Some("0")),
            (Some(f64::NAN), None, Some("11")),
            (None, None, None),
        ] {
            assert_eq!(
                Ratings::of(Some(&scores(imdb, rt)), addon),
                Ratings::default()
            );
        }
        // A Tomatometer of 0% is a score.
        assert_eq!(
            Ratings::of(Some(&scores(None, Some(0))), None)
                .rotten_tomatoes
                .as_deref(),
            Some("0%")
        );
    }

    /// Rotten Tomatoes shows a number or nothing (the owner, 2026-10-23):
    /// the addon's mark stays inside, is never a word on screen and is
    /// never credited.
    #[test]
    fn the_addons_mark_never_shows_as_a_word() {
        let fresh = |imdb, rt| Scores {
            fresh: true,
            ..scores(imdb, rt)
        };
        assert_eq!(
            Ratings::of(Some(&fresh(None, None)), Some("8.4")),
            Ratings {
                imdb: Some("8.4".into()),
                rotten_tomatoes: None,
                metacritic: None
            }
        );
        let number = fresh(Some(8.8), Some(87));
        assert_eq!(
            Ratings::of(Some(&number), None).rotten_tomatoes.as_deref(),
            Some("87%")
        );
        // A Tomatometer off its scale counts as missing, mark or not.
        assert_eq!(
            Ratings::of(Some(&fresh(None, Some(101))), None).rotten_tomatoes,
            None
        );
        // Each source named for what it gave, the meta addon by its name.
        let says = |scores: Scores, addon: Option<&str>| credit(Some(&scores), addon, "Cinemeta");
        assert_eq!(says(fresh(None, None), Some("8.4")), None);
        assert_eq!(says(fresh(None, None), None), None);
        assert_eq!(
            says(fresh(Some(8.8), None), Some("8.4")).as_deref(),
            Some("IMDb via MDBList")
        );
        assert_eq!(
            says(number, Some("8.4")).as_deref(),
            Some("IMDb and Rotten Tomatoes via MDBList")
        );
        assert_eq!(
            says(scores(Some(8.8), None), None).as_deref(),
            Some("IMDb via MDBList")
        );
        assert_eq!(
            says(scores(None, Some(87)), Some("8.4")).as_deref(),
            Some("IMDb from Cinemeta, Rotten Tomatoes via MDBList")
        );
        // The meta addon's rating alone: "Details from" already names it.
        assert_eq!(says(scores(None, None), Some("8.4")), None);
        assert_eq!(credit(None, None, "Cinemeta"), None);
    }

    #[test]
    fn only_movies_and_series_with_an_imdb_id_are_asked() {
        assert_eq!(
            title_of("movie", "tt1375666"),
            Some(("movie".into(), "tt1375666".into()))
        );
        assert_eq!(
            title_of("series", "tt0903747"),
            Some(("show".into(), "tt0903747".into()))
        );
        assert_eq!(title_of("anime", "kitsu:1"), None);
        assert_eq!(title_of("series", "kitsu:1"), None);
        assert_eq!(title_of("tv", "tt1"), None);
        assert_eq!(title_of("movie", "tt1375666:1:2"), None);
    }

    #[test]
    fn a_row_is_asked_again_after_3_days_or_1_without_a_tomatometer() {
        let row = |rt, age| RatingRow {
            imdb_id: "tt1".into(),
            kind: "movie".into(),
            imdb: Some(8.0),
            rt,
            fetched_ms: 10 * DAY_MS - age,
        };
        let now = 10 * DAY_MS;
        assert!(!stale(&row(Some(87), 3 * DAY_MS - 1), now));
        assert!(stale(&row(Some(87), 3 * DAY_MS), now));
        assert!(!stale(&row(None, DAY_MS - 1), now));
        assert!(stale(&row(None, DAY_MS), now));
        // Dated ahead of this clock: asked again, never kept forever.
        assert!(stale(&row(Some(87), -60_000), now));
    }

    #[test]
    fn an_answer_gives_the_ids_with_a_number() {
        let body = serde_json::json!({"provider_id": "imdb", "provider_rating": "tomatoes",
            "mediatype": "movie", "ratings": [{"id": "tt1", "rating": 87}, {"id": "tt2", "rating": null},
            {"id": "tt3"}, {"rating": 50}]});
        assert_eq!(
            scored(&body),
            Some(HashMap::from([("tt1".to_owned(), 87.0)]))
        );
        // An empty array is an answer; anything else of another shape is not.
        assert_eq!(
            scored(&serde_json::json!({"ratings": []})),
            Some(HashMap::new())
        );
        for other in [
            serde_json::json!({"ratings": null}),
            serde_json::json!({}),
            serde_json::json!([{"id": "tt1", "rating": 87}]),
            serde_json::json!({"ratings": [{"imdb_id": "tt1", "rating": 87}]}),
            serde_json::json!({"ratings": [{"id": 1375666, "rating": 87}]}),
        ] {
            assert_eq!(scored(&other), None, "{other}");
        }
    }

    #[test]
    fn the_key_never_prints() {
        let host = Host {
            url: "https://api.mdblist.com".into(),
            key: Some("SECRET-KEY-1234".into()),
            local: false,
        };
        assert!(!format!("{host:?}").contains("SECRET"));
    }

    #[test]
    fn a_blank_key_is_no_key() {
        assert_eq!(baked(Some("  abc123 \n")).as_deref(), Some("abc123"));
        assert_eq!(baked(Some(" \t\n")), None);
        assert_eq!(baked(None), None);
    }
}
