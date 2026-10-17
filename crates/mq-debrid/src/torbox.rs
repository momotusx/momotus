//! TorBox, API v1 (https://api-docs.torbox.app/).
//!
//! Every response is `{success, error, detail, data}`. All files of a torrent
//! download together, so `select_files` is a no-op. `requestdl` takes the key
//! as a `token` query parameter; mq-http redacts query values in logs.

use crate::api::{Api, Auth, Secret, authed, bearer, fixed, with_query};
use crate::login::{Code, Credential, LoginPoll};
use crate::util::{magnet_uri, normalize_hash, parse_utc, unix_now};
use crate::{
    CacheStatus, Capabilities, DebridError, ResolvedLink, Store, StoreId, StoreUser, TorrentFile,
    TorrentHandle, TorrentInfo, TorrentStatus,
};
use mq_http::{Body, HttpClient, HttpError, Method, Priority};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

pub(crate) const BASE: &str = "https://api.torbox.app";
const CHECK_CHUNK: usize = 100;
/// The docs say both 1 and 3 hours; take the shorter.
const LINK_VALID_SECS: u64 = 3600;

#[derive(Clone)]
pub struct TorBox {
    api: Api,
    auth: Arc<dyn Auth>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    success: bool,
    error: Option<String>,
    detail: Option<String>,
    data: Option<T>,
}

impl TorBox {
    pub fn new(http: HttpClient, api_key: impl Into<String>) -> Self {
        Self::with_auth(http, fixed(api_key))
    }

    /// A store that reads its key from `auth` and tells it about a refusal.
    pub fn with_auth(http: HttpClient, auth: Arc<dyn Auth>) -> Self {
        Self {
            api: Api::new(http, BASE),
            auth,
        }
    }

    /// Another base URL (tests, a self-hosted proxy).
    pub fn with_base_url(mut self, base: &str) -> Self {
        self.api.set_base(base);
        self
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.api.set_priority(priority);
        self
    }

    /// One request with `key`, the envelope unwrapped and `success: false`
    /// turned into an error.
    async fn call_as<T: DeserializeOwned>(
        &self,
        key: &Secret,
        method: Method,
        path: &str,
        body: Option<Body>,
    ) -> Result<Option<T>, DebridError> {
        let envelope: Envelope<T> = self
            .api
            .call(method, &format!("/v1/api{path}"), body, bearer(key))
            .await
            .map_err(map_error)?
            .json()?;
        if envelope.success {
            Ok(envelope.data)
        } else {
            Err(envelope_error(
                envelope.error.as_deref(),
                envelope.detail.unwrap_or_default(),
            ))
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Body>,
    ) -> Result<Option<T>, DebridError> {
        authed(self.auth.as_ref(), |key| {
            let (method, body) = (method.clone(), body.clone());
            async move { self.call_as(&key, method, path, body).await }
        })
        .await
    }
}

fn envelope_error(code: Option<&str>, detail: String) -> DebridError {
    match code.unwrap_or("UNKNOWN_ERROR") {
        "NO_AUTH" | "BAD_TOKEN" | "AUTH_ERROR" => DebridError::Auth,
        "PLAN_RESTRICTED_FEATURE" => DebridError::NotPremium,
        "MONTHLY_LIMIT" | "ACTIVE_LIMIT" => DebridError::Quota,
        "COOLDOWN_LIMIT" => DebridError::RateLimited { retry_after: None },
        other => DebridError::provider(other, detail),
    }
}

fn map_error(e: HttpError) -> DebridError {
    let HttpError::Status {
        status,
        body,
        retry_after,
    } = e
    else {
        return e.into();
    };
    if status == 429 {
        return DebridError::RateLimited { retry_after };
    }
    match serde_json::from_slice::<Envelope<serde_json::Value>>(&body) {
        Ok(env) => envelope_error(env.error.as_deref(), env.detail.unwrap_or_default()),
        Err(_) if status == 401 || status == 403 => DebridError::Auth,
        Err(_) => DebridError::provider(status.to_string(), ""),
    }
}

#[derive(Deserialize)]
struct TbUser {
    #[serde(default)]
    email: String,
    #[serde(default)]
    plan: u32,
    premium_expires_at: Option<String>,
}

#[derive(Deserialize)]
struct TbFile {
    id: u32,
    name: String,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize)]
struct TbTorrent {
    id: u64,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    progress: f32,
    #[serde(default)]
    download_state: String,
    #[serde(default)]
    download_finished: bool,
    #[serde(default)]
    download_present: bool,
    #[serde(default)]
    files: Vec<TbFile>,
}

impl TbTorrent {
    fn into_info(self) -> TorrentInfo {
        let status = if self.download_finished || self.download_present {
            TorrentStatus::Ready
        } else {
            match self.download_state.as_str() {
                "cached" | "completed" | "uploading" => TorrentStatus::Ready,
                "metaDL" | "checkingResumeData" => TorrentStatus::Resolving,
                s if s.contains("error") || s == "missingFiles" => {
                    TorrentStatus::Failed(s.to_owned())
                }
                _ => TorrentStatus::Downloading,
            }
        };
        TorrentInfo {
            id: self.id.to_string(),
            hash: self.hash.to_ascii_lowercase(),
            name: self.name,
            status,
            progress: self.progress.clamp(0.0, 1.0),
            files: self
                .files
                .into_iter()
                .map(|f| TorrentFile {
                    index: f.id,
                    path: f.name,
                    size: f.size,
                    selected: true,
                    link: None,
                })
                .collect(),
        }
    }
}

/// `mylist?id=` answers with one object, but accept a list too.
#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(TbTorrent),
    Many(Vec<TbTorrent>),
}

#[derive(Deserialize)]
struct Created {
    torrent_id: u64,
    #[serde(default)]
    hash: String,
}

#[derive(Deserialize)]
struct Cached {
    hash: String,
}

impl TorBox {
    async fn mylist(&self, id: &str) -> Result<TbTorrent, DebridError> {
        let path = with_query("/torrents/mylist", &[("id", id), ("bypass_cache", "true")]);
        let data: Option<OneOrMany> = self.call(Method::GET, &path, None).await?;
        match data {
            Some(OneOrMany::One(t)) => Ok(t),
            Some(OneOrMany::Many(mut list)) if !list.is_empty() => Ok(list.remove(0)),
            _ => Err(DebridError::provider("ITEM_NOT_FOUND", "torrent not found")),
        }
    }

    /// Finds an existing torrent by hash, for the duplicate-item case.
    async fn find_by_hash(&self, hash: &str) -> Result<Option<TbTorrent>, DebridError> {
        let path = with_query("/torrents/mylist", &[("bypass_cache", "true")]);
        let list: Option<Vec<TbTorrent>> = self.call(Method::GET, &path, None).await?;
        Ok(list
            .unwrap_or_default()
            .into_iter()
            .find(|t| t.hash.eq_ignore_ascii_case(hash)))
    }
}

impl Store for TorBox {
    fn id(&self) -> StoreId {
        StoreId::TorBox
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            instant_check: true,
            file_select: false,
            ip_locked: false,
            device_code_login: false,
            rate_limit_per_min: Some(300),
        }
    }

    async fn user(&self) -> Result<StoreUser, DebridError> {
        let user: TbUser = self
            .call(Method::GET, "/user/me", None)
            .await?
            .ok_or_else(|| DebridError::provider("empty", "no user data"))?;
        Ok(StoreUser {
            name: user.email,
            is_premium: user.plan > 0,
            premium_until: user.premium_expires_at.as_deref().and_then(parse_utc),
        })
    }

    async fn check(&self, hashes: &[String]) -> Result<Vec<CacheStatus>, DebridError> {
        let mut cached = HashSet::new();
        for chunk in hashes.chunks(CHECK_CHUNK) {
            let body = Body::Json(serde_json::json!({ "hashes": chunk }));
            let path = with_query(
                "/torrents/checkcached",
                &[("format", "list"), ("list_files", "false")],
            );
            // Only cached torrents are listed; `data` is null when none are.
            let found: Option<Vec<Cached>> = self.call(Method::POST, &path, Some(body)).await?;
            cached.extend(
                found
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| c.hash.to_ascii_lowercase()),
            );
        }
        Ok(hashes
            .iter()
            .map(|h| {
                if cached.contains(&h.to_ascii_lowercase()) {
                    CacheStatus::Cached
                } else {
                    CacheStatus::NotCached
                }
            })
            .collect())
    }

    async fn add_magnet(&self, info_hash: &str) -> Result<TorrentHandle, DebridError> {
        let hash = normalize_hash(info_hash)?;
        let body = Body::Multipart(vec![("magnet".to_owned(), magnet_uri(&hash))]);
        match self
            .call::<Created>(Method::POST, "/torrents/createtorrent", Some(body))
            .await
        {
            Ok(Some(created)) => Ok(TorrentHandle {
                id: created.torrent_id.to_string(),
                hash: if created.hash.is_empty() {
                    hash
                } else {
                    created.hash.to_ascii_lowercase()
                },
            }),
            Ok(None) => Err(DebridError::provider("empty", "no torrent id returned")),
            Err(DebridError::Provider { code, .. }) if code == "DUPLICATE_ITEM" => self
                .find_by_hash(&hash)
                .await?
                .map(|t| TorrentHandle {
                    id: t.id.to_string(),
                    hash: hash.clone(),
                })
                .ok_or_else(|| {
                    DebridError::provider("DUPLICATE_ITEM", "item exists but not found")
                }),
            Err(e) => Err(e),
        }
    }

    async fn torrent(&self, id: &str) -> Result<TorrentInfo, DebridError> {
        Ok(self.mylist(id).await?.into_info())
    }

    async fn select_files(&self, _id: &str, _file_ids: &[u32]) -> Result<(), DebridError> {
        Ok(())
    }

    async fn link(
        &self,
        torrent: &TorrentInfo,
        file_index: u32,
    ) -> Result<ResolvedLink, DebridError> {
        let file = torrent
            .files
            .iter()
            .find(|f| f.index == file_index)
            .ok_or(DebridError::NoPlayableFile)?;
        let file_id = file_index.to_string();
        let url: String = authed(self.auth.as_ref(), |key| {
            let path = with_query(
                "/torrents/requestdl",
                &[
                    ("token", key.expose()),
                    ("torrent_id", &torrent.id),
                    ("file_id", &file_id),
                ],
            );
            async move { self.call_as(&key, Method::GET, &path, None).await }
        })
        .await?
        .ok_or_else(|| DebridError::provider("empty", "no download link"))?;
        Ok(ResolvedLink {
            url,
            size: Some(file.size),
            filename: crate::util::basename(&file.path).to_owned(),
            expires_at: Some(unix_now() + LINK_VALID_SECS),
        })
    }

    async fn remove(&self, id: &str) -> Result<(), DebridError> {
        let torrent_id: u64 = id
            .parse()
            .map_err(|_| DebridError::provider("invalid_id", "torrent id must be numeric"))?;
        let body =
            Body::Json(serde_json::json!({ "torrent_id": torrent_id, "operation": "delete" }));
        self.call::<serde_json::Value>(Method::POST, "/torrents/controltorrent", Some(body))
            .await?;
        Ok(())
    }
}

/// Device sign-in (in the OpenAPI file only): the user enters `code` at
/// `verification_url`; the app collects the account's key once with the
/// device code. A second collection answers `DEVICE_CODE_USED`.
pub(crate) struct Device {
    api: Api,
    device_code: Secret,
}

#[derive(Deserialize)]
struct DeviceStart {
    device_code: String,
    code: String,
    interval: u64,
    expires_in: u64,
    verification_url: String,
    verification_url_complete: Option<String>,
}

/// The collected token: the docs say only "returns the user's token", Kodi
/// reads `data.access_token`; a bare string is accepted too.
#[derive(Deserialize)]
#[serde(untagged)]
enum Collected {
    Token { access_token: String },
    Bare(String),
}

impl Device {
    pub(crate) async fn start(http: &HttpClient, base: &str) -> Result<(Self, Code), DebridError> {
        let api = Api::new(http.clone(), base);
        let path = with_query("/v1/api/user/auth/device/start", &[("app", "Marquee")]);
        let envelope: Envelope<DeviceStart> = api
            .call(Method::GET, &path, None, Vec::new())
            .await
            .map_err(map_error)?
            .json()?;
        let start = match (envelope.success, envelope.data) {
            (true, Some(start)) => start,
            _ => {
                return Err(envelope_error(
                    envelope.error.as_deref(),
                    envelope.detail.unwrap_or_default(),
                ));
            }
        };
        // The complete URL carries only the user code.
        let code = Code {
            qr: start
                .verification_url_complete
                .unwrap_or_else(|| start.verification_url.clone()),
            url: start.verification_url,
            user_code: start.code,
            interval: Duration::from_secs(start.interval.max(1)),
            expires_in: Duration::from_secs(start.expires_in),
        };
        let device = Self {
            api,
            device_code: Secret::new(start.device_code),
        };
        Ok((device, code))
    }

    pub(crate) async fn poll(&self) -> Result<LoginPoll, DebridError> {
        let body = Body::Json(serde_json::json!({ "device_code": self.device_code.expose() }));
        let response = self
            .api
            .call(
                Method::POST,
                "/v1/api/user/auth/device/token",
                Some(body),
                Vec::new(),
            )
            .await
            .map_err(map_error);
        let envelope: Envelope<Collected> = match response {
            Ok(response) => response.json()?,
            Err(DebridError::Provider { code, .. }) => {
                return Ok(match code.as_str() {
                    "DEVICE_CODE_USED" => LoginPoll::Used,
                    "ITEM_NOT_FOUND" => LoginPoll::Expired,
                    _ => LoginPoll::Pending,
                });
            }
            Err(DebridError::RateLimited { .. }) => return Ok(LoginPoll::SlowDown),
            Err(e) => return Err(e),
        };
        match envelope.data {
            Some(Collected::Token { access_token: key } | Collected::Bare(key))
                if envelope.success && !key.is_empty() =>
            {
                Ok(LoginPoll::Approved(Credential::Key(key)))
            }
            _ => Err(DebridError::provider(
                envelope.error.unwrap_or_else(|| "empty".into()),
                "no token in the answer",
            )),
        }
    }
}
