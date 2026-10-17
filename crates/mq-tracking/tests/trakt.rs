use mq_http::{ClientConfig, HttpClient};
use mq_tracking::trakt::{Answer, Api, Auth, AuthError, Hosts, Poll, TraktRecord, auth};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A scripted reply: status, content type, body, delay.
#[derive(Clone)]
struct Reply(u16, &'static str, String, Duration);

fn json(status: u16, body: &str) -> Reply {
    Reply(status, "application/json", body.to_owned(), Duration::ZERO)
}

/// One request as the mock saw it.
#[derive(Debug, Clone)]
struct Seen {
    path: String,
    head: String,
    body: String,
}

#[derive(Clone, Default)]
struct Mock {
    script: Arc<Mutex<HashMap<String, VecDeque<Reply>>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Mock {
    fn on(&self, path: &str, replies: Vec<Reply>) {
        self.script
            .lock()
            .unwrap()
            .entry(path.into())
            .or_default()
            .extend(replies);
    }

    fn seen(&self, path: &str) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == path)
            .cloned()
            .collect()
    }

    async fn start(&self) -> Hosts {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mock = self.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mock = mock.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    // Headers, then the body by Content-Length.
                    let (head, body) = loop {
                        let n = socket.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let head = text[..end].to_owned();
                            let len = head
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse::<usize>().ok())?
                                })
                                .unwrap_or(0);
                            if buf.len() >= end + 4 + len {
                                break (head, text[end + 4..end + 4 + len].to_owned());
                            }
                        }
                    };
                    let path = head
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .split('?')
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    mock.seen.lock().unwrap().push(Seen {
                        path: path.clone(),
                        head: head.to_ascii_lowercase(),
                        body,
                    });
                    let reply = mock
                        .script
                        .lock()
                        .unwrap()
                        .get_mut(&path)
                        .and_then(VecDeque::pop_front)
                        .unwrap_or(json(404, ""));
                    tokio::time::sleep(reply.3).await;
                    let resp = format!(
                        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        reply.0,
                        reply.1,
                        reply.2.len(),
                        reply.2
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });
        let base = format!("http://{addr}");
        Hosts {
            api: base.clone(),
            auth: base,
            local: true,
        }
    }
}

fn api(hosts: Hosts) -> Api {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    Api::new(http, hosts, "client-1", "Marquee/test")
}

fn tokens_body(access: &str, refresh: &str) -> String {
    let now = auth::now_ms() / 1000;
    format!(
        r#"{{"access_token":"{access}","token_type":"bearer","expires_in":604800,"refresh_token":"{refresh}","scope":"public","created_at":{now}}}"#
    )
}

/// A record whose token is past three quarters of its life.
fn aging_record() -> TraktRecord {
    let now = auth::now_ms();
    TraktRecord {
        access_token: "old-access".into(),
        refresh_token: "old-refresh".into(),
        created_at_ms: now - 6 * 86_400_000,
        expires_at_ms: now + 86_400_000,
        scope: "public".into(),
        account: "someone".into(),
        revision: 0,
        generation: 1,
    }
}

#[tokio::test]
async fn device_flow_waits_slows_down_and_gets_tokens() {
    let mock = Mock::default();
    mock.on(
        "/oauth/device/code",
        vec![json(
            200,
            r#"{"device_code":"dc","user_code":"5055CC52","verification_url":"https://trakt.tv/activate","expires_in":600,"interval":5}"#,
        )],
    );
    mock.on(
        "/oauth/device/token",
        vec![
            json(400, ""),
            json(429, ""),
            json(200, &tokens_body("a1", "r1")),
        ],
    );
    let api = api(mock.start().await);
    let code = auth::start(&api).await.unwrap();
    assert_eq!(code.user_code, "5055CC52");
    assert_eq!(code.interval, Duration::from_secs(5));
    assert_eq!(auth::poll(&api, &code).await, Poll::Pending);
    assert_eq!(auth::poll(&api, &code).await, Poll::SlowDown);
    match auth::poll(&api, &code).await {
        Poll::Approved(t) => {
            assert_eq!(t.access_token, "a1");
            assert_eq!(t.expires_at_ms - t.created_at_ms, 604_800_000);
        }
        other => panic!("{other:?}"),
    }
    let sent = mock.seen("/oauth/device/token");
    assert!(sent[0].body.contains(r#""code":"dc""#));
    assert!(sent.iter().all(|s| !s.body.contains("client_secret")));
    assert!(sent[0].head.contains("trakt-api-key: client-1"));
    assert!(sent[0].head.contains("user-agent: marquee/test"));
}

#[tokio::test]
async fn device_poll_endings_are_told_apart() {
    let mock = Mock::default();
    mock.on(
        "/oauth/device/token",
        vec![json(410, ""), json(418, ""), json(409, ""), json(404, "")],
    );
    let api = api(mock.start().await);
    let code = auth::DeviceCode {
        device_code: "dc".into(),
        user_code: "X".into(),
        verification_url: "u".into(),
        expires_in: Duration::from_secs(600),
        interval: Duration::from_secs(5),
    };
    for want in [Poll::Expired, Poll::Denied, Poll::Invalid, Poll::Invalid] {
        assert_eq!(auth::poll(&api, &code).await, want);
    }
}

#[tokio::test]
async fn cloudflare_pages_and_broken_json_are_transient() {
    let mock = Mock::default();
    mock.on(
        "/users/settings",
        vec![
            Reply(
                403,
                "text/html",
                "<html>Attention Required</html>".into(),
                Duration::ZERO,
            ),
            Reply(
                200,
                "text/html",
                "<html>captcha</html>".into(),
                Duration::ZERO,
            ),
            json(200, "{not json"),
            json(503, ""),
            json(420, r#"{"error":"limit"}"#),
        ],
    );
    let api = api(mock.start().await);
    let gate = Default::default();
    for _ in 0..4 {
        assert!(
            matches!(
                api.get(&gate, "/users/settings", "t").await,
                Answer::Transient { .. }
            ),
            "a transient answer"
        );
    }
    assert_eq!(
        api.get(&gate, "/users/settings", "t").await,
        Answer::Refused {
            status: 420,
            error: Some("limit".into())
        }
    );
}

#[tokio::test]
async fn refresh_runs_once_under_concurrency_and_waits_for_the_ack() {
    let mock = Mock::default();
    mock.on(
        "/oauth/token",
        vec![Reply(
            200,
            "application/json",
            tokens_body("new-access", "new-refresh"),
            Duration::from_millis(200),
        )],
    );
    let api = Arc::new(api(mock.start().await));
    let auth = Arc::new(Auth::new("https://example.org/trakt"));
    assert!(auth.set(Some(aging_record())).await);
    let calls: Vec<_> = (0..5)
        .map(|_| {
            let (api, auth) = (api.clone(), auth.clone());
            tokio::spawn(async move { auth.token(&api).await })
        })
        .collect();
    for call in calls {
        assert_eq!(call.await.unwrap().unwrap(), "new-access");
    }
    let sent = mock.seen("/oauth/token");
    assert_eq!(sent.len(), 1, "one refresh for five callers");
    assert!(
        sent[0]
            .body
            .contains(r#""redirect_uri":"https://example.org/trakt""#)
    );
    assert!(!sent[0].body.contains("client_secret"));

    let rotated = auth.unacked().await.expect("held for the app");
    assert_eq!(rotated.revision, 1);
    // An ack of the old revision changes nothing; the right one clears it.
    assert!(!auth.set(Some(aging_record())).await);
    assert!(auth.unacked().await.is_some());
    assert!(auth.set(Some(rotated)).await);
    assert!(auth.unacked().await.is_none());
}

#[tokio::test]
async fn a_401_for_an_older_token_does_not_refresh_again() {
    let mock = Mock::default();
    mock.on(
        "/oauth/token",
        vec![json(200, &tokens_body("new-access", "new-refresh"))],
    );
    let api = api(mock.start().await);
    let auth = Auth::new("https://example.org/trakt");
    auth.set(Some(aging_record())).await;
    assert_eq!(auth.token(&api).await.unwrap(), "new-access");
    // A call made with the old token comes back 401 after the rotation.
    assert_eq!(
        auth.after_401(&api, "old-access").await.unwrap(),
        "new-access"
    );
    assert_eq!(mock.seen("/oauth/token").len(), 1);
}

#[tokio::test]
async fn invalid_grant_needs_a_new_sign_in() {
    let mock = Mock::default();
    mock.on(
        "/oauth/token",
        vec![json(
            400,
            r#"{"error":"invalid_grant","error_description":"session not found"}"#,
        )],
    );
    let api = api(mock.start().await);
    let auth = Auth::new("https://example.org/trakt");
    auth.set(Some(aging_record())).await;
    let err = auth.token(&api).await.unwrap_err();
    assert!(
        matches!(&err, AuthError::Reconnect(r) if r.contains("invalid_grant")),
        "{err:?}"
    );
    // No more refreshes, and the reason carries no response body.
    assert!(matches!(
        auth.token(&api).await,
        Err(AuthError::Reconnect(_))
    ));
    assert_eq!(mock.seen("/oauth/token").len(), 1);
    assert!(!format!("{err}").contains("session not found"));
}

#[tokio::test]
async fn signing_out_during_a_refresh_stays_signed_out() {
    let mock = Mock::default();
    mock.on(
        "/oauth/token",
        vec![Reply(
            200,
            "application/json",
            tokens_body("new-access", "new-refresh"),
            Duration::from_millis(300),
        )],
    );
    let api = Arc::new(api(mock.start().await));
    let auth = Arc::new(Auth::new("https://example.org/trakt"));
    auth.set(Some(aging_record())).await;
    let refreshing = {
        let (api, auth) = (api.clone(), auth.clone());
        tokio::spawn(async move { auth.token(&api).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(auth.set(None).await);
    let _ = refreshing.await.unwrap();
    assert_eq!(auth.status().await, Err(AuthError::Off));
    assert!(auth.unacked().await.is_none());
}

#[tokio::test]
async fn a_fresh_sign_in_is_held_until_stored() {
    let auth = Auth::new("https://example.org/trakt");
    let tokens = auth::Tokens {
        access_token: "a".into(),
        refresh_token: "r".into(),
        created_at_ms: 1,
        expires_at_ms: 2,
        scope: "public".into(),
    };
    let record = auth.adopt(tokens, "someone").await;
    assert_eq!(record.generation, 1);
    assert_eq!(auth.unacked().await.as_ref(), Some(&record));
    assert!(auth.set(Some(record)).await);
    assert!(auth.unacked().await.is_none());
    assert_eq!(auth.account().await.as_deref(), Some("someone"));
    // The record never shows its tokens in logs.
    let shown = format!("{:?}", auth.unacked().await);
    assert!(!shown.contains("\"a\""));
}

/// Live, no client id needed: how the real hosts answer an unknown app.
#[tokio::test]
#[ignore = "network"]
async fn live_unknown_app_is_refused_not_transient() {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let api = Api::new(
        http,
        Hosts::default(),
        "marquee-unknown-client",
        "Marquee/test",
    );
    let started = std::time::Instant::now();
    let answer = api
        .auth(
            "/oauth/device/code",
            &serde_json::json!({"client_id": "marquee-unknown-client"}),
        )
        .await;
    println!(
        "device code with an unknown app: {answer:?} in {:?}",
        started.elapsed()
    );
    assert_eq!(
        answer,
        Answer::Refused {
            status: 401,
            error: Some("invalid_client".into())
        }
    );
    let settings = api
        .get(&Default::default(), "/users/settings", "not-a-token")
        .await;
    println!("settings with an unknown app: {settings:?}");
    assert!(!matches!(settings, Answer::Ok { .. }));
}

#[tokio::test]
async fn a_late_acknowledgement_never_signs_back_in() {
    let auth = Auth::new("https://example.org/trakt");
    let record = aging_record();
    assert!(auth.set(Some(record.clone())).await);
    assert!(auth.set(None).await);
    // The app's acknowledgement of the old record arrives after sign-out.
    assert!(!auth.set(Some(record)).await);
    assert_eq!(auth.status().await, Err(AuthError::Off));
}

/// A caller that goes away mid-refresh does not lose the rotated token:
/// Trakt already burned the old refresh token.
#[tokio::test]
async fn a_dropped_caller_keeps_the_rotated_token() {
    let mock = Mock::default();
    mock.on(
        "/oauth/token",
        vec![Reply(
            200,
            "application/json",
            tokens_body("new-access", "new-refresh"),
            Duration::from_millis(300),
        )],
    );
    let api = api(mock.start().await);
    let auth = Auth::new("https://example.org/trakt");
    auth.set(Some(aging_record())).await;
    let dropped = tokio::time::timeout(Duration::from_millis(50), auth.token(&api)).await;
    assert!(dropped.is_err(), "the caller gave up");
    tokio::time::sleep(Duration::from_millis(600)).await;
    let rotated = auth
        .unacked()
        .await
        .expect("the refresh finished on its own");
    assert_eq!(rotated.revision, 1);
    assert_eq!(rotated.access_token, "new-access");
}

/// With Trakt unreachable, one refresh is tried; calls then fail at once
/// for a while, and signing out never waits for the network.
#[tokio::test]
async fn an_unanswered_refresh_backs_off_and_never_blocks_sign_out() {
    let mock = Mock::default();
    mock.on("/oauth/token", vec![json(503, ""), json(503, "")]);
    let api = api(mock.start().await);
    let auth = Auth::new("https://example.org/trakt");
    auth.set(Some(aging_record())).await;
    assert!(matches!(
        auth.token(&api).await,
        Err(AuthError::Transient(_))
    ));
    let started = std::time::Instant::now();
    assert!(matches!(
        auth.token(&api).await,
        Err(AuthError::Transient(_))
    ));
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "no second refresh"
    );
    assert_eq!(mock.seen("/oauth/token").len(), 1);
    assert!(auth.set(None).await);
    assert_eq!(auth.status().await, Err(AuthError::Off));
}
