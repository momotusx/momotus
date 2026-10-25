//! The ids behind search's "Top results" (plans/search-quality.md): Trakt's
//! title search, kept 10 minutes per folded query, quiet for at least a
//! minute after Trakt asks to slow down. Kids profiles never call it.

use super::{kids, lock};
use crate::Core;
use mq_tracking::trakt::Answer;
use mq_tracking::trakt::search::{self, Found};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;

/// How long an answer is used without asking again.
const FRESH: Duration = Duration::from_secs(10 * 60);
/// The least quiet time after a 429.
const QUIET: Duration = Duration::from_secs(60);
/// The longest a 429 quiets: a `Retry-After` of a day must not silence
/// Search and the title page for a day.
const QUIET_AT_MOST: Duration = Duration::from_secs(15 * 60);

/// How long a 429 quiets a host: its `Retry-After`, between a minute and
/// a quarter of an hour. Trakt's and TMDB's quiet times both use it.
pub(super) fn quiet_for(retry_after: Option<Duration>) -> Duration {
    retry_after.unwrap_or(QUIET).clamp(QUIET, QUIET_AT_MOST)
}
/// ponytail: a flat map, emptied of old answers (or cleared) at this size;
/// an LRU if searches ever outgrow it.
const KEPT: usize = 200;

/// The device's Trakt search answers, by folded query, and its quiet time.
#[derive(Default)]
pub(crate) struct Memory {
    answers: HashMap<String, (Instant, Vec<Found>)>,
    quiet_until: Option<Instant>,
}

/// What memory holds for a query.
#[derive(Debug, PartialEq)]
enum Held {
    Fresh(Vec<Found>),
    Stale(Vec<Found>),
    Nothing,
}

impl Held {
    fn hits(self) -> Option<Vec<Found>> {
        match self {
            Held::Fresh(hits) | Held::Stale(hits) => Some(hits),
            Held::Nothing => None,
        }
    }
}

impl Memory {
    fn held(&self, key: &str, now: Instant) -> Held {
        match self.answers.get(key) {
            Some((at, hits)) if now.duration_since(*at) < FRESH => Held::Fresh(hits.clone()),
            Some((_, hits)) => Held::Stale(hits.clone()),
            None => Held::Nothing,
        }
    }

    /// Trakt's quiet time after a 429: one host and one limit, shared by
    /// Search and the title page's lookups (`lookups.rs`).
    pub(super) fn quiet(&self, now: Instant) -> bool {
        self.quiet_until.is_some_and(|until| now < until)
    }

    fn keep(&mut self, key: &str, hits: Vec<Found>, now: Instant) {
        if self.answers.len() >= KEPT {
            self.answers
                .retain(|_, (at, _)| now.duration_since(*at) < FRESH);
            if self.answers.len() >= KEPT {
                self.answers.clear();
            }
        }
        self.answers.insert(key.to_owned(), (now, hits));
    }

    /// A 429: no call for its `Retry-After`, never under a minute.
    pub(super) fn throttled(&mut self, now: Instant, retry_after: Option<Duration>) {
        self.quiet_until = Some(now + quiet_for(retry_after));
    }
}

impl Core {
    /// Whether search may ask Trakt: an app id is set and the profile is
    /// not a kids profile.
    pub(crate) fn trakt_search_ready(&self) -> bool {
        !kids(self) && self.trakt().config().is_some()
    }

    /// The fresh answer memory holds for `key` (a folded query), with no call.
    pub(crate) fn trakt_titles_held(&self, key: &str) -> Option<Vec<Found>> {
        if kids(self) {
            return None;
        }
        match lock(&self.trakt().search).held(key, Instant::now()) {
            Held::Fresh(hits) => Some(hits),
            _ => None,
        }
    }

    /// Movies and shows Trakt names for `query` (`key` is its folded form):
    /// a fresh answer from memory, else a call, with the profile's own token
    /// when it is signed in on this device and the app's id otherwise. A
    /// failed call, or one Trakt's quiet time holds back, gives the answer
    /// memory still has, however old.
    pub(crate) async fn trakt_titles(&self, query: &str, key: &str) -> Option<Vec<Found>> {
        if kids(self) {
            return None;
        }
        let trakt = self.trakt();
        let (api, _) = trakt.api()?;
        {
            let memory = lock(&trakt.search);
            let now = Instant::now();
            match memory.held(key, now) {
                Held::Fresh(hits) => return Some(hits),
                held if memory.quiet(now) => return held.hits(),
                _ => {}
            }
        }
        let token = self.trakt_conn().auth.held_token().await;
        let mut answer = search::titles(&api, query, token.as_deref()).await;
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
            answer = search::titles(&api, query, None).await;
        }
        let mut memory = lock(&trakt.search);
        let now = Instant::now();
        match answer {
            Answer::Ok { body, .. } => {
                let hits = search::found(&body);
                memory.keep(key, hits.clone(), now);
                Some(hits)
            }
            failed => {
                if failed.throttled() {
                    let retry_after = match &failed {
                        Answer::Transient { retry_after, .. } => *retry_after,
                        _ => None,
                    };
                    memory.throttled(now, retry_after);
                }
                memory.held(key, now).hits()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TraktConfig, TraktHosts};
    use mq_tracking::trakt::TraktRecord;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn found(id: &str) -> Vec<Found> {
        vec![Found {
            kind: "movie",
            imdb_id: id.into(),
            title: id.into(),
        }]
    }

    /// Ten minutes fresh, then stale but still there; a 429 holds calls back
    /// for its Retry-After, never under a minute.
    #[test]
    fn answers_keep_ten_minutes_and_a_429_quiets_a_minute() {
        let start = Instant::now();
        let mut memory = Memory::default();
        memory.keep("sintel", found("tt1"), start);
        let at = |secs| start + Duration::from_secs(secs);
        assert_eq!(memory.held("sintel", at(599)), Held::Fresh(found("tt1")));
        assert_eq!(memory.held("sintel", at(600)), Held::Stale(found("tt1")));
        assert_eq!(memory.held("dune", at(1)), Held::Nothing);
        memory.throttled(at(10), None);
        assert!(memory.quiet(at(69)) && !memory.quiet(at(70)));
        memory.throttled(at(100), Some(Duration::from_secs(5)));
        assert!(memory.quiet(at(159)), "never under a minute");
        memory.throttled(at(200), Some(Duration::from_secs(120)));
        assert!(memory.quiet(at(319)) && !memory.quiet(at(320)));
    }

    /// One fake Trakt host: answers `status` with `body`, records each
    /// request head.
    async fn trakt(status: &'static str, body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let heads = Arc::new(Mutex::new(Vec::new()));
        let seen = heads.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).to_lowercase());
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nRetry-After: 1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        (base, heads)
    }

    fn configured(base: &str) -> Core {
        let core = Core::temporary("p").unwrap();
        core.set_trakt_config(Some(TraktConfig {
            client_id: "client".into(),
            redirect_uri: "https://marquee.example/trakt".into(),
            hosts: TraktHosts {
                api: base.into(),
                auth: base.into(),
                local: true,
            },
        }));
        core
    }

    const SINTEL: &str =
        r#"[{"type":"movie","movie":{"title":"Sintel","ids":{"imdb":"tt1727587"}}}]"#;

    /// Signed in on this device, the call carries the profile's own token;
    /// otherwise the app id alone. A second ask within ten minutes is not sent.
    #[tokio::test]
    async fn a_signed_in_profile_searches_under_its_own_token() {
        let (base, heads) = trakt("200 OK", SINTEL).await;
        let core = configured(&base);
        let hits = core.trakt_titles("Sintel", "sintel").await.unwrap();
        assert_eq!(hits[0].imdb_id, "tt1727587");
        core.trakt_conn()
            .auth
            .set(Some(TraktRecord {
                access_token: "own-token".into(),
                refresh_token: "r".into(),
                created_at_ms: 1,
                expires_at_ms: u64::MAX,
                scope: "public".into(),
                account: "acct".into(),
                user: "me".into(),
                revision: 1,
                generation: 1,
            }))
            .await;
        assert_eq!(core.trakt_titles("sintel!", "sintel").await.unwrap(), hits);
        assert_eq!(heads.lock().unwrap().len(), 1, "kept ten minutes");
        core.trakt_titles("dune", "dune").await.unwrap();
        let heads = heads.lock().unwrap().clone();
        assert!(heads[0].contains("trakt-api-key: client"));
        assert!(heads[0].contains("/search/movie,show?query=sintel&limit=10"));
        assert!(!heads[0].contains("authorization"), "{}", heads[0]);
        assert!(
            heads[1].contains("authorization: bearer own-token"),
            "{}",
            heads[1]
        );
    }

    /// A 429 keeps the older answer and sends nothing more for a minute.
    #[tokio::test]
    async fn a_429_keeps_the_older_answer_and_waits() {
        let (base, heads) = trakt("429 Too Many Requests", "{}").await;
        let core = configured(&base);
        let stale = Instant::now() - FRESH - Duration::from_secs(1);
        lock(&core.trakt().search).keep("sintel", found("tt1727587"), stale);
        assert_eq!(
            core.trakt_titles("sintel", "sintel").await,
            Some(found("tt1727587"))
        );
        assert_eq!(core.trakt_titles("dune", "dune").await, None);
        assert_eq!(heads.lock().unwrap().len(), 1, "quiet after a 429");
    }

    /// A kids profile never asks Trakt, held answer or not.
    #[tokio::test]
    async fn a_kids_profile_never_asks() {
        let (base, heads) = trakt("200 OK", SINTEL).await;
        let core = configured(&base);
        let access = mq_library::SyncRecord {
            coll: mq_library::coll::PROFILE_ACCESS,
            key: "p".into(),
            value: serde_json::json!({"profile": "p", "kids": true}),
            hlc: core.hlc(),
        };
        core.library().apply(&[access]).unwrap();
        assert!(!core.trakt_search_ready());
        assert_eq!(core.trakt_titles("sintel", "sintel").await, None);
        assert!(heads.lock().unwrap().is_empty());
    }
}
