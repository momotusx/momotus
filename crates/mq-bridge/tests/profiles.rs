//! Profiles over the protocol (plans/m11-profiles.md sections 4, 5, 6 and
//! 9): the permission table, locked sessions, switching, and what happens to
//! replies and views across a switch, a lock or a remote deletion.

use mq_bridge::{Outgoing, Output, Session};
use mq_runtime::{Core, CoreConfig};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

/// A session and the messages it sent: replies by id, pushes kept aside.
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

    async fn recv(&mut self) -> Value {
        tokio::time::timeout(Duration::from_secs(10), self.rx.recv())
            .await
            .expect("a message")
            .expect("open")
            .json()
    }

    /// Sends `request` with a fresh id and waits for its reply.
    async fn call(&mut self, mut request: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        request["id"] = json!(id);
        self.session.handle_text(&request.to_string()).await;
        loop {
            let message = self.recv().await;
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

    /// The error code `request` is refused with.
    async fn code(&mut self, request: Value) -> String {
        let reply = self.call(request.clone()).await;
        assert_eq!(reply["ok"], false, "{request} -> {reply}");
        reply["code"].as_str().unwrap_or("none").to_owned()
    }

    async fn create(&mut self, name: &str) -> String {
        let snapshot = self
            .ok(json!({"op": "createProfile", "name": name, "avatar": "fox:amber"}))
            .await;
        id_of(&snapshot, name)
    }

    async fn switch(&mut self, profile: &str, pin: Option<&str>) -> Value {
        self.ok(json!({"op": "switchProfile", "profile": profile, "pin": pin}))
            .await
    }

    async fn set_pin(&mut self, profile: &str, pin: Option<&str>, current: Option<&str>) -> String {
        let request =
            json!({"op": "setPin", "profile": profile, "pin": pin, "currentPin": current});
        let reply = self.call(request).await;
        match reply["ok"].as_bool() {
            Some(true) => "ok".into(),
            _ => reply["code"].as_str().unwrap_or("none").to_owned(),
        }
    }

    async fn subscribe(&mut self, view: &str) -> u64 {
        self.ok(json!({"op": "subscribe", "view": view})).await["sub"]
            .as_u64()
            .unwrap()
    }

    /// Waits for a push that `done` accepts, keeping the others.
    async fn push_where(&mut self, done: impl Fn(&Value) -> bool) -> Value {
        if let Some(at) = self.pushes.iter().position(&done) {
            return self.pushes.remove(at);
        }
        loop {
            let message = self.recv().await;
            if done(&message) {
                return message;
            }
            self.pushes.push(message);
        }
    }
}

fn id_of(snapshot: &Value, name: &str) -> String {
    snapshot["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("{name} in {snapshot}"))["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn profile<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
    snapshot["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap()
}

#[tokio::test]
async fn every_permission_row_is_enforced_in_the_core() {
    let mut c = Client::new(Core::temporary("default").unwrap());

    // Create, rename and delete others: the owner only.
    let kid = c.create("Kid").await;
    let snapshot = c.ok(json!({"op": "lockSession"})).await;
    assert_eq!(snapshot["locked"], true);
    c.ok(json!({"op": "unlockProfile"})).await;
    assert_eq!(
        c.code(json!({"op": "createProfile", "name": ""})).await,
        "badName"
    );
    let long = "x".repeat(31);
    assert_eq!(
        c.code(json!({"op": "createProfile", "name": long})).await,
        "badName"
    );
    let mut extra = Vec::new();
    for name in ["B", "C", "D", "E"] {
        extra.push(c.create(name).await);
    }
    assert_eq!(
        c.code(json!({"op": "createProfile", "name": "G"})).await,
        "tooMany"
    );
    for id in extra {
        c.ok(json!({"op": "deleteProfile", "profile": id})).await;
    }
    let snapshot = c
        .ok(json!({"op": "updateProfile", "profile": kid, "name": "Kiddo"}))
        .await;
    assert_eq!(profile(&snapshot, &kid)["name"], "Kiddo");
    c.ok(json!({"op": "updateProfile", "profile": "default", "avatar": "owl:teal"}))
        .await;
    let code = c
        .code(json!({"op": "deleteProfile", "profile": "default"}))
        .await;
    assert_eq!(code, "ownerProfile");
    let code = c
        .code(json!({"op": "deleteProfile", "profile": "nobody"}))
        .await;
    assert_eq!(code, "unknownProfile");

    // Another profile: only itself.
    let snapshot = c.switch(&kid, None).await;
    assert_eq!(
        (snapshot["active"].as_str(), &snapshot["locked"]),
        (Some(kid.as_str()), &json!(false))
    );
    assert_eq!(
        c.code(json!({"op": "createProfile", "name": "N"})).await,
        "notOwner"
    );
    let rename_owner = json!({"op": "updateProfile", "profile": "default", "name": "Mine"});
    assert_eq!(c.code(rename_owner).await, "notOwner");
    let code = c
        .code(json!({"op": "deleteProfile", "profile": "default"}))
        .await;
    assert_eq!(code, "notOwner");
    c.ok(json!({"op": "updateProfile", "profile": kid, "name": "Me", "avatar": "cat:rose"}))
        .await;

    // Its own PIN: set freely, change or clear with the current one.
    assert_eq!(c.set_pin(&kid, Some("1234"), None).await, "weakPin");
    assert_eq!(c.set_pin(&kid, Some("1357"), None).await, "ok");
    assert_eq!(c.set_pin(&kid, Some("2468"), None).await, "wrongPin");
    assert_eq!(
        c.set_pin(&kid, Some("2468"), Some("9753")).await,
        "wrongPin"
    );
    assert_eq!(c.set_pin(&kid, Some("2468"), Some("1357")).await, "ok");
    assert_eq!(c.set_pin(&kid, None, None).await, "wrongPin");
    assert_eq!(c.set_pin(&kid, None, Some("2468")).await, "ok");
    assert_eq!(c.set_pin(&kid, Some("2468"), None).await, "ok");
    // Never another profile's, set or cleared, unless the owner clears.
    assert_eq!(c.set_pin("default", Some("1470"), None).await, "notOwner");
    assert_eq!(c.set_pin("default", None, None).await, "notOwner");

    // Switching to Y takes Y's PIN; the owner may clear it, never set it.
    c.switch("default", None).await;
    assert_eq!(c.set_pin(&kid, Some("1470"), None).await, "notOwner");
    let switch_kid = |pin: Option<&str>| json!({"op": "switchProfile", "profile": kid, "pin": pin});
    assert_eq!(c.code(switch_kid(None)).await, "wrongPin");
    assert_eq!(c.code(switch_kid(Some("1357"))).await, "wrongPin");
    c.switch(&kid, Some("2468")).await;
    c.switch("default", None).await;
    assert_eq!(c.set_pin(&kid, None, None).await, "ok");
    let snapshot = c.switch(&kid, None).await;
    assert_eq!(profile(&snapshot, &kid)["hasPin"], false);
    let unknown = json!({"op": "switchProfile", "profile": "nobody"});
    assert_eq!(c.code(unknown).await, "unknownProfile");

    // An unreadable hash fails closed: it can only be cleared, by its own
    // session without the current PIN, or by the owner.
    let core = Core::temporary("default").unwrap();
    let mut o = Client::new(core.clone());
    let kid = o.create("Kid").await;
    core.library()
        .set_pin_hash(&kid, Some("$argon2id$garbage"), &core.hlc())
        .unwrap();
    let switch_kid = json!({"op": "switchProfile", "profile": kid, "pin": "1357"});
    assert_eq!(o.code(switch_kid).await, "pinUnreadable");
    core.library()
        .set_pin_hash(&kid, None, &core.hlc())
        .unwrap();
    o.switch(&kid, None).await;
    core.library()
        .set_pin_hash(&kid, Some("$argon2id$garbage"), &core.hlc())
        .unwrap();
    assert_eq!(
        o.set_pin(&kid, Some("1357"), Some("1357")).await,
        "pinUnreadable"
    );
    assert_eq!(o.set_pin(&kid, None, None).await, "ok");
}

#[tokio::test]
async fn wrong_pins_are_throttled_with_the_wait_in_the_reply() {
    let mut c = Client::new(Core::temporary("default").unwrap());
    let kid = c.create("Kid").await;
    c.switch(&kid, None).await;
    assert_eq!(c.set_pin(&kid, Some("1357"), None).await, "ok");
    c.switch("default", None).await;
    let switch = |pin: &str| json!({"op": "switchProfile", "profile": kid, "pin": pin});
    for _ in 0..4 {
        assert_eq!(c.code(switch("0000")).await, "wrongPin");
    }
    let reply = c.call(switch("1357")).await;
    assert_eq!(reply["code"], "locked", "{reply}");
    let wait = reply["retryAfterMs"].as_u64().unwrap();
    assert!((59_000..=60_000).contains(&wait), "{wait}");
}

/// A relay to sync with while locked.
async fn relay() -> (String, std::path::PathBuf) {
    let dir = temp_dir("relay");
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    (server, dir)
}

fn temp_dir(what: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mq-profiles-{what}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn a_locked_session_answers_only_the_picker_unlocking_and_sync() {
    let (server, dir) = relay().await;
    let core = Core::temporary("default").unwrap();
    let mut c = Client::new(core.clone());
    let record = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "TV"}))
        .await;
    assert_eq!(c.set_pin("default", Some("2580"), None).await, "ok");

    // A new session (a cold start, a reconnect) on a profile with a PIN.
    let mut c = Client::new(core);
    let picker = c.subscribe("profiles").await;
    let refused = [
        json!({"op": "subscribe", "view": "home"}),
        json!({"op": "subscribe", "view": "settings"}),
        json!({"op": "command", "sub": picker, "command": {"type": "freeze"}}),
        json!({"op": "installAddon", "url": "https://a.example/manifest.json"}),
        json!({"op": "removeAddon", "url": "https://a.example/manifest.json"}),
        json!({"op": "moveAddon", "url": "https://a.example/manifest.json", "to": 0}),
        json!({"op": "setAddonEnabled", "url": "https://a.example/manifest.json", "enabled": false}),
        json!({"op": "setPrefs", "patch": {}}),
        json!({"op": "homeCatalogs"}),
        json!({"op": "setHomeRow", "rowId": "x", "hidden": true}),
        json!({"op": "moveHomeRow", "rowId": "x", "by": 1}),
        json!({"op": "createSyncAccount", "server": "https://relay.example", "name": "TV"}),
        json!({"op": "createRecoveryKit", "record": record}),
        json!({"op": "syncDevices", "record": record}),
        json!({"op": "signOut", "record": record}),
        json!({"op": "createProfile", "name": "N"}),
        json!({"op": "updateProfile", "profile": "default", "name": "N"}),
        json!({"op": "deleteProfile", "profile": "x"}),
        json!({"op": "setPin", "profile": "default", "pin": null, "currentPin": "2580"}),
        json!({"op": "lockSession"}),
    ];
    for request in refused {
        assert_eq!(c.code(request.clone()).await, "locked", "{request}");
    }
    c.ok(json!({"op": "ping"})).await;
    // Debrid keys are the device's, handed over at start, locked or not.
    let keys = json!({"op": "setDebridKeys", "keys": [["realdebrid", "k"]]});
    assert_eq!(c.ok(keys).await["stores"], 1);
    c.ok(json!({"op": "lifecycle", "event": "background"}))
        .await;
    let report = c.ok(json!({"op": "syncNow", "record": record})).await;
    assert!(report["pushed"].as_u64().is_some(), "{report}");

    assert_eq!(
        c.code(json!({"op": "unlockProfile", "pin": "1111"})).await,
        "wrongPin"
    );
    let snapshot = c.ok(json!({"op": "unlockProfile", "pin": "2580"})).await;
    assert_eq!(snapshot["locked"], false);
    let pushed = c
        .push_where(|m| {
            m["sub"] == picker
                && m["updates"].as_array().unwrap().last().unwrap()["locked"] == false
        })
        .await;
    assert_eq!(pushed["updates"][0]["active"], "default");
    c.subscribe("home").await;
    c.ok(json!({"op": "unsubscribe", "sub": picker})).await;
    let _ = std::fs::remove_dir_all(dir);
}

async fn picker_snapshot(c: &mut Client) -> Value {
    let reply = c.ok(json!({"op": "subscribe", "view": "profiles"})).await;
    c.ok(json!({"op": "unsubscribe", "sub": reply["sub"]}))
        .await;
    reply["snapshot"].clone()
}

#[tokio::test]
async fn a_cold_start_resumes_the_active_profile_locked_and_a_stale_one_falls_back() {
    let dir = temp_dir("data");
    let open = |hint: &str| {
        Core::open(CoreConfig {
            data_dir: dir.clone(),
            profile_id: hint.into(),
            device_id: 0,
        })
        .unwrap()
    };
    let kid = {
        let mut c = Client::new(open("default"));
        let kid = c.create("Kid").await;
        c.switch(&kid, None).await;
        assert_eq!(c.set_pin(&kid, Some("1357"), None).await, "ok");
        kid
    };

    // A restart with the app's usual hint: the device's choice wins, and
    // it is locked.
    let core = open("default");
    assert_eq!(core.profile(), kid);
    let mut c = Client::new(core.clone());
    let snapshot = picker_snapshot(&mut c).await;
    assert_eq!(snapshot["active"], kid.as_str());
    assert_eq!(snapshot["locked"], true);
    let home = json!({"op": "subscribe", "view": "home"});
    assert_eq!(c.code(home).await, "locked");
    c.switch("default", None).await;
    c.ok(json!({"op": "deleteProfile", "profile": kid})).await;
    drop(c);

    // A stored id that is gone, or never existed, opens the owner, and no
    // profile is made from it.
    for stale in [kid.as_str(), "nobody"] {
        core.library().set_active_profile(stale).unwrap();
        let mut c = Client::new(core.clone());
        let snapshot = picker_snapshot(&mut c).await;
        assert_eq!(snapshot["active"], "default", "{stale}");
        assert_eq!(snapshot["locked"], false, "{stale}");
        assert_eq!(open(stale).profile(), "default", "{stale}");
        assert!(core.library().profile(stale).unwrap().is_none());
    }
    drop(core);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn switching_is_refused_while_views_other_than_the_picker_are_open() {
    let core = Core::temporary("default").unwrap();
    let mut c = Client::new(core.clone());
    let kid = c.create("Kid").await;
    let home = c.subscribe("home").await;
    let addons = c.subscribe("addons").await;
    let picker = c.subscribe("profiles").await;
    // The device's debrid accounts are not in the way: a switch keeps them.
    c.subscribe("debrid").await;
    let reply = c.call(json!({"op": "switchProfile", "profile": kid})).await;
    assert_eq!(reply["code"], "viewsOpen", "{reply}");
    let mut views: Vec<&str> = reply["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    views.sort();
    assert_eq!(views, ["addons", "home"]);

    for sub in [home, addons] {
        c.ok(json!({"op": "unsubscribe", "sub": sub})).await;
    }
    c.switch(&kid, None).await;
    // The picker stayed open and shows the switch.
    let pushed = c
        .push_where(|m| m["sub"] == picker && m["updates"][0]["active"] == kid.as_str())
        .await;
    assert_eq!(pushed["updates"][0]["locked"], false);
    // The device remembers it: the next session opens there.
    let mut again = Client::new(core);
    assert_eq!(picker_snapshot(&mut again).await["active"], kid.as_str());
}

#[tokio::test]
async fn a_new_device_calls_its_owner_main() {
    let core = Core::temporary("default").unwrap();
    let mut c = Client::new(core);
    let snapshot = picker_snapshot(&mut c).await;
    assert_eq!(profile(&snapshot, "default")["name"], "Main", "{snapshot}");
}

#[tokio::test]
async fn the_owner_pin_resets_without_it_even_while_locked() {
    let core = Core::temporary("default").unwrap();
    let mut c = Client::new(core.clone());
    assert_eq!(c.set_pin("default", Some("2580"), None).await, "ok");
    c.ok(json!({"op": "lockSession"})).await;
    // Locked: the PIN is forgotten, and the reset still answers.
    let snapshot = c.ok(json!({"op": "resetOwnerPin"})).await;
    assert_eq!(profile(&snapshot, "default")["hasPin"], false, "{snapshot}");
    c.ok(json!({"op": "unlockProfile"})).await;
    // The next cold start opens unlocked.
    let mut again = Client::new(core);
    assert_eq!(picker_snapshot(&mut again).await["locked"], false);
}

/// An addon host that holds each request a moment, then hangs up.
async fn slow_addon() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                drop(socket);
            });
        }
    });
    base
}

#[tokio::test]
async fn work_started_before_a_switch_or_a_lock_answers_switched() {
    let base = slow_addon().await;
    let core = Core::temporary("default").unwrap();
    let mut c = Client::new(core.clone());
    let kid = c.create("Kid").await;
    let install = json!({"op": "installAddon", "url": format!("{base}/manifest.json")});

    // Unchanged: its own answer (here, a failure of its own).
    let reply = c.call(install.clone()).await;
    assert_eq!(
        (&reply["ok"], &reply["code"]),
        (&json!(false), &Value::Null),
        "{reply}"
    );

    // A lock lands while it runs.
    let started = c.next_id;
    c.session
        .handle_text(
            &json!({"id": started, "op": "installAddon", "url": install["url"]}).to_string(),
        )
        .await;
    c.next_id += 1;
    c.ok(json!({"op": "lockSession"})).await;
    let reply = c.push_where(|m| m["id"] == started).await;
    assert_eq!(reply["code"], "switched", "{reply}");

    // And a switch.
    c.ok(json!({"op": "unlockProfile"})).await;
    let started = c.next_id;
    c.session
        .handle_text(
            &json!({"id": started, "op": "installAddon", "url": install["url"]}).to_string(),
        )
        .await;
    c.next_id += 1;
    c.switch(&kid, None).await;
    let reply = c.push_where(|m| m["id"] == started).await;
    assert_eq!(reply["code"], "switched", "{reply}");

    // A view closed by a lock pushes nothing more, even when its profile's
    // data changes.
    c.switch("default", None).await;
    let library = c.subscribe("library").await;
    c.ok(json!({"op": "lockSession"})).await;
    // Whatever it sent before the lock answered is old news.
    c.pushes.retain(|m| m["sub"] != library);
    let item = mq_library::LibraryItemRow {
        item_id: "tt1".into(),
        kind: "movie".into(),
        name: "One".into(),
        poster: None,
        poster_shape: None,
        added_ms: 1,
    };
    core.library()
        .upsert_item("default", &item, true, &core.hlc())
        .unwrap();
    c.ok(json!({"op": "unlockProfile"})).await;
    let late = tokio::time::timeout(
        Duration::from_millis(300),
        c.push_where(|m| m["sub"] == library),
    )
    .await;
    assert!(late.is_err(), "pushed after the lock: {late:?}");
}

#[tokio::test]
async fn a_remote_deletion_locks_the_session_running_as_that_profile() {
    let core = Core::temporary("default").unwrap();
    let mut owner = Client::new(core.clone());
    let kid = owner.create("Kid").await;
    let other = owner.create("Other").await;
    drop(owner);
    let mut c = Client::new(core.clone());
    c.switch(&kid, None).await;
    let picker = c.subscribe("profiles").await;
    let home = c.subscribe("home").await;

    // Another profile removed: only the notice.
    let gone = |id: &str| mq_library::SyncRecord {
        coll: mq_library::coll::PROFILE_GONE,
        key: id.into(),
        value: json!({"profile": id}),
        hlc: core.hlc(),
    };
    core.library().apply(&[gone(&other)]).unwrap();
    c.push_where(|m| m["profile"] == "gone" && m["id"] == other.as_str())
        .await;
    c.subscribe("search").await;

    // Its own profile, removed on another device: told, locked, its views
    // closed.
    core.library().apply(&[gone(&kid)]).unwrap();
    c.push_where(|m| m["profile"] == "gone" && m["id"] == kid.as_str())
        .await;
    let locked = |m: &Value| {
        m["sub"] == picker && m["updates"].as_array().unwrap().last().unwrap()["locked"] == true
    };
    c.push_where(locked).await;
    assert_eq!(
        c.code(json!({"op": "subscribe", "view": "home"})).await,
        "locked"
    );
    assert_eq!(
        c.code(json!({"op": "unlockProfile"})).await,
        "unknownProfile"
    );
    // Only the picker is left open, so a switch goes through.
    c.switch("default", None).await;
    let reply = c
        .call(json!({"op": "command", "sub": home, "command": {"type": "setQuery"}}))
        .await;
    assert_eq!(reply["error"], format!("no subscription {home}"), "{reply}");
}

#[tokio::test]
async fn a_new_session_names_every_profile_removed_before_it() {
    let core = Core::temporary("default").unwrap();
    let mut removed = Vec::new();
    for name in ["A", "B"] {
        let id = core.create_profile(name, "fox").unwrap();
        core.delete_profile(&id).unwrap();
        removed.push(id);
    }
    let mut c = Client::new(core);
    let mut named = Vec::new();
    for _ in 0..2 {
        let push = c.recv().await;
        assert_eq!(push["profile"], "gone", "{push}");
        named.push(push["id"].as_str().unwrap().to_owned());
    }
    named.sort();
    removed.sort();
    assert_eq!(named, removed);
}
