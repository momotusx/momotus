//! Marquee fork (plans/streaming-window.md 2.6): freeing a file's blocks
//! while it keeps its length, so a streaming window gives space back.
//!
//! - Apple (APFS): `fcntl(F_PUNCHHOLE)`, block aligned or EINVAL; HFS+
//!   and exFAT answer ENOTTY.
//! - Linux and Android: `fallocate(PUNCH_HOLE | KEEP_SIZE)`, which zeroes
//!   partial blocks, so the range is rounded inward to whole blocks on
//!   every platform. vfat and exfat answer EOPNOTSUPP.
//! - Windows (NTFS): `FSCTL_SET_ZERO_DATA` on a file marked sparse, which
//!   frees whole 64 KiB sparse units (16 clusters of 4 KiB) and zeroes the
//!   rest; FAT and exFAT refuse it.
//! - Elsewhere: unsupported.

use std::fs::File;
use std::io;
use std::ops::Range;
use std::path::Path;

/// Frees the whole filesystem blocks inside `offset..offset + len` of
/// `file` (at `path`) and returns the range freed, empty when no whole block
/// fits.
pub(crate) fn punch_file(
    file: &File,
    path: &Path,
    offset: u64,
    len: u64,
) -> io::Result<Range<u64>> {
    let _ = path;
    let block = block_size(file)?;
    let start = offset.div_ceil(block) * block;
    let end = (offset + len) / block * block;
    if end <= start {
        return Ok(start..start);
    }
    // A test's failure punches the first block, then fails: neither call
    // promises all or nothing.
    #[cfg(feature = "sync-log")]
    {
        use crate::bitv::sync_log::{Fail, failing};
        if failing(path, Fail::PunchRefused) {
            return Err(io::Error::from_raw_os_error(libc::EOPNOTSUPP));
        }
        if failing(path, Fail::Punch) {
            sys_punch(file, start, block)?;
            return Err(io::Error::other("injected after one block"));
        }
    }
    sys_punch(file, start, end - start)?;
    #[cfg(feature = "sync-log")]
    crate::bitv::sync_log::push(crate::bitv::sync_log::Event::Punched {
        path: path.to_owned(),
        range: start..end,
    });
    Ok(start..end)
}

#[cfg(unix)]
fn block_size(file: &File) -> io::Result<u64> {
    use std::os::fd::AsRawFd;
    // SAFETY: fstatfs only writes the struct we pass.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(file.as_raw_fd(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(u64::try_from(st.f_bsize)
        .ok()
        .filter(|b| *b > 0)
        .unwrap_or(4096))
}

// ponytail: NTFS frees sparse units of 16 clusters, 64 KiB at the default 4 KiB
// cluster; a volume with larger clusters frees less per punch, and the probe
// and the allocated-size reads keep the accounting true.
#[cfg(windows)]
fn block_size(_file: &File) -> io::Result<u64> {
    Ok(64 << 10)
}

#[cfg(not(any(unix, windows)))]
fn block_size(_file: &File) -> io::Result<u64> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

fn off(v: u64) -> io::Result<i64> {
    i64::try_from(v).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

/// The punch, retried while a signal interrupts it.
fn sys_punch(file: &File, offset: u64, len: u64) -> io::Result<()> {
    loop {
        match punch_once(file, offset, len) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            r => return r,
        }
    }
}

#[cfg(target_vendor = "apple")]
fn punch_once(file: &File, offset: u64, len: u64) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let arg = libc::fpunchhole_t {
        fp_flags: 0,
        reserved: 0,
        fp_offset: off(offset)?,
        fp_length: off(len)?,
    };
    // SAFETY: F_PUNCHHOLE reads the struct we pass.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_PUNCHHOLE, &arg) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn punch_once(file: &File, offset: u64, len: u64) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let mode = libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE;
    // SAFETY: fallocate takes plain values.
    // Marquee fork: 32-bit glibc's `fallocate` takes a 32-bit offset (the webOS TV core).
    #[cfg(any(
        target_os = "android",
        all(target_os = "linux", target_pointer_width = "32")
    ))]
    let r = unsafe { libc::fallocate64(file.as_raw_fd(), mode, off(offset)?, off(len)?) };
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    let r = unsafe { libc::fallocate(file.as_raw_fd(), mode, off(offset)?, off(len)?) };
    if r == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn punch_once(file: &File, offset: u64, len: u64) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        System::{
            IO::DeviceIoControl,
            Ioctl::{FILE_ZERO_DATA_INFORMATION, FSCTL_SET_ZERO_DATA},
        },
    };
    // Zeroing only frees blocks of a sparse file; marking twice is harmless.
    if !super::sparse::mark_file_sparse(file) {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    let end = offset.checked_add(len).ok_or(io::ErrorKind::InvalidInput)?;
    let range = FILE_ZERO_DATA_INFORMATION {
        FileOffset: off(offset)?,
        BeyondFinalZero: off(end)?,
    };
    // SAFETY: a valid handle and an input struct of the size given; no output.
    unsafe {
        DeviceIoControl(
            HANDLE(file.as_raw_handle()),
            FSCTL_SET_ZERO_DATA,
            Some((&raw const range).cast()),
            size_of::<FILE_ZERO_DATA_INFORMATION>() as u32,
            None,
            0,
            None,
            None,
        )
    }
    .map_err(io::Error::from)
}

#[cfg(not(any(
    target_vendor = "apple",
    target_os = "linux",
    target_os = "android",
    windows
)))]
fn punch_once(_file: &File, _offset: u64, _len: u64) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Whether files under `dir` give space back when punched: 4 extents of
/// 64 KiB written and synced, the second punched through [`punch_file`] (the
/// path an eviction takes), synced again, and the file's allocated blocks
/// must fall by 64 KiB. Any error, or no fall, is a no. Each call has a
/// scratch file of its own, so two probes at once (the core's at open and
/// the engine's at start) never truncate each other's.
pub fn punch_supported(dir: &Path) -> bool {
    punch_probe(dir) == Some(true)
}

/// Marquee fork (plans/streaming-window-chunks.md): the probe of
/// [`punch_supported`], with `None` when it could not tell (a write or a
/// sync failed, a full disk): only `Some(false)`, a punch refused or no
/// fall, says the volume cannot punch.
pub fn punch_probe(dir: &Path) -> Option<bool> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = dir.join(format!(".mq-punch-probe-{}-{n}", std::process::id()));
    let answer = probe(&path).ok();
    let _ = std::fs::remove_file(&path);
    answer
}

/// A punch the volume refuses before any work: no support for it.
#[cfg(unix)]
fn refused(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Unsupported
        || matches!(
            e.raw_os_error(),
            Some(libc::EOPNOTSUPP | libc::ENOTTY | libc::EINVAL | libc::ENOSYS)
        )
}

#[cfg(unix)]
fn probe(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::{FileExt, MetadataExt};
    const EXTENT: u64 = 64 << 10;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(path)?;
    for i in 0..4 {
        file.write_all_at(&[0xa5; EXTENT as usize], i * EXTENT)?;
    }
    file.sync_all()?;
    let before = file.metadata()?.blocks() * 512;
    let freed = match punch_file(&file, path, EXTENT, EXTENT) {
        Err(e) if refused(&e) => return Ok(false),
        r => r?,
    };
    file.sync_all()?;
    let after = file.metadata()?.blocks() * 512;
    Ok(freed == (EXTENT..2 * EXTENT) && before.saturating_sub(after) >= EXTENT)
}

/// A punch the volume refuses before any work: FAT and exFAT answer
/// `ERROR_INVALID_FUNCTION` to the sparse calls.
#[cfg(windows)]
fn refused(e: &io::Error) -> bool {
    const ERROR_INVALID_FUNCTION: i32 = 1;
    const ERROR_NOT_SUPPORTED: i32 = 50;
    const ERROR_INVALID_PARAMETER: i32 = 87;
    e.kind() == io::ErrorKind::Unsupported
        || matches!(
            e.raw_os_error(),
            Some(ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED | ERROR_INVALID_PARAMETER)
        )
}

/// The Unix probe's steps, with the allocated size read as
/// [`super::allocated`] reads it.
#[cfg(windows)]
fn probe(path: &Path) -> io::Result<bool> {
    use std::os::windows::fs::FileExt;
    const EXTENT: u64 = 64 << 10;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(path)?;
    let block = [0xa5; EXTENT as usize];
    for i in 0..4 {
        let mut done = 0;
        while done < block.len() {
            done += file.seek_write(&block[done..], i * EXTENT + done as u64)?;
        }
    }
    file.sync_all()?;
    let before = super::allocated(path);
    let freed = match punch_file(&file, path, EXTENT, EXTENT) {
        Err(e) if refused(&e) => return Ok(false),
        r => r?,
    };
    file.sync_all()?;
    let after = super::allocated(path);
    Ok(freed == (EXTENT..2 * EXTENT) && before.saturating_sub(after) >= EXTENT)
}

#[cfg(not(any(unix, windows)))]
fn probe(_path: &Path) -> io::Result<bool> {
    Ok(false)
}
