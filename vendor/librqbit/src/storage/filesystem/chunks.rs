//! Marquee fork (plans/streaming-window-chunks.md): a torrent file kept as
//! a folder of piece files, for volumes that cannot punch holes. The folder
//! sits at the file's own path; each file in it holds one piece's part of
//! the torrent file and is named by the piece's index in the torrent. An
//! eviction deletes whole piece files, so space comes back without a punch.

use std::fs::{File, OpenOptions};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use super::opened_file::OurFileExt;

/// One torrent file as piece files under `dir`.
#[derive(Clone, Debug)]
pub struct Chunked {
    dir: PathBuf,
    /// The file's first byte in the torrent.
    start: u64,
    piece_len: u64,
    len: u64,
}

impl Chunked {
    pub fn new(dir: PathBuf, start: u64, piece_len: u64, len: u64) -> Self {
        Self {
            dir,
            start,
            piece_len,
            len,
        }
    }

    /// The piece holding file byte `at` and its part of the file.
    fn piece(&self, at: u64) -> (u64, Range<u64>) {
        let piece = (self.start + at) / self.piece_len;
        let first = (piece * self.piece_len).max(self.start) - self.start;
        let end = ((piece + 1) * self.piece_len - self.start).min(self.len);
        (piece, first..end)
    }

    fn path(&self, piece: u64) -> PathBuf {
        self.dir.join(piece.to_string())
    }

    /// Each piece file `at..at + len` touches: the piece, its file's length,
    /// the offset in it and the bytes of the span it holds.
    fn spans(&self, at: u64, len: u64) -> io::Result<Vec<(u64, u64, u64, Range<usize>)>> {
        let end = at
            .checked_add(len)
            .filter(|e| *e <= self.len)
            .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
        let mut out = Vec::new();
        let mut pos = at;
        while pos < end {
            let (piece, part) = self.piece(pos);
            let upto = part.end.min(end);
            let span = (pos - at) as usize..(upto - at) as usize;
            out.push((piece, part.end - part.start, pos - part.start, span));
            pos = upto;
        }
        Ok(out)
    }

    /// Reads `buf.len()` bytes at `at`; a piece file not there reads zeros,
    /// as a hole does.
    pub fn read_exact_at(&self, at: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        for (piece, _, off, span) in self.spans(at, buf.len() as u64)? {
            let out = &mut buf[span];
            match File::open(self.path(piece)) {
                Ok(f) => f.pread_exact(off, out)?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => out.fill(0),
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Writes `buf` at `at`. A piece file is created at its full length,
    /// sparse, so a read anywhere in it reads data or zeros.
    pub fn write_all_at(&self, at: u64, buf: &[u8]) -> anyhow::Result<()> {
        for (piece, piece_len, off, span) in self.spans(at, buf.len() as u64)? {
            let f = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(self.path(piece))?;
            if f.metadata()?.len() != piece_len {
                f.set_len(piece_len)?;
            }
            f.pwrite_all(off, &buf[span])?;
        }
        Ok(())
    }

    /// Deletes every piece file wholly inside `offset..offset + len` and
    /// returns the range they covered, empty when none is. A part of a
    /// piece stays, as a punch keeps a part of a block.
    pub fn punch(&self, offset: u64, len: u64) -> io::Result<Range<u64>> {
        let end = offset.saturating_add(len).min(self.len);
        let mut freed: Option<Range<u64>> = None;
        let mut pos = offset;
        while pos < end {
            let (piece, part) = self.piece(pos);
            if offset <= part.start && part.end <= end {
                match std::fs::remove_file(self.path(piece)) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
                let start = freed.as_ref().map_or(part.start, |f| f.start);
                freed = Some(start..part.end);
            }
            pos = part.end;
        }
        let freed = freed.unwrap_or(offset..offset);
        #[cfg(feature = "sync-log")]
        if freed.start < freed.end {
            crate::bitv::sync_log::push(crate::bitv::sync_log::Event::Punched {
                path: self.dir.clone(),
                range: freed.clone(),
            });
        }
        Ok(freed)
    }
}

/// What a torrent file at `path` takes on disk: a regular file's blocks, a
/// piece folder's files summed; none when it is not there.
pub fn allocated(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => std::fs::read_dir(path).map_or(0, |entries| {
            entries
                .flatten()
                .map(|e| e.metadata().map_or(0, |m| blocks(&m)))
                .sum()
        }),
        Ok(m) => blocks(&m),
        Err(_) => 0,
    }
}

fn blocks(m: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::blocks(m) * 512
    }
    #[cfg(not(unix))]
    {
        m.len()
    }
}

/// `dir` holds piece files only (files named by a number): one torrent
/// file, not a folder of the torrent.
pub fn is_chunked(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().all(|e| {
            e.file_type().is_ok_and(|t| t.is_file())
                && e.file_name()
                    .to_str()
                    .is_some_and(|n| n.parse::<u64>().is_ok())
        })
    })
}
