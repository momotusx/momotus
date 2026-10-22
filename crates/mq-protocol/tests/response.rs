use mq_protocol::{ResourceResponse, decode_catalog};

fn ids(body: &[u8]) -> Vec<String> {
    match decode_catalog(body).expect("decodes") {
        ResourceResponse::Metas { metas } => metas.into_iter().map(|m| m.id).collect(),
        other => panic!("expected metas, got {other:?}"),
    }
}

/// Trakt Tv's "Trending movies" as it answered on 2026-10-22 (one GET of
/// `/catalog/trakt/trakt_trending_movies.json`, 200): `{}`, no `metas` key.
/// stremio-core refuses it ("didn't contain any of the required keys"), so
/// the Home row read "This row did not load"; it is an empty list, and so
/// is an object holding only caching hints.
#[test]
fn an_empty_object_is_an_empty_catalog() {
    let body = include_bytes!("fixtures/trakt_trending_movies.json");
    assert!(serde_json::from_slice::<ResourceResponse>(body).is_err());
    assert!(ids(body).is_empty());
    assert!(ids(br#"{"cacheMaxAge": 3600, "staleRevalidate": 60, "staleError": 60}"#).is_empty());
}

/// Any other shape without `metas` is never a catalog: an addon's error
/// object is an error, and a list for another resource reads as that
/// resource (as in stremio-core), which every catalog reader refuses.
#[test]
fn an_error_object_or_another_resources_list_is_never_a_catalog() {
    for body in [
        r#"{"err": "upstream failed"}"#,
        r#"{"error": "no user"}"#,
        r#"{"message": "rate limited"}"#,
        r#"{"cacheMaxAge": 60, "error": "x"}"#,
        r#"{"streams": []}"#,
    ] {
        assert!(
            !matches!(
                decode_catalog(body.as_bytes()),
                Ok(ResourceResponse::Metas { .. })
            ),
            "{body}"
        );
    }
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

/// A body that is not an object is still an error.
#[test]
fn a_body_that_is_not_an_object_stays_an_error() {
    assert!(decode_catalog(b"[]").is_err());
    assert!(decode_catalog(b"not json").is_err());
}
