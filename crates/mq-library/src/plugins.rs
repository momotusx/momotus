//! Nuvio plugin repositories per profile and the scripts of their providers
//! (plans/plugins-and-marketplace.md 7.2), never synced. The runtime reads
//! and checks the manifests; this keeps them.

use crate::store::{Library, LibraryError};
use rusqlite::{OptionalExtension, params};

/// Repositories a profile keeps at most, removed ones aside.
pub const MAX_REPOS: i64 = 20;

/// One repository of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRepoRow {
    pub url: String,
    pub enabled: bool,
    /// The last manifest read, as fetched; none before the first read.
    pub manifest_json: Option<String>,
    /// When it was read, in ms; 0 before the first read.
    pub fetched_at: i64,
    /// Providers the user turned off, by id.
    pub off: Vec<String>,
}

/// A provider's script as last fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCode {
    pub version: String,
    pub body: String,
    pub fetched_at: i64,
}

impl Library {
    /// The profile's repositories in order, removed ones left out.
    pub fn plugin_repos(&self, profile: &str) -> Result<Vec<PluginRepoRow>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT url, enabled, manifest_json, fetched_at, off_json FROM plugin_repos
                 WHERE profile_id = ?1 AND removed = 0 ORDER BY position",
            )?;
            stmt.query_map([profile], |r| {
                let off: String = r.get(4)?;
                Ok(PluginRepoRow {
                    url: r.get(0)?,
                    enabled: r.get(1)?,
                    manifest_json: r.get(2)?,
                    fetched_at: r.get(3)?,
                    off: serde_json::from_str(&off).unwrap_or_default(),
                })
            })?
            .collect()
        })
    }

    /// Whether the profile ever had `url`, removed or not.
    pub fn plugin_repo_known(&self, profile: &str, url: &str) -> Result<bool, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT 1 FROM plugin_repos WHERE profile_id = ?1 AND url = ?2",
                params![profile, url],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
        })
    }

    /// Adds `url` at the end, on or off, or brings a removed one back or
    /// switches a present one. False when the profile already keeps
    /// `MAX_REPOS`.
    pub fn put_plugin_repo(
        &self,
        profile: &str,
        url: &str,
        enabled: bool,
    ) -> Result<bool, LibraryError> {
        self.write(|c| {
            let (kept, present): (i64, bool) = c.query_row(
                "SELECT COUNT(*) FILTER (WHERE removed = 0),
                        COUNT(*) FILTER (WHERE removed = 0 AND url = ?2) > 0
                 FROM plugin_repos WHERE profile_id = ?1",
                params![profile, url],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if !present && kept >= MAX_REPOS {
                return Ok(false);
            }
            c.execute(
                "INSERT INTO plugin_repos (profile_id, url, position, enabled)
                 VALUES (?1, ?2, (SELECT COALESCE(MAX(position) + 1, 0) FROM plugin_repos WHERE profile_id = ?1), ?3)
                 ON CONFLICT (profile_id, url) DO UPDATE SET
                   enabled = excluded.enabled,
                   position = CASE WHEN removed = 1 THEN excluded.position ELSE position END,
                   removed = 0",
                params![profile, url, enabled],
            )?;
            Ok(true)
        })
    }

    /// Takes `url` out of the list; its row stays as removed.
    pub fn remove_plugin_repo(&self, profile: &str, url: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "UPDATE plugin_repos SET removed = 1, manifest_json = NULL, fetched_at = 0, off_json = '[]'
                 WHERE profile_id = ?1 AND url = ?2",
                params![profile, url],
            )
            .map(drop)
        })
    }

    /// Keeps the manifest just read for `url`.
    pub fn set_plugin_manifest(
        &self,
        profile: &str,
        url: &str,
        manifest_json: &str,
        fetched_at: i64,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "UPDATE plugin_repos SET manifest_json = ?3, fetched_at = ?4
                 WHERE profile_id = ?1 AND url = ?2 AND removed = 0",
                params![profile, url, manifest_json, fetched_at],
            )
            .map(drop)
        })
    }

    /// Turns provider `id` of `url` on or off.
    pub fn set_plugin_off(
        &self,
        profile: &str,
        url: &str,
        id: &str,
        off: bool,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let json: Option<String> = c
                .query_row(
                    "SELECT off_json FROM plugin_repos WHERE profile_id = ?1 AND url = ?2 AND removed = 0",
                    params![profile, url],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(json) = json else { return Ok(()) };
            let mut list: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
            list.retain(|x| x != id);
            if off {
                list.push(id.to_owned());
            }
            let json = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
            c.execute(
                "UPDATE plugin_repos SET off_json = ?3 WHERE profile_id = ?1 AND url = ?2",
                params![profile, url, json],
            )
            .map(drop)
        })
    }

    /// The script last fetched from `url`.
    pub fn plugin_code(&self, url: &str) -> Result<Option<PluginCode>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT version, body, fetched_at FROM plugin_code WHERE url = ?1",
                [url],
                |r| {
                    Ok(PluginCode {
                        version: r.get(0)?,
                        body: r.get(1)?,
                        fetched_at: r.get(2)?,
                    })
                },
            )
            .optional()
        })
    }

    pub fn put_plugin_code(&self, url: &str, code: &PluginCode) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO plugin_code (url, version, body, fetched_at) VALUES (?1, ?2, ?3, ?4)",
                params![url, code.version, code.body, code.fetched_at],
            )
            .map(drop)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Library;

    #[test]
    fn repositories_keep_order_switches_and_a_removed_mark() {
        let lib = Library::temporary().unwrap();
        let p = "default";
        assert!(
            lib.put_plugin_repo(p, "https://a/manifest.json", true)
                .unwrap()
        );
        assert!(
            lib.put_plugin_repo(p, "https://b/manifest.json", false)
                .unwrap()
        );
        let urls = |lib: &Library| -> Vec<(String, bool)> {
            (lib.plugin_repos(p).unwrap().into_iter())
                .map(|r| (r.url, r.enabled))
                .collect()
        };
        assert_eq!(
            urls(&lib),
            [
                ("https://a/manifest.json".to_owned(), true),
                ("https://b/manifest.json".to_owned(), false)
            ]
        );
        lib.set_plugin_off(p, "https://a/manifest.json", "x", true)
            .unwrap();
        lib.set_plugin_off(p, "https://a/manifest.json", "y", true)
            .unwrap();
        lib.set_plugin_off(p, "https://a/manifest.json", "x", false)
            .unwrap();
        assert_eq!(lib.plugin_repos(p).unwrap()[0].off, ["y"]);
        lib.set_plugin_manifest(p, "https://a/manifest.json", "{}", 5)
            .unwrap();
        lib.remove_plugin_repo(p, "https://a/manifest.json")
            .unwrap();
        assert_eq!(urls(&lib), [("https://b/manifest.json".to_owned(), false)]);
        // Removed, it is still known: a default is not given again.
        assert!(lib.plugin_repo_known(p, "https://a/manifest.json").unwrap());
        assert!(
            !lib.plugin_repo_known("other", "https://a/manifest.json")
                .unwrap()
        );
        // Added again, it comes back last, with nothing of before.
        lib.put_plugin_repo(p, "https://a/manifest.json", true)
            .unwrap();
        let back = lib.plugin_repos(p).unwrap();
        assert_eq!(back[1].url, "https://a/manifest.json");
        assert_eq!(
            (back[1].manifest_json.as_deref(), back[1].off.len()),
            (None, 0)
        );
    }

    #[test]
    fn a_profile_keeps_at_most_twenty_repositories() {
        let lib = Library::temporary().unwrap();
        for i in 0..super::MAX_REPOS {
            assert!(
                lib.put_plugin_repo("default", &format!("https://{i}/"), true)
                    .unwrap()
            );
        }
        assert!(
            !lib.put_plugin_repo("default", "https://more/", true)
                .unwrap()
        );
        // A present one still switches.
        assert!(lib.put_plugin_repo("default", "https://0/", false).unwrap());
    }

    #[test]
    fn code_is_kept_by_link() {
        let lib = Library::temporary().unwrap();
        let code = super::PluginCode {
            version: "1.0".into(),
            body: "x".into(),
            fetched_at: 3,
        };
        lib.put_plugin_code("https://a/p.js", &code).unwrap();
        assert_eq!(lib.plugin_code("https://a/p.js").unwrap(), Some(code));
        assert_eq!(lib.plugin_code("https://b/p.js").unwrap(), None);
    }
}
