//! Kids profiles at the runtime (plans/m11-kids.md, phase K2): creating
//! them under the owner's PIN, the owner's edits of a list, the owner's
//! views of it, and Trakt kept away from kids.

use mq_library::{AddonRow, AllowedRow, OWNER, Policy, SyncRecord, coll};
use mq_runtime::{
    Core, KidsKeysView, KidsTitlesView, PlayTarget, PlaybackView, ProfileError, TraktConfig,
    TraktHosts, TraktPlayback, TraktRecord,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const PIN: &str = "1357";

/// The owner, with a PIN.
async fn owner() -> Core {
    let core = Core::temporary(OWNER).unwrap();
    core.set_pin(OWNER, Some(PIN), None).await.unwrap();
    core
}

fn code(result: Result<impl std::fmt::Debug, ProfileError>) -> Option<&'static str> {
    result.err().and_then(|e| e.code())
}

/// A meta addon answering every request with `body` after `delay`.
async fn meta_addon(n: u32, body: String, delay: Duration) -> AddonRow {
    meta_addon_held(n, body, delay, None).await
}

/// As `meta_addon`; with `held`, no request is answered before every
/// party of that barrier has asked.
async fn meta_addon_held(
    n: u32,
    body: String,
    delay: Duration,
    held: Option<Arc<tokio::sync::Barrier>>,
) -> AddonRow {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = Arc::new(body);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (body, held) = (body.clone(), held.clone());
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                if let Some(held) = held {
                    held.wait().await;
                }
                tokio::time::sleep(delay).await;
                let (status, body) = if body.is_empty() {
                    ("500 Internal Server Error", "{}")
                } else {
                    ("200 OK", body.as_str())
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = serde_json::json!({
        "id": format!("meta.test.{n}"), "version": "1.0.0", "name": format!("Meta {n}"),
        "types": ["series", "movie"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    }
}

fn meta(kind: &str, id: &str, name: &str) -> String {
    serde_json::json!({"meta": {"id": id, "type": kind, "name": name,
                                "poster": format!("https://img.example/{id}.jpg")}})
    .to_string()
}

async fn install(core: &Core, addons: Vec<AddonRow>) {
    core.library()
        .set_addons(OWNER, &addons, &core.hlc())
        .unwrap();
}

#[tokio::test]
async fn kids_profiles_need_a_checkable_owner_pin() {
    let core = Core::temporary(OWNER).unwrap();
    assert_eq!(
        code(core.create_kids_profile("Kid", "fox")),
        Some("kidsNeedPin")
    );
    // An owner hash this build cannot check is no PIN to leave with.
    core.library()
        .set_pin_hash(OWNER, Some("$argon2id$garbage"), &core.hlc())
        .unwrap();
    assert_eq!(
        code(core.create_kids_profile("Kid", "fox")),
        Some("pinUnreadable")
    );
    core.library()
        .set_pin_hash(OWNER, None, &core.hlc())
        .unwrap();
    core.set_pin(OWNER, Some(PIN), None).await.unwrap();
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let row = core.library().profile(&kid).unwrap().unwrap();
    assert!(row.ready() && row.kids() && row.pin_hash().is_none());
    assert!(core.library().addons(&kid).unwrap().is_empty(), "no copy");
    let snapshot = core.profiles_snapshot(false);
    let info = snapshot.profiles.iter().find(|p| p.id == kid).unwrap();
    assert!(info.kids && !snapshot.kids);
    assert!(core.for_profile(&kid).profiles_snapshot(false).kids);
    // Only the owner creates one.
    let other = core.create_profile("Std", "owl").unwrap();
    assert_eq!(
        code(core.for_profile(&other).create_kids_profile("Kid", "fox")),
        Some("notOwner")
    );
}

#[tokio::test]
async fn the_owner_pin_stays_while_kids_exist() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let cleared = core.set_pin(OWNER, None, Some(PIN)).await;
    assert_eq!(code(cleared), Some("kidsNeedPin"));
    assert!(core.has_pin());
    // A change keeps a PIN: allowed.
    core.set_pin(OWNER, Some("2580"), Some(PIN)).await.unwrap();
    core.delete_profile(&kid).unwrap();
    core.set_pin(OWNER, None, Some("2580")).await.unwrap();
    assert!(!core.has_pin());
}

#[tokio::test]
async fn an_allow_keeps_every_verified_name_that_arrives_in_time() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let wait = Duration::from_secs(3);
    install(
        &core,
        vec![
            // Asked at once: two that take 3 s each both arrive in 5 s.
            meta_addon(1, meta("movie", "tt1", "Up"), wait).await,
            meta_addon(2, meta("movie", "tt1", "Yukarı Bak"), wait).await,
            // Another title's answer and a failure are no names.
            meta_addon(3, meta("movie", "tt9", "Other"), Duration::ZERO).await,
            meta_addon(4, String::new(), Duration::ZERO).await,
            // Past the deadline: left out.
            meta_addon(5, meta("movie", "tt1", "Oben"), Duration::from_secs(7)).await,
        ],
    )
    .await;
    let started = Instant::now();
    core.allow_title(&kid, "movie", "tt1").await.unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(6500),
        "one 5 s cap"
    );
    let list = core.library().allowed_list(&kid).unwrap();
    let entry = &list.counted[0];
    assert_eq!(
        (entry.name.as_str(), entry.names.as_slice()),
        ("Up", &["Yukarı Bak".to_owned()][..])
    );
    assert_eq!(entry.poster.as_deref(), Some("https://img.example/tt1.jpg"));
    assert!(core.library().policy(&kid).allows("movie", "tt1"));
}

#[tokio::test]
async fn an_allow_needs_details_for_that_very_title() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    install(
        &core,
        vec![meta_addon(1, String::new(), Duration::ZERO).await],
    )
    .await;
    assert_eq!(
        code(core.allow_title(&kid, "movie", "tt1").await),
        Some("noDetails")
    );
    // `{}` gives no details either.
    install(
        &core,
        vec![meta_addon(1, "{}".into(), Duration::ZERO).await],
    )
    .await;
    assert_eq!(
        code(core.allow_title(&kid, "movie", "tt1").await),
        Some("noDetails")
    );
    install(
        &core,
        vec![meta_addon(1, meta("series", "tt1", "Up"), Duration::ZERO).await],
    )
    .await;
    assert_eq!(
        code(core.allow_title(&kid, "movie", "tt1").await),
        Some("mismatch"),
        "another kind"
    );
    assert!(
        core.library()
            .allowed_list(&kid)
            .unwrap()
            .counted
            .is_empty()
    );
    // Only the owner, only for a kids profile.
    let std = core.create_profile("Std", "owl").unwrap();
    assert_eq!(
        code(core.allow_title(&std, "movie", "tt1").await),
        Some("notKids")
    );
    assert_eq!(
        code(
            core.for_profile(&kid)
                .allow_title(&kid, "movie", "tt1")
                .await
        ),
        Some("notOwner")
    );
    assert_eq!(
        code(core.allow_title("nobody", "movie", "tt1").await),
        Some("unknownProfile")
    );
}

#[tokio::test]
async fn a_remove_sent_during_a_slow_allow_wins() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    install(
        &core,
        vec![meta_addon(1, meta("movie", "tt1", "Up"), Duration::from_millis(1500)).await],
    )
    .await;
    let allow = {
        let (core, kid) = (core.clone(), kid.clone());
        tokio::spawn(async move { core.allow_title(&kid, "movie", "tt1").await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    core.remove_title(&kid, "tt1").unwrap();
    allow.await.unwrap().unwrap();
    assert!(
        !core.library().policy(&kid).allows("movie", "tt1"),
        "the Remove came later"
    );
}

#[tokio::test]
async fn the_list_is_capped_on_this_device() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    for i in 0..1000 {
        let row = AllowedRow {
            item_id: format!("tt{i:04}"),
            kind: "movie".into(),
            name: "T".into(),
            names: vec![],
            poster: None,
            allowed: true,
            added_ms: i,
        };
        core.library()
            .write_allowed(&kid, &row, &core.hlc())
            .unwrap();
    }
    install(
        &core,
        vec![meta_addon(1, meta("movie", "tt0001", "T"), Duration::ZERO).await],
    )
    .await;
    assert_eq!(
        code(core.allow_title(&kid, "movie", "tt5000").await),
        Some("listFull")
    );
    // A title already on it can be allowed again.
    core.allow_title(&kid, "movie", "tt0001").await.unwrap();
}

/// Two Allows at 999 both pass the early check, then wait on their metas
/// together: the write decides, so one lands and one is `listFull`.
#[tokio::test]
async fn two_allows_racing_at_nine_hundred_ninety_nine_admit_one() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    for i in 0..999 {
        let row = AllowedRow {
            item_id: format!("tt{i:04}"),
            kind: "movie".into(),
            name: "T".into(),
            names: vec![],
            poster: None,
            allowed: true,
            added_ms: i,
        };
        core.library()
            .write_allowed(&kid, &row, &core.hlc())
            .unwrap();
    }
    // Each Allow asks both addons; none is answered before all four asks
    // are in, so both Allows passed their early check before either writes.
    let held = Some(Arc::new(tokio::sync::Barrier::new(4)));
    install(
        &core,
        vec![
            meta_addon_held(1, meta("movie", "tta", "A"), Duration::ZERO, held.clone()).await,
            meta_addon_held(2, meta("movie", "ttb", "B"), Duration::ZERO, held).await,
        ],
    )
    .await;
    let allow = |id: &'static str| {
        let (core, kid) = (core.clone(), kid.clone());
        tokio::spawn(async move { core.allow_title(&kid, "movie", id).await })
    };
    let (a, b) = (allow("tta"), allow("ttb"));
    let codes = [code(a.await.unwrap()), code(b.await.unwrap())];
    assert_eq!(
        codes.iter().filter(|c| **c == Some("listFull")).count(),
        1,
        "{codes:?}"
    );
    assert!(codes.contains(&None));
    let list = core.library().allowed_list(&kid).unwrap();
    assert_eq!((list.counted.len(), list.over_count), (1000, 0));
}

/// The picker reads `kids` and `syncing` from the access register, so it
/// follows a make-standard and an access record that arrives on its own.
#[tokio::test]
async fn the_picker_follows_the_access_register() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let (_session, state) = tokio::sync::watch::channel((OWNER.to_owned(), false, None));
    let view = mq_runtime::ProfilesView::open(&core, state);
    let info = |id: &str| view.subscribe().0.profiles.into_iter().find(|p| p.id == id);
    assert!(info(&kid).unwrap().kids);
    core.make_standard(&kid).unwrap();
    until("the picker reads it standard", || !info(&kid).unwrap().kids).await;
    // A profile and its lock first, its access record later on its own.
    let record = |coll, value: serde_json::Value| SyncRecord {
        coll,
        key: "y".into(),
        value,
        hlc: core.hlc(),
    };
    core.library()
        .apply(&[
            record(
                coll::PROFILES,
                serde_json::json!({"profile": "y", "name": "Y"}),
            ),
            record(
                coll::PROFILE_LOCK,
                serde_json::json!({"profile": "y", "pinHash": null}),
            ),
        ])
        .unwrap();
    until("the profile listed as syncing", || {
        info("y").is_some_and(|p| p.syncing)
    })
    .await;
    core.library()
        .apply(&[record(
            coll::PROFILE_ACCESS,
            serde_json::json!({"profile": "y", "kids": false}),
        )])
        .unwrap();
    until("the profile ready", || {
        info("y").is_some_and(|p| !p.syncing && !p.kids)
    })
    .await;
}

#[tokio::test]
async fn remove_and_make_standard() {
    let core = owner().await;
    install(
        &core,
        vec![meta_addon(1, meta("movie", "tt1", "Up"), Duration::ZERO).await],
    )
    .await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    core.allow_title(&kid, "movie", "tt1").await.unwrap();
    core.remove_title(&kid, "tt1").unwrap();
    assert!(!core.library().policy(&kid).allows("movie", "tt1"));
    assert_eq!(
        code(core.for_profile(&kid).make_standard(&kid)),
        Some("notOwner")
    );
    core.make_standard(&kid).unwrap();
    assert_eq!(core.library().policy(&kid), Policy::Open);
    assert_eq!(core.library().addons(&kid).unwrap().len(), 1, "the owner's");
    assert_eq!(code(core.make_standard(&kid)), Some("notKids"));
}

/// Waits for `check` to hold, at most 5 s.
async fn until(what: &str, check: impl Fn() -> bool) {
    for _ in 0..100 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("waited 5 s for {what}");
}

#[tokio::test]
async fn the_owners_kids_views_are_live_and_count_the_cap() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    // Only the owner opens them.
    assert!(matches!(
        KidsKeysView::open(&core.for_profile(&kid)),
        Err(ProfileError::NotOwner)
    ));
    assert!(matches!(
        KidsTitlesView::open(&core.for_profile(&kid), &kid),
        Err(ProfileError::NotOwner)
    ));
    assert!(matches!(
        KidsTitlesView::open(&core, OWNER),
        Err(ProfileError::NotKids)
    ));
    let titles = KidsTitlesView::open(&core, &kid).unwrap();
    let keys = KidsKeysView::open(&core).unwrap();
    assert!(titles.snapshot().titles.is_empty());
    // 1,001 allowed, by two devices past the cap: this one admits 1,000.
    for i in 0..1000 {
        let row = AllowedRow {
            item_id: format!("tt{i:04}"),
            kind: "movie".into(),
            name: format!("T{i}"),
            names: vec![],
            poster: None,
            allowed: true,
            added_ms: i,
        };
        core.library()
            .write_allowed(&kid, &row, &core.hlc())
            .unwrap();
    }
    let other = SyncRecord {
        coll: coll::ALLOWED,
        key: format!("{kid}\u{1f}tt1000"),
        value: serde_json::json!({"profile": kid, "itemId": "tt1000", "kind": "movie",
                                  "name": "T1000", "allowed": true, "addedMs": 1000}),
        hlc: core.hlc(),
    };
    core.library().apply(&[other]).unwrap();
    until("the titles", || titles.snapshot().titles.len() == 1001).await;
    let snapshot = titles.snapshot();
    assert_eq!((snapshot.over_count, snapshot.max), (1, 1000));
    assert_eq!(snapshot.titles[0].id, "tt1000", "newest first");
    assert!(snapshot.titles[0].over && !snapshot.titles[1].over);
    until("the keys", || {
        keys.snapshot()
            .profiles
            .first()
            .is_some_and(|p| !p.keys.is_empty())
    })
    .await;
    let counted = &keys.snapshot().profiles[0];
    assert_eq!(counted.profile, kid);
    assert_eq!(counted.keys.len(), 1000, "the counted titles only");
    assert!(counted.keys.contains(&"movie:tt0000".to_owned()));
    assert!(!counted.keys.contains(&"movie:tt1000".to_owned()));
}

/// A profile whose access record has not arrived reads as kids (fail
/// closed), but no owner view offers it: allowTitle would refuse it.
#[tokio::test]
async fn a_syncing_profile_is_in_no_kids_view() {
    let core = owner().await;
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let arriving = SyncRecord {
        coll: coll::PROFILES,
        key: "y".into(),
        value: serde_json::json!({"profile": "y", "name": "Y"}),
        hlc: core.hlc(),
    };
    core.library().apply(&[arriving]).unwrap();
    assert!(core.library().profile("y").unwrap().unwrap().kids());
    assert!(matches!(
        KidsTitlesView::open(&core, "y"),
        Err(ProfileError::Syncing)
    ));
    let keys = KidsKeysView::open(&core).unwrap();
    let listed: Vec<_> = keys
        .snapshot()
        .profiles
        .into_iter()
        .map(|p| p.profile)
        .collect();
    assert_eq!(listed, [kid]);
}

/// A stand-in Trakt that counts the requests reaching it.
async fn trakt(core: &Core) -> Arc<AtomicUsize> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counted.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 503 X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    )
                    .await;
            });
        }
    });
    core.set_trakt_config(Some(TraktConfig {
        client_id: "client".into(),
        redirect_uri: "https://marquee.example/trakt".into(),
        hosts: TraktHosts {
            api: base.clone(),
            auth: base,
            local: true,
        },
    }));
    hits
}

fn record(account: &str) -> TraktRecord {
    let now = mq_library::hlc::now_ms();
    serde_json::from_value(serde_json::json!({
        "accessToken": "access", "refreshToken": "refresh", "createdAtMs": now,
        "expiresAtMs": now + 7 * 86_400_000, "scope": "public", "account": account,
        "user": "tester", "revision": 0, "generation": 1
    }))
    .unwrap()
}

#[tokio::test]
async fn trakt_is_off_for_kids() {
    let core = owner().await;
    let hits = trakt(&core).await;
    // A kids profile takes no record and runs no round.
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let kid_core = core.for_profile(&kid);
    assert!(kid_core.set_trakt(Some(record("a-kid"))).await.is_err());
    assert!(kid_core.trakt_sync().await.is_err());
    // A profile whose record was taken before it read as kids (a forged or
    // misrouted access record from sync): its scrobbles, rounds and
    // scheduler stop, and its tokens leave memory.
    let x = core.create_profile("X", "owl").unwrap();
    let x_core = core.for_profile(&x);
    x_core.set_trakt(Some(record("a-x"))).await.unwrap();
    until("its first round", || hits.load(Ordering::SeqCst) > 0).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let made_kids = SyncRecord {
        coll: coll::PROFILE_ACCESS,
        key: x.clone(),
        value: serde_json::json!({"profile": x, "kids": true}),
        hlc: core.hlc(),
    };
    core.library().apply(&[made_kids]).unwrap();
    let before = hits.load(Ordering::SeqCst);
    let movie = PlayTarget {
        kind: "movie".into(),
        item_id: "tt1".into(),
        video_id: "tt1".into(),
        name: Some("Film".into()),
        poster: None,
        season: None,
        episode: None,
    };
    let view = PlaybackView::open(&x_core, Vec::new(), movie);
    view.report_progress(10 * 60_000, 100 * 60_000, false);
    assert!(x_core.trakt_sync().await.is_err(), "no round");
    core.trakt_foreground();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(view.snapshot().trakt, TraktPlayback::Off, "no scrobble");
    assert_eq!(hits.load(Ordering::SeqCst), before, "nothing went out");
    let status = mq_runtime::TraktView::open(&x_core).snapshot();
    assert_ne!(
        status.state,
        mq_runtime::TraktState::Connected,
        "its tokens left memory"
    );
    assert_eq!(status.user, None);
    drop(view);
}

/// No code path but `allowTitle` writes an allow (plans/m11-kids.md 18): a
/// names refresh or any other writer could undo a Remove.
#[test]
fn only_allow_title_writes_an_allow() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut writers = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let calls = text.matches(".write_allowed(").count();
            if calls > 0 {
                writers.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    calls,
                ));
            }
        }
    }
    assert_eq!(
        writers,
        [("profiles.rs".to_owned(), 2)],
        "allow and remove only"
    );
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/profiles.rs"),
    )
    .unwrap();
    assert_eq!(text.matches("allowed: true,").count(), 1, "the one Allow");
}
