//! Find addons: the two lists Cinemeta publishes (`addon_catalog`), shown
//! safely and installed in one press (plans/m12-addon-discovery.md 3.1 to
//! 3.3). The lists come from Cinemeta's fixed address, whatever the user has
//! installed: "Official" is that address, never a field a publisher controls.

use crate::core::{canonical, find_row, put_addon};
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

/// Each list's fetch, and a listed install's manifest fetch, from the start
/// of the wait for a host slot to the end of the body.
const TIMEOUT: Duration = Duration::from_secs(10);
/// A list fetch's own timeout, which `mq-http` starts once a host slot is
/// free: a second inside `TIMEOUT`, so when it fires the addon client still
/// has time to serve a stale cached list (stale-if-error). A slot wait over
/// that second ends at `TIMEOUT` instead, without the stale list.
const FETCH_TIMEOUT: Duration = Duration::from_secs(9);
/// Entries shown per list.
const MAX_ENTRIES: usize = 300;
/// A path segment, query key or query value longer than this carries
/// credentials (in the live Community list, someone's OAuth tokens).
const MAX_PART: usize = 64;
/// The path or the query as a whole: a token split over short parts.
const MAX_WHOLE: usize = 128;
/// The canonical link, in bytes.
const KEY_MAX: usize = 512;
/// The manifest id, in bytes.
const ID_MAX: usize = 128;
/// Types and resources: at most this many, each at most `WORD_MAX` bytes.
const WORDS_MAX: usize = 16;
const WORD_MAX: usize = 32;
/// One entry as JSON, its `copyKey` included: two full lists (600 entries)
/// stay far below the bridge's 4 MiB message cap, whatever the lists and
/// the profile's addons hold. An entry without its copy key is checked
/// against `ENTRY_MAX - COPY_KEY_MAX`.
const ENTRY_MAX: usize = 4 * 1024;
/// A copy's link in `copyKey`, as JSON; a longer one is left out (the card
/// reads Set up, off, with no action).
const COPY_KEY_MAX: usize = 1024;
/// A list's body (Community was 122 KiB on 2026-10-18).
const LIST_MAX_BYTES: usize = 1024 * 1024;
/// A listed addon's manifest.
const MANIFEST_MAX_BYTES: usize = 256 * 1024;
const NAME_MAX: usize = 80;
const DESCRIPTION_MAX: usize = 300;
const UNREADABLE: &str = "Cinemeta sent a list Marquee cannot read";

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
    /// The addon's configure page, always set: the apps offer it by
    /// `configuration`, and a `needsSetup` install failure opens it. Built
    /// from `key`, so it may hold keys: never show or log it.
    pub configure_url: String,
    pub installed: Installed,
    /// With `copyOff`: the first such copy in addon order, for
    /// `setAddonEnabled`; none when its link is over 1 KiB as JSON (the
    /// card then has no action). May hold keys: never show or log it.
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

    /// Each update is one whole snapshot, the newest: never a batch, so a
    /// push stays one snapshot's size.
    pub fn subscribe(
        &self,
    ) -> (
        AddonCatalogSnapshot,
        Pin<Box<dyn Stream<Item = Vec<AddonCatalogSnapshot>> + Send>>,
    ) {
        let updates = self.snapshot.subscribe().map(|s| vec![s]);
        (self.snapshot.get(), Box::pin(updates))
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
                max_bytes: MANIFEST_MAX_BYTES,
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

/// `install_listed` with the manifest fetch given: fetch (within `TIMEOUT`,
/// whatever the fetch waits on), validate, and only then persist.
pub async fn install_listed_with<F, Fut>(
    core: &Core,
    entry: &ListedEntry,
    fetch: F,
) -> Result<(String, String), ListedError>
where
    F: FnOnce(Url) -> Fut,
    Fut: Future<Output = Result<Manifest, AddonError>>,
{
    let manifest = tokio::time::timeout(TIMEOUT, fetch(entry.key.clone()))
        .await
        .map_err(|_| ListedFailure::Timeout)?
        .map_err(fetch_failure)?;
    check_listed_manifest(entry, &manifest)?;
    let manifest_json =
        serde_json::to_string(&manifest).map_err(|e| CoreError::BadManifest(e.to_string()))?;
    let url = entry.key.to_string();
    // Installed already (a race, or another way in): the manifest goes in
    // place, as `install_addon` does.
    core.edit_addons(|rows| {
        put_addon(rows, &url, manifest_json);
        Ok(())
    })?;
    Ok((manifest.id, manifest.name))
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
/// that is https, carries no credentials (no userinfo; no path segment,
/// query key or query value over `MAX_PART` characters; no path or query
/// over `MAX_WHOLE`), ends in `/manifest.json` and names a public host.
/// `stremio://` is normalized first.
pub fn listable(raw: &str) -> Option<Url> {
    let url = normalize_transport_url(raw).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > KEY_MAX
    {
        return None;
    }
    let long = |part: &str| part.chars().count() > MAX_PART;
    let whole = url.path().len().max(url.query().unwrap_or_default().len());
    if whole > MAX_WHOLE
        || url.path_segments()?.any(long)
        || url
            .query_pairs()
            .any(|(key, value)| long(&key) || long(&value))
    {
        return None;
    }
    let host = url.host_str()?;
    // Someone's own sign-in in the settings of the link: never offered.
    if mq_http::is_private_host(host) || carries_token(&url) {
        return None;
    }
    Some(url)
}

/// Field names that hold a sign-in or a key in an addon's settings.
const TOKEN_FIELDS: [&str; 5] = [
    "access_token",
    "refresh_token",
    "token",
    "apikey",
    "api_key",
];

/// Whether an addon link carries a sign-in in its settings: a path segment
/// before `manifest.json` that reads (percent-decoded, or as base64) as a
/// JSON object with a token-like field at any depth, or as `key=value`
/// pairs with one. Such a link from someone else gives their account away;
/// Find addons never lists one, and installing one by paste is confirmed
/// first (plans/m13-default-addons.md, decision 11).
pub fn carries_token(url: &Url) -> bool {
    let Some(segments) = url.path_segments() else {
        return false;
    };
    let segments: Vec<&str> = segments.collect();
    let config = &segments[..segments.len().saturating_sub(1)];
    config.iter().any(|segment| {
        let text = percent_encoding::percent_decode_str(segment).decode_utf8_lossy();
        token_json(&text)
            || base64_text(&text).is_some_and(|t| token_json(&t))
            || text.split(['|', '&', ';']).any(|pair| {
                pair.split_once('=')
                    .is_some_and(|(name, _)| token_name(name))
            })
    })
}

/// `carries_token` for a link as the user typed or pasted it.
pub fn link_carries_token(raw: &str) -> bool {
    normalize_transport_url(raw).is_ok_and(|url| carries_token(&url))
}

fn token_name(name: &str) -> bool {
    TOKEN_FIELDS.contains(&name.trim().to_ascii_lowercase().as_str())
}

fn token_json(text: &str) -> bool {
    fn walk(v: &Value) -> bool {
        match v {
            Value::Object(o) => o.iter().any(|(k, v)| token_name(k) || walk(v)),
            Value::Array(a) => a.iter().any(walk),
            _ => false,
        }
    }
    serde_json::from_str::<Value>(text).is_ok_and(|v| walk(&v))
}

/// A segment read as base64 (standard or URL-safe, padded or not) to text.
fn base64_text(segment: &str) -> Option<String> {
    use base64::Engine as _;
    use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
    let trimmed = segment.trim_end_matches('=');
    let bytes = URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| STANDARD_NO_PAD.decode(trimmed))
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// The configure page of an addon whose manifest says it has one.
pub(crate) fn configure_page(url: &Url, manifest: &Manifest) -> Option<String> {
    let hints = &manifest.behavior_hints;
    if !(hints.configurable || hints.configuration_required) {
        return None;
    }
    configure_url(url).ok()
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
        transport_url: url,
        manifest: source_manifest(),
    };
    let path = ResourcePath {
        resource: "addon_catalog".into(),
        r#type: "all".into(),
        id: id.into(),
        extra: vec![],
    };
    // A larger list is refused while it is read, never kept in the cache.
    let client = core.addons().with_max_bytes(LIST_MAX_BYTES);
    let fetched = async {
        if fresh {
            client.fetch_fresh(&source, &path, FETCH_TIMEOUT).await
        } else {
            client.fetch(&source, &path, FETCH_TIMEOUT).await
        }
    };
    // The deadline covers the wait for a host slot too.
    let body = tokio::time::timeout(TIMEOUT, fetched)
        .await
        .map_err(|_| list_failure(&AddonError::TimedOut))?
        .map_err(|e| list_failure(&e))?
        .body;
    // A list cached before the limit.
    if body.len() > LIST_MAX_BYTES {
        return Err(UNREADABLE.into());
    }
    let answer: Value = serde_json::from_slice(&body).map_err(|_| UNREADABLE.to_owned())?;
    let raw = answer["addons"].as_array().ok_or(UNREADABLE)?;
    Ok(entries(raw))
}

/// The entries the list may show, in the served order: each checked
/// (`listable`, not adult, within the caps), the malformed ones skipped,
/// duplicates of a canonical link dropped, at most `MAX_ENTRIES`.
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
    let provides: Vec<String> = manifest
        .resources
        .iter()
        .map(|r| match r {
            ManifestResource::Short(name) => name.clone(),
            ManifestResource::Full { name, .. } => name.clone(),
        })
        .collect();
    let short =
        |words: &[String]| words.len() <= WORDS_MAX && words.iter().all(|w| w.len() <= WORD_MAX);
    let name = cut(&collapse(&manifest.name), NAME_MAX);
    if hints.adult
        || name.is_empty()
        || manifest.id.len() > ID_MAX
        || !short(&manifest.types)
        || !short(&provides)
    {
        return None;
    }
    let configuration = if hints.configuration_required {
        Configuration::Required
    } else if hints.configurable {
        Configuration::Optional
    } else {
        Configuration::None
    };
    let entry = ListedAddon {
        key: url.to_string(),
        id: manifest.id.clone(),
        host: url.host_str()?.to_owned(),
        name,
        description: manifest
            .description
            .as_deref()
            .map(|d| cut(&plain(d), DESCRIPTION_MAX))
            .filter(|d| !d.is_empty()),
        types: manifest.types.clone(),
        provides,
        configuration,
        configure_url: configure_url(&url).ok()?,
        installed: Installed::Absent,
        copy_key: None,
    };
    // Room left for the copy key `mark_installed` may add.
    (serde_json::to_vec(&entry).ok()?.len() + COPY_KEY_MAX <= ENTRY_MAX).then_some(entry)
}

/// Text without markup: tags dropped (a `<` that opens none stays), the
/// common entities decoded once, whitespace collapsed (`collapse`).
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let opens = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        match after.find('>').filter(|_| opens) {
            Some(end) => {
                out.push(' ');
                rest = &after[end + 1..];
            }
            None => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    collapse(&entities(&out))
}

/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;` and numeric
/// references, decoded once (`&amp;lt;` reads `&lt;`); any other `&` stays.
fn entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let named = after.find(';').filter(|&end| end <= 8);
        match named.and_then(|end| Some((entity(&after[..end])?, end))) {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some(c)
}

/// One space between words; control characters count as spaces.
fn collapse(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Sets each entry's `installed` (and `copyKey`) from the profile's addons.
/// Addons that cannot be read fail every loaded list: a state Marquee does
/// not know never shows as absent.
fn mark_installed(core: &Core, snapshot: &mut AddonCatalogSnapshot) {
    let rows = match core.library().addons(core.profile()) {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("Find addons could not read the profile's addons: {e}");
            for list in &mut snapshot.lists {
                if list.state == ListState::Ready {
                    list.state = ListState::Failed {
                        words: "Marquee could not read your addons".into(),
                    };
                    list.entries.clear();
                }
            }
            return;
        }
    };
    let installed: Vec<Row> = rows.iter().map(Row::new).collect();
    for entry in snapshot.lists.iter_mut().flat_map(|l| l.entries.iter_mut()) {
        (entry.installed, entry.copy_key) = installed_state(entry, &installed);
    }
}

/// An installed addon as the lists compare it. A row whose manifest or link
/// cannot be read still matches its exact link; it is nobody's copy.
struct Row {
    url: String,
    stored: String,
    id: Option<String>,
    host: Option<String>,
    enabled: bool,
}

impl Row {
    fn new(row: &AddonRow) -> Row {
        let url = canonical(&row.transport_url);
        let manifest: Option<Value> = serde_json::from_str(&row.manifest_json).ok();
        Row {
            id: manifest.and_then(|m| Some(m["id"].as_str()?.to_owned())),
            host: Url::parse(&url)
                .ok()
                .and_then(|u| Some(u.host_str()?.to_owned())),
            url,
            stored: row.transport_url.clone(),
            enabled: row.enabled,
        }
    }
}

fn installed_state(entry: &ListedAddon, rows: &[Row]) -> (Installed, Option<String>) {
    // The row Turn on and the other commands would act on.
    let links = rows
        .iter()
        .map(|row| (row.stored.as_str(), row.url.as_str(), row.enabled));
    if let Some(row) = find_row(links, &entry.key).map(|at| &rows[at]) {
        let state = if row.enabled {
            Installed::On
        } else {
            Installed::Off
        };
        return (state, None);
    }
    let copies: Vec<&Row> = rows
        .iter()
        .filter(|row| {
            row.id.as_deref() == Some(entry.id.as_str())
                && row.host.as_deref() == Some(entry.host.as_str())
        })
        .collect();
    if copies.iter().any(|row| row.enabled) {
        (Installed::CopyOn, None)
    } else if let Some(first) = copies.first() {
        (Installed::CopyOff, copy_key(&first.stored))
    } else {
        (Installed::Absent, None)
    }
}

/// A copy's link for `copyKey`, unless it is over `COPY_KEY_MAX` as JSON.
fn copy_key(stored: &str) -> Option<String> {
    let json = serde_json::to_string(stored).ok()?;
    (json.len() <= COPY_KEY_MAX).then(|| stored.to_owned())
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
        AddonError::Http(HttpError::TooLarge { .. } | HttpError::Decode(_)) => UNREADABLE.into(),
        _ => "Cinemeta did not answer".into(),
    }
}

/// The source as the addon client needs it. Its id keys the health book, so
/// list fetches are kept apart from Cinemeta's own (Home, title pages).
fn source_manifest() -> Manifest {
    serde_json::from_value(serde_json::json!({
        "id": "marquee.addon-lists",
        "version": "0.0.0",
        "name": "Cinemeta",
        "resources": ["addon_catalog"],
        "types": [],
        "catalogs": [],
    }))
    .expect("a valid manifest")
}
