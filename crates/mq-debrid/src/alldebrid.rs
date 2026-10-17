//! AllDebrid, API v4 / v4.1 (https://docs.alldebrid.com/).
//!
//! The documented endpoint list has no instant availability call, so `check`
//! answers `Unknown`. Files come back as a folder tree with a link per file;
//! the tree is flattened depth first and numbered from 0.

use crate::api::{Api, Auth, Secret, authed, bearer, fixed, form};
use crate::login::{Code, Credential, LoginPoll};
use crate::util::basename;
use crate::util::{magnet_uri, normalize_hash};
use crate::{
    CacheStatus, Capabilities, DebridError, ResolvedLink, Store, StoreId, StoreUser, TorrentFile,
    TorrentHandle, TorrentInfo, TorrentStatus,
};
use mq_http::{Body, HttpClient, HttpError, Method, Priority};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;

pub(crate) const BASE: &str = "https://api.alldebrid.com";

#[derive(Clone)]
pub struct AllDebrid {
    api: Api,
    auth: Arc<dyn Auth>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    status: String,
    data: Option<T>,
    error: Option<ErrorBody>,
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
    #[serde(default)]
    message: String,
}

impl AllDebrid {
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

    /// Sends and unwraps `{status, data | error}`. Errors arrive with HTTP 200.
    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Body>,
    ) -> Result<T, DebridError> {
        self.call_keyed(method, path, body)
            .await
            .map(|(data, _)| data)
    }

    /// Like `call`, also handing back the key the answer was given for, so an
    /// error inside the answer (one magnet of an upload) is stripped of it.
    async fn call_keyed<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Body>,
    ) -> Result<(T, Secret), DebridError> {
        authed(self.auth.as_ref(), |key| {
            let (method, body) = (method.clone(), body.clone());
            async move {
                let response = self
                    .api
                    .call(method, path, body, bearer(&key))
                    .await
                    .map_err(map_http_error)?;
                Ok((unwrap(response.json()?)?, key))
            }
        })
        .await
    }
}

/// An error inside a successful answer, without the key that answer was for.
fn nested(code: &str, message: String, key: &Secret) -> DebridError {
    map_code(code, message).without(&[key.expose()])
}

fn map_code(code: &str, message: String) -> DebridError {
    match code {
        "AUTH_MISSING_APIKEY" | "AUTH_BAD_APIKEY" | "AUTH_USER_BANNED" => DebridError::Auth,
        "MUST_BE_PREMIUM" | "MAGNET_MUST_BE_PREMIUM" => DebridError::NotPremium,
        "MAGNET_TOO_MANY_ACTIVE"
        | "MAGNET_TOO_MANY"
        | "LINK_HOST_LIMIT_REACHED"
        | "FREE_TRIAL_LIMIT_REACHED"
        | "LINK_TOO_MANY_DOWNLOADS" => DebridError::Quota,
        other => DebridError::provider(other, message),
    }
}

fn map_http_error(e: HttpError) -> DebridError {
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
        Ok(Envelope {
            error: Some(error), ..
        }) => map_code(&error.code, error.message),
        _ if status == 401 => DebridError::Auth,
        _ => DebridError::provider(status.to_string(), ""),
    }
}

#[derive(Deserialize)]
struct UserData {
    user: AdUser,
}

#[derive(Deserialize)]
struct AdUser {
    username: String,
    #[serde(default, rename = "isPremium")]
    is_premium: bool,
    #[serde(default, rename = "premiumUntil")]
    premium_until: u64,
}

#[derive(Deserialize)]
struct UploadData {
    magnets: Vec<UploadedMagnet>,
}

#[derive(Deserialize)]
struct UploadedMagnet {
    id: Option<u64>,
    hash: Option<String>,
    error: Option<ErrorBody>,
}

#[derive(Deserialize)]
struct StatusData {
    magnets: OneOrMany,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(AdMagnet),
    Many(Vec<AdMagnet>),
}

#[derive(Deserialize)]
struct AdMagnet {
    id: u64,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    size: u64,
    #[serde(default, rename = "statusCode")]
    status_code: u32,
    #[serde(default)]
    status: String,
    #[serde(default)]
    downloaded: u64,
}

#[derive(Deserialize)]
struct FilesData {
    magnets: Vec<MagnetFiles>,
}

#[derive(Deserialize)]
struct MagnetFiles {
    files: Option<Vec<Node>>,
    error: Option<ErrorBody>,
}

/// A folder (`e`) or a file (`s`, `l`).
#[derive(Deserialize)]
struct Node {
    n: String,
    s: Option<u64>,
    l: Option<String>,
    e: Option<Vec<Node>>,
}

fn flatten(nodes: Vec<Node>, prefix: &str, out: &mut Vec<TorrentFile>) {
    for node in nodes {
        let path = if prefix.is_empty() {
            node.n.clone()
        } else {
            format!("{prefix}/{}", node.n)
        };
        match node.e {
            Some(children) => flatten(children, &path, out),
            None => out.push(TorrentFile {
                index: out.len() as u32,
                path,
                size: node.s.unwrap_or(0),
                selected: true,
                link: node.l,
            }),
        }
    }
}

fn map_status(code: u32, text: &str) -> TorrentStatus {
    match code {
        0 => TorrentStatus::Queued,
        1..=3 => TorrentStatus::Downloading,
        4 => TorrentStatus::Ready,
        _ => TorrentStatus::Failed(text.to_owned()),
    }
}

#[derive(Deserialize)]
struct Unlocked {
    #[serde(default)]
    link: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    filesize: u64,
    delayed: Option<u64>,
}

impl Store for AllDebrid {
    fn id(&self) -> StoreId {
        StoreId::AllDebrid
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            instant_check: false,
            file_select: false,
            ip_locked: true,
            device_code_login: false,
            rate_limit_per_min: Some(600),
        }
    }

    async fn user(&self) -> Result<StoreUser, DebridError> {
        let data: UserData = self.call(Method::GET, "/v4/user", None).await?;
        let user = data.user;
        Ok(StoreUser {
            name: user.username,
            is_premium: user.is_premium,
            premium_until: (user.premium_until > 0).then_some(user.premium_until),
        })
    }

    async fn check(&self, hashes: &[String]) -> Result<Vec<CacheStatus>, DebridError> {
        Ok(vec![CacheStatus::Unknown; hashes.len()])
    }

    async fn add_magnet(&self, info_hash: &str) -> Result<TorrentHandle, DebridError> {
        let hash = normalize_hash(info_hash)?;
        let body = form(&[("magnets[]", &magnet_uri(&hash))]);
        let (data, key): (UploadData, _) = self
            .call_keyed(Method::POST, "/v4/magnet/upload", Some(body))
            .await?;
        let magnet = data
            .magnets
            .into_iter()
            .next()
            .ok_or_else(|| DebridError::provider("empty", "no magnet in response"))?;
        if let Some(error) = magnet.error {
            return Err(nested(&error.code, error.message, &key));
        }
        let id = magnet
            .id
            .ok_or_else(|| DebridError::provider("empty", "no magnet id"))?;
        Ok(TorrentHandle {
            id: id.to_string(),
            hash: magnet.hash.map_or(hash, |h| h.to_ascii_lowercase()),
        })
    }

    async fn torrent(&self, id: &str) -> Result<TorrentInfo, DebridError> {
        let body = form(&[("id", id)]);
        let data: StatusData = self
            .call(Method::POST, "/v4.1/magnet/status", Some(body))
            .await?;
        let magnet = match data.magnets {
            OneOrMany::One(m) => m,
            OneOrMany::Many(mut list) if !list.is_empty() => list.remove(0),
            OneOrMany::Many(_) => {
                return Err(DebridError::provider(
                    "MAGNET_INVALID_ID",
                    "magnet not found",
                ));
            }
        };
        let status = map_status(magnet.status_code, &magnet.status);
        let mut files = Vec::new();
        if status == TorrentStatus::Ready {
            let body = form(&[("id[]", id)]);
            let (data, key): (FilesData, _) = self
                .call_keyed(Method::POST, "/v4/magnet/files", Some(body))
                .await?;
            if let Some(entry) = data.magnets.into_iter().next() {
                if let Some(error) = entry.error {
                    return Err(nested(&error.code, error.message, &key));
                }
                flatten(entry.files.unwrap_or_default(), "", &mut files);
            }
        }
        Ok(TorrentInfo {
            id: magnet.id.to_string(),
            hash: String::new(),
            name: magnet.filename,
            status,
            progress: if magnet.size > 0 {
                (magnet.downloaded as f32 / magnet.size as f32).clamp(0.0, 1.0)
            } else {
                0.0
            },
            files,
        })
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
        let locked = file
            .link
            .as_deref()
            .ok_or_else(|| DebridError::provider("no_link", "file has no link"))?;
        let body = form(&[("link", locked)]);
        let out: Unlocked = self
            .call(Method::POST, "/v4/link/unlock", Some(body))
            .await?;
        // ponytail: hoster style delayed links do not occur for magnet files;
        // add the /link/delayed poll if AllDebrid starts returning them here.
        if out.delayed.is_some() || out.link.is_empty() {
            return Err(DebridError::provider(
                "link_delayed",
                "link is not ready yet",
            ));
        }
        Ok(ResolvedLink {
            url: out.link,
            size: if out.filesize > 0 {
                Some(out.filesize)
            } else {
                Some(file.size)
            },
            filename: if out.filename.is_empty() {
                basename(&file.path).to_owned()
            } else {
                out.filename
            },
            expires_at: None,
        })
    }

    async fn remove(&self, id: &str) -> Result<(), DebridError> {
        let body = form(&[("id", id)]);
        self.call::<serde_json::Value>(Method::POST, "/v4/magnet/delete", Some(body))
            .await?;
        Ok(())
    }
}

/// The docs poll every 5 s, and a check sooner than 5 s after the start
/// answers `PIN_INVALID`.
const PIN_INTERVAL: Duration = Duration::from_secs(5);

/// PIN sign-in: the user enters `pin` at `user_url`; the app checks with
/// `pin` and `check` until AllDebrid hands out the account's key.
pub(crate) struct Pin {
    api: Api,
    pin: String,
    check: String,
}

#[derive(Deserialize)]
struct PinStart {
    pin: String,
    check: String,
    expires_in: u64,
    user_url: String,
    #[serde(default)]
    base_url: String,
}

#[derive(Deserialize)]
struct PinCheck {
    #[serde(default)]
    activated: bool,
    apikey: Option<String>,
}

impl Pin {
    pub(crate) fn secrets(&self) -> Vec<&str> {
        vec![&self.pin, &self.check]
    }

    pub(crate) async fn start(http: &HttpClient, base: &str) -> Result<(Self, Code), DebridError> {
        let api = Api::new(http.clone(), base);
        let start: PinStart = unwrap(
            api.call(Method::GET, "/v4.1/pin/get", None, Vec::new())
                .await
                .map_err(map_http_error)?
                .json()?,
        )?;
        // `user_url` carries only the PIN, so it is the QR as is.
        let code = Code {
            url: if start.base_url.is_empty() {
                start.user_url.clone()
            } else {
                start.base_url
            },
            qr: start.user_url,
            user_code: start.pin.clone(),
            interval: PIN_INTERVAL,
            expires_in: Duration::from_secs(start.expires_in),
        };
        let pin = Self {
            api,
            pin: start.pin,
            check: start.check,
        };
        Ok((pin, code))
    }

    pub(crate) async fn poll(&self) -> Result<LoginPoll, DebridError> {
        let body = form(&[("pin", &self.pin), ("check", &self.check)]);
        let response = self
            .api
            .call(Method::POST, "/v4/pin/check", Some(body), Vec::new())
            .await
            .map_err(map_http_error)?;
        let envelope: Envelope<PinCheck> = response.json()?;
        if let Some(error) = envelope.error {
            return match error.code.as_str() {
                "PIN_EXPIRED" => Ok(LoginPoll::Expired),
                "PIN_ALREADY_AUTHED" => Ok(LoginPoll::Used),
                // Too early, or not entered yet; the deadline bounds it.
                "PIN_INVALID" => Ok(LoginPoll::Pending),
                code => Err(map_code(code, error.message)),
            };
        }
        Ok(match envelope.data {
            Some(PinCheck {
                activated: true,
                apikey: Some(key),
            }) if !key.is_empty() => LoginPoll::Approved(Credential::Key(key)),
            _ => LoginPoll::Pending,
        })
    }
}

fn unwrap<T>(envelope: Envelope<T>) -> Result<T, DebridError> {
    match (envelope.status.as_str(), envelope.data, envelope.error) {
        ("success", Some(data), _) => Ok(data),
        (_, _, Some(error)) => Err(map_code(&error.code, error.message)),
        _ => Err(DebridError::provider("empty", "no data in response")),
    }
}
