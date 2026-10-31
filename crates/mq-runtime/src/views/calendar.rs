//! Calendar: episodes of the profile's library series released in a window
//! around the app's today (plans/m10-calendar.md). Release dates are dates,
//! not air times: the UTC calendar date of `released`, as Title shows it, so
//! no time zone moves an episode to another day.

use crate::Core;
use crate::live::{FRAME, coalesce};
use chrono::{Days, NaiveDate};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_library::Table;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::pin::Pin;
use std::time::{Duration, Instant};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

const PAST_DAYS: u64 = 7;
const FUTURE_DAYS: u64 = 30;
const CONCURRENCY: usize = 4;
// ponytail: a ceiling on series fetched; page or raise it when libraries grow.
const MAX_SERIES: usize = 150;
/// `setToday` fetches metas again when the last fetch is older than this.
const REFRESH_AFTER: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct CalendarItem {
    pub item_id: String,
    /// The meta's own video id: the item's stable key.
    pub video_id: String,
    pub name: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub title: String,
    pub date: String,
    pub poster: Option<String>,
    pub still: Option<String>,
    pub watched: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDay {
    pub date: String,
    pub items: Vec<CalendarItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSeries {
    pub item_id: String,
    pub name: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSnapshot {
    pub today: String,
    pub days: Vec<CalendarDay>,
    /// Series whose details are still loading.
    pub pending: u32,
    /// Series whose details could not be fetched (`reason` set).
    pub failed: Vec<CalendarSeries>,
    /// Series whose details have no dated episode.
    pub no_schedule: Vec<CalendarSeries>,
    /// Library series the calendar follows: 0 means none to follow, so an
    /// empty window says "no series" rather than "nothing airs".
    pub series: u32,
}

/// An episode kept from a meta: only what the calendar shows.
#[derive(Debug, Clone)]
struct Candidate {
    video_id: String,
    season: Option<u32>,
    episode: Option<u32>,
    title: String,
    date: NaiveDate,
    still: Option<String>,
}

enum Outcome {
    Pending,
    Failed(String),
    Ready(Vec<Candidate>),
}

struct Series {
    /// `series` or `anime`: where its meta is asked for.
    kind: String,
    name: String,
    poster: Option<String>,
    /// A fetch answer for an older generation is ignored.
    generation: u64,
    fetched_at: Option<Instant>,
    outcome: Outcome,
}

pub fn parse_day(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

enum Command {
    Today(NaiveDate),
    Retry(String),
}

pub struct CalendarView {
    snapshot: SharedObservable<CalendarSnapshot>,
    commands: mpsc::UnboundedSender<Command>,
    task: JoinHandle<()>,
}

impl CalendarView {
    pub fn open(core: &Core, today: NaiveDate) -> Self {
        // Subscribe before the first read so no change slips in between.
        let changes = core.library().subscribe();
        let snapshot = SharedObservable::new(CalendarSnapshot {
            today: today.to_string(),
            days: Vec::new(),
            pending: 0,
            failed: Vec::new(),
            no_schedule: Vec::new(),
            series: 0,
        });
        let (commands, rx) = mpsc::unbounded_channel();
        let mut owner = Owner {
            core: core.clone(),
            today,
            series: HashMap::new(),
            queue: VecDeque::new(),
            fetches: JoinSet::new(),
            next_generation: 0,
            snapshot: snapshot.clone(),
        };
        // The first snapshot already counts the series it is loading, so an
        // app never shows "nothing scheduled" before the first fetch.
        owner.sync_membership(false);
        owner.render();
        let task = tokio::spawn(owner.run(changes, rx));
        Self {
            snapshot,
            commands,
            task,
        }
    }

    pub fn snapshot(&self) -> CalendarSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        CalendarSnapshot,
        Pin<Box<dyn Stream<Item = Vec<CalendarSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }

    /// The app's day changed (local midnight, or back from the background).
    pub fn set_today(&self, today: NaiveDate) {
        let _ = self.commands.send(Command::Today(today));
    }

    /// Fetches one failed series again; it shows as loading meanwhile.
    pub fn retry(&self, item_id: &str) {
        let _ = self.commands.send(Command::Retry(item_id.to_owned()));
    }
}

impl Drop for CalendarView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The one task that owns the calendar's state.
struct Owner {
    core: Core,
    today: NaiveDate,
    series: HashMap<String, Series>,
    queue: VecDeque<String>,
    fetches: JoinSet<(String, u64, Outcome)>,
    next_generation: u64,
    snapshot: SharedObservable<CalendarSnapshot>,
}

impl Owner {
    async fn run(
        mut self,
        mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
        mut commands: mpsc::UnboundedReceiver<Command>,
    ) {
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None => return,
                    Some(Command::Today(today)) => {
                        self.today = today;
                        let stale = self.series.values().any(|s| {
                            s.fetched_at.is_some_and(|at| at.elapsed() > REFRESH_AFTER)
                        });
                        if stale {
                            self.sync_membership(true);
                        }
                    }
                    Some(Command::Retry(id)) => self.retry(&id),
                },
                change = changes.recv() => match change {
                    Ok(tables) => {
                        let addons = tables.iter().any(|t| matches!(t, Table::Addons));
                        if addons || tables.iter().any(|t| matches!(t, Table::LibraryItems)) {
                            self.sync_membership(addons);
                        } else if !tables.iter().any(|t| matches!(t, Table::Watched)) {
                            continue;
                        }
                    }
                    Err(RecvError::Lagged(_)) => self.sync_membership(true),
                    Err(RecvError::Closed) => return,
                },
                Some(joined) = self.fetches.join_next() => {
                    if let Ok((item_id, generation, outcome)) = joined
                        && let Some(series) = self.series.get_mut(&item_id)
                        && series.generation == generation
                    {
                        // A refresh that fails keeps the episodes it had.
                        let keep = matches!(outcome, Outcome::Failed(_))
                            && matches!(series.outcome, Outcome::Ready(_));
                        if !keep {
                            series.outcome = outcome;
                        }
                        series.fetched_at = Some(Instant::now());
                    }
                    self.start_fetches();
                }
            }
            self.render();
        }
    }

    /// Reads the library's series: new ones are fetched, removed ones go,
    /// and with `all` every series is fetched again.
    fn sync_membership(&mut self, all: bool) {
        let core = &self.core;
        let mut entries: Vec<_> = core
            .library()
            .library(core.profile())
            .unwrap_or_default()
            .into_iter()
            // Kitsu's anime have episodes and air dates too (web journey
            // review P3-20).
            .filter(|e| matches!(e.item.kind.as_str(), "series" | "anime"))
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.last_watched_ms.unwrap_or(e.item.added_ms)));
        let mut seen = HashSet::new();
        entries.retain(|e| seen.insert(e.item.item_id.clone()));
        entries.truncate(MAX_SERIES);
        let keep: HashSet<&str> = entries.iter().map(|e| e.item.item_id.as_str()).collect();
        self.series.retain(|id, _| keep.contains(id.as_str()));
        self.queue.retain(|id| keep.contains(id.as_str()));
        for entry in entries {
            let id = entry.item.item_id.clone();
            let fetch = match self.series.get_mut(&id) {
                Some(series) => {
                    series.name = entry.item.name.clone();
                    series.poster = entry.item.poster.clone();
                    all
                }
                None => true,
            };
            if fetch {
                self.next_generation += 1;
                let previous = self.series.remove(&id);
                self.series.insert(
                    id.clone(),
                    Series {
                        kind: entry.item.kind,
                        name: entry.item.name,
                        poster: entry.item.poster,
                        generation: self.next_generation,
                        fetched_at: None,
                        // A refresh keeps showing what it had meanwhile.
                        outcome: previous.map_or(Outcome::Pending, |p| p.outcome),
                    },
                );
                if !self.queue.contains(&id) {
                    self.queue.push_back(id);
                }
            }
        }
        self.start_fetches();
    }

    /// Only a failed series is fetched again: one loading or loaded has nothing to retry.
    fn retry(&mut self, id: &str) {
        let Some(series) = self.series.get_mut(id) else {
            return;
        };
        if !matches!(series.outcome, Outcome::Failed(_)) {
            return;
        }
        self.next_generation += 1;
        series.generation = self.next_generation;
        series.outcome = Outcome::Pending;
        if !self.queue.iter().any(|q| q == id) {
            self.queue.push_back(id.to_owned());
        }
        self.start_fetches();
    }

    fn start_fetches(&mut self) {
        while self.fetches.len() < CONCURRENCY {
            let Some(id) = self.queue.pop_front() else {
                return;
            };
            let Some(series) = self.series.get(&id) else {
                continue;
            };
            let (core, generation) = (self.core.clone(), series.generation);
            let kind = series.kind.clone();
            self.fetches.spawn(async move {
                let outcome = match core.fetch_meta(&kind, &id).await {
                    Ok((meta, _)) => Outcome::Ready(candidates(&meta)),
                    Err(reason) => Outcome::Failed(reason),
                };
                (id, generation, outcome)
            });
        }
    }

    fn render(&self) {
        let next = render(&self.core, self.today, &self.series);
        if next != self.snapshot.get() {
            self.snapshot.set(next);
        }
    }
}

/// Dated episodes of a meta, specials left out.
fn candidates(meta: &mq_protocol::MetaItem) -> Vec<Candidate> {
    meta.videos
        .iter()
        .filter(|v| v.series_info.as_ref().is_none_or(|s| s.season > 0))
        .filter_map(|v| {
            Some(Candidate {
                video_id: v.id.clone(),
                season: v.series_info.as_ref().map(|s| s.season),
                episode: v.series_info.as_ref().map(|s| s.episode),
                title: v.title.clone(),
                date: v.released?.date_naive(),
                still: v.thumbnail.clone(),
            })
        })
        .collect()
}

fn render(core: &Core, today: NaiveDate, series: &HashMap<String, Series>) -> CalendarSnapshot {
    let first = today
        .checked_sub_days(Days::new(PAST_DAYS))
        .unwrap_or(today);
    let last = today
        .checked_add_days(Days::new(FUTURE_DAYS))
        .unwrap_or(today);
    let mut inside: Vec<(&String, &Series, &Candidate)> = Vec::new();
    let (mut pending, mut failed, mut no_schedule) = (0, Vec::new(), Vec::new());
    for (item_id, s) in series {
        match &s.outcome {
            Outcome::Pending => pending += 1,
            Outcome::Failed(reason) => failed.push(CalendarSeries {
                item_id: item_id.clone(),
                name: s.name.clone(),
                reason: Some(reason.clone()),
            }),
            Outcome::Ready(c) if c.is_empty() => no_schedule.push(CalendarSeries {
                item_id: item_id.clone(),
                name: s.name.clone(),
                reason: None,
            }),
            Outcome::Ready(candidates) => inside.extend(
                candidates
                    .iter()
                    .filter(|c| (first..=last).contains(&c.date))
                    .map(|c| (item_id, s, c)),
            ),
        }
    }
    let ids: Vec<&str> = inside.iter().map(|(_, _, c)| c.video_id.as_str()).collect();
    let watched = core
        .library()
        .watched_among(core.profile(), &ids)
        .unwrap_or_default();
    let mut items: Vec<CalendarItem> = inside
        .into_iter()
        .map(|(item_id, s, c)| CalendarItem {
            item_id: item_id.clone(),
            video_id: c.video_id.clone(),
            name: s.name.clone(),
            season: c.season,
            episode: c.episode,
            title: c.title.clone(),
            date: c.date.to_string(),
            poster: s.poster.clone(),
            still: c.still.clone(),
            watched: watched.contains(&c.video_id),
        })
        .collect();
    items.sort_by(|a, b| {
        (&a.date, &a.name, a.season, a.episode, &a.video_id).cmp(&(
            &b.date,
            &b.name,
            b.season,
            b.episode,
            &b.video_id,
        ))
    });
    // After today a series shows its first episode of a day only: a binge
    // drop, or a placeholder date for episodes with none yet (Kitsu dates
    // Pokemon's episodes 155 to 200 one day), reads as one entry. Today and
    // the past are as they aired.
    let today_text = today.to_string();
    let mut first_of_day = HashSet::new();
    items.retain(|i| {
        i.date <= today_text || first_of_day.insert((i.date.clone(), i.item_id.clone()))
    });
    let mut days: Vec<CalendarDay> = Vec::new();
    for item in items {
        match days.last_mut() {
            Some(day) if day.date == item.date => day.items.push(item),
            _ => days.push(CalendarDay {
                date: item.date.clone(),
                items: vec![item],
            }),
        }
    }
    let by_name =
        |a: &CalendarSeries, b: &CalendarSeries| (&a.name, &a.item_id).cmp(&(&b.name, &b.item_id));
    failed.sort_by(by_name);
    no_schedule.sort_by(by_name);
    CalendarSnapshot {
        today: today.to_string(),
        days,
        pending,
        failed,
        no_schedule,
        series: u32::try_from(series.len()).unwrap_or(u32::MAX),
    }
}
