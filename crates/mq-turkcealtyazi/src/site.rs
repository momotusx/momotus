//! The site over the network, from the device that plays: one title page
//! per title, then a subtitle page and its archive for a played track
//! (plans/m13-turkcealtyazi.md 3.2 to 3.5, 4). One request at a time, every
//! answer checked to come from the site's own https origin, every problem
//! classed so the apps can say what happened, and caches so a title costs
//! one request per 6 hours.

use crate::archive::{self, ArchiveFault, MAX_ARCHIVE, Want};
use crate::form::download_form;
use crate::page::{Kind, Row, TitlePage, title_page};
use crate::rank::{Playing, Target, Track, tracks};
use mq_http::{Body, HttpClient, HttpError, Method, Priority, RequestOptions, Response, Url};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

/// The site's origin; tests point a site at a fixture of their own.
pub const ORIGIN: &str = "https://turkcealtyazi.org";
const PAGE_MAX: usize = 4 * 1024 * 1024;
const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);
const TITLE_KEPT: Duration = Duration::from_secs(6 * 3600);
const ARCHIVE_KEPT: Duration = Duration::from_secs(6 * 3600);
const BAD_KEPT: Duration = Duration::from_secs(6 * 3600);
const TITLES: usize = 64;
const ARCHIVE_BYTES: usize = 8 * 1024 * 1024;
const BAD_TRACKS: usize = 256;
/// Failed download steps kept for the `ALIKE` count (plan 4.4).
const STEPS: usize = 32;
/// The shortest wait a turned-away device keeps, whatever `Retry-After`
/// says (0 included).
const SHORTEST: Duration = Duration::from_secs(30);
/// Different files failing one download step the same way, within an hour,
/// before the downloads count as changed.
const ALIKE: usize = 3;

/// How long things may take. Tests shorten them; the app keeps these.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub request: Duration,
    pub fetch: Duration,
    pub spacing: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            request: Duration::from_secs(8),
            fetch: Duration::from_secs(20),
            spacing: Duration::from_millis(500),
        }
    }
}

/// Why the site gave nothing for now. A fetch that meets one keeps its
/// track: the file is not at fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// No answer: down, slow, or the network.
    Unreachable,
    /// Turned away: a challenge page, 429, or 403 on a title page.
    Blocked,
    /// The pages are not what they were: for this title, or for downloads.
    Changed,
}

/// The tracks of one title.
#[derive(Debug, Clone, PartialEq)]
pub enum Listing {
    Tracks(Vec<Track>),
    /// The site has nothing for this title or episode.
    Nothing,
    Problem(Problem),
}

/// What a played track's fetch gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    /// The subtitle file's raw bytes, for `to_vtt`.
    File(Vec<u8>),
    /// The site, not the file: the track stays offered.
    Problem(Problem),
    /// This file: it stays off the list for 6 hours. `downloads_changed`
    /// when files keep failing this way (the site changed its downloads).
    Fault {
        fault: Fault,
        downloads_changed: bool,
    },
}

/// What was wrong with one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Its page or download answered 403, 404 or 410.
    Gone,
    /// Its page has no usable download form, or moved elsewhere.
    NoForm,
    /// The download is not an archive.
    NotArchive,
    Archive(ArchiveFault),
    /// It failed before and is still off the list.
    KnownBad,
}

/// A download step, for telling a changed site from one bad file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    PageGone,
    PageForm,
    DownloadGone,
    DownloadNotArchive,
}

enum Title {
    Rows(Arc<Vec<Row>>),
    Nothing,
    Changed,
}

#[derive(Default)]
struct State {
    titles: HashMap<String, (Instant, Title)>,
    archives: Vec<(u32, Instant, Arc<Vec<u8>>)>,
    bad: HashMap<(u32, u32), Instant>,
    steps: Vec<(Instant, u32, Step)>,
    backoff: Option<(Instant, Problem)>,
}

pub struct Site {
    http: HttpClient,
    origin: Url,
    limits: Limits,
    /// Held for a whole request, so one goes at a time; holds when the last
    /// one ended.
    gate: tokio::sync::Mutex<Option<Instant>>,
    state: Mutex<State>,
    flights: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Site {
    pub fn new(http: HttpClient) -> Self {
        let origin = Url::parse(ORIGIN).expect("the site's origin");
        Self::with(http, origin, Limits::default())
    }

    /// A site at another origin with other limits: for tests.
    pub fn with(http: HttpClient, origin: Url, limits: Limits) -> Self {
        Self {
            http,
            origin,
            limits,
            gate: tokio::sync::Mutex::new(None),
            state: Mutex::default(),
            flights: Mutex::default(),
        }
    }

    /// The Turkish tracks for `target` of IMDb `digits` (`0903747`), ranked
    /// against `playing`; files that failed lately are left out.
    pub async fn list(&self, digits: &str, target: Target, playing: &Playing<'_>) -> Listing {
        if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Listing::Nothing;
        }
        let flight = self.flight(format!("title {digits}"));
        let _one = flight.lock().await;
        let rows = match self.cached_title(digits) {
            Some(Title::Rows(rows)) => rows,
            Some(Title::Nothing) => return Listing::Nothing,
            Some(Title::Changed) => return Listing::Problem(Problem::Changed),
            None => {
                if let Some(problem) = self.backing_off() {
                    return Listing::Problem(problem);
                }
                match self.fetch_title(digits).await {
                    Ok(rows) => rows,
                    Err(listing) => return listing,
                }
            }
        };
        // Before the cap, so a failed file never hides one that works.
        let open: Vec<Row> = rows
            .iter()
            .filter(|r| !self.is_bad(r, target))
            .cloned()
            .collect();
        let found: Vec<Track> = tracks(&open, target, playing);
        if found.is_empty() {
            // New uploads come: ask again sooner than in 6 hours.
            self.shorten(digits, 10 * MINUTE);
            return Listing::Nothing;
        }
        Listing::Tracks(found)
    }

    async fn fetch_title(&self, digits: &str) -> Result<Arc<Vec<Row>>, Listing> {
        let Some(url) = self.at(&format!("/mov/{digits}/x.html")) else {
            return Err(Listing::Nothing);
        };
        let answer = self
            .send(Method::GET, url, None, None, PAGE_MAX, None)
            .await;
        let response = match answer {
            Ok(response) => response,
            Err(Failed::Waiting(problem)) => return Err(Listing::Problem(problem)),
            Err(Failed::Http(e)) => {
                let problem = match (classify(&e), &e) {
                    (Class::Site(problem), _) => problem,
                    // A title page is never one file's fault: a 403 turns
                    // the device away; gone, too big or downgraded means
                    // the pages changed, for this title.
                    (Class::File, HttpError::Status { status: 403, .. }) => Problem::Blocked,
                    (Class::File | Class::Changed, _) => Problem::Changed,
                };
                if problem == Problem::Changed {
                    tracing::error!("turkcealtyazi: title page answered {}", e.kind());
                    self.remember(digits, Title::Changed, HOUR);
                } else {
                    self.back_off(problem, &e);
                }
                return Err(Listing::Problem(problem));
            }
        };
        if challenged(&response) {
            self.back_off_for(Problem::Blocked, 10 * MINUTE);
            return Err(Listing::Problem(Problem::Blocked));
        }
        if !self.on_site(&response.final_url) {
            tracing::error!("turkcealtyazi: the title page answered from another origin");
            self.remember(digits, Title::Changed, HOUR);
            return Err(Listing::Problem(Problem::Changed));
        }
        let html = String::from_utf8_lossy(&response.body);
        match title_page(response.final_url.path(), digits, &html) {
            Ok(TitlePage::Rows { rows, unparsed }) => {
                if unparsed > 0 {
                    tracing::warn!(
                        "turkcealtyazi: {unparsed} of {} rows unread",
                        rows.len() + unparsed
                    );
                }
                let rows = Arc::new(rows);
                self.remember(digits, Title::Rows(rows.clone()), TITLE_KEPT);
                Ok(rows)
            }
            Ok(page) => {
                if page == TitlePage::OtherTitle {
                    tracing::warn!("turkcealtyazi: another title answered");
                }
                self.remember(digits, Title::Nothing, 10 * MINUTE);
                Err(Listing::Nothing)
            }
            Err(changed) => {
                tracing::error!("turkcealtyazi: {changed}");
                self.remember(digits, Title::Changed, HOUR);
                Err(Listing::Problem(Problem::Changed))
            }
        }
    }

    /// The subtitle file of `track` for `target`, within the fetch limit;
    /// `playing` picks among an archive's files.
    pub async fn fetch(&self, track: &Row, target: Target, playing: &Playing<'_>) -> Fetched {
        let flying = AtomicBool::new(false);
        let fetched = self.fetch_file(track, target, playing, &flying);
        match tokio::time::timeout(self.limits.fetch, fetched).await {
            Ok(fetched) => fetched,
            Err(_) => {
                // Only a request of its own that the site left unanswered
                // backs the site off; waiting behind another play's
                // requests says nothing about the site.
                if flying.load(Ordering::SeqCst) {
                    self.back_off_for(Problem::Unreachable, 2 * MINUTE);
                }
                Fetched::Problem(Problem::Unreachable)
            }
        }
    }

    async fn fetch_file(
        &self,
        track: &Row,
        target: Target,
        playing: &Playing<'_>,
        flying: &AtomicBool,
    ) -> Fetched {
        if self.is_bad(track, target) {
            return fault(Fault::KnownBad, false);
        }
        // A row is public: a path that is not its own subtitle page is the
        // caller's mistake, never a sign about the site or the file.
        if self.page_of(track).is_none() {
            return fault(Fault::NoForm, false);
        }
        let want = match (target, &track.kind) {
            (Target::Episode { season, episode }, Kind::Pack { .. } | Kind::Episodes { .. }) => {
                Want::FromSeveral { season, episode }
            }
            (Target::Episode { season, episode }, _) => Want::Episode { season, episode },
            (Target::Movie, _) => Want::Single,
        };
        let flight = self.flight(format!("archive {}", track.altid));
        let _one = flight.lock().await;
        let archive = match self.cached_archive(track.altid) {
            Some(archive) => archive,
            None => match self.download(track, flying).await {
                Ok(archive) => archive,
                Err(Ok(problem)) => return Fetched::Problem(problem),
                Err(Err(step)) => {
                    let changed = self.step_failed(track.altid, step);
                    self.mark_bad(track, target);
                    let fault = match step {
                        Step::PageGone | Step::DownloadGone => Fault::Gone,
                        Step::PageForm => Fault::NoForm,
                        Step::DownloadNotArchive => Fault::NotArchive,
                    };
                    return Fetched::Fault {
                        fault,
                        downloads_changed: changed,
                    };
                }
            },
        };
        match archive::pick(&archive, want, playing) {
            Ok(file) => Fetched::File(file),
            Err(e) => {
                self.mark_bad(track, target);
                fault(Fault::Archive(e), false)
            }
        }
    }

    /// The archive of `track`: its page's form, then the download. A site
    /// problem is `Err(Ok(_))`; a fault of this file's step `Err(Err(_))`.
    async fn download(
        &self,
        track: &Row,
        flying: &AtomicBool,
    ) -> Result<Arc<Vec<u8>>, Result<Problem, Step>> {
        if let Some(problem) = self.backing_off() {
            return Err(Ok(problem));
        }
        let Some(page_url) = self.page_of(track) else {
            return Err(Err(Step::PageForm));
        };
        let page = self
            .step(
                Method::GET,
                page_url.clone(),
                None,
                None,
                PAGE_MAX,
                Step::PageGone,
                flying,
            )
            .await?;
        if page.final_url != page_url {
            return Err(Err(Step::PageForm));
        }
        let html = String::from_utf8_lossy(&page.body);
        let Some(form) = download_form(&html, track.altid) else {
            return Err(Err(Step::PageForm));
        };
        let body = Body::Form(vec![
            ("idid".into(), form.idid),
            ("altid".into(), form.altid),
            ("sidid".into(), form.sidid),
        ]);
        let ind = self.at("/ind").expect("a path on the origin");
        let answer = self
            .step(
                Method::POST,
                ind.clone(),
                Some(body),
                Some(&page_url),
                MAX_ARCHIVE,
                Step::DownloadGone,
                flying,
            )
            .await?;
        if answer.final_url != ind || !answer.body.starts_with(b"PK\x03\x04") {
            return Err(Err(Step::DownloadNotArchive));
        }
        let archive = Arc::new(answer.body.to_vec());
        self.keep_archive(track.altid, archive.clone());
        Ok(archive)
    }

    /// One request of a download, classed: the site's problems back off,
    /// a 403, 404 or 410 (no challenge) is this file's `gone` step, and an
    /// answer from elsewhere is a change of the site, not of this file.
    #[allow(clippy::too_many_arguments)]
    async fn step(
        &self,
        method: Method,
        url: Url,
        body: Option<Body>,
        referer: Option<&Url>,
        max_bytes: usize,
        gone: Step,
        flying: &AtomicBool,
    ) -> Result<Response, Result<Problem, Step>> {
        let answer = self
            .send(method, url, body, referer, max_bytes, Some(flying))
            .await;
        match answer {
            Ok(response) if challenged(&response) => {
                self.back_off_for(Problem::Blocked, 10 * MINUTE);
                Err(Ok(Problem::Blocked))
            }
            Ok(response) if !self.on_site(&response.final_url) => {
                tracing::error!("turkcealtyazi: a download answered from another origin");
                Err(Ok(Problem::Changed))
            }
            Ok(response) => Ok(response),
            Err(Failed::Waiting(problem)) => Err(Ok(problem)),
            Err(Failed::Http(e)) => match classify(&e) {
                Class::Site(problem) => {
                    self.back_off(problem, &e);
                    Err(Ok(problem))
                }
                Class::File => Err(Err(gone)),
                Class::Changed => Err(Err(Step::DownloadNotArchive)),
            },
        }
    }

    /// One request, after the one before it ended plus the spacing, unless
    /// a backoff began while it waited its turn. `flying` is true while the
    /// request is out, so a fetch cut short knows whether the site kept it
    /// waiting.
    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<Body>,
        referer: Option<&Url>,
        max_bytes: usize,
        flying: Option<&AtomicBool>,
    ) -> Result<Response, Failed> {
        let mut last = self.gate.lock().await;
        if let Some(at) = *last {
            tokio::time::sleep_until(at + self.limits.spacing).await;
        }
        if let Some(problem) = self.backing_off() {
            return Err(Failed::Waiting(problem));
        }
        let mut headers = vec![("Accept-Language".into(), "tr,en;q=0.8".into())];
        if let Some(referer) = referer {
            headers.push(("Origin".into(), self.origin.origin().ascii_serialization()));
            headers.push(("Referer".into(), referer.to_string()));
        }
        let opts = RequestOptions {
            priority: Priority::Addon,
            timeout: self.limits.request,
            max_bytes,
            no_proxy: true,
            headers,
            ..Default::default()
        };
        // Set before it goes too: a request cut short still spaces the next.
        *last = Some(Instant::now());
        let flag = |on| flying.inspect(|f| f.store(on, Ordering::SeqCst));
        flag(true);
        let answer = self.http.request(method, url.as_str(), body, &opts).await;
        flag(false);
        *last = Some(Instant::now());
        answer.map_err(Failed::Http)
    }

    /// The subtitle page of `track`: `/sub/<its altid>/<slug>.html` only.
    fn page_of(&self, track: &Row) -> Option<Url> {
        SUB_PATH
            .captures(&track.path)
            .filter(|c| c[1] == track.altid.to_string())
            .and_then(|_| self.at(&track.path))
    }

    /// `path` on the site's own origin; `None` for anything that leaves it.
    fn at(&self, path: &str) -> Option<Url> {
        self.origin.join(path).ok().filter(|url| self.on_site(url))
    }

    /// Whether a response came from the site's own origin over https.
    fn on_site(&self, url: &Url) -> bool {
        url.scheme() == "https" && url.origin() == self.origin.origin()
    }

    fn flight(&self, key: String) -> Arc<tokio::sync::Mutex<()>> {
        let mut flights = lock(&self.flights);
        flights.retain(|_, f| Arc::strong_count(f) > 1);
        flights.entry(key).or_default().clone()
    }

    fn backing_off(&self) -> Option<Problem> {
        let mut state = lock(&self.state);
        match state.backoff {
            Some((until, problem)) if Instant::now() < until => Some(problem),
            _ => {
                state.backoff = None;
                None
            }
        }
    }

    fn back_off(&self, problem: Problem, error: &HttpError) {
        let wait = match (problem, error) {
            (
                Problem::Blocked,
                HttpError::Status {
                    retry_after: Some(after),
                    ..
                },
            ) => (*after).clamp(SHORTEST, HOUR),
            (Problem::Blocked, _) => 10 * MINUTE,
            _ => 2 * MINUTE,
        };
        tracing::warn!(
            "turkcealtyazi: {:?} ({}), waiting {} s",
            problem,
            error.kind(),
            wait.as_secs()
        );
        self.back_off_for(problem, wait);
    }

    /// A backoff only grows: the later end wins, and `Blocked` stays
    /// `Blocked` until it ends, whatever comes after.
    fn back_off_for(&self, problem: Problem, wait: Duration) {
        let mut state = lock(&self.state);
        let now = Instant::now();
        let until = now + wait;
        state.backoff = Some(match state.backoff {
            Some((old, was)) if old > now => {
                let problem = if was == Problem::Blocked {
                    was
                } else {
                    problem
                };
                (old.max(until), problem)
            }
            _ => (until, problem),
        });
    }

    fn cached_title(&self, digits: &str) -> Option<Title> {
        let state = lock(&self.state);
        let (until, title) = state.titles.get(digits)?;
        (Instant::now() < *until).then(|| match title {
            Title::Rows(rows) => Title::Rows(rows.clone()),
            Title::Nothing => Title::Nothing,
            Title::Changed => Title::Changed,
        })
    }

    fn remember(&self, digits: &str, title: Title, kept: Duration) {
        let mut state = lock(&self.state);
        let now = Instant::now();
        state.titles.retain(|_, (until, _)| *until > now);
        if state.titles.len() >= TITLES {
            let oldest = state
                .titles
                .iter()
                .min_by_key(|(_, (until, _))| *until)
                .map(|(k, _)| k.clone());
            if let Some(oldest) = oldest {
                state.titles.remove(&oldest);
            }
        }
        state.titles.insert(digits.to_owned(), (now + kept, title));
    }

    fn shorten(&self, digits: &str, kept: Duration) {
        let mut state = lock(&self.state);
        let soon = Instant::now() + kept;
        if let Some((until, _)) = state.titles.get_mut(digits) {
            *until = (*until).min(soon);
        }
    }

    fn cached_archive(&self, altid: u32) -> Option<Arc<Vec<u8>>> {
        let state = lock(&self.state);
        let now = Instant::now();
        state
            .archives
            .iter()
            .find(|(id, until, _)| *id == altid && now < *until)
            .map(|(_, _, archive)| archive.clone())
    }

    fn keep_archive(&self, altid: u32, archive: Arc<Vec<u8>>) {
        let mut state = lock(&self.state);
        let now = Instant::now();
        state
            .archives
            .retain(|(id, until, _)| *id != altid && *until > now);
        while state
            .archives
            .iter()
            .map(|(_, _, a)| a.len())
            .sum::<usize>()
            + archive.len()
            > ARCHIVE_BYTES
            && !state.archives.is_empty()
        {
            state.archives.remove(0);
        }
        if archive.len() <= ARCHIVE_BYTES {
            state.archives.push((altid, now + ARCHIVE_KEPT, archive));
        }
    }

    /// A pack is bad for the episode it lacked, a single file for all.
    fn bad_key(track: &Row, target: Target) -> (u32, u32) {
        match (&track.kind, target) {
            (Kind::Pack { .. } | Kind::Episodes { .. }, Target::Episode { episode, .. }) => {
                (track.altid, episode)
            }
            _ => (track.altid, 0),
        }
    }

    fn is_bad(&self, track: &Row, target: Target) -> bool {
        let state = lock(&self.state);
        state
            .bad
            .get(&Self::bad_key(track, target))
            .is_some_and(|until| Instant::now() < *until)
    }

    fn mark_bad(&self, track: &Row, target: Target) {
        let mut state = lock(&self.state);
        let now = Instant::now();
        state.bad.retain(|_, until| *until > now);
        if state.bad.len() < BAD_TRACKS {
            state
                .bad
                .insert(Self::bad_key(track, target), now + BAD_KEPT);
        }
    }

    /// Records a failed download step; true when `ALIKE` different files
    /// failed it the same way within the hour.
    fn step_failed(&self, altid: u32, step: Step) -> bool {
        let mut state = lock(&self.state);
        let now = Instant::now();
        state
            .steps
            .retain(|(at, _, _)| now.duration_since(*at) < HOUR);
        state.steps.push((now, altid, step));
        if state.steps.len() > STEPS {
            state.steps.remove(0);
        }
        let mut files: Vec<u32> = state
            .steps
            .iter()
            .filter(|(_, _, s)| *s == step)
            .map(|(_, id, _)| *id)
            .collect();
        files.sort_unstable();
        files.dedup();
        let changed = files.len() >= ALIKE;
        if changed {
            tracing::error!(
                "turkcealtyazi: {} files failed {:?} within the hour",
                files.len(),
                step
            );
        }
        changed
    }
}

fn fault(fault: Fault, downloads_changed: bool) -> Fetched {
    Fetched::Fault {
        fault,
        downloads_changed,
    }
}

/// Why `send` gave no answer.
enum Failed {
    /// A backoff began while the request waited its turn: never sent.
    Waiting(Problem),
    Http(HttpError),
}

/// A subtitle page: `/sub/<altid>/<slug>.html`.
static SUB_PATH: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^/sub/(\d{1,9})/[a-z0-9-]{1,120}\.html$").expect("pattern")
});

enum Class {
    Site(Problem),
    /// 403, 404 or 410 without a challenge: this file.
    File,
    /// A downgrade or an oversized answer.
    Changed,
}

fn classify(error: &HttpError) -> Class {
    match error {
        HttpError::Status { status: 429, .. } => Class::Site(Problem::Blocked),
        HttpError::Status { status, body, .. }
            if matches!(status, 403 | 503) && challenge_body(body) =>
        {
            Class::Site(Problem::Blocked)
        }
        HttpError::Status {
            status: 403 | 404 | 410,
            ..
        } => Class::File,
        HttpError::Status { status, .. } if *status >= 500 => Class::Site(Problem::Unreachable),
        HttpError::Status { .. } | HttpError::TooLarge { .. } | HttpError::Decode(_) => {
            Class::Changed
        }
        // mq-http refuses a redirect to plain http with this text.
        HttpError::Network(text) if text.contains("redirect from https to http") => Class::Changed,
        _ => Class::Site(Problem::Unreachable),
    }
}

/// A challenge page instead of the answer: Cloudflare marks every one with
/// this header on a 2xx (developers.cloudflare.com, "Detect a Challenge
/// Page response"); an error status carries no headers here, so its body
/// is read instead.
fn challenged(response: &Response) -> bool {
    response.header("cf-mitigated") == Some("challenge")
}

fn challenge_body(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body);
    ["Just a moment", "challenge-platform", "cf-chl"]
        .iter()
        .any(|mark| text.contains(mark))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> Site {
        Site::new(HttpClient::new(mq_http::ClientConfig::default()).expect("a client"))
    }

    #[tokio::test(start_paused = true)]
    async fn a_backoff_only_grows_and_blocked_stays_blocked() {
        let site = site();
        site.back_off_for(Problem::Blocked, 10 * MINUTE);
        site.back_off_for(Problem::Unreachable, 2 * MINUTE);
        tokio::time::advance(3 * MINUTE).await;
        assert_eq!(site.backing_off(), Some(Problem::Blocked));
        tokio::time::advance(8 * MINUTE).await;
        assert_eq!(site.backing_off(), None);
        // A shorter wait never cuts a longer one.
        site.back_off_for(Problem::Unreachable, 2 * MINUTE);
        site.back_off_for(Problem::Unreachable, MINUTE);
        tokio::time::advance(MINUTE + MINUTE / 2).await;
        assert_eq!(site.backing_off(), Some(Problem::Unreachable));
        site.back_off_for(Problem::Blocked, 10 * MINUTE);
        assert_eq!(site.backing_off(), Some(Problem::Blocked));
    }

    #[tokio::test]
    async fn the_failed_step_list_is_bounded() {
        let site = site();
        for altid in 0..100 {
            site.step_failed(altid, Step::PageForm);
        }
        assert_eq!(lock(&site.state).steps.len(), STEPS);
    }
}
