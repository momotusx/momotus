//! The default addons (plans/m13-default-addons.md): installed in one write
//! from bundled manifests, switched-off ones off, a profile with no list
//! stamped with the zero hlc, a present default only switched, a configured
//! copy taking an untouched default's row, and `fresh`.

use mq_library::{AddonRow, Hlc, OWNER, SyncCursor};
use mq_runtime::Core;
use mq_runtime::defaults::{self, DefaultChoice, DefaultInstalled};

fn urls(core: &Core) -> Vec<(String, bool)> {
    core.library()
        .addons(core.profile())
        .unwrap()
        .into_iter()
        .map(|r| (r.transport_url, r.enabled))
        .collect()
}

fn choice(key: &str, on: bool) -> DefaultChoice {
    serde_json::from_value(serde_json::json!({ "key": key, "on": on })).unwrap()
}

fn stamp(core: &Core) -> Hlc {
    Hlc::decode(
        &core
            .library()
            .addon_list_hlc(core.profile())
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn a_fresh_profile_gets_the_nine_in_order_with_its_switches() {
    let core = Core::temporary(OWNER).unwrap();
    assert!(
        core.addons_fresh(),
        "a new device and profile start at the defaults step"
    );
    assert!(
        core.default_addons()
            .unwrap()
            .addons
            .iter()
            .all(|d| d.installed == DefaultInstalled::Absent)
    );
    core.install_defaults(&[choice("torrentio", false), choice("trakt", false)])
        .unwrap();
    let list = defaults::list();
    let expected: Vec<(String, bool)> = list
        .iter()
        .map(|d| (d.url.clone(), d.key != "torrentio" && d.key != "trakt"))
        .collect();
    assert_eq!(urls(&core), expected);
    // The bundled manifests, as an install would store them.
    for (row, d) in core.library().addons(OWNER).unwrap().iter().zip(list) {
        assert_eq!(
            row.manifest_json,
            serde_json::to_string(&d.manifest).unwrap()
        );
    }
    // A switch turned off is the user's choice: an edit, stamped with the clock.
    assert!(stamp(&core).wall_ms > 0);
    assert!(
        !core.addons_fresh(),
        "a profile with a list no longer starts there"
    );
    let shown = core.default_addons().unwrap();
    assert_eq!(shown.version, defaults::LIST_VERSION);
    let state = |key: &str| {
        shown
            .addons
            .iter()
            .find(|d| d.key == key)
            .unwrap()
            .installed
    };
    assert_eq!(state("torrentio"), DefaultInstalled::Off);
    assert_eq!(state("cinemeta"), DefaultInstalled::On);
}

#[tokio::test]
async fn untouched_defaults_take_the_zero_hlc_and_never_outrank_another_devices_list() {
    let c = Core::temporary(OWNER).unwrap();
    c.install_defaults(&[]).unwrap();
    let s = stamp(&c);
    assert_eq!(
        s,
        Hlc::decode(&c.zero_hlc()).unwrap(),
        "this device's zero hlc"
    );
    // Another device's list, written at any real time, replaces them on a pull.
    let a = Core::temporary(OWNER).unwrap();
    a.library()
        .set_addons(
            OWNER,
            &[],
            &Hlc {
                wall_ms: 1,
                counter: 0,
                device: 2,
            }
            .encode(),
        )
        .unwrap();
    let from_a = a
        .library()
        .changed_since(&SyncCursor::default(), 1_000)
        .unwrap();
    // ... and C's untouched list pushed to that device changes nothing there.
    let from_c = c
        .library()
        .changed_since(&SyncCursor::default(), 1_000)
        .unwrap();
    a.library().apply(&from_c).unwrap();
    assert!(urls(&a).is_empty(), "the real list stays");
    c.library().apply(&from_a).unwrap();
    assert!(urls(&c).is_empty(), "the account's list wins");
}

#[tokio::test]
async fn a_retried_continue_changes_nothing_and_pushes_nothing() {
    let core = Core::temporary(OWNER).unwrap();
    core.install_defaults(&[]).unwrap();
    let first = stamp(&core);
    let queued = core.library().outbox(1_000).unwrap().records.len();
    // The answer was lost: the app sends the same Continue again.
    core.install_defaults(&[]).unwrap();
    assert_eq!(stamp(&core), first, "untouched defaults stay untouched");
    assert_eq!(core.library().outbox(1_000).unwrap().records.len(), queued);
    assert_eq!(urls(&core).len(), 9, "nothing added twice");
    // A switch changed later is an edit.
    core.install_defaults(&[choice("subsense", false)]).unwrap();
    assert!(stamp(&core).wall_ms > 0);
    assert_eq!(urls(&core).iter().filter(|(_, on)| !on).count(), 1);
}

#[tokio::test]
async fn a_default_already_there_by_manifest_id_is_only_switched() {
    let core = Core::temporary(OWNER).unwrap();
    // A Torrentio a Stremio user brought, with another config, first in the list.
    let torrentio = defaults::list()
        .iter()
        .find(|d| d.key == "torrentio")
        .unwrap();
    let theirs = "https://torrentio.strem.fun/sort=seeders/manifest.json";
    core.library()
        .set_addons(
            OWNER,
            &[AddonRow {
                transport_url: theirs.into(),
                manifest_json: serde_json::to_string(&torrentio.manifest).unwrap(),
                position: 0,
                enabled: true,
                allow_private: false,
            }],
            &core.hlc(),
        )
        .unwrap();
    core.install_defaults(&[choice("torrentio", false)])
        .unwrap();
    let rows = urls(&core);
    assert_eq!(rows.len(), 9, "Torrentio is not added a second time");
    assert_eq!(
        rows[0],
        (theirs.to_owned(), false),
        "theirs kept first, switched off"
    );
    // Imported first, defaults after, with the clock: the list existed.
    assert!(stamp(&core).wall_ms > 0);
}

#[tokio::test]
async fn unknown_keys_and_kids_profiles_are_refused() {
    let core = Core::temporary(OWNER).unwrap();
    assert!(core.install_defaults(&[choice("nope", true)]).is_err());
    assert!(urls(&core).is_empty(), "nothing written");
    core.set_pin(OWNER, Some("7351"), None).await.unwrap();
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let kids = core.for_profile(&kid);
    assert!(
        !kids.addons_fresh(),
        "a kids profile reads the owner's list"
    );
    assert!(kids.install_defaults(&[]).is_err());
}

#[tokio::test]
async fn a_device_in_a_sync_account_never_starts_at_the_defaults() {
    let core = Core::temporary(OWNER).unwrap();
    assert!(core.addons_fresh());
    core.library()
        .add_roster_entry("genesis", 0, b"{}", &[0; 64])
        .unwrap();
    assert!(
        !core.addons_fresh(),
        "a joined device gets the account's list"
    );
}

/// A device that existed before default addons (its library had the owner)
/// never starts a profile at the defaults step, list or not: those profiles
/// get the card for the new addons instead.
#[tokio::test]
async fn profiles_of_a_device_from_before_defaults_are_not_fresh() {
    let dir = std::env::temp_dir().join(format!("mq-defaults-legacy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    {
        // As an older version left it: the owner, no addon list, no marker.
        let lib = mq_library::Library::open(dir.join("library.sqlite")).unwrap();
        lib.ensure_profile(OWNER, "Me", &Hlc::zero(5).encode())
            .unwrap();
    }
    let core = Core::open(mq_runtime::CoreConfig {
        data_dir: dir.clone(),
        profile_id: OWNER.into(),
        device_id: 5,
    })
    .unwrap();
    assert!(!core.addons_fresh());
    drop(core);
    let _ = std::fs::remove_dir_all(&dir);
}
