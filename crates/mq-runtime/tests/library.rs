use mq_library::{LibraryItemRow, ProgressRow};
use mq_runtime::{Core, LibrarySort, LibraryView};
use std::time::Duration;

fn add(core: &Core, id: &str, name: &str, kind: &str, added: i64) {
    let row = LibraryItemRow {
        item_id: id.into(),
        kind: kind.into(),
        name: name.into(),
        poster: None,
        poster_shape: None,
        added_ms: added,
    };
    core.library()
        .upsert_item(core.profile(), &row, true, &core.hlc())
        .unwrap();
}

fn names(view: &LibraryView) -> Vec<String> {
    view.snapshot()
        .items
        .iter()
        .map(|t| t.name.clone())
        .collect()
}

#[tokio::test]
async fn filters_sorts_and_follows_watching() {
    let core = Core::temporary("p").unwrap();
    add(&core, "tt1", "alpha", "movie", 1);
    add(&core, "tt2", "Bravo", "series", 2);
    add(&core, "tt3", "Charlie", "movie", 3);
    let view = LibraryView::open(&core);
    assert_eq!(names(&view), ["Charlie", "Bravo", "alpha"]);
    let kinds: Vec<_> = view
        .snapshot()
        .kinds
        .iter()
        .map(|k| (k.label.clone(), k.count))
        .collect();
    assert_eq!(
        kinds,
        [("Movies".to_string(), 2), ("Series".to_string(), 1)]
    );

    view.set_sort(LibrarySort::Name);
    assert_eq!(names(&view), ["alpha", "Bravo", "Charlie"]);
    view.set_filter(Some("movie".into()));
    assert_eq!(names(&view), ["alpha", "Charlie"]);

    // Watching moves a title to the front of Recent.
    view.set_filter(None);
    view.set_sort(LibrarySort::Recent);
    core.library()
        .set_progress(
            core.profile(),
            ProgressRow {
                video_id: "tt1",
                item_id: "tt1",
                position_ms: 50,
                duration_ms: 100,
                updated_ms: 10,
            },
            &core.hlc(),
        )
        .unwrap();
    for _ in 0..100 {
        if names(&view)[0] == "alpha" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(names(&view), ["alpha", "Charlie", "Bravo"]);
    assert_eq!(view.snapshot().items[0].progress, Some(0.5));
}

fn play(core: &Core, item: &str, video: &str, position_ms: i64, duration_ms: i64) {
    let row = ProgressRow {
        video_id: video,
        item_id: item,
        position_ms,
        duration_ms,
        updated_ms: 10,
    };
    core.library()
        .set_progress(core.profile(), row, &core.hlc())
        .unwrap();
}

/// Library tiles say where the user is, in Continue Watching's words.
#[tokio::test]
async fn tiles_say_where_the_user_is() {
    let core = Core::temporary("p").unwrap();
    add(&core, "tt1", "Partial movie", "movie", 1);
    add(&core, "tt2", "Watched movie", "movie", 2);
    add(&core, "tt3", "Partial series", "series", 3);
    add(&core, "tt4", "Finished episode", "series", 4);
    add(&core, "tt5", "Never played", "movie", 5);
    play(&core, "tt1", "tt1", 600_000, 6_000_000);
    core.library()
        .set_watched(core.profile(), "tt2", "tt2", true, &core.hlc())
        .unwrap();
    play(&core, "tt3", "tt3:2:3", 120_000, 2_400_000);
    play(&core, "tt4", "tt4:1:8", 2_390_000, 2_400_000);
    let view = LibraryView::open(&core);
    view.set_sort(LibrarySort::Name);
    let captions: Vec<_> = view
        .snapshot()
        .items
        .iter()
        .map(|t| (t.name.clone(), t.release_info.clone()))
        .collect();
    let expect = |name: &str, caption: Option<&str>| (name.to_owned(), caption.map(str::to_owned));
    assert_eq!(
        captions,
        [
            expect("Finished episode", Some("S1:E8 \u{b7} Watched")),
            expect("Never played", None),
            expect("Partial movie", Some("90 min left")),
            expect("Partial series", Some("S2:E3 \u{b7} 38 min left")),
            expect("Watched movie", Some("Watched")),
        ]
    );
}

/// Types Marquee does not know never show as is: they share one "Other"
/// chip, and its filter keeps them all.
#[tokio::test]
async fn unknown_types_share_one_other_chip() {
    let core = Core::temporary("p").unwrap();
    add(&core, "tt1", "alpha", "movie", 1);
    add(&core, "trakt:1", "Bravo", "trakt", 2);
    add(&core, "pod:1", "Charlie", "podcast", 3);
    let view = LibraryView::open(&core);
    let kinds: Vec<_> = view
        .snapshot()
        .kinds
        .iter()
        .map(|k| (k.kind.clone(), k.label.clone(), k.count))
        .collect();
    assert_eq!(
        kinds,
        [
            ("other".to_string(), "Other".to_string(), 2),
            ("movie".into(), "Movies".into(), 1)
        ]
    );
    view.set_sort(LibrarySort::Name);
    view.set_filter(Some("other".into()));
    assert_eq!(names(&view), ["Bravo", "Charlie"]);
}

/// A library title with an IMDb id shows RPDB's cover with the ratings
/// band, its own poster kept as the fallback; any other id keeps its own.
#[tokio::test]
async fn library_covers_carry_the_ratings_band() {
    let core = Core::temporary("p").unwrap();
    for (id, kind) in [("tt0111161", "movie"), ("kitsu:1", "series")] {
        let row = LibraryItemRow {
            item_id: id.into(),
            kind: kind.into(),
            name: id.into(),
            poster: Some(format!("https://img.example/{id}.jpg")),
            poster_shape: None,
            added_ms: 1,
        };
        core.library()
            .upsert_item(core.profile(), &row, true, &core.hlc())
            .unwrap();
    }
    let view = LibraryView::open(&core);
    let covers: Vec<(String, Option<String>, Option<String>)> = view
        .snapshot()
        .items
        .into_iter()
        .map(|t| (t.id, t.poster, t.poster_fallback))
        .collect();
    let rpdb = "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0111161.jpg?fallback=true";
    assert!(
        covers.contains(&(
            "tt0111161".into(),
            Some(rpdb.into()),
            Some("https://img.example/tt0111161.jpg".into())
        )),
        "{covers:?}"
    );
    assert!(
        covers.contains(&(
            "kitsu:1".into(),
            Some("https://img.example/kitsu:1.jpg".into()),
            None
        )),
        "{covers:?}"
    );
}
