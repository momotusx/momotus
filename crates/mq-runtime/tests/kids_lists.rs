//! A kids profile's metadata and lists (plans/m11-kids.md, phase K3a): the
//! meta check at the one choke point, Home, Search and Library from the
//! list only, each following it live, and the owner's addons minus adult
//! ones. A fixture addon logs every request, so "asked nothing" is checked.

use mq_library::{AddonRow, AllowedRow, LibraryItemRow, OWNER, ProgressRow};
use mq_runtime::views::home::{CONTINUE_ROW_ID, KIDS_ROW_ID};
use mq_runtime::{Core, HomeView, LibraryView, SearchView};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A local addon with catalogs, search and metas, logging each request.
#[derive(Clone, Default)]
struct Fixture {
    log: Arc<Mutex<Vec<String>>>,
    /// Meta answers by request path; else a meta of the asked kind and id.
    metas: Arc<Mutex<HashMap<String, Value>>>,
    /// How long a meta answer takes.
    delay: Arc<Mutex<Duration>>,
}

impl Fixture {
    async fn addon(&self, id: &str, adult: bool) -> AddonRow {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fixture = self.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let fixture = fixture.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = head
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned();
                    fixture.log.lock().unwrap().push(path.clone());
                    let body = fixture.answer(&path).await;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });
        let manifest = json!({
            "id": id, "version": "1.0.0", "name": id,
            "types": ["movie", "series"], "resources": ["catalog", "meta"], "idPrefixes": ["tt"],
            "catalogs": [
                {"type": "movie", "id": "top", "name": "Popular"},
                {"type": "movie", "id": "find", "name": "Search",
                 "extra": [{"name": "search", "isRequired": true}]}
            ],
            "behaviorHints": {"adult": adult}
        });
        AddonRow {
            transport_url: format!("http://{addr}/manifest.json"),
            manifest_json: manifest.to_string(),
            position: 0,
            enabled: true,
            allow_private: true,
        }
    }

    async fn answer(&self, path: &str) -> String {
        if path.starts_with("/catalog/") {
            return json!({"metas": [{"id": "tt500", "type": "movie", "name": "Popular",
                                     "background": "https://img.example/bg.jpg"}]})
            .to_string();
        }
        let delay = *self.delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        if let Some(meta) = self.metas.lock().unwrap().get(path) {
            return meta.to_string();
        }
        let rest = path.trim_start_matches("/meta/").trim_end_matches(".json");
        let (kind, id) = rest.split_once('/').unwrap_or_default();
        json!({"meta": {"id": id, "type": kind, "name": format!("Name {id}"),
                        "background": format!("https://img.example/{id}.jpg")}})
        .to_string()
    }

    fn asked(&self, prefix: &str) -> Vec<String> {
        let log = self.log.lock().unwrap();
        log.iter()
            .filter(|p| p.starts_with(prefix))
            .cloned()
            .collect()
    }
}

const PIN: &str = "1357";

/// The owner with a PIN and `fixture`'s addon, and a kids profile.
/// The Rotten Tomatoes addon as the defaults install it: its link carries
/// RPDB's free key, the key every IMDb cover's ratings band is drawn with.
/// No catalogs, so nothing is read from it.
fn rt_addon() -> AddonRow {
    AddonRow {
        transport_url: "https://7a82163c306e-rottentomatoes.baby-beamup.club/%7B%22rpdb_key%22%3A%22t0-free-rpdb%22%7D/manifest.json".into(),
        manifest_json: serde_json::json!({
            "id": "pw.ers.rottentomatoes", "version": "1.0.9", "name": "Rotten Tomatoes",
            "types": ["movie", "series"], "resources": ["catalog"], "catalogs": []
        })
        .to_string(),
        position: 0,
        enabled: true,
        allow_private: false,
    }
}

async fn kid(fixture: &Fixture) -> (Core, Core) {
    let owner = Core::temporary(OWNER).unwrap();
    owner.set_pin(OWNER, Some(PIN), None).await.unwrap();
    let addon = fixture.addon("meta", false).await;
    owner
        .library()
        .set_addons(OWNER, &[addon], &owner.hlc())
        .unwrap();
    let id = owner.create_kids_profile("Kid", "fox").unwrap();
    (owner.clone(), owner.for_profile(&id))
}

fn allow(kid: &Core, id: &str, kind: &str, name: &str, added_ms: i64) {
    let row = AllowedRow {
        item_id: id.into(),
        kind: kind.into(),
        name: name.into(),
        names: vec![],
        poster: None,
        allowed: true,
        added_ms,
    };
    kid.library()
        .write_allowed(kid.profile(), &row, &kid.hlc())
        .unwrap();
}

fn remove(kid: &Core, id: &str) {
    let row = AllowedRow {
        item_id: id.into(),
        kind: "movie".into(),
        name: String::new(),
        names: vec![],
        poster: None,
        allowed: false,
        added_ms: 0,
    };
    kid.library()
        .write_allowed(kid.profile(), &row, &kid.hlc())
        .unwrap();
}

/// What a playback writes: the title row (not in the library) and its
/// progress; `done` finishes the video.
fn played(core: &Core, kind: &str, id: &str, video: &str, at: i64, done: bool) {
    let item = LibraryItemRow {
        item_id: id.into(),
        kind: kind.into(),
        name: format!("Name {id}"),
        poster: None,
        poster_shape: None,
        added_ms: at,
    };
    let library = core.library();
    library
        .upsert_item(core.profile(), &item, false, &core.hlc())
        .unwrap();
    let progress = ProgressRow {
        video_id: video,
        item_id: id,
        position_ms: if done { 1000 } else { 300 },
        duration_ms: 1000,
        updated_ms: at,
    };
    library
        .set_progress(core.profile(), progress, &core.hlc())
        .unwrap();
}

async fn until(what: &str, check: impl Fn() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("waited 5 s for {what}");
}

fn row_ids(view: &HomeView, row: &str) -> Vec<String> {
    view.snapshot()
        .rows
        .into_iter()
        .find(|r| r.row_id == row)
        .map(|r| r.tiles.into_iter().map(|t| t.id).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn a_meta_of_another_title_makes_the_next_addon_answer() {
    let wrong = Fixture::default();
    let right = Fixture::default();
    let (owner, kid) = kid(&right).await;
    wrong.metas.lock().unwrap().extend([
        (
            "/meta/movie/tt1.json".to_owned(),
            json!({"meta": {"id": "tt2", "type": "movie", "name": "Another id"}}),
        ),
        (
            "/meta/movie/tt3.json".to_owned(),
            json!({"meta": {"id": "tt3", "type": "series", "name": "Another kind"}}),
        ),
    ]);
    let first = wrong.addon("wrong", false).await;
    let second = right.addon("right", false).await;
    owner
        .library()
        .set_addons(OWNER, &[first, second], &owner.hlc())
        .unwrap();
    let (meta, from) = kid.fetch_meta("movie", "tt1").await.unwrap();
    assert_eq!((meta.preview.id.as_str(), from.as_str()), ("tt1", "right"));
    let (meta, _) = kid.fetch_meta("movie", "tt3").await.unwrap();
    assert_eq!(meta.preview.name, "Name tt3", "the next addon's");
    // An open profile is as before: the first answer in addon order.
    let (meta, from) = owner.fetch_meta("movie", "tt1").await.unwrap();
    assert_eq!((meta.preview.id.as_str(), from.as_str()), ("tt2", "wrong"));
}

const FOREIGN: &str = "tt10:1:2";

fn series_with_a_foreign_video() -> Value {
    json!({"meta": {"id": "tt1", "type": "series", "name": "Show",
        "background": "https://img.example/tt1.jpg", "videos": [
        {"id": "tt1:1:1", "title": "Pilot", "season": 1, "episode": 1,
         "released": "2020-01-01T00:00:00Z", "thumbnail": "https://img.example/still.jpg"},
        {"id": FOREIGN, "title": "Not of this show", "season": 1, "episode": 2,
         "released": "2020-01-08T00:00:00Z"},
        {"id": "x:1", "title": "Another scheme", "season": 1, "episode": 3,
         "released": "2020-01-15T00:00:00Z"}]}})
}

#[tokio::test]
async fn videos_not_of_the_title_never_reach_a_kid() {
    let fixture = Fixture::default();
    fixture.metas.lock().unwrap().insert(
        "/meta/series/tt1.json".into(),
        series_with_a_foreign_video(),
    );
    let (owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "series", "Show", 1);
    let (meta, _) = kid.fetch_meta("series", "tt1").await.unwrap();
    let videos: Vec<&str> = meta.videos.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(videos, ["tt1:1:1"]);
    let (meta, _) = owner.fetch_meta("series", "tt1").await.unwrap();
    assert_eq!(meta.videos.len(), 3, "an open profile is as before");
    // Next Up after the pilot: the foreign episode is no next one.
    played(&kid, "series", "tt1", "tt1:1:1", 5, true);
    let home = HomeView::open(&kid);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let next: Vec<Option<String>> = home
        .snapshot()
        .rows
        .into_iter()
        .flat_map(|r| r.tiles)
        .map(|t| t.video_id)
        .collect();
    assert!(!next.contains(&Some(FOREIGN.to_owned())), "{next:?}");
}

#[tokio::test]
async fn a_kids_home_asks_no_catalog_and_shows_its_list() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", 1);
    allow(&kid, "tt2", "movie", "Cars", 2);
    allow(&kid, "tt3", "movie", "Gone", 3);
    remove(&kid, "tt3");
    let home = HomeView::open(&kid);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let rows: Vec<String> = home.snapshot().rows.into_iter().map(|r| r.row_id).collect();
    assert_eq!(rows, [KIDS_ROW_ID]);
    assert_eq!(row_ids(&home, KIDS_ROW_ID), ["tt2", "tt1"], "newest first");
    assert!(home.snapshot().header.heroes.is_empty());
    assert!(
        fixture.asked("/catalog/").is_empty(),
        "{:?}",
        fixture.asked("")
    );
}

/// The 20 most recently allowed, not the first 20.
#[tokio::test]
async fn the_kids_row_is_the_twenty_newest() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    for i in 0..25 {
        allow(&kid, &format!("tt{i:02}"), "movie", "T", i);
    }
    let home = HomeView::open(&kid);
    let ids = row_ids(&home, KIDS_ROW_ID);
    assert_eq!(ids.len(), 20);
    assert_eq!((ids[0].as_str(), ids[19].as_str()), ("tt24", "tt05"));
}

#[tokio::test]
async fn continue_watching_is_filtered_before_the_fetch_and_when_metas_land() {
    let fixture = Fixture::default();
    *fixture.delay.lock().unwrap() = Duration::from_millis(1000);
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", 1);
    allow(&kid, "tt3", "movie", "Cars", 2);
    played(&kid, "movie", "tt1", "tt1", 30, false);
    played(&kid, "movie", "tt3", "tt3", 20, false);
    // Progress for titles never on the list (an import, a sync): one under
    // way, one series finished (a Next Up candidate).
    played(&kid, "movie", "tt9", "tt9", 40, false);
    played(&kid, "series", "tt8", "tt8:1:1", 45, true);
    let home = HomeView::open(&kid);
    until("the row from the library", || {
        row_ids(&home, CONTINUE_ROW_ID) == ["tt1", "tt3"]
    })
    .await;
    // Taken off the list while its meta is on the way.
    remove(&kid, "tt1");
    let artwork = || {
        home.snapshot()
            .rows
            .into_iter()
            .flat_map(|r| r.tiles)
            .any(|t| t.id == "tt3" && t.background.is_some())
    };
    until("the metas landed", artwork).await;
    assert_eq!(row_ids(&home, CONTINUE_ROW_ID), ["tt3"]);
    assert!(fixture.asked("/meta/movie/tt9").is_empty(), "never asked");
    assert!(fixture.asked("/meta/series/tt8").is_empty(), "never asked");
}

#[tokio::test]
async fn a_kids_home_follows_the_list() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", 1);
    allow(&kid, "tt2", "movie", "Cars", 2);
    played(&kid, "movie", "tt5", "tt5", 9, false);
    let home = HomeView::open(&kid);
    assert_eq!(row_ids(&home, KIDS_ROW_ID), ["tt2", "tt1"]);
    allow(&kid, "tt4", "movie", "Soul", 3);
    until("the new title", || {
        row_ids(&home, KIDS_ROW_ID) == ["tt4", "tt2", "tt1"]
    })
    .await;
    remove(&kid, "tt2");
    until("the removed title gone", || {
        row_ids(&home, KIDS_ROW_ID) == ["tt4", "tt1"]
    })
    .await;
    // A title played before it was allowed comes back to Continue Watching.
    assert!(row_ids(&home, CONTINUE_ROW_ID).is_empty());
    allow(&kid, "tt5", "movie", "Coco", 4);
    until("its progress shown", || {
        row_ids(&home, CONTINUE_ROW_ID) == ["tt5"]
    })
    .await;
}

fn search_ids(view: &SearchView) -> Vec<String> {
    view.snapshot()
        .rows
        .into_iter()
        .flat_map(|r| r.tiles)
        .map(|t| t.id)
        .collect()
}

#[tokio::test]
async fn a_kids_search_asks_no_addon_and_matches_every_name() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    let up = AllowedRow {
        item_id: "tt1".into(),
        kind: "movie".into(),
        name: "Up".into(),
        names: vec!["Yukarı Bak".into()],
        poster: None,
        allowed: true,
        added_ms: 1,
    };
    kid.library()
        .write_allowed(kid.profile(), &up, &kid.hlc())
        .unwrap();
    allow(&kid, "tt2", "movie", "Up in the Air", 2);
    allow(&kid, "tt3", "movie", "Cars", 3);
    let search = SearchView::open(&kid);
    search.set_query("up");
    until("the matches", || search_ids(&search).len() == 2).await;
    assert_eq!(search_ids(&search), ["tt1", "tt2"], "the exact name first");
    search.set_query("bak");
    until("a match by another name", || search_ids(&search) == ["tt1"]).await;
    search.set_query("zzz");
    until("no match", || {
        search.snapshot().query == "zzz" && search.snapshot().rows.is_empty()
    })
    .await;
    assert!(
        fixture.asked("/catalog/").is_empty(),
        "{:?}",
        fixture.asked("")
    );
    // The list changes under a query on screen: it runs again.
    search.set_query("cars");
    until("Cars", || search_ids(&search) == ["tt3"]).await;
    remove(&kid, "tt3");
    until("Cars gone", || search_ids(&search).is_empty()).await;
}

fn library_ids(view: &LibraryView) -> Vec<String> {
    view.snapshot().items.into_iter().map(|t| t.id).collect()
}

#[tokio::test]
async fn a_kids_library_is_its_list() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", 1);
    allow(&kid, "tt2", "series", "Bluey", 2);
    played(&kid, "series", "tt2", "tt2:1:3", 50, false);
    // A title not on the list, played and in the library (an import, a
    // sync): the library row is written after the play's cache row.
    played(&kid, "movie", "tt9", "tt9", 60, false);
    let other = LibraryItemRow {
        item_id: "tt9".into(),
        kind: "movie".into(),
        name: "Not for kids".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    kid.library()
        .upsert_item(kid.profile(), &other, true, &kid.hlc())
        .unwrap();
    let held = kid.library().library(kid.profile()).unwrap();
    assert!(
        held.iter().any(|e| e.item.item_id == "tt9"),
        "in library_items"
    );
    let library = LibraryView::open(&kid);
    assert_eq!(library_ids(&library), ["tt2", "tt1"], "last watched first");
    let bluey = &library.snapshot().items[0];
    assert_eq!(
        bluey.release_info.as_deref(),
        Some("S1:E3 \u{b7} 1 min left")
    );
    allow(&kid, "tt3", "movie", "Soul", 3);
    until("the new title", || {
        library_ids(&library).contains(&"tt3".into())
    })
    .await;
    remove(&kid, "tt1");
    until("the removed one gone", || {
        !library_ids(&library).contains(&"tt1".into())
    })
    .await;
}

#[tokio::test]
async fn a_kid_reads_the_owners_addons_without_adult_ones() {
    let fixture = Fixture::default();
    let (owner, kid) = kid(&fixture).await;
    let normal = fixture.addon("normal", false).await;
    let adult = fixture.addon("adult", true).await;
    owner
        .library()
        .set_addons(OWNER, &[normal, adult], &owner.hlc())
        .unwrap();
    // The kid's own list is not used.
    let own = fixture.addon("own", false).await;
    kid.library()
        .set_addons(kid.profile(), &[own], &kid.hlc())
        .unwrap();
    assert_eq!(kid.library().addons(kid.profile()).unwrap().len(), 1);
    let names: Vec<String> = kid
        .installed_addons()
        .unwrap()
        .into_iter()
        .map(|a| a.manifest.name)
        .collect();
    assert_eq!(names, ["normal"]);
    let names: Vec<String> = owner
        .installed_addons()
        .unwrap()
        .into_iter()
        .map(|a| a.manifest.name)
        .collect();
    assert_eq!(names, ["normal", "adult"], "the owner keeps both");
}

/// Many newer plays of titles off the list (an owner's prune) never crowd
/// the allowed ones out of Continue Watching or Next Up.
#[tokio::test]
async fn continue_watching_filters_before_it_takes_twenty() {
    let fixture = Fixture::default();
    fixture.metas.lock().unwrap().insert(
        "/meta/series/tt2.json".into(),
        json!({"meta": {"id": "tt2", "type": "series", "name": "Bluey", "videos": [
            {"id": "tt2:1:1", "title": "One", "season": 1, "episode": 1,
             "released": "2020-01-01T00:00:00Z"},
            {"id": "tt2:1:2", "title": "Two", "season": 1, "episode": 2,
             "released": "2020-01-08T00:00:00Z"}]}}),
    );
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", 1);
    allow(&kid, "tt2", "series", "Bluey", 2);
    played(&kid, "movie", "tt1", "tt1", 10, false);
    played(&kid, "series", "tt2", "tt2:1:1", 11, true);
    for i in 0..25 {
        let id = format!("tt1{i:02}");
        played(&kid, "movie", &id, &id, 100 + i, false);
        let show = format!("tt3{i:02}");
        played(&kid, "series", &show, &format!("{show}:1:1"), 200 + i, true);
    }
    let home = HomeView::open(&kid);
    until("the allowed play and its Next Up", || {
        let tiles: Vec<(String, Option<String>)> = home
            .snapshot()
            .rows
            .into_iter()
            .filter(|r| r.row_id == CONTINUE_ROW_ID)
            .flat_map(|r| r.tiles)
            .map(|t| (t.id, t.video_id))
            .collect();
        tiles.contains(&("tt1".into(), Some("tt1".into())))
            && tiles.contains(&("tt2".into(), Some("tt2:1:2".into())))
    })
    .await;
}

/// A progress row that pairs an allowed id with another title's video
/// (an import) gives no tile.
#[tokio::test]
async fn a_progress_row_with_another_titles_video_shows_no_tile() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "series", "Show", 1);
    allow(&kid, "tt2", "movie", "Up", 2);
    played(&kid, "series", "tt1", "tt10:1:1", 20, false);
    played(&kid, "movie", "tt2", "tt2", 10, false);
    let home = HomeView::open(&kid);
    until("the row", || row_ids(&home, CONTINUE_ROW_ID) == ["tt2"]).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(row_ids(&home, CONTINUE_ROW_ID), ["tt2"]);
}

/// A profile whose access cannot be read reads its own addons, never the
/// owner's (whose addresses can hold keys).
#[tokio::test]
async fn an_unreadable_profile_reads_its_own_addons() {
    let fixture = Fixture::default();
    let dir = std::env::temp_dir().join(format!("mq-kids-addons-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let owner = Core::open(mq_runtime::CoreConfig {
        data_dir: dir.clone(),
        profile_id: OWNER.into(),
        device_id: 0,
    })
    .unwrap();
    let owners = fixture.addon("owners", false).await;
    owner
        .library()
        .set_addons(OWNER, &[owners], &owner.hlc())
        .unwrap();
    let p = owner.create_profile("P", "owl").unwrap();
    let own = fixture.addon("own", false).await;
    owner
        .library()
        .set_addons(&p, &[own], &owner.hlc())
        .unwrap();
    rusqlite::Connection::open(dir.join("library.sqlite"))
        .unwrap()
        .execute(
            "UPDATE profile_access SET kids = 'unreadable' WHERE profile_id = ?1",
            [&p],
        )
        .unwrap();
    let names: Vec<String> = owner
        .for_profile(&p)
        .installed_addons()
        .unwrap()
        .into_iter()
        .map(|a| a.manifest.name)
        .collect();
    assert_eq!(names, ["own"]);
    drop(owner);
    let _ = std::fs::remove_dir_all(dir);
}

/// Each tile's id, name and own poster: for an IMDb title the poster is
/// RPDB's cover, so its own is the fallback.
fn tile_faces(view: &HomeView, row: &str) -> Vec<(String, String, Option<String>)> {
    view.snapshot()
        .rows
        .into_iter()
        .filter(|r| r.row_id == row)
        .flat_map(|r| r.tiles)
        .map(|t| (t.id, t.name, t.poster_fallback.or(t.poster)))
        .collect()
}

fn allow_with_poster(kid: &Core, id: &str, name: &str, poster: &str, added_ms: i64) {
    let row = AllowedRow {
        item_id: id.into(),
        kind: "movie".into(),
        name: name.into(),
        names: vec![],
        poster: Some(poster.into()),
        allowed: true,
        added_ms,
    };
    kid.library()
        .write_allowed(kid.profile(), &row, &kid.hlc())
        .unwrap();
}

/// An imported library row pairing an allowed id with another title's name
/// and poster: the kid's tile shows the list's own name and poster, before
/// and after the meta lands.
#[tokio::test]
async fn a_kids_tile_shows_its_lists_name_and_poster() {
    let fixture = Fixture::default();
    let (_owner, kid) = kid(&fixture).await;
    allow_with_poster(&kid, "tt1", "Up", "https://img.example/up.jpg", 1);
    played(&kid, "movie", "tt1", "tt1", 10, false);
    let foreign = LibraryItemRow {
        item_id: "tt1".into(),
        kind: "movie".into(),
        name: "Not for kids".into(),
        poster: Some("https://img.example/foreign.jpg".into()),
        poster_shape: None,
        added_ms: 1,
    };
    kid.library()
        .upsert_item(kid.profile(), &foreign, false, &kid.hlc())
        .unwrap();
    let home = HomeView::open(&kid);
    let up = (
        "tt1".to_owned(),
        "Up".to_owned(),
        Some("https://img.example/up.jpg".to_owned()),
    );
    until("the quick tile", || {
        tile_faces(&home, CONTINUE_ROW_ID) == [up.clone()]
    })
    .await;
    until("the meta's artwork", || {
        home.snapshot()
            .rows
            .into_iter()
            .flat_map(|r| r.tiles)
            .any(|t| t.background.is_some())
    })
    .await;
    assert_eq!(tile_faces(&home, CONTINUE_ROW_ID), [up]);
}

/// Taken off the list while metas are still on their way: the tile, its
/// name and its poster leave at once, not when the metas land.
#[tokio::test]
async fn a_removal_while_metas_are_pending_leaves_at_once() {
    let fixture = Fixture::default();
    *fixture.delay.lock().unwrap() = Duration::from_secs(3);
    let (_owner, kid) = kid(&fixture).await;
    allow_with_poster(&kid, "tt1", "Up", "https://img.example/up.jpg", 1);
    allow_with_poster(&kid, "tt3", "Cars", "https://img.example/cars.jpg", 2);
    played(&kid, "movie", "tt1", "tt1", 30, false);
    played(&kid, "movie", "tt3", "tt3", 20, false);
    let home = HomeView::open(&kid);
    until("the quick row", || {
        row_ids(&home, CONTINUE_ROW_ID) == ["tt1", "tt3"]
    })
    .await;
    remove(&kid, "tt1");
    let gone = || {
        tile_faces(&home, CONTINUE_ROW_ID)
            .iter()
            .all(|(id, name, poster)| {
                id != "tt1"
                    && name != "Up"
                    && poster.as_deref() != Some("https://img.example/up.jpg")
            })
    };
    // Within a second; the metas take three.
    for _ in 0..40 {
        if gone() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        gone(),
        "still shown: {:?}",
        tile_faces(&home, CONTINUE_ROW_ID)
    );
    assert_eq!(row_ids(&home, CONTINUE_ROW_ID), ["tt3"]);
}

/// A kid's Search shows no recent searches: an old query can name a title
/// since taken off the list. An open profile's are as before.
#[tokio::test]
async fn a_kids_search_shows_no_recent_names() {
    let fixture = Fixture::default();
    let (owner, kid) = kid(&fixture).await;
    allow(&kid, "tt1", "movie", "Up", 1);
    let search = SearchView::open(&kid);
    search.set_query("up");
    until("the match", || search_ids(&search) == ["tt1"]).await;
    search.remember(None).unwrap();
    remove(&kid, "tt1");
    search.set_query("");
    assert!(
        search.snapshot().recent.is_empty(),
        "{:?}",
        search.snapshot().recent
    );
    assert!(SearchView::open(&kid).snapshot().recent.is_empty());
    let owners = SearchView::open(&owner);
    owners.set_query("up");
    until("the owner's query", || owners.snapshot().query == "up").await;
    owners.remember(None).unwrap();
    owners.set_query("");
    assert_eq!(owners.snapshot().recent, ["up"]);
}

/// A kids profile's own tiles carry RPDB's cover, drawn with the key of the
/// owner's addon (the list a kids profile reads), on Home, in Search and in
/// its library, as every other tile does.
#[tokio::test]
async fn a_kids_tiles_show_the_rated_cover() {
    let fixture = Fixture::default();
    let (owner, kid) = kid(&fixture).await;
    let mut addons = owner.library().addons(OWNER).unwrap();
    addons.push(rt_addon());
    owner
        .library()
        .set_addons(OWNER, &addons, &owner.hlc())
        .unwrap();
    allow(&kid, "tt1", "movie", "Up", 1);
    let rated = |tiles: &[mq_runtime::views::home::Tile]| {
        tiles.iter().any(|t| {
            t.id == "tt1"
                && t.poster.as_deref()
                    == Some("https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt1.jpg?fallback=true")
        })
    };
    let home = HomeView::open(&kid);
    until("the kids row has RPDB's cover", || {
        home.snapshot()
            .rows
            .iter()
            .any(|r| r.row_id == KIDS_ROW_ID && rated(&r.tiles))
    })
    .await;
    let search = SearchView::open(&kid);
    search.set_query("up");
    until("the kids search has RPDB's cover", || {
        search.snapshot().rows.iter().any(|r| rated(&r.tiles))
    })
    .await;
    let library = LibraryView::open(&kid);
    until("the kids library has RPDB's cover", || {
        rated(&library.snapshot().items)
    })
    .await;
}
