//! Profiles (plans/m11-profiles.md): three registers per profile, the
//! cosmetic `profiles` row, the `profile_lock` (its PIN hash) and the
//! remove-wins `profile_gone`, plus what stays on this device (failed PIN
//! attempts, the active profile).

use crate::store::{AddonRow, Library, LibraryError};
use crate::sync::{OWNER, Stamp, coll, gone, local, local_changed, queue, refuse_gone, sql};
use rusqlite::{OptionalExtension, params};

/// One profile as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRow {
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub position: i64,
    /// The lock register: `None` until it arrives (the profile is still
    /// syncing and cannot be used), `Some(None)` for no PIN.
    pub lock: Option<Option<String>>,
    /// The access register: `None` until it arrives, `Some(true)` for a
    /// kids profile (plans/m11-kids.md 3.1).
    pub access: Option<bool>,
}

impl ProfileRow {
    /// Usable: its lock and access registers are here, so "no PIN" and
    /// "not kids" are known, not guessed.
    pub fn ready(&self) -> bool {
        self.lock.is_some() && self.access.is_some()
    }

    /// Kids, or not known yet: fails closed, as the policy does. Only a
    /// standard access register reads as not kids.
    pub fn kids(&self) -> bool {
        self.access != Some(false)
    }

    pub fn pin_hash(&self) -> Option<&str> {
        self.lock.as_ref().and_then(|l| l.as_deref())
    }
}

const ACTIVE: &str = "active_profile";

impl Library {
    /// Every profile that is not gone, by position.
    pub fn profiles(&self) -> Result<Vec<ProfileRow>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT p.id, p.name, p.avatar, p.position, l.profile_id IS NOT NULL, l.pin_hash,
                        a.kids
                 FROM profiles p LEFT JOIN profile_lock l ON l.profile_id = p.id
                 LEFT JOIN profile_access a ON a.profile_id = p.id
                 WHERE p.id NOT IN (SELECT profile_id FROM profile_gone)
                 ORDER BY p.position, p.id",
            )?;
            stmt.query_map([], |r| {
                Ok(ProfileRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    avatar: r.get(2)?,
                    position: r.get(3)?,
                    lock: r.get::<_, bool>(4)?.then(|| r.get(5)).transpose()?,
                    access: r.get(6)?,
                })
            })?
            .collect()
        })
    }

    pub fn profile(&self, id: &str) -> Result<Option<ProfileRow>, LibraryError> {
        Ok(self.profiles()?.into_iter().find(|p| p.id == id))
    }

    pub fn is_gone(&self, id: &str) -> Result<bool, LibraryError> {
        self.read(|c| gone(c, id))
    }

    /// Ids of every profile removed for good.
    pub fn gone_profiles(&self) -> Result<Vec<String>, LibraryError> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT profile_id FROM profile_gone ORDER BY profile_id")?;
            stmt.query_map([], |r| r.get(0))?.collect()
        })
    }

    /// Creates the profile with its lock register (no PIN), its access
    /// register (standard) and a copy of the owner's addon list, in one
    /// transaction, after the last position.
    pub fn create_profile<'a>(
        &self,
        id: &str,
        name: &str,
        avatar: &str,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        self.create(id, name, avatar, false, stamp.into())
    }

    /// Creates a kids profile (plans/m11-kids.md 6.1): its lock register
    /// (no PIN; a kids profile never has one) and its access register, in
    /// one transaction. No addon copy: it reads the owner's list. Refused
    /// for the owner.
    pub fn create_kids_profile<'a>(
        &self,
        id: &str,
        name: &str,
        avatar: &str,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        if id == OWNER {
            return Err(LibraryError::OwnerKids);
        }
        self.create(id, name, avatar, true, stamp.into())
    }

    /// Whether a profile row with this id is here (gone or not).
    fn exists(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<bool> {
        c.query_row(
            "SELECT EXISTS (SELECT 1 FROM profiles WHERE id = ?1)",
            params![id],
            |r| r.get(0),
        )
    }

    fn create(
        &self,
        id: &str,
        name: &str,
        avatar: &str,
        kids: bool,
        stamp: Stamp<'_>,
    ) -> Result<(), LibraryError> {
        let (mut taken, mut no_pin) = (false, false);
        self.write(|c| {
            // A kids profile is new: never an existing one made kids (its
            // PIN cleared, its kind changed).
            if kids && Self::exists(c, id)? {
                taken = true;
                return Ok(());
            }
            // Read in the write, so a removal of the owner's PIN racing
            // this one ends with one of the two refused.
            if kids && !owner_has_pin(c)? {
                no_pin = true;
                return Ok(());
            }
            let position: i64 = c.query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM profiles",
                [],
                |r| r.get(0),
            )?;
            local(c, coll::PROFILES, &[id], &stamp, |h| {
                sql::set_profile(c, id, name, avatar, position, h)
            })?;
            local(c, coll::PROFILE_LOCK, &[id], &stamp, |h| {
                sql::set_lock(c, id, None, h)
            })?;
            local(c, coll::PROFILE_ACCESS, &[id], &stamp, |h| {
                sql::set_access(c, id, kids, h)
            })?;
            if kids {
                return Ok(());
            }
            copy_owner_addons(c, id, &stamp)
        })?;
        if taken {
            return Err(LibraryError::ProfileExists(id.to_owned()));
        }
        if no_pin {
            return Err(LibraryError::KidsNeedPin);
        }
        Ok(())
    }

    /// Clears the owner's PIN unless a kids profile is here (one whose
    /// access is not known yet counts), read in the same write as the
    /// clear (plans/m11-kids.md 6.1). `resetOwnerPin` goes around it.
    pub fn clear_owner_pin<'a>(&self, stamp: impl Into<Stamp<'a>>) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        let mut kids = false;
        self.write(|c| {
            kids = c.query_row(
                "SELECT EXISTS (SELECT 1 FROM profiles p
                   LEFT JOIN profile_access a ON a.profile_id = p.id
                   WHERE p.id != ?1 AND COALESCE(a.kids, 1) = 1
                     AND p.id NOT IN (SELECT profile_id FROM profile_gone))",
                params![OWNER],
                |r| r.get(0),
            )?;
            if kids {
                return Ok(());
            }
            local(c, coll::PROFILE_LOCK, &[OWNER], &stamp, |h| {
                sql::set_lock(c, OWNER, None, h)
            })
        })?;
        if kids {
            return Err(LibraryError::KidsNeedPin);
        }
        Ok(())
    }

    /// Turns a kids profile into a standard one (plans/m11-kids.md 6.1),
    /// with a copy of the owner's addon list, in one transaction. There is
    /// no way back: a standard profile never becomes kids. Read inside the
    /// write: anything but a kids profile is left as it is (its addon edits
    /// never rewritten from the owner's), and so is one whose stored access
    /// version is at or above a fixed stamp. Returns whether it changed.
    pub fn make_standard<'a>(
        &self,
        id: &str,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<bool, LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            let kids: Option<bool> = c
                .query_row(
                    "SELECT kids FROM profile_access WHERE profile_id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?;
            if kids != Some(true) {
                return Ok(false);
            }
            if !local_changed(c, coll::PROFILE_ACCESS, &[id], &stamp, |h| {
                sql::set_access(c, id, false, h)
            })? {
                return Ok(false);
            }
            copy_owner_addons(c, id, &stamp)?;
            Ok(true)
        })
    }

    /// Rewrites the cosmetic register (name, avatar, position) whole.
    pub fn update_profile<'a>(
        &self,
        id: &str,
        name: &str,
        avatar: &str,
        position: i64,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            local(c, coll::PROFILES, &[id], &stamp, |h| {
                sql::set_profile(c, id, name, avatar, position, h)
            })
        })
    }

    /// Sets (a PHC string) or clears the profile's PIN.
    pub fn set_pin_hash<'a>(
        &self,
        id: &str,
        pin_hash: Option<&str>,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        let stamp = stamp.into();
        self.write(|c| {
            local(c, coll::PROFILE_LOCK, &[id], &stamp, |h| {
                sql::set_lock(c, id, pin_hash, h)
            })
        })
    }

    /// Removes the profile for good: its gone register is written and
    /// queued, and every row it had on this device goes, in one
    /// transaction. Refused for the owner.
    pub fn delete_profile<'a>(
        &self,
        id: &str,
        stamp: impl Into<Stamp<'a>>,
    ) -> Result<(), LibraryError> {
        if id == OWNER {
            return Err(LibraryError::OwnerProfile);
        }
        let stamp = stamp.into();
        self.write(|c| {
            self.mark_removal();
            let changed = sql::set_gone(c, id, &stamp.hlc())?;
            queue(c, changed, coll::PROFILE_GONE, &[id])
        })?;
        self.checkpoint();
        Ok(())
    }

    /// Failed PIN attempts on this device since the last right one.
    pub fn pin_failures(&self, id: &str) -> Result<u32, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT failures FROM pin_attempts WHERE profile_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map(Option::unwrap_or_default)
        })
    }

    /// Refused for a gone profile, so an attempt that ends after the purge
    /// cannot bring its row back.
    pub fn set_pin_failures(&self, id: &str, failures: u32) -> Result<(), LibraryError> {
        self.write(|c| {
            refuse_gone(c, id)?;
            c.execute(
                "INSERT INTO pin_attempts (profile_id, failures) VALUES (?1, ?2)
                 ON CONFLICT(profile_id) DO UPDATE SET failures = excluded.failures",
                params![id, failures],
            )
            .map(drop)
        })
    }

    /// A value this device keeps for itself (never synced, never a profile's).
    pub fn device_value(&self, key: &str) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT value FROM device_state WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn set_device_value(&self, key: &str, value: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO device_state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(drop)
        })
    }

    /// The profile this device last switched to, as stored (it may be gone
    /// since).
    pub fn active_profile(&self) -> Result<Option<String>, LibraryError> {
        self.read(|c| {
            c.query_row(
                "SELECT value FROM device_state WHERE key = ?1",
                params![ACTIVE],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn set_active_profile(&self, id: &str) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO device_state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![ACTIVE, id],
            )
            .map(drop)
        })
    }
}

/// Whether the owner's lock register holds a PIN.
fn owner_has_pin(c: &rusqlite::Connection) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM profile_lock
           WHERE profile_id = ?1 AND pin_hash IS NOT NULL)",
        params![OWNER],
        |r| r.get(0),
    )
}

/// Settings keys under this prefix, one per default addon key, record the
/// defaults a profile has had (mq-runtime's defaults rule). One register
/// each, so two devices that recorded different ones keep both; they travel
/// with a copied addon list.
pub const DEFAULTS_GIVEN: &str = "defaultsGiven.";

/// Copies the owner's addon list to `id`, with what that list was given
/// (`DEFAULTS_GIVEN`): a default the owner removed is not given to the copy.
fn copy_owner_addons(
    c: &rusqlite::Connection,
    id: &str,
    stamp: &Stamp<'_>,
) -> rusqlite::Result<()> {
    let mut stmt = c.prepare(
        "SELECT a.transport_url, a.manifest_json, COALESCE(o.position, 0) AS place, a.enabled, a.allow_private
         FROM addons a LEFT JOIN addon_order o ON o.profile_id = a.profile_id AND o.url_hash = a.url_hash
         WHERE a.profile_id = ?1 AND a.installed = 1 ORDER BY place, a.transport_url",
    )?;
    let addons = stmt
        .query_map(params![OWNER], |r| {
            Ok(AddonRow {
                transport_url: r.get(0)?,
                manifest_json: r.get(1)?,
                position: r.get(2)?,
                enabled: r.get(3)?,
                allow_private: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // The copy replaces the profile's own list, addon by addon.
    let own = crate::store::addon_rows(c, id)?;
    crate::store::write_addons(c, id, &own, addons, stamp)?;
    let mut stmt = c.prepare("SELECT key, value_json FROM settings WHERE profile_id = ?1")?;
    let given = stmt
        .query_map(params![OWNER], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (key, value) in given.iter().filter(|(k, _)| k.starts_with(DEFAULTS_GIVEN)) {
        local(c, coll::SETTINGS, &[id, key], stamp, |h| {
            sql::set_setting(c, id, key, value, h)
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlc::Hlc;
    use crate::store::CachedResponse;
    use crate::sync::{DEVICE_TABLES, PROFILE_TABLES, SyncRecord};

    fn hlc(wall: u64) -> String {
        Hlc {
            wall_ms: wall,
            counter: 0,
            device: 1,
        }
        .encode()
    }

    /// A new table must be purged with a profile or named device-wide, so a
    /// removal can never leave a profile's rows behind unnoticed.
    #[test]
    fn every_table_is_purged_with_a_profile_or_device_wide() {
        let lib = Library::temporary().unwrap();
        let tables: Vec<String> = lib
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                )?;
                stmt.query_map([], |r| r.get(0))?.collect()
            })
            .unwrap();
        for table in &tables {
            let known = table == "profiles"
                || PROFILE_TABLES.contains(&table.as_str())
                || DEVICE_TABLES.contains(&table.as_str());
            assert!(known, "{table} is neither purged nor device-wide");
        }
        // Every purged table really is keyed by profile.
        for table in PROFILE_TABLES {
            let has: bool = lib
                .read(|c| {
                    c.query_row(
                        "SELECT EXISTS (SELECT 1 FROM pragma_table_info(?1) WHERE name = 'profile_id')",
                        [table],
                        |r| r.get(0),
                    )
                })
                .unwrap();
            assert!(has, "{table} has no profile_id");
        }
    }

    /// Removing a profile, here or by sync, leaves none of its text in the
    /// database or its log, not even in free pages.
    #[test]
    fn a_removed_profile_leaves_nothing_readable_in_the_files() {
        let dir = std::env::temp_dir().join(format!("mq-scrub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.sqlite");
        let lib = Library::open(&path).unwrap();
        let fill = |profile: &str, secret: &str| {
            lib.create_profile(profile, profile, "", &hlc(1)).unwrap();
            let value = format!("\"{secret}\"");
            lib.set_setting(profile, "notes", &value, &hlc(2)).unwrap();
            lib.record_search(profile, secret, 1).unwrap();
            let payload = format!("{{\"authKey\":\"{secret}\"}}");
            lib.store_raw_import(profile, "stremio", 1, &payload)
                .unwrap();
            let cached = CachedResponse {
                body: b"{}".to_vec(),
                stored_ms: 1,
                max_age_s: 60,
                stale_revalidate_s: 0,
                stale_error_s: 0,
            };
            let url = format!("https://addon.example/{secret}/catalog");
            lib.cache_put(&url, &cached, lib.removals()).unwrap();
        };
        fill("x", "SECRET-local-7f3a9c");
        fill("z", "SECRET-synced-41be");
        lib.set_setting("y", "notes", "\"kept-0d2e\"", &hlc(2))
            .unwrap();
        // Long since written: the rows sit in the database file itself.
        lib.checkpoint();

        let found = |text: &str| {
            ["", "-wal"].iter().any(|suffix| {
                let mut file = path.clone().into_os_string();
                file.push(suffix);
                let bytes = std::fs::read(file).unwrap_or_default();
                bytes.windows(text.len()).any(|w| w == text.as_bytes())
            })
        };
        assert!(found("SECRET-local"), "the search really reads the files");

        // Each kind of removal is checked before the next one runs.
        lib.delete_profile("x", &hlc(3)).unwrap();
        assert!(!found("SECRET-local"), "a local removal left text behind");
        assert!(found("SECRET-synced"));
        let gone = SyncRecord {
            coll: crate::sync::coll::PROFILE_GONE,
            key: "z".into(),
            value: serde_json::json!({"profile": "z"}),
            hlc: hlc(4),
        };
        lib.apply(&[gone]).unwrap();
        assert!(!found("SECRET-synced"), "a synced removal left text behind");
        assert!(found("kept-0d2e"), "other profiles stay");
        drop(lib);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod verify_round {
    use super::*;
    use crate::hlc::Hlc;
    use crate::store::CachedResponse;
    use std::path::{Path, PathBuf};

    fn hlc(wall: u64) -> String {
        Hlc {
            wall_ms: wall,
            counter: 0,
            device: 1,
        }
        .encode()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mq-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Whether `text` is anywhere in the database or its log.
    fn in_files(path: &Path, text: &str) -> bool {
        ["", "-wal"].iter().any(|suffix| {
            let mut file = path.to_path_buf().into_os_string();
            file.push(suffix);
            let bytes = std::fs::read(file).unwrap_or_default();
            bytes.windows(text.len()).any(|w| w == text.as_bytes())
        })
    }

    /// V7 raw imports had no profile: with one profile they are its, with
    /// more nobody can tell and they go (addon keys can be in them).
    #[test]
    fn raw_imports_from_before_profiles_get_their_owner_or_go() {
        for (profiles, kept) in [(&["default"][..], true), (&["default", "kid"][..], false)] {
            let dir = temp("migrate");
            let path = dir.join("library.sqlite");
            {
                let mut c = rusqlite::Connection::open(&path).unwrap();
                crate::schema::migrations().to_version(&mut c, 7).unwrap();
                for p in profiles {
                    c.execute(
                        "INSERT INTO profiles (id, name, hlc) VALUES (?1, ?1, '0')",
                        [p],
                    )
                    .unwrap();
                }
                c.execute(
                    "INSERT INTO import_raw (source, imported_ms, payload) VALUES ('stremio', 1, 'SECRET-legacy-9b')",
                    [],
                )
                .unwrap();
            }
            let lib = Library::open(&path).unwrap();
            let rows: Vec<(String, String)> = lib
                .read(|c| {
                    let mut stmt = c.prepare("SELECT profile_id, payload FROM import_raw")?;
                    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                        .collect()
                })
                .unwrap();
            if kept {
                assert_eq!(rows, [("default".into(), "SECRET-legacy-9b".into())]);
            } else {
                assert!(rows.is_empty(), "{rows:?}");
                assert!(!in_files(&path, "SECRET-legacy"), "dropped, not readable");
            }
            // New rows must name their profile.
            let unnamed = lib.write(|c| {
                c.execute(
                    "INSERT INTO import_raw (source, imported_ms, payload) VALUES ('x', 1, 'y')",
                    [],
                )
            });
            assert!(unnamed.is_err());
            drop(lib);
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// A reader in the middle of a read holds the scrub up; it finishes
    /// once the reader is done and the library is used again.
    #[test]
    fn a_scrub_held_up_by_a_reader_finishes_after_it() {
        let dir = temp("held");
        let path = dir.join("library.sqlite");
        let lib = Library::open(&path).unwrap();
        lib.create_profile("x", "x", "", &hlc(1)).unwrap();
        lib.set_setting("x", "notes", "\"SECRET-held-5c1\"", &hlc(2))
            .unwrap();
        lib.checkpoint();
        let reader = rusqlite::Connection::open(&path).unwrap();
        reader.execute_batch("BEGIN").unwrap();
        let _: i64 = reader
            .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))
            .unwrap();

        lib.delete_profile("x", &hlc(3)).unwrap();
        assert!(in_files(&path, "SECRET-held"), "the reader holds it up");
        reader.execute_batch("COMMIT").unwrap();
        lib.profiles().unwrap();
        assert!(!in_files(&path, "SECRET-held"), "scrubbed once it could");
        drop((reader, lib));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// An addon answer fetched before a removal is not stored after it.
    #[test]
    fn a_cache_write_a_removal_overtook_is_dropped() {
        let lib = Library::temporary().unwrap();
        lib.create_profile("x", "x", "", &hlc(1)).unwrap();
        let row = CachedResponse {
            body: b"{}".to_vec(),
            stored_ms: 1,
            max_age_s: 60,
            stale_revalidate_s: 0,
            stale_error_s: 0,
        };
        let before = lib.removals();
        lib.delete_profile("x", &hlc(2)).unwrap();
        assert!(!lib.cache_put("https://a/x", &row, before).unwrap());
        assert_eq!(lib.cache_get("https://a/x").unwrap(), None);
        assert!(lib.cache_put("https://a/x", &row, lib.removals()).unwrap());
        // A removal arriving by sync counts too.
        let before = lib.removals();
        let gone = crate::sync::SyncRecord {
            coll: coll::PROFILE_GONE,
            key: "y".into(),
            value: serde_json::json!({"profile": "y"}),
            hlc: hlc(3),
        };
        lib.apply(&[gone]).unwrap();
        assert_ne!(lib.removals(), before);
    }
}
