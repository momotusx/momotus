use mq_protocol::{ResourceResponse, decode_catalog, decode_catalog_head, decode_meta};

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

/// An item's `imdbRating` reads as its IMDb rating (an IMDb link, the form
/// every reader takes it from) also when the item has `links` of its own:
/// stremio-core turns the field into a link only when `links` is absent.
/// A string or a number; an IMDb link already there wins; no usable rating
/// adds nothing; an id that is not an IMDb id gets no IMDb link at all, with
/// `links` or without (stremio-core would make one from the field).
#[test]
fn an_items_imdb_rating_reads_as_its_imdb_link() {
    let imdb = |body: &[u8]| -> Vec<Vec<String>> {
        match decode_catalog(body).expect("decodes") {
            ResourceResponse::Metas { metas } => metas
                .into_iter()
                .map(|m| {
                    m.links
                        .into_iter()
                        .filter(|l| l.category == "imdb")
                        .map(|l| l.name)
                        .collect()
                })
                .collect(),
            other => panic!("expected metas, got {other:?}"),
        }
    };
    let body = br#"{"metas": [
        {"id": "tt1", "type": "movie", "name": "One", "imdbRating": "5.6",
         "links": [{"name": "Drama", "category": "Genres", "url": "stremio:///discover"}]},
        {"id": "tt2", "type": "movie", "name": "Two", "imdbRating": 7, "links": []},
        {"id": "tt3", "type": "movie", "name": "Three", "imdbRating": "8.1",
         "links": [{"name": "8.0", "category": "imdb", "url": "https://imdb.com/title/tt3"}]},
        {"id": "tt4", "type": "movie", "name": "Four", "imdbRating": "N/A", "links": []},
        {"id": "kitsu:5", "type": "series", "name": "Five", "imdbRating": "6.5", "links": null},
        {"id": "kitsu:6", "type": "series", "name": "Six", "imdbRating": "7.6"},
        {"id": "kitsu:7", "type": "series", "name": "Seven", "imdbRating": "7.6", "links": []}
    ]}"#;
    assert_eq!(
        imdb(body),
        [
            vec!["5.6".to_string()],
            vec!["7.0".into()],
            vec!["8.0".into()],
            vec![],
            // Not an IMDb id: its rating is not IMDb's (Torrent Catalogs'
            // anime items carry Kitsu's score with no link, 2026-10-26).
            vec![],
            vec![],
            vec![]
        ]
    );
    // A meta answer too.
    let meta = br#"{"meta": {"id": "tt1", "type": "movie", "name": "One", "imdbRating": "5.6", "links": []}}"#;
    match mq_protocol::decode_meta(meta).expect("decodes") {
        ResourceResponse::Meta { meta } => {
            let names: Vec<String> = meta
                .preview
                .links
                .into_iter()
                .filter(|l| l.category == "imdb")
                .map(|l| l.name)
                .collect();
            assert_eq!(names, ["5.6"]);
        }
        other => panic!("expected a meta, got {other:?}"),
    }
}

/// Cinemeta sends `Judith O&apos;Dea` in a cast link: every text an item shows reads as plain text, decoded
/// once (`&amp;apos;` is the text `&apos;`), in a title page's meta and in a catalog's items alike.
#[test]
fn item_text_has_its_html_entities_decoded_once() {
    let item = serde_json::json!({
        "id": "tt0063350", "type": "movie", "name": "Tom &amp; Jerry",
        "description": "It&apos;s &quot;dark&quot;. &amp;apos; stays &lt;b&gt; &nosuch; R&D",
        "links": [
            {"name": "Judith O&apos;Dea", "category": "Cast", "url": "stremio:///search?search=x"},
            {"name": "Sci-Fi &amp; Horror", "category": "Genres", "url": "stremio:///discover"}
        ],
        "videos": [{"id": "a", "title": "Don&#39;t", "overview": "Rock &amp; roll", "released": "2020-01-01T00:00:00.000Z"}]
    });
    let ResourceResponse::Meta { meta } =
        decode_meta(serde_json::json!({ "meta": item }).to_string().as_bytes()).unwrap()
    else {
        panic!("expected a meta");
    };
    let names: Vec<_> = meta.preview.links.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names[..2], ["Judith O'Dea", "Sci-Fi & Horror"]);
    assert_eq!(meta.preview.name, "Tom & Jerry");
    assert_eq!(
        meta.preview.description.as_deref(),
        Some("It's \"dark\". &apos; stays <b> &nosuch; R&D")
    );
    assert_eq!(meta.videos[0].title, "Don't");
    assert_eq!(meta.videos[0].overview.as_deref(), Some("Rock & roll"));

    let ResourceResponse::Metas { metas } = decode_catalog(
        serde_json::json!({ "metas": [item] })
            .to_string()
            .as_bytes(),
    )
    .unwrap() else {
        panic!("expected metas");
    };
    assert_eq!(metas[0].name, "Tom & Jerry");
    assert_eq!(metas[0].links[0].name, "Judith O'Dea");
}

/// The legacy `genres` string list (read as links when an item has none) decodes too.
#[test]
fn legacy_genres_are_decoded_too() {
    let body = br#"{"meta": {"id": "tt1", "type": "movie", "name": "X", "genres": ["Sci-Fi &amp; Horror"]}}"#;
    let ResourceResponse::Meta { meta } = decode_meta(body).unwrap() else {
        panic!("expected a meta");
    };
    let names: Vec<_> = meta.preview.links.iter().map(|l| l.name.as_str()).collect();
    assert!(names.contains(&"Sci-Fi & Horror"), "{names:?}");
}

/// The head of a catalog is `decode_catalog`'s first distinct ids: a broken
/// item or a repeat in the first part is made up from the items after it,
/// and every shape that is not a plain list reads as `decode_catalog` does.
#[test]
fn a_catalogs_head_is_its_first_distinct_titles() {
    let item = |id: serde_json::Value| serde_json::json!({"id": id, "type": "series", "name": "X", "imdbRating": "7.1"});
    let mut list: Vec<_> = (0..30).map(|n| item(format!("tt{n}").into())).collect();
    list[1] = item(2.into());
    list[3] = item("tt0".into());
    list[5] = item("tt4".into());
    let body = serde_json::json!({ "metas": list, "cacheMaxAge": 60 }).to_string();
    let head = |body: &[u8], limit| match decode_catalog_head(body, limit) {
        Ok(ResourceResponse::Metas { metas }) => Ok(metas),
        Ok(other) => panic!("expected metas, got {other:?}"),
        Err(e) => Err(e.to_string()),
    };
    let ResourceResponse::Metas { metas: all } = decode_catalog(body.as_bytes()).unwrap() else {
        panic!("expected metas");
    };
    let mut seen = std::collections::HashSet::new();
    let distinct: Vec<_> = all
        .into_iter()
        .filter(|m| seen.insert(m.id.clone()))
        .collect();
    for limit in [1, 4, 20, 40] {
        let want: Vec<_> = distinct.iter().take(limit).cloned().collect();
        assert_eq!(head(body.as_bytes(), limit).unwrap(), want, "limit {limit}");
    }
    for body in [
        r#"{}"#,
        r#"{"metas": null}"#,
        r#"{"metas": {"id": "tt1"}}"#,
        r#"{"error": "no user"}"#,
        r#"{"metas": [], "streams": []}"#,
        "[]",
        "not json",
    ] {
        let old = decode_catalog(body.as_bytes()).map_err(|e| e.to_string());
        let new = decode_catalog_head(body.as_bytes(), 20).map_err(|e| e.to_string());
        assert_eq!(new, old, "{body}");
    }
}
