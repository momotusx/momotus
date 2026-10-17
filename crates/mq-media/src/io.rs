use crate::{ByteCache, CancellationToken, ProbeBudget, ProbeError, RangeReader};
use bytes::Bytes;
use std::future::Future;
use tokio::time::Instant;

/// Budgeted, block-cached view of a `RangeReader` for one probe. Reads fail
/// soft: they return `None` and record why in the public flags.
pub(crate) struct Src<'a> {
    reader: &'a dyn RangeReader,
    cancel: &'a CancellationToken,
    blocks: ByteCache,
    deadline: Instant,
    max_bytes: u64,
    pub len: Option<u64>,
    pub bytes_read: u64,
    pub exhausted: bool,
    pub cancelled: bool,
    pub error: Option<ProbeError>,
}

impl<'a> Src<'a> {
    pub fn new(
        reader: &'a dyn RangeReader,
        budget: ProbeBudget,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self {
            reader,
            cancel,
            blocks: ByteCache::new(u64::MAX),
            deadline: Instant::now() + budget.max_time,
            max_bytes: budget.max_bytes,
            len: None,
            bytes_read: 0,
            exhausted: false,
            cancelled: false,
            error: None,
        }
    }

    fn stopped(&mut self) -> bool {
        self.cancelled |= self.cancel.is_cancelled();
        let late = Instant::now() >= self.deadline;
        self.exhausted |= late;
        self.cancelled || self.error.is_some() || late
    }

    async fn guard<T>(&mut self, fut: impl Future<Output = Result<T, ProbeError>>) -> Option<T> {
        if self.stopped() {
            return None;
        }
        let cancel = self.cancel;
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                self.cancelled = true;
                None
            }
            () = tokio::time::sleep_until(self.deadline) => {
                self.exhausted = true;
                None
            }
            r = fut => r.map_err(|e| self.error = Some(e)).ok(),
        }
    }

    /// Learns the source length; an unknown length is not an error.
    pub async fn load_len(&mut self) {
        let reader = self.reader;
        let fut = async move {
            match reader.len().await {
                Ok(len) => Ok(Some(len)),
                Err(ProbeError::UnknownLength) => Ok(None),
                Err(e) => Err(e),
            }
        };
        self.len = self.guard(fut).await.flatten();
    }

    fn clip(&self, off: u64, len: u64) -> u64 {
        self.len.map_or(len, |l| len.min(l.saturating_sub(off)))
    }

    /// Up to `len` bytes at `off`, shorter only at end of file. `None` when the
    /// range is empty, over budget, cancelled or failed.
    pub async fn read(&mut self, off: u64, len: u64) -> Option<Bytes> {
        let len = self.clip(off, len);
        if len == 0 {
            return None;
        }
        if len > self.max_bytes {
            self.exhausted = true;
            return None;
        }
        if let Some(b) = self.blocks.get("", off, len) {
            return (!b.is_empty()).then_some(b);
        }
        let (start, end) = self.blocks.missing("", off, len)?;
        let want = self.clip(start, end - start);
        if self.bytes_read + want > self.max_bytes {
            self.exhausted = true;
            return None;
        }
        let reader = self.reader;
        let data = self.guard(reader.read_at(start, want)).await?;
        let data = data.slice(..data.len().min(want as usize));
        self.bytes_read += data.len() as u64;
        self.blocks.insert("", start, &data);
        self.blocks.get("", off, len).filter(|b| !b.is_empty())
    }

    /// Whether a range is already fetched, so reading it costs no round trip.
    pub fn cached(&self, off: u64, len: u64) -> bool {
        self.blocks
            .missing("", off, self.clip(off, len).min(self.max_bytes))
            .is_none()
    }

    /// Reads a range only when it fits the remaining budget, so later small
    /// reads hit the cache instead of making one round trip each.
    pub async fn prefetch(&mut self, off: u64, len: u64) {
        let len = self.clip(off, len);
        if len == 0 || len > self.max_bytes {
            return;
        }
        if let Some((start, end)) = self.blocks.missing("", off, len)
            && self.bytes_read + self.clip(start, end - start) <= self.max_bytes
        {
            self.read(off, len).await;
        }
    }
}
