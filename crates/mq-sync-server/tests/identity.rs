//! Sign-in through a real relay and the mock provider (plans/accounts.md 2
//! and 7): brokered and native flows, what an attacker or a proxying relay
//! gets (nothing), ID token checks, tickets, sign-ups, links, Apple's
//! revocation, Facebook's code exchange, the replay gate, scheduled
//! deletion, CORS for the link host, and the identity key's fingerprint.
//! Real providers are never called.

#[path = "support/mock_oidc.rs"]
mod mock_oidc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use mock_oidc::{Mock, Next, browse};
use mq_http::{ClientConfig, HttpClient, HttpError, RequestOptions};
use mq_library::Library;
use mq_library::hlc::HlcClock;
use mq_sync::identity::{start_brokered, start_native};
use mq_sync::pair::{approve_pairing, start_pairing};
use mq_sync::roster::{Hex, MemberSecret, Roster};
use mq_sync::wire::{
    ReplayRequest, ScheduledDeletion, StartRequest, StartResponse, Taken, nonce_for,
};
use mq_sync::{SyncConfig, SyncError, SyncReport, create_account_with, sync_once};
use mq_sync_server::{Config, IdentityFile, router};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

fn raw(headers: Vec<(String, String)>) -> RequestOptions {
    RequestOptions {
        allow_private: true,
        headers,
        ..Default::default()
    }
}

fn bearer(token: &str) -> RequestOptions {
    raw(vec![("authorization".into(), format!("Bearer {token}"))])
}

fn fresh_dir() -> PathBuf {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    let dir = std::env::temp_dir().join(format!("mq-identity-{}", hex::encode(b)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The identity file for a relay at `relay` whose every provider is the mock.
fn identity(mock: &Mock, relay: &str, key: &str, apple_key: &str) -> IdentityFile {
    serde_json::from_value(identity_json(mock, relay, key, apple_key)).unwrap()
}

fn identity_json(mock: &Mock, relay: &str, key: &str, apple_key: &str) -> Value {
    json!({
        "key": key,
        "publicUrl": relay,
        "linkHost": "link.example",
        "apple": {
            "teamId": "TEAM", "keyId": "KEY1", "key": apple_key, "servicesId": "apple-web",
            "bundleIds": ["app.ios", "app.tv"], "issuer": mock.url,
        },
        "google": {
            "webClientId": "google-web", "webClientSecret": "gs", "clientIds": ["google-android"],
            "issuer": mock.url,
        },
        "facebook": { "appId": "fb-app", "appSecret": "fs", "graph": mock.url, "dialog": format!("{}/authorize", mock.url) },
        "oidc": [{ "id": "mock", "issuer": mock.url, "clientId": "marquee", "clientSecret": "ms" }],
    })
}

struct Relay {
    url: String,
    db: PathBuf,
    offset: Arc<AtomicI64>,
    mock: Mock,
    apple_key: String,
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(dir) = self.db.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl Relay {
    async fn new() -> Self {
        Self::with(|_| {}).await
    }

    async fn with(tweak: impl FnOnce(&mut Config)) -> Self {
        let mock = Mock::start(0).await;
        let apple_key = mock_oidc::apple_key_pem();
        Self::on(
            mock,
            apple_key,
            fresh_dir().join("relay.sqlite"),
            Arc::default(),
            tweak,
        )
        .await
    }

    async fn on(
        mock: Mock,
        apple_key: String,
        db: PathBuf,
        offset: Arc<AtomicI64>,
        tweak: impl FnOnce(&mut Config),
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let mut config = Config {
            clock_offset_ms: offset.clone(),
            public_host: Some("127.0.0.1".into()),
            identity: Some(identity(&mock, &url, KEY, &apple_key)),
            ..Config::default()
        };
        tweak(&mut config);
        let app = router(&db, config).unwrap();
        tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
        Self {
            url,
            db,
            offset,
            mock,
            apple_key,
        }
    }

    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.db).unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        self.sql()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn later(&self, ms: i64) {
        self.offset.fetch_add(ms, Ordering::Relaxed);
    }

    /// A raw native start, for the relay's own nonce.
    async fn native_start(&self, provider: &str) -> StartResponse {
        http()
            .post_json(
                &format!("{}/v1/auth/{provider}/start", self.url),
                &StartRequest {
                    verifier_hash: hex::encode(sha2_hash("v")),
                    purpose: "signin".into(),
                    return_to: None,
                    native: true,
                },
                &raw(vec![]),
            )
            .await
            .unwrap()
            .json()
            .unwrap()
    }

    /// A brokered sign-in as `sub` at `provider`, the browser emulated: the
    /// ticket, and the account it signs in to.
    async fn sign_in(&self, provider: &str, sub: &str) -> Taken {
        self.mock.next(Next::Pick(sub.into()));
        let flow = start_brokered(&http(), &self.url, provider, "marquee://auth", false)
            .await
            .unwrap();
        let fragment = browse(&flow.url).await;
        flow.finish(&http(), &fragment).await.unwrap()
    }

    /// A native sign-in with a token the mock signs for `aud` and `sub`.
    async fn native(&self, provider: &str, aud: &str, sub: &str) -> Result<Taken, SyncError> {
        let flow = start_native(&http(), &self.url, provider, false)
            .await
            .unwrap();
        let token = self.mock.sign(self.mock.claims(aud, sub, &flow.nonce));
        flow.finish(&http(), &token, None).await
    }
}

fn sha2_hash(text: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes()).to_vec()
}

struct Device {
    record: SyncConfig,
    library: Library,
    clock: HlcClock,
}

impl Device {
    async fn create(relay: &Relay, taken: &Taken, move_link: bool) -> Result<Self, SyncError> {
        let record = create_account_with(
            &http(),
            &relay.url,
            None,
            "Phone",
            Some(&taken.ticket),
            move_link,
        )
        .await?;
        Ok(Self {
            record,
            library: Library::temporary().unwrap(),
            clock: HlcClock::new(1),
        })
    }

    async fn links(&self) -> Vec<mq_sync::wire::IdentityLink> {
        mq_sync::identity::identities(&self.library, &http(), &self.record, &self.clock)
            .await
            .unwrap()
            .0
    }

    async fn link(&self, taken: &Taken, move_link: bool) -> Result<(), SyncError> {
        mq_sync::identity::link_identity(
            &self.library,
            &http(),
            &self.record,
            &self.clock,
            &taken.ticket,
            move_link,
        )
        .await
        .map(drop)
    }

    /// Syncs until the record settles, storing it as an app does.
    async fn sync(&mut self) -> SyncReport {
        loop {
            let report = sync_once(&self.library, &http(), &self.record, &self.clock)
                .await
                .unwrap();
            if let Some(record) = report.record.clone() {
                self.record = record;
            }
            if !report.again {
                return report;
            }
        }
    }

    /// A second device joins by pairing.
    async fn pair(&mut self) -> Device {
        let tv = start_pairing(&http(), &self.record.server).await.unwrap();
        let (code, record) =
            approve_pairing(&self.library, &http(), &self.record, &self.clock, &tv.qr)
                .await
                .unwrap();
        self.record = record;
        let offer = tv.poll(&http()).await.unwrap().expect("offer");
        assert_eq!(offer.code, code);
        Device {
            record: tv.claim(&http(), offer, "TV").await.unwrap(),
            library: Library::temporary().unwrap(),
            clock: HlcClock::new(2),
        }
    }

    async fn cancel_deletion(&self, unlink: bool) {
        mq_sync::identity::cancel_deletion(
            &self.library,
            &http(),
            &self.record,
            &self.clock,
            unlink,
        )
        .await
        .unwrap();
    }
}

/// The deletion page's request with a sign-in's ticket: when it is due.
async fn ask_deletion(relay: &Relay, taken: Taken) -> u64 {
    http()
        .post_json(
            &format!("{}/v1/account/delete-request", relay.url),
            &json!({ "ticket": taken.ticket }),
            &raw(vec![]),
        )
        .await
        .unwrap()
        .json::<Value>()
        .unwrap()["dueMs"]
        .as_u64()
        .unwrap()
}

fn status(answer: Result<mq_http::Response, HttpError>) -> u16 {
    match answer {
        Ok(r) => r.status,
        Err(HttpError::Status { status, .. }) => status,
        Err(e) => panic!("{e}"),
    }
}

#[tokio::test]
async fn a_brokered_sign_in_makes_an_account_and_finds_it_again() {
    let relay = Relay::new().await;
    let taken = relay.sign_in("mock", "alice").await;
    assert_eq!(taken.account, None, "new");
    let phone = Device::create(&relay, &taken, false).await.unwrap();
    assert_eq!((relay.count("identity"), relay.count("signups")), (1, 1));
    let again = relay.sign_in("mock", "alice").await;
    assert_eq!(again.account, Some(phone.record.keys.account), "found");
    let links = phone.links().await;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].provider, format!("oidc:{}", relay.mock.url));
    // Nothing of the provider is kept: no subject in the clear.
    let subject: Vec<u8> = relay
        .sql()
        .query_row("SELECT subject FROM identity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(subject.len(), 32);
    assert!(!subject.windows(5).any(|w| w == b"alice"));
    // A cancelled provider page comes back as cancelled.
    relay.mock.next(Next::Cancel);
    let flow = start_brokered(&http(), &relay.url, "mock", "marquee://auth", false)
        .await
        .unwrap();
    let err = flow
        .finish(&http(), &browse(&flow.url).await)
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Cancelled), "{err:?}");
    // The provider's ID token names another nonce than the flow's.
    relay.mock.next(Next::WrongNonce("alice".into()));
    let flow = start_brokered(&http(), &relay.url, "mock", "marquee://auth", false)
        .await
        .unwrap();
    let fragment = browse(&flow.url).await;
    assert!(fragment.contains("error=refused"), "{fragment}");
    let err = flow.finish(&http(), &fragment).await.unwrap_err();
    assert!(matches!(err, SyncError::SignInRefused), "{err:?}");
}

/// The fragment's code and the starter's verifier together, once: a browser
/// that finished the provider step has no verifier, a starter without that
/// browser has no code. A return for another flow or naming another relay
/// is refused before anything is sent.
#[tokio::test]
async fn a_flow_needs_its_verifier_and_its_code_once() {
    let relay = Relay::new().await;
    relay.mock.next(Next::Pick("alice".into()));
    let flow = start_brokered(&http(), &relay.url, "mock", "marquee://auth", false)
        .await
        .unwrap();
    let fragment = browse(&flow.url).await;
    let field = |key: &str| {
        url::form_urlencoded::parse(fragment.as_bytes())
            .find(|(k, _)| k == key)
            .unwrap()
            .1
            .into_owned()
    };
    let take = |verifier: &str, code: &str| {
        let body = json!({ "flow": field("flow"), "verifier": verifier, "code": code });
        let url = format!("{}/v1/auth/take", relay.url);
        async move { status(http().post_json(&url, &body, &raw(vec![])).await) }
    };
    assert_eq!(take("not the verifier", &field("code")).await, 404);
    assert_eq!(take("", "").await, 404);
    let other = flow
        .finish(&http(), &fragment.replace(&field("code"), "nope"))
        .await
        .unwrap_err();
    assert!(matches!(other, SyncError::SignInRefused), "{other:?}");
    let elsewhere = fragment.replace("relay=127.0.0.1", "relay=evil.example");
    assert!(matches!(
        flow.finish(&http(), &elsewhere).await,
        Err(SyncError::OtherRelay)
    ));
    let another = fragment.replace(&field("flow"), "another-flow");
    assert!(matches!(
        flow.finish(&http(), &another).await,
        Err(SyncError::WrongFlow)
    ));
    assert!(flow.owns(&fragment));
    assert!(!flow.owns(&elsewhere) && !flow.owns(&another));
    let taken = flow.finish(&http(), &fragment).await.unwrap();
    assert!(!taken.ticket.is_empty());
    let twice = flow.finish(&http(), &fragment).await.unwrap_err();
    assert!(matches!(twice, SyncError::SignInRefused), "{twice:?}");
}

/// A flow past its time says so: the late callback sends the app back with
/// `expired`, and a take or a native token hears `ticketExpired`. A
/// callback the relay cannot finish (done already, unknown) shows a page
/// that sends the user back to the app; a finished flow is finished once.
#[tokio::test]
async fn an_expired_flow_says_so_and_a_callback_completes_once() {
    let relay = Relay::new().await;
    relay.mock.next(Next::Pick("alice".into()));
    let late = start_brokered(&http(), &relay.url, "mock", "marquee://auth", false)
        .await
        .unwrap();
    let native = start_native(&http(), &relay.url, "mock", false)
        .await
        .unwrap();
    assert!(late.expires_ms > 9 * 60 * 1000 && native.expires_ms >= late.expires_ms);
    relay.later(11 * 60 * 1000);
    let fragment = browse(&late.url).await;
    assert!(fragment.contains("error=expired"), "{fragment}");
    let expired = late.finish(&http(), &fragment).await.unwrap_err();
    assert!(matches!(expired, SyncError::TicketExpired), "{expired:?}");
    let token = relay
        .mock
        .sign(relay.mock.claims("marquee", "alice", &native.nonce));
    let expired = native.finish(&http(), &token, None).await.unwrap_err();
    assert!(matches!(expired, SyncError::TicketExpired), "{expired:?}");

    // Finished once: a second callback for the same flow is a page, not a return.
    relay.mock.next(Next::Pick("alice".into()));
    let flow = start_brokered(&http(), &relay.url, "mock", "marquee://auth", false)
        .await
        .unwrap();
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let provider = browser.get(&flow.url).send().await.unwrap();
    let callback = provider.headers()["location"].to_str().unwrap().to_owned();
    let first = browser.get(&callback).send().await.unwrap();
    assert_eq!(first.status(), 303);
    let again = browser.get(&callback).send().await.unwrap();
    assert_eq!(again.status(), 400);
    let page = again.text().await.unwrap();
    assert!(page.contains("Go back to the app"), "{page}");
    let unknown = browser
        .get(format!(
            "{}/v1/auth/mock/callback?state=nope&code=x",
            relay.url
        ))
        .send()
        .await
        .unwrap();
    assert!(unknown.text().await.unwrap().contains("Go back to the app"));
}

/// A relay forwarding another relay's flow cannot make the nonce match:
/// the claim is bound to the host the app is configured with.
#[tokio::test]
async fn a_nonce_bound_to_another_host_is_refused() {
    let relay = Relay::new().await;
    let started = relay.native_start("mock").await;
    let nonce = B64URL.decode(started.nonce.unwrap()).unwrap();
    let token = relay.mock.sign(relay.mock.claims(
        "marquee",
        "alice",
        &nonce_for(&nonce, "evil.example"),
    ));
    let body = json!({ "flow": started.flow, "verifier": "v", "idToken": token });
    let answer = http()
        .post_json(&format!("{}/v1/auth/token", relay.url), &body, &raw(vec![]))
        .await;
    assert_eq!(status(answer), 403);
    let token = relay.mock.sign(relay.mock.claims(
        "marquee",
        "alice",
        &nonce_for(&nonce, "127.0.0.1"),
    ));
    let body = json!({ "flow": started.flow, "verifier": "v", "idToken": token });
    let answer = http()
        .post_json(&format!("{}/v1/auth/token", relay.url), &body, &raw(vec![]))
        .await;
    assert_eq!(status(answer), 200);
}

/// Every claim a token must carry, each broken on its own; a signature by
/// an unpublished key; a key rotated since the JWKS was cached, fetched once.
/// A native sign-in at the mock with its claims changed and signed by `sign`.
async fn check(
    relay: &Relay,
    change: fn(&mut Value),
    sign: fn(&Mock, Value) -> String,
) -> Result<Taken, SyncError> {
    let flow = start_native(&http(), &relay.url, "mock", false)
        .await
        .unwrap();
    let mut claims = relay.mock.claims("marquee", "alice", &flow.nonce);
    change(&mut claims);
    flow.finish(&http(), &sign(&relay.mock, claims), None).await
}

#[tokio::test]
async fn id_tokens_that_do_not_check_are_refused() {
    let relay = Relay::new().await;
    let m = &relay.mock;
    let check = |change, sign| check(&relay, change, sign);
    let es = |m: &Mock, c: Value| m.sign(c);
    for (what, change) in [
        (
            "aud",
            (|c: &mut Value| c["aud"] = json!("someone-else")) as fn(&mut Value),
        ),
        ("iss", |c| c["iss"] = json!("https://evil.example")),
        ("exp", |c| {
            c["exp"] = json!(c["iat"].as_u64().unwrap() - 3600)
        }),
        ("iat", |c| {
            c["iat"] = json!(c["iat"].as_u64().unwrap() + 3600)
        }),
        ("nonce", |c| {
            c.as_object_mut().unwrap().remove("nonce");
        }),
        ("sub", |c| c["sub"] = json!("")),
    ] {
        let refused = check(change, es).await.unwrap_err();
        assert!(
            matches!(refused, SyncError::SignInRefused),
            "{what}: {refused:?}"
        );
    }
    let forged = check(|_| {}, |m, c| m.sign_forged(c)).await.unwrap_err();
    assert!(matches!(forged, SyncError::SignInRefused), "{forged:?}");
    // Algorithms the key does not name: none, HMAC keyed with the published
    // key, RS256 named over the EC key.
    for (what, sign) in [
        (
            "none",
            (|m: &Mock, c: Value| {
                m.sign_with(json!({ "alg": "none", "kid": m.kid() }), &c, |_| vec![])
            }) as fn(&Mock, Value) -> String,
        ),
        ("HS256", |m, c| {
            m.sign_with(json!({ "alg": "HS256", "kid": m.kid() }), &c, |input| {
                use hmac::{Hmac, KeyInit, Mac};
                let key = m.public_jwk().to_string();
                let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key.as_bytes()).unwrap();
                mac.update(input);
                mac.finalize().into_bytes().to_vec()
            })
        }),
        ("RS256 over EC", |m, c| {
            m.sign_with(json!({ "alg": "RS256", "kid": m.kid() }), &c, |input| {
                m.ec_signature(input)
            })
        }),
        ("ES256 as in the key", |m, c| {
            m.sign_with(json!({ "alg": "ES256", "kid": m.kid() }), &c, |input| {
                m.ec_signature(input)
            })
        }),
    ] {
        let answer = check(|_| {}, sign).await;
        if what == "ES256 as in the key" {
            assert!(answer.is_ok(), "{what}: the control case checks");
        } else {
            let refused = answer.unwrap_err();
            assert!(
                matches!(refused, SyncError::SignInRefused),
                "{what}: {refused:?}"
            );
        }
    }
    relay.later(61 * 1000);
    // RS256 checks too, with the RSA key the JWKS publishes.
    check(|_| {}, |m, c| m.sign_rsa(c)).await.unwrap();
    // A token in an audience list is one of ours.
    check(|c| c["aud"] = json!(["other", "marquee"]), es)
        .await
        .unwrap();
    let fetched = m.jwks_fetches();
    m.rotate("ec-2");
    check(|_| {}, es).await.unwrap();
    assert_eq!(
        m.jwks_fetches(),
        fetched + 1,
        "fetched once for the new key"
    );
    check(|_| {}, es).await.unwrap();
    assert_eq!(m.jwks_fetches(), fetched + 1, "then cached");
    // Rotated again within the minute: an unknown key fetches nothing yet.
    m.rotate("ec-3");
    check(|_| {}, es).await.unwrap_err();
    assert_eq!(m.jwks_fetches(), fetched + 1, "no fetch within the minute");
    relay.later(61 * 1000);
    check(|_| {}, es).await.unwrap();
    assert_eq!(m.jwks_fetches(), fetched + 2);
}

/// Google signs with either spelling of its issuer: one identity, one row.
#[tokio::test]
async fn both_google_issuer_spellings_are_one_identity() {
    let relay = Relay::new().await;
    let first = relay
        .native("google", "google-android", "g-1")
        .await
        .unwrap();
    let phone = Device::create(&relay, &first, false).await.unwrap();
    let flow = start_native(&http(), &relay.url, "google", false)
        .await
        .unwrap();
    let mut claims = relay.mock.claims("google-web", "g-1", &flow.nonce);
    claims["iss"] = json!(relay.mock.url.split_once("://").unwrap().1);
    let again = flow
        .finish(&http(), &relay.mock.sign(claims), None)
        .await
        .unwrap();
    assert_eq!(again.account, Some(phone.record.keys.account));
    assert_eq!(relay.count("identity"), 1);
}

/// A ticket is used once, and lives 10 minutes.
#[tokio::test]
async fn a_ticket_is_used_once_and_expires() {
    let relay = Relay::new().await;
    let taken = relay.sign_in("mock", "alice").await;
    Device::create(&relay, &taken, false).await.unwrap();
    let again = Device::create(&relay, &taken, false).await.err().unwrap();
    assert!(matches!(again, SyncError::TicketExpired), "{again:?}");
    let late = relay.sign_in("mock", "bob").await;
    relay.later(11 * 60 * 1000);
    let expired = Device::create(&relay, &late, false).await.err().unwrap();
    assert!(matches!(expired, SyncError::TicketExpired), "{expired:?}");
    // Without a ticket an identity relay takes no sign-up.
    let none = mq_sync::create_account(&http(), &relay.url, None, "Phone")
        .await
        .unwrap_err();
    assert!(
        matches!(none, SyncError::Http(HttpError::Status { status: 403, .. })),
        "{none:?}"
    );
}

/// The answer to a create was lost: the retry finds the account made and
/// signs for its token; the ticket made one link and one sign-up.
#[tokio::test]
async fn a_create_retried_after_a_lost_answer_signs_for_its_token() {
    use axum::response::IntoResponse;
    let relay = Relay::new().await;
    let taken = relay.sign_in("mock", "alice").await;
    // A proxy that loses the first answer to /v1/accounts, after the relay acted.
    let target = relay.url.clone();
    let lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
        let (target, lost) = (target.clone(), lost.clone());
        async move {
            let (parts, body) = request.into_parts();
            let body = axum::body::to_bytes(body, usize::MAX).await.unwrap();
            let json = (!body.is_empty())
                .then(|| mq_http::Body::Json(serde_json::from_slice(&body).unwrap()));
            let verb = mq_http::Method::from_bytes(parts.method.as_str().as_bytes()).unwrap();
            let answer = http()
                .request(verb, &format!("{target}{}", parts.uri), json, &raw(vec![]))
                .await;
            if parts.uri.path() == "/v1/accounts" && !lost.swap(true, Ordering::Relaxed) {
                return (axum::http::StatusCode::BAD_GATEWAY, "lost").into_response();
            }
            let (status, body) = match answer {
                Ok(r) => (r.status, r.body),
                Err(HttpError::Status { status, body, .. }) => (status, body),
                Err(e) => (502, e.to_string().into()),
            };
            (axum::http::StatusCode::from_u16(status).unwrap(), body).into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let lossy = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let record = create_account_with(&http(), &lossy, None, "Phone", Some(&taken.ticket), false)
        .await
        .unwrap();
    assert_eq!(
        (
            relay.count("account"),
            relay.count("identity"),
            relay.count("signups")
        ),
        (1, 1, 1)
    );
    let pull = http()
        .get(
            &format!("{}/v1/pull", relay.url),
            &bearer(&record.device_token),
        )
        .await;
    assert_eq!(status(pull), 200);
}

/// One identity, at most 3 sign-ups in 30 days (each Start over is one);
/// after the window, it may again.
#[tokio::test]
async fn a_fourth_sign_up_in_30_days_is_refused_then_allowed() {
    let relay = Relay::new().await;
    Device::create(&relay, &relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    for _ in 0..2 {
        Device::create(&relay, &relay.sign_in("mock", "alice").await, true)
            .await
            .unwrap();
    }
    let fourth = Device::create(&relay, &relay.sign_in("mock", "alice").await, true)
        .await
        .err()
        .unwrap();
    assert!(matches!(fourth, SyncError::SignupLimit), "{fourth:?}");
    assert_eq!(fourth.code(), Some("signupLimit"));
    relay.later(31 * DAY_MS);
    Device::create(&relay, &relay.sign_in("mock", "alice").await, true)
        .await
        .unwrap();
}

/// Linked elsewhere: refused with the ticket kept, then taken over with
/// `move`; the other account takes it back the same way. Start over moves
/// it at sign-up and schedules the old account's deletion.
#[tokio::test]
async fn an_identity_linked_elsewhere_moves_only_when_asked() {
    let relay = Relay::new().await;
    let a = Device::create(&relay, &relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    let b = Device::create(&relay, &relay.sign_in("mock", "bob").await, false)
        .await
        .unwrap();
    let alice = relay.sign_in("mock", "alice").await;
    let refused = b.link(&alice, false).await.unwrap_err();
    assert!(matches!(refused, SyncError::LinkedElsewhere), "{refused:?}");
    b.link(&alice, true).await.unwrap();
    assert_eq!(b.links().await.len(), 2);
    assert!(a.links().await.is_empty());
    a.link(&relay.sign_in("mock", "alice").await, true)
        .await
        .unwrap();
    assert_eq!(a.links().await.len(), 1);
    // Creating with an identity linked elsewhere: refused, unless Start over.
    let taken = relay.sign_in("mock", "alice").await;
    let elsewhere = Device::create(&relay, &taken, false).await.err().unwrap();
    assert!(
        matches!(elsewhere, SyncError::LinkedElsewhere),
        "{elsewhere:?}"
    );
    let fresh = Device::create(&relay, &taken, true).await.unwrap();
    assert_eq!(fresh.links().await.len(), 1);
    let due: i64 = relay
        .sql()
        .query_row(
            "SELECT due_ms FROM deletion WHERE account = ?1",
            [hex::encode(a.record.keys.account.0)],
            |r| r.get(0),
        )
        .unwrap();
    assert!(due > 6 * DAY_MS, "the old account waits 7 days");
}

/// Unlink by link id, only the account's own. A receipt is optional; one
/// that is not this link's is refused. Apple's revoke flows (brokered with
/// the Services ID, native with the bundle id) each sign their client
/// secret for that client and revoke at Apple.
#[tokio::test]
async fn unlinking_and_apple_revocation_receipts() {
    let relay = Relay::new().await;
    let phone = Device::create(&relay, &relay.sign_in("apple", "apple-1").await, false)
        .await
        .unwrap();
    let other = Device::create(&relay, &relay.sign_in("mock", "bob").await, false)
        .await
        .unwrap();
    phone
        .link(&relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    let links = phone.links().await;
    let apple = links.iter().find(|l| l.provider == "apple").unwrap().link;
    let mock = links.iter().find(|l| l.provider != "apple").unwrap().link;
    async fn unlink(device: &Device, link: i64, receipt: Option<&str>) -> Result<(), SyncError> {
        mq_sync::identity::unlink_identity(
            &device.library,
            &http(),
            &device.record,
            &device.clock,
            link,
            receipt,
        )
        .await
        .map(drop)
    }
    // Another account's link is not that account's to drop: nothing changes.
    unlink(&other, mock, None).await.unwrap();
    assert_eq!(phone.links().await.len(), 2);

    // Brokered revoke: the Services ID's secret, a receipt for the Apple link.
    relay.mock.next(Next::Pick("apple-1".into()));
    let flow = start_brokered(&http(), &relay.url, "apple", "marquee://auth", true)
        .await
        .unwrap();
    let revoked = flow
        .finish(&http(), &browse(&flow.url).await)
        .await
        .unwrap();
    let receipt = revoked.receipt.clone().expect("a receipt");
    assert_eq!(revoked.account, Some(phone.record.keys.account));
    assert_eq!(relay.mock.revoked().len(), 1);
    let secret_for = |client: &str| {
        relay
            .mock
            .secrets()
            .into_iter()
            .rev()
            .find(|(c, _)| c == client)
            .map(|(_, s)| s)
            .unwrap()
    };
    let claims = |secret: &str| -> Value {
        serde_json::from_slice(&B64URL.decode(secret.split('.').nth(1).unwrap()).unwrap()).unwrap()
    };
    let brokered = claims(&secret_for("apple-web"));
    assert_eq!(
        (brokered["sub"].as_str(), brokered["iss"].as_str()),
        (Some("apple-web"), Some("TEAM"))
    );
    let wrong = unlink(&phone, mock, Some(&receipt)).await.unwrap_err();
    assert!(matches!(wrong, SyncError::ReceiptRefused), "{wrong:?}");
    unlink(&phone, apple, Some(&receipt)).await.unwrap();
    unlink(&phone, mock, None).await.unwrap();
    assert!(phone.links().await.is_empty());
    // Unlinked already (a retry after a lost answer): done.
    unlink(&phone, mock, None).await.unwrap();

    // Native revoke: the token's bundle id is the code's client.
    let flow = start_native(&http(), &relay.url, "apple", true)
        .await
        .unwrap();
    let token = relay
        .mock
        .sign(relay.mock.claims("app.tv", "apple-1", &flow.nonce));
    let code = relay.mock.native_code("apple-1", "app.tv");
    let native = flow.finish(&http(), &token, Some(&code)).await.unwrap();
    assert!(native.receipt.is_some());
    assert_eq!(claims(&secret_for("app.tv"))["sub"], "app.tv");
    assert_eq!(relay.mock.revoked().len(), 2);
    // A token for one identity with a code for another: no receipt.
    let flow = start_native(&http(), &relay.url, "apple", true)
        .await
        .unwrap();
    let token = relay
        .mock
        .sign(relay.mock.claims("app.tv", "apple-1", &flow.nonce));
    let code = relay.mock.native_code("apple-2", "app.tv");
    let mixed = flow.finish(&http(), &token, Some(&code)).await.unwrap_err();
    assert!(matches!(mixed, SyncError::SignInRefused), "{mixed:?}");
    assert_eq!(relay.mock.revoked().len(), 2);

    // An Apple link unlinked without a receipt (the revoke could not run).
    phone
        .link(&relay.sign_in("apple", "apple-1").await, false)
        .await
        .unwrap();
    let apple = phone.links().await[0].link;
    unlink(&phone, apple, None).await.unwrap();
    assert!(phone.links().await.is_empty());
    let _ = &relay.apple_key;
}

/// Facebook: the code exchanged with the app secret, the user from
/// `debug_token` under the app token; another app's token is refused.
#[tokio::test]
async fn facebook_signs_in_by_code_exchange_and_debug_token() {
    let relay = Relay::new().await;
    let taken = relay.sign_in("facebook", "fb-user").await;
    let phone = Device::create(&relay, &taken, false).await.unwrap();
    assert_eq!(phone.links().await[0].provider, "facebook");
    assert!(
        relay
            .mock
            .secrets()
            .contains(&("fb-app".into(), "fs".into()))
    );
    relay.mock.answer_for_another_app();
    relay.mock.next(Next::Pick("fb-user".into()));
    let flow = start_brokered(&http(), &relay.url, "facebook", "marquee://auth", false)
        .await
        .unwrap();
    let refused = flow
        .finish(&http(), &browse(&flow.url).await)
        .await
        .unwrap_err();
    assert!(matches!(refused, SyncError::SignInRefused), "{refused:?}");
}

/// On a relay with sign-in and no invite, a replayed genesis creates
/// nothing (sign-up takes a ticket); with the invite it does, and in the
/// hour after a restore at most 20 per address do.
#[tokio::test]
async fn a_fresh_genesis_replayed_on_a_sign_in_relay_is_refused() {
    let replay = |url: String, invite: Option<&'static str>| async move {
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).unwrap();
        let (entry, keys) = Roster::genesis_entry(&MemberSecret::generate(), Hex(id), "x", 1);
        let headers = invite
            .map(|i| vec![("x-invite".to_owned(), i.to_owned())])
            .unwrap_or_default();
        let request = ReplayRequest {
            account: keys.account,
            entries: vec![entry],
        };
        status(
            http()
                .post_json(&format!("{url}/v1/roster/replay"), &request, &raw(headers))
                .await,
        )
    };
    let relay = Relay::new().await;
    assert_eq!(replay(relay.url.clone(), None).await, 403);
    let invited = Relay::with(|c| c.invite = Some("letmein".into())).await;
    assert_eq!(replay(invited.url.clone(), Some("nope")).await, 403);
    assert_eq!(replay(invited.url.clone(), Some("letmein")).await, 200);
    // Restored: devices give back what the backup lost, for an hour.
    let restored = Relay::on(
        Mock::start(0).await,
        mock_oidc::apple_key_pem(),
        relay.db.clone(),
        Arc::default(),
        |c| c.new_generation = true,
    )
    .await;
    for _ in 0..20 {
        assert_eq!(replay(restored.url.clone(), None).await, 200);
    }
    // The 21st from one address in the hour (past the minute's rate budget).
    restored.later(61 * 1000);
    assert_eq!(
        replay(restored.url.clone(), None).await,
        403,
        "20 per address"
    );
    restored.later(60 * 60 * 1000);
    assert_eq!(replay(restored.url.clone(), None).await, 403);
    std::mem::forget(restored);
}

/// From the deletion page: a ticket schedules the deletion 7 days out, a
/// second request keeps the date, every device's sync reports it with the
/// provider that asked, a cancel with `unlink` drops the link that asked;
/// scheduled again, it runs when due.
#[tokio::test]
async fn a_scheduled_deletion_shows_can_be_cancelled_and_runs_when_due() {
    let relay = Relay::with(|c| c.housekeeping_every = std::time::Duration::from_millis(50)).await;
    let mut phone = Device::create(&relay, &relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    let mut tv = phone.pair().await;
    assert_eq!(tv.sync().await.deletion, None);
    let due = ask_deletion(&relay, relay.sign_in("mock", "alice").await).await;
    relay.later(DAY_MS);
    assert_eq!(
        ask_deletion(&relay, relay.sign_in("mock", "alice").await).await,
        due,
        "the first date stays"
    );
    // The device that did not ask hears it at its next sync, and who asked.
    let shown = Some(ScheduledDeletion {
        due_ms: due,
        provider: Some(format!("oidc:{}", relay.mock.url)),
    });
    assert_eq!(tv.sync().await.deletion, shown);
    assert_eq!(phone.sync().await.deletion, shown);
    tv.cancel_deletion(true).await;
    assert_eq!((relay.count("deletion"), relay.count("identity")), (0, 0));
    assert_eq!(phone.sync().await.deletion, None);
    phone
        .link(&relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    ask_deletion(&relay, relay.sign_in("mock", "alice").await).await;
    relay.later(8 * DAY_MS);
    for _ in 0..100 {
        if relay.count("gone") == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        (
            relay.count("gone"),
            relay.count("account"),
            relay.count("deletion")
        ),
        (1, 0, 0)
    );
}

/// The journal keeps deletions: a database restored from before the request
/// gets the deletion back, and one restored from before a cancel loses it.
#[tokio::test]
async fn a_restored_database_gets_its_deletion_lines_back() {
    let relay = Relay::new().await;
    let phone = Device::create(&relay, &relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    let journal = relay.db.with_extension("roster.jsonl");
    // A backup of the database file alone, taken now.
    let backup = |name: &str| {
        let path = relay.db.with_file_name(name);
        relay
            .sql()
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        path
    };
    // That backup restored in a directory of its own, the journal beside it.
    let restore = |backup: PathBuf| {
        let db = fresh_dir().join("relay.sqlite");
        std::fs::rename(&backup, &db).unwrap();
        std::fs::copy(&journal, db.with_extension("roster.jsonl")).unwrap();
        let (mock, key) = (relay.mock.clone(), relay.apple_key.clone());
        async move { Relay::on(mock, key, db, Arc::default(), |c| c.new_generation = true).await }
    };
    let before_request = backup("before-request.sqlite");
    let due = ask_deletion(&relay, relay.sign_in("mock", "alice").await).await;
    let restored = restore(before_request).await;
    let back: i64 = restored
        .sql()
        .query_row("SELECT due_ms FROM deletion", [], |r| r.get(0))
        .unwrap();
    assert_eq!(back as u64, due, "the request came back from the journal");
    drop(restored);
    let before_cancel = backup("before-cancel.sqlite");
    phone.cancel_deletion(false).await;
    let restored = restore(before_cancel).await;
    assert_eq!(restored.count("deletion"), 0, "the cancel line removed it");
}

/// A link a deletion row names is only ever this account's to drop: a
/// Start over moves the link and clears it from the old account's deletion,
/// and "Cancel and unlink" never reaches another account's link (a
/// restored journal line can name a link id given to someone else since).
#[tokio::test]
async fn cancel_and_unlink_drops_only_this_accounts_link() {
    let relay = Relay::new().await;
    let mut old = Device::create(&relay, &relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    ask_deletion(&relay, relay.sign_in("mock", "alice").await).await;
    let fresh = Device::create(&relay, &relay.sign_in("mock", "alice").await, true)
        .await
        .unwrap();
    let account = |d: &Device| hex::encode(d.record.keys.account.0);
    let named: Option<i64> = relay
        .sql()
        .query_row(
            "SELECT link FROM deletion WHERE account = ?1",
            [account(&old)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(named, None, "the moved link left the old deletion");
    // The old account's devices see the deletion; no provider asked for it now.
    let shown = old.sync().await.deletion.expect("shown on the old account");
    assert_eq!(shown.provider, None);
    // As a restored journal line could: the deletion names the new account's link.
    relay
        .sql()
        .execute(
            "UPDATE deletion SET link = (SELECT link FROM identity WHERE account = ?2)
             WHERE account = ?1",
            [account(&old), account(&fresh)],
        )
        .unwrap();
    old.cancel_deletion(true).await;
    assert_eq!(
        fresh.links().await.len(),
        1,
        "the new account keeps its link"
    );
    assert_eq!(relay.count("deletion"), 0);
}

/// Apple's notification route: a budget of its own, Apple's keys fetched
/// at most once a minute however many unknown keys are named, and a notice
/// older than a day refused.
#[tokio::test]
async fn apple_notifications_are_bounded() {
    let relay = Relay::new().await;
    Device::create(&relay, &relay.sign_in("apple", "apple-1").await, false)
        .await
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let m = &relay.mock;
    let notice = |iat: u64| {
        json!({
            "iss": m.url, "aud": "apple-web", "iat": iat,
            "events": json!({ "type": "consent-revoked", "sub": "apple-1", "event_time": iat }).to_string(),
        })
    };
    let post = |payload: String| {
        let url = format!("{}/v1/auth/apple/notifications", relay.url);
        async move {
            status(
                http()
                    .post_json(&url, &json!({ "payload": payload }), &raw(vec![]))
                    .await,
            )
        }
    };
    let unknown = || {
        let kid = hex::encode(rand_bytes());
        m.sign_with(json!({ "alg": "ES256", "kid": kid }), &notice(now), |i| {
            m.ec_signature(i)
        })
    };
    let fetched = m.jwks_fetches();
    for _ in 0..9 {
        assert_eq!(post(unknown()).await, 400);
    }
    assert_eq!(
        m.jwks_fetches(),
        fetched,
        "keys fetched at sign-in serve the minute"
    );
    relay.later(61 * 1000);
    for _ in 0..9 {
        assert_eq!(post(unknown()).await, 400);
    }
    assert_eq!(m.jwks_fetches(), fetched + 1, "one fetch for the minute");
    // Its own budget: 10 a minute from one address.
    assert_eq!(post(unknown()).await, 400);
    assert_eq!(post(unknown()).await, 429);
    relay.later(61 * 1000);
    let old = m.sign(notice(now - 2 * 24 * 60 * 60));
    assert_eq!(post(old).await, 400, "older than a day");
    assert_eq!(relay.count("identity"), 1);
    assert_eq!(post(m.sign(notice(now))).await, 200);
    assert_eq!(relay.count("identity"), 0);
}

fn rand_bytes() -> [u8; 8] {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).unwrap();
    b
}

/// CORS: the link host's origin exactly, on `start`, `take` and
/// `delete-request` only.
#[tokio::test]
async fn cors_answers_the_link_host_on_three_routes_only() {
    let relay = Relay::new().await;
    let browser = reqwest::Client::new();
    let origin = |route: &str, origin: &str| {
        let request = browser
            .request(reqwest::Method::OPTIONS, format!("{}{route}", relay.url))
            .header("origin", origin)
            .header("access-control-request-method", "POST");
        async move {
            let answer = request.send().await.unwrap();
            answer
                .headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap().to_owned())
        }
    };
    for route in [
        "/v1/auth/mock/start",
        "/v1/auth/take",
        "/v1/account/delete-request",
    ] {
        assert_eq!(
            origin(route, "https://link.example").await.as_deref(),
            Some("https://link.example"),
            "{route}"
        );
        assert_eq!(origin(route, "https://evil.example").await, None, "{route}");
    }
    let plain = browser
        .get(format!("{}/v1/relay", relay.url))
        .header("origin", "https://link.example")
        .send()
        .await
        .unwrap();
    assert!(plain.headers().get("access-control-allow-origin").is_none());
}

/// The database names its identity key: another key does not start.
#[tokio::test]
async fn another_identity_key_refuses_to_start() {
    let relay = Relay::new().await;
    let other = identity(&relay.mock, &relay.url, &"2".repeat(64), &relay.apple_key);
    let config = Config {
        public_host: Some("127.0.0.1".into()),
        identity: Some(other),
        ..Config::default()
    };
    let refused = router(&relay.db, config).err().unwrap();
    assert!(refused.contains("identity key"), "{refused}");
}

/// Start-up refuses a public host devices cannot match (capitals, a port,
/// a scheme, a path), a `publicUrl` on another host, and a provider address
/// that is not https off this machine's network.
#[tokio::test]
async fn start_up_checks_the_public_host_and_provider_addresses() {
    let dir = fresh_dir();
    let db = dir.join("relay.sqlite");
    for host in [
        "Relay.example",
        "relay.example:8443",
        "https://relay.example",
        "relay.example/x",
    ] {
        let config = Config {
            public_host: Some(host.into()),
            ..Config::default()
        };
        let refused = router(&db, config).err().unwrap();
        assert!(refused.contains("--public-host"), "{host}: {refused}");
    }
    let mock = Mock::start(0).await;
    let key = mock_oidc::apple_key_pem();
    let start = |change: fn(&mut Value)| {
        let mut file = identity_json(&mock, "http://127.0.0.1:1", KEY, &key);
        change(&mut file);
        let config = Config {
            public_host: Some("127.0.0.1".into()),
            identity: Some(serde_json::from_value(file).unwrap()),
            ..Config::default()
        };
        router(&db, config).err()
    };
    assert_eq!(start(|_| {}), None, "the mock on loopback is fine");
    let elsewhere = start(|f| f["publicUrl"] = json!("https://other.example")).unwrap();
    assert!(elsewhere.contains("publicUrl"), "{elsewhere}");
    let plain = start(|f| f["google"]["issuer"] = json!("http://accounts.example.com")).unwrap();
    assert!(plain.contains("https"), "{plain}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Without an identity file: no sign-in routes, no providers.
#[tokio::test]
async fn no_identity_file_no_sign_in() {
    let db = fresh_dir().join("relay.sqlite");
    let app = router(&db, Config::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let info: Value = http()
        .get(&format!("{url}/v1/relay"), &raw(vec![]))
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(info["providers"], json!([]));
    let start = json!({ "verifierHash": "00".repeat(32), "purpose": "signin", "native": true });
    assert_eq!(
        status(
            http()
                .post_json(&format!("{url}/v1/auth/mock/start"), &start, &raw(vec![]))
                .await
        ),
        404
    );
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
    let with: Value = http()
        .get(
            &format!("{}/v1/relay", Relay::new().await.url),
            &raw(vec![]),
        )
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        with["providers"],
        json!(["apple", "google", "facebook", "mock"])
    );
}

/// Apple's signed `consent-revoked` drops that identity's link, twice the same.
#[tokio::test]
async fn apple_consent_revoked_drops_the_link() {
    let relay = Relay::new().await;
    Device::create(&relay, &relay.sign_in("apple", "apple-1").await, false)
        .await
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let note = relay.mock.sign(json!({
        "iss": relay.mock.url, "aud": "apple-web", "iat": now,
        "events": json!({ "type": "consent-revoked", "sub": "apple-1", "event_time": now }).to_string(),
    }));
    for _ in 0..2 {
        let answer = http()
            .post_json(
                &format!("{}/v1/auth/apple/notifications", relay.url),
                &json!({ "payload": note }),
                &raw(vec![]),
            )
            .await;
        assert_eq!(status(answer), 200);
        assert_eq!(relay.count("identity"), 0);
    }
    let forged = relay
        .mock
        .sign_forged(json!({ "iss": relay.mock.url, "aud": "apple-web", "events": "{}" }));
    let answer = http()
        .post_json(
            &format!("{}/v1/auth/apple/notifications", relay.url),
            &json!({ "payload": forged }),
            &raw(vec![]),
        )
        .await;
    assert_eq!(status(answer), 400);
}

/// A brokered return goes only where the allowlist says; at most 20 flows
/// are open per client address.
#[tokio::test]
async fn returns_are_allowlisted_and_open_flows_capped() {
    let relay = Relay::new().await;
    for to in [
        "https://evil.example/auth",
        "marquee://auth?x=1",
        "http://192.168.1.2:80/auth",
        "https://link.example/other",
    ] {
        let refused = start_brokered(&http(), &relay.url, "mock", to, false)
            .await
            .unwrap_err();
        assert!(
            matches!(
                refused,
                SyncError::Http(HttpError::Status { status: 400, .. })
            ),
            "{to}: {refused:?}"
        );
    }
    for to in ["http://127.0.0.1:5173/auth", "https://link.example/delete"] {
        start_brokered(&http(), &relay.url, "mock", to, false)
            .await
            .unwrap();
    }
    for _ in 2..20 {
        start_native(&http(), &relay.url, "mock", false)
            .await
            .unwrap();
    }
    let capped = start_native(&http(), &relay.url, "mock", false)
        .await
        .unwrap_err();
    assert!(matches!(capped, SyncError::Busy), "{capped:?}");
    assert_eq!(capped.code(), Some("busy"));
}

/// Deleting needs the device key's signature, not the token alone.
#[tokio::test]
async fn a_token_alone_deletes_nothing() {
    use mq_sync::roster::TokenRequest;
    let relay = Relay::new().await;
    let phone = Device::create(&relay, &relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    let info: mq_sync::wire::RelayInfo = http()
        .get(&format!("{}/v1/relay", relay.url), &raw(vec![]))
        .await
        .unwrap()
        .json()
        .unwrap();
    let send = |request: &TokenRequest, sig: Hex<64>| {
        let body = json!({ "request": request, "sig": sig });
        let url = format!("{}/v1/account", relay.url);
        let options = bearer(&phone.record.device_token);
        async move {
            status(
                http()
                    .request(
                        mq_http::Method::DELETE,
                        &url,
                        Some(mq_http::Body::Json(body)),
                        &options,
                    )
                    .await,
            )
        }
    };
    let me = phone.record.secret();
    let request = |at: u64| {
        TokenRequest::new(
            info.relay_id,
            phone.record.keys.account,
            me.id(),
            at,
            "127.0.0.1",
        )
    };
    // Signed by a key that is not this device's.
    let asked = request(info.server_ms);
    let stranger = MemberSecret::generate();
    assert_eq!(send(&asked, stranger.sign_delete(&asked)).await, 403);
    // Too far from the relay's time.
    let stale = request(info.server_ms - 60 * 60 * 1000);
    assert_eq!(send(&stale, me.sign_delete(&stale)).await, 403);
    // A body whose nonce the relay already took (a replay): refused.
    let replayed = request(info.server_ms);
    relay
        .sql()
        .execute(
            "INSERT INTO token_nonce (nonce, expires_ms) VALUES (?1, ?2)",
            rusqlite::params![replayed.nonce.0.to_vec(), i64::MAX],
        )
        .unwrap();
    assert_eq!(send(&replayed, me.sign_delete(&replayed)).await, 403);
    // This very request, signed by this device as a token request (devices
    // make those every day): another purpose, refused.
    assert_eq!(send(&asked, me.sign_token(&asked)).await, 403);
    assert_eq!(relay.count("account"), 1);
    // The same request signed for deleting: deleted.
    assert_eq!(send(&asked, me.sign_delete(&asked)).await, 200);
    assert_eq!((relay.count("account"), relay.count("identity")), (0, 0));
}
