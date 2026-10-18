//! Range reads of verified torrent bytes that feed DWH demand events.

use std::io::SeekFrom;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll, ready};
use std::time::Instant;

use mq_engine_sched::DemandId;
use tokio::io::{AsyncRead, AsyncSeekExt, ReadBuf};
use tokio::sync::{mpsc, watch};

use crate::adapter::Msg;
use crate::{Engine, EngineError, FileInfo, Result, Torrent, lock};

/// One file of a selected torrent. Cheap; open a [`RangeReader`] per HTTP
/// range.
pub struct StreamReader {
    engine: Engine,
    torrent: Arc<Torrent>,
    file: FileInfo,
}

impl StreamReader {
    pub(crate) fn new(engine: Engine, torrent: Arc<Torrent>, file: FileInfo) -> Self {
        Self {
            engine,
            torrent,
            file,
        }
    }

    pub fn file(&self) -> &FileInfo {
        &self.file
    }

    pub fn len(&self) -> u64 {
        self.file.len
    }

    pub fn is_empty(&self) -> bool {
        self.file.len == 0
    }

    /// Opens `[start, end)` (file-relative; `None` reads to the end). The
    /// range becomes scheduler demand here, before any body byte is read.
    pub async fn range(&self, start: u64, end: Option<u64>) -> Result<RangeReader> {
        let len = self.file.len;
        let end = end.unwrap_or(len).min(len);
        if start > end {
            return Err(EngineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "range starts past its end",
            )));
        }
        let (managed, msgs) = self
            .engine
            .wake(&self.torrent)
            .await?
            .ok_or(EngineError::NoFile)?;
        self.torrent.readers.fetch_add(1, Ordering::Relaxed);
        let demand = self.engine.next_demand();
        let (ready_tx, ready) = watch::channel(false);
        let _ = msgs.send(Msg::Open {
            demand,
            file: self.file.idx,
            start,
            end: Some(end),
            ready: ready_tx,
        });
        let mut reader = RangeReader {
            inner: None,
            pos: start,
            end,
            demand,
            msgs,
            ready,
            torrent: self.torrent.clone(),
        };
        #[cfg(feature = "test-hooks")]
        crate::test_hooks::wait(&self.engine).await;
        managed.wait_until_initialized().await?;
        let mut stream = managed.stream(self.file.idx).await?;
        stream.seek(SeekFrom::Start(start)).await?;
        reader.inner = Some(Box::new(stream));
        Ok(reader)
    }
}

/// Verified bytes of one range. Dropping it closes the demand.
pub struct RangeReader {
    inner: Option<Box<dyn AsyncRead + Send + Unpin>>,
    pos: u64,
    end: u64,
    demand: DemandId,
    msgs: mpsc::UnboundedSender<Msg>,
    ready: watch::Receiver<bool>,
    torrent: Arc<Torrent>,
}

impl RangeReader {
    /// Resolves once the range's first `blocking_bytes` and its file's tail
    /// are verified (DWH `Ready`; the same rule in stock mode).
    pub async fn ready(&mut self) -> Result<()> {
        self.ready
            .wait_for(|r| *r)
            .await
            .map(|_| ())
            .map_err(|_| EngineError::Torrent(anyhow::anyhow!("torrent stopped")))
    }

    pub fn position(&self) -> u64 {
        self.pos
    }

    pub fn remaining(&self) -> u64 {
        self.end - self.pos
    }
}

impl AsyncRead for RangeReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let want = (this.end - this.pos).min(buf.remaining() as u64) as usize;
        let Some(inner) = this.inner.as_mut().filter(|_| want > 0) else {
            return Poll::Ready(Ok(()));
        };
        let mut part = ReadBuf::new(buf.initialize_unfilled_to(want));
        ready!(Pin::new(inner).poll_read(cx, &mut part))?;
        let n = part.filled().len();
        buf.advance(n);
        if n > 0 {
            this.pos += n as u64;
            let _ = this.msgs.send(Msg::Advance {
                demand: this.demand,
                pos: this.pos,
            });
        }
        Poll::Ready(Ok(()))
    }
}

impl Drop for RangeReader {
    fn drop(&mut self) {
        let _ = self.msgs.send(Msg::Close {
            demand: self.demand,
        });
        self.torrent.readers.fetch_sub(1, Ordering::Relaxed);
        *lock(&self.torrent.last_used) = Instant::now();
    }
}
