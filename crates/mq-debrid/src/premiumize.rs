//! Premiumize.me (https://app.swaggerhub.com/apis-docs/premiumize.me/api).
//!
//! Auth is an `apikey` parameter (form field on POST, query on GET; mq-http
//! redacts query values in logs). Cached magnets resolve instantly through
//! `transfer/directdl`, which needs no transfer on the account; those handles
//! are `dd:<hash>`. Anything else becomes a regular transfer.
//! Responses are `{status: "success" | "error", message}`, often with HTTP 200.

use crate::api::{Api, Secret, form, with_query};
use crate::util::{basename, magnet_uri, normalize_hash, unix_now};
use crate::{
    CacheStatus, Capabilities, DebridError, ResolvedLink, Store, StoreId, StoreUser, TorrentFile,
    TorrentHandle, TorrentInfo, TorrentStatus,
};
use mq_http::{HttpClient, HttpError, Method, Priority, Response};
use serde::Deserialize;
use serde::de::DeserializeOwned;

const BASE: &str = "https://www.premiumize.me/api";
const CHECK_CHUNK: usize = 50;
const MAX_FOLDER_DEPTH: u32 = 4;
const DIRECT_PREFIX: &str = "dd:";

#[derive(Clone)]
pub struct Premiumize {
    api: Api,
    key: Secret,
}

#[derive(Deserialize)]
struct Status {
    status: String,
    message: Option<String>,
}

impl Premiumize {
    pub fn new(http: HttpClient, api_key: impl Into<String>) -> Self {
        Self {
            api: Api::new(http, BASE),
            key: Secret::new(api_key),
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

    async fn get(&self, path: &str, pairs: &[(&str, &str)]) -> Result<Response, DebridError> {
        let mut all = vec![("apikey", self.key.expose())];
        all.extend_from_slice(pairs);
        self.api
            .call(Method::GET, &with_query(path, &all), None, Vec::new())
            .await
            .map_err(map_http_error)
    }

    async fn post(&self, path: &str, pairs: &[(&str, &str)]) -> Result<Response, DebridError> {
        let mut all = vec![("apikey", self.key.expose())];
        all.extend_from_slice(pairs);
        self.api
            .call(Method::POST, path, Some(form(&all)), Vec::new())
            .await
            .map_err(map_http_error)
    }
}

/// Decodes a response, turning `status: "error"` into a [`DebridError`].
fn decode<T: DeserializeOwned>(response: &Response) -> Result<T, DebridError> {
    let status: Status = response.json()?;
    if status.status != "success" {
        return Err(map_message(status.message.unwrap_or_default()));
    }
    Ok(response.json()?)
}

fn map_message(message: String) -> DebridError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("apikey") || lower.contains("api key") || lower.contains("not logged") {
        DebridError::Auth
    } else if lower.contains("premium") || lower.contains("subscription") {
        DebridError::NotPremium
    } else if lower.contains("limit") || lower.contains("fair use") {
        DebridError::Quota
    } else {
        DebridError::provider("error", message)
    }
}

fn map_http_error(e: HttpError) -> DebridError {
    match e {
        HttpError::Status {
            status: 429,
            retry_after,
            ..
        } => DebridError::RateLimited { retry_after },
        HttpError::Status {
            status: 401 | 403, ..
        } => DebridError::Auth,
        HttpError::Status { status, body, .. } => match serde_json::from_slice::<Status>(&body) {
            Ok(s) => map_message(s.message.unwrap_or_default()),
            Err(_) => DebridError::provider(status.to_string(), ""),
        },
        other => other.into(),
    }
}

#[derive(Deserialize)]
struct Account {
    #[serde(default)]
    customer_id: serde_json::Value,
    #[serde(default)]
    premium_until: Option<u64>,
}

#[derive(Deserialize)]
struct CacheCheck {
    #[serde(default)]
    response: Vec<bool>,
}

#[derive(Deserialize)]
struct Created {
    id: String,
}

#[derive(Deserialize)]
struct DirectDl {
    #[serde(default)]
    content: Vec<DirectFile>,
}

#[derive(Deserialize)]
struct DirectFile {
    path: String,
    #[serde(default)]
    size: u64,
    link: Option<String>,
}

#[derive(Deserialize)]
struct Transfers {
    #[serde(default)]
    transfers: Vec<Transfer>,
}

#[derive(Deserialize)]
struct Transfer {
    id: String,
    #[serde(default)]
    name: String,
    status: String,
    #[serde(default)]
    progress: f32,
    folder_id: Option<String>,
    file_id: Option<String>,
}

#[derive(Deserialize)]
struct FolderList {
    #[serde(default)]
    content: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    size: u64,
    link: Option<String>,
}

fn map_transfer_status(status: &str) -> TorrentStatus {
    match status {
        "finished" | "seeding" => TorrentStatus::Ready,
        "running" => TorrentStatus::Downloading,
        "error" | "banned" | "timeout" | "deleted" => TorrentStatus::Failed(status.to_owned()),
        _ => TorrentStatus::Queued,
    }
}

impl Premiumize {
    async fn direct_files(&self, hash: &str) -> Result<Vec<TorrentFile>, DebridError> {
        let response = self
            .post("/transfer/directdl", &[("src", &magnet_uri(hash))])
            .await?;
        let direct: DirectDl = decode(&response)?;
        let mut files: Vec<TorrentFile> = direct
            .content
            .into_iter()
            .enumerate()
            .map(|(i, f)| TorrentFile {
                index: i as u32,
                path: f.path,
                size: f.size,
                selected: true,
                link: f.link,
            })
            .collect();
        files.retain(|f| f.link.is_some());
        Ok(files)
    }

    /// Files of a finished transfer, walking folders to a fixed depth.
    async fn folder_files(&self, folder_id: &str) -> Result<Vec<TorrentFile>, DebridError> {
        let mut files = Vec::new();
        let mut pending = vec![(folder_id.to_owned(), String::new(), 0u32)];
        while let Some((id, prefix, depth)) = pending.pop() {
            let response = self.get("/folder/list", &[("id", &id)]).await?;
            let list: FolderList = decode(&response)?;
            for item in list.content {
                let path = if prefix.is_empty() {
                    item.name.clone()
                } else {
                    format!("{prefix}/{}", item.name)
                };
                if item.kind == "folder" {
                    if depth < MAX_FOLDER_DEPTH {
                        pending.push((item.id, path, depth + 1));
                    }
                } else {
                    files.push(TorrentFile {
                        index: 0,
                        path,
                        size: item.size,
                        selected: true,
                        link: item.link,
                    });
                }
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        for (i, file) in files.iter_mut().enumerate() {
            file.index = i as u32;
        }
        Ok(files)
    }

    async fn single_file(&self, file_id: &str) -> Result<Vec<TorrentFile>, DebridError> {
        let response = self.get("/item/details", &[("id", file_id)]).await?;
        let item: Item = decode(&response)?;
        Ok(vec![TorrentFile {
            index: 0,
            path: item.name,
            size: item.size,
            selected: true,
            link: item.link,
        }])
    }
}

impl Store for Premiumize {
    fn id(&self) -> StoreId {
        StoreId::Premiumize
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            instant_check: true,
            file_select: false,
            ip_locked: false,
            device_code_login: false,
            rate_limit_per_min: None,
        }
    }

    async fn user(&self) -> Result<StoreUser, DebridError> {
        let response = self.get("/account/info", &[]).await?;
        let account: Account = decode(&response)?;
        let name = match &account.customer_id {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        };
        let until = account.premium_until.filter(|&t| t > 0);
        Ok(StoreUser {
            name,
            is_premium: until.is_some_and(|t| t > unix_now()),
            premium_until: until,
        })
    }

    async fn check(&self, hashes: &[String]) -> Result<Vec<CacheStatus>, DebridError> {
        let mut out = Vec::with_capacity(hashes.len());
        for chunk in hashes.chunks(CHECK_CHUNK) {
            let magnets: Vec<String> = chunk.iter().map(|h| magnet_uri(h)).collect();
            let pairs: Vec<(&str, &str)> =
                magnets.iter().map(|m| ("items[]", m.as_str())).collect();
            let response = self.get("/cache/check", &pairs).await?;
            let check: CacheCheck = decode(&response)?;
            out.extend((0..chunk.len()).map(|i| match check.response.get(i) {
                Some(true) => CacheStatus::Cached,
                Some(false) => CacheStatus::NotCached,
                None => CacheStatus::Unknown,
            }));
        }
        Ok(out)
    }

    async fn add_magnet(&self, info_hash: &str) -> Result<TorrentHandle, DebridError> {
        let hash = normalize_hash(info_hash)?;
        // A cached magnet answers directdl; no transfer is created for it.
        if self
            .direct_files(&hash)
            .await
            .is_ok_and(|files| !files.is_empty())
        {
            return Ok(TorrentHandle {
                id: format!("{DIRECT_PREFIX}{hash}"),
                hash,
            });
        }
        let response = self
            .post("/transfer/create", &[("src", &magnet_uri(&hash))])
            .await?;
        let created: Created = decode(&response)?;
        Ok(TorrentHandle {
            id: created.id,
            hash,
        })
    }

    async fn torrent(&self, id: &str) -> Result<TorrentInfo, DebridError> {
        if let Some(hash) = id.strip_prefix(DIRECT_PREFIX) {
            let files = self.direct_files(hash).await?;
            return Ok(TorrentInfo {
                id: id.to_owned(),
                hash: hash.to_owned(),
                name: String::new(),
                status: TorrentStatus::Ready,
                progress: 1.0,
                files,
            });
        }
        let response = self.get("/transfer/list", &[]).await?;
        let list: Transfers = decode(&response)?;
        let transfer = list
            .transfers
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| DebridError::provider("not_found", "transfer not found"))?;
        let status = map_transfer_status(&transfer.status);
        let files = if status != TorrentStatus::Ready {
            Vec::new()
        } else if let Some(folder) = &transfer.folder_id {
            self.folder_files(folder).await?
        } else if let Some(file) = &transfer.file_id {
            self.single_file(file).await?
        } else {
            Vec::new()
        };
        Ok(TorrentInfo {
            id: transfer.id,
            hash: String::new(),
            name: transfer.name,
            status,
            progress: transfer.progress.clamp(0.0, 1.0),
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
        let url = file
            .link
            .clone()
            .ok_or_else(|| DebridError::provider("no_link", "file has no link"))?;
        Ok(ResolvedLink {
            url,
            size: Some(file.size),
            filename: basename(&file.path).to_owned(),
            expires_at: None,
        })
    }

    async fn remove(&self, id: &str) -> Result<(), DebridError> {
        if id.starts_with(DIRECT_PREFIX) {
            return Ok(());
        }
        let response = self.post("/transfer/delete", &[("id", id)]).await?;
        let status: Status = response.json()?;
        if status.status == "success" {
            Ok(())
        } else {
            Err(map_message(status.message.unwrap_or_default()))
        }
    }
}
