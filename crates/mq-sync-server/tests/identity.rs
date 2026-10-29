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
use mock_oidc::{Mock, Next, browse, browse_to};
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
        "facebook": { "appId": "fb-app", "graph": mock.url, "dialog": format!("{}/authorize", mock.url), "issuer": mock.url },
        "oidc": [{ "id": "mock", "issuer": mock.url, "clientId": "marquee", "clientSecret": "ms" }],
        "email": {
            "accessKeyId": mock_oidc::MAIL_KEY_ID, "secretAccessKey": mock_oidc::MAIL_SECRET,
            "region": "us-east-1", "from": "noreply@example.org", "endpoint": mock.url,
        },
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
                    email: None,
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
        let flow = start_brokered(&http(), &self.url, provider, "momotus://auth", false)
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
            Some("phone"),
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

    /// A key rotation with nobody removed: the roster moves on.
    async fn remove(&mut self) {
        self.record =
            mq_sync::remove_devices(&self.library, &http(), &self.record, &self.clock, vec![])
                .await
                .unwrap();
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
    assert_eq!(links[0].provider, "mock", "the route id, never the issuer");
    // A build from before the Momotus ids starts with marquee://auth and is sent back there.
    relay.mock.next(Next::Pick("alice".into()));
    let old = start_brokered(&http(), &relay.url, "mock", "marquee://auth", false)
        .await
        .unwrap();
    let fragment = browse_to(&old.url, "marquee://auth#").await;
    let taken = old.finish(&http(), &fragment).await.unwrap();
    assert_eq!(taken.account, Some(phone.record.keys.account), "old scheme");
    // Nothing of the provider is kept: no subject in the clear.
    let subject: Vec<u8> = relay
        .sql()
        .query_row("SELECT subject FROM identity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(subject.len(), 32);
    assert!(!subject.windows(5).any(|w| w == b"alice"));
    // A cancelled provider page comes back as cancelled.
    relay.mock.next(Next::Cancel);
    let flow = start_brokered(&http(), &relay.url, "mock", "momotus://auth", false)
        .await
        .unwrap();
    let err = flow
        .finish(&http(), &browse(&flow.url).await)
        .await
        .unwrap_err();
    assert!(matches!(err, SyncError::Cancelled), "{err:?}");
    // The provider's ID token names another nonce than the flow's.
    relay.mock.next(Next::WrongNonce("alice".into()));
    let flow = start_brokered(&http(), &relay.url, "mock", "momotus://auth", false)
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
    let flow = start_brokered(&http(), &relay.url, "mock", "momotus://auth", false)
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
    let late = start_brokered(&http(), &relay.url, "mock", "momotus://auth", false)
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
    let flow = start_brokered(&http(), &relay.url, "mock", "momotus://auth", false)
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
    let record = create_account_with(
        &http(),
        &lossy,
        None,
        "Phone",
        Some(&taken.ticket),
        false,
        None,
    )
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
    let flow = start_brokered(&http(), &relay.url, "apple", "momotus://auth", true)
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

/// Facebook with no app secret: OIDC with PKCE. The page asks for `openid`
/// with a nonce and an S256 challenge; the exchange sends the verifier and no
/// secret; the ID token's issuer, audience, nonce and signature are checked,
/// and `/me` must name the same user. Another app's token, a wrong nonce and
/// another user at `/me` are refused.
#[tokio::test]
async fn facebook_signs_in_with_pkce_and_no_app_secret() {
    let relay = Relay::new().await;
    relay.mock.next(Next::Pick("fb-user".into()));
    let flow = start_brokered(&http(), &relay.url, "facebook", "momotus://auth", false)
        .await
        .unwrap();
    let page = url::Url::parse(&flow.url).unwrap();
    let q: std::collections::HashMap<String, String> = page.query_pairs().into_owned().collect();
    assert_eq!(q["scope"], "openid public_profile");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["code_challenge"].len(), 43);
    assert!(!q["nonce"].is_empty());
    assert!(!flow.url.contains("secret"));
    let taken = flow
        .finish(&http(), &browse(&flow.url).await)
        .await
        .unwrap();
    let phone = Device::create(&relay, &taken, false).await.unwrap();
    assert_eq!(phone.links().await[0].provider, "facebook");
    assert_eq!(
        relay.mock.secrets(),
        vec![("fb-app".to_owned(), String::new())]
    );
    let verifier = relay.mock.verifiers()[0].clone();
    assert!(verifier.len() >= 43, "{verifier}");
    assert!(
        !flow.url.contains(&verifier),
        "the verifier never leaves the relay before the exchange"
    );

    // Each refusal on a relay and mock of its own, so one cause at a time.
    async fn refused(next: Next, tweak: impl FnOnce(&Mock)) -> SyncError {
        let relay = Relay::new().await;
        tweak(&relay.mock);
        relay.mock.next(next);
        let flow = start_brokered(&http(), &relay.url, "facebook", "momotus://auth", false)
            .await
            .unwrap();
        flow.finish(&http(), &browse(&flow.url).await)
            .await
            .unwrap_err()
    }
    let wrong_nonce = refused(Next::WrongNonce("fb-user".into()), |_| {}).await;
    assert!(
        matches!(wrong_nonce, SyncError::SignInRefused),
        "{wrong_nonce:?}"
    );
    let other_app = refused(Next::Pick("fb-user".into()), Mock::answer_for_another_app).await;
    assert!(
        matches!(other_app, SyncError::SignInRefused),
        "{other_app:?}"
    );
    let other_user = refused(Next::Pick("fb-user".into()), Mock::answer_for_another_user).await;
    assert!(
        matches!(other_user, SyncError::SignInRefused),
        "{other_user:?}"
    );
}

/// The PKCE verifier comes from the flow's raw nonce, which a Facebook flow
/// never hands out: a native start is refused and a brokered start answers
/// no nonce. An identity file that still names an app secret sends it along
/// with the verifier.
#[tokio::test]
async fn facebook_keeps_its_nonce_and_takes_an_old_secret() {
    let relay = Relay::new().await;
    let browser = reqwest::Client::new();
    let start = |native: bool| {
        let body = if native {
            json!({ "verifierHash": "ab".repeat(32), "purpose": "signin", "native": true })
        } else {
            json!({ "verifierHash": "ab".repeat(32), "purpose": "signin", "return": "momotus://auth" })
        };
        browser
            .post(format!("{}/v1/auth/facebook/start", relay.url))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
    };
    assert_eq!(start(true).await.unwrap().status(), 400);
    let brokered: Value =
        serde_json::from_str(&start(false).await.unwrap().text().await.unwrap()).unwrap();
    assert!(brokered["url"].is_string());
    assert!(
        brokered.get("nonce").is_none_or(Value::is_null),
        "{brokered}"
    );

    let old = Relay::with(|c| {
        let file = c.identity.as_mut().unwrap();
        file.facebook.as_mut().unwrap().app_secret = "fs".into();
    })
    .await;
    let taken = old.sign_in("facebook", "fb-user").await;
    Device::create(&old, &taken, false).await.unwrap();
    assert_eq!(
        old.mock.secrets(),
        vec![("fb-app".to_owned(), "fs".to_owned())]
    );
    assert!(!old.mock.verifiers()[0].is_empty());
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
        provider: Some("mock".into()),
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

/// Forged notices from many addresses, each under its own budget, do not
/// spend the relay-wide one: only notices whose signature checked count,
/// so Apple's own still lands.
#[tokio::test]
async fn forged_notices_from_many_addresses_leave_room_for_apple() {
    let relay = Relay::new().await;
    Device::create(&relay, &relay.sign_in("apple", "apple-1").await, false)
        .await
        .unwrap();
    // The notices come through a trusted proxy (the same database behind
    // `--trust-proxy`), which names each sender's address; the sign-in above
    // came straight, as a proxy-trusting relay refuses a request naming none.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxied = format!("http://{}", listener.local_addr().unwrap());
    let config = Config {
        clock_offset_ms: relay.offset.clone(),
        public_host: Some("127.0.0.1".into()),
        identity: Some(identity(&relay.mock, &proxied, KEY, &relay.apple_key)),
        trust_proxy: true,
        ..Config::default()
    };
    let app = router(&relay.db, config).unwrap();
    tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let notice = json!({
        "iss": relay.mock.url, "aud": "apple-web", "iat": now,
        "events": json!({ "type": "consent-revoked", "sub": "apple-1", "event_time": now }).to_string(),
    });
    let post = |from: String, payload: String| {
        let url = format!("{proxied}/v1/auth/apple/notifications");
        async move {
            let headers = vec![("x-forwarded-for".to_owned(), from)];
            status(
                http()
                    .post_json(&url, &json!({ "payload": payload }), &raw(headers))
                    .await,
            )
        }
    };
    for i in 0..70 {
        let forged = relay.mock.sign_forged(notice.clone());
        assert_eq!(post(format!("203.0.113.{i}"), forged).await, 400);
    }
    let apple = relay.mock.sign(notice);
    assert_eq!(post("17.0.0.1".into(), apple).await, 200);
    assert_eq!(relay.count("identity"), 0);
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
    let graph = start(|f| f["facebook"]["graph"] = json!("http://graph.example.com")).unwrap();
    assert!(graph.contains("https"), "{graph}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A discovery document must name the issuer it was asked for: the mock
/// reached as `localhost` answers for `127.0.0.1`, and nothing starts.
#[tokio::test]
async fn discovery_naming_another_issuer_is_refused() {
    let mock = Mock::start(0).await;
    let key = mock_oidc::apple_key_pem();
    let mut file = identity_json(&mock, "http://127.0.0.1:1", KEY, &key);
    file["oidc"] = json!([{
        "id": "renamed", "issuer": mock.url.replace("127.0.0.1", "localhost"),
        "clientId": "marquee", "clientSecret": "ms",
    }]);
    let relay = Relay::on(
        mock,
        key,
        fresh_dir().join("relay.sqlite"),
        Arc::default(),
        |c| c.identity = Some(serde_json::from_value(file).unwrap()),
    )
    .await;
    let refused = start_brokered(&http(), &relay.url, "renamed", "momotus://auth", false)
        .await
        .unwrap_err();
    assert!(
        matches!(
            refused,
            SyncError::Http(HttpError::Status { status: 502, .. })
        ),
        "{refused:?}"
    );
}

/// A device stuck on a fork (the relay was restored and another device
/// rotated there) still cancels a deletion: the call needs its token, not
/// an agreed roster, as `deleteAccount` does.
#[tokio::test]
async fn a_device_stuck_on_a_fork_can_cancel_a_deletion() {
    let relay = Relay::new().await;
    let mut phone = Device::create(&relay, &relay.sign_in("mock", "alice").await, false)
        .await
        .unwrap();
    let mut tv = phone.pair().await;
    tv.sync().await;
    phone.sync().await;
    let snapshot = relay.db.with_file_name("before.sqlite");
    relay
        .sql()
        .execute("VACUUM INTO ?1", [snapshot.to_str().unwrap()])
        .unwrap();
    phone.remove().await;
    // Restored without its journal: the relay lost the phone's rotation.
    let db = fresh_dir().join("relay.sqlite");
    std::fs::rename(&snapshot, &db).unwrap();
    let restored = Relay::on(
        relay.mock.clone(),
        relay.apple_key.clone(),
        db,
        Arc::default(),
        |c| c.new_generation = true,
    )
    .await;
    for device in [&mut phone, &mut tv] {
        device.record.server = restored.url.clone();
    }
    tv.remove().await;
    let fork = sync_once(&phone.library, &http(), &phone.record, &phone.clock)
        .await
        .unwrap_err();
    assert!(matches!(fork, SyncError::Fork), "{fork:?}");
    ask_deletion(&restored, restored.sign_in("mock", "alice").await).await;
    assert_eq!(restored.count("deletion"), 1);
    phone.cancel_deletion(true).await;
    assert_eq!(
        (restored.count("deletion"), restored.count("identity")),
        (0, 0)
    );
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
        json!(["apple", "google", "facebook", "email", "mock"])
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
        "momotus://auth?x=1",
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
    // The scheme of builds made before the Momotus app ids still returns.
    for to in [
        "marquee://auth",
        "http://127.0.0.1:5173/auth",
        "https://link.example/delete",
    ] {
        start_brokered(&http(), &relay.url, "mock", to, false)
            .await
            .unwrap();
    }
    for _ in 3..20 {
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

/// A sign-in in progress keeps a keyed hash of its client's address, never
/// the address, so neither the database nor a backup taken meanwhile holds
/// one; two starts from one address still count together.
#[tokio::test]
async fn a_sign_in_in_progress_keeps_no_client_address() {
    let relay = Relay::new().await;
    for _ in 0..2 {
        start_brokered(&http(), &relay.url, "mock", "momotus://auth", false)
            .await
            .unwrap();
    }
    let clients: Vec<String> = relay
        .sql()
        .prepare("SELECT client FROM auth_flow")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(clients.len(), 2);
    assert!(
        clients
            .iter()
            .all(|c| c.len() == 64 && !c.contains("127.0.0.1")),
        "{clients:?}"
    );
    assert_eq!(clients[0], clients[1], "one address, one tag");
}

// ---- email (plans/accounts-email.md) ----

/// A coded refusal's status and code.
fn refusal(e: HttpError) -> (u16, String) {
    match e {
        HttpError::Status { status, body, .. } => {
            let code = serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| v.get("code").and_then(Value::as_str).map(str::to_owned));
            (status, code.unwrap_or_default())
        }
        e => panic!("{e}"),
    }
}

/// A raw email start at `url` (as `client`, through a trusting relay, when given).
async fn email_start_at(
    url: &str,
    address: &str,
    verifier: &str,
    client: Option<&str>,
) -> Result<StartResponse, (u16, String)> {
    let headers = client
        .map(|c| vec![("x-forwarded-for".to_owned(), c.to_owned())])
        .unwrap_or_default();
    http()
        .post_json(
            &format!("{url}/v1/auth/email/start"),
            &StartRequest {
                verifier_hash: hex::encode(sha2_hash(verifier)),
                purpose: "signin".into(),
                email: Some(address.into()),
                ..Default::default()
            },
            // Longer than the relay's 8 s wait on a stalled mail service.
            &RequestOptions {
                timeout: std::time::Duration::from_secs(20),
                ..raw(headers)
            },
        )
        .await
        .map_err(refusal)
        .map(|r| r.json().unwrap())
}

async fn email_take_at(
    url: &str,
    flow: &str,
    verifier: &str,
    code: &str,
) -> Result<Taken, (u16, String)> {
    http()
        .post_json(
            &format!("{url}/v1/auth/take"),
            &json!({ "flow": flow, "verifier": verifier, "code": code }),
            &raw(vec![]),
        )
        .await
        .map_err(refusal)
        .map(|r| r.json().unwrap())
}

impl Relay {
    async fn email_start(
        &self,
        address: &str,
        verifier: &str,
    ) -> Result<StartResponse, (u16, String)> {
        email_start_at(&self.url, address, verifier, None).await
    }

    async fn email_take(
        &self,
        flow: &str,
        verifier: &str,
        code: &str,
    ) -> Result<Taken, (u16, String)> {
        email_take_at(&self.url, flow, verifier, code).await
    }

    /// A whole email sign-in, the mail read at the mock; a minute later
    /// (relay time), so the next one is not held to the 60 s rule.
    async fn email(&self, address: &str) -> Taken {
        let flow = mq_sync::identity::start_email(&http(), &self.url, address)
            .await
            .unwrap();
        let to = mq_sync::wire::normalize_email(address).unwrap();
        let taken = flow.finish(&http(), &self.mock.code(&to)).await.unwrap();
        self.later(61_000);
        taken
    }

    fn events(&self, kind: &str) -> i64 {
        self.sql()
            .query_row(
                "SELECT COUNT(*) FROM email_event WHERE kind = ?1",
                [kind],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// A relay on the same database behind a trusted proxy: requests name
    /// their client in `X-Forwarded-For`.
    async fn proxied(&self) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let config = Config {
            clock_offset_ms: self.offset.clone(),
            public_host: Some("127.0.0.1".into()),
            identity: Some(identity(&self.mock, &url, KEY, &self.apple_key)),
            trust_proxy: true,
            ..Config::default()
        };
        let app = router(&self.db, config).unwrap();
        tokio::spawn(async move { mq_sync_server::serve(listener, app).await.unwrap() });
        url
    }

    /// Every text and blob the database holds, as bytes.
    fn every_value(&self) -> Vec<Vec<u8>> {
        let db = self.sql();
        let tables: Vec<String> = db
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut all = Vec::new();
        for table in tables {
            let mut stmt = db.prepare(&format!("SELECT * FROM {table}")).unwrap();
            let n = stmt.column_count();
            let mut rows = stmt.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                for i in 0..n {
                    match row.get_ref(i).unwrap() {
                        rusqlite::types::ValueRef::Text(t) | rusqlite::types::ValueRef::Blob(t) => {
                            all.push(t.to_vec())
                        }
                        _ => {}
                    }
                }
            }
        }
        all
    }
}

fn holds(values: &[Vec<u8>], needle: &str) -> bool {
    values
        .iter()
        .any(|v| v.windows(needle.len()).any(|w| w == needle.as_bytes()))
}

/// The code mailed, `new`, an account; the same address again is `found`.
/// The database never holds the address or the code, and the mail has no link.
#[tokio::test]
async fn an_email_sign_in_makes_an_account_and_keeps_no_address() {
    let relay = Relay::new().await;
    let address = "Alice.Smith@Example.org";
    let flow = mq_sync::identity::start_email(&http(), &relay.url, address)
        .await
        .unwrap();
    let (subject, text) = relay.mock.mail("alice.smith@example.org").expect("mailed");
    let code = relay.mock.code("alice.smith@example.org");
    assert_eq!(
        subject, "Your Momotus sign-in code",
        "no code in the subject"
    );
    assert_eq!(relay.mock.last_sender(), "Momotus <noreply@example.org>");
    assert!(!text.contains("http") && !subject.contains("http"));
    // The kept code is keyed and bound to its flow.
    let kept: Vec<u8> = relay
        .sql()
        .query_row("SELECT code_hash FROM auth_flow", [], |r| r.get(0))
        .unwrap();
    let mut mac =
        <hmac::Hmac<sha2::Sha256> as hmac::KeyInit>::new_from_slice(&hex::decode(KEY).unwrap())
            .unwrap();
    hmac::Mac::update(
        &mut mac,
        format!("email code\0{}\0{code}", flow.flow).as_bytes(),
    );
    assert_eq!(kept, hmac::Mac::finalize(mac).into_bytes().to_vec());
    let taken = flow
        .finish(&http(), &format!("{} {}", &code[..3], &code[3..]))
        .await
        .unwrap();
    assert_eq!(taken.account, None, "new");
    let phone = Device::create(&relay, &taken, false).await.unwrap();
    relay.later(61_000);
    let again = relay.email(" alice.smith@EXAMPLE.org ").await;
    assert_eq!(again.account, Some(phone.record.keys.account), "found");
    assert_eq!(phone.links().await[0].provider, "email");
    let values = relay.every_value();
    for needle in ["alice.smith", "Alice.Smith", "example.org", code.as_str()] {
        assert!(!holds(&values, needle), "the database holds {needle}");
    }
}

/// Case and spaces are one identity; a `+tag` is another identity but the
/// same mailbox for the limits, as are Gmail's dots; malformed addresses
/// answer badAddress and mail nothing.
#[tokio::test]
async fn addresses_normalize_and_mailboxes_share_limits() {
    let relay = Relay::new().await;
    relay.email("bob@example.org").await;
    let tagged = relay.email("bob+films@example.org").await;
    assert_eq!(tagged.account, None, "a tag is another identity");
    relay.email_start("bob@example.org", "v1").await.unwrap();
    assert_eq!(
        relay.email_start("Bob+x@Example.org", "v2").await.err(),
        Some((429, "emailWait".into())),
        "one mailbox"
    );
    relay.email_start("a.b@gmail.com", "v3").await.unwrap();
    assert_eq!(
        relay.email_start("ab+x@googlemail.com", "v4").await.err(),
        Some((429, "emailWait".into()))
    );
    let sent = relay.mock.mails_sent();
    for bad in [
        "no-at.example.org",
        "a@b@example.org",
        "a@127.0.0.1",
        "a@[::1]",
        "<a>@example.org",
        "a,b@example.org",
        "a@example.org.",
        "a@ex%61mple.org",
        "a..b@example.org",
        ".a@example.org",
        "a b@example.org",
        "a@localhost",
        &format!("{}@example.org", "a".repeat(250)),
    ] {
        assert_eq!(
            relay.email_start(bad, "v").await.err(),
            Some((400, "badAddress".into())),
            "{bad}"
        );
    }
    assert_eq!(relay.mock.mails_sent(), sent, "nothing mailed");
    assert_eq!(
        mq_sync::identity::start_email(&http(), &relay.url, "nope")
            .await
            .err()
            .map(|e| e.code()),
        Some(Some("badAddress"))
    );
}

/// Wrong codes: one try answers wrongCode and the right code still works;
/// the fifth ends the flow; misses on an attacker's flow leave the victim's
/// open; another verifier counts nothing; a short code counts nothing.
#[tokio::test]
async fn wrong_codes_end_a_flow_after_five() {
    let relay = Relay::new().await;
    let to = "carol@example.org";
    let flow = relay.email_start(to, "victim").await.unwrap().flow;
    let code = relay.mock.code(to);
    let wrong = if code == "000000" { "111111" } else { "000000" };
    assert_eq!(
        relay.email_take(&flow, "victim", wrong).await.err(),
        Some((403, "wrongCode".into()))
    );
    assert_eq!(
        relay.email_take(&flow, "thief", &code).await.err(),
        Some((404, String::new()))
    );
    assert_eq!(
        relay.email_take(&flow, "victim", "12345").await.err(),
        Some((400, "badCode".into()))
    );
    assert_eq!(
        (relay.events("miss"), relay.events("flowmiss")),
        (1, 1),
        "only the verifier's miss counted"
    );
    relay.email_take(&flow, "victim", &code).await.unwrap();
    assert_eq!(
        relay.email_take(&flow, "victim", &code).await.err(),
        Some((404, String::new())),
        "once"
    );

    relay.later(61_000);
    let mine = relay.email_start(to, "victim").await.unwrap().flow;
    let code = relay.mock.code(to);
    relay.later(61_000);
    let theirs = relay.email_start(to, "attacker").await.unwrap().flow;
    let wrong = if relay.mock.code(to) == "000000" {
        "111111"
    } else {
        "000000"
    };
    for i in 1..=5 {
        let answer = relay
            .email_take(&theirs, "attacker", wrong)
            .await
            .err()
            .unwrap();
        let expected = if i < 5 { "wrongCode" } else { "ticketExpired" };
        assert_eq!(answer, (403, expected.into()), "try {i}");
    }
    assert_eq!(
        relay.email_take(&theirs, "attacker", wrong).await.err(),
        Some((404, String::new())),
        "gone"
    );
    relay.email_take(&mine, "victim", &code).await.unwrap();
}

/// A code lives 10 minutes; a mailbox gets a mail a minute and 10 a day; 20
/// wrong codes a day stop both mail and take; all of it ages out in 24 hours.
#[tokio::test]
async fn mail_and_guess_limits_per_mailbox() {
    let relay = Relay::new().await;
    let to = "dan@example.org";
    let flow = relay.email_start(to, "v").await.unwrap().flow;
    let code = relay.mock.code(to);
    relay.later(10 * 60_000 + 1);
    assert_eq!(
        relay.email_take(&flow, "v", &code).await.err(),
        Some((403, "ticketExpired".into()))
    );
    // From ten networks, so only the mailbox's own cap can say no.
    let proxied = relay.proxied().await;
    for i in 1..10 {
        let from = format!("198.51.100.{i}");
        email_start_at(&proxied, &format!("dan+{i}@example.org"), "v", Some(&from))
            .await
            .unwrap();
        relay.later(61_000);
    }
    assert_eq!(
        email_start_at(&proxied, to, "v", Some("198.51.100.99"))
            .await
            .err(),
        Some((429, "emailLimit".into())),
        "the eleventh"
    );
    relay.later(DAY_MS);
    // Twenty wrong codes over four flows, then the right one is refused too.
    for _ in 0..4 {
        let flow = relay.email_start(to, "v").await.unwrap().flow;
        let code = relay.mock.code(to);
        let wrong = if code == "000000" { "111111" } else { "000000" };
        for _ in 0..5 {
            relay.email_take(&flow, "v", wrong).await.err().unwrap();
        }
        relay.later(61_000);
    }
    let flow = relay.email_start(to, "v").await.err();
    assert_eq!(
        flow,
        Some((429, "emailLimit".into())),
        "no mail after 20 misses"
    );
    relay.later(DAY_MS);
    let flow = relay.email_start(to, "v").await.unwrap().flow;
    let code = relay.mock.code(to);
    relay.email_take(&flow, "v", &code).await.unwrap();
}

/// The twenty-first wrong code in a day: take answers emailLimit, even with
/// the right code, and the flow goes.
#[tokio::test]
async fn take_stops_at_twenty_misses() {
    let relay = Relay::new().await;
    let to = "erin@example.org";
    // The victim's flow first, then an attacker's four flows of five misses.
    let mine = relay.email_start(to, "victim").await.unwrap().flow;
    let code = relay.mock.code(to);
    for _ in 0..4 {
        relay.later(61_000);
        let flow = relay.email_start(to, "attacker").await.unwrap().flow;
        let wrong = if relay.mock.code(to) == "000000" {
            "111111"
        } else {
            "000000"
        };
        for _ in 0..5 {
            relay
                .email_take(&flow, "attacker", wrong)
                .await
                .err()
                .unwrap();
        }
    }
    // A flow id alone deletes nothing, even at the gate.
    assert_eq!(
        relay.email_take(&mine, "stranger", &code).await.err(),
        Some((404, String::new()))
    );
    assert_eq!(
        relay.email_take(&mine, "victim", &code).await.err(),
        Some((429, "emailLimit".into()))
    );
    assert_eq!(
        relay.email_take(&mine, "victim", &code).await.err(),
        Some((404, String::new())),
        "the flow went"
    );
}

/// One client mails 10 mailboxes a day; `dailyLimit` caps the relay; the
/// mail budget stops a sixth start in a minute from one client, not another's.
#[tokio::test]
async fn client_and_relay_mail_caps() {
    let relay = Relay::new().await;
    let proxied = relay.proxied().await;
    for i in 0..5 {
        email_start_at(
            &proxied,
            &format!("f{i}@example.org"),
            "v",
            Some("203.0.113.1"),
        )
        .await
        .unwrap();
    }
    assert_eq!(
        email_start_at(&proxied, "f5@example.org", "v", Some("203.0.113.1"))
            .await
            .err(),
        Some((429, "busy".into())),
        "the minute's budget"
    );
    assert_eq!(
        (relay.count("auth_flow"), relay.count("email_event")),
        (5, 5),
        "a busy start leaves nothing"
    );
    email_start_at(&proxied, "f5@example.org", "v", Some("203.0.113.2"))
        .await
        .unwrap();
    relay.later(61_000);
    for i in 6..11 {
        email_start_at(
            &proxied,
            &format!("f{i}@example.org"),
            "v",
            Some("203.0.113.1"),
        )
        .await
        .unwrap();
    }
    relay.later(61_000);
    assert_eq!(
        email_start_at(&proxied, "f11@example.org", "v", Some("203.0.113.1"))
            .await
            .err(),
        Some((429, "emailLimit".into())),
        "ten a day per client"
    );
    // IPv6 counts per /48 for the day: two /64s of one /48 share it.
    for i in 0..10 {
        let from = format!("2001:db8:7:{}::1", i % 2);
        email_start_at(&proxied, &format!("h{i}@example.org"), "v", Some(&from))
            .await
            .unwrap();
        if i % 4 == 3 {
            relay.later(61_000);
        }
    }
    relay.later(61_000);
    assert_eq!(
        email_start_at(&proxied, "h10@example.org", "v", Some("2001:db8:7:9::1"))
            .await
            .err(),
        Some((429, "emailLimit".into())),
        "one /48"
    );
    email_start_at(&proxied, "h10@example.org", "v", Some("2001:db8:8::1"))
        .await
        .unwrap();

    let capped = Relay::with(|c| {
        if let Some(email) = c.identity.as_mut().and_then(|i| i.email.as_mut()) {
            email.daily_limit = 2;
        }
    })
    .await;
    capped.email_start("g1@example.org", "v").await.unwrap();
    capped.email_start("g2@example.org", "v").await.unwrap();
    assert_eq!(
        capped.email_start("g3@example.org", "v").await.err(),
        Some((503, "mailUnavailable".into()))
    );
}

/// The mail service refusing leaves nothing behind (asking again is not
/// held against the address); no answer keeps the rows, as it may have sent.
#[tokio::test]
async fn a_refused_mail_leaves_nothing_and_an_unclear_one_waits() {
    let relay = Relay::new().await;
    relay.mock.mail_fails(Some(400), false);
    assert_eq!(
        relay.email_start("hal@example.org", "v").await.err(),
        Some((503, "mailUnavailable".into()))
    );
    assert_eq!(
        (relay.count("auth_flow"), relay.count("email_event")),
        (0, 0)
    );
    // Nothing went, so asking again at once is fine.
    relay.mock.mail_fails(None, false);
    relay.email_start("hal@example.org", "v").await.unwrap();
    // A throttle, a 5xx and no answer may all have sent: the user gets the
    // flow and waits for the mail, and the mail counts.
    for (status, stall) in [(Some(429), false), (Some(500), false), (None, true)] {
        relay.later(61_000);
        relay.mock.mail_fails(status, stall);
        let flow = relay
            .email_start("hal@example.org", "v")
            .await
            .unwrap()
            .flow;
        if status == Some(500) {
            // The mock kept this mail before failing, as SES may.
            let code = relay.mock.code("hal@example.org");
            relay.email_take(&flow, "v", &code).await.unwrap();
        }
    }
    assert_eq!(relay.events("mail"), 4, "the unclear mails count");
}

/// An address with an account and one without get the same answer; an
/// email flow is never finished as a native token or by a callback.
#[tokio::test]
async fn email_start_says_nothing_about_membership() {
    let relay = Relay::new().await;
    Device::create(&relay, &relay.email("ivy@example.org").await, false)
        .await
        .unwrap();
    let shape = |s: StartResponse| (s.url, s.nonce, s.expires_ms > 0);
    let member = relay.email_start("ivy@example.org", "v").await.unwrap();
    let stranger = relay.email_start("joe@example.org", "v").await.unwrap();
    assert_eq!(shape(member.clone()), shape(stranger));
    let native = http()
        .post_json(
            &format!("{}/v1/auth/token", relay.url),
            &json!({ "flow": member.flow, "verifier": "v", "idToken": "x" }),
            &raw(vec![]),
        )
        .await;
    assert_eq!(status(native), 404);
    let callback = http()
        .get(
            &format!(
                "{}/v1/auth/email/callback?state={}&code=1",
                relay.url, member.flow
            ),
            &raw(vec![]),
        )
        .await;
    assert_eq!(status(callback), 400, "the closed page");
}

/// The email block: an http mail API off this machine's network is refused,
/// and so is an OpenID entry named `email`. A database from before the
/// email table opens and mails.
#[tokio::test]
async fn email_configuration_and_an_older_database() {
    let mock = Mock::start(0).await;
    let key = mock_oidc::apple_key_pem();
    let dir = fresh_dir();
    let db = dir.join("relay.sqlite");
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
    let plain = start(|f| f["email"]["endpoint"] = json!("http://email.example.com")).unwrap();
    assert!(plain.contains("https"), "{plain}");
    let named = start(|f| f["oidc"][0]["id"] = json!("email")).unwrap();
    assert!(named.contains("email"), "{named}");
    let none = start(|f| f["email"]["dailyLimit"] = json!(0)).unwrap();
    assert!(none.contains("dailyLimit"), "{none}");
    let _ = std::fs::remove_dir_all(dir);

    let relay = Relay::new().await;
    relay.email("kim@example.org").await;
    relay.sql().execute_batch("DROP TABLE email_event").unwrap();
    let again = Relay::on(
        Mock::start(0).await,
        relay.apple_key.clone(),
        relay.db.clone(),
        relay.offset.clone(),
        |_| {},
    )
    .await;
    again.email_start("kim@example.org", "v").await.unwrap();
}

/// A definite refusal gives the client's daily count back: eleven refused
/// starts from one network still leave it all ten mails.
#[tokio::test]
async fn a_refused_mail_gives_the_client_count_back() {
    let relay = Relay::new().await;
    let proxied = relay.proxied().await;
    relay.mock.mail_fails(Some(400), false);
    for i in 0..11 {
        let to = format!("k{i}@example.org");
        let refused = email_start_at(&proxied, &to, "v", Some("192.0.2.7"));
        assert_eq!(
            refused.await.err(),
            Some((503, "mailUnavailable".into())),
            "{i}"
        );
        if i % 4 == 3 {
            relay.later(61_000);
        }
    }
    relay.mock.mail_fails(None, false);
    relay.later(61_000);
    email_start_at(&proxied, "k11@example.org", "v", Some("192.0.2.7"))
        .await
        .unwrap();
}

/// D10 for email: a fourth account in 30 days for one address is refused.
#[tokio::test]
async fn a_fourth_email_sign_up_in_30_days_is_refused() {
    let relay = Relay::new().await;
    Device::create(&relay, &relay.email("sam@example.org").await, false)
        .await
        .unwrap();
    for _ in 0..2 {
        Device::create(&relay, &relay.email("sam@example.org").await, true)
            .await
            .unwrap();
    }
    let fourth = Device::create(&relay, &relay.email("sam@example.org").await, true)
        .await
        .err()
        .unwrap();
    assert!(matches!(fourth, SyncError::SignupLimit), "{fourth:?}");
}

/// The real sign-in budget: the 31st call in a minute from one client
/// answers 429 `busy`; a relay started with a larger budget (the web e2e's
/// `--clients-per-address`) does not.
#[tokio::test]
async fn the_sign_in_budget_is_30_a_minute_unless_one_address_is_many() {
    let take = |url: String| async move {
        let body = json!({ "flow": "none", "verifier": "v", "code": "123456" });
        status(
            http()
                .post_json(&format!("{url}/v1/auth/take"), &body, &raw(vec![]))
                .await,
        )
    };
    let relay = Relay::new().await;
    for i in 0..30 {
        assert_eq!(take(relay.url.clone()).await, 404, "call {i}");
    }
    assert_eq!(take(relay.url.clone()).await, 429, "the 31st");
    let roomy = Relay::with(|c| c.clients_per_address = 2).await;
    for _ in 0..40 {
        assert_eq!(take(roomy.url.clone()).await, 404);
    }
}
