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
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            streams: StreamPrefs::default(),
            subtitle_languages: Vec::new(),
            autoplay_next: true,
        }
    }
}

impl Core {
    /// The profile's preferences, defaults for anything never set.
    pub fn prefs(&self) -> Prefs {
        let mut merged = serde_json::to_value(Prefs::default()).unwrap_or_default();
        for (key, value) in self.library().settings(self.profile()).unwrap_or_default() {
            if let (Some(object), Ok(value)) =
                (merged.as_object_mut(), serde_json::from_str(&value))
            {
                object.insert(key, value);
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
        for key in fields.keys() {
            self.library()
                .set_setting(self.profile(), key, &stored[key].to_string(), &hlc)?;
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
