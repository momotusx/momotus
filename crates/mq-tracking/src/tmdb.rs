//! TMDB, for the title page's score and cast photos
//! (plans/title-page-ratings-genres-similar.md): one `find` by IMDb id
//! gives TMDB's user score and whether the id is a movie or a show, and
//! TMDB's id for its credits. The read token is the owner's, baked at build
//! time like Trakt's app id; it only reads.

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
        self.get(&format!("/3/find/{imdb}?external_source=imdb_id"))
            .await
    }

    /// The cast of TMDB's title `id` of `kind` (`movie` or `series`): a
    /// movie's credits, a show's credits across its seasons.
    pub async fn credits(&self, kind: &str, id: u64) -> Answer {
        let path = if kind == "movie" {
            format!("/3/movie/{id}/credits")
        } else {
            format!("/3/tv/{id}/aggregate_credits")
        };
        self.get(&path).await
    }

    /// TMDB's list of watch providers, with their logos (Discover's service
    /// rail, plans/discover-v2.md 6.6).
    pub async fn watch_providers(&self) -> Answer {
        self.get("/3/watch/providers/movie").await
    }

    /// The images of TMDB's title `id` of `kind` (`movie` or `series`) that
    /// carry no language (first run's still and Home's hero,
    /// plans/batch-2.md R2 and F7): textless backdrops, frames with no title
    /// printed on them.
    pub async fn images(&self, kind: &str, id: u64) -> Answer {
        let kind = if kind == "movie" { "movie" } else { "tv" };
        self.get(&format!(
            "/3/{kind}/{id}/images?include_image_language=null"
        ))
        .await
    }

    async fn get(&self, path: &str) -> Answer {
        let url = format!("{}{path}", self.host);
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

/// Where TMDB serves a logo of `size` ("w154") at `path` ("/abc.png").
pub const IMAGES: &str = "https://image.tmdb.org/t/p";

/// The size first run's still and Home's hero ask a textless backdrop at: a
/// phone's or a TV's full width.
pub const STILL_SIZE: &str = "w1280";

/// An image path TMDB gave ("/abc.png"), only as a plain file name: nothing
/// an answer could turn into a query or another host.
fn image_path(value: &Value) -> Option<&str> {
    value.as_str().filter(|l| {
        l.strip_prefix('/').is_some_and(|f| {
            !f.is_empty()
                && !f.contains("..")
                && f.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
    })
}

/// The logo path of each provider `watch_providers` lists, by provider id.
pub fn provider_logos(body: &Value) -> std::collections::HashMap<u64, String> {
    body["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p["provider_id"].as_u64()?,
                image_path(&p["logo_path"])?.to_owned(),
            ))
        })
        .collect()
}

/// One person of a title's cast, as TMDB lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Credit {
    pub name: String,
    /// The role, when TMDB names one.
    pub character: Option<String>,
    /// The headshot, 185 px wide, when TMDB has one.
    pub photo: Option<String>,
}

/// The cast of a `credits` answer, in TMDB's billing order, each with a
/// movie's `character` or a show's first role's.
pub fn credits(body: &Value) -> Vec<Credit> {
    let text = |v: &Value| {
        v.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    body["cast"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(Credit {
                name: text(&p["name"])?,
                character: text(&p["character"]).or_else(|| text(&p["roles"][0]["character"])),
                photo: image_path(&p["profile_path"]).map(|path| format!("{IMAGES}/w185{path}")),
            })
        })
        .collect()
}

/// The best textless backdrop of an `images` answer, as its path: a
/// landscape frame at least 1280 wide with no language (no title or words
/// printed on it), the best voted first, the first listed on a tie.
pub fn textless_backdrop(body: &Value) -> Option<String> {
    let mut best: Option<(&str, f64, u64)> = None;
    for b in body["backdrops"].as_array()? {
        let wide = b["width"].as_u64().is_some_and(|w| w >= 1280)
            && b["aspect_ratio"].as_f64().is_some_and(|r| r >= 1.5);
        let Some(path) = image_path(&b["file_path"]) else {
            continue;
        };
        if !b["iso_639_1"].is_null() || !wide {
            continue;
        }
        let votes = (
            b["vote_average"].as_f64().unwrap_or(0.0),
            b["vote_count"].as_u64().unwrap_or(0),
        );
        if best.is_none_or(|(_, a, n)| votes.0 > a || (votes.0 == a && votes.1 > n)) {
            best = Some((path, votes.0, votes.1));
        }
    }
    best.map(|(path, ..)| path.to_owned())
}

/// What `find` says of an IMDb id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Found {
    /// `movie` or `series`, as addons name the kinds.
    pub kind: &'static str,
    /// TMDB's own id, for its page.
    pub id: u64,
    /// TMDB's user score, 0 to 10.
    pub score: f64,
    pub votes: u64,
}

/// What `find` lists for an IMDb id: its first movie and its first show.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Finds {
    pub movie: Option<Found>,
    pub show: Option<Found>,
}

impl Finds {
    /// The answer for a page of `kind` (`movie` or `series`): only that
    /// kind's, so TMDB's badge never describes another title than Trakt's;
    /// with no kind (an anime, whose kind TMDB tells), the movie, else the
    /// show.
    pub fn of(&self, kind: Option<&str>) -> Option<Found> {
        match kind {
            Some("movie") => self.movie,
            Some("series") => self.show,
            _ => self.movie.or(self.show),
        }
    }
}

/// Both kinds of a `find` answer.
pub fn finds(body: &Value) -> Finds {
    let item = |list: &str, kind: &'static str| -> Option<Found> {
        let item = body[list].get(0)?;
        Some(Found {
            kind,
            id: item["id"].as_u64()?,
            score: item["vote_average"]
                .as_f64()
                .filter(|s| (0.0..=10.0).contains(s))?,
            votes: item["vote_count"].as_u64()?,
        })
    };
    Finds {
        movie: item("movie_results", "movie"),
        show: item("tv_results", "series"),
    }
}

/// The first movie, else the first show, of a `find` answer.
pub fn found(body: &Value) -> Option<Found> {
    finds(body).of(None)
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
                id: 278,
                score: 8.733,
                votes: 31655
            })
        );
        let show = serde_json::json!({"movie_results": [], "tv_results": [{"id": 1396, "vote_average": 9.0, "vote_count": 18925}]});
        assert_eq!(found(&show).map(|f| f.kind), Some("series"));
        let none = serde_json::json!({"movie_results": [], "tv_results": [], "person_results": []});
        assert_eq!(found(&none), None);
        let odd = serde_json::json!({"movie_results": [{"id": 1, "vote_average": 12.0, "vote_count": 3}]});
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

#[cfg(test)]
mod credit_tests {
    use super::*;

    /// The shapes of a movie's credits and a show's aggregate credits.
    #[test]
    fn credits_give_name_role_and_headshot_in_billing_order() {
        let movie = serde_json::json!({"id": 278, "cast": [
            {"name": "Tim Robbins", "character": "Andy Dufresne", "profile_path": "/djLVFETFTvPyVUdrd7aLVykobof.jpg"},
            {"name": "Morgan Freeman", "character": "Ellis Boyd 'Red' Redding", "profile_path": null},
            {"name": "", "character": "Nobody"}
        ]});
        assert_eq!(
            credits(&movie),
            [
                Credit {
                    name: "Tim Robbins".into(),
                    character: Some("Andy Dufresne".into()),
                    photo: Some(
                        "https://image.tmdb.org/t/p/w185/djLVFETFTvPyVUdrd7aLVykobof.jpg".into()
                    ),
                },
                Credit {
                    name: "Morgan Freeman".into(),
                    character: Some("Ellis Boyd 'Red' Redding".into()),
                    photo: None,
                },
            ]
        );
        let show = serde_json::json!({"id": 1396, "cast": [
            {"name": "Bryan Cranston", "roles": [{"character": "Walter White", "episode_count": 62}], "profile_path": "/x.jpg"},
            {"name": "Odd", "roles": [], "profile_path": "//evil.host/x.jpg"}
        ]});
        let cast = credits(&show);
        assert_eq!(cast[0].character.as_deref(), Some("Walter White"));
        assert_eq!(cast[1].character, None);
        assert_eq!(cast[1].photo, None);
    }
}

#[cfg(test)]
mod still_tests {
    use super::textless_backdrop;

    /// Only a textless (no language), wide, large frame is taken, the best
    /// voted; a path that is not a plain file name never is.
    #[test]
    fn the_still_is_the_best_voted_textless_wide_backdrop() {
        let body = serde_json::json!({"backdrops": [
            {"file_path": "/titled.jpg", "iso_639_1": "en", "width": 3840, "aspect_ratio": 1.778, "vote_average": 9.0, "vote_count": 50},
            {"file_path": "/small.jpg", "iso_639_1": null, "width": 780, "aspect_ratio": 1.778, "vote_average": 8.0, "vote_count": 50},
            {"file_path": "/tall.jpg", "iso_639_1": null, "width": 2000, "aspect_ratio": 0.667, "vote_average": 8.0, "vote_count": 50},
            {"file_path": "//evil.host/x.jpg", "iso_639_1": null, "width": 3840, "aspect_ratio": 1.778, "vote_average": 9.9, "vote_count": 99},
            {"file_path": "/scene.jpg", "iso_639_1": null, "width": 1920, "aspect_ratio": 1.778, "vote_average": 5.4, "vote_count": 4},
            {"file_path": "/best.jpg", "iso_639_1": null, "width": 3840, "aspect_ratio": 1.778, "vote_average": 5.6, "vote_count": 2},
            {"file_path": "/tie.jpg", "iso_639_1": null, "width": 3840, "aspect_ratio": 1.778, "vote_average": 5.6, "vote_count": 2}
        ]});
        assert_eq!(textless_backdrop(&body).as_deref(), Some("/best.jpg"));
        let titled = serde_json::json!({"backdrops": [
            {"file_path": "/titled.jpg", "iso_639_1": "en", "width": 3840, "aspect_ratio": 1.778}
        ]});
        assert_eq!(textless_backdrop(&titled), None);
        assert_eq!(textless_backdrop(&serde_json::json!({"posters": []})), None);
    }
}

#[cfg(test)]
mod logo_tests {
    use super::provider_logos;

    #[test]
    fn a_logo_path_is_a_plain_file_name() {
        let body = serde_json::json!({"results": [
            {"provider_id": 8, "logo_path": "/abc_D-1.png"},
            {"provider_id": 9, "logo_path": "//evil.host/x.png"},
            {"provider_id": 10, "logo_path": "/x.png?y=1"},
            {"provider_id": 11, "logo_path": "/../x.png"},
            {"provider_id": 12, "logo_path": "x.png"}
        ]});
        let logos = provider_logos(&body);
        assert_eq!(logos.len(), 1);
        assert_eq!(logos[&8], "/abc_D-1.png");
    }
}
