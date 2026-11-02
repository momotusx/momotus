//! Nuvio plugin repositories (plans/plugins-and-marketplace.md 6, 7): a
//! profile keeps repository links; each repository's manifest lists
//! providers, each a script that `getStreams` runs in the sandboxed engine
//! (`mq-plugins`) when Versions opens. Their links become rows of a pass of
//! their own, "From plugins": the pick and the failover only when no addon
//! row is left (owner, 2026-11-02), played through a handle that reaches
//! public addresses only.
//!
//! Two repositories come as defaults, on (owner, 2026-10-30, R7). A kids
//! profile has none and runs none.
//!
//! As built, against the plan: a repository's consent covers all of its
//! providers, which may contact any public website (7.5's any-site words);
//! the per-site grants, update holds and their tokens (7.3, 7.6) are later
//! work.

use crate::Core;
use crate::core::CoreError;
use mq_http::Url;
use mq_library::{PluginCode, PluginRepoRow};
use mq_streams::ParsedStream;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A row key's prefix: `plugin:{repo}/{provider}:{url digest}` (4.4).
pub const KEY_PREFIX: &str = "plugin:";
/// The scheme of the link plugin rows carry as their install.
pub const LINK_SCHEME: &str = "mq-plugin";

/// A repository given to every profile once, on (R7).
#[derive(Debug, Clone)]
pub struct PluginDefault {
    pub key: String,
    pub url: String,
    pub name: String,
    pub line: String,
}

/// The defaults: Turkish streams and streams from abroad, as Nuvio
/// repositories (the lead's pick, checked live 2026-10-30).
fn builtin_defaults() -> Vec<PluginDefault> {
    let one = |key: &str, url: &str, name: &str, line: &str| PluginDefault {
        key: key.into(),
        url: url.into(),
        name: name.into(),
        line: line.into(),
    };
    vec![
        one(
            "nuvio-tr",
            "https://raw.githubusercontent.com/manitux-app/nuvio-test-plugins/refs/heads/main/manifest.json",
            "Manitux",
            "Direct links to Turkish films and series from Turkish sites.",
        ),
        one(
            "nuvio-world",
            "https://raw.githubusercontent.com/yoruix/nuvio-providers/refs/heads/main/manifest.json",
            "Yoru's Repo",
            "Direct links to films and series from sites abroad.",
        ),
    ]
}

/// A manifest is read again once this old (7.6).
const STALE_MS: i64 = 6 * 3_600_000;
/// A script is fetched again once this old, or when its version changes.
const CODE_STALE_MS: i64 = 24 * 3_600_000;
const MANIFEST_BYTES: usize = 256 * 1024;
const SCRIPT_BYTES: usize = 2 << 20;
const MAX_ENTRIES: usize = 200;
const ASK_WAIT: Duration = Duration::from_secs(10);
/// Plugin calls running at once on the device (6.2's pool).
const POOL: usize = 3;
/// A plugin's answer for one video is kept this long, in memory only (6.2).
const RESULT_KEPT: Duration = Duration::from_secs(600);
const NAME_MAX: usize = 80;
const TEXT_MAX: usize = 300;

/// The device's plugin state: the pool, the results kept, the defaults.
pub(crate) struct Plugins {
    #[cfg_attr(not(feature = "plugins"), allow(dead_code))]
    pool: tokio::sync::Semaphore,
    results: Mutex<HashMap<String, (Instant, Vec<ParsedStream>)>>,
    defaults: Vec<PluginDefault>,
    /// The directory page's links and the manifests read from it, each kept a day (`directory`).
    pub(crate) listed: crate::plugin_directory::Kept,
}

impl Plugins {
    pub(crate) fn new(defaults: Option<Vec<PluginDefault>>) -> Self {
        Self {
            pool: tokio::sync::Semaphore::new(POOL),
            results: Mutex::default(),
            listed: crate::plugin_directory::Kept::default(),
            defaults: defaults
                .or_else(dev_defaults)
                .unwrap_or_else(builtin_defaults),
        }
    }
}

/// Development builds only: `MQ_DEV_PLUGINS=off` gives no repository as a
/// default, and `MQ_DEV_PLUGINS=<link>[,<link>]` gives these instead, so
/// the web e2e never asks a real repository and can offer its fixture.
fn dev_defaults() -> Option<Vec<PluginDefault>> {
    #[cfg(debug_assertions)]
    {
        let links = std::env::var("MQ_DEV_PLUGINS").ok()?;
        let links = if links == "off" { String::new() } else { links };
        Some(
            (links.split(',').filter(|l| !l.is_empty()).enumerate())
                .map(|(i, url)| PluginDefault {
                    key: format!("dev-plugins-{i}"),
                    url: url.to_owned(),
                    name: "Test plugins".into(),
                    line: "Direct links from the test repository.".into(),
                })
                .collect(),
        )
    }
    #[cfg(not(debug_assertions))]
    None
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// One provider of a repository, as its manifest lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub script: Url,
    /// "movie" and "tv", as Nuvio names them.
    pub types: Vec<String>,
    pub languages: Vec<String>,
    /// The repository lists it as on (`enabled`, true when absent).
    pub listed_on: bool,
}

/// A repository's manifest, read and checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Repo {
    pub name: String,
    pub description: String,
    pub providers: Vec<Provider>,
}

/// A repository as the apps show it (`plugins`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct PluginRepoInfo {
    pub url: String,
    /// The host it is read from, in punycode.
    pub host: String,
    /// The manifest's name; null before the first read.
    pub name: Option<String>,
    pub description: String,
    pub enabled: bool,
    /// The default this is (`defaultAddons` key), if one.
    pub default_key: Option<String>,
    /// Why the last read failed, when no manifest was ever read.
    pub problem: Option<String>,
    pub providers: Vec<PluginInfo>,
}

/// One provider as the apps show it, with its switch.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub languages: Vec<String>,
    /// "movie", "series".
    pub types: Vec<String>,
    pub enabled: bool,
}

/// The `plugins` answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct PluginsAnswer {
    /// This build runs plugins (full builds only) and the profile is not
    /// a kids profile.
    pub available: bool,
    pub repos: Vec<PluginRepoInfo>,
}

/// What a link would add, for the consent sheet (`previewPluginRepo`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct PluginRepoPreview {
    /// The link as it will be kept.
    pub url: String,
    pub host: String,
    pub name: String,
    pub description: String,
    pub providers: Vec<String>,
}

/// Control characters and those that change how text reads without showing
/// dropped, and emoji read as a space ("🧲 Torrentio" reads "Torrentio",
/// "4K🔥HDR" reads "4K HDR"), runs of spaces as one; at most `max` characters.
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !crate::archive::hidden(*c))
        .map(|c| if pictograph(c) { ' ' } else { c })
        .take(max)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Emoji and pictographs, with the selectors and keycap mark that build
/// them: no screen shows emoji, and plugin repositories put them in names.
// ponytail: blocks, not Extended_Pictographic: a plain symbol in them (a star,
// a check mark) goes too, and a text-default one outside them (the trade
// mark, an arrow) stays. Letters of every script are outside them.
fn pictograph(c: char) -> bool {
    matches!(c, '\u{20e3}' | '\u{231a}' | '\u{231b}' | '\u{23e9}'..='\u{23fa}' | '\u{2600}'..='\u{27bf}' | '\u{2b00}'..='\u{2bff}' | '\u{fe0e}' | '\u{fe0f}' | '\u{1f000}'..='\u{1faff}')
}

/// A repository link as Nuvio keeps it (2.1): https added when no scheme
/// is given, the fragment dropped, `/manifest.json` added to a path that
/// names no JSON file. https only; no userinfo.
pub fn normalize_url(input: &str) -> Result<Url, CoreError> {
    let input = input.trim();
    let with_scheme = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let bad = || CoreError::Plugins("That is not a repository link.".into());
    let mut url = Url::parse(&with_scheme).map_err(|_| bad())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(CoreError::Plugins(
            "A repository link starts with https.".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad());
    }
    url.set_fragment(None);
    if !url.path().ends_with(".json") {
        let path = format!("{}/manifest.json", url.path().trim_end_matches('/'));
        url.set_path(&path);
    }
    Ok(url)
}

pub(crate) fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default()
}

fn text(v: &Value, key: &str, max: usize) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .map(|s| clean(s, max))
        .unwrap_or_default()
}

/// A repository's or provider's name, without the punctuation some put
/// first to sort it first ("!DiziBal" reads "DiziBal").
fn label(v: &Value) -> String {
    let name = text(v, "name", NAME_MAX);
    (name.trim_start_matches(['!', '.', '_', '-', '*', '~', '+']))
        .trim_start()
        .to_owned()
}

fn words(v: &Value, key: &str) -> Vec<String> {
    (v.get(key).and_then(Value::as_array).into_iter().flatten())
        .filter_map(Value::as_str)
        .map(|s| clean(s, 16))
        .filter(|s| !s.is_empty())
        .take(16)
        .collect()
}

fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Reads a manifest fetched from `base`: an object with a non-empty
/// `scrapers` list (2.1). An entry without a plain id or a script link
/// over http(s) is left out; ids are unique, the first one kept.
pub(crate) fn parse_manifest(base: &Url, body: &[u8]) -> Result<Repo, String> {
    let v: Value =
        serde_json::from_slice(body).map_err(|_| "Its manifest is not JSON.".to_owned())?;
    let entries = v
        .get("scrapers")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or("Its manifest lists no providers.")?;
    let mut providers: Vec<Provider> = Vec::new();
    for e in entries.iter().take(MAX_ENTRIES) {
        let id = e.get("id").and_then(Value::as_str).unwrap_or_default();
        let Some(file) = e.get("filename").and_then(Value::as_str) else {
            continue;
        };
        let Ok(script) = base.join(file) else {
            continue;
        };
        if !plain_id(id)
            || !matches!(script.scheme(), "http" | "https")
            || providers.iter().any(|p| p.id == id)
        {
            continue;
        }
        let mut types = words(e, "supportedTypes");
        if types.is_empty() {
            types = vec!["movie".into(), "tv".into()];
        }
        let name = label(e);
        providers.push(Provider {
            id: id.to_owned(),
            name: if name.is_empty() { id.to_owned() } else { name },
            version: text(e, "version", 32),
            description: text(e, "description", TEXT_MAX),
            script,
            types,
            languages: words(e, "contentLanguage"),
            listed_on: e.get("enabled").and_then(Value::as_bool) != Some(false),
        });
    }
    if providers.is_empty() {
        return Err("Its manifest lists no providers.".into());
    }
    let name = label(&v);
    Ok(Repo {
        name: if name.is_empty() {
            base.host_str().unwrap_or_default().to_owned()
        } else {
            name
        },
        description: text(&v, "description", TEXT_MAX),
        providers,
    })
}

/// The handle every plugin request goes out on: public addresses only, no
/// install scope (6.4).
fn public(core: &Core) -> mq_http::HttpClient {
    core.http().scoped(&[])
}

pub(crate) async fn fetch_manifest(core: &Core, url: &Url) -> Result<(Repo, String), String> {
    let opts = mq_http::RequestOptions {
        timeout: ASK_WAIT,
        max_bytes: MANIFEST_BYTES,
        ..Default::default()
    };
    let host = url.host_str().unwrap_or_default();
    let response = public(core)
        .get(url.as_str(), &opts)
        .await
        .map_err(|e| match e {
            mq_http::HttpError::TooLarge { .. } => "Its manifest is too large.".to_owned(),
            mq_http::HttpError::Status { status, .. } => {
                tracing::debug!("{host} answered a plugin manifest with status {status}");
                format!("{host} answered with an error.")
            }
            _ => format!("Could not reach {host}."),
        })?;
    let repo = parse_manifest(url, &response.body)?;
    let json = String::from_utf8_lossy(&response.body).into_owned();
    Ok((repo, json))
}

/// Whether provider `p` runs: off when the user turned it off, else on
/// when its repository lists it on or the user turned it on (`+id`).
fn provider_on(row: &PluginRepoRow, p: &Provider) -> bool {
    if row.off.contains(&p.id) {
        return false;
    }
    p.listed_on
        || row
            .off
            .iter()
            .any(|x| x.strip_prefix('+') == Some(p.id.as_str()))
}

impl Core {
    /// Whether this build runs plugins: full builds only (6.1).
    pub fn has_plugins(&self) -> bool {
        cfg!(feature = "plugins")
    }

    fn plugins_ok(&self) -> Result<(), CoreError> {
        if !self.has_plugins() {
            return Err(CoreError::Plugins(
                "Plugins are not available in this version.".into(),
            ));
        }
        if self.policy().kids() {
            return Err(CoreError::Plugins(
                "Kids profiles never run plugins.".into(),
            ));
        }
        Ok(())
    }

    /// The defaults this device gives (`PluginDefault`).
    pub fn plugin_defaults(&self) -> &[PluginDefault] {
        &self.plugins().defaults
    }

    /// Reads a repository's manifest again when it was never read or is
    /// over 6 hours old; keeps it on success.
    async fn refresh_repo(&self, row: &PluginRepoRow) -> Result<(), String> {
        let now = mq_library::hlc::now_ms() as i64;
        if row.manifest_json.is_some() && now - row.fetched_at < STALE_MS {
            return Ok(());
        }
        let url = Url::parse(&row.url).map_err(|_| "A bad link.".to_owned())?;
        let (_, json) = fetch_manifest(self, &url).await?;
        let _ = self
            .library()
            .set_plugin_manifest(self.profile(), &row.url, &json, now);
        Ok(())
    }

    /// The profile's repositories and their providers, each manifest read
    /// again when stale (7.6). A kids profile has none.
    pub async fn plugin_repos(&self) -> Result<PluginsAnswer, CoreError> {
        if !self.has_plugins() || self.policy().kids() {
            return Ok(PluginsAnswer {
                available: false,
                repos: Vec::new(),
            });
        }
        let rows = self.library().plugin_repos(self.profile())?;
        let problems: Vec<Result<(), String>> =
            futures_util::future::join_all(rows.iter().map(|r| self.refresh_repo(r))).await;
        let problems: HashMap<&str, String> = (rows.iter().zip(problems))
            .filter_map(|(r, p)| Some((r.url.as_str(), p.err()?)))
            .collect();
        let repos = (self.library().plugin_repos(self.profile())?.iter())
            .map(|row| self.repo_info(row, problems.get(row.url.as_str()).cloned()))
            .collect();
        Ok(PluginsAnswer {
            available: true,
            repos,
        })
    }

    fn repo_info(&self, row: &PluginRepoRow, problem: Option<String>) -> PluginRepoInfo {
        let repo = Url::parse(&row.url)
            .ok()
            .and_then(|base| parse_manifest(&base, row.manifest_json.as_deref()?.as_bytes()).ok());
        let default_key = (self.plugin_defaults().iter())
            .find(|d| d.url == row.url)
            .map(|d| d.key.clone());
        PluginRepoInfo {
            url: row.url.clone(),
            host: host_of(&row.url),
            name: repo.as_ref().map(|r| r.name.clone()),
            description: (repo.as_ref())
                .map(|r| r.description.clone())
                .unwrap_or_default(),
            enabled: row.enabled,
            default_key,
            problem: if repo.is_some() { None } else { problem },
            providers: (repo.iter().flat_map(|r| &r.providers))
                .map(|p| PluginInfo {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    version: p.version.clone(),
                    description: p.description.clone(),
                    languages: p.languages.clone(),
                    types: (p.types.iter())
                        .map(|t| {
                            if t == "tv" {
                                "series".into()
                            } else {
                                t.clone()
                            }
                        })
                        .collect(),
                    enabled: provider_on(row, p),
                })
                .collect(),
        }
    }

    /// What `input` would add, for the consent sheet; writes nothing.
    pub async fn preview_plugin_repo(&self, input: &str) -> Result<PluginRepoPreview, CoreError> {
        self.plugins_ok()?;
        let url = normalize_url(input)?;
        let (repo, _) = fetch_manifest(self, &url)
            .await
            .map_err(CoreError::Plugins)?;
        Ok(PluginRepoPreview {
            host: url.host_str().unwrap_or_default().to_owned(),
            url: url.to_string(),
            name: repo.name,
            description: repo.description,
            providers: repo.providers.into_iter().map(|p| p.name).collect(),
        })
    }

    /// Adds the repository at `input` once the user agreed to the sheet:
    /// its manifest is read first, and nothing is written when that fails.
    /// Its providers start as the repository lists them.
    pub async fn add_plugin_repo(&self, input: &str) -> Result<PluginsAnswer, CoreError> {
        self.plugins_ok()?;
        let url = normalize_url(input)?;
        let (_, json) = fetch_manifest(self, &url)
            .await
            .map_err(CoreError::Plugins)?;
        if !self
            .library()
            .put_plugin_repo(self.profile(), url.as_str(), true)?
        {
            return Err(CoreError::Plugins(format!(
                "A profile keeps at most {} repositories.",
                mq_library::MAX_REPOS
            )));
        }
        let now = mq_library::hlc::now_ms() as i64;
        self.library()
            .set_plugin_manifest(self.profile(), url.as_str(), &json, now)?;
        self.forget_plugin_results();
        self.plugin_repos().await
    }

    pub fn remove_plugin_repo(&self, url: &str) -> Result<(), CoreError> {
        self.plugins_ok()?;
        self.library().remove_plugin_repo(self.profile(), url)?;
        self.forget_plugin_results();
        Ok(())
    }

    /// Turns a whole repository on or off; its providers keep their own
    /// switches.
    pub fn set_plugin_repo_enabled(&self, url: &str, on: bool) -> Result<(), CoreError> {
        self.plugins_ok()?;
        let rows = self.library().plugin_repos(self.profile())?;
        if rows.iter().any(|r| r.url == url) {
            self.library().put_plugin_repo(self.profile(), url, on)?;
        }
        self.forget_plugin_results();
        Ok(())
    }

    /// Turns provider `id` of repository `url` on or off.
    pub fn set_plugin_enabled(&self, url: &str, id: &str, on: bool) -> Result<(), CoreError> {
        self.plugins_ok()?;
        let lib = self.library();
        lib.set_plugin_off(self.profile(), url, id, !on)?;
        lib.set_plugin_off(self.profile(), url, &format!("+{id}"), on)?;
        self.forget_plugin_results();
        Ok(())
    }

    fn forget_plugin_results(&self) {
        lock(&self.plugins().results).clear();
    }

    /// A default's state for `defaultAddons`: absent, on or off.
    pub(crate) fn plugin_default_state(
        &self,
        d: &PluginDefault,
    ) -> crate::defaults::DefaultInstalled {
        use crate::defaults::DefaultInstalled as I;
        let rows = (self.library().plugin_repos(self.profile())).unwrap_or_default();
        match rows.iter().find(|r| r.url == d.url) {
            None => I::Absent,
            Some(r) if r.enabled => I::On,
            Some(_) => I::Off,
        }
    }

    /// Adds or switches the default `key` (onboarding, the recommended
    /// list). False when no default has that key.
    pub(crate) fn choose_plugin_default(&self, key: &str, on: bool) -> Result<bool, CoreError> {
        let Some(d) = self.plugin_defaults().iter().find(|d| d.key == key) else {
            return Ok(false);
        };
        self.library().put_plugin_repo(self.profile(), &d.url, on)?;
        self.forget_plugin_results();
        Ok(true)
    }

    /// Gives this profile each default it never had, on (R7): one the user
    /// removed stays removed. Nothing for a kids profile or in a build
    /// without plugins. No network: the manifests are read on first use.
    pub(crate) fn give_plugin_defaults(&self) -> Result<(), CoreError> {
        if !self.has_plugins() || self.policy().kids() {
            return Ok(());
        }
        for d in self.plugin_defaults() {
            if !self.library().plugin_repo_known(self.profile(), &d.url)? {
                self.library()
                    .put_plugin_repo(self.profile(), &d.url, true)?;
            }
        }
        Ok(())
    }

    /// Whether Versions has a plugin repository to ask for this profile.
    pub(crate) fn plugins_wanted(&self) -> bool {
        self.has_plugins()
            && !self.policy().kids()
            && (self.library().plugin_repos(self.profile()))
                .unwrap_or_default()
                .iter()
                .any(|r| r.enabled)
    }
}

/// What one Versions open asks the plugins.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "plugins"), allow(dead_code))]
pub(crate) struct Ask {
    /// "movie" or "series".
    pub kind: String,
    pub imdb: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub title: String,
    pub year: Option<u16>,
    /// The video, for the results kept.
    pub video_id: String,
}

/// The providers to run for `ask`, with their repository's link: every
/// one switched on in an enabled repository that lists the kind.
pub(crate) async fn providers_for(core: &Core, ask: &Ask) -> Vec<(String, Provider)> {
    let rows = (core.library().plugin_repos(core.profile())).unwrap_or_default();
    let enabled = rows.iter().filter(|r| r.enabled);
    futures_util::future::join_all(enabled.map(|r| core.refresh_repo(r))).await;
    let rows = (core.library().plugin_repos(core.profile())).unwrap_or_default();
    let nuvio_kind = if ask.kind == "series" { "tv" } else { "movie" };
    let mut out = Vec::new();
    for row in rows.iter().filter(|r| r.enabled) {
        let Some(repo) = Url::parse(&row.url)
            .ok()
            .and_then(|base| parse_manifest(&base, row.manifest_json.as_deref()?.as_bytes()).ok())
        else {
            continue;
        };
        for p in repo.providers {
            if provider_on(row, &p) && p.types.iter().any(|t| t == nuvio_kind) {
                out.push((row.url.clone(), p));
            }
        }
    }
    out
}

/// The script of `p`: the one kept while its version holds and it is under
/// a day old, else fetched (2 MiB) and kept; the kept one when a fetch
/// fails.
/// In Incognito a script the host already has is read from the device,
/// and one fetched inside is kept in the scope only.
async fn script(core: &Core, p: &Provider) -> Option<String> {
    let kept = core.known(|l| l.plugin_code(p.script.as_str()).ok().flatten());
    let now = mq_library::hlc::now_ms() as i64;
    if let Some(k) = &kept
        && k.version == p.version
        && now - k.fetched_at < CODE_STALE_MS
    {
        return Some(k.body.clone());
    }
    let opts = mq_http::RequestOptions {
        timeout: ASK_WAIT,
        max_bytes: SCRIPT_BYTES,
        ..Default::default()
    };
    match public(core).get(p.script.as_str(), &opts).await {
        Ok(r) => {
            let body = String::from_utf8_lossy(&r.body).into_owned();
            let code = PluginCode {
                version: p.version.clone(),
                body: body.clone(),
                fetched_at: now,
            };
            let _ = core.library().put_plugin_code(p.script.as_str(), &code);
            Some(body)
        }
        Err(_) => kept.map(|k| k.body),
    }
}

/// The rows provider `p` of repository `repo` gives for `ask`: an answer
/// kept for 10 minutes, else a call in the pool. A call that throws or finds
/// nothing on a TMDB id runs once more with the IMDb id, which some
/// repositories read without a TMDB key of their own (Manitux answers an
/// empty list to a TMDB id).
pub(crate) async fn streams(
    core: &Core,
    repo: &str,
    p: &Provider,
    ask: &Ask,
    tmdb: Option<u64>,
) -> Vec<ParsedStream> {
    let memo = format!(
        "{}|{repo}|{}|{}|{}",
        core.profile(),
        p.id,
        p.version,
        ask.video_id
    );
    {
        let mut kept = lock(&core.plugins().results);
        kept.retain(|_, (at, _)| at.elapsed() < RESULT_KEPT);
        if let Some((_, rows)) = kept.get(&memo) {
            return rows.clone();
        }
    }
    let Some(code) = script(core, p).await else {
        return Vec::new();
    };
    let Some(found) = run(core, &code, p, ask, tmdb).await else {
        return Vec::new();
    };
    let rows = adapt(repo, p, found);
    lock(&core.plugins().results).insert(memo, (Instant::now(), rows.clone()));
    rows
}

#[cfg(feature = "plugins")]
async fn run(
    core: &Core,
    code: &str,
    p: &Provider,
    ask: &Ask,
    tmdb: Option<u64>,
) -> Option<Vec<PluginStream>> {
    use mq_plugins::engine::Outcome;
    let ids: Vec<String> = (tmdb.map(|id| id.to_string()).into_iter())
        .chain(Some(ask.imdb.clone()))
        .collect();
    let mut answer = None;
    for id in ids {
        let outcome = {
            let _permit = core.plugins().pool.acquire().await.ok()?;
            engine::call(core, code, p, ask, id).await
        };
        match outcome {
            Outcome::Done(found) if !found.is_empty() => return Some(found),
            Outcome::Done(found) => answer = Some(found),
            Outcome::Threw(_) => {}
            // An empty answer by the TMDB id still counts when the second call runs out.
            _ => return answer,
        }
    }
    answer
}

#[cfg(not(feature = "plugins"))]
async fn run(
    _: &Core,
    _: &str,
    _: &Provider,
    _: &Ask,
    _: Option<u64>,
) -> Option<Vec<PluginStream>> {
    None
}

#[cfg(feature = "plugins")]
use mq_plugins::engine::PluginStream;

/// A plugin's stream as the engine reads it; a build without the engine
/// never makes one.
#[cfg(not(feature = "plugins"))]
#[allow(dead_code)]
pub(crate) struct PluginStream {
    url: String,
    name: Option<String>,
    title: Option<String>,
    quality: Option<String>,
    size: Option<String>,
    language: Option<String>,
    headers: Vec<(String, String)>,
    subtitles: Vec<PluginSubtitle>,
}

#[cfg(not(feature = "plugins"))]
#[allow(dead_code)]
pub(crate) struct PluginSubtitle {
    url: String,
    lang: Option<String>,
}

/// Stream headers a plugin may set for its own link (6.3).
const STREAM_HEADERS: [&str; 5] = [
    "referer",
    "user-agent",
    "origin",
    "accept",
    "accept-language",
];

/// A link a row may carry: http(s), no userinfo, at most 8 KiB, and no
/// private host literal (a private name is refused when it plays).
fn playable(raw: &str) -> Option<Url> {
    if raw.len() > 8 * 1024 {
        return None;
    }
    let url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    (!mq_http::is_private_host(url.host_str()?)).then_some(url)
}

/// The plugin's answer as rows, field by field (6.3): its link, its words
/// cleaned, its allowed headers; nothing it says about a service, a cache,
/// seeders or a size is believed.
fn adapt(repo: &str, p: &Provider, found: Vec<PluginStream>) -> Vec<ParsedStream> {
    let repo_hash = &mq_streams::key_digest(repo)[1..13];
    let link = Url::parse(&format!("{LINK_SCHEME}:{repo_hash}/{}", p.id)).ok();
    let mut keys = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (index, s) in found.into_iter().enumerate() {
        let Some(url) = playable(&s.url) else {
            continue;
        };
        let key = format!(
            "{KEY_PREFIX}{repo_hash}/{}:{}",
            p.id,
            mq_streams::key_digest(url.as_str())
        );
        if !keys.insert(key.clone()) {
            continue;
        }
        let name = Some(clean(s.name.as_deref().unwrap_or_default(), NAME_MAX))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| p.name.clone());
        let lines: Vec<String> = [&s.title, &s.quality, &s.size, &s.language]
            .into_iter()
            .flatten()
            .map(|t| clean(t, TEXT_MAX))
            .filter(|t| !t.is_empty())
            .collect();
        let wire = serde_json::json!({
            "url": url.as_str(),
            "name": name,
            "description": lines.join("\n"),
        });
        let Ok(wire) = serde_json::from_value::<mq_protocol::WireStream>(wire) else {
            continue;
        };
        let id = format!("plugin.{}", p.id);
        let mut row = mq_streams::normalize(&wire, &id, &p.name, 0, index as u32);
        row.key = key;
        row.addon_url = link.clone();
        // Nothing in plugin text decides how a row plays or ranks (6.3);
        // it answered for this very video, so its words name no title or
        // episode to hide it by.
        (row.service, row.cached, row.seeders, row.source_site) = (None, None, None, None);
        (row.size_bytes, row.size_is_file) = (None, false);
        row.release.title.clear();
        row.release.seasons.clear();
        row.release.episodes.clear();
        row.headers = (s.headers.iter())
            .filter(|(n, v)| {
                STREAM_HEADERS.contains(&n.to_ascii_lowercase().as_str())
                    && v.len() <= 1024
                    && !v.contains(['\r', '\n', '\0'])
            })
            .take(8)
            .cloned()
            .collect();
        row.subtitles = (s.subtitles.iter())
            .filter_map(|sub| {
                let url = playable(&sub.url)?;
                serde_json::from_value(serde_json::json!({
                    "id": format!("{}:{}", p.id, mq_streams::key_digest(url.as_str())),
                    "url": url.as_str(),
                    "lang": clean(sub.lang.as_deref().unwrap_or_default(), 16),
                }))
                .ok()
            })
            .take(20)
            .collect();
        out.push(row);
    }
    out
}

/// The provider's name and the host a plugin row plays from (4.4).
pub(crate) fn row_source(s: &ParsedStream) -> Option<(String, String)> {
    if !s.key.starts_with(KEY_PREFIX) {
        return None;
    }
    let host = match &s.source {
        mq_protocol::Source::Http { url } => url.host_str().unwrap_or_default().to_owned(),
        _ => String::new(),
    };
    Some((s.addon_name.clone(), host))
}

#[cfg(feature = "plugins")]
mod engine {
    //! One call on the engine, with this core as its host (6.2, 6.4).

    use super::{Ask, Provider, public};
    use crate::Core;
    use mq_http::{Body, HopRule, HttpError, Method as HttpMethod, RequestOptions, Url};
    use mq_plugins::engine::{
        Call, FetchRequest, FetchResponse, HostCall, HostReply, HostRequest, Info, Limits, Media,
        Method, Outcome,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// Ports a browser never fetches (the Fetch standard's bad ports).
    const BAD_PORTS: [u16; 82] = [
        0, 1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95,
        101, 102, 103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143, 161, 179,
        389, 427, 465, 512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601,
        636, 989, 990, 993, 995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566,
        6665, 6666, 6667, 6668, 6669, 6679, 6697,
    ];
    /// Request headers a plugin never sets: the client's own, and those
    /// that speak for a browser (6.4).
    const OWN_HEADERS: [&str; 10] = [
        "host",
        "content-length",
        "connection",
        "transfer-encoding",
        "upgrade",
        "te",
        "trailer",
        "origin",
        "keep-alive",
        "expect",
    ];
    const REQUEST_WAIT: Duration = Duration::from_secs(10);
    const MAX_BODY: usize = 4 << 20;

    /// Where a plugin's request may go: http(s), no userinfo, no bad port;
    /// the handle itself keeps it to public addresses.
    fn allowed(url: &Url) -> bool {
        matches!(url.scheme(), "http" | "https")
            && url.username().is_empty()
            && url.password().is_none()
            && url.host().is_some()
            && (url.port_or_known_default()).is_some_and(|p| !BAD_PORTS.contains(&p))
    }

    fn sendable(name: &str, value: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        !OWN_HEADERS.contains(&lower.as_str())
            && !lower.starts_with("proxy-")
            && !lower.starts_with("sec-")
            && name.len() <= 256
            && value.len() <= 8 * 1024
            && !name.contains(['\r', '\n', '\0'])
            && !value.contains(['\r', '\n', '\0'])
    }

    async fn fetch(http: &mq_http::HttpClient, req: FetchRequest) -> HostReply {
        let Ok(url) = Url::parse(&req.url) else {
            return HostReply::Failed("Invalid URL".into());
        };
        let host = url.host_str().unwrap_or_default().to_owned();
        if !allowed(&url) {
            return HostReply::Failed(format!("could not reach {host}"));
        }
        let mut content_type = None;
        let mut headers = Vec::new();
        for (n, v) in req.headers {
            if !sendable(&n, &v) {
                continue;
            }
            if n.eq_ignore_ascii_case("content-type") {
                content_type = Some(v);
            } else if headers.len() < 32 {
                headers.push((n, v));
            }
        }
        let opts = RequestOptions {
            timeout: REQUEST_WAIT,
            max_bytes: req.max_bytes.min(MAX_BODY),
            headers,
            ..Default::default()
        };
        let method = match req.method {
            Method::Get => HttpMethod::GET,
            Method::Post => HttpMethod::POST,
            Method::Head => HttpMethod::HEAD,
        };
        let body = req.body.map(|bytes| Body::Raw {
            content_type: content_type.unwrap_or_else(|| "text/plain;charset=UTF-8".into()),
            bytes,
        });
        let client = http.with_hops(HopRule::new(allowed, req.follow));
        let said = |status: u16| {
            if (200..300).contains(&status) {
                "OK"
            } else {
                ""
            }
            .to_owned()
        };
        match client.request(method, url.as_str(), body, &opts).await {
            Ok(r) => HostReply::Response(FetchResponse {
                status: r.status,
                status_text: said(r.status),
                url: r.final_url.to_string(),
                headers: (r.headers.iter())
                    .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                    .take(64)
                    .collect(),
                body: r.body.to_vec(),
            }),
            // A refusal is an answer: `ok` is false and its body is read.
            Err(HttpError::Status { status, body, .. }) => HostReply::Response(FetchResponse {
                status,
                status_text: said(status),
                url: url.to_string(),
                headers: Vec::new(),
                body: body.to_vec(),
            }),
            Err(_) => HostReply::Failed(format!("could not reach {host}")),
        }
    }

    /// Stops the engine's call when the view that asked goes.
    struct Stop(Arc<AtomicBool>);

    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    pub(super) async fn call(
        core: &Core,
        code: &str,
        p: &Provider,
        ask: &Ask,
        id: String,
    ) -> Outcome {
        let call = Call {
            script: code.to_owned(),
            plugin_id: p.id.clone(),
            tmdb_id: id,
            media: if ask.kind == "series" {
                Media::Tv
            } else {
                Media::Movie
            },
            season: ask.season,
            episode: ask.episode,
            info: Info {
                imdb_id: Some(ask.imdb.clone()),
                title: ask.title.clone(),
                year: ask.year.map(u32::from),
            },
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HostRequest>();
        let cancel = Arc::new(AtomicBool::new(false));
        let _stop = Stop(cancel.clone());
        let Ok(thread) = mq_plugins::engine::call(call, Limits::default(), cancel, tx) else {
            return Outcome::Memory;
        };
        let http = public(core);
        let mut asks = tokio::task::JoinSet::new();
        // The channel closes when the engine's thread ends.
        while let Some(HostRequest { call, reply }) = rx.recv().await {
            let HostCall::Fetch(req) = call else { continue };
            let http = http.clone();
            asks.spawn(async move {
                let mut reply = reply;
                let answer = tokio::select! {
                    answer = fetch(&http, req) => answer,
                    () = reply.closed() => return,
                };
                let _ = reply.send(answer);
            });
        }
        drop(asks);
        tokio::task::spawn_blocking(move || thread.join())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(Outcome::Memory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_kept_as_nuvio_keeps_them() {
        let n = |s: &str| normalize_url(s).map(|u| u.to_string()).map_err(drop);
        assert_eq!(
            n("example.com/repo/").unwrap(),
            "https://example.com/repo/manifest.json"
        );
        assert_eq!(
            n("https://example.com/r/manifest.json?x=1#top").unwrap(),
            "https://example.com/r/manifest.json?x=1"
        );
        assert!(n("http://example.com/manifest.json").is_err());
        assert!(n("https://user:pw@example.com/").is_err());
    }

    #[test]
    fn a_manifest_lists_plain_providers_once() {
        let base = Url::parse("https://example.com/repo/manifest.json").unwrap();
        let body = br#"{"name": "Repo\u202e", "scrapers": [
            {"id": "one", "name": "One", "filename": "p/one.js", "supportedTypes": ["movie"]},
            {"id": "one", "name": "Again", "filename": "p/again.js"},
            {"id": "bad id", "filename": "p/x.js"},
            {"id": "off", "filename": "https://cdn.example.org/off.js", "enabled": false},
            {"id": "nofile"}
        ]}"#;
        let repo = parse_manifest(&base, body).unwrap();
        assert_eq!(repo.name, "Repo");
        let ids: Vec<&str> = repo.providers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["one", "off"]);
        assert_eq!(
            repo.providers[0].script.as_str(),
            "https://example.com/repo/p/one.js"
        );
        assert_eq!(repo.providers[1].types, ["movie", "tv"]);
        assert!(repo.providers[0].listed_on && !repo.providers[1].listed_on);
        assert!(parse_manifest(&base, br#"{"scrapers": []}"#).is_err());
        assert!(parse_manifest(&base, b"<html>").is_err());
    }

    #[test]
    fn names_lose_emoji_and_sort_marks() {
        // Real names from the All-in-One-Nuvio, MoOnCrOwN and Manitux manifests (2026-10-30).
        let base = Url::parse("https://example.com/repo/manifest.json").unwrap();
        let body = r#"{"name": "MoOnCrOwN-NUVİO ✨", "description": "4K🔥HDR • 1080p Movies",
            "scrapers": [
            {"id": "a", "name": "🧲 Torrentio", "filename": "a.js"},
            {"id": "b", "name": "❤️ VidLove", "filename": "b.js"},
            {"id": "c", "name": "💋 Kisskh", "filename": "c.js"},
            {"id": "d", "name": "🗡️ AnimeWorld", "filename": "d.js"},
            {"id": "e", "name": "🫰 OnlyKDrama", "filename": "e.js"},
            {"id": "f", "name": "Vid 🪨 Rock", "filename": "f.js"},
            {"id": "h", "name": "!DiziBal", "filename": "h.js"},
            {"id": "i", "name": "[TR] Dizi", "filename": "i.js"},
            {"id": "g", "name": "🌸", "filename": "g.js"}
        ]}"#;
        let repo = parse_manifest(&base, body.as_bytes()).unwrap();
        assert_eq!(repo.name, "MoOnCrOwN-NUVİO");
        assert_eq!(repo.description, "4K HDR • 1080p Movies");
        let names: Vec<&str> = repo.providers.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Torrentio",
                "VidLove",
                "Kisskh",
                "AnimeWorld",
                "OnlyKDrama",
                "Vid Rock",
                "DiziBal",
                "[TR] Dizi",
                "g"
            ]
        );
    }

    #[test]
    fn a_provider_the_repository_lists_off_turns_on_by_hand() {
        let p = |listed_on| Provider {
            id: "one".into(),
            name: "One".into(),
            version: String::new(),
            description: String::new(),
            script: Url::parse("https://example.com/one.js").unwrap(),
            types: Vec::new(),
            languages: Vec::new(),
            listed_on,
        };
        let row = |off: &[&str]| PluginRepoRow {
            url: String::new(),
            enabled: true,
            manifest_json: None,
            fetched_at: 0,
            off: off.iter().map(|s| (*s).to_owned()).collect(),
        };
        assert!(provider_on(&row(&[]), &p(true)));
        assert!(!provider_on(&row(&["one"]), &p(true)));
        assert!(!provider_on(&row(&[]), &p(false)));
        assert!(provider_on(&row(&["+one"]), &p(false)));
    }

    #[cfg(feature = "plugins")]
    #[test]
    fn a_row_believes_nothing_but_its_link() {
        let p = Provider {
            id: "one".into(),
            name: "One".into(),
            version: "1".into(),
            description: String::new(),
            script: Url::parse("https://example.com/one.js").unwrap(),
            types: vec!["movie".into()],
            languages: Vec::new(),
            listed_on: true,
        };
        let s = |url: &str, name: &str| PluginStream {
            url: url.into(),
            name: Some(name.into()),
            title: Some("Other Film 2001 S01E05 [RD+]".into()),
            quality: Some("1080p".into()),
            size: Some("12.4 GB".into()),
            language: None,
            headers: vec![
                ("Referer".into(), "https://example.com/".into()),
                ("Cookie".into(), "a=b".into()),
            ],
            subtitles: Vec::new(),
        };
        let rows = adapt(
            "https://example.com/manifest.json",
            &p,
            vec![
                s("https://cdn.example.net/v.mp4", "One [RD+] 1080p"),
                s("https://cdn.example.net/v.mp4", "Same link"),
                s("http://192.168.1.1/v.mp4", "LAN"),
                s("magnet:?xt=urn:btih:abc", "Torrent"),
                s("https://u:p@cdn.example.net/v.mp4", "Userinfo"),
            ],
        );
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert!(r.key.starts_with(KEY_PREFIX));
        assert_eq!(r.addon_url.as_ref().unwrap().scheme(), LINK_SCHEME);
        assert_eq!(
            (r.service.as_deref(), r.cached, r.size_bytes),
            (None, None, None)
        );
        assert!(r.release.title.is_empty() && r.release.episodes.is_empty());
        assert_eq!(r.release.resolution.height(), 1080);
        assert_eq!(
            r.headers,
            [("Referer".to_owned(), "https://example.com/".to_owned())]
        );
    }
}
