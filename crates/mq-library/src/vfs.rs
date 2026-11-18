//! webOS (plans/webos.md 3.3): a SQLite VFS over the service's synchronous
//! file calls (`mqHost.fs*`, `apps/webos/service/host.d.ts`). The main
//! database and its rollback journal live on disk; every other file (temp
//! tables, sorter spill) stays in memory. One process holds one connection
//! (the core child), so locking is a no-op, as in sqlite-wasm-rs's memvfs.
//! Registered as `mq-host` and never made the default, so a database opened
//! without naming it (`:memory:`, Incognito) stays off the disk.

use rusqlite::ffi;
use rusqlite::ffi::WasmOsCallback;
use rusqlite::ffi::utils::ffi::sqlite3_vfs;
use rusqlite::ffi::utils::{
    MemChunksFile, OsCallback, SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, VfsError, VfsFile,
    VfsResult, VfsStore, register_vfs, registered_vfs,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;
use wasm_bindgen::prelude::wasm_bindgen;

pub(crate) const NAME: &str = "mq-host";

/// The host's file calls. They never throw: a failure is -1 or false.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsOpen)]
    fn fs_open(path: &str, create: bool) -> i32;
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsExists)]
    fn fs_exists(path: &str) -> bool;
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsRead)]
    fn fs_read(fd: i32, buf: &mut [u8], offset: f64) -> i32;
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsWrite)]
    fn fs_write(fd: i32, buf: &[u8], offset: f64) -> bool;
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsSync)]
    fn fs_sync(fd: i32) -> bool;
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsTruncate)]
    fn fs_truncate(fd: i32, size: f64) -> bool;
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsSize)]
    fn fs_size(fd: i32) -> f64;
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsClose)]
    fn fs_close(fd: i32);
    #[wasm_bindgen(js_namespace = mqHost, js_name = fsUnlink)]
    fn fs_unlink(path: &str) -> bool;
}

pub(crate) enum HostFile {
    Disk(i32),
    Mem(MemChunksFile),
}

fn io(code: i32, what: &str) -> VfsError {
    VfsError::new(code, what.to_string())
}

impl VfsFile for HostFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<bool> {
        match self {
            HostFile::Mem(f) => f.read(buf, offset),
            HostFile::Disk(fd) => {
                let got = fs_read(*fd, buf, offset as f64);
                // A failed read is an error, never a short read of zeros.
                let Ok(got) = usize::try_from(got) else {
                    return Err(io(ffi::SQLITE_IOERR_READ, "read failed"));
                };
                if got < buf.len() {
                    // SQLite wants the unread tail zeroed on a short read.
                    buf[got..].fill(0);
                    return Ok(false);
                }
                Ok(true)
            }
        }
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        match self {
            HostFile::Mem(f) => f.write(buf, offset),
            HostFile::Disk(fd) if fs_write(*fd, buf, offset as f64) => Ok(()),
            HostFile::Disk(_) => Err(io(ffi::SQLITE_IOERR_WRITE, "write failed")),
        }
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        match self {
            HostFile::Mem(f) => f.truncate(size),
            HostFile::Disk(fd) if fs_truncate(*fd, size as f64) => Ok(()),
            HostFile::Disk(_) => Err(io(ffi::SQLITE_IOERR_TRUNCATE, "truncate failed")),
        }
    }

    fn flush(&mut self) -> VfsResult<()> {
        match self {
            HostFile::Mem(f) => f.flush(),
            HostFile::Disk(fd) if fs_sync(*fd) => Ok(()),
            HostFile::Disk(_) => Err(io(ffi::SQLITE_IOERR_FSYNC, "fsync failed")),
        }
    }

    fn size(&self) -> VfsResult<usize> {
        match self {
            HostFile::Mem(f) => f.size(),
            HostFile::Disk(fd) => {
                let size = fs_size(*fd);
                if size < 0.0 {
                    return Err(io(ffi::SQLITE_IOERR_FSTAT, "fstat failed"));
                }
                Ok(size as usize)
            }
        }
    }
}

type AppData = RefCell<HashMap<String, HostFile>>;

pub(crate) struct HostStore;

fn on_disk(flags: i32) -> bool {
    flags & (ffi::SQLITE_OPEN_MAIN_DB | ffi::SQLITE_OPEN_MAIN_JOURNAL) != 0
}

impl VfsStore<HostFile, AppData> for HostStore {
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) -> VfsResult<()> {
        let entry = if on_disk(flags) {
            let fd = fs_open(file, true);
            if fd < 0 {
                return Err(io(ffi::SQLITE_CANTOPEN, file));
            }
            HostFile::Disk(fd)
        } else {
            HostFile::Mem(MemChunksFile::default())
        };
        unsafe { Self::app_data(vfs) }
            .borrow_mut()
            .insert(file.into(), entry);
        Ok(())
    }

    /// Also opens a file that is on disk but not yet known to this process
    /// (the database after a restart, a hot journal after a cut).
    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<bool> {
        let data = unsafe { Self::app_data(vfs) };
        if data.borrow().contains_key(file) {
            return Ok(true);
        }
        if !fs_exists(file) {
            return Ok(false);
        }
        let fd = fs_open(file, false);
        if fd < 0 {
            return Err(io(ffi::SQLITE_CANTOPEN, file));
        }
        data.borrow_mut().insert(file.into(), HostFile::Disk(fd));
        Ok(true)
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<()> {
        let removed = unsafe { Self::app_data(vfs) }.borrow_mut().remove(file);
        match removed {
            Some(HostFile::Disk(fd)) => {
                fs_close(fd);
                fs_unlink(file);
            }
            Some(HostFile::Mem(_)) => {}
            None if fs_exists(file) => {
                fs_unlink(file);
            }
            None => return Err(io(ffi::SQLITE_IOERR_DELETE, file)),
        }
        Ok(())
    }

    fn with_file<F: Fn(&HostFile) -> VfsResult<i32>>(f: &SQLiteVfsFile, op: F) -> VfsResult<i32> {
        let name = unsafe { f.name() };
        match unsafe { Self::app_data(f.vfs) }.borrow().get(name) {
            Some(file) => op(file),
            None => Err(io(ffi::SQLITE_IOERR, name)),
        }
    }

    fn with_file_mut<F: Fn(&mut HostFile) -> VfsResult<i32>>(
        f: &SQLiteVfsFile,
        op: F,
    ) -> VfsResult<i32> {
        let name = unsafe { f.name() };
        match unsafe { Self::app_data(f.vfs) }.borrow_mut().get_mut(name) {
            Some(file) => op(file),
            None => Err(io(ffi::SQLITE_IOERR, name)),
        }
    }
}

pub(crate) struct HostIo;

impl SQLiteIoMethods for HostIo {
    type File = HostFile;
    type AppData = AppData;
    type Store = HostStore;
    const VERSION: std::ffi::c_int = 1;
}

pub(crate) struct HostVfs;

/// Sleep, randomness and the clock are sqlite-wasm-rs's own (as memvfs's).
impl SQLiteVfs<HostIo> for HostVfs {
    const VERSION: std::ffi::c_int = 1;

    fn sleep(dur: Duration) {
        WasmOsCallback::sleep(dur);
    }

    fn random(buf: &mut [u8]) {
        WasmOsCallback::random(buf);
    }

    fn epoch_timestamp_in_ms() -> i64 {
        WasmOsCallback::epoch_timestamp_in_ms()
    }
}

/// Registers `mq-host` once per process (not as the default VFS).
pub(crate) fn install() -> rusqlite::Result<()> {
    let failed =
        |e: String| rusqlite::Error::SqliteFailure(ffi::Error::new(ffi::SQLITE_CANTOPEN), Some(e));
    if registered_vfs(NAME)
        .map_err(|e| failed(format!("{e:?}")))?
        .is_some()
    {
        return Ok(());
    }
    register_vfs::<HostIo, HostVfs>(NAME, AppData::default(), false)
        .map(|_| ())
        .map_err(|e| failed(format!("{e:?}")))
}
