use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use bitvec::{boxed::BitBox, order::Msb0, slice::BitSlice, vec::BitVec};
use tracing::debug_span;

use crate::{spawn_utils::BlockingSpawner, storage::filesystem::OurFileExt};

pub trait BitV: Send + Sync {
    fn as_slice(&self) -> &BitSlice<u8, Msb0>;
    fn as_slice_mut(&mut self) -> &mut BitSlice<u8, Msb0>;
    fn into_dyn(self) -> Box<dyn BitV>;
    fn as_bytes(&self) -> &[u8];
    fn flush(&mut self, flush_async: bool) -> anyhow::Result<()>;
}

pub type BoxBitV = Box<dyn BitV>;

struct DiskFlushRequest {
    snapshot: BitBox<u8, Msb0>,
}

pub struct DiskBackedBitV {
    bv: BitBox<u8, Msb0>,
    flush_tx: tokio::sync::mpsc::UnboundedSender<DiskFlushRequest>,
    /// Marquee fork (engine review r3, P2-2): the flusher's file, for the
    /// write on drop.
    file: Arc<std::fs::File>,
}

impl Drop for DiskBackedBitV {
    fn drop(&mut self) {
        // Marquee fork (engine review r3, P2-2): written at once as well (a
        // few hundred bytes into the page cache, no fsync): a process that
        // ends right after a torrent goes (the CLI) never runs the flusher
        // again. The snapshot below still follows any older one queued.
        if let Err(e) = self.file.pwrite_all(0, self.bv.as_raw_slice()) {
            tracing::warn!("error writing bitv on drop: {e:#}");
        }
        if self
            .flush_tx
            .send(DiskFlushRequest {
                snapshot: self.bv.clone(),
            })
            .is_err()
        {
            tracing::warn!("error flushing bitv on drop: flusher task is dead")
        }
    }
}

// NOTE on mmap. rqbit used it for a while, but it has issues on slow disks.
// We want writes to bitv to be instant in RAM. However when disk is slow, occasionally
// the writes stall which blocks the executor.
// Thus this separate "thread" of flushing was implemented.
impl DiskBackedBitV {
    pub async fn new(filename: PathBuf, spawner: BlockingSpawner) -> anyhow::Result<Self> {
        let buf = tokio::fs::read(&filename)
            .await
            .with_context(|| format!("error reading {filename:?}"))?;
        let bv = BitVec::from_vec(buf).into_boxed_bitslice();

        // blocking file to avoid double-buffering and double-memcpy
        let file = spawner
            .block_in_place_with_semaphore(|| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(false)
                    .open(&filename)
            })
            .await
            .with_context(|| format!("error opening {filename:?}"))?;
        let file = Arc::new(file);
        let flushed = file.clone();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DiskFlushRequest>();
        librqbit_core::spawn_utils::spawn(
            debug_span!("diskbitv-flusher", ?filename),
            format!("DiskBackedBitV::flusher {filename:?}"),
            async move {
                loop {
                    let Some(mut req) = rx.recv().await else {
                        break;
                    };
                    while let Ok(r) = rx.try_recv() {
                        req = r;
                    }

                    if let Err(e) = spawner
                        .block_in_place_with_semaphore(|| {
                            flushed.pwrite_all(0, req.snapshot.as_raw_slice())
                        })
                        .await
                    {
                        tracing::error!(?filename, "error writing to bitv: {e:#}");
                        if let Err(e) = tokio::fs::remove_file(&filename).await {
                            tracing::error!(?filename, "error removing bitv: {e:#}");
                        }
                        break;
                    }

                    if let Err(e) = spawner
                        .block_in_place_with_semaphore(|| flushed.sync_all())
                        .await
                    {
                        tracing::error!(?filename, "error fsyncing bitv: {e:#}");
                    }
                }

                Ok::<_, anyhow::Error>(())
            },
        );
        Ok(Self {
            bv,
            flush_tx: tx,
            file,
        })
    }
}

#[async_trait::async_trait]
impl BitV for BitBox<u8, Msb0> {
    fn as_slice(&self) -> &BitSlice<u8, Msb0> {
        self.as_bitslice()
    }

    fn as_slice_mut(&mut self) -> &mut BitSlice<u8, Msb0> {
        self.as_mut_bitslice()
    }

    fn as_bytes(&self) -> &[u8] {
        self.as_raw_slice()
    }

    fn flush(&mut self, _flush_async: bool) -> anyhow::Result<()> {
        Ok(())
    }

    fn into_dyn(self) -> Box<dyn BitV> {
        Box::new(self)
    }
}

impl BitV for DiskBackedBitV {
    fn as_slice(&self) -> &BitSlice<u8, Msb0> {
        self.bv.as_bitslice()
    }

    fn as_slice_mut(&mut self) -> &mut BitSlice<u8, Msb0> {
        self.bv.as_mut_bitslice()
    }

    fn as_bytes(&self) -> &[u8] {
        self.bv.as_raw_slice()
    }

    fn flush(&mut self, _flush_async: bool) -> anyhow::Result<()> {
        let req = DiskFlushRequest {
            snapshot: self.bv.clone(),
        };
        self.flush_tx.send(req).context("flusher task is dead")
    }

    fn into_dyn(self) -> Box<dyn BitV> {
        Box::new(self)
    }
}

impl BitV for Box<dyn BitV> {
    fn as_slice(&self) -> &BitSlice<u8, Msb0> {
        (**self).as_slice()
    }

    fn as_slice_mut(&mut self) -> &mut BitSlice<u8, Msb0> {
        (**self).as_slice_mut()
    }

    fn as_bytes(&self) -> &[u8] {
        (**self).as_bytes()
    }

    fn flush(&mut self, flush_async: bool) -> anyhow::Result<()> {
        (**self).flush(flush_async)
    }

    fn into_dyn(self) -> Box<dyn BitV> {
        self
    }
}
