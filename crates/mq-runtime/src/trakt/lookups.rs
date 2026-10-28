//! The title page's lookups (plans/title-page-ratings-genres-similar.md):
//! TMDB's score, Trakt's score and Trakt's related titles for an IMDb id.
//! Answers are kept 24 hours, empty ones too, so an obscure title is not
//! asked again on every open; a failure other than a 429 rests that title a
//! minute. A 429 from Trakt quiets every Trakt call, Search's included (one
//! host, one limit), for its `Retry-After` and at least a minute; TMDB's
//! quiets TMDB alone. Each map keeps 200 titles, the oldest written going
//! first. A kids profile asks nothing.

use super::{kids, lock};
use crate::Core;
use mq_tracking::tmdb::{self, Tmdb};
use mq_tracking::trakt::search::{self, Found};
use mq_tracking::trakt::{Answer, Api};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;

const KEPT_FOR: Duration = Duration::from_secs(24 * 3600);
const REST: Duration = Duration::from_secs(60);
const KEPT: usize = 200;
/// TMDB's and Trakt's numbers show only with at least this many votes: a
/// handful of votes is noise, and one reads 10.0.
pub(crate) const MIN_VOTES: u64 = 10;

/// Where TMDB answers and with which token: the baked one by default, a
/// local fake in tests. With no token no TMDB call is made.
#[derive(Clone)]
pub struct TmdbHost {
    pub url: String,
    pub token: Option<String>,
    pub local: bool,
}

impl Default for TmdbHost {
    fn default() -> Self {
        Self {
            url: tmdb::HOST.to_owned(),
            token: tmdb::TOKEN.map(str::to_owned),
            local: false,
        }
    }
}

impl std::fmt::Debug for TmdbHost {
    // The token never prints.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TmdbHost")
            .field("url", &self.url)
            .field("token", &self.token.as_ref().map(|_| "set"))
            .finish()
    }
}

enum Got<T> {
    Answer(T),
    /// A failure: the title is not asked again until `REST` has passed.
    Rest,
}

enum Held<T> {
    Answer(T),
    Resting,
    Nothing,
}

/// One map of answers by title.
struct Kept<T> {
    map: HashMap<String, (Instant, Got<T>)>,
}

impl<T> Default for Kept<T> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl<T: Clone> Kept<T> {
    fn held(&self, key: &str, now: Instant) -> Held<T> {
        match self.map.get(key) {
            Some((at, Got::Answer(v))) if now.duration_since(*at) < KEPT_FOR => {
                Held::Answer(v.clone())
            }
            Some((at, Got::Rest)) if now.duration_since(*at) < REST => Held::Resting,
            _ => Held::Nothing,
        }
    }

    fn put(&mut self, key: String, got: Got<T>, now: Instant) {
        self.map.retain(|_, (at, got)| {
            let life = match got {
                Got::Answer(_) => KEPT_FOR,
                Got::Rest => REST,
            };
            now.duration_since(*at) < life
        });
        if self.map.len() >= KEPT
            && let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
        {
            self.map.remove(&oldest);
        }
        self.map.insert(key, (now, got));
    }
}

#[derive(Default)]
pub(crate) struct Memory {
    tmdb_host: TmdbHost,
    tmdb_quiet: Option<Instant>,
    tmdb: Kept<tmdb::Finds>,
    scores: Kept<Option<(f64, u64)>>,
    related: Kept<Vec<Found>>,
    /// TMDB's watch provider logos by provider id (Discover's rail), kept a day.
    logos: Kept<HashMap<u64, String>>,
    /// TMDB's cast by `kind:id`, kept a day.
    credits: Kept<Vec<tmdb::Credit>>,
    /// The best textless backdrop's path by TMDB `kind:id` (first run's still, Home's hero), kept a day.
    stills: Kept<Option<String>>,
}

/// The cast kept per title: an addon names the top-billed few, and a
/// long-running show lists hundreds.
const CAST_KEPT: usize = 50;

/// A 429 quiets TMDB for its `Retry-After`; a token TMDB refuses (revoked)
/// is refused for every title, so TMDB quiets as long as a 429 may: a call
/// per quarter hour, not one per title opened.
fn quiet_tmdb(memory: &mut Memory, answer: &Answer, now: Instant) {
    if answer.throttled() {
        memory.tmdb_quiet = Some(now + super::search::quiet_for(retry_after(answer)));
    }
    if matches!(
        answer,
        Answer::Refused {
            status: 401 | 403,
            ..
        }
    ) {
        memory.tmdb_quiet = Some(now + super::search::QUIET_AT_MOST);
    }
}

#[derive(Clone, Copy)]
enum Read {
    Ratings,
    Related,
}

fn key(kind: &str, imdb: &str) -> String {
    format!("{kind}:{imdb}")
}

fn retry_after(answer: &Answer) -> Option<Duration> {
    match answer {
        Answer::Transient { retry_after, .. } => *retry_after,
        _ => None,
    }
}

/// What an answer leaves in memory: `None` keeps nothing (a 429, which
/// quiets instead).
fn kept<T>(
    answer: &Answer,
    read: impl FnOnce(&serde_json::Value) -> T,
    empty: T,
) -> Option<Got<T>> {
    match answer {
        Answer::Ok { body, .. } => Some(Got::Answer(read(body))),
        // TMDB or Trakt has no such title (a 404): an empty answer. Any
        // other refusal (a revoked token, a passing 401 or 403) rests the
        // title a minute, never a day.
        Answer::Refused { status: 404, .. } => Some(Got::Answer(empty)),
        failed if failed.throttled() => None,
        _ => Some(Got::Rest),
    }
}

/// A TMDB answer's original language as the core's tag, when the core
/// knows it.
fn known_language(found: tmdb::Found) -> Option<String> {
    let code = found.original_language?;
    crate::languages::is_known(&code).then(|| crate::languages::tag(&code))
}

impl Core {
    /// Points TMDB at another host and token (tests), or turns it off.
    #[doc(hidden)]
    pub fn set_tmdb_host(&self, host: TmdbHost) {
        self.set_no_tmdb(false);
        lock(&self.trakt().lookups).tmdb_host = host;
    }

    /// Whether a TMDB lookup can be made: a token is set and the profile is
    /// not a kids profile.
    pub(crate) fn tmdb_ready(&self) -> bool {
        !kids(self) && !self.no_tmdb() && lock(&self.trakt().lookups).tmdb_host.token.is_some()
    }

    /// Whether a Trakt lookup can be made, as for Search.
    pub(crate) fn trakt_lookup_ready(&self) -> bool {
        self.trakt_search_ready()
    }

    /// Whether every answer the title page would ask for is kept: then it
    /// shows them at once. `kind` is Trakt's kind, `None` for TMDB's.
    pub(crate) fn lookups_held(&self, kind: Option<&str>, imdb: &str) -> bool {
        let (tmdb_ready, trakt_ready) = (self.tmdb_ready(), self.trakt_lookup_ready());
        let trakt = self.trakt();
        let memory = lock(&trakt.lookups);
        let now = Instant::now();
        let tmdb = memory.tmdb.held(imdb, now);
        let kind = match (kind, &tmdb) {
            (Some(kind), _) => kind,
            (None, Held::Answer(finds)) => match finds.of(None) {
                Some(found) => found.kind,
                // An anime TMDB does not know asks Trakt nothing.
                None => return true,
            },
            (None, _) => return false,
        };
        let key = key(kind, imdb);
        (!tmdb_ready || matches!(tmdb, Held::Answer(_)))
            && (!trakt_ready
                || matches!(memory.scores.held(&key, now), Held::Answer(_))
                    && matches!(memory.related.held(&key, now), Held::Answer(_)))
    }

    /// TMDB's answer for an IMDb id as `kind` (`movie` or `series`; `None`
    /// takes the movie, else the show): its kind, score and votes. `None`
    /// when TMDB does not know the id as that kind, cannot be asked (no
    /// token, kids, quiet, resting) or failed.
    pub(crate) async fn tmdb_find(&self, imdb: &str, kind: Option<&str>) -> Option<tmdb::Found> {
        if kids(self) || self.no_tmdb() || !crate::segments::is_imdb(imdb) {
            return None;
        }
        let trakt = self.trakt();
        let client = {
            let memory = lock(&trakt.lookups);
            let now = Instant::now();
            match memory.tmdb.held(imdb, now) {
                Held::Answer(finds) => return finds.of(kind),
                Held::Resting => return None,
                Held::Nothing => {}
            }
            if memory.tmdb_quiet.is_some_and(|until| now < until) {
                return None;
            }
            let host = &memory.tmdb_host;
            Tmdb::new(
                trakt.http.clone(),
                &host.url,
                host.token.as_deref()?,
                host.local,
            )
        };
        let answer = client.find(imdb).await;
        let mut memory = lock(&trakt.lookups);
        let now = Instant::now();
        quiet_tmdb(&mut memory, &answer, now);
        let got = kept(&answer, tmdb::finds, tmdb::Finds::default())?;
        let found = match &got {
            Got::Answer(finds) => finds.of(kind),
            Got::Rest => None,
        };
        memory.tmdb.put(imdb.to_owned(), got, now);
        found
    }

    /// The language `imdb` was made in, as the core's tag ("en"), from
    /// TMDB's answer for it as `kind`; `None` as for `tmdb_find`, or when
    /// TMDB names a language the core does not know.
    pub(crate) async fn original_language(&self, imdb: &str, kind: &str) -> Option<String> {
        known_language(self.tmdb_find(imdb, Some(kind)).await?)
    }

    /// `original_language` from an answer already kept (the title page's),
    /// asking TMDB nothing: a page in a hidden tab asks nothing until it
    /// shows, and the Versions it opens may not ask for it. `None` while no
    /// answer is kept and one may come; `Some(None)` when none will name it.
    pub(crate) fn original_language_held(&self, imdb: &str, kind: &str) -> Option<Option<String>> {
        if kids(self) || self.no_tmdb() || !crate::segments::is_imdb(imdb) {
            return Some(None);
        }
        let held = lock(&self.trakt().lookups).tmdb.held(imdb, Instant::now());
        match held {
            Held::Answer(finds) => Some(finds.of(Some(kind)).and_then(known_language)),
            Held::Resting => Some(None),
            Held::Nothing => None,
        }
    }

    /// One TMDB answer kept a day under `key` in the map `pick` names:
    /// asked with `ask` and read with `read` (a 404 reads as `empty`) unless
    /// held. `None` with no token, on a kids profile, while TMDB is quiet or
    /// the key rests (a failure rests it a minute), or on a failure.
    async fn tmdb_kept<T: Clone, F: Future<Output = Answer>>(
        &self,
        pick: fn(&mut Memory) -> &mut Kept<T>,
        key: String,
        ask: impl FnOnce(Tmdb) -> F,
        read: impl FnOnce(&serde_json::Value) -> T,
        empty: T,
    ) -> Option<T> {
        if kids(self) || self.no_tmdb() {
            return None;
        }
        let trakt = self.trakt();
        let client = {
            let mut memory = lock(&trakt.lookups);
            let now = Instant::now();
            match pick(&mut memory).held(&key, now) {
                Held::Answer(value) => return Some(value),
                Held::Resting => return None,
                Held::Nothing => {}
            }
            if memory.tmdb_quiet.is_some_and(|until| now < until) {
                return None;
            }
            let host = &memory.tmdb_host;
            Tmdb::new(
                trakt.http.clone(),
                &host.url,
                host.token.as_deref()?,
                host.local,
            )
        };
        let answer = ask(client).await;
        let mut memory = lock(&trakt.lookups);
        let now = Instant::now();
        quiet_tmdb(&mut memory, &answer, now);
        let got = kept(&answer, read, empty)?;
        let value = match &got {
            Got::Answer(value) => Some(value.clone()),
            Got::Rest => None,
        };
        pick(&mut memory).put(key, got, now);
        value
    }

    /// TMDB's watch provider logos, as paths by provider id: asked once a
    /// day, `None` as for `tmdb_kept`. Discover's rail turns them into
    /// image links (plans/discover-v2.md 6.6, the owner, 2026-10-26).
    pub(crate) async fn tmdb_provider_logos(&self) -> Option<HashMap<u64, String>> {
        self.tmdb_kept(
            |m| &mut m.logos,
            "movie".to_owned(),
            |tmdb| async move { tmdb.watch_providers().await },
            tmdb::provider_logos,
            HashMap::new(),
        )
        .await
    }

    /// The cast of the title TMDB `found`, in billing order, the first
    /// `CAST_KEPT` only: asked once a day, `None` as for `tmdb_kept`. The
    /// title page puts its headshots on the addon's names.
    pub(crate) async fn tmdb_credits(&self, found: &tmdb::Found) -> Option<Vec<tmdb::Credit>> {
        let (kind, id) = (found.kind, found.id);
        self.tmdb_kept(
            |m| &mut m.credits,
            format!("{kind}:{id}"),
            move |tmdb| async move { tmdb.credits(kind, id).await },
            |body| {
                let mut cast = tmdb::credits(body);
                cast.truncate(CAST_KEPT);
                cast
            },
            Vec::new(),
        )
        .await
    }

    /// TMDB's best textless backdrop of an IMDb title of `kind` (`movie` or
    /// `series`), as an image URL (first run's still and Home's hero,
    /// plans/batch-2.md R2 and F7): a scene frame with nothing printed on it.
    /// `None` when TMDB has none, does not know the id as that kind, or
    /// cannot be asked (as `tmdb_find`). Kept a day by TMDB kind and id.
    pub(crate) async fn tmdb_textless_backdrop(&self, kind: &str, imdb: &str) -> Option<String> {
        let found = self.tmdb_find(imdb, Some(kind)).await?;
        let (kind, id) = (found.kind, found.id);
        let path = self
            .tmdb_kept(
                |m| &mut m.stills,
                format!("{kind}:{id}"),
                move |tmdb| async move { tmdb.images(kind, id).await },
                tmdb::textless_backdrop,
                None,
            )
            .await??;
        Some(format!("{}/{}{path}", tmdb::IMAGES, tmdb::STILL_SIZE))
    }

    /// Trakt's score (rating 0 to 10, votes) for a title of `kind` (`movie`
    /// or `series`). `None` as for `tmdb_find`.
    pub(crate) async fn trakt_score(&self, kind: &'static str, imdb: &str) -> Option<(f64, u64)> {
        // Before the memory, which is the device's: a kids profile never
        // gets an answer another profile's page kept.
        if kids(self) {
            return None;
        }
        let key = key(kind, imdb);
        match lock(&self.trakt().lookups)
            .scores
            .held(&key, Instant::now())
        {
            Held::Answer(score) => return score,
            Held::Resting => return None,
            Held::Nothing => {}
        }
        let answer = self.trakt_read(Read::Ratings, kind, imdb).await?;
        let got = kept(&answer, search::score, None)?;
        let score = match &got {
            Got::Answer(score) => *score,
            Got::Rest => None,
        };
        lock(&self.trakt().lookups)
            .scores
            .put(key, got, Instant::now());
        score
    }

    /// The titles Trakt relates to this one, with IMDb ids, in its order;
    /// empty as for `tmdb_find`.
    pub(crate) async fn trakt_related(&self, kind: &'static str, imdb: &str) -> Vec<Found> {
        // Before the memory, as `trakt_score`.
        if kids(self) {
            return Vec::new();
        }
        let key = key(kind, imdb);
        match lock(&self.trakt().lookups)
            .related
            .held(&key, Instant::now())
        {
            Held::Answer(related) => return related,
            Held::Resting => return Vec::new(),
            Held::Nothing => {}
        }
        let Some(answer) = self.trakt_read(Read::Related, kind, imdb).await else {
            return Vec::new();
        };
        let Some(got) = kept(
            &answer,
            |body| search::related_found(body, kind),
            Vec::new(),
        ) else {
            return Vec::new();
        };
        let related = match &got {
            Got::Answer(related) => related.clone(),
            Got::Rest => Vec::new(),
        };
        lock(&self.trakt().lookups)
            .related
            .put(key, got, Instant::now());
        related
    }

    /// One Trakt read for the title page, with the profile's own token when
    /// it is signed in here and the app's id otherwise, as Search does.
    /// `None` when Trakt cannot be asked (no app) or is quiet. A 429
    /// quiets every Trakt call.
    async fn trakt_read(&self, read: Read, kind: &str, imdb: &str) -> Option<Answer> {
        // Kids are refused by both callers, before the memory.
        if !crate::segments::is_imdb(imdb) {
            return None;
        }
        let trakt = self.trakt();
        let (api, _) = trakt.api()?;
        if lock(&trakt.search).quiet(Instant::now()) {
            return None;
        }
        let token = self.trakt_conn().auth.held_token().await;
        let mut answer = send(&api, read, kind, imdb, token.as_deref()).await;
        // A token Trakt no longer takes: the app's own limit then.
        if token.is_some()
            && matches!(
                answer,
                Answer::Refused {
                    status: 401 | 403,
                    ..
                }
            )
        {
            answer = send(&api, read, kind, imdb, None).await;
        }
        if answer.throttled() {
            lock(&trakt.search).throttled(Instant::now(), retry_after(&answer));
        }
        Some(answer)
    }
}

async fn send(api: &Api, read: Read, kind: &str, imdb: &str, token: Option<&str>) -> Answer {
    match read {
        Read::Ratings => search::ratings(api, kind, imdb, token).await,
        Read::Related => search::related(api, kind, imdb, token).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway core (tests, e2e) asks TMDB nothing until a test points
    /// it at its fake, whatever token the build baked in.
    #[test]
    fn a_throwaway_core_asks_tmdb_nothing_until_pointed_at_a_fake() {
        let core = Core::temporary("p").unwrap();
        assert!(core.no_tmdb());
        assert!(!core.tmdb_ready());
        core.set_tmdb_host(TmdbHost {
            url: "http://127.0.0.1:9".into(),
            token: Some("test".into()),
            local: true,
        });
        assert!(core.tmdb_ready());
    }

    /// The memory is the device's: an answer the owner's page kept is never
    /// a kids profile's, which is refused before the memory is read.
    #[tokio::test]
    async fn a_kids_profile_never_reads_the_kept_answers() {
        let owner = Core::temporary(mq_library::OWNER).unwrap();
        owner
            .set_pin(mq_library::OWNER, Some("1357"), None)
            .await
            .unwrap();
        let kid = owner.for_profile(&owner.create_kids_profile("Kid", "fox").unwrap());
        {
            let trakt = owner.trakt();
            let mut memory = lock(&trakt.lookups);
            let now = Instant::now();
            let key = key("movie", "tt0111161");
            memory
                .scores
                .put(key.clone(), Got::Answer(Some((9.0, 500))), now);
            memory.related.put(
                key,
                Got::Answer(vec![Found {
                    kind: "movie",
                    imdb_id: "tt0061512".into(),
                    title: "Cool Hand Luke".into(),
                }]),
                now,
            );
        }
        assert_eq!(
            owner.trakt_score("movie", "tt0111161").await,
            Some((9.0, 500))
        );
        assert_eq!(kid.trakt_score("movie", "tt0111161").await, None);
        assert!(kid.trakt_related("movie", "tt0111161").await.is_empty());
        // TMDB's kept answer too, with a token set.
        owner.set_tmdb_host(TmdbHost {
            url: "http://127.0.0.1:9".into(),
            token: Some("test".into()),
            local: true,
        });
        let found = tmdb::Found {
            kind: "movie",
            id: 278,
            score: 8.7,
            votes: 500,
            original_language: None,
        };
        lock(&owner.trakt().lookups).tmdb.put(
            "tt0111161".into(),
            Got::Answer(tmdb::Finds {
                movie: Some(found.clone()),
                show: None,
            }),
            Instant::now(),
        );
        assert_eq!(
            owner.tmdb_find("tt0111161", Some("movie")).await,
            Some(found)
        );
        assert_eq!(kid.tmdb_find("tt0111161", Some("movie")).await, None);
    }

    /// Only a 404 is kept as "none" for a day; another refusal rests a
    /// minute; a 429 keeps nothing (it quiets), and quiets 1 to 15 minutes.
    #[test]
    fn only_a_404_is_an_answer_and_quiet_is_bounded() {
        let refused = |status| Answer::Refused {
            status,
            error: None,
        };
        assert!(matches!(
            kept(&refused(404), |_| 1, 0),
            Some(Got::Answer(0))
        ));
        assert!(matches!(kept(&refused(401), |_| 1, 0), Some(Got::Rest)));
        assert!(matches!(kept(&refused(403), |_| 1, 0), Some(Got::Rest)));
        let day = Some(Duration::from_secs(86_400));
        assert_eq!(
            crate::trakt::search::quiet_for(day),
            Duration::from_secs(900)
        );
        let minute = Duration::from_secs(60);
        assert_eq!(
            crate::trakt::search::quiet_for(Some(Duration::from_secs(5))),
            minute
        );
        assert_eq!(crate::trakt::search::quiet_for(None), minute);
    }

    /// An answer is kept 24 hours, a rest a minute; at 200 titles the
    /// oldest written goes, not all of them.
    #[test]
    fn answers_keep_a_day_rests_a_minute_and_the_oldest_goes() {
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        let mut kept: Kept<u32> = Kept::default();
        kept.put("a".into(), Got::Answer(1), start);
        kept.put("b".into(), Got::Rest, start);
        assert!(matches!(kept.held("a", at(24 * 3600 - 1)), Held::Answer(1)));
        assert!(matches!(kept.held("a", at(24 * 3600)), Held::Nothing));
        assert!(matches!(kept.held("b", at(59)), Held::Resting));
        assert!(matches!(kept.held("b", at(60)), Held::Nothing));
        let mut full: Kept<u32> = Kept::default();
        for i in 0..KEPT {
            full.put(format!("t{i}"), Got::Answer(i as u32), at(i as u64));
        }
        full.put("new".into(), Got::Answer(0), at(KEPT as u64));
        assert_eq!(full.map.len(), KEPT);
        assert!(matches!(full.held("t0", at(KEPT as u64)), Held::Nothing));
        assert!(matches!(full.held("t1", at(KEPT as u64)), Held::Answer(1)));
    }
}
