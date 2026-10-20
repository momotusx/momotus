use crate::{BLOCK_SIZE, ByteCache, ProbeError, RangeReader};
use bytes::Bytes;
use futures_util::future::BoxFuture;
use mq_http::{HttpClient, HttpError, Priority, RequestOptions};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `RangeReader` over HTTP. There is no HEAD request: the length comes from
/// the `Content-Range` of the first ranged read. Reads are widened to whole
/// cache blocks and stored in the shared `ByteCache` under the URL.
pub struct HttpRangeReader {
    http: HttpClient,
    url: String,
    headers: Vec<(String, String)>,
    cache: Arc<ByteCache>,
    allow_private: bool,
    len: Mutex<Option<u64>>,
}

impl HttpRangeReader {
    // ponytail: the URL is the cache key; take a stable source id (infohash,
    // file index) once signed URLs for the same file need to share blocks.
    pub fn new(
        http: HttpClient,
        url: impl Into<String>,
        headers: Vec<(String, String)>,
        cache: Arc<ByteCache>,
    ) -> Self {
        Self {
            http,
            url: url.into(),
            headers,
            cache,
            allow_private: false,
            len: Mutex::new(None),
        }
    }

    /// Allows LAN and loopback origins (local media servers, tests).
    pub fn allow_private(mut self, allow: bool) -> Self {
        self.allow_private = allow;
        self
    }

    fn known_len(&self) -> Option<u64> {
        *self.len.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_len(&self, len: u64) {
        *self.len.lock().unwrap_or_else(|e| e.into_inner()) = Some(len);
    }

    /// Fetches a block-aligned span and caches it. Returns where the body
    /// starts: a server that answers 200 with a body that fits sent the whole
    /// file from offset 0.
    async fn fetch(&self, start: u64, len: u64) -> Result<(u64, Bytes), ProbeError> {
        let opts = RequestOptions {
            priority: Priority::Probe,
            timeout: Duration::from_secs(15),
            max_bytes: usize::try_from(len).unwrap_or(usize::MAX),
            allow_private: self.allow_private,
            headers: self.headers.clone(),
            ..Default::default()
        };
        let (at, body) = match self.http.get_range(&self.url, start, len, &opts).await {
            Ok(r) if r.status == 206 => {
                let total = r
                    .header("content-range")
                    .and_then(|v| v.rsplit_once('/'))
                    .and_then(|(_, total)| total.trim().parse().ok());
                if let Some(total) = total {
                    self.set_len(total);
                }
                (start, r.body)
            }
            Ok(r) => {
                self.set_len(r.body.len() as u64);
                (0, r.body)
            }
            // A 200 larger than the requested span: the server ignored Range.
            Err(HttpError::TooLarge { .. }) => return Err(ProbeError::RangeUnsupported),
            Err(HttpError::Status { status: 416, .. }) => (start, Bytes::new()),
            Err(e) => return Err(ProbeError::Http(e.to_string())),
        };
        self.cache.insert(&self.url, at, &body);
        Ok((at, body))
    }

    async fn read(&self, offset: u64, len: u64) -> Result<Bytes, ProbeError> {
        let total = self.known_len();
        let len = total.map_or(len, |t| len.min(t.saturating_sub(offset)));
        if len == 0 {
            return Ok(Bytes::new());
        }
        if let Some(b) = self.cache.get(&self.url, offset, len) {
            return Ok(b);
        }
        let start = offset / BLOCK_SIZE * BLOCK_SIZE;
        let end = offset.saturating_add(len).div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
        let end = total.map_or(end, |t| end.min(t));
        let (at, body) = self.fetch(start, end - start).await?;
        let from = usize::try_from(offset - at)
            .unwrap_or(usize::MAX)
            .min(body.len());
        let to = from
            .saturating_add(usize::try_from(len).unwrap_or(usize::MAX))
            .min(body.len());
        Ok(body.slice(from..to))
    }
}

impl RangeReader for HttpRangeReader {
    fn len(&self) -> BoxFuture<'_, Result<u64, ProbeError>> {
        Box::pin(async move {
            if let Some(len) = self.known_len() {
                return Ok(len);
            }
            // Bypass the cache: a cached block does not carry the length.
            self.fetch(0, BLOCK_SIZE).await?;
            self.known_len().ok_or(ProbeError::UnknownLength)
        })
    }

    fn read_at(&self, offset: u64, len: u64) -> BoxFuture<'_, Result<Bytes, ProbeError>> {
        Box::pin(self.read(offset, len))
    }
}
