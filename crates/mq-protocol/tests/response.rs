use mq_protocol::{ResourceResponse, decode_response};

fn ids(body: &[u8]) -> Vec<String> {
    match decode_response(body, "catalog").expect("decodes") {
        ResourceResponse::Metas { metas } => metas.into_iter().map(|m| m.id).collect(),
        other => panic!("expected metas, got {other:?}"),
    }
}

/// Trakt Tv's "Trending movies" as it answered on 2026-10-22 (one GET of
/// `/catalog/trakt/trakt_trending_movies.json`, 200): `{}`, no `metas` key.
/// stremio-core refuses it ("didn't contain any of the required keys"), so
/// the Home row read "This row did not load"; it is an empty list.
#[test]
fn trakts_empty_object_is_an_empty_catalog() {
    let body = include_bytes!("fixtures/trakt_trending_movies.json");
    assert!(serde_json::from_slice::<ResourceResponse>(body).is_err());
    assert!(ids(body).is_empty());
}

/// What stremio-core already forgives stays forgiven: a null list is empty,
/// a wrong type in an optional field (a number for `releaseInfo`) still
/// reads, and an item that does not decode (a number for `id`) is skipped
/// while the rest show.
#[test]
fn null_lists_and_broken_items_read_as_stremio_reads_them() {
    assert!(ids(br#"{"metas": null}"#).is_empty());
    let body = br#"{"metas": [
        {"id": "tt1", "type": "movie", "name": "One", "poster": null, "releaseInfo": null},
        {"id": 2, "type": "movie", "name": "Two"},
        {"id": "tt3", "type": "movie", "name": "Three", "releaseInfo": 2010, "extra": {"any": 1}}
    ]}"#;
    assert_eq!(ids(body), ["tt1", "tt3"]);
}

/// Keys for other resources beside the asked one, which stremio-core
/// refuses as "more than 1 of the unique keys", are ignored.
#[test]
fn keys_for_other_resources_are_ignored() {
    let body = br#"{"metas": [{"id": "tt1", "type": "movie", "name": "One"}], "streams": [], "meta": null}"#;
    assert!(serde_json::from_slice::<ResourceResponse>(body).is_err());
    assert_eq!(ids(body), ["tt1"]);
}

/// Leniency stops where there is nothing to read: a meta answer without its
/// meta, or a body that is not an object, is still an error.
#[test]
fn a_missing_meta_or_a_body_that_is_not_an_object_stays_an_error() {
    assert!(decode_response(b"{}", "meta").is_err());
    assert!(decode_response(b"[]", "catalog").is_err());
    assert!(decode_response(b"not json", "catalog").is_err());
}
