//! The addons every new profile starts with (plans/m13-default-addons.md).
//! Their manifests are bundled (`defaults.json`, read from the live addons),
//! so installing them needs no network and first run never waits on an
//! addon; the daily refresh replaces them with the live ones.

use crate::Core;
use crate::core::{CoreError, default_rows, put_addon};
use mq_addons::InstalledAddon;
use mq_library::DEFAULTS_GIVEN;
use mq_protocol::{Manifest, MetaItemPreview, ResourcePath, ResourceResponse, decode_catalog};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;
use stremio_core::types::addon::ManifestCatalog;

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

/// The defaults' logos, bundled so the first-run step asks no logo host
/// anything (assets/defaults/README.md). Cinemeta publishes none.
pub(crate) static LOGOS: &[mq_server::Logo] = &[
    logo(
        "torrentio",
        "image/png",
        include_bytes!("../assets/defaults/torrentio.png"),
    ),
    logo(
        "turkcealtyazi",
        "image/png",
        include_bytes!("../assets/defaults/turkcealtyazi.png"),
    ),
    logo(
        "subsense",
        "image/png",
        include_bytes!("../assets/defaults/subsense.png"),
    ),
    logo(
        "opensubtitles",
        "image/png",
        include_bytes!("../assets/defaults/opensubtitles.png"),
    ),
    logo(
        "trakt",
        "image/png",
        include_bytes!("../assets/defaults/trakt.png"),
    ),
    logo(
        "rottentomatoes",
        "image/png",
        include_bytes!("../assets/defaults/rottentomatoes.png"),
    ),
    logo(
        "torrentcatalogs",
        "image/png",
        include_bytes!("../assets/defaults/torrentcatalogs.png"),
    ),
    logo(
        "kitsu",
        "image/png",
        include_bytes!("../assets/defaults/kitsu.png"),
    ),
    logo(
        "streamingcatalogs",
        "image/png",
        include_bytes!("../assets/defaults/streamingcatalogs.png"),
    ),
    logo(
        "stremioaddons",
        "image/png",
        include_bytes!("../assets/defaults/stremioaddons.png"),
    ),
];

const fn logo(key: &'static str, mime: &'static str, bytes: &'static [u8]) -> mq_server::Logo {
    mq_server::Logo { key, mime, bytes }
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
    /// The bundled logo, served by the core on this device's loopback (no
    /// logo host is asked); none when the addon publishes no logo.
    pub logo: Option<String>,
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

/// The defaults, in order.
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

/// The defaults the owner's rulings of 2026-10-22 bring to existing
/// profiles (Streaming Catalogs the later one), and the stremio-addons.net
/// directory of 2026-10-28 (an `addon_catalog` addon only: its list shows in
/// Find addons, plans/plugins-and-marketplace.md 7.1). A profile with a list
/// from before the profiles kept what they had counts every other default
/// as had: it gets these once, and an older one it removed stays removed.
const BROUGHT: [&str; 5] = [
    "trakt",
    "rottentomatoes",
    "torrentcatalogs",
    "streamingcatalogs",
    "stremioaddons",
];

/// Settings keys under this prefix, one per key in `MOVED`, record that the
/// profile's old link was moved (or that it had none to move).
const DEFAULTS_MOVED: &str = "defaultsMoved.";

/// Links a default was installed with before it moved, by key. Trakt Tv's
/// old deployment answers `{}` whenever its own call to Trakt fails, and its
/// author points to the maintained one; the Rotten Tomatoes addon is the
/// owner's own, configured with RPDB's free key, so its posters carry the
/// ratings band (the owner, 2026-10-22). Streaming Catalogs moved to its
/// configured link with ten services for Discover's service filter
/// (plans/discover-v2.md section 5).
const MOVED: [(&str, &str); 3] = [
    (
        "trakt",
        "https://2ecbbd610840-trakt.baby-beamup.club/manifest.json",
    ),
    (
        "rottentomatoes",
        "https://7a82163c306e-rottentomatoes.baby-beamup.club/manifest.json",
    ),
    (
        "streamingcatalogs",
        "https://7a82163c306e-stremio-netflix-catalog-addon.baby-beamup.club/manifest.json",
    ),
];

impl Core {
    /// Puts every default a profile never had in its list once, on, for
    /// each profile on this device (the owner's ruling, 2026-10-22: a new
    /// default reaches existing profiles with the next build). What a
    /// profile has had is kept by key (`DEFAULTS_GIVEN`, synced): the keys
    /// the defaults step named, those given here, those found in its list,
    /// on or off. One it had and removed stays removed. A kids profile (it
    /// reads the owner's list) and one with no list yet (the defaults step
    /// asks it) are left alone. On a device in a sync account this runs
    /// after the run's first pull (`Core::sync`), so a stale list never
    /// overwrites newer edits made elsewhere. Never fails: a profile it
    /// cannot read is skipped.
    pub(crate) fn give_new_defaults(&self) {
        let Ok(profiles) = self.library().profiles() else {
            return;
        };
        for p in profiles {
            if let Err(e) = self.for_profile(&p.id).give_defaults_once() {
                tracing::warn!("new default addons not given: {e}");
            }
        }
    }

    /// `give_new_defaults` for this profile; the keys it put in the list.
    pub(crate) fn give_defaults_once(&self) -> Result<Vec<String>, CoreError> {
        if self.policy().kids() || self.library().addon_list_hlc(self.profile())?.is_none() {
            return Ok(Vec::new());
        }
        // The plugin repositories this profile never had arrive on (R7),
        // kept per device as the repositories are.
        self.give_plugin_defaults()?;
        self.move_old_links()?;
        let mut given = self.defaults_given();
        if given.is_empty() {
            // A list from before profiles kept what they had.
            given = list()
                .iter()
                .map(|d| d.key.clone())
                .filter(|k| !BROUGHT.contains(&k.as_str()))
                .collect();
        }
        let new: Vec<&DefaultAddon> = list().iter().filter(|d| !given.contains(&d.key)).collect();
        if new.is_empty() {
            return Ok(Vec::new());
        }
        let mut added = Vec::new();
        self.edit_addons(|rows| {
            for d in &new {
                if default_rows(rows, d).is_empty() {
                    let json = serde_json::to_string(&d.manifest)
                        .map_err(|e| CoreError::BadManifest(e.to_string()))?;
                    put_addon(rows, &d.url, json);
                    added.push(d.key.clone());
                }
            }
            Ok(())
        })?;
        self.mark_given(list().iter().map(|d| d.key.as_str()))?;
        if !added.is_empty() {
            // Given, not offered: the card for new defaults has nothing to show.
            self.seen_defaults()?;
            self.refresh_addons_soon()?;
        }
        Ok(added)
    }

    /// Moves each default on an old link (`MOVED`) once per profile: a row
    /// on the old link takes the default's link and bundled manifest in
    /// place, its switch and position kept, and the Home rows the user hid,
    /// showed or moved follow it. The Home rows go first: moving them again
    /// changes nothing, so a stop before the list is written strands none.
    /// When the new link is in the list already, the old row goes and the
    /// new row's own Home choices stay. A setting per key
    /// (`DEFAULTS_MOVED`, synced) records the move, so an old link the user
    /// adds again later is left as they chose.
    fn move_old_links(&self) -> Result<(), CoreError> {
        let settings = self.library().settings(self.profile())?;
        for (key, old) in MOVED {
            let done = format!("{DEFAULTS_MOVED}{key}");
            let Some(d) = list().iter().find(|d| d.key == key) else {
                continue;
            };
            if settings.iter().any(|(k, _)| *k == done) {
                continue;
            }
            let rows = self.library().addons(self.profile())?;
            if rows.iter().any(|r| r.transport_url == old) {
                if !rows.iter().any(|r| r.transport_url == d.url) {
                    self.move_home_rows(old, &d.url)?;
                }
                let json = serde_json::to_string(&d.manifest)
                    .map_err(|e| CoreError::BadManifest(e.to_string()))?;
                // Upkeep, not an edit: stamped just above the old link's
                // own register, so a removal of the new link a person made
                // on another device still wins (plans/accounts-social-sync.md 13).
                self.library().rename_addon(
                    self.profile(),
                    old,
                    &d.url,
                    &json,
                    self.clock().device(),
                )?;
            }
            self.library()
                .set_setting(self.profile(), &done, "true", self.clock())?;
        }
        Ok(())
    }

    /// Tests: the keys of the defaults this profile has had.
    #[doc(hidden)]
    pub fn defaults_had(&self) -> Vec<String> {
        self.defaults_given()
    }

    /// The keys of the defaults this profile has had.
    fn defaults_given(&self) -> Vec<String> {
        let settings = self.library().settings(self.profile()).unwrap_or_default();
        settings
            .into_iter()
            .filter_map(|(key, _)| key.strip_prefix(DEFAULTS_GIVEN).map(str::to_owned))
            .collect()
    }

    /// Records `keys` among the defaults this profile has had, one setting
    /// each; only the new ones are written.
    pub(crate) fn mark_given<'k>(
        &self,
        keys: impl IntoIterator<Item = &'k str>,
    ) -> Result<(), CoreError> {
        let given = self.defaults_given();
        for key in keys {
            if !given.iter().any(|g| g == key) {
                let setting = format!("{DEFAULTS_GIVEN}{key}");
                self.library()
                    .set_setting(self.profile(), &setting, "true", self.clock())?;
            }
        }
        Ok(())
    }

    /// The still the first-run question steps frame (`firstRunStill`), from
    /// the default catalog addon's first catalog, asked from its bundled
    /// manifest without installing it (no account, nothing written).
    ///
    /// With TMDB (the owner's builds): the first of its top `STILL_TRIES`
    /// titles TMDB holds a textless backdrop for, a scene frame with nothing
    /// printed on it, never the key art a catalog's own backdrop often is
    /// (the owner, 2026-10-30, plans/batch-2.md R2); TMDB's answers are kept
    /// a day. Without TMDB (no token, a throwaway core): the first title with
    /// a backdrop. None when the catalog gave no answer within
    /// `WALL_BUDGET`, TMDB none within another, or nothing fits: the frame
    /// stays black.
    pub async fn first_run_still(&self) -> Option<FirstRunStill> {
        let catalog = first_run_source()?.1;
        let metas = self.first_run_catalog().await?;
        if self.tmdb_ready() {
            let textless = async {
                for m in metas.iter().take(STILL_TRIES) {
                    if let Some(background) =
                        self.tmdb_textless_backdrop(&catalog.r#type, &m.id).await
                    {
                        return Some(FirstRunStill {
                            background,
                            logo: m.logo.as_ref().map(|u| u.to_string()),
                            name: m.name.clone(),
                        });
                    }
                }
                None
            };
            return tokio::time::timeout(WALL_BUDGET, textless)
                .await
                .ok()
                .flatten();
        }
        metas.into_iter().find_map(|m| {
            Some(FirstRunStill {
                background: m.background?.to_string(),
                logo: m.logo.map(|u| u.to_string()),
                name: m.name,
            })
        })
    }

    /// The posters first run's wall shows (`firstRunWall`;
    /// plans/first-screens.md 4.1): up to `WALL_POSTERS` links from the
    /// default catalog addon's first catalog, in its order, without repeats,
    /// `http` taken as `https` and any other scheme left out. Asked from its
    /// bundled manifest without installing it (no account, nothing
    /// written). Links only, never a name or an addon. Empty when no answer
    /// came within `WALL_BUDGET`.
    pub async fn first_run_wall(&self) -> Vec<String> {
        let mut posters: Vec<String> = Vec::new();
        for mut url in self
            .first_run_catalog()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|m| m.poster)
        {
            if url.scheme() == "http" {
                let _ = url.set_scheme("https");
            }
            // Links only: never `data:`, `file:` or any other scheme.
            if url.scheme() != "https" {
                continue;
            }
            let url = url.to_string();
            if !posters.contains(&url) {
                posters.push(url);
            }
            if posters.len() == WALL_POSTERS {
                break;
            }
        }
        posters
    }

    /// The first page of the catalog first run's art comes from, or none
    /// past `WALL_BUDGET`.
    async fn first_run_catalog(&self) -> Option<Vec<MetaItemPreview>> {
        let (d, catalog) = first_run_source()?;
        let addon = InstalledAddon {
            transport_url: url::Url::parse(&d.url).ok()?,
            manifest: d.manifest.clone(),
        };
        let path = ResourcePath {
            resource: "catalog".into(),
            r#type: catalog.r#type.clone(),
            id: catalog.id.clone(),
            extra: vec![],
        };
        let fetched = tokio::time::timeout(
            WALL_BUDGET,
            // A little past the budget, so the request itself never times out first and records a failure
            // in the health of an addon this profile may install later.
            self.addons()
                .fetch(&addon, &path, WALL_BUDGET + Duration::from_millis(200)),
        )
        .await
        .ok()?
        .ok()?;
        let ResourceResponse::Metas { metas } = decode_catalog(&fetched.body).ok()? else {
            return None;
        };
        Some(metas)
    }
}

/// First run's art source: the first default of group `catalogs` and its
/// first catalog that needs no extra.
fn first_run_source() -> Option<(&'static DefaultAddon, &'static ManifestCatalog)> {
    let d = list().iter().find(|d| d.group == "catalogs")?;
    let catalog = d
        .manifest
        .catalogs
        .iter()
        .find(|c| c.is_extra_supported(&[]))?;
    Some((d, catalog))
}

/// How long first run waits for its art before it keeps the wall's tiles
/// empty (and the still's frame black): for the catalog, then for TMDB. No
/// screen waits on it (each fades the art in when it lands), so the budget
/// is sized for a phone's first requests after install: cold DNS and TLS to
/// the catalog, its redirect host and TMDB. 1.5 s gave none on most cold
/// starts with 150 ms of round trip (plans/batch-15.md, E images).
const WALL_BUDGET: Duration = Duration::from_secs(6);

/// The most posters the wall is given.
const WALL_POSTERS: usize = 30;

/// How many of the catalog's top titles TMDB is asked for a textless still.
const STILL_TRIES: usize = 5;

/// The `firstRunStill` answer: a title's backdrop, its logo when it has
/// one, and its name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(rename_all = "camelCase")]
pub struct FirstRunStill {
    pub background: String,
    pub logo: Option<String>,
    pub name: String,
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
    fn the_bundled_defaults_are_installable_addons() {
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
                "kitsu",
                "streamingcatalogs",
                "stremioaddons"
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
