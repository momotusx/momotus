//! TMDB, for the title page's score (plans/title-page-ratings-genres-similar.md):
//! one `find` by IMDb id gives TMDB's user score and whether the id is a
//! movie or a show. The read token is the owner's, baked at build time like
//! Trakt's app id; it is used for `find` only.

use crate::trakt::api::{Answer, classify};
use mq_http::{HttpClient, RequestOptions};
use serde_json::Value;
use std::time::Duration;

/// The project's TMDB read token, given at build time
/// (`MARQUEE_TMDB_READ_TOKEN`, from `.env.tmdb.local`) and never committed.
/// Unset or empty, no TMDB call is made. It ships inside every build and
/// can be read out of it (an accepted risk: read-only, revocable by TMDB).
pub const TOKEN: Option<&str> = given(option_env!("MARQUEE_TMDB_READ_TOKEN"));
pub const HOST: &str = "https://api.themoviedb.org";
const TIMEOUT: Duration = Duration::from_secs(8);

const fn given(value: Option<&'static str>) -> Option<&'static str> {
    match value {
        Some(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// Where TMDB answers and with which token; tests point it at a local fake.
#[derive(Clone)]
pub struct Tmdb {
    http: HttpClient,
    host: String,
    token: String,
    local: bool,
}

impl std::fmt::Debug for Tmdb {
    // The token never prints.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tmdb").field("host", &self.host).finish()
    }
}

impl Tmdb {
    pub fn new(http: HttpClient, host: &str, token: &str, local: bool) -> Self {
        Self {
            http,
            host: host.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            local,
        }
    }

    /// TMDB's `find` for an IMDb id (`tt` and digits only: it goes into the
    /// path).
    pub async fn find(&self, imdb: &str) -> Answer {
        let url = format!("{}/3/find/{imdb}?external_source=imdb_id", self.host);
        let opts = RequestOptions {
            timeout: TIMEOUT,
            allow_private: self.local,
            headers: vec![
                ("authorization".to_owned(), format!("Bearer {}", self.token)),
                ("accept".to_owned(), "application/json".to_owned()),
            ],
            ..Default::default()
        };
        classify(
            self.http
                .request(mq_http::Method::GET, &url, None, &opts)
                .await,
        )
    }
}

/// What `find` says of an IMDb id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Found {
    /// `movie` or `series`, as addons name the kinds.
    pub kind: &'static str,
    /// TMDB's user score, 0 to 10.
    pub score: f64,
    pub votes: u64,
}

/// The first movie, else the first show, of a `find` answer.
pub fn found(body: &Value) -> Option<Found> {
    let (kind, item) = match (body["movie_results"].get(0), body["tv_results"].get(0)) {
        (Some(movie), _) => ("movie", movie),
        (None, Some(show)) => ("series", show),
        _ => return None,
    };
    Some(Found {
        kind,
        score: item["vote_average"]
            .as_f64()
            .filter(|s| (0.0..=10.0).contains(s))?,
        votes: item["vote_count"].as_u64()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live shapes of 2026-10-24: a movie, a show, and an id TMDB does
    /// not know.
    #[test]
    fn a_find_answer_gives_kind_score_and_votes() {
        let movie = serde_json::json!({"movie_results": [{"id": 278, "vote_average": 8.733, "vote_count": 31655}], "tv_results": []});
        assert_eq!(
            found(&movie),
            Some(Found {
                kind: "movie",
                score: 8.733,
                votes: 31655
            })
        );
        let show = serde_json::json!({"movie_results": [], "tv_results": [{"id": 1396, "vote_average": 9.0, "vote_count": 18925}]});
        assert_eq!(found(&show).map(|f| f.kind), Some("series"));
        let none = serde_json::json!({"movie_results": [], "tv_results": [], "person_results": []});
        assert_eq!(found(&none), None);
        let odd = serde_json::json!({"movie_results": [{"vote_average": 12.0, "vote_count": 3}]});
        assert_eq!(found(&odd), None);
    }

    #[test]
    fn the_token_never_prints() {
        let tmdb = Tmdb::new(
            HttpClient::new(Default::default()).unwrap(),
            "http://127.0.0.1:9",
            "secret-token",
            true,
        );
        assert!(!format!("{tmdb:?}").contains("secret-token"));
    }
}
