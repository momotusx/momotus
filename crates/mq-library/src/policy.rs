//! Kids mode's data (plans/m11-kids.md 3 and 4.1): the one gate, the
//! allowlist entries, and the reads that pair a decision with the access
//! generation it was made under.

use crate::store::{Library, LibraryEntry, LibraryError, entries};
use crate::sync::{OWNER, coll, queue, refuse_gone, sql};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;

/// Allowed titles a kids profile sees: the first this many by allow time.
pub const MAX_ALLOWED: usize = 1000;
/// Titles past the cap listed for the owner, newest first.
const OVER_LISTED: usize = 50;
/// Characters kept of each name.
const NAME_CHARS: usize = 200;
/// Other names kept per entry.
const MAX_NAMES: usize = 4;
/// `name` and `names` together, as serialized JSON bytes.
const NAMES_BYTES: usize = 1024;
/// Device state key of the access generation (raised by triggers, V10).
const GENERATION: &str = "access_generation";

/// What a profile may see and play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Policy {
    Open,
    /// Allowed titles by id, with their kind: the first 1,000 by allow time.
    Kids(HashMap<String, String>),
}

impl Policy {
    /// The one gate. `kind` and `id` come from an addon answer the core
    /// checked, a library row, the allowlist, or a play target the core
    /// verified. An empty or unknown pair never passes a kids policy.
    pub fn allows(&self, kind: &str, id: &str) -> bool {
        match self {
            Policy::Open => true,
            Policy::Kids(titles) => {
                !kind.is_empty() && !id.is_empty() && titles.get(id).is_some_and(|k| k == kind)
            }
        }
    }

    pub fn kids(&self) -> bool {
        matches!(self, Policy::Kids(_))
    }

    /// Whether `video_id` is `id` or `id` followed by ':' (3.4): `tt1:1:2`
    /// is of `tt1`, `tt10:1:1` is not.
    pub fn belongs(id: &str, video_id: &str) -> bool {
        !id.is_empty()
            && video_id
                .strip_prefix(id)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
    }
}

/// A decision with the access generation of the snapshot it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authorization {
    Authorized { generation: u64 },
    Denied { generation: u64 },
}

/// One allowlist entry (coll 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedRow {
    pub item_id: String,
    pub kind: String,
    pub name: String,
    /// Other names of the title, in the owner's addon order.
    pub names: Vec<String>,
    pub poster: Option<String>,
    /// `false` is a removal.
    pub allowed: bool,
    pub added_ms: i64,
}

impl AllowedRow {
    /// Held to the bounds of 3.1, the same for a local write and a record
    /// from another device: ids 1 to 256 and 1 to 32 bytes; names cleaned
    /// (control and format characters out, whitespace folded) before any
    /// bound, each up to 200 characters, at most 4 others without `name`
    /// or repeats, all together at most 1 KiB of JSON; a poster only as an
    /// `https` URL up to 512 bytes.
    pub(crate) fn bounded(mut self) -> Result<Self, &'static str> {
        if self.item_id.is_empty() || self.item_id.len() > 256 {
            return Err("the id must be 1 to 256 bytes");
        }
        if self.kind.is_empty() || self.kind.len() > 32 {
            return Err("the kind must be 1 to 32 bytes");
        }
        self.name = cut(&clean(&self.name));
        let mut names: Vec<String> = Vec::new();
        for name in std::mem::take(&mut self.names) {
            let name = cut(&clean(&name));
            if names.len() < MAX_NAMES
                && !name.is_empty()
                && name != self.name
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
        // A name alone is at most 200 characters, 802 bytes of JSON: only
        // the other names ever go.
        while json_len(&self.name) + json_len(&names) > NAMES_BYTES && names.pop().is_some() {}
        self.names = names;
        self.poster = self.poster.filter(|p| {
            p.starts_with("https://") && p.len() <= 512 && !p.chars().any(char::is_control)
        });
        Ok(self)
    }
}

/// A kids profile's list as its views and the owner's page read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowedList {
    /// The first 1,000 allowed by allow time: what the kid sees.
    pub counted: Vec<AllowedRow>,
    /// Up to 50 allowed past the cap, newest first.
    pub over: Vec<AllowedRow>,
    /// How many are past the cap.
    pub over_count: usize,
}

impl Library {
    /// The profile's policy. Fails closed: a read error, an unreadable row
    /// or a missing access register gives `Kids` with no titles.
    pub fn policy(&self, profile: &str) -> Policy {
        self.policy_checked(profile)
            .map(|(policy, _)| policy)
            .unwrap_or_else(|_| Policy::Kids(HashMap::new()))
    }

    /// The policy and the access generation, from one read transaction, so
    /// a read error is told apart from an exclusion.
    pub fn policy_checked(&self, profile: &str) -> Result<(Policy, u64), LibraryError> {
        self.policy_checked_with(profile, || {})
    }

    /// `policy_checked`, with `between` run after the policy is read and
    /// before the generation is (tests force a commit there).
    pub(crate) fn policy_checked_with(
        &self,
        profile: &str,
        between: impl FnOnce(),
    ) -> Result<(Policy, u64), LibraryError> {
        self.read(|c| {
            let tx = c.unchecked_transaction()?;
            let policy = read_policy(&tx, profile)?;
            between();
            Ok((policy, access_generation(&tx)?))
        })
    }

    /// Whether the profile may play `kind` `id`, with the generation that
    /// decision was read under: both from one read transaction. Under a kids
    /// policy the video is required and must belong to the title (a series
    /// is never authorized as a whole).
    pub fn authorize(
        &self,
        profile: &str,
        kind: &str,
        id: &str,
        video: Option<&str>,
    ) -> Result<Authorization, LibraryError> {
        self.authorize_with(profile, kind, id, video, || {})
    }

    /// `authorize`, with `between` run after the decision is read and
    /// before the generation is (tests force a commit there).
    pub(crate) fn authorize_with(
        &self,
        profile: &str,
        kind: &str,
        id: &str,
        video: Option<&str>,
        between: impl FnOnce(),
    ) -> Result<Authorization, LibraryError> {
        self.read(|c| {
            let tx = c.unchecked_transaction()?;
            let policy = read_policy(&tx, profile)?;
            let ok = policy.allows(kind, id)
                && (!policy.kids() || video.is_some_and(|v| Policy::belongs(id, v)));
            between();
            let generation = access_generation(&tx)?;
            Ok(if ok {
                Authorization::Authorized { generation }
            } else {
                Authorization::Denied { generation }
            })
        })
    }

    /// Writes an allowlist entry stamped `hlc` (taken when the owner's
    /// request arrived) only when that is newer than the stored one;
    /// otherwise nothing happens and the newer edit stands. Never stamped
    /// again above the stored one, so a slow Allow can never overtake a
    /// later Remove (3.1). Allowing a title that is allowed already keeps
    /// its allow time, so pressing Allow again never moves it past the cap.
    /// A new allow is admitted only while fewer than `MAX_ALLOWED` titles
    /// are allowed, read in the same write, so two Allows racing at 999
    /// cannot both land (`ListFull`); an edit older than the stored one is
    /// dropped quietly before that, full or not. Returns whether it was
    /// written.
    /// Refused for the owner, and for an entry outside its bounds.
    pub fn write_allowed(
        &self,
        profile: &str,
        row: &AllowedRow,
        hlc: &str,
    ) -> Result<bool, LibraryError> {
        if profile == OWNER {
            return Err(LibraryError::OwnerKids);
        }
        let mut row = row.clone().bounded().map_err(LibraryError::BadEntry)?;
        let mut full = false;
        let written = self.write(|c| {
            refuse_gone(c, profile)?;
            // The hlc first: an edit the stored one overtook changes nothing
            // quietly, a full list or not.
            let stored: Option<String> = c
                .query_row(
                    "SELECT hlc FROM allowed WHERE profile_id = ?1 AND item_id = ?2",
                    params![profile, row.item_id],
                    |r| r.get(0),
                )
                .optional()?;
            if stored.is_some_and(|s| s.as_str() >= hlc) {
                return Ok(false);
            }
            if row.allowed {
                let kept: Option<i64> = c
                    .query_row(
                        "SELECT added_ms FROM allowed
                         WHERE profile_id = ?1 AND item_id = ?2 AND allowed = 1",
                        params![profile, row.item_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if kept.is_none() {
                    let total: i64 = c.query_row(
                        "SELECT COUNT(*) FROM allowed WHERE profile_id = ?1 AND allowed = 1",
                        params![profile],
                        |r| r.get(0),
                    )?;
                    if total as usize >= MAX_ALLOWED {
                        full = true;
                        return Ok(false);
                    }
                }
                row.added_ms = kept.unwrap_or(row.added_ms);
            }
            let changed = sql::set_allowed(c, profile, &row, hlc)?;
            queue(c, changed, coll::ALLOWED, &[profile, &row.item_id])?;
            Ok(changed > 0)
        })?;
        if full {
            return Err(LibraryError::ListFull);
        }
        Ok(written)
    }

    /// The profile's allowlist: the counted titles and the extras past the
    /// cap, from one read transaction.
    pub fn allowed_list(&self, profile: &str) -> Result<AllowedList, LibraryError> {
        self.allowed_list_with(profile, || {})
    }

    /// `allowed_list`, with `between` run after the counted titles are read
    /// and before the extras are (tests force a commit there).
    pub(crate) fn allowed_list_with(
        &self,
        profile: &str,
        between: impl FnOnce(),
    ) -> Result<AllowedList, LibraryError> {
        self.read(|c| {
            let tx = c.unchecked_transaction()?;
            let counted = counted(&tx, profile)?;
            between();
            let total: i64 = tx.query_row(
                "SELECT COUNT(*) FROM allowed WHERE profile_id = ?1 AND allowed = 1",
                params![profile],
                |r| r.get(0),
            )?;
            let over_count = (total as usize).saturating_sub(counted.len());
            let mut stmt = tx.prepare(&format!(
                "{SELECT_ROW} FROM allowed WHERE profile_id = ?1 AND allowed = 1
                 ORDER BY added_ms DESC, item_id DESC LIMIT ?2"
            ))?;
            let over = stmt
                .query_map(params![profile, over_count.min(OVER_LISTED) as i64], row)?
                .collect::<rusqlite::Result<_>>()?;
            Ok(AllowedList {
                counted,
                over,
                over_count,
            })
        })
    }

    /// The counted titles as the Library screen shows them, each with its
    /// newest progress row (plans/m11-kids.md 4.6): a kids profile's
    /// library is its list.
    pub fn allowed_entries(&self, profile: &str) -> Result<Vec<LibraryEntry>, LibraryError> {
        self.read(|c| {
            entries(
                c,
                &format!(
                    "SELECT item_id, kind, name, poster, NULL AS poster_shape, added_ms {COUNTED}"
                ),
                params![profile, MAX_ALLOWED as i64],
            )
        })
    }
}

const SELECT_ROW: &str = "SELECT item_id, kind, name, names, poster, allowed, added_ms";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AllowedRow> {
    let names: String = r.get(3)?;
    Ok(AllowedRow {
        item_id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        names: serde_json::from_str(&names).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?,
        poster: r.get(4)?,
        allowed: r.get(5)?,
        added_ms: r.get(6)?,
    })
}

/// The first 1,000 allowed by allow time, ties by id (?1 the profile, ?2
/// `MAX_ALLOWED`): every device holding the same rows counts the same ones
/// (3.4), and every read of the list goes through it. The index orders by
/// allow time only, so the tie-break here is the one that decides.
const COUNTED: &str = "FROM allowed WHERE profile_id = ?1 AND allowed = 1
         ORDER BY added_ms, item_id LIMIT ?2";

fn counted(c: &Connection, profile: &str) -> rusqlite::Result<Vec<AllowedRow>> {
    let mut stmt = c.prepare(&format!("{SELECT_ROW} {COUNTED}"))?;
    stmt.query_map(params![profile, MAX_ALLOWED as i64], row)?
        .collect()
}

fn read_policy(c: &Connection, profile: &str) -> rusqlite::Result<Policy> {
    let kids: Option<bool> = c
        .query_row(
            "SELECT kids FROM profile_access WHERE profile_id = ?1",
            params![profile],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match kids {
        Some(false) => Policy::Open,
        // Kids, or not known yet: only what is allowed.
        Some(true) => Policy::Kids(
            counted(c, profile)?
                .into_iter()
                .map(|r| (r.item_id, r.kind))
                .collect(),
        ),
        None => Policy::Kids(HashMap::new()),
    })
}

fn access_generation(c: &Connection) -> rusqlite::Result<u64> {
    c.query_row(
        "SELECT CAST(value AS INTEGER) FROM device_state WHERE key = ?1",
        params![GENERATION],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .map(|g| g.unwrap_or(0) as u64)
}

/// Whitespace runs (tabs and line breaks too) folded to one space, none at
/// the ends, and every other control (Cc) and format (Cf) character out:
/// JSON escaping can then only grow a name by its quotes and backslashes.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if ch.is_control() || is_format(ch) {
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        out.push(ch);
    }
    out
}

fn cut(s: &str) -> String {
    s.chars().take(NAME_CHARS).collect()
}

fn json_len<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |s| s.len())
}

/// Unicode general category Cf (format characters), Unicode 15.
fn is_format(ch: char) -> bool {
    const CF: [(u32, u32); 20] = [
        (0x00AD, 0x00AD),
        (0x0600, 0x0605),
        (0x061C, 0x061C),
        (0x06DD, 0x06DD),
        (0x070F, 0x070F),
        (0x0890, 0x0891),
        (0x08E2, 0x08E2),
        (0x180E, 0x180E),
        (0x200B, 0x200F),
        (0x202A, 0x202E),
        (0x2060, 0x2064),
        (0x2066, 0x206F),
        (0xFEFF, 0xFEFF),
        (0xFFF9, 0xFFFB),
        (0x110BD, 0x110BD),
        (0x110CD, 0x110CD),
        (0x13430, 0x1343F),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0001, 0xE007F),
    ];
    let c = ch as u32;
    CF.iter().any(|&(lo, hi)| (lo..=hi).contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlc::Hlc;

    fn hlc(wall: u64) -> String {
        Hlc {
            wall_ms: wall,
            counter: 0,
            device: 1,
        }
        .encode()
    }

    /// A kids profile, made under an owner PIN.
    fn kid() -> Library {
        let lib = Library::temporary().unwrap();
        lib.set_pin_hash(OWNER, Some("h"), &hlc(1)).unwrap();
        lib.create_kids_profile("kid", "Kid", "", &hlc(1)).unwrap();
        lib
    }

    fn entry(id: &str, allowed: bool) -> AllowedRow {
        AllowedRow {
            item_id: id.into(),
            kind: "movie".into(),
            name: "Up".into(),
            names: vec![],
            poster: None,
            allowed,
            added_ms: 1,
        }
    }

    /// The decision and its generation come from one snapshot: a removal
    /// committed between the two reads changes neither, so the
    /// authorization names the generation before the removal.
    #[test]
    fn a_decision_and_its_generation_are_one_snapshot() {
        let lib = kid();
        lib.write_allowed("kid", &entry("tt1", true), &hlc(2))
            .unwrap();
        let (_, before) = lib.policy_checked("kid").unwrap();
        let auth = lib
            .authorize_with("kid", "movie", "tt1", Some("tt1"), || {
                lib.write_allowed("kid", &entry("tt1", false), &hlc(3))
                    .unwrap();
            })
            .unwrap();
        assert_eq!(auth, Authorization::Authorized { generation: before });
        let (policy, after) = lib.policy_checked("kid").unwrap();
        assert!(after > before, "the removal raised it");
        assert!(!policy.allows("movie", "tt1"));
        assert_eq!(
            lib.authorize("kid", "movie", "tt1", Some("tt1")).unwrap(),
            Authorization::Denied { generation: after }
        );
    }

    /// The same for the policy the watcher reads: a removal committed
    /// between the policy and the generation leaves both as they were.
    #[test]
    fn a_policy_and_its_generation_are_one_snapshot() {
        let lib = kid();
        lib.write_allowed("kid", &entry("tt1", true), &hlc(2))
            .unwrap();
        let (_, before) = lib.policy_checked("kid").unwrap();
        let (policy, generation) = lib
            .policy_checked_with("kid", || {
                lib.write_allowed("kid", &entry("tt1", false), &hlc(3))
                    .unwrap();
            })
            .unwrap();
        assert!(policy.allows("movie", "tt1"));
        assert_eq!(generation, before, "the generation of that same policy");
    }

    /// The counted titles and the extras come from one snapshot: an allow
    /// committed between the two reads shows in neither.
    #[test]
    fn a_list_and_its_extras_are_one_snapshot() {
        let lib = kid();
        for i in 0..MAX_ALLOWED {
            let row = AllowedRow {
                added_ms: i as i64,
                ..entry(&format!("tt{i:04}"), true)
            };
            lib.write_allowed("kid", &row, &hlc(10 + i as u64)).unwrap();
        }
        let list = lib
            .allowed_list_with("kid", || {
                // Past the cap only from another device.
                let late = crate::sync::SyncRecord {
                    coll: coll::ALLOWED,
                    key: "kid\u{1f}tt9999".into(),
                    value: serde_json::json!({"profile": "kid", "itemId": "tt9999",
                        "kind": "movie", "name": "Up", "allowed": true, "addedMs": 5000}),
                    hlc: hlc(5000),
                };
                lib.apply(&[late]).unwrap();
            })
            .unwrap();
        assert_eq!(list.counted.len(), MAX_ALLOWED);
        assert_eq!((list.over_count, list.over.len()), (0, 0), "{list:?}");
        assert_eq!(lib.allowed_list("kid").unwrap().over_count, 1);
    }

    /// Every Cf range of the table goes, at both ends.
    #[test]
    fn format_characters_go_and_whitespace_folds() {
        let ranges = [
            (0x00AD, 0x00AD),
            (0x0600, 0x0605),
            (0x200B, 0x200F),
            (0x202A, 0x202E),
            (0x2060, 0x2064),
            (0x2066, 0x206F),
            (0xFEFF, 0xFEFF),
            (0x13430, 0x1343F),
            (0xE0001, 0xE007F),
        ];
        for (lo, hi) in ranges {
            for c in [lo, hi] {
                let ch = char::from_u32(c).unwrap();
                assert_eq!(clean(&format!("U{ch}p")), "Up", "U+{c:04X}");
            }
        }
        assert_eq!(clean("\t Up \n\n and\u{7}  away "), "Up and away");
    }
}
