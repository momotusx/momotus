//! IMDb and Rotten Tomatoes scores for every title, from MDBList
//! (plans/research/ratings.md). One developer key is baked in at build time
//! (`MARQUEE_MDBLIST_KEY`); without it nothing is asked, the IMDb score comes
//! from the addon and Rotten Tomatoes shows nothing. Answers are kept in the
//! library: fresh for 3 days, for a day when MDBList had no Tomatometer (a
//! new release gains one), and shown for up to 30 days while MDBList cannot
//! be asked. Views never wait: they show what the cache holds and patch the
//! rest in when a lookup lands.

use crate::views::home::Tile;
use mq_http::{HttpClient, HttpError, Priority, RequestOptions};
use mq_library::{Library, RatingRow};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

const fn given(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// A title as MDBList is asked about it: its kind ("movie" or "show") and
/// its IMDb id.
pub type Title = (String, String);

/// A title's IMDb and Rotten Tomatoes scores, written as shown ("8.8",
/// "87%"). A score nobody gave is null: apps show no slot for it, never a
/// placeholder.
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
}

/// What MDBList answered for one title: IMDb's average and the Tomatometer.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scores {
    pub imdb: Option<f64>,
    pub rt: Option<i64>,
}

impl Ratings {
    /// The row a title shows: MDBList's IMDb score, else the addon's own
    /// (`addon_imdb`, as the addon wrote it); the Tomatometer from MDBList
    /// only. A score off its scale (null, negative, above 10 for IMDb, above
    /// 100 for Rotten Tomatoes) counts as missing.
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

/// The words that credit the scores on a title page ("Ratings from"), when
/// MDBList gave one of them.
pub(crate) fn credit(scores: Option<&Scores>) -> Option<String> {
    let s = scores?;
    (s.imdb.is_some_and(imdb_ok) || s.rt.is_some_and(rt_ok))
        .then(|| "IMDb and Rotten Tomatoes via MDBList".to_owned())
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
            key: KEY
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_owned),
            local: false,
        }
    }
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
        }
    }

    pub fn set_host(&self, host: Host) {
        *lock(&self.host) = host;
        *lock(&self.resting) = None;
    }

    /// The cached scores of these titles: rows up to 30 days old.
    pub fn known(&self, titles: &[Title]) -> HashMap<Title, Scores> {
        let now = now_ms();
        self.rows(titles)
            .into_iter()
            .filter(|r| now - r.fetched_ms < KEPT_MS)
            .map(|r| {
                let scores = Scores {
                    imdb: r.imdb,
                    rt: r.rt,
                };
                ((r.kind, r.imdb_id), scores)
            })
            .collect()
    }

    /// Fills tiles from the cache, before any lookup lands.
    pub(crate) fn fill<'a>(&self, tiles: impl IntoIterator<Item = &'a mut Tile>) {
        let mut tiles: Vec<&mut Tile> = tiles.into_iter().collect();
        let known = self.known(&titles(tiles.iter().map(|t| &**t)));
        apply(tiles.iter_mut().map(|t| &mut **t), &known);
    }

    /// Asks MDBList about the titles whose rows are missing or stale, then
    /// answers what the cache holds for all of them. `None` when nothing
    /// was due or nothing could be asked (no key, MDBList resting), so the
    /// caller has nothing new to show.
    pub async fn lookup(&self, titles: &[Title]) -> Option<HashMap<Title, Scores>> {
        let host = lock(&self.host).clone();
        let key = host.key.clone()?;
        // A development build whose requests go to the e2e recorder: the
        // recorder would keep the key on disk and spend the shared quota.
        if !host.local && mq_http::dev_replaying() {
            return None;
        }
        let mine = self.due(titles, now_ms());
        if mine.is_empty() || self.resting() {
            return None;
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
        Some(self.known(titles))
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
                            // At least 3 days old: due, and still shown.
                            fetched_ms: if partial {
                                was.map_or(now - FRESH_MS, |w| w.fetched_ms.max(now - FRESH_MS))
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

    /// Tests: how long MDBList still rests.
    #[doc(hidden)]
    pub fn rest_left(&self) -> Option<Duration> {
        let until = (*lock(&self.resting))?;
        until.checked_duration_since(Instant::now())
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
        Scores { imdb, rt }
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
                rotten_tomatoes: None
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
}
