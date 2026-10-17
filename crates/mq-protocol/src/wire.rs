//! Extension-preserving wire types for stream and subtitle responses.
//!
//! stremio-core's `Stream` source enum is closed, so streams that only carry
//! extensions (for example `clientResolve`) would be dropped. These DTOs accept
//! every known source field as optional, keep unknown keys, and classify the
//! source afterwards.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use url::Url;

fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Deserializes a list and drops entries that fail to parse.
fn lenient_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let values: Option<Vec<Value>> = Option::deserialize(deserializer)?;
    Ok(values
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

/// Accepts numbers or numeric strings.
fn lenient_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

fn lenient_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(lenient_u64(deserializer)?.and_then(|n| u32::try_from(n).ok()))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyHeaders {
    #[serde(default, deserialize_with = "null_as_default")]
    pub request: HashMap<String, String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub response: HashMap<String, String>,
}

/// Stream behavior hints: the official fields plus the AIOStreams extensions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamHints {
    #[serde(default, deserialize_with = "null_as_default")]
    pub not_web_ready: bool,
    #[serde(default)]
    pub binge_group: Option<String>,
    #[serde(default)]
    pub country_whitelist: Option<Vec<String>>,
    #[serde(default)]
    pub proxy_headers: Option<ProxyHeaders>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub video_hash: Option<String>,
    #[serde(default, deserialize_with = "lenient_u64")]
    pub video_size: Option<u64>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub cached: Option<bool>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub seeders: Option<u32>,
    #[serde(default)]
    pub indexer: Option<String>,
    #[serde(default)]
    pub publish_date: Option<String>,
    #[serde(default, deserialize_with = "lenient_u64")]
    pub duration: Option<u64>,
    #[serde(default)]
    pub folder_name: Option<String>,
    #[serde(default, deserialize_with = "lenient_u64")]
    pub folder_size: Option<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Client-side debrid resolution request carried by some addons.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientResolve {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub service: Option<String>,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub file_idx: Option<u32>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub season: Option<u32>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub episode: Option<u32>,
    #[serde(default)]
    pub is_cached: Option<bool>,
    #[serde(default)]
    pub stream: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireSubtitle {
    #[serde(default, deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub lang: String,
    pub url: Url,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub fonts: Vec<Url>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub forced: bool,
    #[serde(default, deserialize_with = "null_as_default")]
    pub hearing_impaired: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One stream object exactly as an addon sent it, minus nothing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireStream {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub yt_id: Option<String>,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default, deserialize_with = "lenient_u32")]
    pub file_idx: Option<u32>,
    #[serde(default, alias = "announce", deserialize_with = "null_as_default")]
    pub sources: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub file_must_include: Vec<String>,
    #[serde(default)]
    pub external_url: Option<String>,
    #[serde(default)]
    pub android_tv_url: Option<String>,
    #[serde(default)]
    pub player_frame_url: Option<String>,
    #[serde(default)]
    pub nzb_url: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub nzb_urls: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub servers: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub rar_urls: Vec<Value>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub zip_urls: Vec<Value>,
    #[serde(default, rename = "7zipUrls", deserialize_with = "null_as_default")]
    pub seven_zip_urls: Vec<Value>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub tgz_urls: Vec<Value>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub tar_urls: Vec<Value>,
    #[serde(default)]
    pub client_resolve: Option<ClientResolve>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, alias = "title")]
    pub description: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub subtitles: Vec<WireSubtitle>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub behavior_hints: StreamHints,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArchiveKind {
    Rar,
    Zip,
    SevenZip,
    Tgz,
    Tar,
}

/// What the client can do with a stream, decided once from the wire fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "camelCase")]
pub enum Source {
    Http {
        url: Url,
    },
    Torrent {
        /// 40 lowercase hex characters.
        info_hash: String,
        file_idx: Option<u32>,
        trackers: Vec<String>,
        file_must_include: Vec<String>,
    },
    ClientResolve(ClientResolve),
    YouTube {
        id: String,
    },
    PlayerFrame {
        url: Url,
    },
    External {
        url: Option<Url>,
        android_tv_url: Option<Url>,
    },
    Usenet {
        nzb_urls: Vec<String>,
        servers: Vec<String>,
    },
    Archive {
        kind: ArchiveKind,
        urls: Vec<Value>,
        file_idx: Option<u32>,
    },
    Invalid {
        reason: String,
    },
}

impl WireStream {
    /// Classifies the stream source. Order follows how specific each field is.
    pub fn source(&self) -> Source {
        if let Some(raw) = self.url.as_deref() {
            return classify_url(raw, self);
        }
        if let Some(hash) = self.info_hash.as_deref() {
            return torrent_source(hash, self.file_idx, &self.sources, &self.file_must_include);
        }
        if let Some(cr) = &self.client_resolve {
            return Source::ClientResolve(cr.clone());
        }
        if let Some(id) = &self.yt_id {
            return Source::YouTube { id: id.clone() };
        }
        if let Some(url) = self.player_frame_url.as_deref().and_then(parse_url) {
            return Source::PlayerFrame { url };
        }
        if self.external_url.is_some() || self.android_tv_url.is_some() {
            return Source::External {
                url: self.external_url.as_deref().and_then(parse_url),
                android_tv_url: self.android_tv_url.as_deref().and_then(parse_url),
            };
        }
        if self.nzb_url.is_some() || !self.nzb_urls.is_empty() {
            let mut nzb_urls = self.nzb_urls.clone();
            nzb_urls.extend(self.nzb_url.clone());
            return Source::Usenet {
                nzb_urls,
                servers: self.servers.clone(),
            };
        }
        self.archive_source().unwrap_or_else(|| Source::Invalid {
            reason: "no playable source field".into(),
        })
    }

    fn archive_source(&self) -> Option<Source> {
        let candidates = [
            (ArchiveKind::Rar, &self.rar_urls),
            (ArchiveKind::Zip, &self.zip_urls),
            (ArchiveKind::SevenZip, &self.seven_zip_urls),
            (ArchiveKind::Tgz, &self.tgz_urls),
            (ArchiveKind::Tar, &self.tar_urls),
        ];
        candidates
            .into_iter()
            .find(|(_, urls)| !urls.is_empty())
            .map(|(kind, urls)| Source::Archive {
                kind,
                urls: urls.clone(),
                file_idx: self.file_idx,
            })
    }
}

fn parse_url(raw: &str) -> Option<Url> {
    Url::parse(raw.trim()).ok()
}

fn classify_url(raw: &str, stream: &WireStream) -> Source {
    let Some(url) = parse_url(raw) else {
        return Source::Invalid {
            reason: format!("unparseable url: {}", truncate(raw, 64)),
        };
    };
    match url.scheme() {
        "http" | "https" => Source::Http { url },
        "magnet" => magnet_source(&url, stream),
        other => Source::Invalid {
            reason: format!("unsupported url scheme: {other}"),
        },
    }
}

fn magnet_source(url: &Url, stream: &WireStream) -> Source {
    let mut hash = None;
    let mut trackers = stream.sources.clone();
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "xt" => hash = value.strip_prefix("urn:btih:").map(str::to_owned),
            "tr" => trackers.push(format!("tracker:{value}")),
            _ => {}
        }
    }
    match hash {
        Some(h) => torrent_source(&h, stream.file_idx, &trackers, &stream.file_must_include),
        None => Source::Invalid {
            reason: "magnet without btih".into(),
        },
    }
}

fn torrent_source(
    hash: &str,
    file_idx: Option<u32>,
    trackers: &[String],
    file_must_include: &[String],
) -> Source {
    match normalize_info_hash(hash) {
        Some(info_hash) => Source::Torrent {
            info_hash,
            file_idx,
            trackers: trackers.to_vec(),
            file_must_include: file_must_include.to_vec(),
        },
        None => Source::Invalid {
            reason: format!("bad info hash: {}", truncate(hash, 64)),
        },
    }
}

/// Accepts 40-char hex or 32-char base32 v1 info hashes; returns lowercase hex.
pub fn normalize_info_hash(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.len() == 40 && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(raw.to_ascii_lowercase());
    }
    if raw.len() == 32 {
        return base32_to_hex(raw);
    }
    None
}

fn base32_to_hex(raw: &str) -> Option<String> {
    let mut bits: u64 = 0;
    let mut nbits = 0;
    let mut out = String::with_capacity(40);
    for c in raw.bytes() {
        let v = match c.to_ascii_uppercase() {
            b @ b'A'..=b'Z' => b - b'A',
            b @ b'2'..=b'7' => b - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | u64::from(v);
        nbits += 5;
        while nbits >= 4 {
            nbits -= 4;
            let nibble = (bits >> nbits) & 0xF;
            out.push(char::from_digit(nibble as u32, 16)?);
        }
    }
    (out.len() == 40).then_some(out)
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Result of decoding a `{ "streams": [...] }` response.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamsResponse {
    pub streams: Vec<WireStream>,
    /// Entries that were not objects or failed to decode.
    pub skipped: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubtitlesResponse {
    pub subtitles: Vec<WireSubtitle>,
    pub skipped: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("response has no `{0}` array")]
    MissingKey(&'static str),
}

fn decode_list<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    key: &'static str,
) -> Result<(Vec<T>, usize), WireError> {
    let root: Value = serde_json::from_slice(bytes)?;
    let items = match root.get(key) {
        Some(Value::Array(items)) => items,
        Some(Value::Null) => return Ok((Vec::new(), 0)),
        _ => return Err(WireError::MissingKey(key)),
    };
    let mut skipped = 0;
    let decoded = items
        .iter()
        .filter_map(|v| {
            let item = serde_json::from_value(v.clone()).ok();
            skipped += usize::from(item.is_none());
            item
        })
        .collect();
    Ok((decoded, skipped))
}

pub fn decode_streams(bytes: &[u8]) -> Result<StreamsResponse, WireError> {
    let (streams, skipped) = decode_list(bytes, "streams")?;
    Ok(StreamsResponse { streams, skipped })
}

pub fn decode_subtitles(bytes: &[u8]) -> Result<SubtitlesResponse, WireError> {
    let (subtitles, skipped) = decode_list(bytes, "subtitles")?;
    Ok(SubtitlesResponse { subtitles, skipped })
}
