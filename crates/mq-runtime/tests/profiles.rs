//! Profiles across opens of one data directory: shared device state, and
//! which profile a core opens as.

use mq_library::OWNER;
use mq_runtime::{Core, CoreConfig};
use std::path::PathBuf;
use std::time::Duration;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(what: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("mq-runtime-{what}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

fn open(dir: &Dir, hint: &str) -> Core {
    Core::open(CoreConfig {
        data_dir: dir.0.clone(),
        profile_id: hint.into(),
        device_id: 0,
    })
    .unwrap()
}

#[tokio::test]
async fn two_opens_of_one_directory_share_the_device() {
    let dir = dir("shared");
    let a = open(&dir, OWNER);
    // The same directory through another spelling of its path.
    let b = Core::open(CoreConfig {
        data_dir: dir.0.join("."),
        profile_id: OWNER.into(),
        device_id: 0,
    })
    .unwrap();
    assert!(std::ptr::eq(a.library(), b.library()), "one library");
    assert!(std::ptr::eq(a.clock(), b.clock()), "one clock");

    // A deletion through one handle reaches the other's listeners.
    let kid = a.create_profile("Kid", "fox").unwrap();
    let mut changes = b.library().subscribe();
    a.delete_profile(&kid).unwrap();
    tokio::time::timeout(Duration::from_secs(5), changes.recv())
        .await
        .expect("b hears a's deletion")
        .unwrap();

    // One throttle: the fifth try waits whichever handle makes it.
    a.set_pin(OWNER, Some("1357"), None).await.unwrap();
    for _ in 0..4 {
        let wrong = a.check_pin(OWNER, Some("0000")).await.unwrap_err();
        assert_eq!(wrong.code(), Some("wrongPin"));
    }
    let waited = b.check_pin(OWNER, Some("1357")).await.unwrap_err();
    assert_eq!(waited.code(), Some("locked"), "{waited:?}");

    // Closed everywhere, the directory opens again cleanly.
    drop((a, b));
    let c = open(&dir, OWNER);
    assert_eq!(c.library().pin_failures(OWNER).unwrap(), 4);
}

#[tokio::test]
async fn a_stale_stored_profile_opens_the_owner_and_the_hint_counts_only_on_first_run() {
    let dir = dir("active");
    let core = open(&dir, OWNER);
    let kept = core.create_profile("Kept", "fox").unwrap();
    // First run (nothing stored): the app's hint, when it names a profile.
    assert_eq!(open(&dir, &kept).profile(), kept);
    assert_eq!(open(&dir, "nobody").profile(), OWNER);

    // A stored choice that is gone opens the owner, whatever the hint.
    let gone = core.create_profile("Gone", "owl").unwrap();
    core.for_profile(&gone).make_active().unwrap();
    core.delete_profile(&gone).unwrap();
    assert_eq!(open(&dir, &kept).profile(), OWNER);
    assert_eq!(open(&dir, &gone).profile(), OWNER);
    assert!(core.library().profile(&gone).unwrap().is_none());
}

#[tokio::test]
async fn a_pin_fails_closed_unless_known_to_be_absent() {
    let core = Core::temporary(OWNER).unwrap();
    assert!(!core.has_pin(), "the owner has none");
    assert!(
        core.for_profile("nobody").has_pin(),
        "unknown counts as locked"
    );
    let synced = mq_library::SyncRecord {
        coll: mq_library::coll::PROFILES,
        key: "new".into(),
        value: serde_json::json!({"profile": "new", "name": "new"}),
        hlc: core.hlc(),
    };
    core.library().apply(&[synced]).unwrap();
    assert!(core.for_profile("new").has_pin(), "no lock record yet");
}

#[tokio::test]
async fn a_pin_set_by_someone_allowed_starts_the_count_again() {
    let core = Core::temporary(OWNER).unwrap();
    let kid = core.create_profile("Kid", "fox").unwrap();
    let as_kid = core.for_profile(&kid);
    as_kid.set_pin(&kid, Some("1357"), None).await.unwrap();
    for _ in 0..4 {
        let wrong = core.check_pin(&kid, Some("0000")).await.unwrap_err();
        assert_eq!(wrong.code(), Some("wrongPin"));
    }
    let waits = core.check_pin(&kid, Some("1357")).await.unwrap_err();
    assert_eq!(waits.code(), Some("locked"));
    // The owner clears it and the kid sets another: the old tries are gone.
    core.set_pin(&kid, None, None).await.unwrap();
    as_kid.set_pin(&kid, Some("2468"), None).await.unwrap();
    assert_eq!(core.library().pin_failures(&kid).unwrap(), 0);
    let wrong = core.check_pin(&kid, Some("0000")).await.unwrap_err();
    assert_eq!(wrong.code(), Some("wrongPin"), "no wait carried over");
    core.check_pin(&kid, Some("2468")).await.unwrap();
}

#[tokio::test]
async fn avatars_are_short_app_ids() {
    let core = Core::temporary(OWNER).unwrap();
    for bad in [
        "x".repeat(65),
        "<svg onload=x>".into(),
        "fox amber".into(),
        "ü".into(),
    ] {
        let err = core.create_profile("Kid", &bad).unwrap_err();
        assert_eq!(err.code(), Some("badAvatar"), "{bad}");
    }
    let kid = core.create_profile("Kid", "fox:amber_2-b").unwrap();
    core.create_profile("Other", "").unwrap();
    let err = core
        .update_profile(&kid, None, Some("a/b"), None)
        .unwrap_err();
    assert_eq!(err.code(), Some("badAvatar"));
    core.update_profile(&kid, None, Some(&"o".repeat(64)), None)
        .unwrap();
}

/// A catalog addon on loopback whose answers may be cached for ten minutes.
async fn cached_addon(delay: Duration) -> mq_library::AddonRow {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let body = r#"{"metas":[]}"#;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: max-age=600\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "cat", "version": "1.0.0", "name": "Cat", "types": ["movie"],
        "resources": ["catalog"], "catalogs": [{"type": "movie", "id": "top"}]
    });
    mq_library::AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

#[tokio::test]
async fn a_removal_forgets_cached_addon_answers() {
    let core = Core::temporary(OWNER).unwrap();
    let row = cached_addon(Duration::ZERO).await;
    core.library()
        .set_addons(OWNER, &[row], &core.hlc())
        .unwrap();
    let addon = core.installed_addons().unwrap().remove(0);
    let path = mq_protocol::ResourcePath {
        resource: "catalog".into(),
        r#type: "movie".into(),
        id: "top".into(),
        extra: vec![],
    };
    let cached = || async {
        core.addons()
            .fetch(&addon, &path, Duration::from_secs(5))
            .await
            .unwrap()
            .from_cache
    };
    assert!(!cached().await);
    assert!(cached().await);
    // Cache keys are addon URLs, which can hold a profile's keys.
    let kid = core.create_profile("Kid", "fox").unwrap();
    core.delete_profile(&kid).unwrap();
    assert!(
        !cached().await,
        "memory and disk copies went with the profile"
    );
}

fn catalog() -> mq_protocol::ResourcePath {
    mq_protocol::ResourcePath {
        resource: "catalog".into(),
        r#type: "movie".into(),
        id: "top".into(),
        extra: vec![],
    }
}

#[tokio::test]
async fn an_answer_that_a_removal_overtakes_is_not_cached() {
    let core = Core::temporary(OWNER).unwrap();
    let row = cached_addon(Duration::from_millis(300)).await;
    core.library()
        .set_addons(OWNER, &[row], &core.hlc())
        .unwrap();
    let addon = core.installed_addons().unwrap().remove(0);
    let kid = core.create_profile("Kid", "fox").unwrap();
    let slow = {
        let (core, addon) = (core.clone(), addon.clone());
        tokio::spawn(async move {
            core.addons()
                .fetch(&addon, &catalog(), Duration::from_secs(5))
                .await
                .unwrap()
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    core.delete_profile(&kid).unwrap();
    assert!(!slow.await.unwrap().from_cache);
    let again = core
        .addons()
        .fetch(&addon, &catalog(), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(!again.from_cache, "the overtaken answer was not kept");
}

#[tokio::test]
async fn a_removal_through_one_open_retires_another_opens_cache() {
    let dir = dir("caches");
    let (a, b) = (open(&dir, OWNER), open(&dir, OWNER));
    let row = cached_addon(Duration::ZERO).await;
    a.library().set_addons(OWNER, &[row], &a.hlc()).unwrap();
    let addon = b.installed_addons().unwrap().remove(0);
    let cached = || async {
        b.addons()
            .fetch(&addon, &catalog(), Duration::from_secs(5))
            .await
            .unwrap()
            .from_cache
    };
    assert!(!cached().await);
    assert!(cached().await);
    let kid = a.create_profile("Kid", "fox").unwrap();
    a.delete_profile(&kid).unwrap();
    assert!(!cached().await, "b's memory copy went too");
}

/// A meta addon on loopback answering every title with `meta` after `delay`.
async fn meta_addon(meta: serde_json::Value, delay: Duration) -> mq_library::AddonRow {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = serde_json::json!({ "meta": meta }).to_string();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": "meta", "version": "1.0.0", "name": "Meta", "types": ["movie", "series"],
        "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    mq_library::AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

/// `profile` played `video` of `item` (named `name` when given) at `at`.
fn play(
    core: &Core,
    profile: &str,
    kind: &str,
    video: &str,
    name: Option<&str>,
    done: bool,
    at: i64,
) {
    let item = video.split(':').next().unwrap();
    if let Some(name) = name {
        let row = mq_library::LibraryItemRow {
            item_id: item.into(),
            kind: kind.into(),
            name: name.into(),
            poster: Some("https://img.example/poster.jpg".into()),
            poster_shape: None,
            added_ms: 1,
        };
        core.library()
            .upsert_item(profile, &row, true, &core.hlc())
            .unwrap();
    }
    let progress = mq_library::ProgressRow {
        video_id: video,
        item_id: item,
        position_ms: if done { 3_000_000 } else { 600_000 },
        duration_ms: 3_000_000,
        updated_ms: at,
    };
    core.library()
        .set_progress(profile, progress, &core.hlc())
        .unwrap();
}

/// plans/profile-panel.md 6: each profile's newest play (a finished film
/// too), its label and spoken line from the core; nothing for a profile
/// with no play or no name; a PIN profile only to itself and the owner; a
/// kids profile only what its list allows, under the list's name; nothing
/// at all for a kids session.
#[tokio::test]
async fn each_profile_gives_its_last_title_to_whom_may_see_it() {
    let core = Core::temporary(OWNER).unwrap();
    core.set_pin(OWNER, Some("1357"), None).await.unwrap();
    let art = "https://img.example/background.jpg";
    let meta = serde_json::json!({"id": "tt0", "type": "series", "name": "From the addon", "background": art});
    let addon = meta_addon(meta, Duration::ZERO).await;
    core.library()
        .set_addons(OWNER, &[addon], &core.hlc())
        .unwrap();
    // The owner: an episode, then a film finished after it.
    play(
        &core,
        OWNER,
        "series",
        "tt5875444:4:2",
        Some("Slow Horses"),
        false,
        10,
    );
    play(&core, OWNER, "movie", "tt0113277", Some("Heat"), true, 20);
    let std = core.create_profile("Std", "fox").unwrap();
    play(
        &core,
        &std,
        "series",
        "tt14688458:2:4",
        Some("Silo"),
        false,
        30,
    );
    let _empty = core.create_profile("Empty", "fox").unwrap();
    // Progress that synced before its title's row: no raw id as a name.
    let raw = core.create_profile("Raw", "fox").unwrap();
    play(&core, &raw, "movie", "tt9", None, false, 40);
    let pinned = core.create_profile("Pinned", "fox").unwrap();
    play(
        &core,
        &pinned,
        "series",
        "tt10986410:1:3",
        Some("Ted Lasso"),
        false,
        50,
    );
    core.for_profile(&pinned)
        .set_pin(&pinned, Some("2468"), None)
        .await
        .unwrap();
    // A kid: an allowed film, then a newer play of one off the list.
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let allowed = mq_library::AllowedRow {
        item_id: "tt0317219".into(),
        kind: "movie".into(),
        name: "Cars".into(),
        names: vec![],
        poster: None,
        allowed: true,
        added_ms: 1,
    };
    core.library()
        .write_allowed(&kid, &allowed, &core.hlc())
        .unwrap();
    play(
        &core,
        &kid,
        "movie",
        "tt0317219",
        Some("Another name"),
        false,
        60,
    );
    play(
        &core,
        &kid,
        "movie",
        "tt0110912",
        Some("Pulp Fiction"),
        false,
        70,
    );
    // A profile still arriving from another device (no access register yet).
    let record = |coll, value: serde_json::Value| mq_library::SyncRecord {
        coll,
        key: "y".into(),
        value,
        hlc: core.hlc(),
    };
    core.library()
        .apply(&[
            record(
                mq_library::coll::PROFILES,
                serde_json::json!({"profile": "y", "name": "Y"}),
            ),
            record(
                mq_library::coll::PROFILE_LOCK,
                serde_json::json!({"profile": "y", "pinHash": null}),
            ),
        ])
        .unwrap();
    play(
        &core,
        "y",
        "movie",
        "tt0133093",
        Some("The Matrix"),
        false,
        80,
    );

    let seen = |of: &Core| {
        let of = of.clone();
        async move {
            of.profiles_last_watched()
                .await
                .into_iter()
                .map(|w| (w.profile, w.label, w.spoken, w.art))
                .collect::<Vec<_>>()
        }
    };
    let entry = |p: &str, label: &str, spoken: &str| {
        (
            p.to_owned(),
            label.to_owned(),
            spoken.to_owned(),
            Some(art.to_owned()),
        )
    };
    let owner_sees = seen(&core).await;
    assert_eq!(
        owner_sees,
        vec![
            entry(OWNER, "Heat", "Heat"),
            entry(&std, "Silo, S2:E4", "Silo, season 2, episode 4"),
            entry(
                &pinned,
                "Ted Lasso, S1:E3",
                "Ted Lasso, season 1, episode 3"
            ),
            entry(&kid, "Cars", "Cars"),
        ]
    );
    // Std sees no PIN profile's title, the owner's included.
    let std_sees = seen(&core.for_profile(&std)).await;
    assert_eq!(
        std_sees.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(),
        [std.as_str(), kid.as_str()]
    );
    // A PIN profile sees its own.
    let pinned_sees = seen(&core.for_profile(&pinned)).await;
    assert!(pinned_sees.iter().any(|e| e.0 == pinned), "{pinned_sees:?}");
    assert!(seen(&core.for_profile(&kid)).await.is_empty());
}

/// The art: the title's backdrop, else the episode's still, never the
/// poster; a meta that has not answered within 1.5 s leaves the entry
/// without it, and the answer never waits for it.
#[tokio::test]
async fn art_is_the_backdrop_else_the_still_and_never_waits_long() {
    let core = Core::temporary(OWNER).unwrap();
    play(
        &core,
        OWNER,
        "series",
        "tt5875444:4:2",
        Some("Slow Horses"),
        false,
        10,
    );
    let still = "https://img.example/still.jpg";
    let meta = serde_json::json!({
        "id": "tt5875444", "type": "series", "name": "Slow Horses",
        "poster": "https://img.example/poster.jpg",
        "videos": [{"id": "tt5875444:4:2", "title": "E2", "season": 4, "episode": 2, "thumbnail": still}]
    });
    let quick = meta_addon(meta.clone(), Duration::ZERO).await;
    core.library()
        .set_addons(OWNER, &[quick], &core.hlc())
        .unwrap();
    let art = core.profiles_last_watched().await[0].art.clone();
    assert_eq!(art.as_deref(), Some(still));

    let slow = meta_addon(meta, Duration::from_secs(8)).await;
    core.library()
        .set_addons(OWNER, &[slow], &core.hlc())
        .unwrap();
    let started = std::time::Instant::now();
    let answer = core.profiles_last_watched().await;
    let took = started.elapsed();
    assert_eq!(answer[0].label, "Slow Horses, S4:E2");
    assert_eq!(answer[0].art, None);
    assert!(took < Duration::from_millis(2_500), "{took:?}");
    eprintln!("profiles_last_watched with a silent addon: {took:?}");
}

// First run's profile step and a later join (plans/first-screens.md 4.2).

fn at(wall_ms: u64, device: u64) -> String {
    mq_library::Hlc {
        wall_ms,
        counter: 0,
        device,
    }
    .encode()
}

/// The account's registers: its owner, on device 9, renamed "Robin" with a
/// picture at a fixed past time, or never renamed (the zero time).
fn account(renamed: bool) -> Vec<mq_library::SyncRecord> {
    let a = mq_library::Library::temporary().unwrap();
    a.ensure_profile(OWNER, mq_library::OWNER_NAME, &at(0, 9))
        .unwrap();
    if renamed {
        a.update_profile(OWNER, "Robin", "pic:robin", 0, &at(1_000, 9))
            .unwrap();
    }
    a.changed_since(&mq_library::SyncCursor::default(), 100_000)
        .unwrap()
}

/// A join as the engine runs it: marked, the first pull, the settle.
fn join(core: &Core, records: &[mq_library::SyncRecord]) {
    core.joined_sync_account();
    core.library().apply(records).unwrap();
    core.library()
        .settle_join_defaults(core.clock().device())
        .unwrap();
}

fn owner(core: &Core) -> (String, String) {
    let row = core.library().profile(OWNER).unwrap().unwrap();
    (row.name, row.avatar)
}

fn owner_hlc(core: &Core) -> String {
    core.library()
        .changed_since(&mq_library::SyncCursor::default(), 100_000)
        .unwrap()
        .into_iter()
        .find(|r| r.coll == mq_library::coll::PROFILES && r.key == OWNER)
        .unwrap()
        .hlc
}

/// The owner records waiting to be sent: (name, hlc).
fn queued_owner(core: &Core) -> Vec<(String, String)> {
    core.library()
        .outbox(1_000)
        .unwrap()
        .records
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.coll == mq_library::coll::PROFILES && r.key == OWNER)
        .map(|r| {
            (
                r.value["name"].as_str().unwrap_or_default().to_owned(),
                r.hlc,
            )
        })
        .collect()
}

fn mine(name: &str) -> (String, String) {
    (name.to_owned(), String::new())
}

#[tokio::test]
async fn an_untouched_continue_then_join_keeps_the_accounts_owner() {
    let core = Core::temporary(OWNER).unwrap();
    let queued = core.library().outbox(1_000).unwrap().records.len();
    let mut changes = core.library().subscribe();
    core.finish_profile_step("", "").unwrap();
    let mut heard = Vec::new();
    while let Ok(tables) = changes.try_recv() {
        heard.extend(tables);
    }
    assert!(heard.contains(&mq_library::Table::Profiles), "announced");
    // The stored values count as untouched too.
    core.finish_profile_step(" Main ", "").unwrap();
    assert_eq!(core.library().outbox(1_000).unwrap().records.len(), queued);
    assert!(
        owner_hlc(&core).starts_with("000000000000"),
        "never written"
    );
    assert!(core.profiles_snapshot(false).profile_step_done);

    join(&core, &account(true));
    assert_eq!(owner(&core), ("Robin".into(), "pic:robin".into()));
}

#[tokio::test]
async fn a_colour_only_edit_then_join_yields_to_a_renamed_account_owner() {
    let core = Core::temporary(OWNER).unwrap();
    core.finish_profile_step("", "glyph:sea").unwrap();
    assert_eq!(owner(&core), ("Main".into(), "glyph:sea".into()));
    join(&core, &account(true));
    assert_eq!(owner(&core), ("Robin".into(), "pic:robin".into()));
    let sent = queued_owner(&core);
    assert!(!sent.is_empty());
    assert!(
        sent.iter().all(|s| *s == ("Robin".into(), at(1_000, 9))),
        "{sent:?}"
    );
}

#[tokio::test]
async fn a_typed_name_then_join_yields_to_a_renamed_account_owner() {
    let core = Core::temporary(OWNER).unwrap();
    core.finish_profile_step("Deniz", "").unwrap();
    assert_eq!(owner(&core), mine("Deniz"));
    join(&core, &account(true));
    assert_eq!(owner(&core), ("Robin".into(), "pic:robin".into()));
}

#[tokio::test]
async fn a_not_now_owner_edit_is_kept_when_the_account_owner_was_never_renamed() {
    let core = Core::temporary(OWNER).unwrap();
    core.finish_profile_step("Deniz", "").unwrap();
    let edited = owner_hlc(&core);
    join(&core, &account(false));
    assert_eq!(owner(&core), mine("Deniz"));
    assert_eq!(owner_hlc(&core), edited);
    assert!(
        queued_owner(&core).contains(&("Deniz".into(), edited.clone())),
        "{:?}",
        queued_owner(&core)
    );
}

#[tokio::test]
async fn an_empty_field_keeps_a_picked_avatar() {
    let core = Core::temporary(OWNER).unwrap();
    core.update_profile(OWNER, None, Some("glyph:sea"), None)
        .unwrap();
    let queued = core.library().outbox(1_000).unwrap().records.len();
    core.finish_profile_step("", "").unwrap();
    assert_eq!(core.library().outbox(1_000).unwrap().records.len(), queued);
    assert_eq!(owner(&core), ("Main".into(), "glyph:sea".into()));
    core.finish_profile_step("Deniz", "").unwrap();
    assert_eq!(owner(&core), ("Deniz".into(), "glyph:sea".into()));
}

#[tokio::test]
async fn a_not_now_owner_edit_survives_a_join_left_before_its_first_pull() {
    let core = Core::temporary(OWNER).unwrap();
    core.finish_profile_step("Deniz", "").unwrap();
    let edited = owner_hlc(&core);
    // A pairing tried again marks the join twice before any pull: the
    // first hold's time is the one kept.
    core.joined_sync_account();
    core.joined_sync_account();
    core.library().forget_sync().unwrap();
    assert_eq!(owner(&core), mine("Deniz"));
    assert_eq!(owner_hlc(&core), edited);
}

#[tokio::test]
async fn profile_step_done_is_device_only_and_wiped() {
    let core = Core::temporary(OWNER).unwrap();
    assert!(!core.profiles_snapshot(false).profile_step_done);
    core.finish_profile_step("Deniz", "glyph:sea").unwrap();
    assert!(core.profiles_snapshot(false).profile_step_done);
    // Another device takes everything this one has, but not the mark.
    let other = Core::temporary(OWNER).unwrap();
    other
        .library()
        .apply(
            &core
                .library()
                .changed_since(&mq_library::SyncCursor::default(), 100_000)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(owner(&other).0, "Deniz");
    assert!(!other.profiles_snapshot(false).profile_step_done);
    core.wipe().unwrap();
    assert!(!core.profiles_snapshot(false).profile_step_done);
    // Not the owner: refused, nothing marked.
    let kid = other.create_profile("Kid", "fox").unwrap();
    let kid = other.for_profile(&kid);
    assert!(kid.finish_profile_step("", "").is_err());
    assert!(!kid.profiles_snapshot(false).profile_step_done);
}
