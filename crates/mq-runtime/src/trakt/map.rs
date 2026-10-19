//! Identity between the library's ids and Trakt's (plans/m7-trakt.md 6):
//! movies by IMDb id, episodes by the TVDB id the show's meta gives each
//! video. Anything else is listed as unmapped, with the reason.

use crate::{Core, TvdbMiss};
use mq_library::TrackerKey;
use mq_library::hlc::now_ms;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Tracker state names of unmapped items start with this.
pub(super) const UNMAPPED: &str = "unmapped:";
/// Tracker state names of items whose details failed start with this.
const FAILING: &str = "failing:";
/// A failure that lasts gives up: after this many failed looks (looks
/// closer than `LOOK_GAP_MS` count once) spread over at least a day. Rounds
/// run every 5 minutes in the foreground, so a count alone would give up
/// within half an hour of an addon outage; the day keeps any outage shorter
/// than that from dropping a write, and four separate failures across it
/// say the details are not coming, even for an app opened only now and
/// then.
const FAILED_LOOKS: u64 = 4;
const FAILING_FOR_MS: i64 = 24 * 60 * 60 * 1000;
const LOOK_GAP_MS: i64 = 60 * 1000;

/// What Trakt calls a video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TraktIds {
    Movie { imdb: String },
    Episode { tvdb: u64, show_imdb: String },
}

impl TraktIds {
    /// The item part of a scrobble body.
    pub(super) fn scrobble(&self, progress: f64) -> Value {
        let progress = (progress * 100.0).round() / 100.0;
        match self {
            TraktIds::Movie { imdb } => {
                json!({"movie": {"ids": {"imdb": imdb}}, "progress": progress})
            }
            TraktIds::Episode { tvdb, .. } => {
                json!({"episode": {"ids": {"tvdb": tvdb}}, "progress": progress})
            }
        }
    }

    /// The list a history body puts it in, and its entry.
    pub(super) fn history(&self, watched_at: Option<String>) -> (&'static str, Value) {
        let (list, mut entry) = match self {
            TraktIds::Movie { imdb } => ("movies", json!({"ids": {"imdb": imdb}})),
            TraktIds::Episode { tvdb, .. } => ("episodes", json!({"ids": {"tvdb": tvdb}})),
        };
        if let Some(at) = watched_at {
            entry["watched_at"] = json!(at);
        }
        (list, entry)
    }

    /// Whether a Trakt answer resolved to this item: the episode's TVDB id
    /// and its show's IMDb id must both match.
    pub(super) fn resolved(&self, answer: &Value) -> bool {
        match self {
            TraktIds::Movie { imdb } => answer["movie"]["ids"]["imdb"].as_str() == Some(imdb),
            TraktIds::Episode { tvdb, show_imdb } => {
                answer["episode"]["ids"]["tvdb"].as_u64() == Some(*tvdb)
                    && answer["show"]["ids"]["imdb"].as_str() == Some(show_imdb)
            }
        }
    }
}

/// Why an item has no Trakt ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Miss {
    /// Trakt cannot name it: no IMDb id, a special, no TVDB id in the
    /// show's details, no addon with the show, details failing for a day,
    /// or a show Trakt matched to another title. Listed as unmapped; its
    /// writes are dropped.
    Permanent(String),
    /// The show's details could not be read now: tried again later, never
    /// listed.
    Transient(String),
}

/// What a failed look was for; each counts on its own.
#[derive(Debug, Clone, Copy)]
pub(super) enum Look {
    /// A show's TVDB ids.
    Show,
    /// One episode's TVDB id, in details that have others.
    Episode,
    /// A title's runtime (for a resume point).
    Runtime,
}

impl Look {
    fn name(self, item: &str) -> String {
        let kind = match self {
            Look::Show => "show",
            Look::Episode => "episode",
            Look::Runtime => "runtime",
        };
        format!("{FAILING}{kind}:{item}")
    }
}

/// The show's TVDB ids, or why not. A failure is transient until it has
/// lasted (`FAILED_LOOKS` over a day); every addon saying it has no such
/// show is permanent at once. Only an answer counts as a look: when no
/// addon answered at all (no network, or none in time) the device may be
/// offline, and nothing is counted.
pub(super) async fn show_ids(
    core: &Core,
    key: TrackerKey<'_>,
    show: &str,
) -> Result<HashMap<String, u64>, Miss> {
    match core.episode_tvdb_ids(show).await {
        Ok(ids) => {
            succeeded(core, key, Look::Show, show);
            Ok(ids)
        }
        Err(TvdbMiss::Unknown(reason)) => Err(Miss::Permanent(reason)),
        Err(TvdbMiss::Unreached(reason)) => Err(Miss::Transient(reason)),
        Err(TvdbMiss::Unavailable(reason)) => Err(failed(core, key, Look::Show, show, reason)),
    }
}

/// A look for `item` worked: its failures are forgotten.
pub(super) fn succeeded(core: &Core, key: TrackerKey<'_>, look: Look, item: &str) {
    let name = look.name(item);
    if core
        .library()
        .tracker_state(key, &name)
        .ok()
        .flatten()
        .is_some()
    {
        let _ = core.library().delete_tracker_state(key, &name);
    }
}

/// A look for `item` failed: counted, and permanent once it has lasted.
pub(super) fn failed(
    core: &Core,
    key: TrackerKey<'_>,
    look: Look,
    item: &str,
    reason: String,
) -> Miss {
    let name = look.name(item);
    let name = name.as_str();
    let library = core.library();
    let now = now_ms() as i64;
    let mut stored: Value = library
        .tracker_state(key, name)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    // Failures long past (it worked meanwhile, unrecorded) start over.
    if stored["last"]
        .as_i64()
        .is_some_and(|last| now - last > 2 * FAILING_FOR_MS)
    {
        stored = Value::Null;
    }
    let first = stored["first"].as_i64().unwrap_or(now);
    let last = stored["last"].as_i64().unwrap_or(i64::MIN / 2);
    let mut looks = stored["looks"].as_u64().unwrap_or(0);
    if now - last >= LOOK_GAP_MS {
        looks += 1;
        let value = json!({"first": first, "last": now, "looks": looks}).to_string();
        let _ = library.set_tracker_state(key, name, &value);
    }
    if looks >= FAILED_LOOKS && now - first >= FAILING_FOR_MS {
        Miss::Permanent(format!("{reason}, for a day"))
    } else {
        Miss::Transient(reason)
    }
}

/// The Trakt ids of one of the library's videos, for this account.
pub(super) async fn ids(
    core: &Core,
    key: TrackerKey<'_>,
    kind: &str,
    item_id: &str,
    video_id: &str,
) -> Result<TraktIds, Miss> {
    if let Some(reason) = known(core, key, item_id) {
        return Err(Miss::Permanent(reason));
    }
    if !item_id.starts_with("tt") {
        return Err(Miss::Permanent("no IMDb id".into()));
    }
    if kind == "movie" || video_id == item_id {
        return Ok(TraktIds::Movie {
            imdb: item_id.to_owned(),
        });
    }
    if video_id.split(':').nth(1) == Some("0") {
        return Err(Miss::Permanent("a special (season 0)".into()));
    }
    let tvdb = show_ids(core, key, item_id).await?;
    // No addon may list it yet (a new episode, a season to come).
    let reason = "no addon gives this episode a TVDB id";
    let tvdb = episode_id(core, key, video_id, tvdb.get(video_id).copied(), reason)?;
    Ok(TraktIds::Episode {
        tvdb,
        show_imdb: item_id.to_owned(),
    })
}

/// One episode in its show's details: found, its failed looks are
/// forgotten; missing, the look counts as failed.
fn episode_id<T>(
    core: &Core,
    key: TrackerKey<'_>,
    item: &str,
    found: Option<T>,
    reason: &str,
) -> Result<T, Miss> {
    match found {
        Some(found) => {
            succeeded(core, key, Look::Episode, item);
            Ok(found)
        }
        None => Err(failed(core, key, Look::Episode, item, reason.to_owned())),
    }
}

/// A Trakt row (history, watched list or playback) as the library knows it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Mapped {
    pub item_id: String,
    pub video_id: String,
    pub kind: String,
    pub name: String,
}

/// An item Trakt gave that the library cannot place.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Unplaced {
    pub id: String,
    pub name: String,
    pub reason: String,
}

/// Why a pulled row has no place now.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum PlaceMiss {
    /// Listed as unmapped.
    Permanent(Unplaced),
    /// The show's details could not be read: the row is left for a later
    /// round, and nothing that would pass it (a cursor, the show's
    /// checkpoint) moves; other rows go on.
    Transient(String),
}

/// Places pulled rows, looking at each show's details once per round.
pub(super) struct Mapper<'a> {
    core: &'a Core,
    key: TrackerKey<'a>,
    shows: HashMap<String, Result<HashMap<u64, String>, Miss>>,
}

impl<'a> Mapper<'a> {
    pub(super) fn new(core: &'a Core, key: TrackerKey<'a>) -> Self {
        Self {
            core,
            key,
            shows: HashMap::new(),
        }
    }

    /// A movie row (`movie`), or an episode row (`episode` and `show`).
    pub(super) async fn place(&mut self, row: &Value) -> Result<Mapped, PlaceMiss> {
        if row["movie"].is_object() {
            let movie = &row["movie"];
            let name = movie["title"].as_str().unwrap_or_default().to_owned();
            return match movie["ids"]["imdb"].as_str() {
                Some(imdb) if imdb.starts_with("tt") => Ok(Mapped {
                    item_id: imdb.to_owned(),
                    video_id: imdb.to_owned(),
                    kind: "movie".into(),
                    name,
                }),
                _ => Err(PlaceMiss::Permanent(unplaced(movie, &name, "no IMDb id"))),
            };
        }
        let (show, episode) = (&row["show"], &row["episode"]);
        let name = show["title"].as_str().unwrap_or_default().to_owned();
        let permanent = |reason: &str| Err(PlaceMiss::Permanent(unplaced(show, &name, reason)));
        let Some(imdb) = show["ids"]["imdb"].as_str().filter(|i| i.starts_with("tt")) else {
            return permanent("no IMDb id");
        };
        if episode["season"].as_u64() == Some(0) {
            return permanent("a special (season 0)");
        }
        let Some(tvdb) = episode["ids"]["tvdb"].as_u64() else {
            return permanent("no TVDB episode id");
        };
        if !self.shows.contains_key(imdb) {
            // Paced: one show's details at a time, then from this round's
            // memory (a failure too, so its other rows skip it).
            let ids = show_ids(self.core, self.key, imdb).await;
            let by_tvdb =
                ids.map(|ids| ids.into_iter().map(|(video, tvdb)| (tvdb, video)).collect());
            self.shows.insert(imdb.to_owned(), by_tvdb);
        }
        let by_tvdb = match &self.shows[imdb] {
            Ok(by_tvdb) => by_tvdb,
            Err(Miss::Permanent(reason)) => return permanent(reason.as_str()),
            Err(Miss::Transient(reason)) => return Err(PlaceMiss::Transient(reason.clone())),
        };
        // No addon may list it yet: a look that failed, as above.
        let item = format!("{imdb}:tvdb{tvdb}");
        let found = by_tvdb.get(&tvdb).cloned();
        match episode_id(
            self.core,
            self.key,
            &item,
            found,
            "no addon lists this episode yet",
        ) {
            Ok(video_id) => Ok(Mapped {
                item_id: imdb.to_owned(),
                video_id,
                kind: "series".into(),
                name,
            }),
            Err(Miss::Permanent(reason)) => Err(PlaceMiss::Permanent(Unplaced {
                id: imdb.to_owned(),
                name,
                reason,
            })),
            Err(Miss::Transient(reason)) => Err(PlaceMiss::Transient(reason)),
        }
    }
}

fn unplaced(item: &Value, name: &str, reason: &str) -> Unplaced {
    let id = item["ids"]["imdb"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| item["ids"]["trakt"].as_u64().map(|t| format!("trakt:{t}")))
        .unwrap_or_else(|| name.to_owned());
    Unplaced {
        id,
        name: name.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Lists an item in the status as unmapped.
/// Never over a show listed for good (`wrong_show`).
pub(super) fn record(core: &Core, key: TrackerKey<'_>, id: &str, name: &str, reason: &str) {
    if known(core, key, id).is_some() {
        return;
    }
    let value = json!({"name": name, "reason": reason, "at": now_ms()}).to_string();
    if let Err(e) = core
        .library()
        .set_tracker_state(key, &format!("{UNMAPPED}{id}"), &value)
    {
        tracing::debug!("trakt: unmapped item not recorded: {e}");
    }
}

/// An item listed as unmapped for good (a show Trakt resolved to another).
pub(super) fn known(core: &Core, key: TrackerKey<'_>, id: &str) -> Option<String> {
    let value = core
        .library()
        .tracker_state(key, &format!("{UNMAPPED}{id}"))
        .ok()??;
    let value: Value = serde_json::from_str(&value).ok()?;
    value["sticky"]
        .as_bool()
        .filter(|s| *s)
        .and(value["reason"].as_str().map(str::to_owned))
}

/// Lists a show Trakt resolved to another one: it is not scrobbled again
/// for this account.
pub(super) fn wrong_show(core: &Core, key: TrackerKey<'_>, id: &str, name: &str) {
    let reason = "Trakt matched it to another title";
    let value = json!({"name": name, "reason": reason, "at": now_ms(), "sticky": true}).to_string();
    let _ = core
        .library()
        .set_tracker_state(key, &format!("{UNMAPPED}{id}"), &value);
}
