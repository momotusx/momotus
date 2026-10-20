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
    /// Marquee fork (plans/streaming-window.md 2.5): writes the bitfield as
    /// it is now and answers once that write and its fsync are done, or
    /// with the step that failed. In memory there is nothing to write.
    fn flush_acked(&mut self) -> FlushAck {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = tx.send(Ok(()));
        rx
    }
}

pub type BoxBitV = Box<dyn BitV>;

/// Marquee fork: the answer to [`BitV::flush_acked`]. A dead flusher drops
/// the sender, so the receiver errs.
pub type FlushAck = tokio::sync::oneshot::Receiver<Result<(), String>>;

struct DiskFlushRequest {
    snapshot: BitBox<u8, Msb0>,
    /// Marquee fork: answered after the fsync of a write that includes this
    /// snapshot (a later one merged with it holds it too).
    acks: Vec<tokio::sync::oneshot::Sender<Result<(), String>>>,
}

/// Marquee fork (engine review r5, P3-3): what happened to which file, in
/// order, for tests: a data file synced, or a bitfield written with how
/// many pieces it claims. Each test reads its own folder's events. Also
/// the points where a test holds an eviction round, a stream's read or a
/// bitfield's fsync, and the steps it makes fail (plans/streaming-window.md
/// phase 1), each for files under one folder.
#[cfg(feature = "sync-log")]
pub mod sync_log {
    use std::ops::Range;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Condvar, Mutex};

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Event {
        DataSynced(PathBuf),
        HaveWritten {
            path: PathBuf,
            claimed: usize,
        },
        /// A bitfield write was fsynced: its bytes.
        HaveSynced {
            path: PathBuf,
            pieces: Vec<u8>,
        },
        /// Bytes of a data file were punched out.
        Punched {
            path: PathBuf,
            range: Range<u64>,
        },
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Point {
        /// A stream holds its piece's read lock, before its read.
        Read,
        /// An eviction round after each step: gated, acknowledged, punched,
        /// released.
        Gated,
        Acked,
        Punched,
        Released,
        /// A bitfield write, before its fsync.
        Fsync,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Fail {
        DataSync,
        HaveWrite,
        HaveFsync,
        Punch,
    }

    #[derive(Default)]
    pub struct Hold {
        /// (reached, open)
        state: Mutex<(bool, bool)>,
        cv: Condvar,
    }

    impl Hold {
        /// Something stopped here.
        pub fn reached(&self) -> bool {
            self.state.lock().unwrap_or_else(|e| e.into_inner()).0
        }

        /// Lets every held step, and every later one, go on.
        pub fn open(&self) {
            self.state.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
            self.cv.notify_all();
        }
    }

    type Holds = Vec<(PathBuf, Point, Arc<Hold>)>;
    static HOLDS: Mutex<Holds> = Mutex::new(Vec::new());
    static FAILS: Mutex<Vec<(PathBuf, Fail, usize)>> = Mutex::new(Vec::new());

    /// Stops every step at `point` for a file under `dir` until
    /// [`Hold::open`].
    pub fn hold(dir: &Path, point: Point) -> Arc<Hold> {
        let h = Arc::new(Hold::default());
        let mut holds = HOLDS.lock().unwrap_or_else(|e| e.into_inner());
        holds.push((dir.to_owned(), point, h.clone()));
        h
    }

    /// The next `times` steps `step` for a file under `dir` fail.
    pub fn fail(dir: &Path, step: Fail, times: usize) {
        let mut fails = FAILS.lock().unwrap_or_else(|e| e.into_inner());
        fails.push((dir.to_owned(), step, times));
    }

    /// Blocks while a hold for `path` at `point` is closed.
    pub(crate) fn pass(path: &Path, point: Point) {
        let hold = HOLDS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(d, p, _)| *p == point && path.starts_with(d))
            .map(|(_, _, h)| h.clone());
        if let Some(h) = hold {
            let mut s = h.state.lock().unwrap_or_else(|e| e.into_inner());
            s.0 = true;
            while !s.1 {
                s = h.cv.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        }
    }

    /// Whether this `step` for `path` is one a test made fail.
    pub(crate) fn failing(path: &Path, step: Fail) -> bool {
        let mut fails = FAILS.lock().unwrap_or_else(|e| e.into_inner());
        let hit = fails
            .iter_mut()
            .find(|(d, s, n)| *s == step && *n > 0 && path.starts_with(d));
        hit.map(|(_, _, n)| *n -= 1).is_some()
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
                Event::DataSynced(p)
                | Event::HaveWritten { path: p, .. }
                | Event::HaveSynced { path: p, .. }
                | Event::Punched { path: p, .. } => p.starts_with(dir),
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
                acks: Vec::new(),
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
                    // Marquee fork: every merged request is answered.
                    let mut acks = std::mem::take(&mut req.acks);
                    while let Ok(r) = rx.try_recv() {
                        req = r;
                        acks.append(&mut req.acks);
                    }
                    let answer = |acks: Vec<_>, r: Result<(), String>| {
                        for a in acks {
                            let _: Result<(), _> = tokio::sync::oneshot::Sender::send(a, r.clone());
                        }
                    };

                    // Marquee fork (engine review r4, P1-1): the data first.
                    if let Err(e) = spawner
                        .block_in_place_with_semaphore(|| {
                            #[cfg(feature = "sync-log")]
                            if sync_log::failing(&filename, sync_log::Fail::DataSync) {
                                return Err(std::io::Error::other("injected"));
                            }
                            to_sync.lock().sync_data(&req.snapshot)
                        })
                        .await
                    {
                        tracing::error!(
                            ?filename,
                            "torrent data did not sync, bitv not written: {e:#}"
                        );
                        answer(acks, Err(format!("data sync: {e}")));
                        continue;
                    }

                    if let Err(e) = spawner
                        .block_in_place_with_semaphore(|| {
                            #[cfg(feature = "sync-log")]
                            if sync_log::failing(&filename, sync_log::Fail::HaveWrite) {
                                anyhow::bail!("injected");
                            }
                            flushed.pwrite_all(0, req.snapshot.as_raw_slice())
                        })
                        .await
                    {
                        tracing::error!(?filename, "error writing to bitv: {e:#}");
                        answer(acks, Err(format!("bitfield write: {e:#}")));
                        if let Err(e) = tokio::fs::remove_file(&filename).await {
                            tracing::error!(?filename, "error removing bitv: {e:#}");
                        }
                        break;
                    }
                    log_written(&filename, req.snapshot.count_ones());

                    if let Err(e) = spawner
                        .block_in_place_with_semaphore(|| {
                            #[cfg(feature = "sync-log")]
                            {
                                sync_log::pass(&filename, sync_log::Point::Fsync);
                                if sync_log::failing(&filename, sync_log::Fail::HaveFsync) {
                                    return Err(std::io::Error::other("injected"));
                                }
                            }
                            flushed.sync_all()
                        })
                        .await
                    {
                        tracing::error!(?filename, "error fsyncing bitv: {e:#}");
                        answer(acks, Err(format!("bitfield fsync: {e}")));
                        continue;
                    }
                    #[cfg(feature = "sync-log")]
                    sync_log::push(sync_log::Event::HaveSynced {
                        path: filename.clone(),
                        pieces: req.snapshot.as_raw_slice().to_vec(),
                    });
                    answer(acks, Ok(()));
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
            acks: Vec::new(),
        };
        self.flush_tx.send(req).context("flusher task is dead")
    }

    fn flush_acked(&mut self) -> FlushAck {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self.flush_tx.send(DiskFlushRequest {
            snapshot: self.bv.clone(),
            acks: vec![tx],
        });
        rx
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

    fn flush_acked(&mut self) -> FlushAck {
        (**self).flush_acked()
    }

    fn into_dyn(self) -> Box<dyn BitV> {
        self
    }
}
