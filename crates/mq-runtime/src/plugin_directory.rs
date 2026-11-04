//! Nuvio repositories to browse in Find addons (R8, lead 2026-10-30): read from a third-party
//! directory, nuvioplugins.com, as Find addons reads stremio-addons.net's list. The page is read
//! for the repository manifest links it lists (https on raw.githubusercontent.com only), each
//! manifest is read with R7's parser, and both are kept a day in memory. When the page cannot be
//! read or lists none, a snapshot of its links taken 2026-10-30 stands in. Listed in the page's
//! order, nothing featured; installing goes through R7's sheet (`previewPluginRepo`,
//! `addPluginRepo`). Kids profiles and builds without plugins see none.

use crate::Core;
use crate::plugins::{Repo, fetch_manifest, host_of};
use mq_http::Url;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The directory, credited on its cards.
pub const SOURCE: &str = "nuvioplugins.com";
const PAGE: &str = "https://nuvioplugins.com/";
/// The only host a listed manifest may be on.
const MANIFEST_HOST: &str = "raw.githubusercontent.com";
const KEPT: Duration = Duration::from_secs(86_400);
const PAGE_BYTES: usize = 2 << 20;
const MAX_LISTED: usize = 50;

/// The page's links as of 2026-10-30, in its order, less three that answered 404 on 2026-11-02:
/// what the section lists when the page cannot be read or lists none.
const SNAPSHOT: [&str; 8] = [
    "https://raw.githubusercontent.com/yoruix/nuvio-providers/refs/heads/main/manifest.json",
    "https://raw.githubusercontent.com/D3adlyRocket/All-in-One-Nuvio/refs/heads/main/manifest.json",
    "https://raw.githubusercontent.com/michat88/nuvio-providers/refs/heads/main/manifest.json",
    "https://raw.githubusercontent.com/Abinanthankv/NuvioRepo/refs/heads/master/manifest.json",
    "https://raw.githubusercontent.com/saimuelbr/saimuel-nuvio-repo/refs/heads/main/manifest.json",
    "https://raw.githubusercontent.com/Gowaru/gowaru-nuvio-providers/refs/heads/main/manifest.json",
    "https://raw.githubusercontent.com/mooncrown04/nuviotr/refs/heads/main/manifest.json",
    "https://raw.githubusercontent.com/KennethJYS/Nuvio-Providers-Latino/refs/heads/main/manifest.json",
];

/// What was read, kept a day: the page's links, and each manifest by its link.
#[derive(Default)]
pub(crate) struct Kept {
    page: Mutex<Option<(Instant, Vec<Url>)>>,
    manifests: Mutex<HashMap<String, (Instant, Repo)>>,
    /// Tests: the directory page somewhere else.
    origin: Mutex<Option<Url>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The `pluginDirectory` answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct PluginDirectory {
    /// "nuvioplugins.com", for the cards' credit.
    pub source: String,
    /// The page could not be read or listed none: these come from the shipped snapshot.
    pub from_snapshot: bool,
    pub repos: Vec<ListedRepo>,
}

/// One repository as the directory lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct ListedRepo {
    /// Its manifest link: what `previewPluginRepo` and `addPluginRepo` take.
    pub url: String,
    pub host: String,
    pub name: String,
    pub description: String,
    /// Its providers' names, in its order.
    pub providers: Vec<String>,
    /// Every language its providers name, first seen first.
    pub languages: Vec<String>,
    /// "movie", "series".
    pub types: Vec<String>,
    /// In this profile's list already.
    pub installed: bool,
}

/// The manifest links on `page`, in its order, once each: https on raw.githubusercontent.com,
/// a path ending in `/manifest.json`, no userinfo; at most `MAX_LISTED`.
pub fn links(page: &str) -> Vec<Url> {
    let mut out: Vec<Url> = Vec::new();
    let start = format!("https://{MANIFEST_HOST}");
    let mut rest = page;
    while let Some(at) = rest.find(&start) {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`' | ')')
            })
            .unwrap_or(tail.len());
        rest = &tail[end..];
        let Ok(url) = Url::parse(&tail[..end]) else {
            continue;
        };
        let fits = url.scheme() == "https"
            && url.host_str() == Some(MANIFEST_HOST)
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().ends_with("/manifest.json");
        if fits && !out.contains(&url) {
            out.push(url);
            if out.len() == MAX_LISTED {
                break;
            }
        }
    }
    out
}

fn snapshot() -> Vec<Url> {
    SNAPSHOT.iter().filter_map(|u| Url::parse(u).ok()).collect()
}

impl Core {
    /// Tests: the directory page at a fixture; only before its first read. Development builds only.
    #[doc(hidden)]
    #[cfg(debug_assertions)]
    pub fn use_plugin_directory_at(&self, page: Url) {
        *lock(&self.plugins().listed.origin) = Some(page);
    }

    /// The directory's links: read from the page once a day; the snapshot when the page cannot
    /// be read or lists none (that answer is not kept, so the next open asks again).
    async fn directory_links(&self) -> (Vec<Url>, bool) {
        let kept = &self.plugins().listed;
        if let Some((at, links)) = lock(&kept.page).as_ref()
            && at.elapsed() < KEPT
        {
            return (links.clone(), false);
        }
        let page = lock(&kept.origin)
            .clone()
            .unwrap_or_else(|| Url::parse(PAGE).expect("a valid page"));
        let opts = mq_http::RequestOptions {
            timeout: Duration::from_secs(10),
            max_bytes: PAGE_BYTES,
            ..Default::default()
        };
        let read = self.http().scoped(&[]).get(page.as_str(), &opts).await;
        let found = read
            .map(|r| links(&String::from_utf8_lossy(&r.body)))
            .unwrap_or_default();
        if found.is_empty() {
            return (snapshot(), true);
        }
        *lock(&kept.page) = Some((Instant::now(), found.clone()));
        (found, false)
    }

    /// A listed manifest, kept a day; none when it cannot be read.
    async fn listed_manifest(&self, url: &Url) -> Option<Repo> {
        let kept = &self.plugins().listed;
        if let Some((at, repo)) = lock(&kept.manifests).get(url.as_str())
            && at.elapsed() < KEPT
        {
            return Some(repo.clone());
        }
        let (repo, _) = fetch_manifest(self, url).await.ok()?;
        lock(&kept.manifests).insert(url.to_string(), (Instant::now(), repo.clone()));
        Some(repo)
    }

    /// The directory's repositories that could be read, in its order, each with whether this
    /// profile has it. None for a kids profile or a build without plugins.
    pub async fn plugin_directory(&self) -> PluginDirectory {
        let mut answer = PluginDirectory {
            source: SOURCE.into(),
            from_snapshot: false,
            repos: Vec::new(),
        };
        if !self.has_plugins() || self.policy().kids() {
            return answer;
        }
        let (links, from_snapshot) = self.directory_links().await;
        answer.from_snapshot = from_snapshot;
        let read =
            futures_util::future::join_all(links.iter().map(|u| self.listed_manifest(u))).await;
        let have: Vec<String> = (self.library().plugin_repos(self.profile()))
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.url)
            .collect();
        for (url, repo) in links.iter().zip(read) {
            let Some(repo) = repo else { continue };
            let mut languages: Vec<String> = Vec::new();
            let mut types: Vec<String> = Vec::new();
            for p in &repo.providers {
                for l in &p.languages {
                    if !languages.contains(l) {
                        languages.push(l.clone());
                    }
                }
                for t in &p.types {
                    let t = if t == "tv" {
                        "series".to_owned()
                    } else {
                        t.clone()
                    };
                    if !types.contains(&t) {
                        types.push(t);
                    }
                }
            }
            answer.repos.push(ListedRepo {
                url: url.to_string(),
                host: host_of(url.as_str()),
                name: repo.name.clone(),
                description: repo.description.clone(),
                providers: repo.providers.iter().map(|p| p.name.clone()).collect(),
                languages,
                types,
                installed: have.iter().any(|h| h == url.as_str()),
            });
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_gives_raw_github_manifest_links_once_in_its_order() {
        let page = r#"<a href="https://raw.githubusercontent.com/b/r/main/manifest.json">B</a>
            <code>https://raw.githubusercontent.com/a/r/main/manifest.json</code>
            "https://raw.githubusercontent.com/b/r/main/manifest.json"
            https://raw.githubusercontent.com/c/r/main/other.json
            http://raw.githubusercontent.com/d/r/main/manifest.json
            https://raw.githubusercontent.com.evil.example/e/manifest.json
            https://gist.github.com/f/manifest.json"#;
        let found: Vec<String> = links(page).iter().map(Url::to_string).collect();
        assert_eq!(
            found,
            [
                "https://raw.githubusercontent.com/b/r/main/manifest.json",
                "https://raw.githubusercontent.com/a/r/main/manifest.json"
            ]
        );
        assert!(links("nothing here").is_empty());
        assert_eq!(snapshot().len(), SNAPSHOT.len());
    }
}
