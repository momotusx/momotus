//! Find addons: the two lists Cinemeta publishes (`addon_catalog`), shown
//! safely and installed in one press (plans/m12-addon-discovery.md 3.1 to
//! 3.3). The lists come from Cinemeta's fixed address, whatever the user has
//! installed: "Official" is that address, never a field a publisher controls.
//! After them come the lists of the addons the user installed that publish
//! `addonCatalogs` (plans/plugins-and-marketplace.md 7.1), each named with the
//! host it comes from and never in Cinemeta's slots.

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
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use stremio_core::types::addon::{ManifestCatalog, ManifestResource};
use tokio::task::{AbortHandle, JoinHandle, JoinSet};

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
/// Entries a list from an installed addon may show, and its body
/// (stremio-addons.net's 728 entries were 1.2 MiB on 2026-10-26).
const ADDON_LIST_MAX_ENTRIES: usize = 2000;
const ADDON_LIST_MAX_BYTES: usize = 4 * 1024 * 1024;
/// Every list's entries together, each counted with room for its copy key:
/// one push stays under the bridge's 4 MiB message however many lists the
/// installed addons publish. Lists fill in order; one cut short says so
/// (`more`).
const SNAPSHOT_ENTRIES_MAX: usize = 3 * 1024 * 1024 + 512 * 1024;
/// Lists one installed addon may add, and all installed addons together:
/// with the room above, a push stays under the bridge's 4 MiB message
/// however many `addonCatalogs` a manifest declares.
const LISTS_PER_ADDON: usize = 8;
const ADDON_LISTS_MAX: usize = 32;
/// Installed addons' lists read at once (each body up to 4 MiB).
const FETCHES_AT_ONCE: usize = 2;
/// A listed addon's manifest.
const MANIFEST_MAX_BYTES: usize = 256 * 1024;
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
    /// "official" or "community" for Cinemeta's lists; `addon-<hash>:<id>`
    /// for a list an installed addon publishes.
    pub id: String,
    pub name: String,
    /// The host of the installed addon a list comes from, shown as "From
    /// {host}" and linking to it; none for Cinemeta's two.
    pub host: Option<String>,
    pub state: ListState,
    pub entries: Vec<ListedAddon>,
    /// The list holds more than it shows: its own cap, or the snapshot's room.
    pub more: bool,
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
        let mut lists = Lists::new(source);
        lists.follow_addons(core);
        let snapshot = SharedObservable::new(lists.render(core));
        let task = tokio::spawn(run(core.clone(), fresh, lists, changes, snapshot.clone()));
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

/// Debrid services whose key Torrentio and its forks keep under the
/// service's own name (`realdebrid=<key>`).
const DEBRID_FIELDS: [&str; 7] = [
    "realdebrid",
    "alldebrid",
    "premiumize",
    "torbox",
    "debridlink",
    "offcloud",
    "putio",
];

/// How deep one setting is unwrapped (percent, base64, JSON in a string).
const UNWRAP_DEPTH: u8 = 5;

/// Whether an addon link carries a sign-in in its settings: a path segment
/// before `manifest.json`, or a query parameter, whose name or contents
/// name a token, an API key, a secret or a debrid key. Contents are read
/// percent-decoded (twice encoded too), as base64 (leniently), as JSON
/// (strings inside it again) and as `key=value` pairs, at any depth. Such
/// a link from someone else gives their account away; Find addons never
/// lists one, and installing one by paste is confirmed first
/// (plans/m13-default-addons.md, decision 11). This is a guard against
/// sharing an account by mistake, not a security boundary.
pub fn carries_token(url: &Url) -> bool {
    let config = url
        .path_segments()
        .map(|s| {
            let s: Vec<&str> = s.collect();
            s[..s.len().saturating_sub(1)]
                .iter()
                .any(|segment| holds_token(segment, UNWRAP_DEPTH))
        })
        .unwrap_or(false);
    config
        || url.query_pairs().any(|(name, value)| {
            (token_name(&name) && !value.trim().is_empty()) || holds_token(&value, UNWRAP_DEPTH)
        })
}

/// `carries_token` for a link as the user typed or pasted it.
pub fn link_carries_token(raw: &str) -> bool {
    normalize_transport_url(raw).is_ok_and(|url| carries_token(&url))
}

/// A setting's name that holds a secret, whatever its case and separators:
/// `access_token`, `accessToken`, `api-key`, `clientSecret`, `realdebrid`.
fn token_name(name: &str) -> bool {
    let name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    name.ends_with("token")
        || name.ends_with("apikey")
        || name.ends_with("secret")
        || name.ends_with("password")
        || DEBRID_FIELDS.contains(&name.as_str())
}

/// Whether a piece of a link's settings holds a secret, unwrapping it
/// `depth` times at most.
fn holds_token(text: &str, depth: u8) -> bool {
    let Some(depth) = depth.checked_sub(1) else {
        return false;
    };
    let decoded = percent_encoding::percent_decode_str(text).decode_utf8_lossy();
    if decoded != text {
        return holds_token(&decoded, depth);
    }
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return json_holds_token(&value, depth);
    }
    let pairs = text.split(['|', '&', ';']).any(|pair| {
        pair.split_once('=').is_some_and(|(name, value)| {
            (token_name(name) && !value.trim().is_empty()) || holds_token(value, depth)
        })
    });
    pairs || base64_text(text).is_some_and(|t| holds_token(&t, depth))
}

fn json_holds_token(value: &Value, depth: u8) -> bool {
    match value {
        Value::Object(o) => o.iter().any(|(k, v)| {
            let filled =
                !matches!(v, Value::Null) && v.as_str().is_none_or(|s| !s.trim().is_empty());
            (token_name(k) && filled) || json_holds_token(v, depth)
        }),
        Value::Array(a) => a.iter().any(|v| json_holds_token(v, depth)),
        Value::String(s) => holds_token(s, depth),
        _ => false,
    }
}

/// Text read as base64 the way browsers and Node do: either alphabet,
/// padding or not, stray characters and trailing bits ignored. `None` when
/// it is too short to hold anything or does not decode to text.
fn base64_text(text: &str) -> Option<String> {
    use base64::Engine as _;
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    const LENIENT: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    let mut clean: String = text
        .chars()
        .filter_map(|c| match c {
            '-' => Some('+'),
            '_' => Some('/'),
            c if c.is_ascii_alphanumeric() || c == '+' || c == '/' => Some(c),
            _ => None,
        })
        .collect();
    if clean.len() % 4 == 1 {
        clean.pop();
    }
    if clean.len() < 8 {
        return None;
    }
    String::from_utf8(LENIENT.decode(&clean).ok()?).ok()
}

/// The configure page of an addon whose manifest says it has one.
pub(crate) fn configure_page(url: &Url, manifest: &Manifest) -> Option<String> {
    let hints = &manifest.behavior_hints;
    if !(hints.configurable || hints.configuration_required) {
        return None;
    }
    configure_url(url).ok()
}

/// Where one list comes from.
#[derive(Clone)]
struct ListSource {
    id: String,
    name: String,
    host: Option<String>,
    addon: InstalledAddon,
    catalog: (String, String),
}

impl ListSource {
    /// The same list from the same link: a held list stays as it is.
    fn same(&self, other: &ListSource) -> bool {
        (
            &self.id,
            &self.name,
            &self.catalog,
            &self.addon.transport_url,
        ) == (
            &other.id,
            &other.name,
            &other.catalog,
            &other.addon.transport_url,
        )
    }
}

/// One list as the view holds it: every entry it read with its size as
/// JSON (measured once, as read), before the snapshot's room is shared out.
struct Held {
    source: ListSource,
    state: ListState,
    entries: Vec<ListedAddon>,
    sizes: Vec<usize>,
    more: bool,
    /// The fetch whose answer this list takes: an older one's is ignored.
    generation: u64,
}

impl Held {
    fn loading(source: ListSource) -> Self {
        Self {
            source,
            state: ListState::Loading,
            entries: Vec::new(),
            sizes: Vec::new(),
            more: false,
            generation: 0,
        }
    }
}

/// The lists in order (Cinemeta's two, then the installed addons' in addon
/// order) and the fetches still running.
struct Lists {
    cinemeta: Url,
    held: Vec<Held>,
}

impl Lists {
    fn new(source: &str) -> Self {
        let cinemeta = Url::parse(source).unwrap_or_else(|_| Url::parse(CINEMETA).expect("a URL"));
        let addon = InstalledAddon {
            transport_url: cinemeta.clone(),
            manifest: source_manifest(),
        };
        let held = LISTS
            .iter()
            .map(|(id, name)| {
                Held::loading(ListSource {
                    id: (*id).into(),
                    name: (*name).into(),
                    host: None,
                    addon: addon.clone(),
                    catalog: ("all".into(), (*id).into()),
                })
            })
            .collect();
        Self { cinemeta, held }
    }

    /// Brings the installed addons' lists in line with the profile's
    /// addons, keeping lists that stay; returns the ones to fetch.
    fn follow_addons(&mut self, core: &Core) -> Vec<ListSource> {
        let wanted = addon_lists(core, &self.cinemeta);
        let mut kept: HashMap<String, Held> = self
            .held
            .drain(LISTS.len()..)
            .map(|held| (held.source.id.clone(), held))
            .collect();
        let mut fetch = Vec::new();
        for source in wanted {
            match kept.remove(&source.id) {
                Some(held) if held.source.same(&source) => self.held.push(held),
                _ => {
                    fetch.push(source.clone());
                    self.held.push(Held::loading(source));
                }
            }
        }
        fetch
    }

    /// A fetch's answer, for the list it was started for: a list gone, or
    /// started again since, takes none.
    fn set(&mut self, id: &str, generation: u64, result: Fetched) {
        let Some(held) = self
            .held
            .iter_mut()
            .find(|h| h.source.id == id && h.generation == generation)
        else {
            return;
        };
        match result {
            Ok((entries, more)) => {
                held.state = ListState::Ready;
                held.sizes = entries.iter().map(json_size).collect();
                held.entries = entries;
                held.more = more;
            }
            Err(words) => held.state = ListState::Failed { words },
        }
    }

    fn ids(&self) -> HashSet<&str> {
        self.held.iter().map(|h| h.source.id.as_str()).collect()
    }

    /// The snapshot: lists in order with their installed states, each given
    /// entries while the room lasts. An entry's size is the one measured as
    /// read, grown by what its installed state adds (`copyKey`, a longer
    /// state word), so nothing is serialized again here. A list cut short
    /// says `more`; a later list that fits is still shown whole.
    fn render(&self, core: &Core) -> AddonCatalogSnapshot {
        let lists = self
            .held
            .iter()
            .map(|held| AddonList {
                id: held.source.id.clone(),
                name: held.source.name.clone(),
                host: held.source.host.clone(),
                state: held.state.clone(),
                entries: held.entries.clone(),
                more: held.more,
            })
            .collect();
        let mut snapshot = AddonCatalogSnapshot { lists };
        mark_installed(core, &mut snapshot);
        let mut room = SNAPSHOT_ENTRIES_MAX;
        for (list, held) in snapshot.lists.iter_mut().zip(&self.held) {
            let fits = list
                .entries
                .iter()
                .zip(&held.sizes)
                .take_while(|(entry, size)| {
                    let size = *size + grown(entry);
                    room.checked_sub(size).map(|left| room = left).is_some()
                })
                .count();
            if fits < list.entries.len() {
                list.entries.truncate(fits);
                list.more = true;
            }
        }
        snapshot
    }
}

/// An entry's size as JSON, as read (absent, no copy key).
fn json_size(entry: &ListedAddon) -> usize {
    serde_json::to_vec(entry).map_or(ENTRY_MAX, |b| b.len())
}

/// What an entry's installed state adds to its size as read: a copy key in
/// place of `null`, and a state word longer than `absent`.
fn grown(entry: &ListedAddon) -> usize {
    let word = serde_json::to_vec(&entry.installed).map_or(0, |b| b.len());
    let key = entry
        .copy_key
        .as_ref()
        .and_then(|k| serde_json::to_vec(k).ok())
        .map_or(0, |b| b.len());
    word.saturating_sub("\"absent\"".len()) + key.saturating_sub("null".len())
}

/// The lists the installed, enabled addons publish, in addon order: one per
/// `addonCatalogs` id (its `all` type when it has one) that needs no extra.
/// Cinemeta, whose two lists come first anyway, and addons on a private
/// host are left out.
fn addon_lists(core: &Core, cinemeta: &Url) -> Vec<ListSource> {
    let addons = core.installed_addons().unwrap_or_default();
    let mut lists: Vec<ListSource> = Vec::new();
    for addon in addons {
        let url = &addon.transport_url;
        let Some(host) = url.host_str().map(str::to_owned) else {
            continue;
        };
        if url == cinemeta
            || addon.manifest.id == "com.linvo.cinemeta"
            || mq_http::is_private_host(&host)
        {
            continue;
        }
        let mut seen = HashSet::new();
        let own = catalogs(&addon.manifest.addon_catalogs)
            .into_iter()
            .filter(|catalog| seen.insert(catalog.id.clone()))
            .take(LISTS_PER_ADDON);
        for catalog in own {
            let name = cut(
                &collapse(catalog.name.as_deref().unwrap_or(&addon.manifest.name)),
                NAME_MAX,
            );
            lists.push(ListSource {
                id: format!("addon-{:016x}:{}", link_hash(url), cut(&catalog.id, ID_MAX)),
                name: if name.is_empty() { host.clone() } else { name },
                host: Some(host.clone()),
                addon: addon.clone(),
                catalog: (catalog.r#type.clone(), catalog.id.clone()),
            });
        }
    }
    lists.truncate(ADDON_LISTS_MAX);
    name_apart(&mut lists);
    lists
}

/// A list whose name is Cinemeta's ("Official", "Community") or another
/// list's, whatever the case, is named with its host too: no client can show
/// two lists with one name, and none but Cinemeta's reads "Official".
fn name_apart(lists: &mut [ListSource]) {
    let key = |name: &str| name.to_lowercase();
    let mut count: HashMap<String, usize> = LISTS.iter().map(|(_, name)| (key(name), 1)).collect();
    for list in lists.iter() {
        *count.entry(key(&list.name)).or_default() += 1;
    }
    for list in lists.iter_mut() {
        if count[&key(&list.name)] > 1
            && let Some(host) = &list.host
        {
            list.name = format!("{}, {host}", list.name);
        }
    }
}

/// An addon's catalogs that need no extra, each id's `all` type first.
fn catalogs(all: &[ManifestCatalog]) -> Vec<&ManifestCatalog> {
    let mut usable: Vec<&ManifestCatalog> = all
        .iter()
        .filter(|c| c.is_extra_supported(&[]) && c.id.len() <= ID_MAX && c.r#type.len() <= WORD_MAX)
        .collect();
    usable.sort_by_key(|c| c.r#type != "all");
    let order: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
    usable.sort_by_key(|c| order.iter().position(|id| *id == c.id));
    usable
}

/// A stable name for an addon's link within this build, never the link
/// itself (it may hold keys).
// ponytail: the std hasher, fixed keys; lists only need ids that hold
// while the page is open.
fn link_hash(url: &Url) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical(url.as_str()).hash(&mut hasher);
    hasher.finish()
}

/// A list's entries and whether it held more, or the words of its failure.
type Fetched = Result<(Vec<ListedAddon>, bool), String>;

/// The fetches running, by list: each with the generation it was started
/// as, and the task it runs in.
struct Fetches {
    tasks: JoinSet<(String, u64, Fetched)>,
    running: HashMap<String, AbortHandle>,
    by_task: HashMap<tokio::task::Id, (String, u64)>,
    generation: u64,
    /// Installed addons' lists only: Cinemeta's two never wait.
    slots: Arc<tokio::sync::Semaphore>,
}

impl Fetches {
    fn new() -> Self {
        Self {
            tasks: JoinSet::new(),
            running: HashMap::new(),
            by_task: HashMap::new(),
            generation: 0,
            slots: Arc::new(tokio::sync::Semaphore::new(FETCHES_AT_ONCE)),
        }
    }

    /// Starts `list`'s fetch, ending one still running for the same id.
    fn start(&mut self, core: &Core, lists: &mut Lists, list: ListSource, fresh: bool) {
        self.generation += 1;
        let generation = self.generation;
        if let Some(held) = lists.held.iter_mut().find(|h| h.source.id == list.id) {
            held.generation = generation;
        }
        let (core, slots, id) = (core.clone(), self.slots.clone(), list.id.clone());
        let handle = self.tasks.spawn(async move {
            let _slot = match list.host {
                Some(_) => Some(slots.acquire_owned().await),
                None => None,
            };
            let result = fetch_list(&core, &list, fresh).await;
            (list.id, generation, result)
        });
        self.by_task.insert(handle.id(), (id.clone(), generation));
        if let Some(old) = self.running.insert(id, handle) {
            old.abort();
        }
    }

    /// Ends the fetches of lists that are gone.
    fn keep_only(&mut self, ids: &HashSet<&str>) {
        self.running.retain(|id, handle| {
            let kept = ids.contains(id.as_str());
            if !kept {
                handle.abort();
            }
            kept
        });
    }
}

async fn run(
    core: Core,
    fresh: bool,
    mut lists: Lists,
    mut changes: tokio::sync::broadcast::Receiver<Vec<Table>>,
    snapshot: SharedObservable<AddonCatalogSnapshot>,
) {
    let mut fetches = Fetches::new();
    let first: Vec<ListSource> = lists.held.iter().map(|h| h.source.clone()).collect();
    for list in first {
        fetches.start(&core, &mut lists, list, fresh);
    }
    let publish = |lists: &Lists| {
        let next = lists.render(&core);
        if next != snapshot.get() {
            snapshot.set(next);
        }
    };
    loop {
        tokio::select! {
            // Taken whole: an aborted or failed task is handled here, so the
            // arm is never left disabled for the others still running.
            Some(joined) = fetches.tasks.join_next_with_id(), if !fetches.tasks.is_empty() => {
                let (id, generation, result) = match joined {
                    Ok((task, answer)) => {
                        fetches.by_task.remove(&task);
                        answer
                    }
                    Err(error) => {
                        let Some((id, generation)) = fetches.by_task.remove(&error.id()) else {
                            continue;
                        };
                        if !error.is_panic() {
                            continue;
                        }
                        tracing::warn!("a Find addons list fetch failed: {error}");
                        (id, generation, Err("Momotus could not read this list".into()))
                    }
                };
                if fetches.running.get(&id).is_some_and(|h| !fetches.by_task.contains_key(&h.id())) {
                    fetches.running.remove(&id);
                }
                lists.set(&id, generation, result);
                publish(&lists);
            }
            change = crate::views::next_change(&mut changes) => {
                let Some(tables) = change else { return };
                if tables.contains(&Table::Addons) {
                    for list in lists.follow_addons(&core) {
                        fetches.start(&core, &mut lists, list, fresh);
                    }
                    fetches.keep_only(&lists.ids());
                    publish(&lists);
                }
            }
        }
    }
}

/// One list from its source, its entries checked and capped; whether the
/// source held more than the cap.
async fn fetch_list(core: &Core, list: &ListSource, fresh: bool) -> Fetched {
    let who = list.host.as_deref().unwrap_or("Cinemeta");
    let (max_entries, max_bytes) = if list.host.is_some() {
        (ADDON_LIST_MAX_ENTRIES, ADDON_LIST_MAX_BYTES)
    } else {
        (MAX_ENTRIES, LIST_MAX_BYTES)
    };
    let path = ResourcePath {
        resource: "addon_catalog".into(),
        r#type: list.catalog.0.clone(),
        id: list.catalog.1.clone(),
        extra: vec![],
    };
    // A larger list is refused while it is read, never kept in the cache.
    let client = core.addons().with_max_bytes(max_bytes);
    let fetched = async {
        if fresh {
            client.fetch_fresh(&list.addon, &path, FETCH_TIMEOUT).await
        } else {
            client.fetch(&list.addon, &path, FETCH_TIMEOUT).await
        }
    };
    // The deadline covers the wait for a host slot too.
    let body = tokio::time::timeout(TIMEOUT, fetched)
        .await
        .map_err(|_| list_failure(&AddonError::TimedOut, who))?
        .map_err(|e| list_failure(&e, who))?
        .body;
    let unreadable = || format!("{who} sent a list Momotus cannot read");
    // A list cached before the limit.
    if body.len() > max_bytes {
        return Err(unreadable());
    }
    read_list(&body, max_entries).ok_or_else(unreadable)
}

/// A list's body read one entry at a time: the `addons` array is held as
/// raw slices of the body, each turned into a checked entry and dropped,
/// never one tree of the whole list (4 MiB of JSON is tens of MiB as a
/// tree). `None` when the body is not such a list.
pub fn read_list(body: &[u8], max: usize) -> Option<(Vec<ListedAddon>, bool)> {
    #[derive(serde::Deserialize)]
    struct Answer<'a> {
        #[serde(borrow)]
        addons: Vec<&'a serde_json::value::RawValue>,
    }
    let answer: Answer = serde_json::from_slice(body).ok()?;
    let mut seen = HashSet::new();
    let mut shown = Vec::new();
    for raw in answer.addons {
        let Ok(value) = serde_json::from_str::<Value>(raw.get()) else {
            continue;
        };
        let Some(entry) = entry(&value) else { continue };
        if !seen.insert(entry.key.clone()) {
            continue;
        }
        if shown.len() == max {
            return Some((shown, true));
        }
        shown.push(entry);
    }
    Some((shown, false))
}

/// The entries the list may show, in the served order: each checked
/// (`listable`, not adult, within the caps), the malformed ones skipped,
/// duplicates of a canonical link dropped, at most `MAX_ENTRIES`.
pub fn entries(raw: &[Value]) -> Vec<ListedAddon> {
    entries_up_to(raw, MAX_ENTRIES).0
}

/// `entries` with another cap; also whether the list held more.
fn entries_up_to(raw: &[Value], max: usize) -> (Vec<ListedAddon>, bool) {
    let mut seen = HashSet::new();
    let mut shown: Vec<ListedAddon> = raw
        .iter()
        .filter_map(entry)
        .filter(|entry| seen.insert(entry.key.clone()))
        .take(max + 1)
        .collect();
    let more = shown.len() > max;
    shown.truncate(max);
    (shown, more)
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
                        words: "Momotus could not read your addons".into(),
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

/// The words of a list that could not be read, naming its source.
fn list_failure(error: &AddonError, who: &str) -> String {
    match error {
        AddonError::TimedOut | AddonError::Http(HttpError::Timeout(_)) => {
            format!("{who} did not answer in time")
        }
        AddonError::Http(HttpError::Status { status, .. }) => {
            format!("{who} answered with an error ({status})")
        }
        AddonError::Http(HttpError::TooLarge { .. } | HttpError::Decode(_)) => {
            format!("{who} sent a list Momotus cannot read")
        }
        _ => format!("{who} did not answer"),
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
