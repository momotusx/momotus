use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

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
    /// Marquee fork (engine review r4, P1-1): the torrent's data files, each
    /// with its pieces, synced before a write that claims a new piece in it.
    fn sync_data_of(&mut self, _files: Vec<(PathBuf, Range<usize>)>) {}
}

pub type BoxBitV = Box<dyn BitV>;

struct DiskFlushRequest {
    snapshot: BitBox<u8, Msb0>,
}

/// Marquee fork (engine review r5, P3-3): what happened to which file, in
/// order, for tests: a data file synced, or a bitfield written with how
/// many pieces it claims. Each test reads its own folder's events.
#[cfg(feature = "sync-log")]
pub mod sync_log {
    use std::path::{Path, PathBuf};

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Event {
        DataSynced(PathBuf),
        HaveWritten { path: PathBuf, claimed: usize },
    }

    static LOG: std::sync::Mutex<Vec<Event>> = std::sync::Mutex::new(Vec::new());

    pub(crate) fn push(e: Event) {
        LOG.lock().unwrap_or_else(|e| e.into_inner()).push(e);
    }

    /// Every event about a file under `dir`, in order.
    pub fn under(dir: &Path) -> Vec<Event> {
        LOG.lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|e| match e {
                Event::DataSynced(p) | Event::HaveWritten { path: p, .. } => p.starts_with(dir),
            })
            .cloned()
            .collect()
    }
}

/// A bitfield claiming `claimed` pieces was written to `path`.
pub(crate) fn log_written(_path: &Path, _claimed: usize) {
    #[cfg(feature = "sync-log")]
    sync_log::push(sync_log::Event::HaveWritten {
        path: _path.to_owned(),
        claimed: _claimed,
    });
}

/// Marquee fork (engine review r4, P1-1): what a bitfield write makes
/// durable first. Every piece in a snapshot was written before the
/// snapshot was taken, so with its files synced first the bitfield never
/// claims a piece the disk lost (a power cut after a flush).
#[derive(Default)]
pub(crate) struct Written {
    files: Vec<(PathBuf, Range<usize>)>,
    /// The last state whose data was synced.
    last: Option<BitBox<u8, Msb0>>,
}

impl Written {
    /// Nothing synced yet for `files`.
    pub(crate) fn new(files: Vec<(PathBuf, Range<usize>)>) -> Self {
        Self { files, last: None }
    }

    /// Syncs every file with a piece `next` claims that the last state did
    /// not. Blocking.
    pub(crate) fn sync_data(&mut self, next: &BitSlice<u8, Msb0>) -> std::io::Result<()> {
        let last = self.last.as_deref();
        let new = |i: usize| {
            next.get(i).is_some_and(|b| *b) && !last.and_then(|l| l.get(i)).is_some_and(|b| *b)
        };
        for (path, pieces) in &self.files {
            if !pieces.clone().any(&new) {
                continue;
            }
            // Write access: Windows flushes only a writable handle
            // (engine review r5, P3-4). No create, no truncate.
            match std::fs::OpenOptions::new().write(true).open(path) {
                Ok(f) => f.sync_data()?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            }
            #[cfg(feature = "sync-log")]
            sync_log::push(sync_log::Event::DataSynced(path.clone()));
        }
        self.last = Some(next.to_bitvec().into_boxed_bitslice());
        Ok(())
    }
}

pub struct DiskBackedBitV {
    bv: BitBox<u8, Msb0>,
    flush_tx: tokio::sync::mpsc::UnboundedSender<DiskFlushRequest>,
    /// Marquee fork (engine review r3, P2-2): the flusher's file, for the
    /// write on drop.
    file: Arc<std::fs::File>,
    /// Marquee fork (engine review r4, P1-1).
    written: Arc<parking_lot::Mutex<Written>>,
    spawner: BlockingSpawner,
    filename: PathBuf,
}

impl Drop for DiskBackedBitV {
    fn drop(&mut self) {
        // Marquee fork (engine review r3, P2-2): written at once as well (a
        // few hundred bytes into the page cache, no fsync): a process that
        // ends right after a torrent goes (the CLI) never runs the flusher
        // again. The snapshot below still follows any older one queued.
        // The data it claims is synced first (engine review r4, P1-1).
        let synced = self
            .spawner
            .block_in_place(|| self.written.lock().sync_data(&self.bv));
        match synced {
            Ok(()) => match self.file.pwrite_all(0, self.bv.as_raw_slice()) {
                Ok(()) => log_written(&self.filename, self.bv.count_ones()),
                Err(e) => tracing::warn!("error writing bitv on drop: {e:#}"),
            },
            Err(e) => tracing::warn!("bitv not written on drop, its data did not sync: {e:#}"),
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
        let written = Arc::new(parking_lot::Mutex::new(Written {
            files: Vec::new(),
            last: Some(bv.clone()),
        }));
        let (to_sync, own_spawner, own_filename) =
            (written.clone(), spawner.clone(), filename.clone());

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

                    // Marquee fork (engine review r4, P1-1): the data first.
                    if let Err(e) = spawner
                        .block_in_place_with_semaphore(|| to_sync.lock().sync_data(&req.snapshot))
                        .await
                    {
                        tracing::error!(
                            ?filename,
                            "torrent data did not sync, bitv not written: {e:#}"
                        );
                        continue;
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
                    log_written(&filename, req.snapshot.count_ones());

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
            written,
            spawner: own_spawner,
            filename: own_filename,
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

    fn sync_data_of(&mut self, files: Vec<(PathBuf, Range<usize>)>) {
        self.written.lock().files = files;
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

    fn sync_data_of(&mut self, files: Vec<(PathBuf, Range<usize>)>) {
        (**self).sync_data_of(files)
    }

    fn into_dyn(self) -> Box<dyn BitV> {
        self
    }
}
