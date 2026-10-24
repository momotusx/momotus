//! Every surface that shows a cover shows RPDB's band (the owner, 2026-10-23:
//! "the tomato shows on all of them"): Home's rows and hero, Continue
//! Watching, Next Up, Library, Search, Discover and the title page. A title
//! with an IMDb id gets that id's cover; an anime its addon names by a
//! `kitsu:` id gets the cover of the IMDb id the addon gave beside it
//! (`imdb_id`), on surfaces that never read the addon's answer too
//! (Library); a title with neither keeps its addon's poster. The key is the
//! one an installed, enabled addon was configured with.

use mq_library::{AddonRow, LibraryItemRow, ProgressRow};
use mq_runtime::views::home::{CONTINUE_ROW_ID, Tile};
use mq_runtime::views::title::TitleState;
use mq_runtime::{Core, DiscoverView, HomeView, LibraryView, SearchView, TitleView};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const MOVIE: &str = r#"{"id":"tt0111161","type":"movie","name":"The Shawshank Redemption","poster":"https://img.example/tt0111161.jpg","background":"https://img.example/bg.jpg"}"#;
const ANIME: &str = r#"{"id":"kitsu:50016","type":"anime","name":"Cyberpunk: Edgerunners II","poster":"https://img.example/50016.jpg","imdb_id":"tt12590266"}"#;
const UNMAPPED: &str = r#"{"id":"kitsu:13874","type":"anime","name":"Fate/Grand Order CMs","poster":"https://img.example/13874.jpg","imdb_id":null}"#;
/// A catalog item whose addon serves an RPDB poster itself, under its own key
/// (the TMDB addon and AIOMetadata take one in their own config).
const SERVED: &str = r#"{"id":"tt0068646","type":"movie","name":"The Godfather","poster":"https://api.ratingposterdb.com/addon-own/imdb/poster-default/tt0068646.jpg"}"#;
const ANIME_META: &str = r#"{"meta":{"id":"kitsu:50016","type":"anime","name":"Cyberpunk: Edgerunners II","poster":"https://img.example/50016.jpg","imdb_id":"tt12590266","videos":[
  {"id":"kitsu:50016:1","title":"One","season":1,"episode":1,"released":"2020-01-01T00:00:00Z"},
  {"id":"kitsu:50016:2","title":"Two","season":1,"episode":2,"released":"2020-01-08T00:00:00Z"}]}}"#;

fn rpdb(imdb: &str) -> String {
    format!(
        "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/{imdb}.jpg?fallback=true"
    )
}

/// One addon with a catalog (as Home, Discover and Search ask it) and metas
/// for the three titles; the Kitsu addon's shape, `imdb_id` beside the id.
async fn fixture() -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
                let path = percent_encoding::percent_decode_str(&path)
                    .decode_utf8_lossy()
                    .into_owned();
                let (status, body) = if path.starts_with("/catalog/") {
                    (
                        200,
                        format!(r#"{{"metas":[{MOVIE},{ANIME},{UNMAPPED},{SERVED}]}}"#),
                    )
                } else if path.starts_with("/meta/") && path.contains("kitsu:50016") {
                    (200, ANIME_META.to_owned())
                } else if path.starts_with("/meta/") && path.contains("kitsu:13874") {
                    (200, format!(r#"{{"meta":{UNMAPPED}}}"#))
                } else if path.starts_with("/meta/") && path.contains("tt0111161") {
                    (200, format!(r#"{{"meta":{MOVIE}}}"#))
                } else {
                    (404, "{}".to_owned())
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "fixture", "version": "1.0.0", "name": "Fixture",
        "types": ["movie", "anime"], "resources": ["catalog", "meta"], "idPrefixes": ["tt", "kitsu"],
        "catalogs": [
            {"type": "movie", "id": "top", "name": "Popular"},
            {"type": "movie", "id": "find", "name": "Find", "extra": [{"name": "search", "isRequired": true}]}
        ]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

/// The Rotten Tomatoes addon as the defaults install it, its link carrying
/// RPDB's free key. No catalogs, so nothing is read from it.
fn keyed() -> AddonRow {
    AddonRow {
        transport_url: "https://7a82163c306e-rottentomatoes.baby-beamup.club/%7B%22rpdb_key%22%3A%22t0-free-rpdb%22%7D/manifest.json".into(),
        manifest_json: serde_json::json!({
            "id": "pw.ers.rottentomatoes", "version": "1.0.9", "name": "Rotten Tomatoes",
            "types": ["movie", "series"], "resources": ["catalog"], "catalogs": []
        })
        .to_string(),
        position: 1,
        enabled: true,
        allow_private: false,
    }
}

async fn until<T>(what: &str, mut got: impl FnMut() -> Option<T>) -> T {
    for _ in 0..500 {
        if let Some(v) = got() {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{what} never came");
}

fn poster<'a>(tiles: &'a [Tile], id: &str) -> Option<&'a str> {
    tiles
        .iter()
        .find(|t| t.id == id)
        .and_then(|t| t.poster.as_deref())
}

/// The three titles' covers on one surface: IMDb's, the anime's by the IMDb
/// id its addon gave, and the unmapped anime's own poster.
fn all_three(surface: &str, tiles: &[Tile]) {
    assert_eq!(
        poster(tiles, "tt0111161"),
        Some(rpdb("tt0111161").as_str()),
        "{surface}: IMDb title"
    );
    assert_eq!(
        poster(tiles, "kitsu:50016"),
        Some(rpdb("tt12590266").as_str()),
        "{surface}: anime with an IMDb id"
    );
    assert_eq!(
        poster(tiles, "kitsu:13874"),
        Some("https://img.example/13874.jpg"),
        "{surface}: anime without one"
    );
}

#[tokio::test]
async fn every_surface_that_shows_a_cover_shows_the_band() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[fixture().await, keyed()], &core.hlc())
        .unwrap();
    let lib = core.library();
    for (id, kind, name) in [
        ("tt0111161", "movie", "The Shawshank Redemption"),
        ("kitsu:50016", "anime", "Cyberpunk: Edgerunners II"),
        ("kitsu:13874", "anime", "Fate/Grand Order CMs"),
    ] {
        let row = LibraryItemRow {
            item_id: id.into(),
            kind: kind.into(),
            name: name.into(),
            poster: Some(format!(
                "https://img.example/{}.jpg",
                id.trim_start_matches("kitsu:")
            )),
            poster_shape: None,
            added_ms: 1,
        };
        lib.upsert_item("p", &row, true, &core.hlc()).unwrap();
    }
    // The movie part way (Continue Watching); the anime's first episode done (Next Up).
    for (video, item, position) in [
        ("tt0111161", "tt0111161", 600_000),
        ("kitsu:50016:1", "kitsu:50016", 2_990_000),
    ] {
        let p = ProgressRow {
            video_id: video,
            item_id: item,
            position_ms: position,
            duration_ms: 3_000_000,
            updated_ms: 5,
        };
        lib.set_progress("p", p, &core.hlc()).unwrap();
    }

    // Home: the catalog row and the hero.
    let home = HomeView::open(&core);
    let row = until("Home's catalog row", || {
        home.snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id != CONTINUE_ROW_ID && r.tiles.len() == 4)
    })
    .await;
    all_three("Home row", &row.tiles);
    let hero = until("the hero", || home.snapshot().header.heroes.first().cloned()).await;
    assert_eq!(
        hero.tile.poster.as_deref(),
        Some(rpdb("tt0111161").as_str()),
        "hero"
    );

    // Continue Watching and Next Up.
    let cw = until("Continue Watching with both", || {
        home.snapshot().rows.into_iter().find(|r| {
            r.row_id == CONTINUE_ROW_ID
                && r.tiles.len() == 2
                && r.tiles
                    .iter()
                    .any(|t| t.video_id.as_deref() == Some("kitsu:50016:2"))
        })
    })
    .await;
    assert_eq!(
        poster(&cw.tiles, "tt0111161"),
        Some(rpdb("tt0111161").as_str()),
        "Continue Watching"
    );
    assert_eq!(
        poster(&cw.tiles, "kitsu:50016"),
        Some(rpdb("tt12590266").as_str()),
        "Next Up"
    );

    // Library: never reads the addon's answer, so the anime's IMDb id is the kept one.
    let library = LibraryView::open(&core);
    all_three("Library", &library.snapshot().items);

    // Discover.
    let discover = DiscoverView::open(&core);
    let items = until("Discover's items", || {
        let items = discover.snapshot().items;
        (items.len() == 4).then_some(items)
    })
    .await;
    all_three("Discover", &items);

    // Search.
    let search = SearchView::open(&core);
    search.set_query("cyberpunk");
    let tiles = until("Search's results", || {
        search
            .snapshot()
            .rows
            .into_iter()
            .find(|r| r.tiles.len() == 4)
            .map(|r| r.tiles)
    })
    .await;
    all_three("Search", &tiles);

    // The title page: the rated cover beside the addon's poster.
    for (kind, id, want) in [
        ("movie", "tt0111161", Some(rpdb("tt0111161"))),
        ("anime", "kitsu:50016", Some(rpdb("tt12590266"))),
        ("anime", "kitsu:13874", None),
    ] {
        let view = TitleView::open(&core, kind, id);
        let details = until("the title page", || {
            let header = view.snapshot().header;
            (header.state == TitleState::Ready)
                .then_some(header.details)
                .flatten()
        })
        .await;
        assert_eq!(details.rated_poster, want, "title page {id}");
        assert!(
            details.poster.is_some(),
            "the addon's poster stays beside it"
        );
    }
}

/// No enabled addon with a key: no band anywhere, the title page's rated
/// cover is null.
#[tokio::test]
async fn without_a_key_no_surface_shows_the_band() {
    let core = Core::temporary("p").unwrap();
    let mut off = keyed();
    off.enabled = false;
    core.library()
        .set_addons("p", &[fixture().await, off], &core.hlc())
        .unwrap();
    let home = HomeView::open(&core);
    let row = until("Home's catalog row", || {
        home.snapshot()
            .rows
            .into_iter()
            .find(|r| r.tiles.len() == 4)
    })
    .await;
    assert_eq!(
        poster(&row.tiles, "tt0111161"),
        Some("https://img.example/tt0111161.jpg")
    );
    assert_eq!(
        poster(&row.tiles, "kitsu:50016"),
        Some("https://img.example/50016.jpg")
    );
    // An RPDB poster the addon serves itself stays: no key of ours is in it.
    assert_eq!(
        poster(&row.tiles, "tt0068646"),
        Some("https://api.ratingposterdb.com/addon-own/imdb/poster-default/tt0068646.jpg")
    );
    let view = TitleView::open(&core, "movie", "tt0111161");
    let details = until("the title page", || {
        let header = view.snapshot().header;
        (header.state == TitleState::Ready)
            .then_some(header.details)
            .flatten()
    })
    .await;
    assert_eq!(details.rated_poster, None);
}

/// An anime's IMDb id also comes from its meta (the title page, Continue
/// Watching, Next Up read it), and is kept: a Library that never asked the
/// addon shows the band once the meta was read.
#[tokio::test]
async fn a_meta_answer_gives_an_anime_its_imdb_id_for_good() {
    let core = Core::temporary("p").unwrap();
    core.library()
        .set_addons("p", &[fixture().await, keyed()], &core.hlc())
        .unwrap();
    let row = LibraryItemRow {
        item_id: "kitsu:50016".into(),
        kind: "anime".into(),
        name: "Cyberpunk: Edgerunners II".into(),
        poster: Some("https://img.example/50016.jpg".into()),
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("p", &row, true, &core.hlc())
        .unwrap();
    let before = LibraryView::open(&core).snapshot().items;
    assert_eq!(
        poster(&before, "kitsu:50016"),
        Some("https://img.example/50016.jpg"),
        "no answer read yet"
    );
    let view = TitleView::open(&core, "anime", "kitsu:50016");
    let details = until("the title page", || {
        let header = view.snapshot().header;
        (header.state == TitleState::Ready)
            .then_some(header.details)
            .flatten()
    })
    .await;
    assert_eq!(details.rated_poster, Some(rpdb("tt12590266")));
    let after = LibraryView::open(&core).snapshot().items;
    assert_eq!(
        poster(&after, "kitsu:50016"),
        Some(rpdb("tt12590266").as_str())
    );
}

/// A cover stored as RPDB's (a play record from an older build) is dropped
/// once no enabled addon gives a key: no band, and no key that is gone.
#[tokio::test]
async fn a_stored_rpdb_cover_goes_with_the_key() {
    let core = Core::temporary("p").unwrap();
    let mut off = keyed();
    off.enabled = false;
    core.library().set_addons("p", &[off], &core.hlc()).unwrap();
    let row = LibraryItemRow {
        item_id: "tt0111161".into(),
        kind: "movie".into(),
        name: "The Shawshank Redemption".into(),
        poster: Some(rpdb("tt0111161")),
        poster_shape: None,
        added_ms: 1,
    };
    let lib = core.library();
    lib.upsert_item("p", &row, false, &core.hlc()).unwrap();
    let p = ProgressRow {
        video_id: "tt0111161",
        item_id: "tt0111161",
        position_ms: 600_000,
        duration_ms: 3_000_000,
        updated_ms: 5,
    };
    lib.set_progress("p", p, &core.hlc()).unwrap();
    let home = HomeView::open(&core);
    let cw = until("Continue Watching", || {
        home.snapshot()
            .rows
            .into_iter()
            .find(|r| r.row_id == CONTINUE_ROW_ID && !r.tiles.is_empty())
    })
    .await;
    assert_eq!(cw.tiles[0].poster, None);
    assert_eq!(cw.tiles[0].poster_fallback, None);
}
