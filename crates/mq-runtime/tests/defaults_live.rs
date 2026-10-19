//! The default addons against the real ones (plans/m13-default-addons.md 10):
//! a fresh core installs the nine, then reads each live manifest; each must
//! answer with the bundled id and need no setup. Prints one JSON line per
//! addon for `bench/m13/defaults-live.jsonl`.
//!
//! `cargo test -p mq-runtime --test defaults_live -- --ignored --nocapture`

use mq_library::OWNER;
use mq_runtime::Core;
use mq_runtime::defaults;
use std::time::Instant;

#[tokio::test]
#[ignore = "network: the nine default addons"]
async fn the_nine_defaults_install_on_a_fresh_core_and_each_live_manifest_answers() {
    let core = Core::temporary(OWNER).unwrap();
    assert!(core.addons_fresh());
    core.install_defaults(&[]).unwrap();
    let rows = core.library().addons(OWNER).unwrap();
    assert_eq!(rows.len(), 9);
    let mut failed = Vec::new();
    for d in defaults::list() {
        let started = Instant::now();
        let answer = core.addons().fetch_manifest(&d.url).await;
        let ms = started.elapsed().as_millis();
        let (ok, id, version, setup) = match &answer {
            Ok(a) => (
                a.manifest.id == d.manifest.id && !a.manifest.behavior_hints.configuration_required,
                a.manifest.id.clone(),
                a.manifest.version.to_string(),
                a.manifest.behavior_hints.configuration_required,
            ),
            Err(_) => (false, String::new(), String::new(), false),
        };
        println!(
            "{}",
            serde_json::json!({
                "date": "2026-10-19", "kind": "live", "addon": d.key, "ok": ok,
                "manifestId": id, "bundledId": d.manifest.id, "version": version,
                "bundledVersion": d.manifest.version.to_string(),
                "configurationRequired": setup, "ms": ms,
                "error": answer.as_ref().err().map(|e| e.to_string().chars().take(80).collect::<String>()),
            })
        );
        if !ok {
            failed.push(d.key.clone());
        }
    }
    assert!(
        failed.is_empty(),
        "these did not answer as bundled: {failed:?}"
    );
}
