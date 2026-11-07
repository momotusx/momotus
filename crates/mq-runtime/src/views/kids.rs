//! The owner's views of kids profiles (plans/m11-kids.md 9.1): one kids
//! profile's list with its extras past the cap (`kidsTitles`), and the
//! counted keys of every kids profile (`kidsKeys`) that tile menus, Search's
//! add mode and the title page read Allow or Remove from. Both are live and
//! owner only.

use crate::Core;
use crate::live::{FRAME, snapshots};
use crate::profiles::ProfileError;
use eyeball::SharedObservable;
use futures_util::Stream;
use mq_library::{AllowedRow, MAX_ALLOWED, OWNER, Table};
use serde::Serialize;
use std::pin::Pin;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct KidsTitle {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub poster: Option<String>,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub added_ms: i64,
    /// Past the cap (synced from two devices): the kid does not see it.
    pub over: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct KidsTitlesSnapshot {
    pub profile: String,
    /// Newest first: the extras past the cap (at most 50), then the counted.
    pub titles: Vec<KidsTitle>,
    /// How many are past the cap.
    pub over_count: u32,
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct KidsKeys {
    pub profile: String,
    pub name: String,
    /// `kind:id` of each counted title.
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct KidsKeysSnapshot {
    pub profiles: Vec<KidsKeys>,
}

/// A live snapshot kept by a task that rebuilds it on library changes.
struct Live<T> {
    snapshot: SharedObservable<T>,
    task: JoinHandle<()>,
}

impl<T: Clone + PartialEq + Send + Sync + 'static> Live<T> {
    fn start(core: &Core, read: fn(&Core) -> T) -> Self {
        // Subscribe before the first read so no change slips in between.
        let mut changes = core.library().subscribe();
        let snapshot = SharedObservable::new(read(core));
        let (core, shared) = (core.clone(), snapshot.clone());
        let task = tokio::spawn(async move {
            while let Some(tables) = crate::views::next_change(&mut changes).await {
                if !tables.contains(&Table::Access) && !tables.contains(&Table::Profiles) {
                    continue;
                }
                let next = read(&core);
                if next != shared.get() {
                    shared.set(next);
                }
            }
        });
        Self { snapshot, task }
    }

    fn subscribe(&self) -> (T, Pin<Box<dyn Stream<Item = Vec<T>> + Send>>) {
        (
            self.snapshot.get(),
            snapshots(self.snapshot.subscribe(), FRAME),
        )
    }
}

impl<T> Drop for Live<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// `kidsTitles {profile}`: one kids profile's list, for its owner.
pub struct KidsTitlesView(Live<KidsTitlesSnapshot>);

impl KidsTitlesView {
    pub fn open(core: &Core, profile: &str) -> Result<Self, ProfileError> {
        core.kids_target(profile)?;
        Ok(Self(Live::start(&core.for_profile(profile), titles)))
    }

    pub fn snapshot(&self) -> KidsTitlesSnapshot {
        self.0.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        KidsTitlesSnapshot,
        Pin<Box<dyn Stream<Item = Vec<KidsTitlesSnapshot>> + Send>>,
    ) {
        self.0.subscribe()
    }
}

/// `kidsKeys`: the counted keys of every kids profile, for the owner.
pub struct KidsKeysView(Live<KidsKeysSnapshot>);

impl KidsKeysView {
    pub fn open(core: &Core) -> Result<Self, ProfileError> {
        if core.profile() != OWNER {
            return Err(ProfileError::NotOwner);
        }
        Ok(Self(Live::start(core, keys)))
    }

    pub fn snapshot(&self) -> KidsKeysSnapshot {
        self.0.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        KidsKeysSnapshot,
        Pin<Box<dyn Stream<Item = Vec<KidsKeysSnapshot>> + Send>>,
    ) {
        self.0.subscribe()
    }
}

/// The list of the handle's profile; a read error shows an empty one.
fn titles(core: &Core) -> KidsTitlesSnapshot {
    let list = core
        .library()
        .allowed_list(core.profile())
        .unwrap_or_default();
    let title = |over: bool| {
        move |r: AllowedRow| KidsTitle {
            id: r.item_id,
            kind: r.kind,
            name: r.name,
            poster: r.poster,
            added_ms: r.added_ms,
            over,
        }
    };
    let titles = list
        .over
        .into_iter()
        .map(title(true))
        .chain(list.counted.into_iter().rev().map(title(false)))
        .collect();
    KidsTitlesSnapshot {
        profile: core.profile().to_owned(),
        titles,
        over_count: list.over_count as u32,
        max: MAX_ALLOWED as u32,
    }
}

/// The counted keys of every kids profile, in the picker's order. One still
/// syncing is left out: `allowTitle` would refuse it.
fn keys(core: &Core) -> KidsKeysSnapshot {
    let library = core.library();
    let profiles = library
        .profiles()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.ready() && p.kids())
        .map(|p| {
            let counted = library.allowed_list(&p.id).unwrap_or_default().counted;
            KidsKeys {
                keys: counted
                    .into_iter()
                    .map(|r| format!("{}:{}", r.kind, r.item_id))
                    .collect(),
                profile: p.id,
                name: p.name,
            }
        })
        .collect();
    KidsKeysSnapshot { profiles }
}
