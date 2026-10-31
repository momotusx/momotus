//! Trakt over the protocol: a build with no Trakt app answers every op
//! and view with `notConfigured`.

use mq_bridge::{Outgoing, Output, Session};
use mq_runtime::Core;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

/// The reply to request `id`, skipping pushes and other replies.
async fn reply(rx: &mut mpsc::UnboundedReceiver<Outgoing>, id: u64) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("reply")
            .expect("open")
            .json();
        if message["id"] == id {
            return message;
        }
    }
}

#[tokio::test]
async fn trakt_ops_say_not_configured_without_an_app() {
    let (tx, mut rx) = Output::channel();
    let mut session = Session::new(Core::temporary("p").unwrap(), tx);
    let record = json!({
        "accessToken": "a", "refreshToken": "r", "createdAtMs": 1, "expiresAtMs": 2,
        "scope": "public", "account": "u", "user": "u", "revision": 0, "generation": 1
    });
    session
        .handle_text(&json!({"id": 1, "op": "setTrakt", "record": record}).to_string())
        .await;
    let refused = reply(&mut rx, 1).await;
    assert_eq!(refused["code"], "notConfigured", "{refused}");
    // Its words are a sentence for the screen.
    assert_eq!(refused["kind"], "said", "{refused}");
    session
        .handle_text(&json!({"id": 2, "op": "traktSync"}).to_string())
        .await;
    assert_eq!(reply(&mut rx, 2).await["code"], "notConfigured");
    // No record is always fine.
    session
        .handle_text(&json!({"id": 3, "op": "setTrakt", "record": null}).to_string())
        .await;
    assert_eq!(reply(&mut rx, 3).await["ok"], true);
    session
        .handle_text(&json!({"id": 4, "op": "subscribe", "view": "trakt"}).to_string())
        .await;
    let status = reply(&mut rx, 4).await;
    assert_eq!(
        status["result"]["snapshot"]["state"], "notConfigured",
        "{status}"
    );
    session
        .handle_text(&json!({"id": 5, "op": "subscribe", "view": "traktLogin"}).to_string())
        .await;
    let login = reply(&mut rx, 5).await;
    assert_eq!(
        login["result"]["snapshot"]["phase"], "notConfigured",
        "{login}"
    );
    session
        .handle_text(&json!({"id": 6, "op": "lifecycle", "event": "background"}).to_string())
        .await;
    assert_eq!(reply(&mut rx, 6).await["ok"], true);
}
