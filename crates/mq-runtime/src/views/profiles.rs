//! The profile picker (plans/m11-profiles.md section 6): every profile, and
//! the session's active profile and lock state. Refreshes on profile
//! changes (sync included) and on the session's switches and locks.

use crate::Core;
use crate::live::{FRAME, coalesce};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
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
    /// Its PIN record has not arrived from another device yet: show it,
    /// but it cannot be opened.
    pub syncing: bool,
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
}

/// A session's (active profile, locked), sent on every switch and lock.
pub type SessionState = (String, bool);

pub struct ProfilesView {
    snapshot: SharedObservable<ProfilesSnapshot>,
    task: JoinHandle<()>,
}

impl ProfilesView {
    pub fn open(core: &Core, session: watch::Receiver<SessionState>) -> Self {
        // Subscribe before the first read so no change slips in between.
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
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for ProfilesView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn read(core: &Core, session: &watch::Receiver<SessionState>) -> ProfilesSnapshot {
    let (active, locked) = session.borrow().clone();
    core.for_profile(&active).profiles_snapshot(locked)
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
                Some(tables) if tables.contains(&Table::Profiles) => {}
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
