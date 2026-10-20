//! A mock OpenID provider for the relay's sign-in, on loopback: discovery,
//! a JWKS from a key made at start (ES256; RS256 from a test-only key), an
//! authorize page with a subject picker, cancel and stall, a token endpoint,
//! revocation, and Facebook's code exchange and `debug_token`. The relay's
//! tests use it in-process; `cargo run -p mq-sync-server --example mock-oidc`
//! serves it for a person to click through. Real providers are never called.
#![allow(dead_code)]

use axum::extract::{Form, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{
    ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair, RSA_PKCS1_SHA256, RsaKeyPair,
    RsaPublicKeyComponents,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// What the next visit to the authorize page does.
#[derive(Clone, Debug)]
pub enum Next {
    /// The user signs in as this subject.
    Pick(String),
    /// As `Pick`, but the ID token carries another nonce than the flow's.
    WrongNonce(String),
    /// The user cancels: the relay gets `error=access_denied`.
    Cancel,
    /// The page never answers.
    Stall,
}

struct Grant {
    sub: String,
    nonce: Option<String>,
    client_id: String,
}

enum Key {
    Ec { pair: EcdsaKeyPair, kid: String },
    Rsa { pair: RsaKeyPair, kid: String },
}

#[derive(Default)]
struct Shared {
    issuer: String,
    keys: Vec<Key>,
    next: VecDeque<Next>,
    codes: HashMap<String, Grant>,
    access: HashMap<String, (String, String)>,
    secrets: Vec<(String, String)>,
    revoked: Vec<String>,
    /// Facebook's `debug_token` answers for this app instead of the real one.
    other_app: bool,
}

#[derive(Clone)]
pub struct Mock {
    pub url: String,
    shared: Arc<Mutex<Shared>>,
    fetches: Arc<AtomicUsize>,
}

const RSA_TEST_KEY: &[u8] = include_bytes!("mock-rsa.der");

fn random() -> String {
    let mut b = [0u8; 24];
    SystemRandom::new().fill(&mut b).expect("os randomness");
    B64URL.encode(b)
}

fn ec_key(kid: &str) -> Key {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).expect("key");
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
        .expect("key");
    Key::Ec {
        pair,
        kid: kid.into(),
    }
}

/// A P-256 key as PKCS#8 PEM, as Apple hands a `.p8` out.
pub fn apple_key_pem() -> String {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .expect("key");
    let body = base64::engine::general_purpose::STANDARD.encode(pkcs8.as_ref());
    format!("-----BEGIN PRIVATE KEY-----\n{body}\n-----END PRIVATE KEY-----\n")
}

impl Key {
    fn kid(&self) -> &str {
        match self {
            Key::Ec { kid, .. } | Key::Rsa { kid, .. } => kid,
        }
    }

    fn jwk(&self) -> Value {
        match self {
            Key::Ec { pair, kid } => {
                let point = pair.public_key().as_ref();
                json!({
                    "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig", "kid": kid,
                    "x": B64URL.encode(&point[1..33]), "y": B64URL.encode(&point[33..65]),
                })
            }
            Key::Rsa { pair, kid } => {
                let parts = RsaPublicKeyComponents::<Vec<u8>>::from(pair.public());
                json!({
                    "kty": "RSA", "alg": "RS256", "use": "sig", "kid": kid,
                    "n": B64URL.encode(&parts.n), "e": B64URL.encode(&parts.e),
                })
            }
        }
    }

    fn sign(&self, claims: &Value) -> String {
        let (alg, kid) = match self {
            Key::Ec { kid, .. } => ("ES256", kid),
            Key::Rsa { kid, .. } => ("RS256", kid),
        };
        let header = json!({ "alg": alg, "kid": kid, "typ": "JWT" });
        let signed = format!(
            "{}.{}",
            B64URL.encode(header.to_string()),
            B64URL.encode(claims.to_string())
        );
        let rng = SystemRandom::new();
        let sig = match self {
            Key::Ec { pair, .. } => pair
                .sign(&rng, signed.as_bytes())
                .expect("sign")
                .as_ref()
                .to_vec(),
            Key::Rsa { pair, .. } => {
                let mut sig = vec![0u8; pair.public().modulus_len()];
                pair.sign(&RSA_PKCS1_SHA256, &rng, signed.as_bytes(), &mut sig)
                    .expect("sign");
                sig
            }
        };
        format!("{signed}.{}", B64URL.encode(sig))
    }
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

impl Mock {
    /// Serves on `127.0.0.1:port` (0: any free port).
    pub async fn start(port: u16) -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let rsa = RsaKeyPair::from_pkcs8(RSA_TEST_KEY).expect("the test RSA key");
        let shared = Shared {
            issuer: url.clone(),
            keys: vec![
                ec_key("ec-1"),
                Key::Rsa {
                    pair: rsa,
                    kid: "rsa-1".into(),
                },
            ],
            ..Shared::default()
        };
        let mock = Self {
            url,
            shared: Arc::new(Mutex::new(shared)),
            fetches: Arc::default(),
        };
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .route("/revoke", post(revoke))
            .route("/oauth/access_token", get(facebook_exchange))
            .route("/debug_token", get(debug_token))
            .with_state(mock.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        mock
    }

    fn with<T>(&self, f: impl FnOnce(&mut Shared) -> T) -> T {
        f(&mut self.shared.lock().unwrap())
    }

    /// Queues what the next authorize visit does.
    pub fn next(&self, next: Next) {
        self.with(|s| s.next.push_back(next));
    }

    /// Signs `claims` with the current ES256 key (the first key).
    pub fn sign(&self, claims: Value) -> String {
        self.with(|s| s.keys[0].sign(&claims))
    }

    /// Signs with the RS256 test key.
    pub fn sign_rsa(&self, claims: Value) -> String {
        self.with(|s| {
            s.keys
                .iter()
                .find(|k| matches!(k, Key::Rsa { .. }))
                .unwrap()
                .sign(&claims)
        })
    }

    /// Signs with a key the JWKS does not publish (made now, same kid as
    /// the current one): its signature cannot check.
    pub fn sign_forged(&self, claims: Value) -> String {
        let kid = self.with(|s| s.keys[0].kid().to_owned());
        ec_key(&kid).sign(&claims)
    }

    /// The current key's kid.
    pub fn kid(&self) -> String {
        self.with(|s| s.keys[0].kid().to_owned())
    }

    /// The current key as the JWKS publishes it.
    pub fn public_jwk(&self) -> Value {
        self.with(|s| s.keys[0].jwk())
    }

    /// The current (ES256) key's signature of `input`.
    pub fn ec_signature(&self, input: &[u8]) -> Vec<u8> {
        self.with(|s| match &s.keys[0] {
            Key::Ec { pair, .. } => pair
                .sign(&SystemRandom::new(), input)
                .expect("sign")
                .as_ref()
                .to_vec(),
            Key::Rsa { .. } => unreachable!("the current key is EC"),
        })
    }

    /// A token with this header, signed by `sign` over its signing input:
    /// the alg-confusion and unknown-kid cases.
    pub fn sign_with(
        &self,
        header: Value,
        claims: &Value,
        sign: impl FnOnce(&[u8]) -> Vec<u8>,
    ) -> String {
        let signed = format!(
            "{}.{}",
            B64URL.encode(header.to_string()),
            B64URL.encode(claims.to_string())
        );
        let sig = sign(signed.as_bytes());
        format!("{signed}.{}", B64URL.encode(sig))
    }

    /// A new current key under a new kid, published from now on.
    pub fn rotate(&self, kid: &str) {
        self.with(|s| s.keys.insert(0, ec_key(kid)));
    }

    /// ID token claims as the mock issues them, for `aud` and `sub`.
    pub fn claims(&self, aud: &str, sub: &str, nonce: &str) -> Value {
        let now = now_s();
        let iss = self.with(|s| s.issuer.clone());
        json!({ "iss": iss, "aud": aud, "sub": sub, "nonce": nonce, "iat": now, "exp": now + 600 })
    }

    /// How often the JWKS was fetched.
    pub fn jwks_fetches(&self) -> usize {
        self.fetches.load(Ordering::Relaxed)
    }

    /// Every (client id, client secret) the token endpoint saw.
    pub fn secrets(&self) -> Vec<(String, String)> {
        self.with(|s| s.secrets.clone())
    }

    pub fn revoked(&self) -> Vec<String> {
        self.with(|s| s.revoked.clone())
    }

    /// An authorization code as a native sheet gets it, for `client_id`.
    pub fn native_code(&self, sub: &str, client_id: &str) -> String {
        let code = random();
        self.with(|s| {
            s.codes.insert(
                code.clone(),
                Grant {
                    sub: sub.into(),
                    nonce: None,
                    client_id: client_id.into(),
                },
            )
        });
        code
    }

    /// Facebook's `debug_token` names another app from now on.
    pub fn answer_for_another_app(&self) {
        self.with(|s| s.other_app = true);
    }
}

async fn discovery(State(mock): State<Mock>) -> Json<Value> {
    let url = &mock.url;
    Json(json!({
        "issuer": url,
        "authorization_endpoint": format!("{url}/authorize"),
        "token_endpoint": format!("{url}/token"),
        "jwks_uri": format!("{url}/jwks"),
        "revocation_endpoint": format!("{url}/revoke"),
    }))
}

async fn jwks(State(mock): State<Mock>) -> Response {
    mock.fetches.fetch_add(1, Ordering::Relaxed);
    let keys: Vec<Value> = mock.with(|s| s.keys.iter().map(Key::jwk).collect());
    (
        [(header::CACHE_CONTROL, "public, max-age=3600")],
        Json(json!({ "keys": keys })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct Authorize {
    client_id: String,
    redirect_uri: String,
    state: String,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    response_mode: Option<String>,
    /// From the picker page: who signs in, or `cancel`.
    #[serde(default)]
    choose: Option<String>,
}

fn back(to: &str, fields: &[(&str, &str)]) -> String {
    let mut url = url::Url::parse(to).expect("a redirect URI");
    url.query_pairs_mut().extend_pairs(fields.iter().copied());
    url.into()
}

/// The authorize page: what the test queued, or (nothing queued) a picker
/// for a person.
async fn authorize(State(mock): State<Mock>, Query(q): Query<Authorize>) -> Response {
    let next = match q.choose.as_deref() {
        Some("cancel") => Some(Next::Cancel),
        Some("stall") => Some(Next::Stall),
        Some(sub) => Some(Next::Pick(sub.into())),
        None => mock.with(|s| s.next.pop_front()),
    };
    let Some(next) = next else {
        let link = |choose: &str| {
            let mut url = url::Url::parse(&format!("{}/authorize", mock.url)).unwrap();
            url.query_pairs_mut()
                .append_pair("client_id", &q.client_id)
                .append_pair("redirect_uri", &q.redirect_uri)
                .append_pair("state", &q.state)
                .append_pair("nonce", q.nonce.as_deref().unwrap_or_default())
                .append_pair(
                    "response_mode",
                    q.response_mode.as_deref().unwrap_or_default(),
                )
                .append_pair("choose", choose);
            String::from(url)
        };
        return Html(format!(
            "<!doctype html><title>Mock sign-in</title><h1>Sign in to the mock provider</h1>\
             <p><a href=\"{}\">Sign in as alice</a></p><p><a href=\"{}\">Sign in as bob</a></p>\
             <p><a href=\"{}\">Cancel</a></p><p><a href=\"{}\">Stall</a></p>",
            link("alice"),
            link("bob"),
            link("cancel"),
            link("stall")
        ))
        .into_response();
    };
    match next {
        Next::Stall => std::future::pending().await,
        Next::Cancel => (
            StatusCode::FOUND,
            [(
                header::LOCATION,
                back(
                    &q.redirect_uri,
                    &[("error", "access_denied"), ("state", &q.state)],
                ),
            )],
        )
            .into_response(),
        Next::Pick(_) | Next::WrongNonce(_) => {
            let (sub, nonce) = match next {
                Next::WrongNonce(sub) => (sub, Some("not the flow's nonce".to_owned())),
                Next::Pick(sub) => (sub, q.nonce.clone().filter(|n| !n.is_empty())),
                _ => unreachable!(),
            };
            let code = random();
            mock.with(|s| {
                s.codes.insert(
                    code.clone(),
                    Grant {
                        sub,
                        nonce,
                        client_id: q.client_id.clone(),
                    },
                )
            });
            if q.response_mode.as_deref() == Some("form_post") {
                return Html(format!(
                    "<!doctype html><form method=\"post\" action=\"{}\">\
                     <input type=\"hidden\" name=\"code\" value=\"{code}\">\
                     <input type=\"hidden\" name=\"state\" value=\"{}\"></form>\
                     <script>document.forms[0].submit()</script>",
                    q.redirect_uri, q.state
                ))
                .into_response();
            }
            (
                StatusCode::FOUND,
                [(
                    header::LOCATION,
                    back(&q.redirect_uri, &[("code", &code), ("state", &q.state)]),
                )],
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct TokenForm {
    code: String,
    client_id: String,
    client_secret: String,
}

async fn token(State(mock): State<Mock>, Form(form): Form<TokenForm>) -> Response {
    let issued = mock.with(|s| {
        s.secrets.push((form.client_id.clone(), form.client_secret.clone()));
        let grant = s.codes.remove(&form.code)?;
        if grant.client_id != form.client_id {
            return None;
        }
        let now = now_s();
        let mut claims = json!({ "iss": s.issuer, "aud": grant.client_id, "sub": grant.sub, "iat": now, "exp": now + 600 });
        if let Some(nonce) = &grant.nonce {
            claims["nonce"] = json!(nonce);
        }
        let access = random();
        s.access.insert(access.clone(), (grant.sub.clone(), grant.client_id.clone()));
        Some(json!({
            "id_token": s.keys[0].sign(&claims), "access_token": access,
            "refresh_token": random(), "token_type": "Bearer", "expires_in": 3600,
        }))
    });
    match issued {
        Some(answer) => Json(answer).into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant" })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct RevokeForm {
    client_id: String,
    client_secret: String,
    token: String,
}

async fn revoke(State(mock): State<Mock>, Form(form): Form<RevokeForm>) -> StatusCode {
    mock.with(|s| {
        s.secrets.push((form.client_id, form.client_secret));
        s.revoked.push(form.token);
    });
    StatusCode::OK
}

#[derive(Deserialize)]
struct FacebookExchange {
    code: String,
    client_id: String,
    client_secret: String,
}

async fn facebook_exchange(
    State(mock): State<Mock>,
    Query(q): Query<FacebookExchange>,
) -> Response {
    let access = mock.with(|s| {
        s.secrets
            .push((q.client_id.clone(), q.client_secret.clone()));
        let grant = s
            .codes
            .remove(&q.code)
            .filter(|g| g.client_id == q.client_id)?;
        let access = random();
        s.access
            .insert(access.clone(), (grant.sub, grant.client_id));
        Some(access)
    });
    match access {
        Some(access) => {
            Json(json!({ "access_token": access, "token_type": "bearer" })).into_response()
        }
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": { "message": "bad code" } })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct DebugToken {
    input_token: String,
    access_token: String,
}

async fn debug_token(State(mock): State<Mock>, Query(q): Query<DebugToken>) -> Json<Value> {
    Json(mock.with(|s| match s.access.get(&q.input_token) {
        Some((sub, app)) if q.access_token.starts_with(&format!("{app}|")) => json!({ "data": {
            "app_id": if s.other_app { "another app" } else { app.as_str() },
            "is_valid": true, "user_id": sub,
        }}),
        _ => json!({ "data": { "is_valid": false } }),
    }))
}

/// The test's browser: follows the provider page and the relay's callback
/// (a redirect, or Apple's form post) and stops at the return, whose
/// fragment it answers.
pub async fn browse(url: &str) -> String {
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let page = browser.get(url).send().await.unwrap();
    let back = match page.headers().get("location") {
        Some(to) => browser.get(to.to_str().unwrap()).send().await.unwrap(),
        None => {
            let html = page.text().await.unwrap();
            let pick = |name: &str| {
                let at = html.find(&format!("name=\"{name}\" value=\"")).unwrap() + name.len() + 15;
                html[at..html[at..].find('"').unwrap() + at].to_owned()
            };
            let action = {
                let at = html.find("action=\"").unwrap() + 8;
                html[at..html[at..].find('"').unwrap() + at].to_owned()
            };
            browser
                .post(&action)
                .form(&[("code", pick("code")), ("state", pick("state"))])
                .send()
                .await
                .unwrap()
        }
    };
    assert_eq!(back.status(), 303, "{:?}", back.text().await);
    let to = back
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(to.starts_with("marquee://auth#"), "{to}");
    to.split_once('#').unwrap().1.to_owned()
}
