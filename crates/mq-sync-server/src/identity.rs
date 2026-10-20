//! Sign-in with Apple, Google, Facebook or any OpenID provider
//! (plans/accounts.md section 2). An identity only finds an account, never
//! opens one: the relay keeps a keyed hash of the provider's subject, and
//! nothing else a provider sends.
//!
//! A flow starts with the device's verifier hash. Brokered, the provider
//! page comes back to the relay's callback, which redirects to the app with
//! a one-time code in the fragment; the device takes its result with the
//! verifier and that code. Native, the device hands the ID token of its own
//! sheet over, its nonce bound to the relay's host. Either way the result is
//! a ticket: 10 minutes, one use, the identity and nothing more.

// The routes here answer early with whole responses; they are cold (one
// sign-in at a time), so boxing those answers would buy nothing.
#![allow(clippy::result_large_err)]

use crate::jwt::{self, Jwk, JwkSet, Jwt};
use crate::{
    APPLE_NOTICES, AUTH, AppState, active, authorize, client, client_limited, error,
    journal_deletion, limited, lock, random, relay_limited, same, storage_error, token_hash,
};
use axum::extract::Path as UrlPath;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Form, Json};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use hmac::{Hmac, KeyInit, Mac};
use mq_http::{Body, HttpClient, Method, RequestOptions};
use mq_library::hlc::now_ms;
use mq_sync::roster::Id;
use mq_sync::wire::{
    CancelDeletion, IdentityLink, LinkRequest, NativeToken, StartRequest, StartResponse,
    TakeRequest, Taken, UnlinkRequest, nonce_for,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

/// An open flow, a ticket and a receipt each live this long.
pub(crate) const FLOW_MS: u64 = 10 * 60 * 1000;
pub(crate) const TICKET_MS: u64 = 10 * 60 * 1000;
const RECEIPT_MS: u64 = 10 * 60 * 1000;
/// Sign-ups one identity may make in `SIGNUP_WINDOW_MS`.
pub(crate) const SIGNUP_CAP: i64 = 3;
pub(crate) const SIGNUP_WINDOW_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// A deletion asked for from the web page waits this long, shown on every device.
pub(crate) const DELETION_WAIT_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// A flow past its time is kept this much longer, emptied, so a late
/// return or take hears that it expired rather than that it never was.
const EXPIRED_KEPT_MS: u64 = 10 * 60 * 1000;
const MAX_FLOWS: i64 = 100_000;
const FLOWS_PER_CLIENT: i64 = 20;
/// A provider's key set is fetched at most this often: a token naming an
/// unknown key fetches nothing more inside it.
const REFETCH_MS: u64 = 60 * 1000;
/// Apple's notifications older than this are refused (a captured one
/// replayed later would drop a link the user made again).
const NOTICE_MAX_AGE_S: u64 = 24 * 60 * 60;

const APPLE_ISSUER: &str = "https://appleid.apple.com";
const GOOGLE_ISSUER: &str = "https://accounts.google.com";

/// The identity file (`--identity`). Secrets stay in memory; nothing here
/// is ever logged.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityFile {
    /// 32 bytes as hex: the key of the subject hashes. Back it up with the
    /// database: another key turns every identity into a new one.
    pub key: String,
    /// The relay's own URL as providers redirect to it; `https://<public host>` by default.
    #[serde(default)]
    pub public_url: Option<String>,
    /// The link host: the deletion page's origin (CORS) and return.
    #[serde(default)]
    pub link_host: Option<String>,
    #[serde(default)]
    pub apple: Option<AppleConfig>,
    #[serde(default)]
    pub google: Option<GoogleConfig>,
    #[serde(default)]
    pub facebook: Option<FacebookConfig>,
    /// Standard OpenID providers (the mock among them), by route id.
    #[serde(default)]
    pub oidc: Vec<OidcConfig>,
}

impl std::fmt::Debug for IdentityFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityFile").finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppleConfig {
    pub team_id: String,
    pub key_id: String,
    /// The Sign in with Apple key, PEM (the `.p8` file's text).
    pub key: String,
    /// The Services ID: the client of brokered flows.
    pub services_id: String,
    /// Bundle ids the native sheet signs for, the tvOS app's among them.
    #[serde(default)]
    pub bundle_ids: Vec<String>,
    #[serde(default = "apple_issuer")]
    pub issuer: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoogleConfig {
    pub web_client_id: String,
    pub web_client_secret: String,
    /// The Android and iOS client ids, taken as audiences of native tokens.
    #[serde(default)]
    pub client_ids: Vec<String>,
    #[serde(default = "google_issuer")]
    pub issuer: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FacebookConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default = "facebook_graph")]
    pub graph: String,
    #[serde(default = "facebook_dialog")]
    pub dialog: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OidcConfig {
    /// The route name (`/v1/auth/{id}`).
    pub id: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
}

fn apple_issuer() -> String {
    APPLE_ISSUER.into()
}
fn google_issuer() -> String {
    GOOGLE_ISSUER.into()
}
fn facebook_graph() -> String {
    "https://graph.facebook.com/v21.0".into()
}
fn facebook_dialog() -> String {
    "https://www.facebook.com/v21.0/dialog/oauth".into()
}

/// A provider or relay address: https, or plain http only on this machine's
/// network (the mock). An `http://` issuer elsewhere would let anyone on the
/// path serve keys and mint any identity.
fn secure(what: &str, address: &str) -> Result<(), String> {
    let url = url::Url::parse(address).map_err(|_| format!("{what} is not a URL: {address}"))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_local(&url) => Ok(()),
        _ => Err(format!("{what} must be https: {address}")),
    }
}

fn is_local(url: &url::Url) -> bool {
    url.host_str().is_some_and(mq_http::is_private_host)
}

/// An endpoint a discovery document names: by its issuer's rule, not its
/// own. Only an issuer on this machine's network (the mock) may name plain
/// http or private addresses; a public issuer's endpoints are https on
/// public hosts, so a hostile document cannot aim the relay inward.
fn discovered(issuer: &str, endpoint: &str) -> Result<(), String> {
    let local_issuer = url::Url::parse(issuer).is_ok_and(|u| is_local(&u));
    if local_issuer {
        return secure("a discovered endpoint", endpoint);
    }
    let url = url::Url::parse(endpoint)
        .map_err(|_| format!("a discovered endpoint is not a URL: {endpoint}"))?;
    if url.scheme() == "https" && !is_local(&url) {
        Ok(())
    } else {
        Err(format!(
            "a public issuer's endpoint must be https on a public host: {endpoint}"
        ))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Apple,
    Google,
    Facebook,
    Oidc(usize),
}

#[derive(Clone, Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    revocation_endpoint: Option<String>,
}

struct CachedKeys {
    keys: Vec<Jwk>,
    until_ms: u64,
    /// When the set was last asked for, answered or not.
    fetched_ms: u64,
}

/// What the callback or the token route learned: whose identity, and what
/// the flow asked for.
#[derive(Serialize, Deserialize)]
struct Verified {
    provider: String,
    subject: String,
    #[serde(default)]
    receipt: Option<String>,
}

/// The relay's sign-in state: the configuration and the provider caches.
pub struct Identity {
    file: IdentityFile,
    key: [u8; 32],
    apple_key: Option<Vec<u8>>,
    public_host: String,
    public_url: String,
    http: HttpClient,
    discovery: Mutex<HashMap<String, Discovery>>,
    jwks: Mutex<HashMap<String, CachedKeys>>,
}

impl Identity {
    pub fn new(file: IdentityFile, public_host: &str) -> Result<Self, String> {
        let key: [u8; 32] = hex::decode(&file.key)
            .ok()
            .and_then(|k| k.try_into().ok())
            .ok_or("the identity key must be 32 bytes of hex")?;
        let apple_key = match &file.apple {
            Some(apple) => Some(jwt::pem_der(&apple.key).ok_or("the Apple key is not PEM")?),
            None => None,
        };
        let public_url = file
            .public_url
            .clone()
            .unwrap_or_else(|| format!("https://{public_host}"));
        let public_url = public_url.trim_end_matches('/').to_owned();
        // Callbacks and nonces name one relay: the one devices know.
        let named = url::Url::parse(&public_url).ok();
        if named.as_ref().and_then(|u| u.host_str()) != Some(public_host) {
            return Err(format!(
                "publicUrl's host must be the public host {public_host}"
            ));
        }
        let f = &file;
        let addresses = [
            Some(("publicUrl", public_url.as_str())),
            f.apple
                .as_ref()
                .map(|a| ("the Apple issuer", a.issuer.as_str())),
            f.google
                .as_ref()
                .map(|g| ("the Google issuer", g.issuer.as_str())),
            f.facebook
                .as_ref()
                .map(|fb| ("Facebook's graph", fb.graph.as_str())),
            f.facebook
                .as_ref()
                .map(|fb| ("Facebook's dialog", fb.dialog.as_str())),
        ];
        let oidc = f
            .oidc
            .iter()
            .map(|o| ("an OpenID issuer", o.issuer.as_str()));
        for (what, address) in addresses.into_iter().flatten().chain(oidc) {
            secure(what, address)?;
        }
        Ok(Self {
            key,
            apple_key,
            public_host: public_host.to_owned(),
            public_url,
            http: HttpClient::new(Default::default()).map_err(|e| e.to_string())?,
            discovery: Mutex::default(),
            jwks: Mutex::default(),
            file,
        })
    }

    /// Names the key without revealing it, for the database to check.
    pub fn fingerprint(&self) -> String {
        hex::encode(&Sha256::digest([b"marquee/identity-key/v1\0", &self.key[..]].concat())[..16])
    }

    /// The route names of the configured providers.
    pub fn providers(&self) -> Vec<String> {
        let f = &self.file;
        let fixed = [
            f.apple.as_ref().map(|_| "apple"),
            f.google.as_ref().map(|_| "google"),
            f.facebook.as_ref().map(|_| "facebook"),
        ];
        fixed
            .into_iter()
            .flatten()
            .map(str::to_owned)
            .chain(f.oidc.iter().map(|o| o.id.clone()))
            .collect()
    }

    pub fn link_host(&self) -> Option<&str> {
        self.file.link_host.as_deref()
    }

    fn kind(&self, name: &str) -> Option<Kind> {
        match name {
            "apple" if self.file.apple.is_some() => Some(Kind::Apple),
            "google" if self.file.google.is_some() => Some(Kind::Google),
            "facebook" if self.file.facebook.is_some() => Some(Kind::Facebook),
            _ => self
                .file
                .oidc
                .iter()
                .position(|o| o.id == name)
                .map(Kind::Oidc),
        }
    }

    /// The fixed provider name a subject is hashed under, never a raw `iss`.
    fn provider_name(&self, kind: Kind) -> String {
        match kind {
            Kind::Apple => "apple".into(),
            Kind::Google => "google".into(),
            Kind::Facebook => "facebook".into(),
            Kind::Oidc(i) => format!("oidc:{}", self.file.oidc[i].issuer),
        }
    }

    /// A stored provider name as the apps see it: the route id `/v1/relay`
    /// lists (`apple`, `google`, `facebook`, an OpenID entry's `id`), never
    /// an issuer URL; `oidc` for an OpenID provider no longer configured.
    pub(crate) fn display(&self, stored: &str) -> String {
        match stored.strip_prefix("oidc:") {
            Some(issuer) => self
                .file
                .oidc
                .iter()
                .find(|o| o.issuer == issuer)
                .map_or_else(|| "oidc".to_owned(), |o| o.id.clone()),
            None => stored.to_owned(),
        }
    }

    /// `HMAC-SHA256(identity key, provider || 0x00 || sub)`.
    fn subject(&self, provider: &str, sub: &str) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).expect("any key length");
        mac.update(provider.as_bytes());
        mac.update(&[0]);
        mac.update(sub.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    fn issuer(&self, kind: Kind) -> Option<&str> {
        match kind {
            Kind::Apple => self.file.apple.as_ref().map(|a| a.issuer.as_str()),
            Kind::Google => self.file.google.as_ref().map(|g| g.issuer.as_str()),
            Kind::Oidc(i) => Some(self.file.oidc[i].issuer.as_str()),
            Kind::Facebook => None,
        }
    }

    /// Google documents two spellings of its issuer, with and without the
    /// scheme (`https://accounts.google.com`, `accounts.google.com`); both
    /// are one provider.
    fn issuers(&self, kind: Kind) -> Vec<String> {
        let Some(issuer) = self.issuer(kind) else {
            return Vec::new();
        };
        let mut all = vec![issuer.to_owned()];
        if kind == Kind::Google
            && let Some((_, bare)) = issuer.split_once("://")
        {
            all.push(bare.to_owned());
        }
        all
    }

    /// The client of brokered flows: its id and secret at the token endpoint.
    fn brokered_client(&self, kind: Kind) -> Result<(String, String), String> {
        let f = &self.file;
        Ok(match kind {
            Kind::Apple => {
                let apple = f.apple.as_ref().ok_or("no Apple")?;
                (
                    apple.services_id.clone(),
                    self.apple_secret(&apple.services_id)?,
                )
            }
            Kind::Google => {
                let g = f.google.as_ref().ok_or("no Google")?;
                (g.web_client_id.clone(), g.web_client_secret.clone())
            }
            Kind::Facebook => {
                let fb = f.facebook.as_ref().ok_or("no Facebook")?;
                (fb.app_id.clone(), fb.app_secret.clone())
            }
            Kind::Oidc(i) => (f.oidc[i].client_id.clone(), f.oidc[i].client_secret.clone()),
        })
    }

    /// The audiences a native sheet's token may carry.
    fn native_audiences(&self, kind: Kind) -> Vec<String> {
        let f = &self.file;
        match kind {
            Kind::Apple => f.apple.iter().flat_map(|a| a.bundle_ids.clone()).collect(),
            Kind::Google => f
                .google
                .iter()
                .flat_map(|g| std::iter::once(g.web_client_id.clone()).chain(g.client_ids.clone()))
                .collect(),
            Kind::Oidc(i) => vec![f.oidc[i].client_id.clone()],
            Kind::Facebook => Vec::new(),
        }
    }

    /// An Apple client secret: an ES256 JWT for `client_id` (the Services ID
    /// for brokered codes, a bundle id for native ones), valid an hour.
    fn apple_secret(&self, client_id: &str) -> Result<String, String> {
        let apple = self.file.apple.as_ref().ok_or("no Apple")?;
        let key = self.apple_key.as_deref().ok_or("no Apple key")?;
        let now = now_ms() / 1000;
        let claims = json!({
            "iss": apple.team_id, "iat": now, "exp": now + 3600,
            "aud": apple.issuer, "sub": client_id,
        });
        jwt::sign_es256(key, &apple.key_id, &claims)
    }

    fn callback(&self, provider: &str) -> String {
        format!("{}/v1/auth/{provider}/callback", self.public_url)
    }

    /// Provider calls go where the operator configured them; a provider on
    /// this machine's network (the mock) is reached there too.
    fn options(&self, url: &str) -> RequestOptions {
        let private = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(mq_http::is_private_host))
            .unwrap_or(false);
        RequestOptions {
            allow_private: private,
            timeout: Duration::from_secs(15),
            max_bytes: 1024 * 1024,
            ..Default::default()
        }
    }

    // ponytail: discovery cached for the process's life; refetch on a
    // schedule if a provider ever moves its endpoints.
    async fn discover(&self, kind: Kind) -> Result<Discovery, String> {
        let issuer = self.issuer(kind).ok_or("no discovery for this provider")?;
        if let Some(found) = self
            .discovery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(issuer)
        {
            return Ok(found.clone());
        }
        let url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        let found: Discovery = self
            .http
            .get(&url, &self.options(&url))
            .await
            .and_then(|r| r.json())
            .map_err(|e| format!("discovery: {}", e.kind()))?;
        // OpenID Discovery 4.3: the document names the issuer it was asked for.
        if found.issuer != issuer {
            return Err("discovery: another issuer".into());
        }
        let endpoints = [
            Some(found.authorization_endpoint.as_str()),
            Some(found.token_endpoint.as_str()),
            Some(found.jwks_uri.as_str()),
            found.revocation_endpoint.as_deref(),
        ];
        for endpoint in endpoints.into_iter().flatten() {
            discovered(issuer, endpoint)?;
        }
        self.discovery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(issuer.to_owned(), found.clone());
        Ok(found)
    }

    /// The key a token names, from the JWKS cached per `Cache-Control`; an
    /// unknown `kid` fetches the set once more (a provider rotated its keys),
    /// but never twice within `REFETCH_MS`: tokens anyone can send must not
    /// make the relay call the provider at will. `now` is relay time.
    async fn key_for(&self, uri: &str, kid: Option<&str>, now: u64) -> Result<Jwk, String> {
        {
            let mut cache = self.jwks.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(cached) = cache.get(uri) {
                let recent = now < cached.fetched_ms + REFETCH_MS;
                if cached.until_ms > now || recent {
                    if let Some(key) = jwt::find(&cached.keys, kid) {
                        return Ok(key.clone());
                    }
                    if recent {
                        return Err("unknown key, the set was fetched just now".into());
                    }
                }
            }
            // Marked before the fetch, so requests meanwhile do not fetch too.
            cache
                .entry(uri.to_owned())
                .and_modify(|c| c.fetched_ms = now)
                .or_insert(CachedKeys {
                    keys: Vec::new(),
                    until_ms: 0,
                    fetched_ms: now,
                });
        }
        let response = self
            .http
            .get(uri, &self.options(uri))
            .await
            .map_err(|e| format!("keys: {}", e.kind()))?;
        let set: JwkSet = response.json().map_err(|e| format!("keys: {}", e.kind()))?;
        let max_age = response
            .headers
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.split(',')
                    .find_map(|d| d.trim().strip_prefix("max-age=")?.parse::<u64>().ok())
            })
            .unwrap_or(3600);
        let key = jwt::find(&set.keys, kid).cloned();
        self.jwks.lock().unwrap_or_else(|p| p.into_inner()).insert(
            uri.to_owned(),
            CachedKeys {
                keys: set.keys,
                until_ms: now + max_age * 1000,
                fetched_ms: now,
            },
        );
        key.ok_or_else(|| "unknown key".into())
    }

    /// Checks an ID token of `kind` and returns its subject. Its times go by
    /// the machine's clock, as the provider minted it; `now` (relay time)
    /// paces the key fetches.
    async fn verify(
        &self,
        kind: Kind,
        token: &str,
        audiences: &[String],
        nonce: &str,
        now: u64,
    ) -> Result<String, String> {
        let disc = self.discover(kind).await?;
        let jwt = Jwt::parse(token).ok_or("not a JWT")?;
        let key = self.key_for(&disc.jwks_uri, jwt.kid(), now).await?;
        if !jwt.verifies(&key) {
            return Err("signature".into());
        }
        jwt::check_claims(
            jwt.claims(),
            &self.issuers(kind),
            audiences,
            nonce,
            now_ms(),
        )
        .map_err(str::to_owned)
    }

    /// The provider page of a brokered flow.
    async fn authorize_url(
        &self,
        kind: Kind,
        provider: &str,
        flow: &str,
        nonce: &str,
    ) -> Result<String, String> {
        let (client_id, _) = match kind {
            // The secret is not needed for the page; Apple's is never made for it.
            Kind::Apple => (
                self.file
                    .apple
                    .as_ref()
                    .ok_or("no Apple")?
                    .services_id
                    .clone(),
                String::new(),
            ),
            _ => self.brokered_client(kind)?,
        };
        let base = match kind {
            Kind::Facebook => self
                .file
                .facebook
                .as_ref()
                .ok_or("no Facebook")?
                .dialog
                .clone(),
            _ => self.discover(kind).await?.authorization_endpoint,
        };
        let mut url = url::Url::parse(&base).map_err(|e| e.to_string())?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("client_id", &client_id)
                .append_pair("redirect_uri", &self.callback(provider))
                .append_pair("response_type", "code")
                .append_pair("state", flow);
            match kind {
                Kind::Facebook => {
                    q.append_pair("scope", "public_profile");
                }
                Kind::Apple => {
                    q.append_pair("response_mode", "form_post")
                        .append_pair("nonce", nonce);
                }
                _ => {
                    q.append_pair("scope", "openid").append_pair("nonce", nonce);
                }
            }
        }
        Ok(url.into())
    }

    /// Exchanges a code at the token endpoint: the JSON answer.
    async fn exchange(
        &self,
        kind: Kind,
        client: (String, String),
        code: &str,
        redirect: &str,
    ) -> Result<Value, String> {
        let (endpoint, method) = match kind {
            Kind::Facebook => (
                format!(
                    "{}/oauth/access_token",
                    self.file.facebook.as_ref().ok_or("no Facebook")?.graph
                ),
                Method::GET,
            ),
            _ => (self.discover(kind).await?.token_endpoint, Method::POST),
        };
        // A native sheet's code has no redirect: none is sent for it.
        let fields: Vec<(String, String)> = [
            ("grant_type", "authorization_code".to_owned()),
            ("code", code.to_owned()),
            ("redirect_uri", redirect.to_owned()),
            ("client_id", client.0),
            ("client_secret", client.1),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        let answer = if method == Method::GET {
            let mut url = url::Url::parse(&endpoint).map_err(|e| e.to_string())?;
            url.query_pairs_mut()
                .extend_pairs(fields.iter().map(|(k, v)| (k, v)));
            self.http.get(url.as_str(), &self.options(&endpoint)).await
        } else {
            self.http
                .request(
                    Method::POST,
                    &endpoint,
                    Some(Body::Form(fields)),
                    &self.options(&endpoint),
                )
                .await
        };
        answer
            .and_then(|r| r.json())
            .map_err(|e| format!("code exchange: {}", e.kind()))
    }

    /// Facebook (D12): the user token checked with `debug_token` under the
    /// app token; the app, validity and user id, then the token is dropped.
    async fn facebook_user(&self, access: &str) -> Result<String, String> {
        let fb = self.file.facebook.as_ref().ok_or("no Facebook")?;
        let mut url =
            url::Url::parse(&format!("{}/debug_token", fb.graph)).map_err(|e| e.to_string())?;
        url.query_pairs_mut()
            .append_pair("input_token", access)
            .append_pair("access_token", &format!("{}|{}", fb.app_id, fb.app_secret));
        let answer: Value = self
            .http
            .get(url.as_str(), &self.options(&fb.graph))
            .await
            .and_then(|r| r.json())
            .map_err(|e| format!("debug_token: {}", e.kind()))?;
        let data = answer.get("data").ok_or("debug_token: no data")?;
        let app_ok = data.get("app_id").and_then(Value::as_str) == Some(fb.app_id.as_str());
        let valid = data.get("is_valid").and_then(Value::as_bool) == Some(true);
        match data.get("user_id").and_then(Value::as_str) {
            Some(user) if app_ok && valid && !user.is_empty() => Ok(user.to_owned()),
            _ => Err("debug_token: not this app's valid user".into()),
        }
    }

    /// Apple revocation: the tokens a fresh code gives are revoked at once
    /// and dropped.
    async fn apple_revoke(&self, client_id: &str, tokens: &Value) -> Result<(), String> {
        let disc = self.discover(Kind::Apple).await?;
        let endpoint = disc.revocation_endpoint.ok_or("no revocation endpoint")?;
        let (token, hint) = match tokens.get("refresh_token").and_then(Value::as_str) {
            Some(t) => (t, "refresh_token"),
            None => (
                tokens
                    .get("access_token")
                    .and_then(Value::as_str)
                    .ok_or("no token to revoke")?,
                "access_token",
            ),
        };
        let fields = vec![
            ("client_id".to_owned(), client_id.to_owned()),
            ("client_secret".to_owned(), self.apple_secret(client_id)?),
            ("token".to_owned(), token.to_owned()),
            ("token_type_hint".to_owned(), hint.to_owned()),
        ];
        self.http
            .request(
                Method::POST,
                &endpoint,
                Some(Body::Form(fields)),
                &self.options(&endpoint),
            )
            .await
            .map(drop)
            .map_err(|e| format!("revoke: {}", e.kind()))
    }
}

// ---- answers ----

/// An error the apps act on: words and a code.
pub(crate) fn coded(status: StatusCode, code: &str, words: &str) -> Response {
    (status, Json(json!({ "error": words, "code": code }))).into_response()
}

/// A sign-in budget spent: the apps say to try again in a minute.
fn busy() -> Response {
    coded(
        StatusCode::TOO_MANY_REQUESTS,
        "busy",
        "too many sign-in requests; try again in a minute",
    )
}

/// A flow that is over, or not ours to finish (the user may have
/// abandoned it, or it expired long ago).
fn refused() -> Response {
    error(StatusCode::NOT_FOUND, "no such sign-in")
}

/// The browser reached a sign-in that is not open (finished already, or
/// unknown here): a page that sends the user back, not a bare error.
fn closed_page() -> Response {
    const PAGE: &str = "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\">\
        <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
        <title>Sign-in closed</title>\
        <body style=\"margin:0;min-height:100vh;display:grid;place-items:center;\
        background:#000;color:#f5f5f7;font:17px/1.5 system-ui,sans-serif;text-align:center\">\
        <main style=\"padding:24px;max-width:28em\"><h1 style=\"font-size:28px;font-weight:600\">\
        This sign-in is no longer open</h1><p>Go back to the app and sign in again from there.</p>\
        </main></body></html>";
    (
        StatusCode::BAD_REQUEST,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        PAGE,
    )
        .into_response()
}

fn is_open(row: &FlowRow, now: u64) -> bool {
    (row.created_ms as u64) + FLOW_MS > now
}

/// Open flows past their time are emptied (their client address, code and
/// result go) and kept `EXPIRED_KEPT_MS` more to say so; then they go.
fn sweep_flows(db: &Connection, now: u64) -> rusqlite::Result<()> {
    db.execute(
        "DELETE FROM auth_flow WHERE created_ms <= ?1",
        params![now.saturating_sub(FLOW_MS + EXPIRED_KEPT_MS) as i64],
    )?;
    db.execute(
        "UPDATE auth_flow SET client = '', code_hash = NULL, verified = NULL
         WHERE created_ms <= ?1 AND client <> ''",
        params![now.saturating_sub(FLOW_MS) as i64],
    )?;
    Ok(())
}

fn identity(state: &AppState) -> Result<&Identity, Response> {
    state
        .identity
        .as_deref()
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "this relay has no sign-in"))
}

/// CORS for the three routes the link host's pages call: that origin
/// exactly, nothing for anyone else.
fn cors(state: &AppState, headers: &HeaderMap, mut response: Response) -> Response {
    let allowed = state
        .identity
        .as_deref()
        .and_then(Identity::link_host)
        .map(|host| format!("https://{host}"));
    if let (Some(allowed), Some(origin)) = (allowed, headers.get(header::ORIGIN))
        && origin.as_bytes() == allowed.as_bytes()
    {
        let h = response.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
        h.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    response
}

/// The preflight of those three routes.
pub(crate) async fn preflight(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let mut response = cors(&state, &headers, StatusCode::NO_CONTENT.into_response());
    if response
        .headers()
        .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    {
        let h = response.headers_mut();
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("POST"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type"),
        );
        h.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
    }
    response
}

fn sha(text: &str) -> Vec<u8> {
    token_hash(text)
}

fn fresh() -> String {
    B64URL.encode(random::<32>())
}

/// Only the app's own scheme, a loopback `/auth` page (RFC 8252 7.3: the
/// web app in development, e2e and the desktop shells) and the link host's
/// deletion page take a brokered result.
fn allowed_return(identity: &Identity, to: &str) -> bool {
    if to == "marquee://auth" {
        return true;
    }
    let Ok(url) = url::Url::parse(to) else {
        return false;
    };
    let plain = url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    let loopback = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.path() == "/auth";
    let page = identity.link_host().is_some_and(|host| {
        url.scheme() == "https"
            && url.host_str() == Some(host)
            && url.port().is_none()
            && url.path() == "/delete"
    });
    plain && (loopback || page)
}

/// The account an identity links to.
pub(crate) fn identity_link(
    db: &Connection,
    provider: &str,
    subject: &[u8],
) -> rusqlite::Result<Option<(i64, String)>> {
    db.query_row(
        "SELECT link, account FROM identity WHERE provider = ?1 AND subject = ?2",
        params![provider, subject],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

// ---- flows ----

#[derive(Deserialize)]
struct FlowRow {
    provider: String,
    purpose: String,
    verifier_hash: Vec<u8>,
    nonce: String,
    return_to: Option<String>,
    created_ms: i64,
    code_hash: Option<Vec<u8>>,
    verified: Option<String>,
}

fn flow_row(db: &Connection, flow: &str) -> rusqlite::Result<Option<FlowRow>> {
    db.query_row(
        "SELECT provider, purpose, verifier_hash, nonce, return_to, created_ms, code_hash, verified
         FROM auth_flow WHERE id_hash = ?1",
        params![sha(flow)],
        |r| {
            Ok(FlowRow {
                provider: r.get(0)?,
                purpose: r.get(1)?,
                verifier_hash: r.get(2)?,
                nonce: r.get(3)?,
                return_to: r.get(4)?,
                created_ms: r.get(5)?,
                code_hash: r.get(6)?,
                verified: r.get(7)?,
            })
        },
    )
    .optional()
}

/// Opens a flow. Brokered: the provider page; native: the relay's nonce.
pub(crate) async fn start(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    UrlPath(provider): UrlPath<String>,
    headers: HeaderMap,
    Json(request): Json<StartRequest>,
) -> Response {
    let answer = start_flow(&state, peer, &provider, &headers, request).await;
    cors(&state, &headers, answer.unwrap_or_else(|r| r))
}

async fn start_flow(
    state: &AppState,
    peer: SocketAddr,
    provider: &str,
    headers: &HeaderMap,
    request: StartRequest,
) -> Result<Response, Response> {
    let id = identity(state)?;
    let address = client(state, peer, headers);
    if limited(state, address, AUTH) {
        return Err(busy());
    }
    let kind = id
        .kind(provider)
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "no such provider"))?;
    let revoke = match request.purpose.as_str() {
        "signin" => false,
        "revoke" if kind == Kind::Apple => true,
        _ => {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "purpose is signin, or revoke for Apple",
            ));
        }
    };
    let verifier_hash = hex::decode(&request.verifier_hash)
        .ok()
        .filter(|h| h.len() == 32)
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "verifierHash is 32 bytes of hex"))?;
    let return_to = match (request.native, request.return_to) {
        (true, None) if kind != Kind::Facebook => None,
        (true, _) => {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "a native flow has no return",
            ));
        }
        (false, Some(to)) if allowed_return(id, &to) => Some(to),
        (false, _) => return Err(error(StatusCode::BAD_REQUEST, "this return is not allowed")),
    };
    let (flow, nonce) = (fresh(), fresh());
    let nonce_bytes = B64URL.decode(&nonce).expect("just encoded");
    let url = match return_to {
        Some(_) => Some(
            id.authorize_url(
                kind,
                provider,
                &flow,
                &nonce_for(&nonce_bytes, &id.public_host),
            )
            .await
            .map_err(|e| {
                tracing::warn!("sign-in start: {e}");
                error(StatusCode::BAD_GATEWAY, "the provider did not answer")
            })?,
        ),
        None => None,
    };
    let now = state.now_ms();
    let db = lock(state);
    let open = (|| -> rusqlite::Result<(i64, i64)> {
        sweep_flows(&db, now)?;
        let all = db.query_row(
            "SELECT COUNT(*) FROM auth_flow WHERE client <> ''",
            [],
            |r| r.get(0),
        )?;
        let mine = db.query_row(
            "SELECT COUNT(*) FROM auth_flow WHERE client = ?1",
            params![address.to_string()],
            |r| r.get(0),
        )?;
        Ok((all, mine))
    })()
    .map_err(|e| storage_error("start sign-in", e))?;
    if open.0 >= MAX_FLOWS || open.1 >= FLOWS_PER_CLIENT {
        return Err(busy());
    }
    db.execute(
        "INSERT INTO auth_flow (id_hash, provider, purpose, verifier_hash, nonce, return_to, client, created_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            sha(&flow),
            provider,
            if revoke { "revoke" } else { "signin" },
            verifier_hash,
            nonce,
            return_to,
            address.to_string(),
            now as i64
        ],
    )
    .map_err(|e| storage_error("start sign-in", e))?;
    let native = url.is_none();
    Ok(Json(StartResponse {
        flow,
        expires_ms: now + FLOW_MS,
        url,
        nonce: native.then_some(nonce),
    })
    .into_response())
}

#[derive(Deserialize)]
pub(crate) struct Returned {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub(crate) async fn callback_get(
    State(state): State<AppState>,
    UrlPath(provider): UrlPath<String>,
    Query(back): Query<Returned>,
) -> Response {
    callback(&state, &provider, back).await
}

/// Apple returns by POST (`form_post`).
pub(crate) async fn callback_post(
    State(state): State<AppState>,
    UrlPath(provider): UrlPath<String>,
    Form(back): Form<Returned>,
) -> Response {
    callback(&state, &provider, back).await
}

fn redirect(to: &str) -> Response {
    (StatusCode::SEE_OTHER, [(header::LOCATION, to.to_owned())]).into_response()
}

/// The provider sent the user back: verify, keep the result under a
/// one-time code, and hand that code to the starting app in the fragment
/// (no log or `Referer` carries a fragment), with this relay's name.
async fn callback(state: &AppState, provider: &str, back: Returned) -> Response {
    let Ok(id) = identity(state) else {
        return error(StatusCode::NOT_FOUND, "this relay has no sign-in");
    };
    let Some(flow) = back.state else {
        return closed_page();
    };
    let now = state.now_ms();
    let row = {
        let db = lock(state);
        flow_row(&db, &flow)
    };
    let row = match row {
        Ok(Some(row)) if row.provider == provider && row.return_to.is_some() => row,
        Ok(_) => return closed_page(),
        Err(e) => return storage_error("sign-in callback", e),
    };
    let to = row.return_to.clone().unwrap_or_default();
    let fragment =
        |rest: &str| redirect(&format!("{to}#flow={flow}&{rest}&relay={}", id.public_host));
    // Past its time: the app hears it and says to sign in again.
    if !is_open(&row, now) {
        return fragment("error=expired");
    }
    // A callback completes a flow once; a second one (a reload) has no
    // return to make, the first already went.
    if row.code_hash.is_some() {
        return closed_page();
    }
    let failed = |why: &str| {
        let db = lock(state);
        let _ = db.execute(
            "DELETE FROM auth_flow WHERE id_hash = ?1 AND code_hash IS NULL",
            params![sha(&flow)],
        );
        fragment(&format!("error={why}"))
    };
    let (Some(code), None) = (back.code, back.error) else {
        return failed("cancelled");
    };
    // The provider left the identity file since the flow began.
    let Some(kind) = id.kind(provider) else {
        return failed("refused");
    };
    let verified = verify_code(id, kind, provider, &row, &code, now).await;
    let (name, sub, tokens) = match verified {
        Ok(v) => v,
        Err(why) => {
            tracing::warn!("sign-in refused: {why}");
            return failed("refused");
        }
    };
    let subject = id.subject(&name, &sub);
    let receipt = if row.purpose == "revoke" {
        let services = id
            .file
            .apple
            .as_ref()
            .map(|a| a.services_id.clone())
            .unwrap_or_default();
        match id.apple_revoke(&services, &tokens).await {
            Ok(()) => Some(fresh()),
            Err(why) => {
                tracing::warn!("Apple revoke: {why}");
                return failed("revoke");
            }
        }
    } else {
        None
    };
    let one_time = fresh();
    let db = lock(state);
    let stored = (|| -> rusqlite::Result<bool> {
        if let Some(receipt) = &receipt {
            keep_receipt(&db, &name, &subject, receipt, now)?;
        }
        let verified = Verified {
            provider: name.clone(),
            subject: hex::encode(&subject),
            receipt: receipt.clone(),
        };
        // Two callbacks raced over the provider exchange: the first stores.
        let stored = db.execute(
            "UPDATE auth_flow SET code_hash = ?2, verified = ?3
             WHERE id_hash = ?1 AND code_hash IS NULL AND client <> ''",
            params![
                sha(&flow),
                sha(&one_time),
                serde_json::to_string(&verified).expect("plain")
            ],
        )?;
        Ok(stored == 1)
    })();
    match stored {
        Ok(true) => fragment(&format!("code={one_time}")),
        Ok(false) => closed_page(),
        Err(e) => storage_error("sign-in callback", e),
    }
}

/// The code of a brokered flow, exchanged and checked: the provider name a
/// subject is hashed under, the subject, and the token answer (for a revoke).
async fn verify_code(
    id: &Identity,
    kind: Kind,
    provider: &str,
    row: &FlowRow,
    code: &str,
    now: u64,
) -> Result<(String, String, Value), String> {
    let client = id.brokered_client(kind)?;
    let audience = vec![client.0.clone()];
    let tokens = id
        .exchange(kind, client, code, &id.callback(provider))
        .await?;
    let name = id.provider_name(kind);
    let sub = match kind {
        Kind::Facebook => {
            let access = tokens
                .get("access_token")
                .and_then(Value::as_str)
                .ok_or("no access token")?;
            id.facebook_user(access).await?
        }
        _ => {
            let token = tokens
                .get("id_token")
                .and_then(Value::as_str)
                .ok_or("no ID token")?;
            let nonce = B64URL.decode(&row.nonce).map_err(|_| "bad nonce")?;
            id.verify(
                kind,
                token,
                &audience,
                &nonce_for(&nonce, &id.public_host),
                now,
            )
            .await?
        }
    };
    Ok((name, sub, tokens))
}

/// A revoke's receipt, for the link of that identity (if one exists).
fn keep_receipt(
    db: &Connection,
    provider: &str,
    subject: &[u8],
    receipt: &str,
    now: u64,
) -> rusqlite::Result<()> {
    let link = identity_link(db, provider, subject)?.map(|(link, _)| link);
    db.execute(
        "INSERT INTO receipt (hash, link, expires_ms) VALUES (?1, ?2, ?3)",
        params![sha(receipt), link, (now + RECEIPT_MS) as i64],
    )?;
    Ok(())
}

/// A ticket for `provider`'s `subject`, and the account it links to.
fn mint_ticket(
    db: &Connection,
    provider: &str,
    subject: &[u8],
    receipt: Option<String>,
    now: u64,
) -> rusqlite::Result<Taken> {
    let ticket = fresh();
    db.execute(
        "INSERT INTO ticket (hash, provider, subject, expires_ms) VALUES (?1, ?2, ?3, ?4)",
        params![sha(&ticket), provider, subject, (now + TICKET_MS) as i64],
    )?;
    let account = identity_link(db, provider, subject)?.and_then(|(_, a)| crate::account_id(&a));
    Ok(Taken {
        ticket,
        account,
        receipt,
    })
}

/// Takes a brokered flow's result, once: the starter's verifier and the
/// code the return carried. A browser that only finished the provider
/// step, without the verifier, gets nothing; nor does the starter without
/// the code.
pub(crate) async fn take(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<TakeRequest>,
) -> Response {
    let answer = (|| {
        identity(&state)?;
        if limited(&state, client(&state, peer, &headers), AUTH) {
            return Err(busy());
        }
        let now = state.now_ms();
        let db = lock(&state);
        let row = flow_row(&db, &request.flow)
            .map_err(|e| storage_error("take sign-in", e))?
            .ok_or_else(refused)?;
        let mine = same(&row.verifier_hash, &sha(&request.verifier));
        if mine && !is_open(&row, now) {
            return Err(ticket_refused());
        }
        let fits = mine
            && row
                .code_hash
                .as_deref()
                .is_some_and(|c| same(c, &sha(&request.code)));
        let verified: Verified = row
            .verified
            .as_deref()
            .and_then(|v| serde_json::from_str(v).ok())
            .filter(|_| fits)
            .ok_or_else(refused)?;
        let subject = hex::decode(&verified.subject).map_err(|_| refused())?;
        let taken = (|| {
            db.execute(
                "DELETE FROM auth_flow WHERE id_hash = ?1",
                params![sha(&request.flow)],
            )?;
            mint_ticket(&db, &verified.provider, &subject, verified.receipt, now)
        })()
        .map_err(|e| storage_error("take sign-in", e))?;
        Ok(Json(taken).into_response())
    })();
    cors(&state, &headers, answer.unwrap_or_else(|r| r))
}

/// A native sheet's ID token: audience one of the apps', nonce bound to the
/// flow and to this relay's host. A revoke also exchanges Apple's fresh
/// code (with the bundle id the token names) and revokes at once.
pub(crate) async fn native(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<NativeToken>,
) -> Response {
    let id = match identity(&state) {
        Ok(id) => id,
        Err(r) => return r,
    };
    if limited(&state, client(&state, peer, &headers), AUTH) {
        return busy();
    }
    let now = state.now_ms();
    let row = match flow_row(&lock(&state), &request.flow) {
        Ok(Some(row))
            if row.return_to.is_none() && same(&row.verifier_hash, &sha(&request.verifier)) =>
        {
            row
        }
        Ok(_) => return refused(),
        Err(e) => return storage_error("native sign-in", e),
    };
    if !is_open(&row, now) {
        return ticket_refused();
    }
    // The provider left the identity file since the flow began.
    let Some(kind) = id.kind(&row.provider) else {
        return refused();
    };
    let Ok(nonce) = B64URL.decode(&row.nonce) else {
        return refused();
    };
    let audiences = id.native_audiences(kind);
    let sub = match id
        .verify(
            kind,
            &request.id_token,
            &audiences,
            &nonce_for(&nonce, &id.public_host),
            now,
        )
        .await
    {
        Ok(sub) => sub,
        Err(why) => {
            tracing::warn!("native sign-in refused: {why}");
            return error(StatusCode::FORBIDDEN, "the sign-in did not check");
        }
    };
    let name = id.provider_name(kind);
    let subject = id.subject(&name, &sub);
    let receipt = match (row.purpose.as_str(), &request.authorization_code) {
        ("revoke", Some(code)) => {
            // The bundle id the token was issued to is the code's client.
            let client_id = Jwt::parse(&request.id_token)
                .and_then(|t| {
                    t.claims()
                        .get("aud")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            let exchanged = async {
                let secret = id.apple_secret(&client_id)?;
                id.exchange(kind, (client_id.clone(), secret), code, "")
                    .await
            };
            let tokens = match exchanged.await {
                Ok(tokens) => tokens,
                Err(why) => {
                    tracing::warn!("Apple revoke: {why}");
                    return error(StatusCode::BAD_GATEWAY, "Apple did not take the revocation");
                }
            };
            // The code must be the signed-in identity's: Apple's answer
            // (over TLS, from the token endpoint) names whose it is.
            let whose = tokens
                .get("id_token")
                .and_then(Value::as_str)
                .and_then(Jwt::parse)
                .and_then(|t| {
                    t.claims()
                        .get("sub")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            if whose.as_deref() != Some(sub.as_str()) {
                tracing::warn!("Apple revoke: the code is another identity's");
                return error(StatusCode::FORBIDDEN, "the sign-in did not check");
            }
            match id.apple_revoke(&client_id, &tokens).await {
                Ok(()) => Some(fresh()),
                Err(why) => {
                    tracing::warn!("Apple revoke: {why}");
                    return error(StatusCode::BAD_GATEWAY, "Apple did not take the revocation");
                }
            }
        }
        ("revoke", None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "a revoke needs Apple's authorization code",
            );
        }
        _ => None,
    };
    let db = lock(&state);
    let taken = (|| {
        db.execute(
            "DELETE FROM auth_flow WHERE id_hash = ?1",
            params![sha(&request.flow)],
        )?;
        if let Some(receipt) = &receipt {
            keep_receipt(&db, &name, &subject, receipt, now)?;
        }
        mint_ticket(&db, &name, &subject, receipt.clone(), now)
    })();
    match taken {
        Ok(taken) => Json(taken).into_response(),
        Err(e) => storage_error("native sign-in", e),
    }
}

// ---- tickets, links ----

/// The identity a ticket stands for, still valid. `consume` uses it up.
pub(crate) fn ticket(
    db: &Connection,
    ticket: &str,
    now: u64,
    consume: bool,
) -> rusqlite::Result<Option<(String, Vec<u8>)>> {
    let found = db
        .query_row(
            "SELECT provider, subject FROM ticket WHERE hash = ?1 AND expires_ms > ?2",
            params![sha(ticket), now as i64],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if consume && found.is_some() {
        db.execute("DELETE FROM ticket WHERE hash = ?1", params![sha(ticket)])?;
    }
    Ok(found)
}

pub(crate) fn ticket_refused() -> Response {
    coded(
        StatusCode::FORBIDDEN,
        "ticketExpired",
        "sign in again to finish",
    )
}

pub(crate) fn linked_elsewhere() -> Response {
    coded(
        StatusCode::CONFLICT,
        "linkedElsewhere",
        "this identity already signs in to another account",
    )
}

/// Sign-ups of this identity in the window.
pub(crate) fn signups(db: &Connection, subject: &[u8], now: u64) -> rusqlite::Result<i64> {
    db.query_row(
        "SELECT COUNT(*) FROM signups WHERE subject = ?1 AND at_ms > ?2",
        params![subject, now.saturating_sub(SIGNUP_WINDOW_MS) as i64],
        |r| r.get(0),
    )
}

/// The account's links.
pub(crate) async fn links(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    let rows = (|| -> rusqlite::Result<Vec<IdentityLink>> {
        let mut stmt = db.prepare(
            "SELECT link, provider, created_ms FROM identity WHERE account = ?1 ORDER BY link",
        )?;
        stmt.query_map(params![hex::encode(auth.account.0)], |r| {
            Ok(IdentityLink {
                link: r.get(0)?,
                provider: r.get(1)?,
                created_ms: r.get::<_, i64>(2)? as u64,
            })
        })?
        .collect()
    })();
    match rows {
        Ok(mut links) => {
            if let Some(id) = state.identity.as_deref() {
                for link in &mut links {
                    link.provider = id.display(&link.provider);
                }
            }
            Json(json!({ "links": links })).into_response()
        }
        Err(e) => storage_error("links", e),
    }
}

/// Links a ticket's identity to the token's account. Linked elsewhere: 409,
/// the ticket kept, unless `move` takes it over (no sign-up counted), so a
/// user can take an identity back after someone else's `move`.
pub(crate) async fn link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LinkRequest>,
) -> Response {
    let now = state.now_ms();
    let mut db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    let mine = hex::encode(auth.account.0);
    let done = (|| -> rusqlite::Result<Result<(), Response>> {
        let tx = db.transaction()?;
        let Some((provider, subject)) = ticket(&tx, &request.ticket, now, false)? else {
            return Ok(Err(ticket_refused()));
        };
        match identity_link(&tx, &provider, &subject)? {
            Some((_, account)) if account == mine => {}
            Some(_) if !request.move_link => return Ok(Err(linked_elsewhere())),
            Some((link, _)) => {
                tx.execute(
                    "UPDATE identity SET account = ?2, created_ms = ?3 WHERE link = ?1",
                    params![link, mine, now as i64],
                )?;
                tx.execute(
                    "UPDATE deletion SET link = NULL WHERE link = ?1",
                    params![link],
                )?;
            }
            None => {
                tx.execute(
                    "INSERT INTO identity (provider, subject, account, created_ms) VALUES (?1, ?2, ?3, ?4)",
                    params![provider, subject, mine, now as i64],
                )?;
            }
        }
        ticket(&tx, &request.ticket, now, true)?;
        tx.commit()?;
        Ok(Ok(()))
    })();
    match done {
        Ok(Ok(())) => Json(json!({})).into_response(),
        Ok(Err(refused)) => refused,
        Err(e) => storage_error("link", e),
    }
}

/// Unlinks by link id; a link this account does not hold is already gone.
/// A receipt is optional: when it is sent it must be this link's; an Apple
/// link unlinked without one is logged (its revoke failed or could not
/// run, and the user chose to go on).
pub(crate) async fn unlink(
    State(state): State<AppState>,
    UrlPath(link): UrlPath<i64>,
    headers: HeaderMap,
    body: Option<Json<UnlinkRequest>>,
) -> Response {
    let receipt = body.and_then(|Json(b)| b.receipt);
    let now = state.now_ms();
    let mut db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    let done = (|| -> rusqlite::Result<Result<(), Response>> {
        let tx = db.transaction()?;
        let provider: Option<String> = tx
            .query_row(
                "SELECT provider FROM identity WHERE link = ?1 AND account = ?2",
                params![link, hex::encode(auth.account.0)],
                |r| r.get(0),
            )
            .optional()?;
        // Not this account's (any more): nothing to do, as a retry after a
        // lost answer expects.
        let Some(provider) = provider else {
            return Ok(Ok(()));
        };
        if let Some(refused) = check_receipt(&tx, receipt.as_deref(), &[link], now)? {
            return Ok(Err(refused));
        }
        if receipt.is_none() && provider == "apple" {
            tracing::info!("an Apple link was unlinked without a revoke receipt");
        }
        tx.execute("DELETE FROM identity WHERE link = ?1", params![link])?;
        tx.execute(
            "UPDATE deletion SET link = NULL WHERE link = ?1",
            params![link],
        )?;
        tx.commit()?;
        Ok(Ok(()))
    })();
    match done {
        Ok(Ok(())) => Json(json!({})).into_response(),
        Ok(Err(refused)) => refused,
        Err(e) => storage_error("unlink", e),
    }
}

/// A receipt sent with an unlink or a deletion: valid, and for one of
/// `links`; it is used up. None is fine (the user went on without the
/// revoke). Answers the refusal, if any.
pub(crate) fn check_receipt(
    db: &Connection,
    receipt: Option<&str>,
    links: &[i64],
    now: u64,
) -> rusqlite::Result<Option<Response>> {
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    let link: Option<Option<i64>> = db
        .query_row(
            "SELECT link FROM receipt WHERE hash = ?1 AND expires_ms > ?2",
            params![sha(receipt), now as i64],
            |r| r.get(0),
        )
        .optional()?;
    match link {
        Some(Some(link)) if links.contains(&link) => {
            db.execute("DELETE FROM receipt WHERE hash = ?1", params![sha(receipt)])?;
            Ok(None)
        }
        _ => Ok(Some(coded(
            StatusCode::FORBIDDEN,
            "receiptRefused",
            "this receipt is not for this link",
        ))),
    }
}

// ---- scheduled deletion ----

#[derive(Deserialize)]
pub(crate) struct DeletionAsk {
    ticket: String,
}

/// From the deletion page (the link host): a ticket schedules the linked
/// account's deletion in 7 days and records the link that asked. A second
/// request while one waits keeps the first date. Every device shows it.
pub(crate) async fn delete_request(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(ask): Json<DeletionAsk>,
) -> Response {
    let answer = (|| {
        identity(&state)?;
        if limited(&state, client(&state, peer, &headers), AUTH) {
            return Err(busy());
        }
        let now = state.now_ms();
        let mut db = lock(&state);
        if let Some(stopped) = crate::journal_stopped(&state) {
            return Err(stopped);
        }
        type Scheduled = (Id, u64, Option<i64>);
        let scheduled = (|| -> rusqlite::Result<Result<Scheduled, Response>> {
            let tx = db.transaction()?;
            let Some((provider, subject)) = ticket(&tx, &ask.ticket, now, true)? else {
                return Ok(Err(ticket_refused()));
            };
            let Some((link, account)) = identity_link(&tx, &provider, &subject)? else {
                return Ok(Err(error(
                    StatusCode::NOT_FOUND,
                    "this identity signs in to no account",
                )));
            };
            let due: Option<i64> = tx
                .query_row(
                    "SELECT due_ms FROM deletion WHERE account = ?1",
                    params![account],
                    |r| r.get(0),
                )
                .optional()?;
            let due = match due {
                Some(due) => due as u64,
                None => {
                    let due = now + DELETION_WAIT_MS;
                    tx.execute(
                        "INSERT INTO deletion (account, due_ms, link) VALUES (?1, ?2, ?3)",
                        params![account, due as i64, link],
                    )?;
                    due
                }
            };
            tx.commit()?;
            let account = crate::account_id(&account).expect("stored account ids are hex");
            Ok(Ok((account, due, Some(link))))
        })()
        .map_err(|e| storage_error("delete request", e))?;
        let (account, due, link) = scheduled?;
        if let Err(e) = journal_deletion(&state, &account, Some(due), link) {
            return Err(crate::journal_error(e));
        }
        Ok(Json(json!({ "dueMs": due })).into_response())
    })();
    cors(&state, &headers, answer.unwrap_or_else(|r| r))
}

/// Cancels a scheduled deletion; `unlink` also removes the link that asked,
/// so whoever holds that identity cannot simply ask again.
pub(crate) async fn cancel_deletion(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<CancelDeletion>>,
) -> Response {
    let unlink = body.is_some_and(|Json(b)| b.unlink);
    let mut db = lock(&state);
    let auth = match active(authorize(&db, &headers)) {
        Ok(auth) => auth,
        Err(refusal) => return *refusal,
    };
    if let Some(stopped) = crate::journal_stopped(&state) {
        return stopped;
    }
    let account = hex::encode(auth.account.0);
    let cancelled = (|| -> rusqlite::Result<bool> {
        let tx = db.transaction()?;
        let link: Option<Option<i64>> = tx
            .query_row(
                "SELECT link FROM deletion WHERE account = ?1",
                params![account],
                |r| r.get(0),
            )
            .optional()?;
        let Some(link) = link else {
            return Ok(false);
        };
        tx.execute("DELETE FROM deletion WHERE account = ?1", params![account])?;
        // Only this account's: a link id from a journal line can name
        // another account's link after a restore.
        if let (true, Some(link)) = (unlink, link) {
            tx.execute(
                "DELETE FROM identity WHERE link = ?1 AND account = ?2",
                params![link, account],
            )?;
        }
        tx.commit()?;
        Ok(true)
    })();
    match cancelled {
        Ok(true) => match journal_deletion(&state, &auth.account, None, None) {
            Ok(()) => Json(json!({})).into_response(),
            Err(e) => crate::journal_error(e),
        },
        Ok(false) => Json(json!({})).into_response(),
        Err(e) => storage_error("cancel deletion", e),
    }
}

// ---- Apple's server-to-server notifications ----

#[derive(Deserialize)]
pub(crate) struct Notification {
    payload: String,
}

/// `consent-revoked` and `account-deleted` drop that Apple identity's link.
/// Signed by Apple, at most a day old; idempotent. Anyone can post here, so
/// it has a budget of its own and fetches Apple's keys as rarely as tokens do.
pub(crate) async fn apple_notification(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(note): Json<Notification>,
) -> Response {
    let Ok(id) = identity(&state) else {
        return error(StatusCode::NOT_FOUND, "this relay has no sign-in");
    };
    let Some(apple) = id.file.apple.as_ref() else {
        return error(StatusCode::NOT_FOUND, "no Apple sign-in here");
    };
    if client_limited(&state, client(&state, peer, &headers), APPLE_NOTICES) {
        return busy();
    }
    let now = state.now_ms();
    let checked = async {
        let disc = id.discover(Kind::Apple).await?;
        let jwt = Jwt::parse(&note.payload).ok_or("not a JWT")?;
        let key = id.key_for(&disc.jwks_uri, jwt.kid(), now).await?;
        if !jwt.verifies(&key) {
            return Err("signature".to_owned());
        }
        // Signed by Apple: only now does it count against the relay-wide budget.
        if relay_limited(&state, APPLE_NOTICES) {
            return Err("busy".to_owned());
        }
        let claims = jwt.claims();
        let audiences: Vec<String> = std::iter::once(apple.services_id.clone())
            .chain(apple.bundle_ids.clone())
            .collect();
        let iss = claims.get("iss").and_then(Value::as_str) == Some(apple.issuer.as_str());
        let aud = claims
            .get("aud")
            .and_then(Value::as_str)
            .is_some_and(|a| audiences.iter().any(|x| x == a));
        if !(iss && aud) {
            return Err("issuer or audience".to_owned());
        }
        // Apple's time, on the machine's clock: a captured notice replayed
        // later must not drop a link made again since.
        let issued = claims.get("iat").and_then(Value::as_u64).ok_or("no iat")?;
        let age = (now_ms() / 1000).saturating_sub(issued);
        if age > NOTICE_MAX_AGE_S || issued > now_ms() / 1000 + 60 {
            return Err("too old".to_owned());
        }
        let events: Value = claims
            .get("events")
            .and_then(Value::as_str)
            .and_then(|e| serde_json::from_str(e).ok())
            .ok_or("no events")?;
        let kind = events
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let sub = events
            .get("sub")
            .and_then(Value::as_str)
            .ok_or("no subject")?
            .to_owned();
        Ok::<_, String>((kind, sub))
    };
    match checked.await {
        Ok((kind, sub)) => {
            if matches!(
                kind.as_str(),
                "consent-revoked" | "account-deleted" | "account-delete"
            ) {
                let subject = id.subject("apple", &sub);
                let db = lock(&state);
                let dropped = (|| -> rusqlite::Result<()> {
                    if let Some((link, _)) = identity_link(&db, "apple", &subject)? {
                        db.execute("DELETE FROM identity WHERE link = ?1", params![link])?;
                        db.execute(
                            "UPDATE deletion SET link = NULL WHERE link = ?1",
                            params![link],
                        )?;
                    }
                    Ok(())
                })();
                if let Err(e) = dropped {
                    return storage_error("Apple notification", e);
                }
            }
            Json(json!({})).into_response()
        }
        Err(why) if why == "busy" => busy(),
        Err(why) => {
            tracing::warn!("Apple notification refused: {why}");
            error(StatusCode::BAD_REQUEST, "not a notification from Apple")
        }
    }
}

/// Sign-in rows past their time go.
pub(crate) fn expire(db: &Connection, now: u64) -> rusqlite::Result<()> {
    sweep_flows(db, now)?;
    db.execute(
        "DELETE FROM ticket WHERE expires_ms <= ?1",
        params![now as i64],
    )?;
    db.execute(
        "DELETE FROM receipt WHERE expires_ms <= ?1",
        params![now as i64],
    )?;
    db.execute(
        "DELETE FROM signups WHERE at_ms <= ?1",
        params![now.saturating_sub(SIGNUP_WINDOW_MS) as i64],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_public_issuer_names_only_public_https_endpoints() {
        let public = "https://accounts.example.com";
        assert!(discovered(public, "https://keys.example.com/jwks").is_ok());
        for inward in [
            "http://keys.example.com/jwks",
            "https://169.254.169.254/latest",
            "https://127.0.0.1/jwks",
            "http://10.0.0.5/token",
        ] {
            assert!(discovered(public, inward).is_err(), "{inward}");
        }
        // The mock on loopback may name its own plain http endpoints.
        assert!(discovered("http://127.0.0.1:8790", "http://127.0.0.1:8790/jwks").is_ok());
    }

    #[test]
    fn providers_read_as_their_route_ids() {
        let file: IdentityFile = serde_json::from_value(json!({
            "key": "1".repeat(64),
            "oidc": [{ "id": "corp", "issuer": "https://id.corp.example", "clientId": "c", "clientSecret": "s" }],
        }))
        .unwrap();
        let id = Identity::new(file, "relay.example").unwrap();
        assert_eq!(id.display("oidc:https://id.corp.example"), "corp");
        assert_eq!(id.display("oidc:https://gone.example"), "oidc");
        assert_eq!(id.display("apple"), "apple");
    }
}
