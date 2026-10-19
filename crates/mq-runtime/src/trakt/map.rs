//! Identity between the library's ids and Trakt's (plans/m7-trakt.md 6):
//! movies by IMDb id, episodes by the TVDB id the show's meta gives each
//! video. Anything else is listed as unmapped, with the reason.

use crate::Core;
use mq_library::TrackerKey;
use mq_library::hlc::now_ms;
use serde_json::{Value, json};
use std::collections::HashMap;

/// Tracker state names of unmapped items start with this.
pub(super) const UNMAPPED: &str = "unmapped:";

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
    /// show's details, or a show Trakt matched to another title. Listed as
    /// unmapped; its writes are dropped.
    Permanent(String),
    /// The show's details could not be read now: tried again later, never
    /// listed and never remembered.
    Transient(String),
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
    let tvdb = core
        .episode_tvdb_ids(item_id)
        .await
        .map_err(|_| Miss::Transient("the show's details are not available".into()))?;
    match tvdb.get(video_id) {
        Some(&tvdb) => Ok(TraktIds::Episode {
            tvdb,
            show_imdb: item_id.to_owned(),
        }),
        None => Err(Miss::Permanent("no TVDB id in the show's details".into())),
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
    /// The show's details could not be read: the round stops before any
    /// cursor or checkpoint moves.
    Transient(String),
}

/// Places pulled rows, reading each show's details once per round (only
/// details that were read are kept).
pub(super) struct Mapper<'a> {
    core: &'a Core,
    shows: HashMap<String, HashMap<u64, String>>,
}

impl<'a> Mapper<'a> {
    pub(super) fn new(core: &'a Core) -> Self {
        Self {
            core,
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
        let permanent = |reason| Err(PlaceMiss::Permanent(unplaced(show, &name, reason)));
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
            // Paced: one show's details at a time, then from the cache. A
            // failed read is not kept: the round stops, and tries again.
            let Ok(ids) = self.core.episode_tvdb_ids(imdb).await else {
                return Err(PlaceMiss::Transient(
                    "the show's details are not available".into(),
                ));
            };
            let by_tvdb = ids.into_iter().map(|(video, tvdb)| (tvdb, video)).collect();
            self.shows.insert(imdb.to_owned(), by_tvdb);
        }
        match self.shows[imdb].get(&tvdb) {
            Some(video) => Ok(Mapped {
                item_id: imdb.to_owned(),
                video_id: video.clone(),
                kind: "series".into(),
                name,
            }),
            None => Err(PlaceMiss::Permanent(Unplaced {
                id: imdb.to_owned(),
                name,
                reason: "the episode is not in the show's details".into(),
            })),
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
