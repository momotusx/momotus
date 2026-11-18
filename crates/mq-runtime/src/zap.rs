//! Zap (plans/zap.md 1.1): a random film or episode and a point in its
//! middle to start at, from where the profile's dial points: Same (the
//! library and everything played), Near (Trakt's related titles of a Same
//! one) or Far (a random genre of a random installed catalog).

use crate::Core;
use crate::views::discover::{Catalog, browsable, fetch_page};
use crate::views::versions::TitleFacts;
use mq_protocol::ResourceResponse;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Zapped ids remembered per profile, so a zap does not repeat soon.
const RECENT: usize = 50;
/// Candidates one zap tries before it gives up.
const CANDIDATES: usize = 6;
/// History rows Same draws from.
const HISTORY: usize = 200;
/// The start's bounds, as parts of the runtime: clear of the opening, and
/// 5 minutes of play (the hold, playback.rs) end before Trakt's 80 percent.
const EARLIEST: f64 = 0.15;
const LATEST: f64 = 0.75;
const HOLD_ENDS_BY: f64 = 0.80;
pub(crate) const HOLD_MS: u64 = 5 * 60 * 1000;
// ponytail: a guessed runtime when the meta has none; a short file that ends
// before the guessed point starts from 0 (playback.rs, `zap_too_late`).
const FILM_GUESS_MS: u64 = 90 * 60 * 1000;
const EPISODE_GUESS_MS: u64 = 22 * 60 * 1000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum Dial {
    Same,
    #[default]
    Near,
    Far,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ZapPick {
    pub kind: String,
    pub item_id: String,
    pub video_id: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub name: String,
    pub poster: Option<String>,
    pub episode_name: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start_ms: u64,
    /// The pool that answered: Near falls to Far with no Trakt answer.
    pub from: Dial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZapMiss {
    /// No installed catalog to draw from, and nothing else answered.
    NoAddons,
    /// Every candidate tried had nothing to play.
    NothingCameUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    kind: String,
    id: String,
}

impl Core {
    /// A zap for this profile, of `kind` when given (Discover's), else a
    /// film or a series drawn for each candidate.
    pub async fn zap(&self, kind: Option<&str>) -> Result<ZapPick, ZapMiss> {
        let mut dial = self.prefs().zap_dial;
        let same = self.same_pool();
        let catalogs = browsable(
            &self.installed_addons().unwrap_or_default(),
            &self.prefs().home_rows,
        );
        let mut dealt = false;
        for _ in 0..CANDIDATES {
            let kind = kind.map_or_else(|| ["movie", "series"][roll(2)].to_owned(), str::to_owned);
            let Some((candidate, from)) = self.candidate(dial, &kind, &same, &catalogs).await
            else {
                continue;
            };
            dealt = true;
            self.remember_zap(&candidate.id);
            if let Some(pick) = self.ready(candidate, from).await {
                return Ok(pick);
            }
            // Nothing to play there (an addon gone, a title too short):
            // this zap moves on to the next pool, so one dead title in a
            // small library never uses up every try.
            dial = match from {
                Dial::Same => Dial::Near,
                Dial::Near | Dial::Far => Dial::Far,
            };
        }
        Err(match catalogs.is_empty() && !dealt {
            true => ZapMiss::NoAddons,
            false => ZapMiss::NothingCameUp,
        })
    }

    /// The library and the history, each title once, a history row of an
    /// episode filed as a series whatever its stored kind (`LAST_PLAYED`
    /// falls back to `movie` with no library row).
    fn same_pool(&self) -> Vec<Candidate> {
        let profile = self.profile();
        let library = self.library().library(profile).unwrap_or_default();
        let played = self
            .library()
            .last_played(profile, HISTORY)
            .unwrap_or_default();
        let mut seen = HashSet::new();
        let policy = self.policy();
        library
            .into_iter()
            .map(|e| (e.item.kind, e.item.item_id))
            .chain(played.into_iter().map(|p| {
                let kind = match p.video_id != p.item_id {
                    true => "series".to_owned(),
                    false => p.kind,
                };
                (kind, p.item_id)
            }))
            .filter(|(kind, id)| policy.allows(kind, id))
            .filter(|(_, id)| seen.insert(id.clone()))
            .map(|(kind, id)| Candidate { kind, id })
            .collect()
    }

    /// One candidate from the dial's pool, falling to the next pool when it
    /// has none: Same to Near to Far. A kind the library does not store
    /// (anime) goes to Far at once.
    async fn candidate(
        &self,
        dial: Dial,
        kind: &str,
        same: &[Candidate],
        catalogs: &[Catalog],
    ) -> Option<(Candidate, Dial)> {
        let stored = matches!(kind, "movie" | "series");
        let mine: Vec<&Candidate> = same.iter().filter(|c| c.kind == kind).collect();
        if dial == Dial::Same
            && stored
            && let Some(c) = self.fresh(mine.iter().copied().cloned().collect())
        {
            return Some((c, Dial::Same));
        }
        if dial != Dial::Far
            && stored
            && let Some(c) = self.near(kind, &mine, same).await
        {
            return Some((c, Dial::Near));
        }
        self.far(kind, same, catalogs).await.map(|c| (c, Dial::Far))
    }

    /// Trakt's related titles of a random Same title of `kind`, minus Same.
    async fn near(&self, kind: &str, mine: &[&Candidate], same: &[Candidate]) -> Option<Candidate> {
        let seeds: Vec<&&Candidate> = mine
            .iter()
            .filter(|c| crate::segments::is_imdb(&c.id))
            .collect();
        let seed = seeds.get(roll(seeds.len()))?;
        let kind: &'static str = if kind == "movie" { "movie" } else { "series" };
        let policy = self.policy();
        let related = self
            .trakt_related(kind, &seed.id)
            .await
            .into_iter()
            .filter(|f| f.kind == kind && crate::segments::is_imdb(&f.imdb_id))
            .filter(|f| policy.allows(f.kind, &f.imdb_id))
            .filter(|f| !same.iter().any(|s| s.id == f.imdb_id))
            .map(|f| Candidate {
                kind: f.kind.to_owned(),
                id: f.imdb_id,
            })
            .collect();
        self.fresh(related)
    }

    /// A random item of a random genre of a random catalog of `kind`, minus
    /// Same. Each item keeps its own type.
    async fn far(&self, kind: &str, same: &[Candidate], catalogs: &[Catalog]) -> Option<Candidate> {
        let ours: Vec<&Catalog> = catalogs
            .iter()
            .filter(|c| c.shelf == kind)
            .filter(|c| !(c.genre_required && c.genres.is_empty()))
            .collect();
        let catalog = ours.get(roll(ours.len()))?;
        let genre = catalog
            .genres
            .get(roll(catalog.genres.len()))
            .map(String::as_str);
        let Ok(ResourceResponse::Metas { metas }) = fetch_page(self, catalog, genre, 0).await
        else {
            return None;
        };
        let policy = self.policy();
        let items = metas
            .into_iter()
            // A film list's item of another type is not a film; anime lists keep any.
            .filter(|m| !matches!(kind, "movie" | "series") || m.r#type == kind)
            .filter(|m| policy.allows(&m.r#type, &m.id))
            .filter(|m| !same.iter().any(|s| s.id == m.id))
            .map(|m| Candidate {
                kind: m.r#type,
                id: m.id,
            })
            .collect();
        self.fresh(items)
    }

    /// A random one of `pool` not zapped lately; when all were, any.
    fn fresh(&self, pool: Vec<Candidate>) -> Option<Candidate> {
        let recent = self.zapped_lately();
        let new: Vec<&Candidate> = pool.iter().filter(|c| !recent.contains(&c.id)).collect();
        match new.is_empty() {
            false => new.get(roll(new.len())).map(|c| (*c).clone()),
            true => pool.get(roll(pool.len())).cloned(),
        }
    }

    /// The candidate's video and start: an episode of season 1 or later out
    /// by now, or the title itself when it has no videos. `None` when its
    /// meta does not load, or it has videos but none to play.
    async fn ready(&self, c: Candidate, from: Dial) -> Option<ZapPick> {
        let (meta, _) = self.fetch_meta(&c.kind, &c.id).await.ok()?;
        let now = mq_library::hlc::now_ms() as i64;
        let episodes: Vec<_> = meta
            .videos
            .iter()
            .filter(|v| v.series_info.as_ref().is_some_and(|i| i.season >= 1))
            .filter(|v| crate::views::playback::aired(v, now))
            .collect();
        let video = match (c.kind.as_str(), meta.videos.is_empty()) {
            ("movie", _) | (_, true) => None,
            _ => Some(*episodes.get(roll(episodes.len()))?),
        };
        let runtime_ms = TitleFacts::read(meta.preview.runtime.as_deref(), None, None, None)
            .runtime_s
            .map(|s| u64::from(s) * 1000);
        // Too short for the hold to end before Trakt's 80 percent: not zapped.
        if runtime_ms.is_some_and(|r| r > 0 && (r as f64) < SHORTEST_MS) {
            return None;
        }
        let guess = if video.is_some() {
            EPISODE_GUESS_MS
        } else {
            FILM_GUESS_MS
        };
        let info = video.and_then(|v| v.series_info.as_ref());
        Some(ZapPick {
            video_id: video.map_or_else(|| c.id.clone(), |v| v.id.clone()),
            season: info.map(|i| i.season),
            episode: info.map(|i| i.episode),
            episode_name: video.map(|v| v.title.clone()).filter(|t| !t.is_empty()),
            name: meta.preview.name.clone(),
            poster: meta.preview.poster.as_ref().map(ToString::to_string),
            start_ms: start_within(runtime_ms.filter(|&r| r > 0).unwrap_or(guess), roll_u64()),
            kind: c.kind,
            item_id: c.id,
            from,
        })
    }

    fn zapped_lately(&self) -> HashSet<String> {
        lock(self.zapped())
            .get(self.profile())
            .map(|ids| ids.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn remember_zap(&self, id: &str) {
        let mut zapped = lock(self.zapped());
        let ids = zapped.entry(self.profile().to_owned()).or_default();
        ids.push_back(id.to_owned());
        if ids.len() > RECENT {
            ids.pop_front();
        }
    }
}

/// A start in the middle of a `runtime_ms` file: from 15 percent to 75, and
/// never so late that the 5 minute hold ends past 80 percent.
pub(crate) fn start_within(runtime_ms: u64, random: u64) -> u64 {
    let r = runtime_ms as f64;
    let lo = (r * EARLIEST) as u64;
    let hi = (r * LATEST)
        .min(r * HOLD_ENDS_BY - HOLD_MS as f64)
        .max(lo as f64) as u64;
    lo + random % (hi - lo + 1)
}

/// A start too late for a file of `duration_ms` (the meta's runtime, or the
/// guess, was longer than the file): past three quarters, or so late that
/// the hold would end past 80 percent. The file then starts from 0.
pub(crate) fn too_late(zap_at: u64, duration_ms: u64) -> bool {
    let (at, d) = (zap_at as f64, duration_ms as f64);
    duration_ms > 0 && (at > d * LATEST || at + HOLD_MS as f64 > d * HOLD_ENDS_BY)
}

/// The shortest file a zap starts in: one where 5 minutes of play from 15
/// percent still end by 80 percent (about 7.7 minutes).
const SHORTEST_MS: f64 = HOLD_MS as f64 / (HOLD_ENDS_BY - EARLIEST);

/// A random index below `n` (0 when `n` is 0: callers `get` with it).
fn roll(n: usize) -> usize {
    match n {
        0 => 0,
        n => (roll_u64() % n as u64) as usize,
    }
}

fn roll_u64() -> u64 {
    // ponytail: the OS source per draw, a few per zap; a seeded generator if zaps ever draw in bulk.
    getrandom::u64().unwrap_or(0)
}

/// The ids zapped lately, per profile (`Core::zapped`); in memory only.
pub(crate) type Zapped = std::sync::Mutex<HashMap<String, VecDeque<String>>>;

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_inside_the_bounds() {
        for runtime in [22 * 60_000, 45 * 60_000, 142 * 60_000, 8 * 60_000] {
            let lo = (runtime as f64 * EARLIEST) as u64;
            for random in [0, 1, 7, u64::MAX / 3, u64::MAX] {
                let at = start_within(runtime, random);
                assert!(at >= lo, "{at} before {lo} in {runtime}");
                assert!(
                    at as f64 <= runtime as f64 * LATEST,
                    "{at} past 75% of {runtime}"
                );
                // The hold ends by 80 percent unless the file is too short to allow it.
                let hold_room = runtime as f64 * HOLD_ENDS_BY - HOLD_MS as f64;
                if hold_room >= lo as f64 {
                    assert!(
                        at as f64 <= hold_room,
                        "{at} leaves the hold past 80% of {runtime}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_22_minute_episode_tops_out_at_80_percent_less_the_hold() {
        let runtime: u64 = 22 * 60_000;
        let (lo, hi) = (198_000, 756_000); // 15 percent; 80 percent less 5 minutes
        assert_eq!(start_within(runtime, 0), lo);
        assert_eq!(start_within(runtime, hi - lo), hi);
        assert_eq!(
            start_within(runtime, hi - lo + 1),
            lo,
            "wraps, never past hi"
        );
    }

    #[test]
    fn too_late_is_past_three_quarters_or_a_hold_past_80_percent() {
        let min = 60_000;
        assert!(too_late(10 * min, 11 * min), "past three quarters");
        // A 60 minute meta's point in a 44 minute file: the hold would end at 84 percent.
        assert!(too_late(32 * min, 44 * min));
        assert!(!too_late(20 * min, 44 * min));
        assert!(!too_late(5 * min, 0), "no length yet");
    }
}
