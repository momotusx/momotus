//! Incognito over the protocol (plans/batch-10-incognito.md 2.7 and 7.1):
//! entry and its PIN rule, the refusals (wall 3), Leave, the ends, a new
//! session behind a live scope, a relaunch and the relay (wall 1).

use mq_bridge::{Outgoing, Output, Session};
use mq_library::OWNER;
use mq_runtime::{Core, CoreConfig};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

struct Client {
    session: Session,
    rx: mpsc::UnboundedReceiver<Outgoing>,
    next_id: u64,
    pushes: Vec<Value>,
}

impl Client {
    fn new(core: Core) -> Self {
        let (tx, rx) = Output::channel();
        Self {
            session: Session::new(core, tx),
            rx,
            next_id: 1,
            pushes: Vec::new(),
        }
    }

    async fn call(&mut self, mut request: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        request["id"] = json!(id);
        self.session.handle_text(&request.to_string()).await;
        loop {
            let message = tokio::time::timeout(Duration::from_secs(10), self.rx.recv())
                .await
                .expect("a message")
                .expect("open")
                .json();
            if message["id"] == json!(id) {
                return message;
            }
            self.pushes.push(message);
        }
    }

    async fn ok(&mut self, request: Value) -> Value {
        let reply = self.call(request.clone()).await;
        assert_eq!(reply["ok"], true, "{request} -> {reply}");
        reply["result"].clone()
    }

    async fn code(&mut self, request: Value) -> String {
        let reply = self.call(request.clone()).await;
        assert_eq!(reply["ok"], false, "{request} -> {reply}");
        reply["code"].as_str().unwrap_or("none").to_owned()
    }

    fn enter(pin: Option<&str>, from_chooser: bool) -> Value {
        json!({"op": "switchProfile", "profile": "incognito", "pin": pin, "fromChooser": from_chooser})
    }

    async fn snapshot(&mut self) -> Value {
        self.ok(json!({"op": "ackIncognitoEnded"})).await
    }

    /// Pushes so far, and any waiting now.
    async fn pushed(&mut self, push: Value) -> usize {
        tokio::time::sleep(Duration::from_millis(50)).await;
        while let Ok(message) = self.rx.try_recv() {
            self.pushes.push(message.json());
        }
        self.pushes.iter().filter(|p| **p == push).count()
    }
}

fn ended() -> Value {
    json!({"incognito": "ended"})
}

/// The owner with an addon list (an empty one: nothing is fetched).
fn device() -> Core {
    let core = Core::temporary(OWNER).unwrap();
    give_list(&core, OWNER);
    core
}

fn give_list(core: &Core, profile: &str) {
    let url = "http://127.0.0.1:9/manifest.json";
    let row = mq_library::AddonRow {
        transport_url: url.into(),
        manifest_json: r#"{"id":"x","version":"1.0.0","name":"X","resources":[],"types":[]}"#
            .into(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    core.library()
        .set_addons(profile, std::slice::from_ref(&row), core.clock())
        .unwrap();
    core.library()
        .set_addons(profile, &[], core.clock())
        .unwrap();
}

/// A device with the owner's PIN, a kids profile and a standard one.
async fn kids_device() -> (Core, String) {
    let core = device();
    core.set_pin(OWNER, Some("1357"), None).await.unwrap();
    core.create_kids_profile("Kid", "fox").unwrap();
    let ada = core.create_profile("Ada", "fox").unwrap();
    give_list(&core, &ada);
    (core, ada)
}

#[tokio::test]
async fn entry_from_the_menu_needs_no_pin_without_kids() {
    let core = device();
    let mut c = Client::new(core.clone());
    let entered = c.ok(Client::enter(None, false)).await;
    assert_eq!(entered["active"], "incognito");
    assert_eq!(entered["incognitoHost"], OWNER);
    assert_eq!(entered["locked"], false);
    assert_eq!(entered["profiles"].as_array().unwrap().len(), 1);
    assert_eq!(
        core.library().active_profile().unwrap().as_deref(),
        Some(OWNER)
    );
    assert_eq!(c.ok(json!({"op": "ping"})).await, json!({"liveScopes": 1}));
}

/// On a device with kids, a host without its own PIN asks the owner's; a
/// host with its own PIN asks none; the snapshot names whose (B P2-4).
#[tokio::test]
async fn on_a_kids_device_the_owners_pin_is_asked_unless_the_host_has_one() {
    let (core, ada) = kids_device().await;
    let mut c = Client::new(core.clone());
    c.ok(json!({"op": "unlockProfile", "pin": "1357"})).await;
    c.ok(json!({"op": "switchProfile", "profile": ada})).await;
    assert_eq!(c.snapshot().await["incognitoPin"], OWNER);
    assert_eq!(c.code(Client::enter(None, false)).await, "wrongPin");
    let entered = c.ok(Client::enter(Some("1357"), false)).await;
    assert_eq!(entered["incognitoHost"], ada);
    c.ok(json!({"op": "leaveIncognito"})).await;

    c.ok(json!({"op": "setPin", "profile": ada, "pin": "2468"}))
        .await;
    assert_eq!(c.snapshot().await["incognitoPin"], Value::Null);
    c.ok(Client::enter(None, false)).await;
    assert_eq!(
        core.library().active_profile().unwrap().as_deref(),
        Some(ada.as_str())
    );
}

/// From a locked session: the owner's PIN, a wrong one counted by the
/// throttle; the host is the owner. From kids: `kids`. With views open:
/// `viewsOpen`.
#[tokio::test]
async fn entry_from_a_locked_session_kids_and_open_views() {
    let (core, _ada) = kids_device().await;
    let mut c = Client::new(core.clone());
    assert_eq!(c.snapshot().await["incognitoPin"], OWNER);
    for _ in 0..4 {
        assert_eq!(c.code(Client::enter(Some("0000"), true)).await, "wrongPin");
    }
    let throttled = c.call(Client::enter(Some("1357"), true)).await;
    assert_eq!(throttled["code"], "locked", "{throttled}");

    let core = device();
    core.set_pin(OWNER, Some("1357"), None).await.unwrap();
    let kid = core.create_kids_profile("Kid", "fox").unwrap();
    let mut c = Client::new(core.clone());
    let entered = c.ok(Client::enter(Some("1357"), true)).await;
    assert_eq!(entered["incognitoHost"], OWNER);
    c.ok(json!({"op": "leaveIncognito"})).await;
    c.ok(json!({"op": "unlockProfile", "pin": "1357"})).await;
    c.ok(json!({"op": "switchProfile", "profile": kid})).await;
    assert_eq!(c.code(Client::enter(Some("1357"), false)).await, "kids");

    let mut c = Client::new(device());
    c.ok(json!({"op": "subscribe", "view": "settings"})).await;
    assert_eq!(c.code(Client::enter(None, false)).await, "viewsOpen");
}

/// Wall 3: in Incognito the account, Trakt, profile-management, import and
/// debrid-write ops and views answer `incognito`; what the apps send there
/// does not.
#[tokio::test]
async fn incognito_refuses_the_device_ops_and_views() {
    let mut c = Client::new(device());
    c.ok(Client::enter(None, false)).await;
    let refused = [
        json!({"op": "createProfile", "name": "X", "avatar": "fox"}),
        json!({"op": "deleteProfile", "profile": "x"}),
        json!({"op": "setPin", "profile": "default", "pin": "1357"}),
        json!({"op": "updateProfile", "profile": "default", "name": "X"}),
        json!({"op": "resetOwnerPin"}),
        json!({"op": "unlockProfile"}),
        json!({"op": "setTrakt", "record": null}),
        json!({"op": "traktSync"}),
        json!({"op": "traktSignOut"}),
        json!({"op": "setDebridKey", "service": "realdebrid", "key": "k"}),
        json!({"op": "disconnectDebrid", "service": "realdebrid", "generation": "g"}),
        json!({"op": "setDownloadSettings", "wifiOnly": true}),
        json!({"op": "defaultAddons"}),
        json!({"op": "installDefaults", "choices": []}),
        json!({"op": "firstRunWall"}),
        json!({"op": "finishProfileStep", "name": "", "avatar": ""}),
        json!({"op": "profilesLastWatched"}),
        json!({"op": "relayInfo", "server": "http://127.0.0.1:9"}),
        json!({"op": "createSyncAccount", "server": "http://127.0.0.1:9", "name": "X"}),
        json!({"op": "readPairingCode", "qr": "x"}),
        json!({"op": "subscribe", "view": "trakt"}),
        json!({"op": "subscribe", "view": "traktLogin"}),
        json!({"op": "subscribe", "view": "import"}),
        json!({"op": "subscribe", "view": "pairing", "params": {"server": "x"}}),
        json!({"op": "subscribe", "view": "debridLogin", "params": {"service": "realdebrid"}}),
        json!({"op": "subscribe", "view": "signIn"}),
        json!({"op": "subscribe", "view": "anything"}),
    ];
    for request in refused {
        assert_eq!(c.code(request.clone()).await, "incognito", "{request}");
    }
    let answered = [
        json!({"op": "ping"}),
        json!({"op": "languages"}),
        json!({"op": "streamConditions"}),
        json!({"op": "homeCatalogs"}),
        json!({"op": "setPrefs", "patch": {"autoplayNext": false}}),
        json!({"op": "lifecycle", "event": "idle"}),
        json!({"op": "lifecycle", "event": "active"}),
        json!({"op": "downloadsAllowed", "allowed": true}),
        json!({"op": "subscribe", "view": "settings"}),
        json!({"op": "subscribe", "view": "addons"}),
        json!({"op": "subscribe", "view": "library"}),
        json!({"op": "subscribe", "view": "search"}),
        json!({"op": "subscribe", "view": "downloads"}),
        json!({"op": "subscribe", "view": "debrid"}),
    ];
    for request in answered {
        c.ok(request).await;
    }
    // No command reaches the views that outlive a switch.
    let picker = c.ok(json!({"op": "subscribe", "view": "profiles"})).await;
    let sub = picker["sub"].clone();
    let command =
        json!({"op": "command", "sub": sub, "command": {"type": "setQuery", "query": "x"}});
    let reply = c.call(command).await;
    assert_eq!(reply["ok"], false, "{reply}");
}

/// Leave: from a profile, unlocked; from the chooser, locked with no lock
/// line for a host without a PIN; with the host removed, the owner, locked.
#[tokio::test]
async fn leave_goes_back_where_incognito_was_entered() {
    let core = device();
    let mut c = Client::new(core.clone());
    c.ok(Client::enter(None, false)).await;
    let left = c.ok(json!({"op": "leaveIncognito"})).await;
    assert_eq!(
        (left["active"].clone(), left["locked"].clone()),
        (json!(OWNER), json!(false))
    );
    assert_eq!(left["incognitoHost"], Value::Null);
    assert_eq!(c.pushed(ended()).await, 0, "a Leave says nothing");

    c.ok(Client::enter(None, true)).await;
    let left = c.ok(json!({"op": "leaveIncognito"})).await;
    assert_eq!(
        (left["active"].clone(), left["locked"].clone()),
        (json!(OWNER), json!(true))
    );
    let owner = &left["profiles"][0];
    assert_eq!(owner["hasPin"], false, "no lock line: {left}");
    assert_eq!(left["incognitoEnded"], false);
    // Pressed once more as the clock ended it: the snapshot, no error.
    c.ok(json!({"op": "leaveIncognito"})).await;

    c.ok(json!({"op": "unlockProfile"})).await;
    let ada = core.create_profile("Ada", "fox").unwrap();
    give_list(&core, &ada);
    c.ok(json!({"op": "switchProfile", "profile": ada})).await;
    c.ok(Client::enter(None, false)).await;
    core.delete_profile(&ada).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after = c.ok(json!({"op": "leaveIncognito"})).await;
    assert_eq!(
        (after["active"].clone(), after["locked"].clone()),
        (json!(OWNER), json!(true))
    );
}

/// `lockSession` ends it at the host's chooser with the push; a switch to
/// another profile ends it quietly; the scope folder goes either way.
#[tokio::test]
async fn a_lock_and_a_switch_end_it() {
    let core = device();
    let ada = core.create_profile("Ada", "fox").unwrap();
    let mut c = Client::new(core.clone());
    c.ok(Client::enter(None, false)).await;
    let locked = c.ok(json!({"op": "lockSession"})).await;
    assert_eq!(
        (locked["active"].clone(), locked["locked"].clone()),
        (json!(OWNER), json!(true))
    );
    assert_eq!(locked["incognitoEnded"], true);
    assert_eq!(c.pushed(ended()).await, 1);
    assert_eq!(core.live_scope_count(), 0);

    c.ok(json!({"op": "unlockProfile"})).await;
    c.ok(Client::enter(None, false)).await;
    let switched = c.ok(json!({"op": "switchProfile", "profile": ada})).await;
    assert_eq!(switched["active"], ada.as_str());
    assert_eq!(switched["incognitoEnded"], false);
    assert_eq!(c.pushed(ended()).await, 1, "no second push");
    assert_eq!(core.live_scope_count(), 0);
}

/// A dropped session ends its scope with no async runtime at hand (A
/// P3-7): the count drops and the next session shows the ended line.
#[tokio::test]
async fn a_dropped_session_ends_it_without_a_runtime() {
    let core = device();
    let mut c = Client::new(core.clone());
    c.ok(Client::enter(None, false)).await;
    assert_eq!(core.live_scope_count(), 1);
    std::thread::spawn(move || drop(c)).join().unwrap();
    assert_eq!(core.live_scope_count(), 0);
    assert!(*core.incognito_ended_unseen().borrow());
    let mut next = Client::new(core);
    let view = next
        .ok(json!({"op": "subscribe", "view": "profiles"}))
        .await;
    assert_eq!(view["snapshot"]["incognitoEnded"], true, "{view}");
    let snapshot = next.snapshot().await;
    assert_eq!(snapshot["incognitoEnded"], false, "the ack read it");
}

/// A session started behind a live scope starts at the chooser and hears
/// that scope's end (C P2-6).
#[tokio::test]
async fn a_session_behind_a_live_scope_starts_locked_and_hears_its_end() {
    let core = device();
    let mut a = Client::new(core.clone());
    a.ok(Client::enter(None, false)).await;
    let mut b = Client::new(core.clone());
    let view = b.ok(json!({"op": "subscribe", "view": "profiles"})).await;
    assert_eq!(view["snapshot"]["locked"], true, "{view}");
    a.ok(json!({"op": "lockSession"})).await;
    assert_eq!(b.pushed(ended()).await, 1);
    let snapshot = b.ok(json!({"op": "ping"})).await;
    assert_eq!(snapshot, json!({"liveScopes": 0}));
}

/// A relaunch after every handle dropped opens the last normal profile,
/// with nothing of Incognito on disk.
#[tokio::test]
async fn a_relaunch_opens_the_last_normal_profile() {
    let dir = tempfile_dir("relaunch");
    let config = CoreConfig {
        data_dir: dir.clone(),
        profile_id: OWNER.into(),
        device_id: 0,
    };
    {
        let core = Core::open(config.clone()).unwrap();
        give_list(&core, OWNER);
        let mut c = Client::new(core);
        c.ok(Client::enter(None, false)).await;
    }
    let core = Core::open(config).unwrap();
    let mut c = Client::new(core.clone());
    let snapshot = c.snapshot().await;
    assert_eq!(
        (snapshot["active"].clone(), snapshot["locked"].clone()),
        (json!(OWNER), json!(false))
    );
    assert_eq!(c.ok(json!({"op": "ping"})).await, json!({"liveScopes": 0}));
    let left: Vec<_> = std::fs::read_dir(dir.join("incognito"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();
    assert!(left.is_empty(), "{left:?}");
    let _ = std::fs::remove_dir_all(dir);
}

fn tempfile_dir(what: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mq-incognito-{what}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Wall 1: `syncNow` from Incognito pushes the device's own edits, and the
/// relay holds nothing of Incognito: a second device pulls none of it.
#[tokio::test]
async fn sync_from_incognito_sends_only_the_devices_edits() {
    let dir = tempfile_dir("relay");
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });

    let mut a = Client::new(device());
    let record = a
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "Mac"}))
        .await;
    a.ok(json!({"op": "setPrefs", "patch": {"subtitleLanguages": ["tur"]}}))
        .await;
    a.ok(Client::enter(None, false)).await;
    a.ok(json!({"op": "setPrefs", "patch": {"subtitleLanguages": ["fra"], "autoplayNext": false}}))
        .await;
    // Positive control: the scope holds the edit.
    let inside = a.ok(json!({"op": "setPrefs", "patch": {}})).await;
    assert_eq!(inside["subtitleLanguages"], json!(["fra"]));
    let report = a.ok(json!({"op": "syncNow", "record": record})).await;
    assert!(report["pushed"].as_u64().unwrap() >= 1, "{report}");

    let b_core = Core::temporary(OWNER).unwrap();
    let mut b = Client::new(b_core.clone());
    b.ok(json!({"op": "syncNow", "record": record})).await;
    assert_eq!(b_core.prefs().subtitle_languages, ["tur"]);
    assert!(b_core.prefs().autoplay_next, "nothing of Incognito");
    let profiles: Vec<String> = b_core
        .library()
        .profiles()
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert!(!profiles.iter().any(|p| p == "incognito"), "{profiles:?}");
    a.ok(json!({"op": "leaveIncognito"})).await;
    let _ = std::fs::remove_dir_all(dir);
}
