//! Incognito (plans/batch-10-incognito.md 2.3): each entry gets a throwaway
//! scope, its own SQLite file in `<data_dir>/incognito/<16 hex>/` and its own
//! addon client. While a handle carries a scope, `Core::library` and
//! `Core::addons` are the scope's, so every profile write lands there and
//! goes with the folder at the end. Device work starts from `Core::device`.

use crate::Core;
use mq_addons::{AddonClient, ResponseCache};
pub use mq_library::INCOGNITO;
use mq_library::Library;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Free space under which no scope opens (`noSpace`).
const MIN_FREE: u64 = 16 << 20;

/// One Incognito entry. Strong handles keep it readable; `ended` fences off
/// every `WeakCore` and the torrent record step, `Library::close` every
/// late write.
pub(crate) struct Scope {
    /// 16 hex: the folder's name and the owner of its torrent lines.
    pub(crate) id: String,
    pub(crate) library: Arc<Library>,
    pub(crate) addons: AddonClient,
    /// The profile it copied; device work runs as this one.
    pub(crate) host: String,
    dir: PathBuf,
    /// Held for the scope's life: a folder whose lock can be taken belongs
    /// to no live scope.
    _lock: std::fs::File,
    pub(crate) ended: AtomicBool,
    /// The device's count of live scopes (`liveScopes`).
    live: Arc<std::sync::atomic::AtomicUsize>,
    /// Taken by the torrent record step and by the end, so no line is
    /// written after the end's pass.
    pub(crate) record: std::sync::Mutex<()>,
}

impl Drop for Scope {
    /// A backstop only: `end` removed the folder and counted down already.
    /// A scope dropped without its end does both here, so `liveScopes`
    /// never counts one that is gone. Never touches `incognito/` itself.
    fn drop(&mut self) {
        if !self.ended.swap(true, Ordering::SeqCst) {
            self.live.fetch_sub(1, Ordering::SeqCst);
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Why no scope opened.
#[derive(Debug, thiserror::Error)]
pub enum IncognitoError {
    /// A kids profile never enters Incognito.
    #[error("a kids profile cannot open Incognito")]
    Kids,
    /// The words stay neutral; the code says why (`noSpace`).
    #[error("Incognito could not start")]
    NoSpace,
    #[error("Incognito could not start: {0}")]
    Failed(String),
}

impl IncognitoError {
    /// The protocol's code (plans/batch-10-incognito.md 2.8).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Kids => "kids",
            Self::NoSpace => "noSpace",
            Self::Failed(_) => "incognitoFailed",
        }
    }
}

impl From<std::io::Error> for IncognitoError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::StorageFull {
            Self::NoSpace
        } else {
            Self::Failed(e.to_string())
        }
    }
}

impl From<mq_library::LibraryError> for IncognitoError {
    fn from(e: mq_library::LibraryError) -> Self {
        Self::Failed(e.to_string())
    }
}

impl Core {
    /// Whether this handle runs Incognito.
    pub fn incognito(&self) -> bool {
        self.scope().is_some()
    }

    /// The profile this handle's Incognito copied.
    pub fn incognito_host(&self) -> Option<&str> {
        self.scope().map(|s| s.host.as_str())
    }

    /// Opens a new scope over this handle's profile, the host
    /// (plans/batch-10-incognito.md 2.3): its addons and their order, its
    /// plugin repositories and its settings are copied. The handle answered
    /// runs as `incognito`.
    pub fn open_incognito(&self) -> Result<Core, IncognitoError> {
        let host = self.device();
        if host.policy().kids() {
            return Err(IncognitoError::Kids);
        }
        let from = host.library();
        let profile = host.profile();
        if from.addon_list_hlc(profile)?.is_none() {
            return Err(IncognitoError::Failed(
                "the profile has no addon list".into(),
            ));
        }
        if host.free_bytes().is_some_and(|free| free < MIN_FREE) {
            return Err(IncognitoError::NoSpace);
        }
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("os randomness");
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let dir = self.incognito_dir().join(&id);
        std::fs::create_dir_all(&dir)?;
        let opened = (|| {
            let lock = std::fs::File::create(dir.join("lock"))?;
            lock.try_lock()
                .map_err(|e| IncognitoError::Failed(e.to_string()))?;
            let library = Library::temporary_at(dir.join("library.sqlite"))?;
            // Before the first write: nothing here is ever queued (wall 2).
            library.drop_outbox()?;
            library.ensure_profile(INCOGNITO, "Incognito", &self.zero_hlc())?;
            library.set_addons(INCOGNITO, &from.addons(profile)?, self.clock())?;
            for repo in from.plugin_repos(profile)? {
                library.put_plugin_repo(INCOGNITO, &repo.url, repo.enabled)?;
                if let Some(manifest) = &repo.manifest_json {
                    library.set_plugin_manifest(INCOGNITO, &repo.url, manifest, repo.fetched_at)?;
                }
                for off in &repo.off {
                    library.set_plugin_off(INCOGNITO, &repo.url, off, true)?;
                }
            }
            for (key, value) in from.settings(profile)? {
                library.set_setting(INCOGNITO, &key, &value, self.clock())?;
            }
            Ok::<_, IncognitoError>((lock, library))
        })();
        let (lock, library) = match opened {
            Ok(opened) => opened,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e);
            }
        };
        let library = Arc::new(library);
        let store = Arc::new(crate::core::LibraryCache(library.clone()));
        let addons =
            AddonClient::with_cache(self.http().clone(), ResponseCache::with_store(512, store));
        let scope = Scope {
            id,
            library,
            addons,
            host: profile.to_owned(),
            dir,
            _lock: lock,
            ended: AtomicBool::new(false),
            live: self.live_scopes().clone(),
            record: std::sync::Mutex::new(()),
        };
        self.live_scopes().fetch_add(1, Ordering::SeqCst);
        Ok(self.with_scope(Arc::new(scope)))
    }

    /// Ends this handle's scope (plans/batch-10-incognito.md 2.3): later
    /// writes are refused, the folder goes at once, and nothing holding a
    /// `WeakCore` of it runs again. True when this call ended it; any later
    /// call does nothing. Needs no async runtime: a dropped session runs it.
    pub fn end_incognito(&self) -> bool {
        let Some(scope) = self.scope() else {
            return false;
        };
        let _record = scope.record.lock().unwrap_or_else(|p| p.into_inner());
        if scope.ended.swap(true, Ordering::SeqCst) {
            return false;
        }
        scope.library.close();
        self.live_scopes().fetch_sub(1, Ordering::SeqCst);
        if let Err(e) = std::fs::remove_dir_all(&scope.dir) {
            tracing::warn!("an Incognito folder stayed: {e}");
        }
        drop(_record);
        // Its torrents go now, or at the next launch without a runtime.
        self.sweep_torrents_soon();
        true
    }
}

/// Removes every scope folder no live scope holds (launch recovery,
/// plans/batch-10-incognito.md 2.3): one whose lock can be taken, or that
/// has none. `incognito/` itself stays. True when any went.
pub(crate) fn recover(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut removed = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || live(&path) {
            continue;
        }
        removed |= std::fs::remove_dir_all(&path).is_ok();
    }
    removed
}

/// A scope folder that exists and whose lock another handle holds.
pub(crate) fn live(folder: &Path) -> bool {
    std::fs::File::open(folder.join("lock")).is_ok_and(|lock| lock.try_lock().is_err())
}

/// The device's list of torrents Incognito created, one `<scope id> <hash>`
/// line each (plans/batch-10-incognito.md 2.5), beside the scope folders.
const TORRENTS: &str = "torrents";

/// One reader or writer of a torrent list at a time in this process.
static LIST: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn list_lock() -> std::sync::MutexGuard<'static, ()> {
    LIST.lock().unwrap_or_else(|p| p.into_inner())
}

/// A torrent's info hash as the engine names its folder: 40 lowercase hex.
/// Nothing else is ever joined into a path.
fn is_hash(hash: &str) -> bool {
    hash.len() == 40 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A scope folder's name: 16 lowercase hex.
fn is_scope(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The list's lines; a malformed one is dropped (and logged), never used.
fn read_list(dir: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(dir.join(TORRENTS)).unwrap_or_default();
    text.lines()
        .filter_map(|line| match line.split_once(' ') {
            Some((scope, hash)) if is_scope(scope) && is_hash(hash) => {
                Some((scope.to_owned(), hash.to_owned()))
            }
            _ => {
                tracing::warn!("a malformed Incognito torrent line was dropped");
                None
            }
        })
        .collect()
}

/// Takes `done` out of the list, keeping lines added meanwhile; a
/// malformed line goes too.
fn drop_lines(dir: &Path, done: &[(String, String)]) {
    let _list = list_lock();
    if !dir.join(TORRENTS).exists() {
        return;
    }
    let left: String = read_list(dir)
        .into_iter()
        .filter(|line| !done.contains(line))
        .map(|(scope, hash)| format!("{scope} {hash}\n"))
        .collect();
    // Written aside then renamed: a kill mid-write loses no line.
    let aside = dir.join(format!("{TORRENTS}.new"));
    let written =
        std::fs::write(&aside, left).and_then(|()| std::fs::rename(&aside, dir.join(TORRENTS)));
    if let Err(e) = written {
        tracing::warn!("the Incognito torrent list was not written: {e}");
    }
}

/// The lines whose scope is no longer live: ended, or of a process that
/// died. A live scope of another process keeps its folder and lock.
fn ended_lines(dir: &Path) -> Vec<(String, String)> {
    let lines = {
        let _list = list_lock();
        read_list(dir)
    };
    lines
        .into_iter()
        .filter(|(scope, _)| !live(&dir.join(scope)))
        .collect()
}

/// Deletes a torrent's folder, `.have` and `.torrent` as the engine does:
/// the folder set aside first, then removed.
pub(crate) fn delete_torrent_files(torrent_dir: &Path, hash: &str) {
    if !is_hash(hash) {
        return;
    }
    let folder = torrent_dir.join(hash);
    // The engine's endings (mq-engine `HAVE`, `TORRENT`, `SET_ASIDE`).
    let _ = std::fs::remove_file(folder.with_extension("have"));
    let _ = std::fs::remove_file(folder.with_extension("torrent"));
    let aside = folder.with_extension("evicted");
    let _ = std::fs::remove_dir_all(&aside);
    let gone = if std::fs::rename(&folder, &aside).is_ok() {
        aside
    } else {
        folder
    };
    let _ = std::fs::remove_dir_all(gone);
}

/// Launch recovery's torrent pass, before the engine starts: every line no
/// live scope owns has its files deleted, unless a download names its
/// torrent, and goes.
pub(crate) fn recover_torrents(dir: &Path, torrent_dir: &Path, device: &Library) {
    let ended = ended_lines(dir);
    for (_, hash) in &ended {
        if !crate::downloads::names_torrent(device, hash) {
            delete_torrent_files(torrent_dir, hash);
        }
    }
    drop_lines(dir, &ended);
}

/// What the record step answered.
#[derive(Debug, PartialEq, Eq)]
pub enum Record {
    /// The scope ended: no add.
    Ended,
    /// Recorded: the handle the add gives is kept for the removal.
    Recorded,
    /// Not Incognito's (outside it, cached before, or a download's).
    Not,
}

#[cfg_attr(not(feature = "torrent"), allow(dead_code))]
impl Core {
    /// Before a torrent's add (plans/batch-10-incognito.md 2.5): in
    /// Incognito, `hash` (lowercase hex) is recorded as the scope's when no
    /// folder of it is here and no download names it. Under the record
    /// mutex `end_incognito` takes, so after the end nothing is recorded or
    /// added.
    #[doc(hidden)]
    pub fn record_torrent(&self, hash: &str) -> Record {
        let Some(scope) = self.scope() else {
            return Record::Not;
        };
        let _record = scope.record.lock().unwrap_or_else(|p| p.into_inner());
        if scope.ended.load(Ordering::SeqCst) {
            return Record::Ended;
        }
        if !is_hash(hash) {
            tracing::warn!("a torrent with a malformed hash was not recorded");
            return Record::Not;
        }
        // Kept here before (its folder, or its details the engine loads
        // first): never Incognito's.
        let folder = self.torrent_dir().join(hash);
        let kept = [
            folder.clone(),
            folder.with_extension("torrent"),
            folder.with_extension("have"),
        ];
        if kept.iter().any(|p| p.exists())
            || crate::downloads::names_torrent(self.device_library(), hash)
        {
            return Record::Not;
        }
        self.append_line(&scope.id, hash);
        Record::Recorded
    }

    fn append_line(&self, scope: &str, hash: &str) {
        let _list = list_lock();
        let line = format!("{scope} {hash}\n");
        let path = self.incognito_dir().join(TORRENTS);
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
        if let Err(e) = written {
            tracing::warn!("an Incognito torrent was not recorded: {e}");
        }
    }

    /// After the add of a torrent `record_torrent` recorded: its handle is
    /// kept for the removal. An end that came while the add ran (a Leave
    /// while a stream starts) has swept the line already, so the line is
    /// written again and a pass runs: the torrent never outlives the scope.
    #[cfg(feature = "torrent")]
    #[doc(hidden)]
    pub fn added_torrent(&self, hash: &str, handle: mq_engine::TorrentHandle) {
        let Some(scope) = self.scope() else {
            return;
        };
        let _record = scope.record.lock().unwrap_or_else(|p| p.into_inner());
        self.incognito_handles().insert(hash.to_owned(), handle);
        if scope.ended.load(Ordering::SeqCst) {
            self.append_line(&scope.id, hash);
            drop(_record);
            self.sweep_torrents_soon();
        }
    }

    /// Works through every line no live scope owns, at once and then every
    /// 60 seconds while any is left (a torrent still claimed by a stream
    /// stays for the next pass). One pass loop per device; it ends with
    /// the runtime.
    pub(crate) fn sweep_torrents_soon(&self) {
        if tokio::runtime::Handle::try_current().is_err()
            || self.sweeping().swap(true, Ordering::SeqCst)
        {
            return;
        }
        let device = self.device().downgrade();
        tokio::spawn(async move {
            // The first retry comes soon: a Leave from the player drops the
            // stream's claim a moment after the end's pass.
            let mut wait = SWEEP_SOON;
            loop {
                let Some(core) = device.upgrade() else {
                    return;
                };
                let left = core.sweep_torrents().await;
                if left == 0 {
                    if core.sweep_done() {
                        return;
                    }
                    continue;
                }
                drop(core);
                tokio::time::sleep(wait).await;
                wait = SWEEP_EVERY;
            }
        });
    }

    /// The pass loop is about to stop: true when it may. An end that came
    /// while it finished found the flag still set and started no loop, so
    /// its lines are looked for once more here.
    #[doc(hidden)]
    pub fn sweep_done(&self) -> bool {
        self.sweeping().store(false, Ordering::SeqCst);
        ended_lines(self.incognito_dir()).is_empty() || self.sweeping().swap(true, Ordering::SeqCst)
    }

    /// One pass over the lines no live scope owns; how many are left.
    #[doc(hidden)]
    pub async fn sweep_torrents(&self) -> usize {
        let dir = self.incognito_dir();
        let ended = ended_lines(dir);
        let mut done = Vec::new();
        for line in &ended {
            if self.remove_torrent(&line.1).await {
                done.push(line.clone());
            }
        }
        drop_lines(dir, &done);
        ended.len() - done.len()
    }

    /// Removes torrent `hash` Incognito created; false while a stream still
    /// claims it. A download's torrent keeps its files.
    async fn remove_torrent(&self, hash: &str) -> bool {
        if crate::downloads::names_torrent(self.device_library(), hash) {
            return true;
        }
        #[cfg(feature = "torrent")]
        if let Some(engine) = self.running_engine() {
            let handle = self.incognito_handles().remove(hash);
            if let Some(handle) = handle {
                match engine.remove(handle, false).await {
                    Ok(()) => return true,
                    Err(mq_engine::EngineError::Claimed) => {
                        self.incognito_handles().insert(hash.to_owned(), handle);
                        return false;
                    }
                    // Forgotten with its folder kept (a look-ahead's own
                    // cleanup): the files go as recovery deletes them.
                    Err(_) => {}
                }
            }
        }
        // ponytail: a torrent another profile's playback loaded after the
        // record would lose its folder here; the record only takes hashes
        // with no folder, so that needs the same new title on both at once.
        delete_torrent_files(self.torrent_dir(), hash);
        true
    }
}

/// The first retry after an end.
const SWEEP_SOON: std::time::Duration = std::time::Duration::from_secs(2);
/// A torrent line still claimed is tried again this often.
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    async fn scope() -> (Core, Core) {
        let host = Core::temporary(mq_library::OWNER).unwrap();
        let row = mq_library::AddonRow {
            transport_url: "https://addon.example/manifest.json".into(),
            manifest_json: r#"{"id":"x","version":"1.0.0","name":"X","resources":[],"types":[]}"#
                .into(),
            position: 0,
            enabled: true,
            allow_private: false,
        };
        host.library()
            .set_addons(mq_library::OWNER, &[row], host.clock())
            .unwrap();
        let core = host.open_incognito().unwrap();
        (host, core)
    }

    /// A `WeakCore` taken in Incognito upgrades no more once it ended
    /// (7.1): background work of the scope stops there.
    #[tokio::test]
    async fn a_weak_handle_of_an_ended_scope_upgrades_to_none() {
        let (_host, core) = scope().await;
        let weak = core.downgrade();
        assert!(weak.upgrade().is_some_and(|c| c.incognito()));
        assert!(core.end_incognito());
        assert!(weak.upgrade().is_none());
        assert!(!core.end_incognito(), "one end");
    }

    /// Knowledge reads the device and the scope merged; writes go to the
    /// scope, so the device's is the same after (7.1).
    #[tokio::test]
    async fn knowledge_is_the_device_s_and_the_scope_s() {
        let (host, core) = scope().await;
        let now = mq_library::hlc::now_ms();
        let (old, new) = ("a".repeat(40), "b".repeat(40));
        crate::speed::record_dead(host.library(), &old, now);
        crate::speed::record_dead(core.library(), &new, now);
        let dead = core.knowledge(|l| crate::speed::dead_swarms(l, now));
        assert!(dead.contains(&old) && dead.contains(&new), "{dead:?}");
        core.end_incognito();
        assert_eq!(crate::speed::dead_swarms(host.library(), now), [old]);
    }

    /// Incognito never starts a Trakt scrobble, nor Trakt itself.
    #[tokio::test]
    async fn incognito_never_scrobbles() {
        let (host, core) = scope().await;
        let target = crate::trakt::ScrobbleTarget {
            kind: "movie".into(),
            item_id: "tt1".into(),
            video_id: "tt1".into(),
            name: "Title".into(),
        };
        assert!(core.trakt_scrobble(target, Box::new(|_| {})).is_none());
        assert!(host.inner_trakt_unstarted());
        core.end_incognito();
    }

    /// No handle for another profile is ever Incognito.
    #[tokio::test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "assertion `left != right` failed")]
    async fn for_profile_never_makes_incognito() {
        let (host, core) = scope().await;
        core.end_incognito();
        let _ = host.for_profile(INCOGNITO);
    }
}
