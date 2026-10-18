use mq_bridge::{Outgoing, Output, Session};
use mq_runtime::Core;
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
    assert_eq!(next(&mut rx).await["ok"], true);
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
