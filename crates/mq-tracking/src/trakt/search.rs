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
