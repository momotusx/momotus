//! Step 3 of the import: maps a fetched [`Snapshot`] into the local library.
//!
//! The mapping is idempotent. Every synced row is written with an HLC derived
//! from the Stremio item's `_mtime` (counter 0, device 0), so re-running it
//! changes nothing, and any edit made on a real device afterwards carries a
//! later clock and wins over the imported value.

use mq_http::{is_private_host, redact_url};
use mq_library::{AddonRow, Hlc, Library, LibraryError, LibraryItemRow, ProgressRow};
use mq_protocol::Manifest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use stremio_core::constants::ADDON_LEGACY_PATH;
use stremio_core::types::library::{LibraryItem, LibraryItemState};
use stremio_core::types::resource::Video;
use stremio_core::types::watched_bitfield::{BitField8, WatchedField};
use url::Url;

/// Clock device id of every imported row. Zero sorts below any real device,
/// so a local write at the same millisecond still wins.
const IMPORT_DEVICE: u64 = 0;

/// What the three Stremio reads returned, as raw JSON. This is what
/// `store_raw_import("stremio", ..)` keeps, so it holds no session key, no
/// email and no Trakt token.
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub user: Value,
    /// The `addonCollectionGet` result: `{addons: [..], lastModified}`.
    pub addon_collection: Value,
    /// The `datastoreGet` result: library items.
    pub library: Vec<Value>,
}

// Addon transport URLs often carry API keys, so Debug never prints them.
impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Snapshot")
            .field("library_items", &self.library.len())
            .finish_non_exhaustive()
    }
}

/// Ordered video lists of series, by library item id, as the item's meta
/// addon returns them. A series' `watched` bitfield only decodes against this
/// list, so series without an entry are reported as deferred.
pub type SeriesVideos = HashMap<String, Vec<Video>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedAddon {
    pub name: String,
    /// Redacted: path segments and queries that look like secrets are hidden.
    pub url: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub addons_imported: usize,
    pub addons_skipped: Vec<SkippedAddon>,
    /// Titles now in the library (`removed` false, `temp` false).
    pub items_imported: usize,
    /// Progress rows written: items with a resume position, in the library or not.
    pub in_progress: usize,
    /// Watched videos written: decoded episodes plus watched movies.
    pub watched_marked: usize,
    /// Library item ids whose watched bitfield needs the series' video list.
    /// Pass the lists in and run [`apply_snapshot`] again.
    pub deferred_watched_items: Vec<String>,
    /// Stremio fields seen with data that Marquee does not map. All of it
    /// stays in the raw import.
    pub unsupported_fields: Vec<String>,
    /// Set by `import_account`: where the raw import was stored.
    pub raw_import_id: Option<i64>,
}

/// Stremio's modification time as the register clock, never later than now:
/// a future time would win every later edit and be refused by the relay.
fn import_hlc(wall_ms: i64) -> String {
    Hlc {
        wall_ms: (wall_ms.max(0) as u64).min(mq_library::hlc::now_ms()),
        counter: 0,
        device: IMPORT_DEVICE,
    }
    .encode()
}

pub fn apply_snapshot(
    lib: &Library,
    profile_id: &str,
    profile_name: &str,
    snapshot: &Snapshot,
    videos: &SeriesVideos,
    now_ms: u64,
) -> Result<ImportReport, LibraryError> {
    let mut report = ImportReport::default();
    let mut unsupported = BTreeSet::new();
    let now_hlc = import_hlc(now_ms as i64);
    lib.ensure_profile(profile_id, profile_name, &now_hlc)?;
    map_addons(
        lib,
        profile_id,
        snapshot,
        &now_hlc,
        &mut report,
        &mut unsupported,
    )?;
    map_items(
        lib,
        profile_id,
        snapshot,
        videos,
        &mut report,
        &mut unsupported,
    )?;
    if snapshot.user["traktLinked"] == true {
        unsupported.insert("user.trakt".to_owned());
    }
    report.unsupported_fields = unsupported.into_iter().collect();
    Ok(report)
}

fn addon_row(raw: &Value) -> Result<AddonRow, String> {
    let url = raw["transportUrl"]
        .as_str()
        .and_then(|s| Url::parse(s).ok())
        .ok_or("invalid transport url")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("unsupported transport scheme {}", url.scheme()));
    }
    Manifest::deserialize(&raw["manifest"]).map_err(|e| format!("invalid manifest: {e}"))?;
    Ok(AddonRow {
        transport_url: url.to_string(),
        manifest_json: raw["manifest"].to_string(),
        position: 0,
        enabled: true,
        allow_private: url.host_str().is_some_and(is_private_host),
    })
}

fn skipped(raw: &Value, reason: String) -> SkippedAddon {
    SkippedAddon {
        name: raw["manifest"]["name"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned(),
        url: raw["transportUrl"]
            .as_str()
            .and_then(|s| Url::parse(s).ok())
            .map_or_else(|| "<invalid>".to_owned(), |u| redact_url(&u)),
        reason,
    }
}

/// Imported addons come first, in Stremio's order. Addons that exist only
/// locally stay after them, and an addon the user disabled locally stays
/// disabled, so importing again never deletes local choices.
fn map_addons(
    lib: &Library,
    profile_id: &str,
    snapshot: &Snapshot,
    hlc: &str,
    report: &mut ImportReport,
    unsupported: &mut BTreeSet<String>,
) -> Result<(), LibraryError> {
    let mut rows: Vec<AddonRow> = Vec::new();
    let mut seen = HashSet::new();
    for raw in snapshot.addon_collection["addons"]
        .as_array()
        .into_iter()
        .flatten()
    {
        match addon_row(raw) {
            Ok(row) if !seen.insert(row.transport_url.clone()) => report
                .addons_skipped
                .push(skipped(raw, "duplicate transport url".into())),
            Ok(row) => {
                if row.transport_url.ends_with(ADDON_LEGACY_PATH) {
                    unsupported.insert("addon.transport.legacy_jsonrpc".to_owned());
                }
                rows.push(row);
            }
            Err(reason) => report.addons_skipped.push(skipped(raw, reason)),
        }
    }
    report.addons_imported = rows.len();
    let local = lib.addons(profile_id)?;
    for row in &mut rows {
        if let Some(existing) = local.iter().find(|e| e.transport_url == row.transport_url) {
            row.enabled = existing.enabled;
        }
    }
    rows.extend(
        local
            .into_iter()
            .filter(|e| !seen.contains(&e.transport_url)),
    );
    lib.set_addons(profile_id, &rows, hlc)
}

fn map_items(
    lib: &Library,
    profile_id: &str,
    snapshot: &Snapshot,
    videos: &SeriesVideos,
    report: &mut ImportReport,
    unsupported: &mut BTreeSet<String>,
) -> Result<(), LibraryError> {
    let mut undecodable = 0;
    for raw in &snapshot.library {
        match LibraryItem::deserialize(raw) {
            Ok(item) => map_item(lib, profile_id, &item, raw, videos, report, unsupported)?,
            Err(_) => undecodable += 1,
        }
    }
    if undecodable > 0 {
        unsupported.insert(format!(
            "libraryItem: {undecodable} undecodable items, kept in the raw import"
        ));
    }
    Ok(())
}

fn map_item(
    lib: &Library,
    profile_id: &str,
    item: &LibraryItem,
    raw: &Value,
    videos: &SeriesVideos,
    report: &mut ImportReport,
    unsupported: &mut BTreeSet<String>,
) -> Result<(), LibraryError> {
    // Removed and not temp is a tombstone: the user took it out of the library.
    if item.removed && !item.temp {
        return Ok(());
    }
    note_unsupported(raw, unsupported);
    let state = &item.state;
    let in_library = !item.removed && !item.temp;
    let in_progress = state.time_offset > 0;
    let mtime_ms = item.mtime.timestamp_millis();
    let hlc = import_hlc(mtime_ms);
    // Movies play under their own id unless a default video is declared.
    let video_id = state
        .video_id
        .as_deref()
        .or(item.behavior_hints.default_video_id.as_deref())
        .unwrap_or(&item.id);

    // Temp items still get a row when they have progress, so Continue
    // Watching can show a name and poster for them.
    if in_library || in_progress {
        lib.upsert_item(
            profile_id,
            &LibraryItemRow {
                item_id: item.id.clone(),
                kind: item.r#type.clone(),
                name: item.name.clone(),
                poster: item.poster.as_ref().map(Url::to_string),
                poster_shape: serde_json::to_value(&item.poster_shape)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned)),
                added_ms: item.ctime.map_or(mtime_ms, |c| c.timestamp_millis()),
            },
            in_library,
            &hlc,
        )?;
    }
    if in_library {
        report.items_imported += 1;
    }
    if in_progress {
        lib.set_progress(
            profile_id,
            ProgressRow {
                video_id,
                item_id: &item.id,
                position_ms: clamp_ms(state.time_offset),
                duration_ms: clamp_ms(state.duration),
                updated_ms: mtime_ms,
            },
            &hlc,
        )?;
        report.in_progress += 1;
    }

    let marks = match &state.watched {
        Some(field) => match decode_watched(state, field, videos.get(&item.id)) {
            Watched::Videos(ids) => ids,
            Watched::Empty => Vec::new(),
            Watched::Deferred => {
                report.deferred_watched_items.push(item.id.clone());
                Vec::new()
            }
        },
        // A movie has no bitfield; Stremio counts it watched once played through.
        None if item.r#type == "movie" && state.times_watched > 0 => vec![video_id.to_owned()],
        None => Vec::new(),
    };
    for id in &marks {
        lib.set_watched(profile_id, &item.id, id, true, &hlc)?;
    }
    report.watched_marked += marks.len();
    Ok(())
}

fn clamp_ms(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX)
}

enum Watched {
    /// No bit is set, there is nothing to write.
    Empty,
    /// The series' video list is missing, or does not contain the bitfield's
    /// anchor video, so the bits cannot be placed on episodes.
    Deferred,
    Videos(Vec<String>),
}

/// Decodes `anchorVideo:anchorLength:bitfield` against the series' videos
/// with stremio-core's own decoder (`LibraryItemState::watched_bitfield`),
/// which shifts the bits if the video list changed since the field was
/// written, as long as the anchor video is still in it.
fn decode_watched(
    state: &LibraryItemState,
    field: &WatchedField,
    videos: Option<&Vec<Video>>,
) -> Watched {
    let text = field.to_string();
    let mut parts = text.rsplitn(3, ':');
    let (Some(bits), Some(_), Some(anchor)) = (parts.next(), parts.next(), parts.next()) else {
        return Watched::Empty;
    };
    match bits.parse::<BitField8>() {
        Ok(bits) if bits.last_index_of(true).is_some() => {}
        _ => return Watched::Empty,
    }
    let Some(videos) = videos.filter(|v| v.iter().any(|v| v.id == anchor)) else {
        return Watched::Deferred;
    };
    let decoded = state.watched_bitfield(videos);
    Watched::Videos(
        videos
            .iter()
            .filter(|v| decoded.get_video(&v.id))
            .map(|v| v.id.clone())
            .collect(),
    )
}

/// Item keys that are consumed by the mapping. Everything else that carries
/// data is reported (and preserved in the raw import).
const MAPPED: &[&str] = &[
    "_id",
    "name",
    "type",
    "poster",
    "posterShape",
    "removed",
    "temp",
    "_ctime",
    "_mtime",
    "state",
];
const MAPPED_STATE: &[&str] = &[
    "timeOffset",
    "duration",
    "video_id",
    "watched",
    "timesWatched",
];

fn note_unsupported(raw: &Value, out: &mut BTreeSet<String>) {
    scan(raw, MAPPED, "libraryItem", out);
    scan(&raw["state"], MAPPED_STATE, "libraryItem.state", out);
}

fn scan(object: &Value, mapped: &[&str], prefix: &str, out: &mut BTreeSet<String>) {
    for (key, value) in object.as_object().into_iter().flatten() {
        if !mapped.contains(&key.as_str()) && is_set(value) {
            out.insert(format!("{prefix}.{key}"));
        }
    }
}

/// False for the empty values Stremio writes by default.
fn is_set(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => a.iter().any(is_set),
        Value::Object(o) => o.values().any(is_set),
    }
}
