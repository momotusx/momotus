//! Kids profiles over the protocol (plans/m11-kids.md 6, 7, 8 and 9): the
//! owner's list ops and views, owner ops refused in a kids session, leaving
//! with the owner's PIN, and a policy change reaching an open kids session.

use mq_bridge::{Outgoing, Output, Session};
use mq_library::{AddonRow, OWNER};
use mq_runtime::Core;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const PIN: &str = "1357";

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

    async fn create(&mut self, name: &str, kids: bool) -> String {
        let snapshot = self
            .ok(json!({"op": "createProfile", "name": name, "avatar": "fox:amber", "kids": kids}))
            .await;
        snapshot["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn switch(&mut self, profile: &str, pin: Option<&str>) -> Value {
        self.ok(json!({"op": "switchProfile", "profile": profile, "pin": pin}))
            .await
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

    /// The latest snapshot a view pushed or answered that `done` accepts.
    async fn update_where(&mut self, sub: u64, done: impl Fn(&Value) -> bool) -> Value {
        let push = self
            .push_where(|m| {
                m["sub"] == sub
                    && m["updates"]
                        .as_array()
                        .and_then(|u| u.last())
                        .is_some_and(&done)
            })
            .await;
        push["updates"].as_array().unwrap().last().unwrap().clone()
    }
}

/// The owner with a PIN, and a session on it.
async fn owner() -> (Core, Client) {
    let core = Core::temporary(OWNER).unwrap();
    let mut c = Client::new(core.clone());
    let reply = c
        .ok(json!({"op": "setPin", "profile": OWNER, "pin": PIN, "currentPin": null}))
        .await;
    assert_eq!(reply["active"], OWNER);
    (core, c)
}

/// A meta addon answering every request with Up (movie tt1) on the owner.
async fn meta_addon(core: &Core) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let body =
                    json!({"meta": {"id": "tt1", "type": "movie", "name": "Up"}}).to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            });
        }
    });
    let manifest = json!({
        "id": "meta.test", "version": "1.0.0", "name": "Meta",
        "types": ["movie"], "resources": ["meta"], "catalogs": [], "idPrefixes": ["tt"]
    });
    let row = AddonRow {
        transport_url: format!("http://{addr}/manifest.json"),
        manifest_json: manifest.to_string(),
        position: 0,
        enabled: true,
        allow_private: true,
    };
    core.library()
        .set_addons(OWNER, &[row], &core.hlc())
        .unwrap();
}

/// The owner allows, lists and removes a kids profile's titles and makes it
/// standard, with the views following each write.
#[tokio::test]
async fn the_owner_manages_a_kids_list_over_the_protocol() {
    let (core, mut c) = owner().await;
    meta_addon(&core).await;
    let kid = c.create("Kid", true).await;
    let dad = c.create("Dad", false).await;
    let titles = c
        .ok(json!({"op": "subscribe", "view": "kidsTitles", "params": {"profile": kid}}))
        .await;
    let titles_sub = titles["sub"].as_u64().unwrap();
    assert_eq!(titles["snapshot"]["titles"], json!([]), "{titles}");
    assert_eq!(titles["snapshot"]["max"], 1000, "{titles}");
    let keys = c.ok(json!({"op": "subscribe", "view": "kidsKeys"})).await;
    let keys_sub = keys["sub"].as_u64().unwrap();
    assert_eq!(
        keys["snapshot"]["profiles"],
        json!([{"profile": kid, "name": "Kid", "keys": []}]),
        "{keys}"
    );

    let allowed = c
        .ok(json!({"op": "allowTitle", "profile": kid, "kind": "movie", "itemId": "tt1"}))
        .await;
    assert_eq!(allowed, json!({}));
    let listed = c
        .update_where(titles_sub, |s| {
            s["titles"].as_array().is_some_and(|t| !t.is_empty())
        })
        .await;
    assert_eq!(listed["titles"][0]["id"], "tt1", "{listed}");
    assert_eq!(listed["titles"][0]["name"], "Up", "{listed}");
    let counted = c
        .update_where(keys_sub, |s| {
            s["profiles"][0]["keys"] == json!(["movie:tt1"])
        })
        .await;
    assert_eq!(counted["profiles"][0]["profile"], kid.as_str());

    let removed = c
        .ok(json!({"op": "removeTitle", "profile": kid, "itemId": "tt1"}))
        .await;
    assert_eq!(removed, json!({}));
    c.update_where(titles_sub, |s| s["titles"] == json!([]))
        .await;
    c.update_where(keys_sub, |s| s["profiles"][0]["keys"] == json!([]))
        .await;

    // The kids-only ops on a standard profile, and on none.
    for op in [
        json!({"op": "allowTitle", "profile": dad, "kind": "movie", "itemId": "tt1"}),
        json!({"op": "removeTitle", "profile": dad, "itemId": "tt1"}),
        json!({"op": "makeStandard", "profile": dad}),
        json!({"op": "subscribe", "view": "kidsTitles", "params": {"profile": dad}}),
    ] {
        assert_eq!(c.code(op.clone()).await, "notKids", "{op}");
    }
    for op in [
        json!({"op": "removeTitle", "profile": "nobody", "itemId": "tt1"}),
        json!({"op": "makeStandard", "profile": "nobody"}),
    ] {
        assert_eq!(c.code(op.clone()).await, "unknownProfile", "{op}");
    }

    let snapshot = c.ok(json!({"op": "makeStandard", "profile": kid})).await;
    let made = snapshot["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == kid.as_str())
        .unwrap();
    assert_eq!(made["kids"], false, "{snapshot}");
    c.update_where(keys_sub, |s| s["profiles"] == json!([]))
        .await;
    assert_eq!(
        c.code(json!({"op": "makeStandard", "profile": kid})).await,
        "notKids"
    );
}

/// A kids session is refused every owner op and view with `kids`; a
/// standard profile's session reaches the core, which says `notOwner`.
#[tokio::test]
async fn a_kids_session_is_refused_every_owner_op() {
    let (_core, mut c) = owner().await;
    let kid = c.create("Kid", true).await;
    let dad = c.create("Dad", false).await;
    let owner_ops = [
        json!({"op": "allowTitle", "profile": kid, "kind": "movie", "itemId": "tt1"}),
        json!({"op": "removeTitle", "profile": kid, "itemId": "tt1"}),
        json!({"op": "makeStandard", "profile": kid}),
        json!({"op": "subscribe", "view": "kidsTitles", "params": {"profile": kid}}),
        json!({"op": "subscribe", "view": "kidsKeys"}),
    ];
    c.switch(&dad, None).await;
    for op in &owner_ops {
        assert_eq!(c.code(op.clone()).await, "notOwner", "{op}");
    }
    c.switch(&kid, None).await;
    for op in &owner_ops {
        assert_eq!(c.code(op.clone()).await, "kids", "{op}");
    }
    assert_eq!(c.code(json!({"op": "resetOwnerPin"})).await, "kids");
    // No other profile's history for a kid (plans/profile-panel.md 6).
    assert_eq!(c.code(json!({"op": "profilesLastWatched"})).await, "kids");
    // Locked, the kids session still enters no Incognito.
    c.ok(json!({"op": "lockSession"})).await;
    let incognito = json!({"op": "switchProfile", "profile": "incognito", "pin": PIN});
    assert_eq!(c.code(incognito).await, "kids");
}

/// Out of a kids session every switch asks the owner's PIN, never the
/// target's; with no owner PIN only `deviceAuth` lets it out.
#[tokio::test]
async fn leaving_a_kids_session_needs_the_owners_pin() {
    let (_core, mut c) = owner().await;
    let kid = c.create("Kid", true).await;
    let dad = c.create("Dad", false).await;
    let snapshot = c.switch(&kid, None).await;
    assert_eq!(snapshot["kids"], true, "{snapshot}");
    let to = |profile: &str, pin: Option<&str>| json!({"op": "switchProfile", "profile": profile, "pin": pin});
    // Dad has no PIN, yet a kid cannot walk into him, nor into itself.
    assert_eq!(c.code(to(&dad, None)).await, "wrongPin");
    assert_eq!(c.code(to(&kid, None)).await, "wrongPin");
    assert_eq!(c.code(to(&dad, Some("2468"))).await, "wrongPin");
    let snapshot = c.switch(&dad, Some(PIN)).await;
    assert_eq!(snapshot["active"], dad.as_str());
    assert_eq!(snapshot["kids"], false, "{snapshot}");
    // From a standard profile the target's PIN rules again: Dad has none.
    c.switch(OWNER, Some(PIN)).await;
    c.switch(&dad, None).await;

    // The owner's PIN cleared (a reset on another device): the exit then
    // needs the device's own lock.
    c.switch(OWNER, Some(PIN)).await;
    c.ok(json!({"op": "resetOwnerPin"})).await;
    c.switch(&kid, None).await;
    assert_eq!(c.code(to(&dad, None)).await, "kidsNeedPin");
    let out = json!({"op": "switchProfile", "profile": dad, "deviceAuth": true});
    assert_eq!(c.ok(out).await["active"], dad.as_str());
}

/// The kids profile removed under its session: the session locks, and its
/// picker still says kids and asks the owner's PIN until a switch.
#[tokio::test]
async fn a_removed_kids_profile_keeps_asking_the_owners_pin() {
    let (core, mut c) = owner().await;
    let kid = c.create("Kid", true).await;
    let dad = c.create("Dad", false).await;
    c.switch(&kid, None).await;
    let picker = c.ok(json!({"op": "subscribe", "view": "profiles"})).await;
    let sub = picker["sub"].as_u64().unwrap();
    core.delete_profile(&kid).unwrap();
    let locked = c.update_where(sub, |s| s["locked"] == true).await;
    assert_eq!(locked["kids"], true, "{locked}");
    let to_dad = json!({"op": "switchProfile", "profile": dad, "pin": null});
    assert_eq!(c.code(to_dad).await, "wrongPin");
    let snapshot = c.switch(&dad, Some(PIN)).await;
    assert_eq!(snapshot["kids"], false, "{snapshot}");
}

/// A title taken off the list closes the kids session's views of it with
/// one `closed` push; unsubscribing them answers ok once.
#[tokio::test]
async fn a_title_leaving_the_list_closes_the_kids_views_of_it() {
    let (core, mut c) = owner().await;
    meta_addon(&core).await;
    let kid = c.create("Kid", true).await;
    core.allow_title(&kid, "movie", "tt1").await.unwrap();
    c.switch(&kid, None).await;
    let title = c
        .ok(json!({"op": "subscribe", "view": "title", "params": {"kind": "movie", "id": "tt1"}}))
        .await;
    let sub = title["sub"].as_u64().unwrap();
    assert_ne!(
        title["snapshot"]["header"]["state"]["state"], "notAllowed",
        "{title}"
    );
    let home = c.ok(json!({"op": "subscribe", "view": "home"})).await["sub"]
        .as_u64()
        .unwrap();
    core.remove_title(&kid, "tt1").unwrap();
    let closed = c.push_where(|m| m["closed"].is_string()).await;
    assert_eq!(closed, json!({"closed": "notAllowed", "subs": [sub]}));
    c.ok(json!({"op": "unsubscribe", "sub": sub})).await;
    let again = c.call(json!({"op": "unsubscribe", "sub": sub})).await;
    assert_eq!(again["ok"], false, "{again}");
    // Home is no view of one title: it stays.
    c.ok(json!({"op": "unsubscribe", "sub": home})).await;
    // Opened again, the title is refused at once.
    let reopened = c
        .ok(json!({"op": "subscribe", "view": "title", "params": {"kind": "movie", "id": "tt1"}}))
        .await;
    assert_eq!(
        reopened["snapshot"]["header"]["state"]["state"], "notAllowed",
        "{reopened}"
    );
}

/// Made standard while its session runs: the session moves to the same
/// profile unlocked, says so with `access`, and leaves without a PIN.
#[tokio::test]
async fn a_kids_session_made_standard_moves_and_leaves_freely() {
    let (core, mut c) = owner().await;
    let kid = c.create("Kid", true).await;
    let dad = c.create("Dad", false).await;
    c.switch(&kid, None).await;
    let home = c.ok(json!({"op": "subscribe", "view": "home"})).await["sub"]
        .as_u64()
        .unwrap();
    core.make_standard(&kid).unwrap();
    let access = c.push_where(|m| m["profile"] == "access").await;
    assert_eq!(access, json!({"profile": "access", "id": kid}));
    // Its views closed with the move.
    let gone = c.call(json!({"op": "unsubscribe", "sub": home})).await;
    assert_eq!(gone["ok"], false, "{gone}");
    let picker = c.ok(json!({"op": "subscribe", "view": "profiles"})).await;
    assert_eq!(picker["snapshot"]["kids"], false, "{picker}");
    assert_eq!(picker["snapshot"]["locked"], false, "{picker}");
    let sub = picker["sub"].as_u64().unwrap();
    c.ok(json!({"op": "unsubscribe", "sub": sub})).await;
    c.switch(&dad, None).await;
}
