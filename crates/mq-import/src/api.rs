//! Stremio account API: device-code linking and the three reads the import
//! needs. Request and response shapes follow stremio-core (MIT) `types::api`.

use crate::ImportError;
use crate::map::Snapshot;
use mq_http::{HttpClient, RequestOptions, is_private_host};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt;
use std::time::{Duration, Instant};
use stremio_core::types::api::{APIResult, LinkAuthKey};
use url::Url;

/// `getUser`, `addonCollectionGet` and `datastoreGet` answer a bad session
/// with this code (`Session does not exist`).
const CODE_SESSION: u64 = 1;
/// `link.stremio.com` answers `read` with this code both while the code is
/// still waiting for a login and when it is unknown or expired.
const CODE_LINK_NOT_READY: u64 = 101;

/// Library of a large account can run to tens of MB.
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

/// A Stremio session key. It never prints, never serializes, and leaves this
/// crate only through [`AuthKey::expose`], which the caller hands to its
/// secret store (keychain, keystore). This crate never writes it anywhere.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthKey(String);

impl AuthKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AuthKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthKey(<redacted>)")
    }
}

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub api: String,
    pub link: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            api: "https://api.strem.io".into(),
            link: "https://link.stremio.com".into(),
        }
    }
}

/// Private addresses are allowed only when the configured endpoint itself is
/// one (tests, a self-hosted gateway); the real endpoints are public hosts.
fn opts(base: &str, timeout: Duration) -> RequestOptions {
    let allow_private = Url::parse(base)
        .ok()
        .and_then(|u| u.host_str().map(is_private_host))
        .unwrap_or(false);
    RequestOptions {
        timeout,
        // A large library's answer may start only after a long wait.
        hop_headers: timeout,
        max_bytes: MAX_RESPONSE_BYTES,
        allow_private,
        ..Default::default()
    }
}

/// Unwraps the `{"result": ..}` / `{"error": {code, message}}` envelope.
async fn call(
    http: &HttpClient,
    url: &str,
    body: Option<&Value>,
    options: &RequestOptions,
) -> Result<Value, ImportError> {
    let response = match body {
        Some(body) => http.post_json(url, body, options).await?,
        None => http.get(url, options).await?,
    };
    match response.json::<APIResult<Value>>()? {
        APIResult::Ok(value) => Ok(value),
        APIResult::Err(e) if e.code == CODE_SESSION => Err(ImportError::InvalidAuthKey),
        APIResult::Err(e) => Err(ImportError::Api {
            code: e.code,
            message: e.message,
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkPoll {
    Pending,
    Authorized { auth_key: AuthKey },
    Expired,
}

#[derive(Deserialize)]
struct LinkCode {
    code: String,
    link: Option<String>,
    qrcode: Option<String>,
}

/// Device-code login through `link.stremio.com`: the app shows `code` (and
/// `qr_url`), the user enters it at `link_url` while signed in to Stremio, and
/// `poll` returns the session key. No password ever reaches Marquee.
pub struct LinkLogin {
    pub code: String,
    pub link_url: String,
    pub qr_url: Option<String>,
    /// The service answers an unknown code and a not-yet-linked code with the
    /// same error, so expiry is decided locally. The service does not publish
    /// a lifetime; 10 minutes is the working default.
    pub ttl: Duration,
    http: HttpClient,
    endpoints: Endpoints,
    started: Instant,
}

impl LinkLogin {
    pub async fn start(http: HttpClient, endpoints: Endpoints) -> Result<Self, ImportError> {
        let url = format!("{}/api/v2/create?type=Create", endpoints.link);
        let options = opts(&endpoints.link, Duration::from_secs(10));
        let created = call(&http, &url, None, &options).await?;
        let created: LinkCode = serde_json::from_value(created)
            .map_err(|e| ImportError::Decode(format!("link code: {e}")))?;
        let link_url = created
            .link
            .unwrap_or_else(|| format!("{}/{}", endpoints.link, created.code));
        Ok(Self {
            code: created.code,
            link_url,
            qr_url: created.qrcode,
            ttl: Duration::from_secs(600),
            http,
            endpoints,
            started: Instant::now(),
        })
    }

    /// One `read`. Network failures are errors the caller can retry on the
    /// next tick; `Expired` is final.
    pub async fn poll(&self) -> Result<LinkPoll, ImportError> {
        let mut url = Url::parse(&format!("{}/api/v2/read", self.endpoints.link))
            .map_err(|_| ImportError::Decode("link endpoint".into()))?;
        url.query_pairs_mut()
            .append_pair("type", "Read")
            .append_pair("code", &self.code);
        let options = opts(&self.endpoints.link, Duration::from_secs(10));
        match call(&self.http, url.as_str(), None, &options).await {
            Ok(value) => {
                let key: LinkAuthKey = serde_json::from_value(value)
                    .map_err(|e| ImportError::Decode(format!("link key: {e}")))?;
                Ok(LinkPoll::Authorized {
                    auth_key: AuthKey::new(key.auth_key),
                })
            }
            Err(ImportError::Api { code, .. }) if code == CODE_LINK_NOT_READY => {
                if self.started.elapsed() >= self.ttl {
                    Ok(LinkPoll::Expired)
                } else {
                    Ok(LinkPoll::Pending)
                }
            }
            Err(e) => Err(e),
        }
    }
}

/// An authenticated Stremio account, read-only.
pub struct StremioClient {
    http: HttpClient,
    endpoints: Endpoints,
    auth_key: AuthKey,
    user: Value,
}

impl StremioClient {
    /// Verifies the key with `getUser`. Fails with `InvalidAuthKey` when the
    /// session is gone.
    pub async fn login_with_auth_key(
        http: HttpClient,
        endpoints: Endpoints,
        auth_key: AuthKey,
    ) -> Result<Self, ImportError> {
        let user = call(
            &http,
            &format!("{}/api/getUser", endpoints.api),
            Some(&json!({ "authKey": auth_key.expose() })),
            &opts(&endpoints.api, Duration::from_secs(20)),
        )
        .await?;
        Ok(Self {
            http,
            endpoints,
            auth_key,
            user: minimal_user(&user),
        })
    }

    /// Step 1 of the import: network reads only, nothing is written anywhere.
    /// `update: false` keeps this a pure read of the cloud collection.
    pub async fn fetch(&self) -> Result<Snapshot, ImportError> {
        let key = self.auth_key.expose();
        let long = opts(&self.endpoints.api, Duration::from_secs(120));
        let addons = call(
            &self.http,
            &format!("{}/api/addonCollectionGet", self.endpoints.api),
            Some(&json!({ "authKey": key, "update": false })),
            &long,
        )
        .await?;
        if !addons["addons"].is_array() {
            return Err(ImportError::Decode(
                "addonCollectionGet: no addons array".into(),
            ));
        }
        let library = call(
            &self.http,
            &format!("{}/api/datastoreGet", self.endpoints.api),
            Some(&json!({
                "authKey": key,
                "collection": "libraryItem",
                "ids": [],
                "all": true,
            })),
            &long,
        )
        .await?;
        let Value::Array(library) = library else {
            return Err(ImportError::Decode(
                "datastoreGet: expected an array".into(),
            ));
        };
        Ok(Snapshot {
            version: 1,
            user: self.user.clone(),
            addon_collection: addons,
            library,
        })
    }
}

/// The raw import is kept on disk, so it carries only what the mapping and
/// the report need: no email, no Trakt token.
fn minimal_user(user: &Value) -> Value {
    json!({
        "_id": user.get("_id"),
        "traktLinked": user.get("trakt").is_some_and(|t| !t.is_null()),
    })
}
