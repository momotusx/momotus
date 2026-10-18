//! Find addons: the two lists Cinemeta publishes (`addon_catalog`), shown
//! safely and installed in one press (plans/m12-addon-discovery.md 3.1 to
//! 3.3). The lists come from Cinemeta's fixed address, whatever the user has
//! installed: "Official" is that address, never a field a publisher controls.

use crate::live::{FRAME, coalesce};
use crate::{Core, CoreError};
use eyeball::SharedObservable;
use futures_util::{Stream, StreamExt};
use mq_addons::{AddonError, InstalledAddon, configure_url, normalize_transport_url};
use mq_http::{HttpError, RequestOptions, Url};
use mq_library::{AddonRow, Table};
use mq_protocol::{Manifest, ResourcePath};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::pin::{Pin, pin};
use std::time::Duration;
use stremio_core::types::addon::ManifestResource;
use tokio::task::JoinHandle;

/// Cinemeta, the source of both lists (the address the onboardings use).
pub const CINEMETA: &str = "https://v3-cinemeta.strem.io/manifest.json";

/// Each list's fetch, and a listed install's manifest fetch.
const TIMEOUT: Duration = Duration::from_secs(10);
/// Entries shown per list.
const MAX_ENTRIES: usize = 300;
/// A path segment or query value longer than this carries credentials (in
/// the live Community list, someone's OAuth tokens).
const MAX_PART: usize = 64;
const NAME_MAX: usize = 80;
const DESCRIPTION_MAX: usize = 300;

/// The lists, in order: Official, then Community.
const LISTS: [(&str, &str); 2] = [("official", "Official"), ("community", "Community")];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct AddonCatalogSnapshot {
    pub lists: Vec<AddonList>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct AddonList {
    /// "official" or "community".
    pub id: String,
    pub name: String,
    pub state: ListState,
    pub entries: Vec<ListedAddon>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ListState {
    Loading,
    Ready,
    Failed { words: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ListedAddon {
    /// The canonical transport URL, for `installListed` only. May hold keys:
    /// never show or log it.
    pub key: String,
    /// The manifest id.
    pub id: String,
    /// What to show instead of the URL.
    pub host: String,
    pub name: String,
    pub description: Option<String>,
    pub types: Vec<String>,
    /// "catalog", "meta", "stream", "subtitles", ...
    pub provides: Vec<String>,
    pub configuration: Configuration,
    /// The addon's configure page, when it has one.
    pub configure_url: Option<String>,
    pub installed: Installed,
    /// With `copyOff`: the first such copy in addon order, for
    /// `setAddonEnabled`. May hold keys: never show or log it.
    pub copy_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum Configuration {
    None,
    Optional,
    Required,
}

/// This link installed (`on`, `off`), or a configured copy of the addon
/// under another link (same manifest id and host: `copyOn` when any such
/// copy is on, else `copyOff`), or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum Installed {
    Absent,
    On,
    Off,
    CopyOn,
    CopyOff,
}

/// What a listed install takes from the view: the entry's canonical URL and
/// the manifest id the list named.
#[derive(Debug, Clone)]
pub struct ListedEntry {
    pub key: Url,
    pub id: String,
}

/// Why a listed install did not happen, as a code and words for the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListedFailure {
    Timeout,
    Unreachable,
    NeedsSetup,
    Mismatch,
    Adult,
    NotListed,
}

impl ListedFailure {
    pub fn code(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Unreachable => "unreachable",
            Self::NeedsSetup => "needsSetup",
            Self::Mismatch => "mismatch",
            Self::Adult => "adult",
            Self::NotListed => "notListed",
        }
    }

    pub fn words(self) -> &'static str {
        match self {
            Self::Timeout => "This addon's server did not answer in time",
            Self::Unreachable => "This addon's server could not be reached",
            Self::NeedsSetup => "This addon now needs setup first",
            Self::Mismatch => "This addon is not the one the list named",
            Self::Adult => "This addon is for adults and is not shown",
            Self::NotListed => "This addon is no longer in the list",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListedError {
    #[error("{}", .0.words())]
    Refused(ListedFailure),
    #[error(transparent)]
    Core(#[from] CoreError),
}

impl From<ListedFailure> for ListedError {
    fn from(failure: ListedFailure) -> Self {
        Self::Refused(failure)
    }
}

pub struct AddonCatalogView {
    snapshot: SharedObservable<AddonCatalogSnapshot>,
    task: JoinHandle<()>,
}

impl AddonCatalogView {
    /// Opens on Cinemeta. `fresh` (Refresh, Try again) waits for the
    /// network instead of the addon cache, which would serve a stale answer
    /// at once.
    pub fn open(core: &Core, fresh: bool) -> Self {
        Self::open_from(core, CINEMETA, fresh)
    }

    /// `open` on another source's address, for tests.
    pub fn open_from(core: &Core, source: &str, fresh: bool) -> Self {
        // Subscribe before the first read so no change slips in between.
        let changes = core.library().subscribe();
        let snapshot = SharedObservable::new(AddonCatalogSnapshot {
            lists: LISTS
                .iter()
                .map(|(id, name)| AddonList {
                    id: (*id).into(),
                    name: (*name).into(),
                    state: ListState::Loading,
                    entries: Vec::new(),
                })
                .collect(),
        });
        let task = tokio::spawn(run(
            core.clone(),
            source.to_owned(),
            fresh,
            changes,
            snapshot.clone(),
        ));
        Self { snapshot, task }
    }

    pub fn snapshot(&self) -> AddonCatalogSnapshot {
        self.snapshot.get()
    }

    pub fn subscribe(
        &self,
    ) -> (
        AddonCatalogSnapshot,
        Pin<Box<dyn Stream<Item = Vec<AddonCatalogSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (
            self.snapshot.get(),
            Box::pin(coalesce(Box::pin(updates), FRAME)),
        )
    }

    /// The entry this view shows for `key`: an install takes only those.
    pub fn listed_entry(&self, key: &str) -> Result<ListedEntry, ListedFailure> {
        let snapshot = self.snapshot.get();
        let entry = snapshot
            .lists
            .iter()
            .flat_map(|list| &list.entries)
            .find(|entry| entry.key == key)
            .ok_or(ListedFailure::NotListed)?;
        Ok(ListedEntry {
            key: Url::parse(&entry.key).map_err(|_| ListedFailure::NotListed)?,
            id: entry.id.clone(),
        })
    }
}

impl Drop for AddonCatalogView {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Core {
    /// Installs an entry the open list shows: fetches its manifest through
    /// the proxy-free public-only path, checks it against the entry, and only
    /// then writes the row (never with private reach). A link already
    /// installed (a race) counts as installed. Returns the id and name.
    pub async fn install_listed(
        &self,
        entry: &ListedEntry,
    ) -> Result<(String, String), ListedError> {
        install_listed_with(self, entry, |url| async move {
            let opts = RequestOptions {
                no_proxy: true,
                allow_private: false,
                timeout: TIMEOUT,
                ..Default::default()
            };
            self.http()
                .get_json::<Manifest>(url.as_str(), &opts)
                .await
                .map_err(AddonError::Http)
        })
        .await
    }
}

/// `install_listed` with the manifest fetch given: fetch, validate, and only
/// then persist.
pub async fn install_listed_with<F, Fut>(
    core: &Core,
    entry: &ListedEntry,
    fetch: F,
) -> Result<(String, String), ListedError>
where
    F: FnOnce(Url) -> Fut,
    Fut: Future<Output = Result<Manifest, AddonError>>,
{
    let manifest = fetch(entry.key.clone()).await.map_err(fetch_failure)?;
    check_listed_manifest(entry, &manifest)?;
    let manifest_json =
        serde_json::to_string(&manifest).map_err(|e| CoreError::BadManifest(e.to_string()))?;
    let url = entry.key.to_string();
    let written = core.edit_addons(|rows| {
        if rows.iter().any(|row| canonical(&row.transport_url) == url) {
            return Err(CoreError::AlreadyInstalled(mq_http::redact_url(&entry.key)));
        }
        rows.push(AddonRow {
            transport_url: url.clone(),
            manifest_json,
            position: 0,
            enabled: true,
            allow_private: false,
        });
        Ok(())
    });
    match written {
        Ok(()) | Err(CoreError::AlreadyInstalled(_)) => Ok((manifest.id, manifest.name)),
        Err(e) => Err(e.into()),
    }
}

/// The fetched manifest is the addon the list named, needs no setup and is
/// not for adults.
pub fn check_listed_manifest(
    entry: &ListedEntry,
    manifest: &Manifest,
) -> Result<(), ListedFailure> {
    if manifest.id != entry.id {
        return Err(ListedFailure::Mismatch);
    }
    if manifest.behavior_hints.configuration_required {
        return Err(ListedFailure::NeedsSetup);
    }
    if manifest.behavior_hints.adult {
        return Err(ListedFailure::Adult);
    }
    Ok(())
}

fn fetch_failure(error: AddonError) -> ListedFailure {
    match error {
        AddonError::TimedOut | AddonError::Http(HttpError::Timeout(_)) => ListedFailure::Timeout,
        _ => ListedFailure::Unreachable,
    }
}

/// An entry's link as the list may show it: the canonical form of a link
/// that is https, carries no credentials (no userinfo, no path segment or
/// query value over `MAX_PART` characters), ends in `/manifest.json` and
/// names a public host. `stremio://` is normalized first.
pub fn listable(raw: &str) -> Option<Url> {
    let url = normalize_transport_url(raw).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let long = |part: &str| part.chars().count() > MAX_PART;
    if url.path_segments()?.any(long) || url.query_pairs().any(|(_, value)| long(&value)) {
        return None;
    }
    let host = url.host_str()?;
    if mq_http::is_private_host(host) {
        return None;
    }
    Some(url)
}

/// The configure page of an addon whose manifest says it has one.
pub(crate) fn configure_page(url: &Url, manifest: &Manifest) -> Option<String> {
    let hints = &manifest.behavior_hints;
    if !(hints.configurable || hints.configuration_required) {
        return None;
    }
    configure_url(url).ok()
}

/// The canonical form of a stored link, or the link as stored.
fn canonical(transport_url: &str) -> String {
    normalize_transport_url(transport_url)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| transport_url.to_owned())
}

async fn run(
    core: Core,
    source: String,
    fresh: bool,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    snapshot: SharedObservable<AddonCatalogSnapshot>,
) {
    let mut official = pin!(fetch_list(&core, &source, LISTS[0].0, fresh));
    let mut community = pin!(fetch_list(&core, &source, LISTS[1].0, fresh));
    let (mut official_done, mut community_done) = (false, false);
    loop {
        tokio::select! {
            result = &mut official, if !official_done => {
                official_done = true;
                set_list(&core, &snapshot, 0, result);
            }
            result = &mut community, if !community_done => {
                community_done = true;
                set_list(&core, &snapshot, 1, result);
            }
            change = crate::views::next_change(&mut changes) => {
                let Some(tables) = change else { return };
                if tables.contains(&Table::Addons) {
                    let mut next = snapshot.get();
                    mark_installed(&core, &mut next);
                    if next != snapshot.get() {
                        snapshot.set(next);
                    }
                }
            }
        }
    }
}

fn set_list(
    core: &Core,
    snapshot: &SharedObservable<AddonCatalogSnapshot>,
    at: usize,
    result: Result<Vec<ListedAddon>, String>,
) {
    let mut next = snapshot.get();
    let list = &mut next.lists[at];
    match result {
        Ok(entries) => {
            list.state = ListState::Ready;
            list.entries = entries;
        }
        Err(words) => list.state = ListState::Failed { words },
    }
    mark_installed(core, &mut next);
    snapshot.set(next);
}

/// One list from the source, its entries checked and capped.
async fn fetch_list(
    core: &Core,
    source: &str,
    id: &str,
    fresh: bool,
) -> Result<Vec<ListedAddon>, String> {
    let url = Url::parse(source).map_err(|_| "Cinemeta could not be reached".to_owned())?;
    let source = InstalledAddon {
        allow_private: url.host_str().is_some_and(mq_http::is_private_host),
        transport_url: url,
        manifest: cinemeta_manifest(),
    };
    let path = ResourcePath {
        resource: "addon_catalog".into(),
        r#type: "all".into(),
        id: id.into(),
        extra: vec![],
    };
    let fetched = if fresh {
        core.addons().fetch_fresh(&source, &path, TIMEOUT).await
    } else {
        core.addons().fetch(&source, &path, TIMEOUT).await
    };
    let body = fetched.map_err(|e| list_failure(&e))?.body;
    let answer: Value = serde_json::from_slice(&body)
        .map_err(|_| "Cinemeta sent a list Marquee cannot read".to_owned())?;
    let raw = answer["addons"]
        .as_array()
        .ok_or("Cinemeta sent a list Marquee cannot read")?;
    Ok(entries(raw))
}

/// The entries the list may show, in the served order: each checked
/// (`listable`, not adult), the malformed ones skipped, duplicates of a
/// canonical link dropped, at most `MAX_ENTRIES`.
pub fn entries(raw: &[Value]) -> Vec<ListedAddon> {
    let mut seen = HashSet::new();
    raw.iter()
        .filter_map(entry)
        .filter(|entry| seen.insert(entry.key.clone()))
        .take(MAX_ENTRIES)
        .collect()
}

fn entry(raw: &Value) -> Option<ListedAddon> {
    let url = listable(raw["transportUrl"].as_str()?)?;
    let manifest: Manifest = serde_json::from_value(raw["manifest"].clone()).ok()?;
    let hints = &manifest.behavior_hints;
    if hints.adult {
        return None;
    }
    let configuration = if hints.configuration_required {
        Configuration::Required
    } else if hints.configurable {
        Configuration::Optional
    } else {
        Configuration::None
    };
    Some(ListedAddon {
        key: url.to_string(),
        id: manifest.id.clone(),
        host: url.host_str()?.to_owned(),
        name: cut(manifest.name.trim(), NAME_MAX),
        description: manifest
            .description
            .as_deref()
            .map(|d| cut(&plain(d), DESCRIPTION_MAX))
            .filter(|d| !d.is_empty()),
        types: manifest.types.clone(),
        provides: manifest
            .resources
            .iter()
            .map(|r| match r {
                ManifestResource::Short(name) => name.clone(),
                ManifestResource::Full { name, .. } => name.clone(),
            })
            .collect(),
        configuration,
        configure_url: configure_page(&url, &manifest),
        installed: Installed::Absent,
        copy_key: None,
    })
}

/// Text without markup: tags dropped, whitespace collapsed to one space.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Sets each entry's `installed` (and `copyKey`) from the profile's addons.
fn mark_installed(core: &Core, snapshot: &mut AddonCatalogSnapshot) {
    let rows = core.library().addons(core.profile()).unwrap_or_default();
    let installed: Vec<Row> = rows.iter().filter_map(Row::new).collect();
    for entry in snapshot.lists.iter_mut().flat_map(|l| l.entries.iter_mut()) {
        (entry.installed, entry.copy_key) = installed_state(entry, &installed);
    }
}

/// An installed addon as the lists compare it.
struct Row {
    url: String,
    stored: String,
    id: String,
    host: String,
    enabled: bool,
}

impl Row {
    fn new(row: &AddonRow) -> Option<Row> {
        let manifest: Value = serde_json::from_str(&row.manifest_json).ok()?;
        let url = canonical(&row.transport_url);
        let host = Url::parse(&url).ok()?.host_str()?.to_owned();
        Some(Row {
            url,
            stored: row.transport_url.clone(),
            id: manifest["id"].as_str()?.to_owned(),
            host,
            enabled: row.enabled,
        })
    }
}

fn installed_state(entry: &ListedAddon, rows: &[Row]) -> (Installed, Option<String>) {
    if let Some(row) = rows.iter().find(|row| row.url == entry.key) {
        let state = if row.enabled {
            Installed::On
        } else {
            Installed::Off
        };
        return (state, None);
    }
    let copies: Vec<&Row> = rows
        .iter()
        .filter(|row| row.id == entry.id && row.host == entry.host)
        .collect();
    if copies.iter().any(|row| row.enabled) {
        (Installed::CopyOn, None)
    } else if let Some(first) = copies.first() {
        (Installed::CopyOff, Some(first.stored.clone()))
    } else {
        (Installed::Absent, None)
    }
}

/// The words of a list that could not be read.
fn list_failure(error: &AddonError) -> String {
    match error {
        AddonError::TimedOut | AddonError::Http(HttpError::Timeout(_)) => {
            "Cinemeta did not answer in time".into()
        }
        AddonError::Http(HttpError::Status { status, .. }) => {
            format!("Cinemeta answered with an error ({status})")
        }
        AddonError::Http(HttpError::TooLarge { .. } | HttpError::Decode(_)) => {
            "Cinemeta sent a list Marquee cannot read".into()
        }
        _ => "Cinemeta did not answer".into(),
    }
}

/// The source addon's manifest as the addon client needs it (its id keys the
/// health book): the lists come from Cinemeta whatever is installed.
fn cinemeta_manifest() -> Manifest {
    serde_json::from_value(serde_json::json!({
        "id": "com.linvo.cinemeta",
        "version": "0.0.0",
        "name": "Cinemeta",
        "resources": ["addon_catalog"],
        "types": [],
        "catalogs": [],
    }))
    .expect("a valid manifest")
}
