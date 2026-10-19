//! The addons every new profile starts with (plans/m13-default-addons.md).
//! Their manifests are bundled (`defaults.json`, read from the live addons),
//! so installing them needs no network and first run never waits on an
//! addon; the daily refresh replaces them with the live ones.

use mq_protocol::Manifest;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Raised when the list gains entries, so an existing profile is offered
/// the new ones once (`defaultsSeen`).
pub const LIST_VERSION: u32 = 1;

/// One default addon, in list order (Settings, asking order, Home rows).
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultAddon {
    /// A stable name for choices (`installDefaults`).
    pub key: String,
    /// The link it is installed with.
    pub url: String,
    pub name: String,
    /// What it does, in one line (onboarding and Settings).
    pub line: String,
    /// "catalogs", "streams", "subtitles" or "lists": how onboarding groups it.
    pub group: String,
    pub manifest: Manifest,
}

/// Whether a default is in the profile's list, by manifest id or its own
/// link: a copy installed another way (another config, a Stremio import)
/// counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub enum DefaultInstalled {
    Absent,
    On,
    Off,
}

/// A default as the apps show it (`defaultAddons`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct DefaultAddonInfo {
    pub key: String,
    pub name: String,
    pub line: String,
    pub group: String,
    pub installed: DefaultInstalled,
}

/// The `defaultAddons` answer: the list, and the version an existing
/// profile is offered (`defaultsSeen` below it shows the card once).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct DefaultAddons {
    pub version: u32,
    pub addons: Vec<DefaultAddonInfo>,
}

/// One switch of the defaults step (`installDefaults`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct DefaultChoice {
    pub key: String,
    pub on: bool,
}

/// The nine defaults, in order.
pub fn list() -> &'static [DefaultAddon] {
    static LIST: OnceLock<Vec<DefaultAddon>> = OnceLock::new();
    LIST.get_or_init(|| {
        serde_json::from_str(include_str!("defaults.json"))
            .expect("defaults.json is checked by its tests")
    })
}

/// The default whose manifest has this id.
pub fn by_id(id: &str) -> Option<&'static DefaultAddon> {
    list().iter().find(|d| d.manifest.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::put_addon;
    use mq_library::AddonRow;
    use std::collections::HashSet;

    fn row(url: &str, manifest: &Manifest, enabled: bool) -> AddonRow {
        AddonRow {
            transport_url: url.into(),
            manifest_json: serde_json::to_string(manifest).unwrap(),
            position: 0,
            enabled,
            allow_private: false,
        }
    }

    #[test]
    fn a_configured_copy_takes_the_untouched_defaults_row_and_never_a_changed_one() {
        let torrentio = list().iter().find(|d| d.key == "torrentio").unwrap();
        let cinemeta = list().iter().find(|d| d.key == "cinemeta").unwrap();
        let mine = "https://torrentio.strem.fun/realdebrid=xyz/manifest.json";
        let json = serde_json::to_string(&torrentio.manifest).unwrap();
        // Untouched: the configured link takes its row, place and switch kept.
        let mut rows = vec![
            row(&cinemeta.url, &cinemeta.manifest, true),
            row(&torrentio.url, &torrentio.manifest, false),
        ];
        put_addon(&mut rows, mine, json.clone());
        let links: Vec<(&str, bool)> = rows
            .iter()
            .map(|r| (r.transport_url.as_str(), r.enabled))
            .collect();
        assert_eq!(links, [(cinemeta.url.as_str(), true), (mine, false)]);
        // A row the user already moved to another link stays; the new one is added.
        let theirs = "https://torrentio.strem.fun/sort=size/manifest.json";
        let mut rows = vec![row(theirs, &torrentio.manifest, true)];
        put_addon(&mut rows, mine, json);
        let links: Vec<&str> = rows.iter().map(|r| r.transport_url.as_str()).collect();
        assert_eq!(links, [theirs, mine]);
        // Another addon is appended as before.
        let mut rows = vec![row(&torrentio.url, &torrentio.manifest, true)];
        put_addon(
            &mut rows,
            &cinemeta.url,
            serde_json::to_string(&cinemeta.manifest).unwrap(),
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn the_bundled_defaults_are_nine_installable_addons() {
        let list = list();
        let keys: Vec<&str> = list.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "cinemeta",
                "torrentio",
                "turkcealtyazi",
                "subsense",
                "opensubtitles",
                "trakt",
                "rottentomatoes",
                "torrentcatalogs",
                "kitsu"
            ]
        );
        let ids: HashSet<&str> = list.iter().map(|d| d.manifest.id.as_str()).collect();
        assert_eq!(ids.len(), list.len(), "manifest ids are unique");
        for d in list {
            // Canonical: what an install of the same link would store.
            let url = mq_addons::normalize_transport_url(&d.url).unwrap();
            assert_eq!(url.as_str(), d.url, "{}", d.key);
            assert_eq!(url.scheme(), "https", "{}", d.key);
            assert!(d.url.ends_with("/manifest.json"), "{}", d.key);
            let hints = &d.manifest.behavior_hints;
            assert!(!hints.configuration_required && !hints.adult, "{}", d.key);
            assert!(
                ["catalogs", "streams", "subtitles", "lists"].contains(&d.group.as_str()),
                "{}",
                d.key
            );
            assert!(!d.name.is_empty() && d.line.ends_with('.'), "{}", d.key);
        }
        // The links of section 1, character for character where it matters.
        let link = |key: &str| list.iter().find(|d| d.key == key).unwrap().url.as_str();
        assert_eq!(
            link("torrentio"),
            "https://torrentio.strem.fun/providers=yts,eztv,rarbg,1337x,ext,thepiratebay,kickasstorrents,torrentgalaxy,magnetdl,horriblesubs,nyaasi,tokyotosho,anidex,nekobt|qualityfilter=threed,scr,cam|sizefilter=15GB,5GB/manifest.json"
        );
        assert_eq!(
            link("subsense"),
            "https://subsense.nepiraw.com/%7B%22languages%22%3A%5B%22tur%22%2C%22eng%22%5D%2C%22maxSubtitles%22%3A10%7D/manifest.json"
        );
    }
}
