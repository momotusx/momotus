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
async fn a_fresh_profile_gets_the_ten_in_order_with_its_switches() {
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
    // Another device's list, written at any real time, replaces them on a
    // pull. (A list is its addons' registers: one that never held an addon
    // sends nothing, so the other device holds one it chose.)
    let a = Core::temporary(OWNER).unwrap();
    let own = "https://own.example/manifest.json";
    a.library()
        .set_addons(
            OWNER,
            &[AddonRow {
                transport_url: own.into(),
                manifest_json: "{}".into(),
                position: 0,
                enabled: true,
                allow_private: false,
            }],
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
    // C joins A's account: what it had only as untouched defaults goes, so
    // nothing of it reaches A (plans/accounts-social-sync.md 13).
    c.joined_sync_account();
    c.library().apply(&from_a).unwrap();
    let device = Hlc::decode(&c.zero_hlc()).unwrap().device;
    c.library().settle_join_defaults(device).unwrap();
    let from_c = c
        .library()
        .changed_since(&SyncCursor::default(), 1_000)
        .unwrap();
    a.library().apply(&from_c).unwrap();
    assert_eq!(urls(&a), [(own.to_owned(), true)], "the real list stays");
    c.library().apply(&from_a).unwrap();
    assert_eq!(
        urls(&c),
        [(own.to_owned(), true)],
        "the account's list wins"
    );
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
    assert_eq!(urls(&core).len(), 11, "nothing added twice");
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
    assert_eq!(rows.len(), 11, "Torrentio is not added a second time");
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
    assert_eq!(urls(&b).len(), 10, "the account's list, without Torrentio");
    // The card's Review, built before the pull, named only Trakt.
    b.install_defaults(&[choice("trakt", true)]).unwrap();
    assert!(
        urls(&b).iter().all(|(url, _)| *url != torrentio.url),
        "Torrentio stays removed"
    );
    assert_eq!(urls(&b).len(), 10);
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

/// A device that joins takes the account's list: its untouched defaults,
/// and choices it made only at onboarding, are not added; a removal its
/// person made before joining merges (plans/accounts-social-sync.md 13).
#[tokio::test]
async fn a_joining_device_takes_the_accounts_list_and_its_persons_edits_merge() {
    let changes = |c: &Core| {
        c.library()
            .changed_since(&SyncCursor::default(), 1_000)
            .unwrap()
    };
    let off = |c: &Core| urls(c).iter().filter(|(_, on)| !on).count();
    // The account's first device turned Torrentio off at onboarding.
    let account = Core::temporary(OWNER).unwrap();
    account.install_defaults(&switches(&["torrentio"])).unwrap();
    // The joining device kept the defaults, then its person removed one.
    let joining = Core::temporary(OWNER).unwrap();
    joining.install_defaults(&switches(&[])).unwrap();
    joining.remove_addon(&defaults::list()[0].url).unwrap();
    joining.joined_sync_account();
    // Its first pull, then the settle, before it sends anything.
    joining.library().apply(&changes(&account)).unwrap();
    let device = Hlc::decode(&joining.zero_hlc()).unwrap().device;
    joining.library().settle_join_defaults(device).unwrap();
    account.library().apply(&changes(&joining)).unwrap();
    for core in [&account, &joining] {
        assert_eq!(
            urls(core).len(),
            defaults::list().len() - 1,
            "the person's removal merges"
        );
        assert_eq!(off(core), 1, "the account's choice stays");
    }
    assert_eq!(urls(&account), urls(&joining));
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
    assert_eq!(served.len(), 10, "{served:?}");
    let torrentio = shown.addons[1].logo.clone().unwrap();
    let unknown = torrentio.replace("/torrentio", "/nope");
    assert_eq!(get(&unknown).await.0, 404);
    let token = torrentio.rsplit('/').nth(1).unwrap();
    let other = torrentio.replace(token, &"0".repeat(token.len()));
    assert_eq!(get(&other).await.0, 404);
}

/// A data directory of its own, opened as the apps open it (`Core::open`,
/// which gives the new defaults), as often as a test starts the app.
struct Device {
    dir: std::path::PathBuf,
}

impl Device {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mq-defaults-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn start(&self) -> Core {
        self.start_as(9)
    }

    /// Starts as device `device_id`: two devices of one sync account have their own.
    fn start_as(&self, device_id: u64) -> Core {
        Core::open(mq_runtime::CoreConfig {
            data_dir: self.dir.clone(),
            profile_id: OWNER.into(),
            device_id,
        })
        .unwrap()
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The owner's list as an older onboarding left it: these defaults, on
/// unless named in `off`, with their bundled manifests.
fn old_list(core: &Core, keys: &[&str], off: &[&str]) {
    list_of(core, OWNER, keys, off);
}

fn list_of(core: &Core, profile: &str, keys: &[&str], off: &[&str]) {
    let rows: Vec<AddonRow> = keys
        .iter()
        .enumerate()
        .map(|(position, key)| {
            let d = defaults::list().iter().find(|d| d.key == *key).unwrap();
            AddonRow {
                transport_url: d.url.clone(),
                manifest_json: serde_json::to_string(&d.manifest).unwrap(),
                position: position as i64,
                enabled: !off.contains(key),
                allow_private: false,
            }
        })
        .collect();
    core.library()
        .set_addons(profile, &rows, &core.hlc())
        .unwrap();
}

fn url(key: &str) -> String {
    defaults::list()
        .iter()
        .find(|d| d.key == key)
        .unwrap()
        .url
        .clone()
}

/// The six an older onboarding installed (the owner's Pixel, 2026-10-22).
const OLD_SIX: [&str; 6] = [
    "cinemeta",
    "kitsu",
    "torrentio",
    "turkcealtyazi",
    "subsense",
    "opensubtitles",
];

#[tokio::test]
async fn an_old_profile_gets_each_default_it_never_had_once_on() {
    let device = Device::new("old");
    let core = device.start();
    old_list(&core, &OLD_SIX, &[]);
    drop(core);

    // The next build starts: the five it never had arrive after its own
    // six, on, in list order, and the card has nothing left to offer.
    let core = device.start();
    let mut expected: Vec<(String, bool)> = OLD_SIX.iter().map(|k| (url(k), true)).collect();
    for key in [
        "trakt",
        "rottentomatoes",
        "torrentcatalogs",
        "streamingcatalogs",
        "stremioaddons",
    ] {
        expected.push((url(key), true));
    }
    assert_eq!(urls(&core), expected);
    assert_eq!(core.prefs().defaults_seen, defaults::LIST_VERSION);
    let stamped = stamp(&core);
    drop(core);

    // A second start adds nothing and writes nothing.
    let core = device.start();
    assert_eq!(urls(&core), expected);
    assert_eq!(stamp(&core), stamped);
}

#[tokio::test]
async fn a_default_switched_off_or_removed_keeps_the_profiles_choice() {
    let device = Device::new("choice");
    let core = device.start();
    // Rotten Tomatoes there but off, Trakt never had.
    let mut keys = OLD_SIX.to_vec();
    keys.push("rottentomatoes");
    old_list(&core, &keys, &["rottentomatoes"]);
    drop(core);
    let core = device.start();
    let shown = |core: &Core, key: &str| {
        urls(core)
            .into_iter()
            .find(|(u, _)| *u == url(key))
            .map(|(_, on)| on)
    };
    assert_eq!(shown(&core, "rottentomatoes"), Some(false), "off stays off");
    assert_eq!(shown(&core, "trakt"), Some(true), "never had: given, on");

    // Removed after it arrived: it stays removed.
    core.remove_addon(&url("trakt")).unwrap();
    core.remove_addon(&url("kitsu")).unwrap();
    drop(core);
    let core = device.start();
    assert_eq!(shown(&core, "trakt"), None);
    assert_eq!(shown(&core, "kitsu"), None);
    assert_eq!(shown(&core, "rottentomatoes"), Some(false));
}

#[tokio::test]
async fn the_defaults_step_counts_what_it_named_and_a_new_or_kids_profile_gets_nothing() {
    let device = Device::new("step");
    let core = device.start();
    // No list yet: the defaults step asks, a start adds nothing.
    drop(core);
    let core = device.start();
    assert_eq!(core.library().addon_list_hlc(OWNER).unwrap(), None);
    assert!(core.addons_fresh());

    // The step named all ten; ones removed later do not come back.
    core.install_defaults(&switches(&[])).unwrap();
    core.remove_addon(&url("kitsu")).unwrap();
    core.remove_addon(&url("rottentomatoes")).unwrap();
    core.set_pin(OWNER, Some("7351"), None).await.unwrap();
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    // A kids profile reads the owner's list: one of its own (a profile
    // made a kid later) is left as it is.
    list_of(&core, &kid, &OLD_SIX, &[]);
    // A new profile copies the owner's list and what it was given, so a
    // default the owner removed (Rotten Tomatoes too, one a list from
    // before would get) is not given to the copy.
    core.create_profile("Sam", "owl").unwrap();
    let sam = core
        .library()
        .profiles()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Sam")
        .unwrap()
        .id;
    drop(core);
    let core = device.start();
    let has = |profile: &str, key: &str| {
        core.library()
            .addons(profile)
            .unwrap()
            .iter()
            .any(|r| r.transport_url == url(key))
    };
    let count = |profile: &str| core.library().addons(profile).unwrap().len();
    assert!(!has(OWNER, "kitsu") && !has(OWNER, "rottentomatoes"));
    assert_eq!(count(&kid), 6);
    assert_eq!(count(&sam), 9);
    assert!(!has(&sam, "kitsu") && !has(&sam, "rottentomatoes"));

    // A kids profile made standard copies the same.
    core.make_standard(&kid).unwrap();
    drop(core);
    let core = device.start();
    let has = |profile: &str, key: &str| {
        core.library()
            .addons(profile)
            .unwrap()
            .iter()
            .any(|r| r.transport_url == url(key))
    };
    assert_eq!(core.library().addons(&kid).unwrap().len(), 9);
    assert!(!has(&kid, "kitsu") && !has(&kid, "rottentomatoes"));
}

#[tokio::test]
async fn a_list_from_before_gets_only_the_brought_and_an_older_default_it_removed_stays_removed() {
    let device = Device::new("before");
    let core = device.start();
    // An older onboarding's six, Kitsu since removed; a second profile of
    // that time with all six.
    let five: Vec<&str> = OLD_SIX.iter().copied().filter(|k| *k != "kitsu").collect();
    old_list(&core, &five, &[]);
    core.library()
        .ensure_profile("old", "Old", &core.zero_hlc())
        .unwrap();
    list_of(&core, "old", &OLD_SIX, &[]);
    drop(core);
    let core = device.start();
    let mut expected: Vec<(String, bool)> = five.iter().map(|k| (url(k), true)).collect();
    for key in [
        "trakt",
        "rottentomatoes",
        "torrentcatalogs",
        "streamingcatalogs",
        "stremioaddons",
    ] {
        expected.push((url(key), true));
    }
    assert_eq!(urls(&core), expected, "Kitsu stays removed");
    assert_eq!(core.library().addons("old").unwrap().len(), 11);
}

/// The owner's later ruling of 2026-10-22: a profile whose defaults step
/// named the nine before Streaming Catalogs gets it once, on, at the end of
/// its list; removed, it never comes back.
#[tokio::test]
async fn a_profile_from_the_nine_gets_streaming_catalogs_once() {
    let device = Device::new("streaming");
    let core = device.start();
    let nine: Vec<DefaultChoice> = switches(&[])
        .into_iter()
        .filter(|c| c.key != "streamingcatalogs")
        .collect();
    core.install_defaults(&nine).unwrap();
    assert_eq!(urls(&core).len(), 10);
    drop(core);
    let core = device.start();
    let rows = urls(&core);
    assert_eq!(rows.len(), 11);
    assert_eq!(rows[10], (url("streamingcatalogs"), true));
    // Given this way, Home shows the five services it showed before and the new ones are off.
    let mut shown = shown_titles(&core);
    shown.sort();
    let mut five: Vec<String> = FIVE.iter().map(|t| t.to_string()).collect();
    five.sort();
    assert_eq!(shown, five);
    assert!(
        service_rows(&core).len() > FIVE.len(),
        "the new services are there, off Home"
    );
    core.remove_addon(&url("streamingcatalogs")).unwrap();
    drop(core);
    let core = device.start();
    assert_eq!(urls(&core).len(), 10, "removed, it stays removed");
}

#[tokio::test]
async fn what_two_devices_gave_is_kept_whole_when_their_records_meet() {
    let a = Core::temporary(OWNER).unwrap();
    let b = Core::temporary(OWNER).unwrap();
    a.install_defaults(&[choice("trakt", true)]).unwrap();
    b.install_defaults(&[choice("rottentomatoes", true)])
        .unwrap();
    let settings = |core: &Core| {
        core.library()
            .outbox(1000)
            .unwrap()
            .records
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.coll == mq_library::coll::SETTINGS)
            .collect::<Vec<_>>()
    };
    let (from_a, from_b) = (settings(&a), settings(&b));
    a.library().apply(&from_b).unwrap();
    b.library().apply(&from_a).unwrap();
    for core in [&a, &b] {
        let had = core.defaults_had();
        assert!(
            had.iter().any(|k| k == "trakt") && had.iter().any(|k| k == "rottentomatoes"),
            "{had:?}"
        );
    }
}

/// Trakt Tv's old deployment, the default's link before 2026-10-22.
const OLD_TRAKT: &str = "https://2ecbbd610840-trakt.baby-beamup.club/manifest.json";

/// A list of the defaults `keys` with Trakt among them on its old link, on
/// or off; the device then starts again.
fn with_old_trakt(core: &Core, keys: &[&str], trakt_on: bool) {
    let rows: Vec<AddonRow> = keys
        .iter()
        .enumerate()
        .map(|(position, key)| {
            let d = defaults::list().iter().find(|d| d.key == *key).unwrap();
            let old = *key == "trakt";
            AddonRow {
                transport_url: if old { OLD_TRAKT.into() } else { d.url.clone() },
                manifest_json: serde_json::to_string(&d.manifest).unwrap(),
                position: position as i64,
                enabled: !old || trakt_on,
                allow_private: false,
            }
        })
        .collect();
    core.library()
        .set_addons(OWNER, &rows, &core.hlc())
        .unwrap();
}

/// The owner, 2026-10-22: a profile on Trakt Tv's old link gets the
/// maintained one in its place once, its switch, its position and the Home
/// rows it hid kept; a second start changes nothing.
#[tokio::test]
async fn trakts_old_link_moves_to_the_maintained_one_once() {
    let four = ["cinemeta", "torrentio", "trakt", "opensubtitles"];
    for on in [true, false] {
        let device = Device::new(&format!("trakt-{on}"));
        let core = device.start();
        with_old_trakt(&core, &four, on);
        let hidden = core
            .home_catalogs()
            .into_iter()
            .find(|c| c.row_id.starts_with("community.trakt-tv@") && c.title == "Trending movies");
        if let Some(row) = &hidden {
            assert!(!row.hidden, "no twin: shown until hidden");
            core.set_home_row_hidden(&row.row_id, true).unwrap();
        }
        assert_eq!(hidden.is_some(), on, "an addon that is off has no rows");
        drop(core);

        let core = device.start();
        let rows = urls(&core);
        assert_eq!(rows[2], (url("trakt"), on), "same place, same switch");
        assert!(rows.iter().all(|(u, _)| u != OLD_TRAKT));
        assert_eq!(rows.iter().filter(|(u, _)| *u == url("trakt")).count(), 1);
        if on {
            let moved = core
                .home_catalogs()
                .into_iter()
                .find(|c| {
                    c.row_id.starts_with("community.trakt-tv@") && c.title == "Trending movies"
                })
                .unwrap();
            assert!(moved.hidden, "the row it hid stays hidden");
            // Only the leaf that changed is written: another device's order
            // edit is not overwritten.
            let written: Vec<String> = core
                .library()
                .settings(OWNER)
                .unwrap()
                .into_iter()
                .map(|(k, _)| k)
                .filter(|k| k.starts_with("homeRows."))
                .collect();
            assert!(
                !written.iter().any(|k| k == "homeRows.order"),
                "{written:?}"
            );
            assert_ne!(Some(moved.row_id), hidden.map(|r| r.row_id));
        }
        let stamped = stamp(&core);
        drop(core);
        let core = device.start();
        assert_eq!(urls(&core), rows);
        assert_eq!(stamp(&core), stamped, "nothing left to move");

        // The old link added again by hand stays: the move was once.
        let mut list = core.library().addons(OWNER).unwrap();
        let mut again = list[2].clone();
        again.transport_url = OLD_TRAKT.into();
        again.position = list.len() as i64;
        list.push(again);
        core.library()
            .set_addons(OWNER, &list, &core.hlc())
            .unwrap();
        drop(core);
        let core = device.start();
        assert!(
            urls(&core).iter().any(|(u, _)| u == OLD_TRAKT),
            "left as the user chose"
        );
    }
}

/// Trakt's "Trending movies" rows in Home's order: (row id, hidden).
fn trakt_trending(core: &Core) -> Vec<(String, bool)> {
    core.home_catalogs()
        .into_iter()
        .filter(|c| c.row_id.starts_with("community.trakt-tv@") && c.title == "Trending movies")
        .map(|c| (c.row_id, c.hidden))
        .collect()
}

/// A stop between moving the Home rows and writing the list (the rows go
/// first) strands nothing: the next start moves the list, and the row the
/// user hid stays hidden.
#[tokio::test]
async fn a_move_stopped_after_the_home_rows_ends_on_the_next_start() {
    let device = Device::new("trakt-stopped");
    let core = device.start();
    with_old_trakt(&core, &["cinemeta", "trakt"], true);
    // The id the moved row will have: the same catalog on the new link
    // (row ids depend on the link, not the device).
    let probe = Device::new("trakt-probe");
    let other = probe.start();
    list_of(&other, OWNER, &["cinemeta", "trakt"], &[]);
    let new_id = trakt_trending(&other)[0].0.clone();
    assert_ne!(new_id, trakt_trending(&core)[0].0);
    // As the stopped move left it: the Home rows already on the new link.
    core.set_prefs(&serde_json::json!({"homeRows": {"hidden": [new_id.clone()]}}))
        .unwrap();
    drop(core);
    let core = device.start();
    assert_eq!(urls(&core)[1], (url("trakt"), true));
    assert_eq!(trakt_trending(&core), [(new_id.clone(), true)]);
    assert_eq!(core.prefs().home_rows.hidden, [new_id]);
}

/// Both links in the list (the new one installed by hand): the old row
/// goes, the new one stays once. A profile without Trakt stays without it.
#[tokio::test]
async fn an_old_trakt_beside_the_new_one_goes_and_no_trakt_stays_none() {
    let device = Device::new("trakt-both");
    let core = device.start();
    with_old_trakt(&core, &["cinemeta", "trakt"], true);
    let mut rows = core.library().addons(OWNER).unwrap();
    let mut new = rows[1].clone();
    new.transport_url = url("trakt");
    new.position = 2;
    rows.push(new);
    core.library()
        .set_addons(OWNER, &rows, &core.hlc())
        .unwrap();
    // The old row's Trending hidden on purpose; the new one, a twin, waits.
    let both = trakt_trending(&core);
    assert_eq!(both.len(), 2);
    core.set_home_row_hidden(&both[0].0, true).unwrap();
    drop(core);
    let core = device.start();
    // The new row's own choice wins: the old row's is not moved onto it.
    assert_eq!(trakt_trending(&core), [(both[1].0.clone(), false)]);
    assert_eq!(core.prefs().home_rows.hidden, [both[0].0.clone()]);
    let trakts: Vec<_> = urls(&core)
        .into_iter()
        .filter(|(u, _)| u.contains("trakt"))
        .collect();
    assert_eq!(trakts, [(url("trakt"), true)]);
    drop(core);

    let device = Device::new("trakt-none");
    let core = device.start();
    core.install_defaults(&switches(&[])).unwrap();
    core.remove_addon(&url("trakt")).unwrap();
    drop(core);
    let core = device.start();
    assert!(urls(&core).iter().all(|(u, _)| !u.contains("trakt")));
}

/// The owner, 2026-10-22: the Rotten Tomatoes addon is his own, configured
/// with RPDB's free key, so its posters carry the ratings band. A profile on
/// the plain link moves to it once, its switch and place kept.
#[tokio::test]
async fn the_rotten_tomatoes_addon_moves_to_the_owners_configured_link() {
    const PLAIN: &str = "https://7a82163c306e-rottentomatoes.baby-beamup.club/manifest.json";
    assert!(url("rottentomatoes").contains("rpdb_key"));
    let device = Device::new("rt-moved");
    let core = device.start();
    let rows: Vec<AddonRow> = ["cinemeta", "rottentomatoes", "opensubtitles"]
        .iter()
        .enumerate()
        .map(|(position, key)| {
            let d = defaults::list().iter().find(|d| d.key == *key).unwrap();
            let rt = *key == "rottentomatoes";
            AddonRow {
                transport_url: if rt { PLAIN.into() } else { d.url.clone() },
                manifest_json: serde_json::to_string(&d.manifest).unwrap(),
                position: position as i64,
                enabled: !rt,
                allow_private: false,
            }
        })
        .collect();
    core.library()
        .set_addons(OWNER, &rows, &core.hlc())
        .unwrap();
    drop(core);
    let core = device.start();
    let rows = urls(&core);
    assert_eq!(
        rows[1],
        (url("rottentomatoes"), false),
        "same place, still off"
    );
    assert!(rows.iter().all(|(u, _)| u != PLAIN));
}

/// Streaming Catalogs before 2026-10-26: the unconfigured link with its
/// five services.
const OLD_SERVICES: &str =
    "https://7a82163c306e-stremio-netflix-catalog-addon.baby-beamup.club/manifest.json";

fn old_services_manifest() -> String {
    let catalogs: Vec<serde_json::Value> = ["nfx", "hbm", "dnp", "amp", "atp"]
        .iter()
        .zip(["Netflix", "HBO Max", "Disney+", "Prime  Video", "Apple TV+"])
        .flat_map(|(id, name)| {
            ["movie", "series"].map(|t| serde_json::json!({"id": id, "type": t, "name": name}))
        })
        .collect();
    serde_json::json!({
        "id": "pw.ers.netflix-catalog", "version": "1.1.1", "name": "Streaming Catalogs",
        "resources": ["catalog"], "types": ["movie", "series"], "idPrefixes": ["tt"], "catalogs": catalogs
    })
    .to_string()
}

/// The service rows on Home: (title, hidden), in Home's order.
fn service_rows(core: &Core) -> Vec<(String, bool)> {
    core.home_catalogs()
        .into_iter()
        .filter(|c| c.row_id.starts_with("pw.ers.netflix-catalog@"))
        .map(|c| (c.title, c.hidden))
        .collect()
}

fn shown_titles(core: &Core) -> Vec<String> {
    service_rows(core)
        .into_iter()
        .filter(|(_, hidden)| !hidden)
        .map(|(t, _)| t)
        .collect()
}

const FIVE: [&str; 10] = [
    "Netflix Movies",
    "Netflix Series",
    "Disney+ Movies",
    "Disney+ Series",
    "Prime Video Movies",
    "Prime Video Series",
    "Apple TV+ Movies",
    "Apple TV+ Series",
    "HBO Max Movies",
    "HBO Max Series",
];

/// plans/discover-v2.md section 5: a profile on the old link moves to the
/// ten-service one once; Home keeps the five services it showed and the
/// row the user hid, and the five new services are off Home until turned on.
#[tokio::test]
async fn streaming_catalogs_moves_to_ten_services_and_home_keeps_its_five() {
    assert!(url("streamingcatalogs").contains("/bmZ4"));
    let device = Device::new("services-moved");
    let core = device.start();
    let cinemeta = defaults::list()
        .iter()
        .find(|d| d.key == "cinemeta")
        .unwrap();
    let rows = vec![
        AddonRow {
            transport_url: cinemeta.url.clone(),
            manifest_json: serde_json::to_string(&cinemeta.manifest).unwrap(),
            position: 0,
            enabled: true,
            allow_private: false,
        },
        AddonRow {
            transport_url: OLD_SERVICES.into(),
            manifest_json: old_services_manifest(),
            position: 1,
            enabled: true,
            allow_private: false,
        },
    ];
    core.library()
        .set_addons(OWNER, &rows, &core.hlc())
        .unwrap();
    let netflix_series = core
        .home_catalogs()
        .into_iter()
        .find(|c| c.title == "Netflix Series")
        .unwrap();
    core.set_home_row_hidden(&netflix_series.row_id, true)
        .unwrap();
    // The user moved Apple TV+ Movies above Disney+ Movies.
    let row = |title: &str| {
        core.home_catalogs()
            .into_iter()
            .find(|c| c.title == title)
            .unwrap()
            .row_id
    };
    let ids: Vec<String> = core.home_catalogs().into_iter().map(|c| c.row_id).collect();
    let at = |id: String| ids.iter().position(|x| *x == id).unwrap() as i64;
    let (apple, disney) = (row("Apple TV+ Movies"), row("Disney+ Movies"));
    core.move_home_row(&apple, at(disney) - at(apple.clone()))
        .unwrap();
    let before = shown_titles(&core);
    drop(core);

    let core = device.start();
    assert_eq!(
        urls(&core)[1],
        (url("streamingcatalogs"), true),
        "same place, same switch"
    );
    // The five it showed, less the row it hid, in the order Home had them: HBO Max second, the user's move kept.
    assert_eq!(shown_titles(&core), before);
    assert_eq!(&before[..2], ["Netflix Movies", "HBO Max Movies"]);
    let pos = |t: &str| before.iter().position(|x| x == t);
    assert!(pos("Apple TV+ Movies") < pos("Disney+ Movies"));
    assert_eq!(pos("Netflix Series"), None);
    assert_eq!(before.len(), FIVE.len() - 1);
    assert!(
        service_rows(&core)
            .iter()
            .any(|(t, hidden)| t == "Hulu Movies" && *hidden),
        "a new service is off Home"
    );
    // Turned on in Customize Home, a new service shows.
    let hulu = core
        .home_catalogs()
        .into_iter()
        .find(|c| c.title == "Hulu Movies")
        .unwrap();
    core.set_home_row_hidden(&hulu.row_id, false).unwrap();
    assert!(shown_titles(&core).contains(&"Hulu Movies".to_string()));
}

/// A fresh profile's Home shows the five services; a user's own configured
/// link and another addon's "Hulu" list keep every row.
#[tokio::test]
async fn only_the_default_links_new_services_are_off_home() {
    let core = Core::temporary(OWNER).unwrap();
    core.install_defaults(&switches(&[])).unwrap();
    let mut shown = shown_titles(&core);
    shown.sort();
    let mut five: Vec<String> = FIVE.iter().map(|t| t.to_string()).collect();
    five.sort();
    assert_eq!(shown, five);

    // The user's own configuration of the same addon: all its rows show.
    let core = Core::temporary(OWNER).unwrap();
    let d = defaults::list()
        .iter()
        .find(|d| d.key == "streamingcatalogs")
        .unwrap();
    let own = AddonRow {
        transport_url: d.url.replace("/bmZ4", "/aGx1"),
        manifest_json: serde_json::to_string(&d.manifest).unwrap(),
        position: 0,
        enabled: true,
        allow_private: false,
    };
    let mut other = own.clone();
    other.transport_url = "https://example.com/lists/manifest.json".into();
    other.position = 1;
    other.manifest_json = serde_json::json!({
        "id": "example.lists", "version": "1.0.0", "name": "Lists", "resources": ["catalog"], "types": ["series"],
        "catalogs": [{"id": "hulu", "type": "series", "name": "Hulu"}]
    })
    .to_string();
    core.library()
        .set_addons(OWNER, &[own, other], &core.hlc())
        .unwrap();
    let rows = service_rows(&core);
    assert!(rows.iter().all(|(_, hidden)| !hidden), "{rows:?}");
    assert!(
        core.home_catalogs()
            .iter()
            .any(|c| c.title == "Hulu Series" && !c.hidden),
        "another addon's list stays"
    );
}

/// The Streaming Catalogs addon on its old link, at `position`, on or off.
fn old_services(position: i64, enabled: bool) -> AddonRow {
    AddonRow {
        transport_url: OLD_SERVICES.into(),
        manifest_json: old_services_manifest(),
        position,
        enabled,
        allow_private: false,
    }
}

/// Where the profile has `link`: Some(on) or None.
fn has(core: &Core, link: &str) -> Option<bool> {
    urls(core)
        .into_iter()
        .find(|(u, _)| u == link)
        .map(|(_, on)| on)
}

/// plans/discover-v2.md section 5: the old link switched off moves off, and
/// once removed it is never given back.
#[tokio::test]
async fn streaming_catalogs_switched_off_or_removed_is_left_as_chosen() {
    let device = Device::new("services-off");
    let core = device.start();
    core.library()
        .set_addons(OWNER, &[old_services(0, false)], &core.hlc())
        .unwrap();
    drop(core);
    let core = device.start();
    assert_eq!(
        has(&core, &url("streamingcatalogs")),
        Some(false),
        "off stays off"
    );
    assert_eq!(has(&core, OLD_SERVICES), None);
    core.remove_addon(&url("streamingcatalogs")).unwrap();
    drop(core);
    let core = device.start();
    assert_eq!(
        has(&core, &url("streamingcatalogs")),
        None,
        "removed stays removed"
    );
    assert_eq!(has(&core, OLD_SERVICES), None);
}

/// plans/discover-v2.md section 5: two devices of one account, the old
/// build on one and the new on the other, end with one row per service.
#[tokio::test]
async fn two_devices_end_with_one_row_per_service() {
    let (one, two) = (Device::new("services-one"), Device::new("services-two"));
    let records = |core: &Core| {
        core.library()
            .outbox(1000)
            .unwrap()
            .records
            .into_iter()
            .map(|(_, r)| r)
            .collect::<Vec<_>>()
    };
    // The old build on the first device: the old link, which the account carries to the second.
    let a = one.start_as(1);
    a.library()
        .set_addons(OWNER, &[old_services(0, true)], &a.hlc())
        .unwrap();
    let b = two.start_as(2);
    b.library().apply(&records(&a)).unwrap();
    drop(b);
    // The new build on the second device moves it; the first takes its records, then is updated too.
    let b = two.start_as(2);
    a.library().apply(&records(&b)).unwrap();
    drop(a);
    let a = one.start_as(1);
    b.library().apply(&records(&a)).unwrap();
    for core in [&a, &b] {
        assert_eq!(has(core, OLD_SERVICES), None);
        assert_eq!(has(core, &url("streamingcatalogs")), Some(true));
        let mut titles: Vec<String> = service_rows(core).into_iter().map(|(t, _)| t).collect();
        let all = titles.len();
        titles.sort();
        titles.dedup();
        assert_eq!(titles.len(), all, "one row per service: {titles:?}");
    }
}

/// The stremio-addons.net directory (plans/plugins-and-marketplace.md 7.1,
/// the owner's decision of 2026-10-28): a default that lists addons, no
/// catalogs and no streams, given once to a profile that had a list before.
#[tokio::test]
async fn the_addon_directory_is_a_default_given_once() {
    let d = mq_runtime::defaults::list()
        .iter()
        .find(|d| d.key == "stremioaddons")
        .expect("the directory is a default");
    assert_eq!(d.url, "https://stremio-addons.net/api/manifest.json");
    assert_eq!(d.group, "lists");
    let resources: Vec<String> = d
        .manifest
        .resources
        .iter()
        .map(|r| {
            serde_json::to_value(r)
                .unwrap()
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(
        resources,
        ["addon_catalog"],
        "it lists addons and does nothing else"
    );
    assert!(d.manifest.catalogs.is_empty());
    assert_eq!(d.manifest.addon_catalogs.len(), 1);
    // A profile that had its defaults before this one gets it once, on.
    let device = Device::new("directory");
    let core = device.start();
    let before: Vec<DefaultChoice> = switches(&[])
        .into_iter()
        .filter(|c| c.key != "stremioaddons")
        .collect();
    core.install_defaults(&before).unwrap();
    assert!(!urls(&core).iter().any(|(u, _)| u == &d.url));
    drop(core);
    let core = device.start();
    assert!(urls(&core).contains(&(d.url.clone(), true)));
    // Removed, it stays removed.
    core.remove_addon(&d.url).unwrap();
    drop(core);
    let core = device.start();
    assert!(!urls(&core).iter().any(|(u, _)| u == &d.url));
}

/// A stand-in for every host off this machine (`set_dev_replay`): the
/// default catalog's first page answers `status` and `body` after `delay`, anything else 404.
async fn still_host(delay: std::time::Duration, status: u16, body: &'static str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                let resp = if head
                    .starts_with("GET /https/v3-cinemeta.strem.io/catalog/movie/top.json ")
                {
                    tokio::time::sleep(delay).await;
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_owned()
                };
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}")
}

/// The replay base is one switch for the whole process: the tests that set it take turns.
static REPLAY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// First run's wall (plans/first-screens.md 4.1): the default catalog's
/// poster links in its order, `https`, without repeats, at most 30, and
/// nothing else (no name, no addon); empty on a failure or past the budget.
#[tokio::test]
async fn first_run_wall_is_the_default_catalogs_posters_or_nothing() {
    use std::time::{Duration, Instant};
    let _replay = REPLAY.lock().await;
    let metas = r#"{"metas":[
        {"id":"tt1","type":"movie","name":"First","poster":"https://images.example/poster/tt1.jpg"},
        {"id":"tt2","type":"movie","name":"No Poster","background":"https://images.example/bg/tt2.jpg"},
        {"id":"tt3","type":"movie","name":"Plain","poster":"http://images.example/poster/tt3.jpg"},
        {"id":"tt6","type":"movie","name":"Inline","poster":"data:image/png;base64,AAAA"},
        {"id":"tt7","type":"movie","name":"Local","poster":"file:///etc/poster.jpg"},
        {"id":"tt4","type":"movie","name":"Again","poster":"https://images.example/poster/tt1.jpg"},
        {"id":"tt5","type":"movie","name":"Last","poster":"https://images.example/poster/tt5.jpg"}
    ]}"#;
    mq_http::set_dev_replay(&still_host(Duration::ZERO, 200, metas).await);
    let core = Core::temporary(OWNER).unwrap();
    assert_eq!(
        core.first_run_wall().await,
        [
            "https://images.example/poster/tt1.jpg",
            "https://images.example/poster/tt3.jpg",
            "https://images.example/poster/tt5.jpg",
        ]
    );
    assert!(core.library().addons(OWNER).unwrap().is_empty());

    // At most 30, the catalog's first 30 in its order.
    let many: String = (0..40)
        .map(|i| format!(r#"{{"id":"tt{i}","type":"movie","name":"T{i}","poster":"https://p.example/{i}.jpg"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let many: &'static str = Box::leak(format!(r#"{{"metas":[{many}]}}"#).into_boxed_str());
    mq_http::set_dev_replay(&still_host(Duration::ZERO, 200, many).await);
    let wall = Core::temporary(OWNER).unwrap().first_run_wall().await;
    let want: Vec<String> = (0..30)
        .map(|i| format!("https://p.example/{i}.jpg"))
        .collect();
    assert_eq!(wall, want);

    // A failing host, an answer that is not JSON, or one of another shape: empty.
    for (status, body) in [(500, metas), (200, "not json"), (200, r#"{"streams":[]}"#)] {
        mq_http::set_dev_replay(&still_host(Duration::ZERO, status, body).await);
        assert!(
            Core::temporary(OWNER)
                .unwrap()
                .first_run_wall()
                .await
                .is_empty(),
            "{status} {body}"
        );
    }

    // As slow as a phone's first request after install: it still comes.
    mq_http::set_dev_replay(&still_host(Duration::from_millis(2500), 200, metas).await);
    assert_eq!(
        Core::temporary(OWNER).unwrap().first_run_wall().await.len(),
        3
    );

    // No answer: empty once the budget is spent.
    mq_http::set_dev_replay(&still_host(Duration::from_secs(30), 200, metas).await);
    let started = Instant::now();
    assert!(
        Core::temporary(OWNER)
            .unwrap()
            .first_run_wall()
            .await
            .is_empty()
    );
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(5800) && waited < Duration::from_millis(7500),
        "waited {waited:?}"
    );
}
