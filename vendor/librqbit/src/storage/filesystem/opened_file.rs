use std::{
    fs::File,
    io::IoSlice,
    ops::{Deref, DerefMut},
    path::PathBuf,
};

use anyhow::Context;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::Error;

pub trait OurFileExt {
    fn pwrite_all_vectored(&self, offset: u64, bufs: [IoSlice<'_>; 2]) -> anyhow::Result<usize>;
    fn pread_exact(&self, offset: u64, buf: &mut [u8]) -> anyhow::Result<()>;
    fn pwrite_all(&self, offset: u64, buf: &[u8]) -> anyhow::Result<()>;
}

impl OurFileExt for File {
    // Marquee fork: 32-bit Android takes the portable path. Its off_t is 32 bits, so nix's
    // pwritev refuses any offset past 2 GiB; pwrite_all is std's pwrite64 there.
    #[cfg(all(unix, not(all(target_os = "android", target_pointer_width = "32"))))]
    fn pwrite_all_vectored(&self, offset: u64, bufs: [IoSlice<'_>; 2]) -> anyhow::Result<usize> {
        nix::sys::uio::pwritev(self, &bufs, offset.try_into()?).context("error calling pwritev")
    }

    #[cfg(not(all(unix, not(all(target_os = "android", target_pointer_width = "32")))))]
    fn pwrite_all_vectored(&self, offset: u64, bufs: [IoSlice<'_>; 2]) -> anyhow::Result<usize> {
        portable_pwrite_all_vectored(self, offset, bufs)
    }

    #[cfg(unix)]
    fn pread_exact(&self, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        use std::os::unix::fs::FileExt;

        Ok(self.read_exact_at(buf, offset)?)
    }

    #[cfg(windows)]
    fn pread_exact(&self, mut offset: u64, mut buf: &mut [u8]) -> anyhow::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            let n = self.seek_read(buf, offset)?;
            if n == 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof").into());
            }
            offset += n as u64;
            buf = &mut buf[n..];
        }
        Ok(())
    }

    #[cfg(not(any(windows, unix)))]
    fn pread_exact(&self, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        anyhow::bail!("pread_exact not implemented for your platform")
    }

    #[cfg(unix)]
    fn pwrite_all(&self, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        use std::os::unix::fs::FileExt;
        Ok(self.write_all_at(buf, offset)?)
    }

    #[cfg(windows)]
    fn pwrite_all(&self, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        use std::os::windows::fs::FileExt;

        let mut remaining = buf.len();
        let mut buf = buf;
        let mut offset = offset;
        while remaining > 0 {
            let written = self.seek_write(&buf[..remaining], offset)?;
            remaining -= written;
            offset += written as u64;
            buf = &buf[written..];
        }
        Ok(())
    }

    #[cfg(not(any(windows, unix)))]
    fn pwrite_all(&self, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        anyhow::bail!("pwrite_all not implemented for your platform")
    }
}

/// The write without pwritev: through `pwrite_all` (std's pwrite64 on unix), one call for both
/// buffers. Marquee fork: 32-bit Android and the platforms without unix take it; tests run it on
/// every target, so the 32-bit path is checked on a 64-bit host too.
#[cfg(any(
    test,
    not(all(unix, not(all(target_os = "android", target_pointer_width = "32"))))
))]
fn portable_pwrite_all_vectored(
    file: &File,
    offset: u64,
    bufs: [IoSlice<'_>; 2],
) -> anyhow::Result<usize> {
    match (bufs[0].len(), bufs[1].len()) {
        (len, 0) if len > 0 => {
            file.pwrite_all(offset, &bufs[0])?;
            Ok(len)
        }
        (0, len) if len > 0 => {
            file.pwrite_all(offset, &bufs[1])?;
            Ok(len)
        }
        (0, 0) => Ok(0),
        (l0, l1) => {
            // concatenate the buffers in memory so that we issue one write call instead of 2
            // assumes the message is <= CHUNK_SIZE
            use librqbit_core::constants::CHUNK_SIZE;
            let mut buf = [0u8; CHUNK_SIZE as usize];

            buf.get_mut(..l0)
                .context("buf too small")?
                .copy_from_slice(&bufs[0]);
            buf.get_mut(l0..l0 + l1)
                .context("buf too small")?
                .copy_from_slice(&bufs[1]);
            file.pwrite_all(offset, &buf[..l0 + l1])?;
            Ok(l0 + l1)
        }
    }
}

#[derive(Default, Debug)]
struct OpenedFileLocked {
    path: PathBuf,
    fd: Option<File>,
    #[cfg(windows)]
    tried_marking_sparse: bool,
}

impl Deref for OpenedFileLocked {
    type Target = Option<File>;

    fn deref(&self) -> &Self::Target {
        &self.fd
    }
}

impl DerefMut for OpenedFileLocked {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.fd
    }
}

#[derive(Debug)]
pub(crate) struct OpenedFile {
    file: RwLock<OpenedFileLocked>,
}

impl OpenedFile {
    pub fn new(path: PathBuf, f: File) -> Self {
        Self {
            file: RwLock::new(OpenedFileLocked {
                path,
                fd: Some(f),
                #[cfg(windows)]
                tried_marking_sparse: false,
            }),
        }
    }

    pub fn new_dummy() -> Self {
        Self {
            file: RwLock::new(Default::default()),
        }
    }

    pub fn take_clone(&self) -> anyhow::Result<Self> {
        let f = std::mem::take(&mut *self.file.write());
        Ok(Self {
            file: RwLock::new(f),
        })
    }

    pub fn lock_read(&self) -> crate::Result<impl Deref<Target = File>> {
        RwLockReadGuard::try_map(self.file.read(), |f| f.as_ref())
            .ok()
            .ok_or(Error::FsFileIsNone)
    }

    /// Marquee fork (plans/streaming-window.md 2.5): see
    /// [`crate::storage::TorrentStorage::punch`].
    pub fn punch(&self, offset: u64, len: u64) -> anyhow::Result<std::ops::Range<u64>> {
        let g = self.file.read();
        let f = g.fd.as_ref().ok_or(Error::FsFileIsNone)?;
        Ok(super::punch::punch_file(f, &g.path, offset, len)?)
    }

    #[allow(dead_code)]
    pub fn lock_write(&self) -> crate::Result<impl DerefMut<Target = File>> {
        RwLockWriteGuard::try_map(self.file.write(), |f| f.as_mut())
            .ok()
            .ok_or(Error::FsFileIsNone)
    }

    #[cfg(windows)]
    pub fn try_mark_sparse(&self) -> crate::Result<impl Deref<Target = File>> {
        {
            let g = self.file.read();
            if g.tried_marking_sparse {
                return RwLockReadGuard::try_map(g, |f| f.fd.as_ref())
                    .ok()
                    .ok_or(Error::FsFileIsNone);
            }
        }
        let mut g = self.file.write();
        if !g.tried_marking_sparse {
            g.tried_marking_sparse = true;
            let f = g.fd.as_ref().ok_or(Error::FsFileIsNone)?;
            tracing::debug!(path=?g.path, marked=super::sparse::mark_file_sparse(f), "marking sparse");
        }
        let g = parking_lot::RwLockWriteGuard::downgrade(g);
        Ok(RwLockReadGuard::try_map(g, |f| f.fd.as_ref()).ok().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{IoSlice, Read};

    use librqbit_core::constants::CHUNK_SIZE;
    use peer_binary_protocol::DoubleBufHelper;
    use tempfile::TempDir;

    use crate::storage::filesystem::opened_file::{OurFileExt, portable_pwrite_all_vectored};

    #[test]
    fn test_pwrite_all_vectored_past_4_gib() {
        // Marquee fork: a sparse file, so only a block is written; on a 32-bit device the
        // offset must reach the kernel whole (off_t there is 32 bits).
        let td = TempDir::with_prefix("test_pwrite_past_4_gib").unwrap();
        let path = td.path().join("sparse");
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let offset = 5u64 << 30;
        let bufs = [IoSlice::new(b"ab"), IoSlice::new(b"cd")];
        assert_eq!(file.pwrite_all_vectored(offset, bufs).unwrap(), 4);
        let mut back = [0u8; 4];
        file.pread_exact(offset, &mut back).unwrap();
        assert_eq!(&back, b"abcd");
        assert_eq!(file.metadata().unwrap().len(), offset + 4);
    }

    #[test]
    fn test_portable_pwrite_past_4_gib() {
        // Marquee fork: the path 32-bit Android takes, run here on any host. Sparse files, each
        // buffer shape at 5 GiB, read back whole.
        let td = TempDir::with_prefix("test_portable_past_4_gib").unwrap();
        let offset = 5u64 << 30;
        let shapes: [(&[u8], &[u8]); 3] = [(b"abcd", b""), (b"", b"abcd"), (b"ab", b"cd")];
        for (i, (first, second)) in shapes.into_iter().enumerate() {
            let path = td.path().join(format!("sparse_{i}"));
            let file = std::fs::OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            let bufs = [IoSlice::new(first), IoSlice::new(second)];
            assert_eq!(
                portable_pwrite_all_vectored(&file, offset, bufs).unwrap(),
                4
            );
            let mut back = [0u8; 4];
            file.pread_exact(offset, &mut back).unwrap();
            assert_eq!(&back, b"abcd", "shape {i}");
            assert_eq!(file.metadata().unwrap().len(), offset + 4, "shape {i}");
        }
    }

    #[test]
    fn test_pwrite_all_vectored() {
        let td = TempDir::with_prefix("test_pwrite_all_vectored").unwrap();
        let mut tmp_buf = [0u8; CHUNK_SIZE as usize];
        for bufsize in [10000usize, CHUNK_SIZE as usize] {
            let mut buf = vec![0u8; bufsize];
            rand::fill(&mut buf[..]);
            for split_point in [0, bufsize / 2, bufsize] {
                let path = td.path().join(format!("file_{bufsize}_{split_point}"));
                let file = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)
                    .unwrap();
                let (first, second) = buf.split_at(split_point);
                let bufs = DoubleBufHelper::new(first, second).as_ioslices(bufsize);
                file.pwrite_all_vectored(0, bufs).unwrap();

                let mut file = std::fs::File::open(&path).unwrap();
                assert_eq!(file.metadata().unwrap().len(), bufsize as u64, "{path:?}");
                file.read_exact(&mut tmp_buf[..bufsize]).unwrap();
                assert_eq!(&tmp_buf[..bufsize], buf);
            }
        }
    }
}
