use mq_bridge::{Outgoing, Output, Session};
use mq_runtime::Core;
use mq_sync::SyncConfig;
use mq_sync::roster::{Bundle, Hex};
use mq_sync::wire::RosterHead;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

async fn next(rx: &mut mpsc::UnboundedReceiver<Outgoing>) -> Value {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("reply")
        .expect("open")
        .json()
}

/// The reply to request `id`, past the view pushes that arrive meanwhile.
async fn reply_to(rx: &mut mpsc::UnboundedReceiver<Outgoing>, id: u64) -> Value {
    loop {
        let message = next(rx).await;
        if message["id"].as_u64() == Some(id) {
            return message;
        }
    }
}

#[tokio::test]
async fn subscribe_command_unsubscribe_and_errors() {
    let core = Core::temporary("p").unwrap();
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(core, tx);

    session
        .handle_text(&json!({"id": 1, "op": "subscribe", "view": "home"}).to_string())
        .await;
    let reply = next(&mut rx).await;
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["ok"], true);
    let sub = reply["result"]["sub"].as_u64().unwrap();
    assert!(reply["result"]["snapshot"]["rows"].is_array());

    session.handle_text(&json!({"id": 2, "op": "command", "sub": sub, "command": {"type": "selectSeason", "season": 1}}).to_string()).await;
    let reply = next(&mut rx).await;
    assert_eq!(reply["ok"], false, "home has no seasons");

    session
        .handle_text(&json!({"id": 3, "op": "subscribe", "view": "nope"}).to_string())
        .await;
    assert_eq!(next(&mut rx).await["error"], "unknown view: nope");

    session
        .handle_text(&json!({"id": 4, "op": "unsubscribe", "sub": sub}).to_string())
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);

    session
        .handle_text("{\"id\": 5, \"op\": \"whatever\"}")
        .await;
    let reply = next(&mut rx).await;
    assert_eq!(reply["id"], 5);
    assert_eq!(reply["ok"], false);

    // Ordered pairs: the first store is the default.
    session
        .handle_text(
            &json!({"id": 6, "op": "setDebridKeys", "keys": [["realdebrid", "k1"], ["tb", "k2"]]})
                .to_string(),
        )
        .await;
    assert_eq!(next(&mut rx).await["result"]["stores"], 2);
    session
        .handle_text(&json!({"id": 7, "op": "setDebridKeys", "keys": [["nope", "k"]]}).to_string())
        .await;
    assert_eq!(next(&mut rx).await["error"], "unknown debrid service: nope");
}

#[tokio::test]
#[ignore = "network"]
async fn live_home_and_title_over_the_protocol() {
    let core = Core::temporary("p").unwrap();
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(core, tx);
    session.handle_text(&json!({"id": 1, "op": "installAddon", "url": "https://v3-cinemeta.strem.io/manifest.json"}).to_string()).await;
    assert_eq!(next(&mut rx).await["result"]["name"], "Cinemeta");
    session.handle_text(&json!({"id": 2, "op": "subscribe", "view": "title", "params": {"kind": "series", "id": "tt0944947"}}).to_string()).await;
    let reply = next(&mut rx).await;
    let sub = reply["result"]["sub"].as_u64().unwrap();
    let push = next(&mut rx).await;
    assert_eq!(push["sub"].as_u64(), Some(sub));
    assert!(push["updates"].is_array());
}

/// One local server that is both a stream addon and the media origin.
async fn local_addon() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let origin = base.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let origin = origin.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let body = if head.starts_with("GET /manifest.json") {
                    json!({"id": "local", "version": "1.0.0", "name": "Local", "types": ["movie"],
                           "resources": ["stream"], "catalogs": []})
                    .to_string()
                } else if head.starts_with("GET /stream/") {
                    json!({"streams": [{"name": "1080p", "url": format!("{origin}/video.mkv"),
                           "behaviorHints": {"filename": "Sintel.2010.1080p.mkv"}}]})
                    .to_string()
                } else {
                    "M".into()
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    base
}

/// Waits for a push on `sub` whose last update satisfies `done`.
async fn push_until(
    rx: &mut mpsc::UnboundedReceiver<Outgoing>,
    sub: u64,
    done: impl Fn(&Value) -> bool,
) -> Value {
    loop {
        let message = next(rx).await;
        if message["sub"].as_u64() == Some(sub)
            && let Some(last) = message["updates"].as_array().and_then(|u| u.last())
            && done(last)
        {
            return last.clone();
        }
    }
}

#[tokio::test]
async fn versions_then_playback_then_progress() {
    let base = local_addon().await;
    let core = Core::temporary("p").unwrap();
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(core.clone(), tx);
    let request = |v: Value| v.to_string();

    session
        .handle_text(&request(
            json!({"id": 1, "op": "installAddon", "url": format!("{base}/manifest.json")}),
        ))
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);

    session
        .handle_text(&request(
            json!({"id": 2, "op": "subscribe", "view": "versions",
            "params": {"kind": "movie", "videoId": "tt1727587", "title": "Sintel"}}),
        ))
        .await;
    let versions = next(&mut rx).await["result"]["sub"].as_u64().unwrap();
    let snapshot = push_until(&mut rx, versions, |s| s["done"] == true).await;
    let key = snapshot["recommended"]["key"].as_str().unwrap().to_owned();

    session
        .handle_text(&request(
            json!({"id": 3, "op": "subscribe", "view": "playback",
            "params": {"versionsSub": versions, "key": key, "kind": "movie",
                       "itemId": "tt1727587", "videoId": "tt1727587", "name": "Sintel"}}),
        ))
        .await;
    let reply = next(&mut rx).await;
    assert_eq!(reply["result"]["snapshot"]["phase"], "resolving", "{reply}");
    let playback = reply["result"]["sub"].as_u64().unwrap();
    let ready = push_until(&mut rx, playback, |s| s["phase"] != "resolving").await;
    assert_eq!(ready["phase"], "ready", "{ready}");
    assert!(
        ready["url"]
            .as_str()
            .unwrap()
            .ends_with("/Sintel.2010.1080p.mkv")
    );

    session
        .handle_text(&request(json!({"id": 4, "op": "command", "sub": playback,
            "command": {"type": "progress", "positionMs": 30000, "durationMs": 888000}})))
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);
    // `duration` needs its number; a report for another version is accepted
    // and ignored (no lookup starts, so no network in this test).
    session
        .handle_text(&request(json!({"id": 41, "op": "command", "sub": playback,
            "command": {"type": "duration", "key": key}})))
        .await;
    assert_eq!(next(&mut rx).await["ok"], false);
    session
        .handle_text(&request(json!({"id": 42, "op": "command", "sub": playback,
            "command": {"type": "duration", "key": "another-version", "durationMs": 888000}})))
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);
    // A failOver about a source already given up on leaves this one playing.
    let gone = ready["attempt"].as_u64().unwrap() - 1;
    session
        .handle_text(&request(json!({"id": 43, "op": "command", "sub": playback,
            "command": {"type": "failOver", "attempt": gone, "error": "late"}})))
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);
    let moved = tokio::time::timeout(
        Duration::from_millis(300),
        push_until(&mut rx, playback, |s| s["phase"] != "ready"),
    )
    .await;
    assert!(moved.is_err(), "{moved:?}");
    // The player reports what failed; the attempt list has the core's words.
    let current = ready["attempt"].as_u64().unwrap();
    session
        .handle_text(&request(json!({"id": 44, "op": "command", "sub": playback,
            "command": {"type": "failOver", "attempt": current, "kind": "http", "status": 404,
                        "error": "The source answered with HTTP 404"}})))
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);
    let failed = push_until(&mut rx, playback, |s| s["phase"] == "failed").await;
    assert_eq!(
        failed["attempts"][0]["error"], "The link no longer exists (404)",
        "{failed}"
    );
    session
        .handle_text(&request(
            json!({"id": 5, "op": "unsubscribe", "sub": playback}),
        ))
        .await;
    // The failed source relearns the versions view, which pushes.
    assert_eq!(reply_to(&mut rx, 5).await["ok"], true);
    assert_eq!(
        core.library().progress("p", "tt1727587").unwrap(),
        Some((30000, 888000))
    );
}

#[tokio::test]
async fn addons_view_follows_disable_and_remove() {
    let base = local_addon().await;
    let core = Core::temporary("p").unwrap();
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(core, tx);
    let url = format!("{base}/manifest.json");
    session
        .handle_text(&json!({"id": 1, "op": "installAddon", "url": url}).to_string())
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);
    session
        .handle_text(&json!({"id": 2, "op": "subscribe", "view": "addons"}).to_string())
        .await;
    let reply = next(&mut rx).await;
    let sub = reply["result"]["sub"].as_u64().unwrap();
    assert_eq!(
        reply["result"]["snapshot"]["addons"][0]["provides"],
        json!(["stream"])
    );

    session
        .handle_text(
            &json!({"id": 3, "op": "setAddonEnabled", "url": url, "enabled": false}).to_string(),
        )
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);
    let s = push_until(&mut rx, sub, |s| s["addons"][0]["enabled"] == false).await;
    assert_eq!(s["addons"][0]["host"], "127.0.0.1");

    session
        .handle_text(&json!({"id": 4, "op": "removeAddon", "url": url}).to_string())
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);
    push_until(&mut rx, sub, |s| {
        s["addons"].as_array().is_some_and(Vec::is_empty)
    })
    .await;
    session
        .handle_text(&json!({"id": 5, "op": "removeAddon", "url": url}).to_string())
        .await;
    assert_eq!(next(&mut rx).await["ok"], false);
}

#[tokio::test]
async fn sync_account_then_two_devices_share_preferences() {
    let dir = std::env::temp_dir().join(format!("mq-bridge-sync-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });

    let (tx_a, mut rx_a) = Output::channel();
    let mut a = Session::new(Core::temporary("p").unwrap(), tx_a);
    a.handle_text(
        &json!({"id": 1, "op": "createSyncAccount", "server": server, "name": "Phone"}).to_string(),
    )
    .await;
    let record = next(&mut rx_a).await["result"].clone();
    assert_eq!(record["keys"]["ak"].as_str().unwrap().len(), 64);
    assert_ne!(record["keys"]["ak"], record["keys"]["idKey"]);
    let sync = json!({"op": "syncNow", "record": record});
    a.handle_text(&json!({"id": 9, "op": "syncDevices", "record": record}).to_string())
        .await;
    let devices = next(&mut rx_a).await;
    assert_eq!(
        devices["result"]["members"][0]["name"], "Phone",
        "{devices}"
    );
    assert_eq!(devices["result"]["fingerprint"].as_str().unwrap().len(), 12);
    // Without a fork both sides of the preview are the same, and nothing drops.
    a.handle_text(&json!({"id": 10, "op": "forkPreview", "record": record}).to_string())
        .await;
    let preview = next(&mut rx_a).await;
    assert_eq!(preview["result"]["mine"][0]["name"], "Phone", "{preview}");
    assert_eq!(
        preview["result"]["mine"][0]["thisDevice"], true,
        "{preview}"
    );
    assert_eq!(
        preview["result"]["mine"], preview["result"]["server"],
        "{preview}"
    );
    assert_eq!(preview["result"]["drops"], json!([]), "{preview}");
    assert_eq!(preview["result"]["onServer"], true, "{preview}");
    assert_eq!(preview["result"]["kits"], json!([]), "{preview}");
    assert!(
        preview["result"]["head"]
            .as_str()
            .is_some_and(|h| h.len() == 64),
        "{preview}"
    );

    a.handle_text(
        &json!({"id": 2, "op": "setPrefs", "patch": {"subtitleLanguages": ["tur"]}}).to_string(),
    )
    .await;
    assert_eq!(next(&mut rx_a).await["ok"], true);
    let mut push = sync.clone();
    push["id"] = json!(3);
    a.handle_text(&push.to_string()).await;
    let report = next(&mut rx_a).await;
    assert!(
        report["result"]["pushed"].as_u64().unwrap() >= 1,
        "{report}"
    );

    let (tx_b, mut rx_b) = Output::channel();
    let core_b = Core::temporary("p").unwrap();
    let mut b = Session::new(core_b.clone(), tx_b);
    let mut pull = sync.clone();
    pull["id"] = json!(4);
    b.handle_text(&pull.to_string()).await;
    assert_eq!(next(&mut rx_b).await["ok"], true);
    assert_eq!(core_b.prefs().subtitle_languages, ["tur"]);

    let mut bad = sync.clone();
    bad["id"] = json!(5);
    bad["record"]["keys"]["ak"] = json!("zz");
    b.handle_text(&bad.to_string()).await;
    assert_eq!(next(&mut rx_b).await["ok"], false);
    // Errors an app acts on carry a code next to the message.
    let mut far = sync;
    far["id"] = json!(6);
    far["record"]["server"] = json!("http://sync.example.com");
    b.handle_text(&far.to_string()).await;
    let failed = next(&mut rx_b).await;
    assert_eq!(failed["code"], "insecure", "{failed}");

    // An approval ended with cancelPairing rotates the key at once.
    let http = mq_http::HttpClient::new(mq_http::ClientConfig::default()).unwrap();
    let tv = mq_sync::pair::start_pairing(&http, &server).await.unwrap();
    a.handle_text(
        &json!({"id": 11, "op": "approvePairing", "record": record, "qr": tv.qr}).to_string(),
    )
    .await;
    let approved = next(&mut rx_a).await;
    assert_eq!(
        approved["result"]["code"].as_str().map(str::len),
        Some(6),
        "{approved}"
    );
    let after = approved["result"]["record"].clone();
    a.handle_text(
        &json!({"id": 12, "op": "cancelPairing", "record": after, "qr": tv.qr}).to_string(),
    )
    .await;
    let cancelled = next(&mut rx_a).await;
    assert_eq!(cancelled["result"]["keys"]["epoch"], 1, "{cancelled}");

    // A kit's reply names its own entry in the device list.
    a.handle_text(
        &json!({"id": 16, "op": "createRecoveryKit", "record": cancelled["result"]}).to_string(),
    )
    .await;
    let made = next(&mut rx_a).await;
    let kit_id = made["result"]["id"].clone();
    a.handle_text(
        &json!({"id": 17, "op": "syncDevices", "record": made["result"]["record"]}).to_string(),
    )
    .await;
    let listed = next(&mut rx_a).await;
    let members = listed["result"]["members"]
        .as_array()
        .expect("a device list");
    assert!(
        members
            .iter()
            .any(|m| m["id"] == kit_id && m["kit"] == true),
        "{kit_id} not a kit in {listed}"
    );

    // A made-up record (another account's) wipes nothing: refused with its own code.
    a.handle_text(
        &json!({"id": 14, "op": "createSyncAccount", "server": server, "name": "Other"})
            .to_string(),
    )
    .await;
    let other = next(&mut rx_a).await["result"].clone();
    a.handle_text(&json!({"id": 15, "op": "signOut", "record": other, "local": true}).to_string())
        .await;
    let refused = next(&mut rx_a).await;
    assert_eq!(refused["code"], "wrongRecord", "{refused}");

    // Leaving here only answers without any relay: this record names a dead one.
    let mut dead = record.clone();
    dead["server"] = json!("http://127.0.0.1:9");
    a.handle_text(&json!({"id": 13, "op": "signOut", "record": dead, "local": true}).to_string())
        .await;
    let left = next(&mut rx_a).await;
    assert_eq!(left["ok"], true, "{left}");

    // Only the owner profile deletes the account; this session runs as "p".
    a.handle_text(&json!({"id": 18, "op": "deleteAccount", "record": other}).to_string())
        .await;
    let refused = next(&mut rx_a).await;
    assert_eq!(refused["code"], "notOwner", "{refused}");
    // The owner's answer comes once the relay confirmed; the account then
    // answers `accountGone` to every sync.
    let (tx_o, mut rx_o) = Output::channel();
    let mut owner = Session::new(Core::temporary("default").unwrap(), tx_o);
    owner
        .handle_text(&json!({"id": 1, "op": "deleteAccount", "record": other}).to_string())
        .await;
    let deleted = next(&mut rx_o).await;
    assert_eq!(deleted["result"], json!({}), "{deleted}");
    a.handle_text(&json!({"id": 19, "op": "syncNow", "record": other}).to_string())
        .await;
    let gone = next(&mut rx_a).await;
    assert_eq!(gone["code"], "accountGone", "{gone}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A sync against a relay that never answers must not hold up the requests
/// after it (a sync on foreground used to delay playback commands).
#[tokio::test]
async fn a_hanging_sync_does_not_block_other_requests() {
    let dir = std::env::temp_dir().join(format!("mq-bridge-hang-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(Core::temporary("p").unwrap(), tx);
    session
        .handle_text(
            &json!({"id": 1, "op": "createSyncAccount", "server": server, "name": "Phone"})
                .to_string(),
        )
        .await;
    let mut record = next(&mut rx).await["result"].clone();

    // A relay that accepts connections and never answers.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    record["server"] = json!(format!("http://{}", silent.local_addr().unwrap()));
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = silent.accept().await {
            held.push(socket);
        }
    });
    let sync = json!({"id": 2, "op": "syncNow", "record": record}).to_string();
    tokio::time::timeout(Duration::from_secs(2), session.handle_text(&sync))
        .await
        .expect("the request loop does not wait for the sync");
    session
        .handle_text(&json!({"id": 3, "op": "setPrefs", "patch": {}}).to_string())
        .await;
    let reply = next(&mut rx).await;
    assert_eq!(reply["id"], 3, "{reply}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Sends `request` and returns the result of its successful reply.
async fn ok(
    session: &mut Session,
    rx: &mut mpsc::UnboundedReceiver<Outgoing>,
    request: Value,
) -> Value {
    session.handle_text(&request.to_string()).await;
    let reply = next(rx).await;
    assert_eq!(reply["ok"], true, "{reply}");
    reply["result"].clone()
}

#[tokio::test]
async fn debrid_records_round_trip_through_the_bridge() {
    let core = Core::temporary("p").unwrap();
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(core, tx);
    let (s, rx) = (&mut session, &mut rx);
    let reply = ok(s, rx, json!({"id": 1, "op": "subscribe", "view": "debrid"})).await;
    let sub = reply["sub"].as_u64().unwrap();
    // Debrid is the device's: the view stays open through a lock, and
    // every call works on a locked session.
    ok(s, rx, json!({"id": 2, "op": "lockSession"})).await;
    // As a JavaScript number this would lose its low bits.
    let generation = "fffffffffffffffffffffffffffffff1";
    let answer = ok(
        s,
        rx,
        json!({"id": 3, "op": "restoreDebrid", "records": [{
        "service": "premiumize", "generation": generation, "revision": 1,
        "credential": {"kind": "key", "key": "pm"}
    }], "tombstones": []}),
    )
    .await;
    assert_eq!(answer, json!({"refused": [], "tombstones": []}));
    let pushed = push_until(rx, sub, |v| {
        v["accounts"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    assert_eq!(
        pushed["accounts"],
        json!([{"service": "premiumize", "generation": generation, "state": "ok"}])
    );
    let reply = ok(
        s,
        rx,
        json!({"id": 4, "op": "setDebridKey", "service": "torbox", "key": "tb"}),
    )
    .await;
    let torbox = reply["generation"].as_str().unwrap().to_owned();
    assert_eq!(torbox.len(), 32);
    let pushed = push_until(rx, sub, |v| {
        v["pending"].as_array().is_some_and(|p| !p.is_empty())
    })
    .await;
    assert_eq!(
        pushed["pending"],
        json!([{"service": "torbox", "generation": torbox, "revision": 1,
                "credential": {"kind": "key", "key": "tb"}}])
    );
    ok(
        s,
        rx,
        json!({"id": 5, "op": "ackDebrid", "service": "torbox",
        "generation": torbox, "revision": 1}),
    )
    .await;
    push_until(rx, sub, |v| v["pending"] == json!([])).await;
    ok(
        s,
        rx,
        json!({"id": 6, "op": "disconnectDebrid", "service": "premiumize",
        "generation": generation}),
    )
    .await;
    let pushed = push_until(rx, sub, |v| {
        v["accounts"].as_array().is_some_and(|a| a.len() == 1)
    })
    .await;
    assert_eq!(pushed["accounts"][0]["service"], "torbox");
    // A sign-in is a screen: not while locked.
    s.handle_text(
        &json!({"id": 7, "op": "subscribe", "view": "debridLogin",
        "params": {"service": "torbox"}})
        .to_string(),
    )
    .await;
    assert_eq!(next(rx).await["ok"], false);
}

/// C19: a consumer that never reads under a burst. The reserved bytes stay
/// within the budget, the output closes with `overflow` once, and nothing is
/// queued after it; the session keeps answering requests without growing.
#[tokio::test]
async fn a_stalled_consumer_never_holds_more_than_the_budget() {
    const BUDGET: usize = 64 * 1024;
    let (tx, mut rx) = Output::with_limits(BUDGET, 4 * 1024);
    let out = tx.clone();
    let mut session = Session::new(Core::temporary("p").unwrap(), tx);
    let mut most = 0;
    for id in 1..=10_000 {
        session
            .handle_text(&json!({ "id": id, "op": "ping" }).to_string())
            .await;
        most = most.max(out.reserved());
    }
    assert!(most <= BUDGET, "reserved {most} of {BUDGET}");
    assert!(out.is_closed());
    let mut messages = 0;
    let mut overflow = 0;
    while let Ok(message) = rx.try_recv() {
        match message {
            Outgoing::Message { .. } => messages += 1,
            Outgoing::Overflow => overflow += 1,
        }
    }
    assert_eq!(overflow, 1);
    assert!(messages < 10_000, "{messages} queued");
    assert_eq!(out.reserved(), 0, "everything given back once dropped");
    // After the overflow nothing more is queued.
    session
        .handle_text(&json!({ "id": 10_001, "op": "ping" }).to_string())
        .await;
    assert!(rx.try_recv().is_err());
}

/// C19: a snapshot over the message limit opens no view; the client gets a
/// failed subscribe with `code: tooLarge`.
#[tokio::test]
async fn an_oversized_snapshot_opens_no_view() {
    // Small enough that the profiles picker's snapshot does not fit.
    let (tx, mut rx) = Output::with_limits(1024 * 1024, 120);
    let mut session = Session::new(Core::temporary("p").unwrap(), tx);
    session
        .handle_text(
            &json!({ "id": 1, "op": "subscribe", "view": "profiles", "params": {} }).to_string(),
        )
        .await;
    let reply = next(&mut rx).await;
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["code"], "tooLarge");
    // The view was not kept: unsubscribing its would-be id finds nothing.
    session
        .handle_text(&json!({ "id": 2, "op": "unsubscribe", "sub": 1 }).to_string())
        .await;
    assert_eq!(next(&mut rx).await["ok"], false);
}

/// `useServerRoster` takes back the head `forkPreview` showed: a call without
/// it is refused before anything is read or written.
#[tokio::test]
async fn use_server_roster_without_a_head_is_refused() {
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(Core::temporary("p").unwrap(), tx);
    let record = SyncConfig {
        server: "https://relay.example".into(),
        device_token: "t".into(),
        member: Hex([1; 32]),
        keys: Bundle {
            v: 1,
            account: Hex([2; 16]),
            genesis: Hex([3; 32]),
            epoch: 0,
            ak: Hex([4; 32]),
            id_key: Hex([5; 32]),
        },
        checkpoint: RosterHead {
            seq: 0,
            hash: Hex([6; 32]),
        },
        relay_id: Hex([7; 16]),
        revision: 0,
    };
    session
        .handle_text(
            &json!({"id": 1, "op": "useServerRoster", "record": record, "drop": []}).to_string(),
        )
        .await;
    let reply = next(&mut rx).await;
    assert_eq!(reply["ok"], false, "{reply}");
    assert!(
        reply["error"]
            .as_str()
            .is_some_and(|e| e.contains("missing field `head`")),
        "{reply}"
    );
}

/// The defaults over the protocol (plans/m13-default-addons.md): read, then
/// installed with a switch off; and a pasted link carrying a sign-in is
/// refused until confirmed, before anything is fetched.
#[tokio::test]
async fn defaults_and_links_with_a_sign_in_over_the_protocol() {
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(Core::temporary("p").unwrap(), tx);
    session
        .handle_text(&json!({"id": 1, "op": "defaultAddons"}).to_string())
        .await;
    let listed = next(&mut rx).await;
    let addons = listed["result"]["addons"].as_array().unwrap();
    assert_eq!(addons.len(), 9, "{listed}");
    assert_eq!(addons[1]["key"], "torrentio");
    assert_eq!(addons[1]["installed"], "absent");
    session
        .handle_text(
            &json!({"id": 2, "op": "installDefaults", "choices": [
                {"key": "cinemeta", "on": true}, {"key": "torrentio", "on": false}]})
            .to_string(),
        )
        .await;
    assert_eq!(next(&mut rx).await["ok"], true);
    session
        .handle_text(&json!({"id": 3, "op": "defaultAddons"}).to_string())
        .await;
    let listed = next(&mut rx).await;
    assert_eq!(listed["result"]["addons"][1]["installed"], "off");
    assert_eq!(listed["result"]["addons"][0]["installed"], "on");
    assert_eq!(
        listed["result"]["addons"][2]["installed"], "absent",
        "a key left out is not installed"
    );

    let theirs = "https://trakt.example/eyJ0b2tlbiI6IngifQ/manifest.json";
    session
        .handle_text(&json!({"id": 4, "op": "installAddon", "url": theirs}).to_string())
        .await;
    let refused = next(&mut rx).await;
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["code"], "tokenInLink", "{refused}");
    // Percent-encoded JSON settings are read too.
    let encoded = "https://a.example/%7B%22apikey%22%3A%22k%22%7D/manifest.json";
    session
        .handle_text(&json!({"id": 5, "op": "installAddon", "url": encoded}).to_string())
        .await;
    assert_eq!(next(&mut rx).await["code"], "tokenInLink");
    // Acknowledged, the install goes ahead (here it then fails to reach the made-up host).
    session
        .handle_text(
            &json!({"id": 6, "op": "installAddon", "url": theirs, "acknowledgeToken": true})
                .to_string(),
        )
        .await;
    let tried = next(&mut rx).await;
    assert_eq!(tried["ok"], false);
    assert_ne!(tried["code"], "tokenInLink", "{tried}");
}

/// A device that pairs into or recovers a sync account gets the account's
/// addons by sync: it never starts at the defaults step, also before its
/// first sync has stored the roster. One that creates an account still
/// gets the recommended addons.
#[tokio::test]
async fn joining_a_sync_account_ends_the_defaults_step() {
    use mq_runtime::views::pairing::{PairingSnapshot, PairingView};
    let dir = std::env::temp_dir().join(format!("mq-bridge-join-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });

    let core_a = Core::temporary("p").unwrap();
    assert!(core_a.addons_fresh());
    let (tx_a, mut rx_a) = Output::channel();
    let mut a = Session::new(core_a.clone(), tx_a);
    a.handle_text(
        &json!({"id": 1, "op": "createSyncAccount", "server": server, "name": "Phone"}).to_string(),
    )
    .await;
    let record = next(&mut rx_a).await["result"].clone();
    // A new account has nothing to bring: the recommended addons still come
    // (plans/accounts.md 3.3), also once its first sync stored the roster.
    assert!(core_a.addons_fresh(), "created an account");
    a.handle_text(&json!({"id": 40, "op": "syncNow", "record": record}).to_string())
        .await;
    let synced = next(&mut rx_a).await;
    assert_eq!(synced["ok"], true, "{synced}");
    assert!(core_a.library().in_sync_account().unwrap());
    assert!(core_a.addons_fresh(), "created, then synced");
    let record = match &synced["result"]["record"] {
        Value::Null => record,
        stored => stored.clone(),
    };

    // Paired: B shows its QR, A approves it, B confirms the code.
    let core_b = Core::temporary("p").unwrap();
    assert!(core_b.addons_fresh());
    // An addons view open through the join drops `fresh` at once.
    let view = mq_runtime::views::addons::AddonsView::open(&core_b);
    assert!(view.snapshot().fresh);
    let pairing = PairingView::open(&core_b, &server, "TV", Some("tv"));
    let until = |want: fn(&PairingSnapshot) -> bool| {
        let pairing = &pairing;
        async move {
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    let now = pairing.snapshot();
                    if want(&now) {
                        return now;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("pairing moved on")
        }
    };
    let PairingSnapshot::Waiting { qr, .. } =
        until(|s| matches!(s, PairingSnapshot::Waiting { .. })).await
    else {
        unreachable!()
    };
    a.handle_text(
        &json!({"id": 2, "op": "approvePairing", "record": record, "qr": qr}).to_string(),
    )
    .await;
    let approved = next(&mut rx_a).await;
    assert_eq!(approved["ok"], true, "{approved}");
    until(|s| matches!(s, PairingSnapshot::Confirm { .. })).await;
    pairing.confirm();
    until(|s| matches!(s, PairingSnapshot::Joined { .. })).await;
    assert!(!core_b.addons_fresh(), "paired");
    tokio::time::timeout(Duration::from_secs(5), async {
        while view.snapshot().fresh {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the open addons view follows the join");

    // Recovered with a kit.
    a.handle_text(
        &json!({"id": 3, "op": "createRecoveryKit", "record": approved["result"]["record"]})
            .to_string(),
    )
    .await;
    let made = next(&mut rx_a).await;
    let core_c = Core::temporary("p").unwrap();
    assert!(core_c.addons_fresh());
    let (tx_c, mut rx_c) = Output::channel();
    let mut c = Session::new(core_c.clone(), tx_c);
    c.handle_text(
        &json!({"id": 1, "op": "recover", "server": server, "kit": made["result"]["kit"], "name": "Laptop"})
            .to_string(),
    )
    .await;
    let recovered = next(&mut rx_c).await;
    assert_eq!(recovered["ok"], true, "{recovered}");
    assert!(!core_c.addons_fresh(), "recovered");
    let _ = std::fs::remove_dir_all(&dir);
}

#[path = "../../mq-sync-server/tests/support/mock_oidc.rs"]
mod mock_oidc;

/// A relay with sign-in through the mock provider (route id `mock`): the
/// mock, the relay's address, and its directory (remove it after).
async fn sign_in_relay(name: &str) -> (mock_oidc::Mock, String, std::path::PathBuf) {
    let mock = mock_oidc::Mock::start(0).await;
    let dir = std::env::temp_dir().join(format!("mq-bridge-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    let identity: mq_sync_server::IdentityFile = serde_json::from_value(json!({
        "key": "3".repeat(64),
        "publicUrl": server,
        "oidc": [{ "id": "mock", "issuer": mock.url, "clientId": "marquee", "clientSecret": "s" }],
    }))
    .unwrap();
    let config = mq_sync_server::Config {
        public_host: Some("127.0.0.1".into()),
        identity: Some(identity),
        ..Default::default()
    };
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), config).unwrap();
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    (mock, server, dir)
}

/// A proxy in front of `target` whose first `POST /v1/auth/token` never
/// reaches the relay (502, as a gateway whose upstream dropped); everything
/// else passes through.
async fn flaky_proxy(target: String) -> String {
    use axum::response::IntoResponse;
    use std::sync::atomic::{AtomicBool, Ordering};
    let dropped = std::sync::Arc::new(AtomicBool::new(false));
    let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
        let (target, dropped) = (target.clone(), dropped.clone());
        async move {
            if request.uri().path() == "/v1/auth/token" && !dropped.swap(true, Ordering::Relaxed) {
                return (axum::http::StatusCode::BAD_GATEWAY, "upstream gone").into_response();
            }
            let (parts, body) = request.into_parts();
            let body = axum::body::to_bytes(body, usize::MAX).await.unwrap();
            let mut ask = reqwest::Client::new()
                .request(parts.method, format!("{target}{}", parts.uri))
                .body(body);
            if let Some(kind) = parts.headers.get("content-type") {
                ask = ask.header("content-type", kind);
            }
            let answer = ask.send().await.unwrap();
            let status = answer.status();
            (status, answer.bytes().await.unwrap()).into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

/// Passkeys over the protocol (plans/accounts.md 3.1): saved from a PRF
/// output, opened again by `recover {prf}`, whose record carries `via`.
/// Over a plain link from another machine neither is taken: the output is
/// a key.
#[tokio::test]
async fn passkeys_over_the_protocol() {
    use base64::Engine as _;
    let dir = std::env::temp_dir().join(format!("mq-bridge-passkey-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let app = mq_sync_server::router(&dir.join("relay.sqlite"), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let session = |private: bool| {
        let (tx, rx) = Output::channel();
        (
            Session::over(Core::temporary("default").unwrap(), tx, private),
            rx,
        )
    };
    async fn ask(
        s: &mut Session,
        rx: &mut mpsc::UnboundedReceiver<Outgoing>,
        request: Value,
    ) -> Value {
        let id = request["id"].as_u64().unwrap();
        s.handle_text(&request.to_string()).await;
        reply_to(rx, id).await
    }
    let prf = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([5u8; 32]);
    let (mut phone, mut phone_rx) = session(true);
    let made = ask(
        &mut phone,
        &mut phone_rx,
        json!({"id": 1, "op": "createSyncAccount", "server": server, "name": "Phone", "form": "phone"}),
    )
    .await;
    let record = made["result"].clone();
    let saved = ask(
        &mut phone,
        &mut phone_rx,
        json!({"id": 2, "op": "savePasskey", "record": record, "prf": prf, "label": "Passkey, saved on the phone"}),
    )
    .await;
    assert!(saved["result"]["kit"].is_string(), "{saved}");
    let record = saved["result"]["record"].clone();
    let (mut laptop, mut laptop_rx) = session(true);
    let back = ask(
        &mut laptop,
        &mut laptop_rx,
        json!({"id": 1, "op": "recover", "server": server, "prf": prf, "name": "Laptop", "form": "computer"}),
    )
    .await;
    assert_eq!(back["result"]["via"], "passkey", "{back}");
    assert!(back["result"]["deviceToken"].is_string());
    let devices = ask(
        &mut phone,
        &mut phone_rx,
        json!({"id": 3, "op": "syncDevices", "record": record}),
    )
    .await;
    let members = devices["result"]["members"].as_array().unwrap();
    let kit = members.iter().find(|m| m["kit"] == true).unwrap();
    assert_eq!(kit["passkey"], true, "{kit}");
    assert_eq!(kit["name"], "Passkey, saved on the phone");
    let joined = members.iter().find(|m| m["viaPasskey"] == true).unwrap();
    assert_eq!(joined["form"], "computer", "{joined}");

    let (mut far, mut far_rx) = session(false);
    for request in [
        json!({"id": 1, "op": "savePasskey", "record": record, "prf": prf, "label": "x"}),
        json!({"id": 2, "op": "recover", "server": server, "prf": prf, "name": "x"}),
    ] {
        let refused = ask(&mut far, &mut far_rx, request).await;
        assert_eq!(refused["code"], "plainLink", "{refused}");
    }
    let both = ask(
        &mut laptop,
        &mut laptop_rx,
        json!({"id": 2, "op": "recover", "server": server, "prf": prf, "kit": "AAAA", "name": "x"}),
    )
    .await;
    assert_eq!(both["ok"], false, "a kit or a passkey, not both");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A native sign-in whose token request was lost on the way (no definite
/// answer): the flow stays, and the same token succeeds when sent again.
#[tokio::test]
async fn a_native_sign_in_sent_again_after_a_lost_request() {
    let (mock, server, dir) = sign_in_relay("retry").await;
    let proxy = flaky_proxy(server).await;
    let (tx, mut rx) = Output::channel();
    let mut s = Session::new(Core::temporary("default").unwrap(), tx);
    let mut ask = async |request: Value| {
        let id = request["id"].as_u64().unwrap();
        s.handle_text(&request.to_string()).await;
        reply_to(&mut rx, id).await
    };
    let started =
        ask(json!({"id": 1, "op": "nativeSignInStart", "server": proxy, "provider": "mock"})).await;
    let flow = &started["result"]["flow"];
    let token = mock.sign(mock.claims(
        "marquee",
        "alice",
        started["result"]["nonce"].as_str().unwrap(),
    ));
    let lost = ask(json!({"id": 2, "op": "nativeSignIn", "flow": flow, "idToken": token})).await;
    assert_eq!(lost["ok"], false, "{lost}");
    let again = ask(json!({"id": 3, "op": "nativeSignIn", "flow": flow, "idToken": token})).await;
    assert_eq!(again["result"]["phase"], "new", "{again}");
    // Taken: the flow is gone from the core now.
    let twice = ask(json!({"id": 4, "op": "nativeSignIn", "flow": flow, "idToken": token})).await;
    assert_eq!(twice["code"], "wrongFlow", "{twice}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Sign-in over the protocol (plans/accounts.md 3.4), against the relay and
/// its mock provider: the relay's providers, a native sheet's token and the
/// brokered `signIn` view each end in a ticket; the ticket makes an account
/// linked to the identity, which the next sign-in finds. A flow this
/// session did not start is refused; a return for another flow is ignored,
/// a cancelled one fails the view. Links and unlinks answer `{record}`, the
/// owner only (D16); any profile reads the links and cancels a deletion.
#[tokio::test]
async fn sign_in_over_the_protocol() {
    let (mock, server, dir) = sign_in_relay("signin").await;
    let (tx, mut rx) = Output::channel();
    let mut s = Session::new(Core::temporary("default").unwrap(), tx);
    let mut ask = async |request: Value| {
        let id = request["id"].as_u64().unwrap();
        s.handle_text(&request.to_string()).await;
        reply_to(&mut rx, id).await
    };
    let offered = ask(json!({"id": 20, "op": "relayInfo", "server": server})).await;
    assert_eq!(
        offered["result"],
        json!({ "providers": ["mock"] }),
        "{offered}"
    );
    let started =
        ask(json!({"id": 1, "op": "nativeSignInStart", "server": server, "provider": "mock"}))
            .await;
    let (flow, nonce) = (
        &started["result"]["flow"],
        started["result"]["nonce"].as_str().unwrap(),
    );
    assert!(
        started["result"]["expiresMs"].as_u64().unwrap() > 0,
        "{started}"
    );
    let token = mock.sign(mock.claims("marquee", "alice", nonce));
    let signed = ask(json!({"id": 2, "op": "nativeSignIn", "flow": flow, "idToken": token})).await;
    assert_eq!(signed["result"]["phase"], "new", "{signed}");
    let ticket = &signed["result"]["ticket"];
    let made = ask(json!({"id": 3, "op": "createSyncAccount", "server": server, "name": "Phone", "ticket": ticket})).await;
    let record = made["result"].clone();
    let links = ask(json!({"id": 4, "op": "identities", "record": record})).await;
    assert_eq!(
        links["result"]["links"].as_array().map(Vec::len),
        Some(1),
        "{links}"
    );
    let refused = ask(json!({"id": 5, "op": "nativeSignIn", "flow": "nope", "idToken": "x"})).await;
    assert_eq!(refused["code"], "wrongFlow", "{refused}");

    mock.next(mock_oidc::Next::Pick("alice".into()));
    let params = json!({"server": server, "provider": "mock", "return": "marquee://auth"});
    s.handle_text(
        &json!({"id": 6, "op": "subscribe", "view": "signIn", "params": params}).to_string(),
    )
    .await;
    let sub = reply_to(&mut rx, 6).await["result"]["sub"]
        .as_u64()
        .unwrap();
    let browser = push_until(&mut rx, sub, |u| u["phase"] == "browser").await;
    let fragment = mock_oidc::browse(browser["url"].as_str().unwrap()).await;
    s.handle_text(
        &json!({"id": 7, "op": "command", "sub": sub, "command": {"type": "returned", "fragment": fragment}})
            .to_string(),
    )
    .await;
    let found = push_until(&mut rx, sub, |u| {
        u["phase"] == "found" || u["phase"] == "failed"
    })
    .await;
    assert_eq!(found["account"], record["keys"]["account"], "{found}");

    // A return for another flow is ignored; this flow's cancel fails the view.
    mock.next(mock_oidc::Next::Cancel);
    s.handle_text(
        &json!({"id": 8, "op": "subscribe", "view": "signIn", "params": params}).to_string(),
    )
    .await;
    let sub = reply_to(&mut rx, 8).await["result"]["sub"]
        .as_u64()
        .unwrap();
    let browser = push_until(&mut rx, sub, |u| u["phase"] == "browser").await;
    assert!(browser["expiresMs"].as_u64().unwrap() > 0, "{browser}");
    let returned = |id: u64, fragment: &str| {
        json!({"id": id, "op": "command", "sub": sub, "command": {"type": "returned", "fragment": fragment}})
            .to_string()
    };
    s.handle_text(&returned(9, "flow=another&code=x&relay=127.0.0.1"))
        .await;
    let cancelled = mock_oidc::browse(browser["url"].as_str().unwrap()).await;
    s.handle_text(&returned(10, &cancelled)).await;
    let failed = push_until(&mut rx, sub, |u| u["phase"] == "failed").await;
    assert_eq!(failed["code"], "cancelled", "{failed}");

    // Link and unlink: `{record}` (none: the stored token was good).
    let mut ask = async |request: Value| {
        let id = request["id"].as_u64().unwrap();
        s.handle_text(&request.to_string()).await;
        reply_to(&mut rx, id).await
    };
    let bob =
        ask(json!({"id": 11, "op": "nativeSignInStart", "server": server, "provider": "mock"}))
            .await;
    let token = mock.sign(mock.claims("marquee", "bob", bob["result"]["nonce"].as_str().unwrap()));
    let bob = ask(
        json!({"id": 12, "op": "nativeSignIn", "flow": bob["result"]["flow"], "idToken": token}),
    )
    .await;
    let ticket = &bob["result"]["ticket"];
    let linked =
        ask(json!({"id": 13, "op": "linkIdentity", "record": record, "ticket": ticket})).await;
    assert_eq!(linked["result"], json!({ "record": null }), "{linked}");
    let links = ask(json!({"id": 14, "op": "identities", "record": record})).await;
    let links = links["result"]["links"].as_array().unwrap().clone();
    assert_eq!(links.len(), 2);
    let link = &links[1]["link"];

    // Another profile reads the links and cancels a deletion; it does not
    // change who signs in.
    let (tx_p, mut rx_p) = Output::channel();
    let mut p = Session::new(Core::temporary("p").unwrap(), tx_p);
    let mut ask_p = async |request: Value| {
        let id = request["id"].as_u64().unwrap();
        p.handle_text(&request.to_string()).await;
        reply_to(&mut rx_p, id).await
    };
    let refused =
        ask_p(json!({"id": 1, "op": "unlinkIdentity", "record": record, "link": link})).await;
    assert_eq!(refused["code"], "notOwner", "{refused}");
    let refused =
        ask_p(json!({"id": 2, "op": "linkIdentity", "record": record, "ticket": ticket})).await;
    assert_eq!(refused["code"], "notOwner", "{refused}");
    let read = ask_p(json!({"id": 3, "op": "identities", "record": record})).await;
    assert_eq!(
        read["result"]["links"].as_array().map(Vec::len),
        Some(2),
        "{read}"
    );
    let kept = ask_p(json!({"id": 4, "op": "cancelDeletion", "record": record})).await;
    assert_eq!(kept["result"], json!({ "record": null }), "{kept}");

    let unlinked =
        ask(json!({"id": 15, "op": "unlinkIdentity", "record": record, "link": link})).await;
    assert_eq!(unlinked["result"], json!({ "record": null }), "{unlinked}");
    let links = ask(json!({"id": 16, "op": "identities", "record": record})).await;
    assert_eq!(
        links["result"]["links"].as_array().map(Vec::len),
        Some(1),
        "{links}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
