use mq_runtime::views::settings::SubtitleSize;
use mq_runtime::{Core, SettingsView};
use mq_streams::QualityPref;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn patches_merge_per_field_and_the_view_follows() {
    let core = Core::temporary("p").unwrap();
    let view = SettingsView::open(&core);
    assert!(view.snapshot().autoplay_next, "default on");

    core.set_prefs(&json!({"streams": {"languages": ["tr", "en"], "maxHeight": 1080}}))
        .unwrap();
    // A later nested patch keeps the fields it does not name.
    let prefs = core
        .set_prefs(&json!({"streams": {"quality": "dataSaver"}, "autoplayNext": false}))
        .unwrap();
    assert_eq!(prefs.streams.languages, ["tr", "en"]);
    assert_eq!(prefs.streams.quality, QualityPref::DataSaver);
    // The old `quality` is stored as the fields it maps to
    // (plans/m13-stream-choice.md 4.1): Data saver is 720p and 2 GB.
    assert_eq!(prefs.streams.max_height, Some(720));
    assert_eq!(prefs.streams.size_limit_bytes, Some(2 << 30));
    assert_eq!(core.prefs(), prefs, "stored and read back");

    for _ in 0..100 {
        if !view.snapshot().autoplay_next {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(view.snapshot(), prefs);

    assert!(core.set_prefs(&json!({"nope": 1})).is_err());
    assert!(core.set_prefs(&json!({"autoplayNext": "yes"})).is_err());
    assert_eq!(core.prefs(), prefs, "rejected patches change nothing");
}

/// Journey reviews 2026-10-20: the subtitle size is a synced preference,
/// medium until set; a size this version does not know is refused.
#[tokio::test]
async fn the_subtitle_size_is_a_preference() {
    let core = Core::temporary("p").unwrap();
    assert_eq!(core.prefs().subtitle_size, SubtitleSize::Medium);
    let prefs = core
        .set_prefs(&json!({"subtitleSize": "extraLarge"}))
        .unwrap();
    assert_eq!(prefs.subtitle_size, SubtitleSize::ExtraLarge);
    assert_eq!(core.prefs().subtitle_size, SubtitleSize::ExtraLarge);
    assert!(core.set_prefs(&json!({"subtitleSize": "huge"})).is_err());
    let stored = core.library().settings("p").unwrap();
    assert!(stored.contains(&("subtitleSize".into(), "\"extraLarge\"".into())));
}

#[tokio::test]
async fn nested_fields_are_separate_registers_and_bad_leaves_write_nothing() {
    let core = Core::temporary("p").unwrap();
    core.set_prefs(&json!({"streams": {"quality": "balanced", "maxHeight": 720}}))
        .unwrap();
    let keys: Vec<String> = core
        .library()
        .settings("p")
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert!(keys.contains(&"streams.quality".to_string()), "{keys:?}");
    assert!(keys.contains(&"streams.maxHeight".to_string()), "{keys:?}");

    let before = core.prefs();
    assert!(
        core.set_prefs(&json!({"streams": {"quality": "best", "bogus": 1}}))
            .is_err()
    );
    assert_eq!(core.prefs(), before, "nothing of the bad patch was stored");

    // A value from a newer app version this one cannot read is skipped alone.
    core.library()
        .set_setting("p", "streams.quality", "\"ultra\"", &core.hlc())
        .unwrap();
    let prefs = core.prefs();
    assert_eq!(prefs.streams.max_height, Some(720));
}
