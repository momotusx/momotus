//! Marquee fork (plans/streaming-window.md 2.6): freeing a file's blocks
//! while it keeps its length, so a streaming window gives space back.
//!
//! - Apple (APFS): `fcntl(F_PUNCHHOLE)`, block aligned or EINVAL; HFS+
//!   and exFAT answer ENOTTY.
//! - Linux and Android: `fallocate(PUNCH_HOLE | KEEP_SIZE)`, which zeroes
//!   partial blocks, so the range is rounded inward to whole blocks on
//!   every platform. vfat and exfat answer EOPNOTSUPP.
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
    #[cfg(feature = "sync-log")]
    if crate::bitv::sync_log::failing(path, crate::bitv::sync_log::Fail::Punch) {
        return Err(io::Error::from(io::ErrorKind::Unsupported));
    }
    let block = block_size(file)?;
    let start = offset.div_ceil(block) * block;
    let end = (offset + len) / block * block;
    if end <= start {
        return Ok(start..start);
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

#[cfg(not(unix))]
fn block_size(_file: &File) -> io::Result<u64> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

fn off(v: u64) -> io::Result<i64> {
    i64::try_from(v).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

#[cfg(target_vendor = "apple")]
fn sys_punch(file: &File, offset: u64, len: u64) -> io::Result<()> {
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
fn sys_punch(file: &File, offset: u64, len: u64) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let mode = libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE;
    // SAFETY: fallocate takes plain values.
    #[cfg(target_os = "android")]
    let r = unsafe { libc::fallocate64(file.as_raw_fd(), mode, off(offset)?, off(len)?) };
    #[cfg(target_os = "linux")]
    let r = unsafe { libc::fallocate(file.as_raw_fd(), mode, off(offset)?, off(len)?) };
    if r == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
fn sys_punch(_file: &File, _offset: u64, _len: u64) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Whether files under `dir` give space back when punched: 4 extents of
/// 64 KiB written and synced, the second punched through [`punch_file`] (the
/// path an eviction takes), synced again, and the file's allocated blocks
/// must fall by 64 KiB. Any error, or no fall, is a no.
pub fn punch_supported(dir: &Path) -> bool {
    let path = dir.join(".mq-punch-probe");
    let ok = probe(&path).unwrap_or(false);
    let _ = std::fs::remove_file(&path);
    ok
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
    let freed = punch_file(&file, path, EXTENT, EXTENT)?;
    file.sync_all()?;
    let after = file.metadata()?.blocks() * 512;
    Ok(freed == (EXTENT..2 * EXTENT) && before.saturating_sub(after) >= EXTENT)
}

#[cfg(not(unix))]
fn probe(_path: &Path) -> io::Result<bool> {
    Ok(false)
}
