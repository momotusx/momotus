//! Title search for the search page's "Top results" (plans/search-quality.md):
//! movies and shows by name, read as IMDb ids.

use super::api::{Answer, Api};
use mq_http::Url;
use serde_json::Value;

/// A title Trakt found, by its IMDb id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `movie` or `series`, as addons name the kinds.
    pub kind: &'static str,
    pub imdb_id: String,
    pub title: String,
}

/// Up to 10 movies and shows for `query`. With the profile's own `token`
/// the call counts against that user's limit instead of the app's.
pub async fn titles(api: &Api, query: &str, token: Option<&str>) -> Answer {
    let mut url = Url::parse("http://trakt/search/movie,show").expect("a fixed URL");
    url.query_pairs_mut()
        .append_pair("query", query)
        .append_pair("limit", "10");
    let path = format!("{}?{}", url.path(), url.query().unwrap_or_default());
    api.read(&path, token).await
}

/// Trakt's path segment for a kind: `movies` for a movie, `shows` for a
/// series.
fn plural(kind: &str) -> &'static str {
    if kind == "movie" { "movies" } else { "shows" }
}

/// Trakt's community score for a title by its IMDb id (`kind` is `movie`
/// or `series`), for the title page (plans/title-page-ratings-genres-similar.md).
pub async fn ratings(api: &Api, kind: &str, imdb: &str, token: Option<&str>) -> Answer {
    api.read(&format!("/{}/{imdb}/ratings", plural(kind)), token)
        .await
}

/// Up to 20 titles Trakt relates to this one, for "More like this".
pub async fn related(api: &Api, kind: &str, imdb: &str, token: Option<&str>) -> Answer {
    api.read(&format!("/{}/{imdb}/related?limit=20", plural(kind)), token)
        .await
}

/// The rating (0 to 10) and vote count of a ratings answer.
pub fn score(body: &Value) -> Option<(f64, u64)> {
    let rating = body["rating"]
        .as_f64()
        .filter(|r| (0.0..=10.0).contains(r))?;
    Some((rating, body["votes"].as_u64()?))
}

/// The titles of a related answer that carry an IMDb id, in Trakt's order,
/// as `kind` (`movie` or `series`): the related titles share the kind.
pub fn related_found(body: &Value, kind: &'static str) -> Vec<Found> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let imdb_id = item["ids"]["imdb"]
                .as_str()
                .filter(|id| id.starts_with("tt"))?;
            Some(Found {
                kind,
                imdb_id: imdb_id.to_owned(),
                title: item["title"].as_str()?.to_owned(),
            })
        })
        .collect()
}

/// The hits of a search answer that carry an IMDb id, in Trakt's order.
pub fn found(body: &Value) -> Vec<Found> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|hit| {
            let (kind, item) = match hit["type"].as_str()? {
                "movie" => ("movie", &hit["movie"]),
                "show" => ("series", &hit["show"]),
                _ => return None,
            };
            let imdb_id = item["ids"]["imdb"]
                .as_str()
                .filter(|id| id.starts_with("tt"))?;
            Some(Found {
                kind,
                imdb_id: imdb_id.to_owned(),
                title: item["title"].as_str()?.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ratings answer gives its rating and votes; a rating off the scale
    /// or a missing field gives nothing.
    #[test]
    fn a_ratings_answer_gives_rating_and_votes() {
        let body =
            serde_json::json!({"rating": 9.16432, "votes": 48623, "distribution": {"10": 1}});
        assert_eq!(score(&body), Some((9.16432, 48623)));
        assert_eq!(
            score(&serde_json::json!({"rating": 11.0, "votes": 3})),
            None
        );
        assert_eq!(score(&serde_json::json!({"votes": 3})), None);
        assert_eq!(score(&serde_json::Value::Null), None);
    }

    /// Related titles keep Trakt's order and their IMDb ids; one with none
    /// is dropped.
    #[test]
    fn related_titles_read_as_imdb_ids() {
        let body = serde_json::json!([
            {"title": "Cool Hand Luke", "year": 1967, "ids": {"imdb": "tt0061512", "tmdb": 903}},
            {"title": "No Id", "ids": {"imdb": null}},
            {"title": "Papillon", "ids": {"imdb": "tt0070511"}},
        ]);
        let found = related_found(&body, "movie");
        assert_eq!(
            found
                .iter()
                .map(|f| (f.kind, f.imdb_id.as_str()))
                .collect::<Vec<_>>(),
            [("movie", "tt0061512"), ("movie", "tt0070511")]
        );
    }

    /// Shows read as series; a hit with no IMDb id, or of another type, is
    /// dropped; Trakt's order stays.
    #[test]
    fn hits_read_as_kinds_and_imdb_ids() {
        let body = serde_json::json!([
            {"type": "movie", "score": 1000, "movie": {"title": "Sintel", "year": 2010, "ids": {"trakt": 1, "imdb": "tt1727587"}}},
            {"type": "show", "show": {"title": "Severance", "year": 2022, "ids": {"imdb": "tt11280740"}}},
            {"type": "movie", "movie": {"title": "No Id", "ids": {"imdb": null}}},
            {"type": "person", "person": {"name": "Someone", "ids": {"imdb": "nm1"}}},
        ]);
        let hits = found(&body);
        assert_eq!(
            hits,
            [
                Found {
                    kind: "movie",
                    imdb_id: "tt1727587".into(),
                    title: "Sintel".into()
                },
                Found {
                    kind: "series",
                    imdb_id: "tt11280740".into(),
                    title: "Severance".into()
                },
            ]
        );
        assert!(found(&serde_json::json!({"error": "x"})).is_empty());
    }
}
