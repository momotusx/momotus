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

/// Every default named, as the defaults step sends them: on unless listed in `off`.
fn switches(off: &[&str]) -> Vec<DefaultChoice> {
    defaults::list()
        .iter()
        .map(|d| choice(&d.key, !off.contains(&d.key.as_str())))
        .collect()
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
            .await
            .unwrap()
            .addons
            .iter()
            .all(|d| d.installed == DefaultInstalled::Absent)
    );
    core.install_defaults(&switches(&["torrentio", "trakt"]))
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
    // A switch turned off: still below any list a person built (wall time
    // zero), above untouched defaults (counter 1).
    let device = Hlc::decode(&core.zero_hlc()).unwrap().device;
    assert_eq!(
        stamp(&core),
        Hlc {
            wall_ms: 0,
            counter: 1,
            device
        }
    );
    assert_eq!(
        core.prefs().defaults_seen,
        defaults::LIST_VERSION,
        "answered: no card for this list"
    );
    assert!(
        !core.addons_fresh(),
        "a profile with a list no longer starts there"
    );
    let shown = core.default_addons().await.unwrap();
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
    c.install_defaults(&switches(&[])).unwrap();
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
    core.install_defaults(&switches(&[])).unwrap();
    let first = stamp(&core);
    let queued = core.library().outbox(1_000).unwrap().records.len();
    // The answer was lost: the app sends the same Continue again.
    core.install_defaults(&switches(&[])).unwrap();
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
    core.install_defaults(&switches(&["torrentio"])).unwrap();
    let rows = urls(&core);
    assert_eq!(rows.len(), 9, "Torrentio is not added a second time");
    assert_eq!(
        rows[0],
        (theirs.to_owned(), false),
        "theirs kept first, switched off"
    );
    // Imported first, defaults after, with the clock: the list existed.
    assert!(stamp(&core).wall_ms > 0);
    // Review names only the missing ones: a present default left out keeps its switch.
    core.install_defaults(&[choice("kitsu", true)]).unwrap();
    assert_eq!(
        urls(&core)[0],
        (theirs.to_owned(), false),
        "left out, left off"
    );
}

/// A call adds only what it names: a Review or a resent Continue made before
/// a pull never brings back a default another device removed.
#[tokio::test]
async fn a_default_not_named_is_never_added() {
    let a = Core::temporary(OWNER).unwrap();
    a.install_defaults(&switches(&[])).unwrap();
    let torrentio = &defaults::list()[1];
    assert_eq!(torrentio.key, "torrentio");
    a.remove_addon(&torrentio.url).unwrap();
    let b = Core::temporary(OWNER).unwrap();
    b.install_defaults(&switches(&[])).unwrap();
    let from_a = a
        .library()
        .changed_since(&SyncCursor::default(), 1_000)
        .unwrap();
    b.library().apply(&from_a).unwrap();
    assert_eq!(urls(&b).len(), 8, "the account's list, without Torrentio");
    // The card's Review, built before the pull, named only Trakt.
    b.install_defaults(&[choice("trakt", true)]).unwrap();
    assert!(
        urls(&b).iter().all(|(url, _)| *url != torrentio.url),
        "Torrentio stays removed"
    );
    assert_eq!(urls(&b).len(), 8);
    // Nothing named, nothing written.
    let before = stamp(&b);
    b.install_defaults(&[]).unwrap();
    assert_eq!(stamp(&b), before);
}

/// The switch lands on the default's own row, found by its link when a
/// refresh changed its manifest id, never on whatever row is last.
#[tokio::test]
async fn a_default_whose_manifest_id_changed_is_switched_on_its_own_row() {
    let core = Core::temporary(OWNER).unwrap();
    let kitsu = defaults::list().iter().find(|d| d.key == "kitsu").unwrap();
    let mut renamed = kitsu.manifest.clone();
    renamed.id = "org.kitsu.renamed".into();
    let other = "https://other.example/manifest.json";
    let mut other_manifest = kitsu.manifest.clone();
    other_manifest.id = "example.other".into();
    let row = |url: &str, manifest: &mq_protocol::Manifest| AddonRow {
        transport_url: url.into(),
        manifest_json: serde_json::to_string(manifest).unwrap(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    core.library()
        .set_addons(
            OWNER,
            &[row(&kitsu.url, &renamed), row(other, &other_manifest)],
            &core.hlc(),
        )
        .unwrap();
    core.install_defaults(&[choice("kitsu", false)]).unwrap();
    let rows = core.library().addons(OWNER).unwrap();
    assert_eq!(rows.len(), 2, "nothing added");
    assert_eq!(
        (rows[0].transport_url.as_str(), rows[0].enabled),
        (kitsu.url.as_str(), false)
    );
    assert!(
        rows[0].manifest_json.contains("org.kitsu.renamed"),
        "the live manifest kept"
    );
    assert_eq!(
        (rows[1].transport_url.as_str(), rows[1].enabled),
        (other, true),
        "the other addon untouched"
    );
    let shown = core.default_addons().await.unwrap();
    let kitsu_state = shown
        .addons
        .iter()
        .find(|d| d.key == "kitsu")
        .unwrap()
        .installed;
    assert_eq!(kitsu_state, DefaultInstalled::Off);
}

/// Onboarding lists meet on a pull: a list a person built beats any of
/// them, and one with a switch turned off beats untouched ones, whichever
/// device pairs first.
#[tokio::test]
async fn onboarding_lists_lose_to_real_ones_and_a_choice_beats_untouched() {
    let changes = |c: &Core| {
        c.library()
            .changed_since(&SyncCursor::default(), 1_000)
            .unwrap()
    };
    let real = Core::temporary(OWNER).unwrap();
    real.install_defaults(&switches(&[])).unwrap();
    real.remove_addon(&defaults::list()[0].url).unwrap();
    let chose = Core::temporary(OWNER).unwrap();
    chose.install_defaults(&switches(&["torrentio"])).unwrap();
    let untouched = Core::temporary(OWNER).unwrap();
    untouched.install_defaults(&switches(&[])).unwrap();
    let off = |c: &Core| urls(c).iter().filter(|(_, on)| !on).count();

    // A real list against a choice, either way round.
    let (from_real, from_chose) = (changes(&real), changes(&chose));
    real.library().apply(&from_chose).unwrap();
    assert_eq!(
        (urls(&real).len(), off(&real)),
        (8, 0),
        "the real list stays"
    );
    chose.library().apply(&from_real).unwrap();
    assert_eq!(
        (urls(&chose).len(), off(&chose)),
        (8, 0),
        "the real list wins"
    );

    // A choice against untouched defaults, either way round.
    let chose = Core::temporary(OWNER).unwrap();
    chose.install_defaults(&switches(&["torrentio"])).unwrap();
    let (from_chose, from_untouched) = (changes(&chose), changes(&untouched));
    chose.library().apply(&from_untouched).unwrap();
    assert_eq!(off(&chose), 1, "the choice stays");
    untouched.library().apply(&from_chose).unwrap();
    assert_eq!(off(&untouched), 1, "the choice wins");
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
    assert!(kids.install_defaults(&switches(&[])).is_err());
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

/// A device that made its sync account has no list to receive: it keeps the
/// defaults step after its roster is stored (plans/accounts.md 3.3).
#[tokio::test]
async fn a_device_that_created_its_account_still_starts_at_the_defaults() {
    let core = Core::temporary(OWNER).unwrap();
    core.created_sync_account();
    core.library()
        .add_roster_entry("genesis", 0, b"{}", &[0; 64])
        .unwrap();
    assert!(core.addons_fresh(), "a genesis receives no list");
    core.joined_sync_account();
    assert!(
        !core.addons_fresh(),
        "joined elsewhere: that account's list"
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

/// A plain GET on loopback: status, content type and body.
async fn get(url: &str) -> (u16, String, Vec<u8>) {
    let url = url::Url::parse(url).unwrap();
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Write};
        let host = format!("{}:{}", url.host_str().unwrap(), url.port().unwrap());
        let mut socket = std::net::TcpStream::connect(&host).unwrap();
        write!(
            socket,
            "GET {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n",
            url.path()
        )
        .unwrap();
        let mut reply = Vec::new();
        socket.read_to_end(&mut reply).unwrap();
        let split = reply.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&reply[..split]).to_lowercase();
        let status = head[9..12].parse().unwrap();
        let mime = head
            .lines()
            .find_map(|l| l.strip_prefix("content-type: "))
            .unwrap_or_default()
            .to_owned();
        (status, mime, reply[split + 4..].to_vec())
    })
    .await
    .unwrap()
}

/// The logos come from the core's own loopback server, so the first-run
/// step asks no logo host: every default but Cinemeta (it publishes none)
/// has one, and an unknown key or another token is not found.
#[tokio::test]
async fn every_default_logo_is_served_on_loopback_and_nothing_else() {
    let core = Core::temporary(OWNER).unwrap();
    let shown = core.default_addons().await.unwrap();
    let mut served = Vec::new();
    for d in &shown.addons {
        let Some(url) = &d.logo else {
            assert_eq!(d.key, "cinemeta");
            continue;
        };
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        assert!(url.ends_with(&format!("/{}", d.key)), "{url}");
        let (status, mime, body) = get(url).await;
        assert_eq!(status, 200, "{url}");
        // Bitmaps only: the apps' image loaders do not draw SVG.
        let image = mime == "image/png" && body.starts_with(b"\x89PNG");
        assert!(image, "{}: {mime}, {} bytes", d.key, body.len());
        served.push(d.key.clone());
    }
    assert_eq!(served.len(), 8, "{served:?}");
    let torrentio = shown.addons[1].logo.clone().unwrap();
    let unknown = torrentio.replace("/torrentio", "/nope");
    assert_eq!(get(&unknown).await.0, 404);
    let token = torrentio.rsplit('/').nth(1).unwrap();
    let other = torrentio.replace(token, &"0".repeat(token.len()));
    assert_eq!(get(&other).await.0, 404);
}
