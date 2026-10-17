//! Profile preferences. Each top-level field is its own row in `settings`,
//! so two devices changing different fields both keep their change (last
//! writer wins per field, by HLC).

use crate::live::{FRAME, coalesce};
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_library::Table;
use mq_streams::StreamPrefs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    pub streams: StreamPrefs,
    /// Subtitle languages as addons name them (usually ISO 639-2, "eng"),
    /// most wanted first. Matching tracks are listed first.
    pub subtitle_languages: Vec<String>,
    /// Start the next episode when one ends.
    pub autoplay_next: bool,
    /// Which addon catalogs Home shows, and in what order.
    pub home_rows: HomeRows,
}

/// Home rows the user hid or moved, by row id (`<addonId>:<type>:<catalogId>`).
/// Rows in `order` come first in that order, then the rest in addon order.
/// Ids of catalogs that are gone are kept, so a reinstalled addon gets its
/// place back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase", default)]
pub struct HomeRows {
    pub hidden: Vec<String>,
    pub order: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            streams: StreamPrefs::default(),
            subtitle_languages: Vec::new(),
            autoplay_next: true,
            home_rows: HomeRows::default(),
        }
    }
}

impl Core {
    /// The profile's preferences, defaults for anything never set.
    pub fn prefs(&self) -> Prefs {
        let mut merged = serde_json::to_value(Prefs::default()).unwrap_or_default();
        // Oldest first: a whole field ("streams") and a leaf inside it
        // ("streams.quality") end with whichever was written last.
        for (key, value) in self.library().settings(self.profile()).unwrap_or_default() {
            let Ok(value) = serde_json::from_str::<Value>(&value) else {
                continue;
            };
            let mut next = merged.clone();
            let slot = key.split('.').try_fold(&mut next, |at, part| {
                at.as_object_mut()
                    .map(|o| o.entry(part).or_insert(Value::Null))
            });
            let Some(slot) = slot else { continue };
            *slot = value;
            // A value this version cannot read (written by a newer one) is
            // skipped on its own instead of resetting everything.
            if serde_json::from_value::<Prefs>(next.clone()).is_ok() {
                merged = next;
            }
        }
        serde_json::from_value(merged).unwrap_or_default()
    }

    /// Merges `patch` into the preferences, e.g. `{"streams": {"languages":
    /// ["tr"]}}`; nested objects merge, other values replace. Unknown
    /// top-level keys and values of the wrong shape are rejected.
    pub fn set_prefs(&self, patch: &Value) -> Result<Prefs, CoreError> {
        let fields = patch
            .as_object()
            .ok_or_else(|| CoreError::BadPrefs("expected an object".into()))?;
        let mut merged = serde_json::to_value(self.prefs()).unwrap_or_default();
        for key in fields.keys() {
            if merged.get(key).is_none() {
                return Err(CoreError::BadPrefs(format!("unknown setting: {key}")));
            }
        }
        merge(&mut merged, patch);
        let prefs: Prefs =
            serde_json::from_value(merged).map_err(|e| CoreError::BadPrefs(e.to_string()))?;
        let stored = serde_json::to_value(&prefs).unwrap_or_default();
        let hlc = self.hlc();
        // Objects are stored leaf by leaf, so two devices changing different
        // stream preferences both keep their change.
        let mut writes = Vec::new();
        for (key, value) in fields {
            let leaves: Vec<String> = match (value, &stored[key]) {
                (Value::Object(patch), Value::Object(_)) => {
                    patch.keys().map(|leaf| format!("{key}.{leaf}")).collect()
                }
                _ => vec![key.clone()],
            };
            for leaf in leaves {
                let value = leaf
                    .split('.')
                    .try_fold(&stored, |at, part| at.get(part))
                    .ok_or_else(|| CoreError::BadPrefs(format!("unknown setting: {leaf}")))?;
                writes.push((leaf, value.to_string()));
            }
        }
        // Checked in full above, so a bad patch writes nothing.
        for (leaf, value) in writes {
            self.library()
                .set_setting(self.profile(), &leaf, &value, &hlc)?;
        }
        Ok(prefs)
    }
}

fn merge(target: &mut Value, patch: &Value) {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                merge(target.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (target, patch) => *target = patch.clone(),
    }
}

pub struct SettingsView {
    snapshot: SharedObservable<Prefs>,
    task: JoinHandle<()>,
}

impl SettingsView {
    pub fn open(core: &Core) -> Self {
        // Subscribe before the first read so no change slips in between.
        let changes = core.library().subscribe();
        let snapshot = SharedObservable::new(core.prefs());
        let task = tokio::spawn(watch(core.clone(), changes, snapshot.clone()));
        Self { snapshot, task }
    }

    pub fn snapshot(&self) -> Prefs {
        self.snapshot.get()
    }

    pub fn subscribe(&self) -> (Prefs, Pin<Box<dyn Stream<Item = Vec<Prefs>> + Send>>) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }
}

impl Drop for SettingsView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn watch(
    core: Core,
    mut changes: broadcast::Receiver<Vec<Table>>,
    snapshot: SharedObservable<Prefs>,
) {
    while let Ok(tables) = changes.recv().await {
        if tables.contains(&Table::Settings) {
            let next = core.prefs();
            if next != snapshot.get() {
                snapshot.set(next);
            }
        }
    }
}
