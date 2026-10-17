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
    assert_eq!(prefs.streams.max_height, Some(1080));
    assert_eq!(prefs.streams.quality, QualityPref::DataSaver);
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
