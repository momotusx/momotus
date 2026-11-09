//! The profile picker (plans/m11-profiles.md section 6): every profile, and
//! the session's active profile and lock state. Refreshes on profile
//! changes (sync included) and on the session's switches and locks.

use crate::Core;
use crate::live::{FRAME, snapshots};
use eyeball::SharedObservable;
use futures_util::Stream;
use mq_library::Table;
use serde::Serialize;
use std::pin::Pin;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    pub id: String,
    pub name: String,
    /// A bundled avatar id and colour token, as the app stored it.
    pub avatar: String,
    /// Order in the picker (the list is already sorted by it).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub position: i64,
    pub has_pin: bool,
    pub is_owner: bool,
    /// Its PIN or access record has not arrived from another device yet:
    /// show it, but it cannot be opened.
    pub syncing: bool,
    /// A kids profile (plans/m11-kids.md 9.1).
    pub kids: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesSnapshot {
    pub profiles: Vec<ProfileInfo>,
    /// The session's profile (it may be gone: then the session is locked).
    pub active: String,
    /// The session is locked: only the picker and unlocking work.
    pub locked: bool,
    /// How many profiles `createProfile` allows.
    pub max: u32,
    /// The session runs a kids profile: adult screens hide and leaving asks
    /// the owner's PIN (plans/m11-kids.md 9.1). The bridge keeps it set
    /// until the session's next switch.
    pub kids: bool,
    /// A sign-out that removes the library (D13) was asked here and its
    /// wipe has not answered yet: run steps two and three (on the owner
    /// profile, with the debrid connections when `debrid`). Set by
    /// `signOut {removeAfter}` before the leave, cleared by a leave that
    /// failed, by any join and by the wipe.
    pub wipe_pending: Option<WipePending>,
    /// The profile this session's Incognito copied, while it runs
    /// (plans/batch-10-incognito.md 2.8). While it is set, `active` is
    /// `incognito`, `locked` and `kids` are false, and `profiles` never
    /// lists Incognito.
    pub incognito_host: Option<String>,
    /// The profile whose PIN entering Incognito asks for (the owner's on a
    /// locked session, or on a device with a kids profile when the host has
    /// no PIN of its own); null when no PIN is asked.
    pub incognito_pin: Option<String>,
    /// An Incognito end this session has not read yet: set until its next
    /// profile op or `ackIncognitoEnded`; automatic requests never clear
    /// it.
    pub incognito_ended: bool,
    /// First run's profile step was answered on this device
    /// (`finishProfileStep`; plans/first-screens.md 4.2): never synced,
    /// gone with the wipe. In Incognito, the host's.
    pub profile_step_done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct WipePending {
    pub debrid: bool,
}

/// A session's (active profile, locked, Incognito host, Incognito ended
/// unseen, kids), sent on every switch and lock. The host is set only while
/// the session runs Incognito; the fourth is the snapshot's
/// `incognitoEnded`; `kids` says the session ran as a kids profile until
/// its next switch (plans/m11-kids.md 7), so the picker still asks the
/// owner's PIN after that profile is removed.
pub type SessionState = (String, bool, Option<String>, bool, bool);

pub struct ProfilesView {
    snapshot: SharedObservable<ProfilesSnapshot>,
    task: JoinHandle<()>,
}

impl ProfilesView {
    pub fn open(core: &Core, session: watch::Receiver<SessionState>) -> Self {
        // Subscribe before the first read so no change slips in between.
        // The device's profiles, in Incognito too.
        let core = &core.device();
        let changes = core.library().subscribe();
        let snapshot = SharedObservable::new(read(core, &session));
        let task = tokio::spawn(watch(core.clone(), changes, session, snapshot.clone()));
        Self { snapshot, task }
    }

    pub fn subscribe(
        &self,
    ) -> (
        ProfilesSnapshot,
        Pin<Box<dyn Stream<Item = Vec<ProfilesSnapshot>> + Send>>,
    ) {
        (
            self.snapshot.get(),
            snapshots(self.snapshot.subscribe(), FRAME),
        )
    }
}

impl Drop for ProfilesView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn read(core: &Core, session: &watch::Receiver<SessionState>) -> ProfilesSnapshot {
    let state = session.borrow().clone();
    core.profiles_snapshot_for(&state)
}

async fn watch(
    core: Core,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    mut session: watch::Receiver<SessionState>,
    snapshot: SharedObservable<ProfilesSnapshot>,
) {
    loop {
        tokio::select! {
            tables = crate::views::next_change(&mut changes) => match tables {
                // Access too: `kids` and `syncing` read the access register.
                Some(tables) if tables.iter().any(|t| matches!(t, Table::Profiles | Table::Access)) => {}
                Some(_) => continue,
                None => return,
            },
            changed = session.changed() => if changed.is_err() {
                return;
            },
        }
        let next = read(&core, &session);
        if next != snapshot.get() {
            snapshot.set(next);
        }
    }
}
