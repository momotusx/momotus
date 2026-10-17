//! Real-Debrid, REST 1.0 (https://api.real-debrid.com/).
//!
//! `instantAvailability` is documented but disabled by the provider (the
//! error table lists "Disabled endpoint"), so `check` answers `Unknown`.

use crate::api::{Api, Auth, authed, bearer, fixed, form, with_query};
use crate::login::{Code, Credential, LoginPoll};
use crate::util::{magnet_uri, normalize_hash, parse_utc};
use crate::{
    CacheStatus, Capabilities, DebridError, ResolvedLink, Store, StoreId, StoreUser, TorrentFile,
    TorrentHandle, TorrentInfo, TorrentStatus,
};
use mq_http::{Body, HttpClient, HttpError, Method, Priority, Response};
use serde::Deserialize;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

const BASE: &str = "https://api.real-debrid.com/rest/1.0";
pub(crate) const OAUTH_BASE: &str = "https://api.real-debrid.com/oauth/v2";
/// Client id Real-Debrid publishes for open source apps.
const OPEN_SOURCE_CLIENT_ID: &str = "X245A4XAIBGVM";
const DEVICE_GRANT: &str = "http://oauth.net/grant_type/device/1.0";

#[derive(Clone)]
pub struct RealDebrid {
    api: Api,
    auth: Arc<dyn Auth>,
}

impl RealDebrid {
    pub fn new(http: HttpClient, access_token: impl Into<String>) -> Self {
        Self::with_auth(http, fixed(access_token))
    }

    /// A store that reads its token from `auth` per request and asks it for
    /// a newer one on a refusal (a rotated OAuth token).
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

    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Body>,
    ) -> Result<Response, DebridError> {
        authed(self.auth.as_ref(), |token| {
            let (method, body) = (method.clone(), body.clone());
            async move {
                self.api
                    .call(method, path, body, bearer(&token))
                    .await
                    .map_err(map_error)
            }
        })
        .await
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
    #[derive(Deserialize, Default)]
    struct ErrorBody {
        error: Option<String>,
        error_code: Option<i64>,
    }
    let parsed: ErrorBody = serde_json::from_slice(&body).unwrap_or_default();
    match (status, parsed.error_code) {
        (429, _) | (_, Some(5 | 34)) => DebridError::RateLimited { retry_after },
        (401, _) | (_, Some(8 | 12 | 13 | 14 | 15)) => DebridError::Auth,
        (403, _) | (_, Some(9)) => DebridError::NotPremium,
        (_, Some(21 | 23 | 36)) => DebridError::Quota,
        (status, code) => DebridError::provider(
            code.map_or_else(|| status.to_string(), |c| c.to_string()),
            parsed.error.unwrap_or_default(),
        ),
    }
}

#[derive(Deserialize)]
struct RdUser {
    username: String,
    #[serde(default, rename = "type")]
    kind: String,
    expiration: Option<String>,
}

#[derive(Deserialize)]
struct RdFile {
    id: u32,
    path: String,
    bytes: u64,
    #[serde(default)]
    selected: u8,
}

#[derive(Deserialize)]
struct RdTorrent {
    id: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    progress: f32,
    status: String,
    #[serde(default)]
    files: Vec<RdFile>,
    #[serde(default)]
    links: Vec<String>,
}

impl RdTorrent {
    fn into_info(mut self) -> TorrentInfo {
        self.files.sort_by_key(|f| f.id);
        // Links come back in the order of the selected files. If the counts
        // differ (archives were merged) the mapping is unknown, so none is set.
        let selected = self.files.iter().filter(|f| f.selected == 1).count();
        let aligned = selected == self.links.len();
        let mut next = 0;
        let files = self
            .files
            .into_iter()
            .map(|f| {
                let is_selected = f.selected == 1;
                let link = (aligned && is_selected).then(|| {
                    next += 1;
                    self.links[next - 1].clone()
                });
                TorrentFile {
                    index: f.id,
                    path: f.path.trim_start_matches('/').to_owned(),
                    size: f.bytes,
                    selected: is_selected,
                    link,
                }
            })
            .collect();
        TorrentInfo {
            id: self.id,
            hash: self.hash.to_ascii_lowercase(),
            name: self.filename,
            status: map_status(&self.status),
            progress: self.progress / 100.0,
            files,
        }
    }
}

fn map_status(status: &str) -> TorrentStatus {
    match status {
        "magnet_conversion" => TorrentStatus::Resolving,
        "waiting_files_selection" => TorrentStatus::WaitingFileSelection,
        "downloaded" => TorrentStatus::Ready,
        "downloading" | "compressing" | "uploading" => TorrentStatus::Downloading,
        "magnet_error" | "error" | "virus" | "dead" => TorrentStatus::Failed(status.to_owned()),
        _ => TorrentStatus::Queued,
    }
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

#[derive(Deserialize)]
struct Unrestricted {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    filesize: u64,
    download: String,
}

impl Store for RealDebrid {
    fn id(&self) -> StoreId {
        StoreId::RealDebrid
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            instant_check: false,
            file_select: true,
            ip_locked: true,
            device_code_login: true,
            rate_limit_per_min: Some(250),
        }
    }

    async fn user(&self) -> Result<StoreUser, DebridError> {
        let user: RdUser = self.call(Method::GET, "/user", None).await?.json()?;
        Ok(StoreUser {
            name: user.username,
            is_premium: user.kind == "premium",
            premium_until: user.expiration.as_deref().and_then(parse_utc),
        })
    }

    async fn check(&self, hashes: &[String]) -> Result<Vec<CacheStatus>, DebridError> {
        Ok(vec![CacheStatus::Unknown; hashes.len()])
    }

    async fn add_magnet(&self, info_hash: &str) -> Result<TorrentHandle, DebridError> {
        let hash = normalize_hash(info_hash)?;
        let body = form(&[("magnet", &magnet_uri(&hash))]);
        let created: Created = self
            .call(Method::POST, "/torrents/addMagnet", Some(body))
            .await?
            .json()?;
        Ok(TorrentHandle {
            id: created.id,
            hash,
        })
    }

    async fn torrent(&self, id: &str) -> Result<TorrentInfo, DebridError> {
        let torrent: RdTorrent = self
            .call(Method::GET, &format!("/torrents/info/{id}"), None)
            .await?
            .json()?;
        Ok(torrent.into_info())
    }

    async fn select_files(&self, id: &str, file_ids: &[u32]) -> Result<(), DebridError> {
        let files = if file_ids.is_empty() {
            "all".to_owned()
        } else {
            file_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        let body = form(&[("files", &files)]);
        self.call(
            Method::POST,
            &format!("/torrents/selectFiles/{id}"),
            Some(body),
        )
        .await?;
        Ok(())
    }

    async fn link(
        &self,
        torrent: &TorrentInfo,
        file_index: u32,
    ) -> Result<ResolvedLink, DebridError> {
        let host_link = torrent
            .files
            .iter()
            .find(|f| f.index == file_index)
            .and_then(|f| f.link.as_deref())
            .ok_or_else(|| DebridError::provider("no_link", "file has no link yet"))?;
        let body = form(&[("link", host_link)]);
        let out: Unrestricted = self
            .call(Method::POST, "/unrestrict/link", Some(body))
            .await?
            .json()?;
        Ok(ResolvedLink {
            url: out.download,
            size: (out.filesize > 0).then_some(out.filesize),
            filename: out.filename,
            expires_at: None,
        })
    }

    async fn remove(&self, id: &str) -> Result<(), DebridError> {
        self.call(Method::DELETE, &format!("/torrents/delete/{id}"), None)
            .await?;
        Ok(())
    }
}

/// Tokens from a finished device login. Debug output hides every secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds the access token stays valid.
    pub expires_in: u64,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    refresh_token: String,
}

async fn token_request(api: &Api, pairs: &[(&str, &str)]) -> Result<TokenResponse, DebridError> {
    let response = api
        .call(Method::POST, "/token", Some(form(pairs)), Vec::new())
        .await
        .map_err(map_error)?;
    Ok(response.json()?)
}

impl Credentials {
    /// Trades the refresh token for a new access token.
    pub async fn refresh(&self, http: &HttpClient) -> Result<Credentials, DebridError> {
        self.refresh_at(http, OAUTH_BASE).await
    }

    /// Like [`Credentials::refresh`] against another OAuth base URL.
    pub async fn refresh_at(
        &self,
        http: &HttpClient,
        base: &str,
    ) -> Result<Credentials, DebridError> {
        let api = Api::new(http.clone(), base);
        let token = token_request(
            &api,
            &[
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
                ("code", &self.refresh_token),
                ("grant_type", DEVICE_GRANT),
            ],
        )
        .await?;
        Ok(Credentials {
            access_token: token.access_token,
            expires_in: token.expires_in,
            refresh_token: if token.refresh_token.is_empty() {
                self.refresh_token.clone()
            } else {
                token.refresh_token
            },
            ..self.clone()
        })
    }
}

/// Device-code sign-in for open source apps: the user types `user_code` at
/// `verification_url`; each poll asks for the user-bound client id and
/// secret, then trades them and the device code for tokens.
pub(crate) struct Device {
    api: Api,
    device_code: String,
}

#[derive(Deserialize)]
struct DeviceCode {
    device_code: String,
    user_code: String,
    interval: u64,
    expires_in: u64,
    verification_url: String,
}

impl Device {
    pub(crate) async fn start(http: &HttpClient, base: &str) -> Result<(Self, Code), DebridError> {
        let api = Api::new(http.clone(), base);
        let path = with_query(
            "/device/code",
            &[
                ("client_id", OPEN_SOURCE_CLIENT_ID),
                ("new_credentials", "yes"),
            ],
        );
        let code: DeviceCode = api
            .call(Method::GET, &path, None, Vec::new())
            .await
            .map_err(map_error)?
            .json()?;
        // The QR carries the page and the user code, never the device code:
        // the undocumented `direct_verification_url` embeds it.
        let shown = Code {
            qr: code.verification_url.clone(),
            url: code.verification_url,
            user_code: code.user_code,
            interval: Duration::from_secs(code.interval.max(1)),
            expires_in: Duration::from_secs(code.expires_in),
        };
        let device = Self {
            api,
            device_code: code.device_code,
        };
        Ok((device, shown))
    }

    pub(crate) async fn poll(&self) -> Result<LoginPoll, DebridError> {
        let path = with_query(
            "/device/credentials",
            &[
                ("client_id", OPEN_SOURCE_CLIENT_ID),
                ("code", &self.device_code),
            ],
        );
        let body = match self.api.call(Method::GET, &path, None, Vec::new()).await {
            Ok(response) => response.body,
            // Until the user approves, the provider answers with an error body.
            Err(HttpError::Status {
                status: 400 | 403,
                body,
                ..
            }) => body,
            Err(e) => return Err(map_error(e)),
        };
        #[derive(Deserialize, Default)]
        struct Pair {
            client_id: Option<String>,
            client_secret: Option<String>,
            error: Option<String>,
        }
        let pair: Pair = serde_json::from_slice(&body).unwrap_or_default();
        let (Some(client_id), Some(client_secret)) = (pair.client_id, pair.client_secret) else {
            let error = pair.error.unwrap_or_default();
            return Ok(if error.contains("expired") {
                LoginPoll::Expired
            } else if error.contains("denied") {
                LoginPoll::Denied
            } else if error.contains("slow_down") {
                LoginPoll::SlowDown
            } else {
                LoginPoll::Pending
            });
        };
        let token = token_request(
            &self.api,
            &[
                ("client_id", &client_id),
                ("client_secret", &client_secret),
                ("code", &self.device_code),
                ("grant_type", DEVICE_GRANT),
            ],
        )
        .await?;
        Ok(LoginPoll::Approved(Credential::OAuth(Credentials {
            client_id,
            client_secret,
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_in: token.expires_in,
        })))
    }
}
