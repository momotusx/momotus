//! Profile preferences. Each top-level field is its own row in `settings`,
//! so two devices changing different fields both keep their change (last
//! writer wins per field, by HLC).

use crate::learn::{self, Learned, LearnedChoice};
use crate::live::{FRAME, coalesce};
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_library::Table;
use mq_streams::{QualityPref, StreamPrefs};
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
    /// How big subtitles show, on every device of the profile (journey
    /// reviews 2026-10-20). The delay stays per play, in the apps.
    pub subtitle_size: SubtitleSize,
    /// Start the next episode when one ends.
    pub autoplay_next: bool,
    /// Which addon catalogs Home shows, and in what order.
    pub home_rows: HomeRows,
    /// The default addons list version this profile was last offered
    /// (`defaultAddons`): below the current one, Home offers the new ones once.
    pub defaults_seen: u32,
    /// What the user's picks taught, by screen class (plans/m13-stream-choice.md 3.9).
    pub learned: crate::learn::LearnedByClass,
    /// Raised by "Forget what was learned"; a counter stamped with an older
    /// epoch reads as zero.
    pub learned_epoch: u32,
    /// Discover's last choice, which it opens on (plans/discover-v2.md 6.1).
    pub discover: DiscoverPrefs,
    /// Films from the Internet Archive in Versions
    /// (plans/plugins-and-marketplace.md 5.1).
    pub archive: bool,
    /// A screen showed this profile the Archive line: the first-run addon
    /// step, the Versions notice or the Settings notice. Until then the
    /// core never contacts archive.org for it.
    pub archive_notice_seen: bool,
}

/// Discover's kind, service, sort, genre and decade, as `select` takes
/// them; null for All, Any or not chosen. Written by Discover itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscoverPrefs {
    pub kind: Option<String>,
    pub platform: Option<String>,
    pub sort: Option<String>,
    pub genre: Option<String>,
    pub decade: Option<String>,
}

/// Subtitle text size, each app mapping it to its player's scale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleSize {
    Small,
    #[default]
    Medium,
    Large,
    ExtraLarge,
}

/// Home rows the user hid, showed or moved, by row id (`<addonId>:<type>:<catalogId>`).
/// Rows in `order` come first in that order, then the rest in addon order.
/// `shown` holds the twins the user turned on (a row titled as one before it
/// is hidden until then). Ids of catalogs that are gone are kept, so a
/// reinstalled addon gets its place back.
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
    pub shown: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            streams: StreamPrefs::default(),
            subtitle_languages: Vec::new(),
            subtitle_size: SubtitleSize::default(),
            autoplay_next: true,
            home_rows: HomeRows::default(),
            defaults_seen: 0,
            learned: crate::learn::LearnedByClass::default(),
            learned_epoch: 0,
            discover: DiscoverPrefs::default(),
            archive: true,
            archive_notice_seen: false,
        }
    }
}

impl Core {
    /// The size limit in effect is one the user set (or an older app's
    /// quality set), not the default (plans/m13-stream-choice.md 3.7).
    pub(crate) fn size_limit_chosen(&self) -> bool {
        let rows = self.library().settings(self.profile()).unwrap_or_default();
        rows.iter().any(|(key, value)| {
            key == "streams.sizeLimitBytes"
                || (key == "streams" && value.contains("\"sizeLimitBytes\""))
        }) || quality_only(&rows)
    }

    /// The profile's preferences, defaults for anything never set.
    pub fn prefs(&self) -> Prefs {
        let rows = self.library().settings(self.profile()).unwrap_or_default();
        let mut merged = serde_json::to_value(Prefs::default()).unwrap_or_default();
        // Oldest first: a whole field ("streams") and a leaf inside it
        // ("streams.quality") end with whichever was written last.
        for (key, value) in &rows {
            let Ok(value) = serde_json::from_str::<Value>(value) else {
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
        let mut prefs: Prefs = serde_json::from_value(merged).unwrap_or_default();
        if quality_only(&rows) {
            from_quality(&mut prefs.streams);
        }
        prefs
    }

    /// Merges `patch` into the preferences, e.g. `{"streams": {"languages":
    /// ["tr"]}}`; nested objects merge, other values replace. Unknown
    /// top-level keys and values of the wrong shape are rejected.
    pub fn set_prefs(&self, patch: &Value) -> Result<Prefs, CoreError> {
        let fields = patch
            .as_object()
            .ok_or_else(|| CoreError::BadPrefs("expected an object".into()))?;
        let rows = self.library().settings(self.profile()).unwrap_or_default();
        let mut merged = serde_json::to_value(self.prefs()).unwrap_or_default();
        for key in fields.keys() {
            if merged.get(key).is_none() {
                return Err(CoreError::BadPrefs(format!("unknown setting: {key}")));
            }
        }
        merge(&mut merged, patch);
        let mut prefs: Prefs =
            serde_json::from_value(merged).map_err(|e| CoreError::BadPrefs(e.to_string()))?;
        // The old `quality` is mapped once (plans/m13-stream-choice.md
        // 4.1): an older app's choice, written now or before any of the new
        // fields, is stored as the fields it maps to.
        let streams = fields.get("streams").and_then(Value::as_object);
        let (quality, v2) = stored_fields(&rows);
        let writes_quality = streams.is_some_and(|s| s.contains_key("quality"));
        let migrate = streams.is_some() && !v2 && (quality || writes_quality);
        if migrate && writes_quality {
            from_quality(&mut prefs.streams);
        }
        let stored = serde_json::to_value(&prefs).unwrap_or_default();
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
        if migrate {
            for leaf in ["sizeLimitBytes", "minSeeders", "maxHeight"] {
                let key = format!("streams.{leaf}");
                if !writes.iter().any(|(k, _)| *k == key) {
                    writes.push((key, stored["streams"][leaf].to_string()));
                }
            }
        }
        // Checked in full above, so a bad patch writes nothing.
        for (leaf, value) in writes {
            self.library()
                .set_setting(self.profile(), &leaf, &value, self.clock())?;
        }
        // The Archive's list goes once no profile here wants it (5.1).
        if fields.contains_key("archive") {
            crate::archive::forget_unwanted(self);
        }
        // A preference set by hand zeroes what learning counted for it (3.9).
        let by_hand: Vec<LearnedChoice> = [
            ("maxHeight", LearnedChoice::Height),
            ("sizeLimitBytes", LearnedChoice::Size),
            ("preferReady", LearnedChoice::Ready),
            ("hdr", LearnedChoice::Hdr),
        ]
        .into_iter()
        .filter(|(leaf, _)| streams.is_some_and(|s| s.contains_key(*leaf)))
        .map(|(_, choice)| choice)
        .collect();
        if !by_hand.is_empty() {
            let l = &prefs.learned;
            let zeroed = |c: &Learned| by_hand.iter().fold(c.clone(), |c, ch| learn::zero(&c, *ch));
            let patch = serde_json::json!({ "learned": {
                "uhd": zeroed(&l.uhd), "fhd": zeroed(&l.fhd), "hd": zeroed(&l.hd), "sd": zeroed(&l.sd),
            }});
            return self.set_prefs(&patch);
        }
        Ok(prefs)
    }
}

/// The profile's stream settings name the old `quality` and none of the
/// fields that replace it: an older device wrote them.
fn quality_only(rows: &[(String, String)]) -> bool {
    let (quality, v2) = stored_fields(rows);
    quality && !v2
}

/// Whether the stored stream settings name the old `quality`, and any of
/// the fields that replace it.
fn stored_fields(rows: &[(String, String)]) -> (bool, bool) {
    let (mut quality, mut v2) = (false, false);
    for (key, value) in rows {
        match key.as_str() {
            "streams.quality" => quality = true,
            "streams.sizeLimitBytes" | "streams.minSeeders" => v2 = true,
            "streams" => {
                quality |= value.contains("\"quality\"");
                v2 |= value.contains("\"sizeLimitBytes\"") || value.contains("\"minSeeders\"");
            }
            _ => {}
        }
    }
    (quality, v2)
}

/// The old `quality` as the new fields (4.1): Best is no limit and 20
/// seeders, Balanced the defaults, Data saver 2 GB and 720p.
fn from_quality(streams: &mut StreamPrefs) {
    match streams.quality {
        QualityPref::Best => {
            streams.size_limit_bytes = None;
            streams.min_seeders = 20;
        }
        QualityPref::Balanced => {}
        QualityPref::DataSaver => {
            streams.size_limit_bytes = Some(2 << 30);
            streams.max_height = Some(streams.max_height.map_or(720, |h| h.min(720)));
        }
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
    while let Some(tables) = crate::views::next_change(&mut changes).await {
        if tables.contains(&Table::Settings) {
            let next = core.prefs();
            if next != snapshot.get() {
                snapshot.set(next);
            }
        }
    }
}
