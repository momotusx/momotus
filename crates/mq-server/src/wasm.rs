//! webOS (plans/webos.md 3.4): no listener and no proxy. The TV's player
//! gets the source's own final URL, so video bytes never pass through the
//! core; the service's server asks `StreamServer::answer` for `/sub/` and
//! `/logo/`, which name the service's address (`set_origin`). A source is
//! told apart from a playlist by one bounded prefix, with the native
//! `declared`, `candidate` and `sniff`.

use super::*;
use std::sync::OnceLock;

static ORIGIN: OnceLock<SocketAddr> = OnceLock::new();

/// The service's address, which subtitle and logo URLs name (`origin` in the
/// binding's config). Set once, before the core opens.
pub fn set_origin(addr: SocketAddr) {
    let _ = ORIGIN.set(addr);
}

/// An answer for the service's server.
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

/// A direct playback's source: the player reads `final_url` itself; the
/// core reads it only for the container probe, the subtitle hash and
/// `Lease::probe`.
pub(crate) struct RangeSource {
    /// The playback's scoped handle.
    pub(crate) http: HttpClient,
    /// The URL after every hop the core followed: what the player gets.
    pub(crate) final_url: String,
    headers: Vec<(String, String)>,
}

impl RangeSource {
    pub(crate) fn options(&self) -> RequestOptions {
        RequestOptions {
            priority: Priority::Playback,
            headers: self.headers.clone(),
            ..Default::default()
        }
    }

    pub(crate) fn reader(&self) -> mq_media::HttpRangeReader {
        // ponytail: a small cache per reader; the probe and the hash read
        // the same few blocks.
        let cache = Arc::new(mq_media::ByteCache::new(8 << 20));
        mq_media::HttpRangeReader::new(
            self.http.clone(),
            self.final_url.clone(),
            self.headers.clone(),
            cache,
        )
    }
}

impl StreamServer {
    /// Binds nothing: the service serves, at the address `set_origin` gave.
    pub async fn start(http: HttpClient) -> std::io::Result<Self> {
        let addr = *ORIGIN
            .get()
            .ok_or_else(|| std::io::Error::other("no origin set"))?;
        Ok(Self {
            shared: Arc::new(Shared {
                http,
                playbacks: Mutex::default(),
                addr: Mutex::new(addr),
                logos: OnceLock::new(),
                logo_token: new_token(),
            }),
        })
    }

    /// Nothing to bind again.
    pub async fn revive(&self) -> std::io::Result<bool> {
        Ok(false)
    }

    /// A playback whose source answered within `deadline`. A file, or a
    /// playlist of a format the player plays as listed, is leased under its
    /// final URL; a playlist the player cannot play as listed (DASH served
    /// as HLS needs a rewrite, which webOS has no server for) fails with the
    /// format words, so the next candidate is tried.
    pub async fn open_for(
        &self,
        upstream: Upstream,
        filename: &str,
        deadline: Duration,
        plays: &[Format],
    ) -> Result<Lease, String> {
        let _ = filename;
        let Upstream::Http {
            url,
            headers,
            addon_url,
        } = upstream
        else {
            // Downloads are off on webOS: no file of this device plays.
            return Err(FORMAT_NOT_HERE.into());
        };
        let http = upstream_http(&self.shared.http, addon_url.as_ref());
        let detect = detect(http, &url, headers, plays);
        let (source, format) = match mq_rt::timeout(deadline, detect).await {
            Ok(found) => found?,
            Err(_) => return Err(SILENT.into()),
        };
        let token = new_token();
        let url = source.final_url.clone();
        let playback = Arc::new(Playback {
            body: Content::Http(Arc::new(source)),
            released: CancellationToken::new(),
            subtitles: Mutex::default(),
            trouble: Mutex::default(),
            relabel: Default::default(),
            fault: Mutex::default(),
            waits: Arc::default(),
        });
        lock(&self.shared.playbacks).insert(token.clone(), playback.clone());
        Ok(Lease {
            url,
            format,
            token,
            playback,
            shared: self.shared.clone(),
        })
    }

    /// The service's `/sub/` and `/logo/` routes; `None` is 404.
    pub async fn answer(&self, path: &str) -> Option<Answer> {
        let path = path.split('?').next().unwrap_or_default();
        let parts: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
        let text = |pairs: &[(header::HeaderName, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(k, v)| (k.as_str().to_owned(), (*v).to_owned()))
                .collect()
        };
        match parts.as_slice() {
            ["sub", token, id, _name] => {
                let vtt = subtitle_vtt(&self.shared, token, id).await?;
                let mut headers = text(&VTT_HEADERS);
                headers.push(("access-control-allow-origin".into(), "*".into()));
                Some(Answer {
                    status: 200,
                    headers,
                    body: Bytes::from(vtt),
                })
            }
            ["logo", token, key] => {
                let logo = logo(&self.shared, token, key)?;
                Some(Answer {
                    status: 200,
                    headers: text(&logo_headers(logo)),
                    body: Bytes::from_static(logo.bytes),
                })
            }
            _ => None,
        }
    }
}

/// The source's first `SNIFF` bytes through the playback's scoped handle,
/// whatever its status: what it is, and the URL the player gets.
async fn detect(
    http: HttpClient,
    url: &str,
    headers: Vec<(String, String)>,
    plays: &[Format],
) -> Result<(RangeSource, Option<Format>), String> {
    let source = RangeSource {
        http,
        final_url: url.to_owned(),
        headers,
    };
    let opts = source.options();
    let prefix = source
        .http
        .get_prefix(url, 0, manifest::SNIFF as u64, &opts)
        .await
        .map_err(|e| error_words(&e))?;
    let answer = &prefix.response;
    if !(200..300).contains(&answer.status) {
        return Err(StatusCode::from_u16(answer.status).map_or(UNREACHABLE.to_owned(), refused));
    }
    let content_type = header_text(&answer.headers);
    let final_url = answer.final_url.to_string();
    let declared = manifest::declared(content_type, &[url, &final_url]);
    let len = total_len(answer, prefix.truncated);
    let source = RangeSource {
        final_url,
        ..source
    };
    if declared.is_none() && !manifest::candidate(content_type, len) {
        return Ok((source, None));
    }
    let format = match (manifest::sniff(&answer.body), declared) {
        (Sniffed::Hls, None | Some(Declared::Hls)) => Format::Hls,
        (Sniffed::Dash, None | Some(Declared::Dash)) => Format::Dash,
        (Sniffed::File, None) => return Ok((source, None)),
        // Declared but not that format: never handed to the player raw.
        _ => return Err(UNREADABLE_PLAYLIST.into()),
    };
    if !plays.contains(&format) {
        return Err(FORMAT_NOT_HERE.into());
    }
    Ok((source, Some(format)))
}

/// The whole body's length: `Content-Range`'s total for a 206, the body for
/// a 200 read whole, its `Content-Length` for one cut at the prefix.
fn total_len(answer: &mq_http::Response, truncated: bool) -> Option<u64> {
    let header = |name| answer.headers.get(name).and_then(|v| v.to_str().ok());
    if answer.status == 206 {
        let (_, total) = header(header::CONTENT_RANGE)?.rsplit_once('/')?;
        return total.trim().parse().ok();
    }
    if !truncated {
        return Some(answer.body.len() as u64);
    }
    header(header::CONTENT_LENGTH)?.trim().parse().ok()
}
