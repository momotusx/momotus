//! Profiles (plans/m11-profiles.md): three registers per profile, the
//! cosmetic `profiles` row, the `profile_lock` (its PIN hash) and the
//! remove-wins `profile_gone`, plus what stays on this device (failed PIN
//! attempts, the active profile).

use crate::store::{AddonRow, Library, LibraryError};
use crate::sync::{OWNER, coll, gone, local, queue, sql};
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
}

impl ProfileRow {
    /// Usable: its lock register is here, so "no PIN" is known, not guessed.
    pub fn ready(&self) -> bool {
        self.lock.is_some()
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
                "SELECT p.id, p.name, p.avatar, p.position, l.profile_id IS NOT NULL, l.pin_hash
                 FROM profiles p LEFT JOIN profile_lock l ON l.profile_id = p.id
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

    /// Creates the profile with its lock register (no PIN) and a copy of
    /// the owner's addon list, in one transaction, after the last position.
    pub fn create_profile(
        &self,
        id: &str,
        name: &str,
        avatar: &str,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            let position: i64 = c.query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM profiles",
                [],
                |r| r.get(0),
            )?;
            local(c, coll::PROFILES, &[id], hlc, |h| {
                sql::set_profile(c, id, name, avatar, position, h)
            })?;
            local(c, coll::PROFILE_LOCK, &[id], hlc, |h| {
                sql::set_lock(c, id, None, h)
            })?;
            let mut stmt = c.prepare(
                "SELECT transport_url, manifest_json, position, enabled, allow_private
                 FROM addons WHERE profile_id = ?1 ORDER BY position",
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
            local(c, coll::ADDONS, &[id], hlc, |h| {
                sql::set_addons(c, id, &addons, h)
            })
        })
    }

    /// Rewrites the cosmetic register (name, avatar, position) whole.
    pub fn update_profile(
        &self,
        id: &str,
        name: &str,
        avatar: &str,
        position: i64,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            local(c, coll::PROFILES, &[id], hlc, |h| {
                sql::set_profile(c, id, name, avatar, position, h)
            })
        })
    }

    /// Sets (a PHC string) or clears the profile's PIN.
    pub fn set_pin_hash(
        &self,
        id: &str,
        pin_hash: Option<&str>,
        hlc: &str,
    ) -> Result<(), LibraryError> {
        self.write(|c| {
            local(c, coll::PROFILE_LOCK, &[id], hlc, |h| {
                sql::set_lock(c, id, pin_hash, h)
            })
        })
    }

    /// Removes the profile for good: its gone register is written and
    /// queued, and every row it had on this device goes, in one
    /// transaction. Refused for the owner.
    pub fn delete_profile(&self, id: &str, hlc: &str) -> Result<(), LibraryError> {
        if id == OWNER {
            return Err(LibraryError::OwnerProfile);
        }
        self.write(|c| {
            let changed = sql::set_gone(c, id, hlc)?;
            queue(c, changed, coll::PROFILE_GONE, &[id])
        })
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

    pub fn set_pin_failures(&self, id: &str, failures: u32) -> Result<(), LibraryError> {
        self.write(|c| {
            c.execute(
                "INSERT INTO pin_attempts (profile_id, failures) VALUES (?1, ?2)
                 ON CONFLICT(profile_id) DO UPDATE SET failures = excluded.failures",
                params![id, failures],
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
