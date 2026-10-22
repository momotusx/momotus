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
    // The account holds every profile: refused before anything reaches a relay.
    let record = json!({
        "server": "https://relay.invalid", "deviceToken": "t", "member": "01".repeat(32),
        "keys": {"v": 1, "account": "02".repeat(16), "genesis": "03".repeat(32), "epoch": 0,
                 "ak": "04".repeat(32), "idKey": "05".repeat(32)},
        "checkpoint": {"seq": 0, "hash": "06".repeat(32)}, "relayId": "07".repeat(16), "revision": 0,
    });
    let code = c
        .code(json!({"op": "deleteAccount", "record": record}))
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
        json!({"op": "subscribe", "view": "addonCatalog"}),
        json!({"op": "installListed", "key": "https://a.example/manifest.json"}),
        json!({"op": "setPrefs", "patch": {}}),
        json!({"op": "homeCatalogs"}),
        json!({"op": "setHomeRow", "rowId": "x", "hidden": true}),
        json!({"op": "moveHomeRow", "rowId": "x", "by": 1}),
        json!({"op": "createSyncAccount", "server": "https://relay.example", "name": "TV"}),
        json!({"op": "createRecoveryKit", "record": record}),
        json!({"op": "syncDevices", "record": record}),
        json!({"op": "signOut", "record": record}),
        json!({"op": "deleteAccount", "record": record}),
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

/// Sign out with "Remove your profiles and library from this device" (D13,
/// plans/accounts.md 3.3) runs in three steps the app repeats until done:
/// leave; sign each profile out of Trakt, drop the keystore entries and
/// disconnect each debrid account, tombstone first; then a wipe without a
/// record, the owner's to send. A restart between any two steps ends the
/// same way, and the wipe waits while a debrid key or a Trakt sign-in of the
/// previous owner is left.
#[tokio::test]
async fn a_sign_out_with_wipe_leaves_the_device_as_new() {
    let dir = temp_dir("wipe");
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let open = || {
        let core = Core::open(CoreConfig {
            data_dir: dir.join("device"),
            profile_id: "default".into(),
            device_id: 0,
        })
        .unwrap();
        // Debrid calls go nowhere: only the connection's record matters here.
        core.set_debrid_origin("http://127.0.0.1:9");
        core
    };

    let core = open();
    let device_id = core.library().sync_state("device_id").unwrap();
    assert!(device_id.is_some());
    let mut c = Client::new(core.clone());
    let record = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "Phone"}))
        .await;
    let kid = c.create("Kid").await;
    let generation = c
        .ok(json!({"op": "setDebridKey", "service": "realdebrid", "key": "k"}))
        .await["generation"]
        .clone();
    // The record the app keeps in its keystore.
    let key = json!({"service": "realdebrid", "generation": generation, "revision": 1,
        "credential": {"kind": "key", "key": "k"}});
    let synced = c.ok(json!({"op": "syncNow", "record": record})).await;
    let record = match &synced["record"] {
        Value::Null => record,
        stored => stored.clone(),
    };
    // Another profile never wipes the device (D13).
    c.switch(&kid, None).await;
    assert_eq!(
        c.code(json!({"op": "signOut", "wipe": true})).await,
        "notOwner"
    );
    c.switch("default", None).await;
    // A device that still syncs leaves first, and the wipe never rides on
    // the leave.
    assert_eq!(
        c.code(json!({"op": "signOut", "wipe": true})).await,
        "wrongRecord"
    );
    c.code(json!({"op": "signOut", "record": record, "wipe": true}))
        .await;
    assert!(
        core.library().in_sync_account().unwrap(),
        "it did not leave"
    );

    // Step 1, then the app stops.
    c.ok(json!({"op": "signOut", "record": record})).await;
    drop((c, core));

    // The restart gets the debrid record back with no tombstone yet: the
    // wipe that takes debrid waits, and wipes nothing.
    let core = open();
    let mut c = Client::new(core.clone());
    c.ok(json!({"op": "restoreDebrid", "records": [key], "tombstones": []}))
        .await;
    assert_eq!(
        c.code(json!({"op": "signOut", "wipe": true, "disconnectDebrid": true}))
            .await,
        "debridConnected"
    );
    assert_eq!(core.library().profiles().unwrap().len(), 2);
    // Step 2 wrote the tombstone; the app stops before `disconnectDebrid`.
    drop((c, core));

    let core = open();
    let mut c = Client::new(core.clone());
    let tombstone = json!({"service": "realdebrid", "generation": key["generation"]});
    let restored = c
        .ok(json!({"op": "restoreDebrid", "records": [key], "tombstones": [tombstone]}))
        .await;
    assert_eq!(restored["refused"], json!([key["generation"]]));
    // Kid is still signed in to Trakt on this device: the wipe waits until
    // the owner signs it out (only the owner may, for another profile).
    let signed_in = format!("trakt.device.{}", core.clock().device());
    core.library()
        .set_setting(&kid, &signed_in, "\"kid-account\"", core.clock())
        .unwrap();
    assert_eq!(
        c.code(json!({"op": "signOut", "wipe": true, "disconnectDebrid": true}))
            .await,
        "traktConnected"
    );
    c.switch(&kid, None).await;
    assert_eq!(
        c.code(json!({"op": "traktSignOut", "profile": "default"}))
            .await,
        "notOwner"
    );
    c.switch("default", None).await;
    c.ok(json!({"op": "traktSignOut", "profile": kid})).await;
    // Step 3.
    assert_eq!(
        c.ok(json!({"op": "signOut", "wipe": true, "disconnectDebrid": true}))
            .await,
        json!({})
    );
    for id in ["default".to_owned(), kid] {
        c.push_where(|m| m["profile"] == "gone" && m["id"] == json!(id))
            .await;
    }
    let left: Vec<String> = core
        .library()
        .profiles()
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(left, ["default"]);
    assert!(!core.library().in_sync_account().unwrap());
    assert_eq!(core.library().sync_state("device_id").unwrap(), device_id);
    assert!(
        core.addons_fresh(),
        "the recommended addons come first again"
    );
    let debrid = c.ok(json!({"op": "subscribe", "view": "debrid"})).await;
    assert_eq!(
        debrid["snapshot"],
        json!({"accounts": [], "pending": []}),
        "{debrid}"
    );
    drop((c, core));
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the profiles view says of a removal: `{debrid}` while one waits.
async fn wipe_pending(c: &mut Client) -> Value {
    c.ok(json!({"op": "subscribe", "view": "profiles"})).await["snapshot"]["wipePending"].clone()
}

/// D13's removal is the core's to remember (phase 2 web reviews, A P2-1 and
/// B P2-1): `signOut {record, removeAfter}` records it before the leave goes
/// out, so an app closed or crashed while the leave runs finds it in the
/// profiles view at its next start. A leave that fails clears it, and so does
/// any join, so a later plain sign-out never wipes unasked; the wipe ends it.
#[tokio::test]
async fn a_removal_is_recorded_before_the_leave_and_ends_with_the_wipe_or_a_join() {
    let dir = temp_dir("removal");
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    // A relay that takes the connection and never answers: a leave that is still running.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hanging = format!("http://{}", silent.local_addr().unwrap());
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = silent.accept().await {
            held.push(socket);
        }
    });
    let open = || {
        Core::open(CoreConfig {
            data_dir: dir.join("device"),
            profile_id: "default".into(),
            device_id: 0,
        })
        .unwrap()
    };
    let core = open();
    let mut c = Client::new(core.clone());
    let record = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "Phone"}))
        .await;
    assert_eq!(wipe_pending(&mut c).await, Value::Null);

    // Another profile neither asks nor records a removal (D13); a plain sign-out stays open to it.
    let kid = c.create("Kid").await;
    c.switch(&kid, None).await;
    let remove = json!({"debrid": true});
    assert_eq!(
        c.code(json!({"op": "signOut", "record": record, "removeAfter": remove}))
            .await,
        "notOwner"
    );
    c.switch("default", None).await;
    assert_eq!(wipe_pending(&mut c).await, Value::Null);

    // A leave that fails (a record that is not this device's) keeps the account, and the
    // removal is off.
    let mut other = record.clone();
    other["member"] = json!("11".repeat(32));
    assert_eq!(
        c.code(json!({"op": "signOut", "record": other, "local": true, "removeAfter": remove}))
            .await,
        "wrongRecord"
    );
    assert_eq!(wipe_pending(&mut c).await, Value::Null);

    // Signed out with the removal (without the debrid connections, as asked): it waits
    // through a restart until a join or the wipe ends it.
    c.ok(
        json!({"op": "signOut", "record": record, "force": true, "removeAfter": {"debrid": false}}),
    )
    .await;
    drop((c, core));
    let core = open();
    let mut c = Client::new(core.clone());
    assert_eq!(wipe_pending(&mut c).await, json!({"debrid": false}));

    // A join since: the removal is off, and a later plain sign-out keeps the library.
    let record = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "Phone"}))
        .await;
    assert_eq!(wipe_pending(&mut c).await, Value::Null);
    c.ok(json!({"op": "signOut", "record": record, "force": true}))
        .await;
    drop((c, core));
    let core = open();
    let mut c = Client::new(core.clone());
    assert_eq!(wipe_pending(&mut c).await, Value::Null);
    assert_eq!(
        core.library().profiles().unwrap().len(),
        2,
        "nothing was wiped"
    );

    // Joined again, signed out with the removal: the wipe ends it.
    let record = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "Phone"}))
        .await;
    c.ok(json!({"op": "signOut", "record": record, "force": true, "removeAfter": remove}))
        .await;
    assert_eq!(wipe_pending(&mut c).await, json!({"debrid": true}));
    c.ok(json!({"op": "traktSignOut", "profile": kid})).await;
    c.ok(json!({"op": "signOut", "wipe": true, "disconnectDebrid": true}))
        .await;
    assert_eq!(wipe_pending(&mut c).await, Value::Null);
    assert_eq!(core.pending_removal(), None);

    // A device whose account was deleted records the removal without a leave; one that
    // still holds an account is refused, and records nothing.
    c.ok(json!({"op": "signOut", "removeAfter": remove})).await;
    assert_eq!(wipe_pending(&mut c).await, json!({"debrid": true}));
    c.ok(json!({"op": "signOut", "wipe": true})).await;
    assert_eq!(wipe_pending(&mut c).await, Value::Null);
    let created = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "Phone"}))
        .await;
    // Its first round stores the roster, as the apps run one at once.
    let synced = c.ok(json!({"op": "syncNow", "record": created})).await;
    let record = match &synced["record"] {
        Value::Null => created,
        stored => stored.clone(),
    };
    assert_eq!(
        c.code(json!({"op": "signOut", "removeAfter": remove}))
            .await,
        "wrongRecord"
    );
    assert_eq!(wipe_pending(&mut c).await, Value::Null);

    // The leave is under way (its relay does not answer) when the app is closed: the
    // removal was recorded before the leave went out, so the next start finds it.
    let mut stuck = record.clone();
    stuck["server"] = json!(hanging);
    let request =
        json!({"id": 99, "op": "signOut", "record": stuck, "force": true, "removeAfter": remove});
    c.session.handle_text(&request.to_string()).await;
    assert_eq!(core.pending_removal(), Some(true));
    drop((c, core));
    let core = open();
    let mut c = Client::new(core.clone());
    assert_eq!(wipe_pending(&mut c).await, json!({"debrid": true}));
    drop((c, core));
    let _ = std::fs::remove_dir_all(&dir);
}

/// When the flag clears (phase 2 web review A round 2): a leave with no
/// answer (a network error or a 5xx; the relay may have taken it) keeps the
/// removal, so the next start runs it once the device holds no account; only
/// a definite refusal clears it. A later sign-out without `removeAfter` is
/// the user's latest choice: an older removal that never ran is off.
#[tokio::test]
async fn a_removal_stays_through_a_lost_answer_and_goes_with_a_later_plain_sign_out() {
    let dir = temp_dir("removal-answers");
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    // A relay that is not there (its port closed) and one that answers 503 to everything.
    let closed = {
        let gone = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        format!("http://{}", gone.local_addr().unwrap())
    };
    let failing = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unavailable = format!("http://{}", failing.local_addr().unwrap());
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut socket, _)) = failing.accept().await {
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await;
            let busy = "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
            let _ = socket.write_all(busy.as_bytes()).await;
        }
    });
    let core = Core::open(CoreConfig {
        data_dir: dir.join("device"),
        profile_id: "default".into(),
        device_id: 0,
    })
    .unwrap();
    let mut c = Client::new(core.clone());
    let created = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "Phone"}))
        .await;
    let synced = c.ok(json!({"op": "syncNow", "record": created})).await;
    let record = match &synced["record"] {
        Value::Null => created,
        stored => stored.clone(),
    };
    for (relay, what) in [(&closed, "no relay"), (&unavailable, "a 503")] {
        let mut lost = record.clone();
        lost["server"] = json!(relay);
        let leave = json!({"op": "signOut", "record": lost, "force": true, "removeAfter": {"debrid": true}});
        let reply = c.call(leave).await;
        assert_eq!(reply["ok"], false, "{what}: {reply}");
        assert_eq!(
            wipe_pending(&mut c).await,
            json!({"debrid": true}),
            "{what}: the relay may have taken the leave"
        );
    }
    // The user signs out keeping the library: the older removal is off, and nothing is wiped.
    c.ok(json!({"op": "signOut", "record": record, "force": true}))
        .await;
    assert_eq!(wipe_pending(&mut c).await, Value::Null);
    assert!(!core.library().in_sync_account().unwrap());
    assert_eq!(core.library().profiles().unwrap().len(), 1);
    drop((c, core));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A device in a sync account gives the defaults its profiles never had only
/// after the run's first pull (plans/m13-default-addons.md 5): a stale list
/// given to before then would win over newer edits made on other devices.
#[tokio::test]
async fn in_a_sync_account_new_defaults_wait_for_the_runs_first_pull() {
    let relay_dir = temp_dir("defaults-relay");
    let app = mq_sync_server::router(&relay_dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let data = temp_dir("defaults-pull");
    let open = || {
        Core::open(CoreConfig {
            data_dir: data.clone(),
            profile_id: "default".into(),
            device_id: 11,
        })
        .unwrap()
    };
    let core = open();
    let mut c = Client::new(core.clone());
    let record = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "TV"}))
        .await;
    drop(c);
    // The six an older onboarding installed, as an older build left them.
    let six = [
        "cinemeta",
        "kitsu",
        "torrentio",
        "turkcealtyazi",
        "subsense",
        "opensubtitles",
    ];
    let rows: Vec<mq_library::AddonRow> = mq_runtime::defaults::list()
        .iter()
        .filter(|d| six.contains(&d.key.as_str()))
        .enumerate()
        .map(|(position, d)| mq_library::AddonRow {
            transport_url: d.url.clone(),
            manifest_json: serde_json::to_string(&d.manifest).unwrap(),
            position: position as i64,
            enabled: true,
            allow_private: false,
        })
        .collect();
    core.library()
        .set_addons("default", &rows, &core.hlc())
        .unwrap();
    drop(core);

    // The next build starts in the account: nothing before the pull.
    let core = open();
    let count = || core.library().addons("default").unwrap().len();
    assert_eq!(count(), 6);
    let mut c = Client::new(core.clone());
    c.ok(json!({"op": "syncNow", "record": record})).await;
    assert_eq!(count(), 9, "the three it never had, after the pull");
    drop(c);
    drop(core);
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&relay_dir);
}

/// A device that signed out of its account is no longer in one: the next
/// start gives its profiles the defaults they never had, as on any device
/// outside an account.
#[tokio::test]
async fn a_device_signed_out_of_its_account_gets_new_defaults_again() {
    let relay_dir = temp_dir("signed-out-relay");
    let app = mq_sync_server::router(&relay_dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let data = temp_dir("signed-out");
    let open = || {
        Core::open(CoreConfig {
            data_dir: data.clone(),
            profile_id: "default".into(),
            device_id: 12,
        })
        .unwrap()
    };
    let core = open();
    let mut c = Client::new(core.clone());
    let record = c
        .ok(json!({"op": "createSyncAccount", "server": server, "name": "TV"}))
        .await;
    let six = [
        "cinemeta",
        "kitsu",
        "torrentio",
        "turkcealtyazi",
        "subsense",
        "opensubtitles",
    ];
    let rows: Vec<mq_library::AddonRow> = mq_runtime::defaults::list()
        .iter()
        .filter(|d| six.contains(&d.key.as_str()))
        .enumerate()
        .map(|(position, d)| mq_library::AddonRow {
            transport_url: d.url.clone(),
            manifest_json: serde_json::to_string(&d.manifest).unwrap(),
            position: position as i64,
            enabled: true,
            allow_private: false,
        })
        .collect();
    core.library()
        .set_addons("default", &rows, &core.hlc())
        .unwrap();
    c.ok(json!({"op": "signOut", "record": record})).await;
    drop(c);
    drop(core);

    let core = open();
    assert_eq!(core.library().addons("default").unwrap().len(), 9);
    drop(core);
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&relay_dir);
}
