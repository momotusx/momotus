use rusqlite_migration::{M, Migrations};

pub(crate) fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(V1),
        M::up(V2),
        M::up(V3),
        M::up(V4),
        M::up(V5),
        M::up(V6),
        M::up(V7),
        M::up(V8),
        M::up(V9),
        M::up(V10),
    ])
}

const V1: &str = r#"
CREATE TABLE profiles (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    kids        INTEGER NOT NULL DEFAULT 0,
    pin_hash    TEXT,
    hlc         TEXT NOT NULL
);

CREATE TABLE addons (
    profile_id     TEXT NOT NULL,
    transport_url  TEXT NOT NULL,
    manifest_json  TEXT NOT NULL,
    position       INTEGER NOT NULL,
    enabled        INTEGER NOT NULL DEFAULT 1,
    allow_private  INTEGER NOT NULL DEFAULT 0,
    hlc            TEXT NOT NULL,
    PRIMARY KEY (profile_id, transport_url)
);

CREATE TABLE library_items (
    profile_id    TEXT NOT NULL,
    item_id       TEXT NOT NULL,
    kind          TEXT NOT NULL,
    name          TEXT NOT NULL,
    poster        TEXT,
    poster_shape  TEXT,
    in_library    INTEGER NOT NULL DEFAULT 1,
    added_ms      INTEGER NOT NULL,
    hlc           TEXT NOT NULL,
    PRIMARY KEY (profile_id, item_id)
);

CREATE TABLE progress (
    profile_id   TEXT NOT NULL,
    video_id     TEXT NOT NULL,
    item_id      TEXT NOT NULL,
    position_ms  INTEGER NOT NULL,
    duration_ms  INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL,
    hlc          TEXT NOT NULL,
    PRIMARY KEY (profile_id, video_id)
);
CREATE INDEX progress_recent ON progress (profile_id, updated_ms DESC);

CREATE TABLE watched (
    profile_id  TEXT NOT NULL,
    video_id    TEXT NOT NULL,
    item_id     TEXT NOT NULL,
    watched     INTEGER NOT NULL,
    hlc         TEXT NOT NULL,
    PRIMARY KEY (profile_id, video_id)
);
CREATE INDEX watched_by_item ON watched (profile_id, item_id);

CREATE TABLE stream_memory (
    profile_id   TEXT NOT NULL,
    item_id      TEXT NOT NULL,
    binge_group  TEXT,
    stream_json  TEXT NOT NULL,
    hlc          TEXT NOT NULL,
    PRIMARY KEY (profile_id, item_id)
);

CREATE TABLE settings (
    profile_id  TEXT NOT NULL,
    key         TEXT NOT NULL,
    value_json  TEXT NOT NULL,
    hlc         TEXT NOT NULL,
    PRIMARY KEY (profile_id, key)
);

CREATE TABLE search_history (
    profile_id  TEXT NOT NULL,
    query       TEXT NOT NULL,
    used_ms     INTEGER NOT NULL,
    PRIMARY KEY (profile_id, query)
);

CREATE TABLE import_raw (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    source       TEXT NOT NULL,
    imported_ms  INTEGER NOT NULL,
    payload      TEXT NOT NULL
);
"#;

const V2: &str = r#"
CREATE TABLE http_cache (
    key                 TEXT PRIMARY KEY,
    body                BLOB NOT NULL,
    stored_ms           INTEGER NOT NULL,
    max_age_s           INTEGER NOT NULL,
    stale_revalidate_s  INTEGER NOT NULL,
    stale_error_s       INTEGER NOT NULL
);
CREATE INDEX http_cache_age ON http_cache (stored_ms);
"#;

/// Newest progress per title (Continue Watching, Library) without a scan.
const V3: &str = r#"
CREATE INDEX progress_by_item ON progress (profile_id, item_id, updated_ms);
"#;

/// Sync: the addon list is one register, so its HLC lives here and survives
/// an empty list; `sync_state` holds this device's cursors (never synced).
const V4: &str = r#"
CREATE TABLE addon_list (
    profile_id  TEXT PRIMARY KEY,
    hlc         TEXT NOT NULL
);
INSERT INTO addon_list (profile_id, hlc) SELECT profile_id, MAX(hlc) FROM addons GROUP BY profile_id;
CREATE TABLE sync_state (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
"#;

/// Sync outbox: every local register write queues its key in the same
/// transaction, so a push never misses a write that committed after a later
/// hlc did. Pulled writes are not queued; the first sync queues everything.
const V5: &str = r#"
CREATE TABLE sync_outbox (
    id    INTEGER PRIMARY KEY AUTOINCREMENT,
    coll  INTEGER NOT NULL,
    key   TEXT NOT NULL
);
"#;

/// The account roster as this device verified it, per account (keyed by the
/// genesis hash): signed entries in order. Changing relay or token keeps it.
const V6: &str = r#"
CREATE TABLE sync_roster (
    genesis  TEXT NOT NULL,
    seq      INTEGER NOT NULL,
    body     BLOB NOT NULL,
    sig      BLOB NOT NULL,
    PRIMARY KEY (genesis, seq)
);
"#;

/// Watch trackers (Trakt): the account connected per profile, writes waiting
/// to be sent (one per video, `held` while their playback session lives),
/// and cursors. Local to the device, never synced.
const V7: &str = r#"
CREATE TABLE tracker_account (
    profile_id   TEXT NOT NULL,
    tracker      TEXT NOT NULL,
    account      TEXT NOT NULL,
    generation   INTEGER NOT NULL,
    connected_ms INTEGER NOT NULL,
    PRIMARY KEY (profile_id, tracker)
);

CREATE TABLE tracker_outbox (
    profile_id  TEXT NOT NULL,
    tracker     TEXT NOT NULL,
    account     TEXT NOT NULL,
    video_id    TEXT NOT NULL,
    item_id     TEXT NOT NULL,
    rev         INTEGER NOT NULL,
    op          TEXT NOT NULL,
    watched_ms  INTEGER NOT NULL,
    session     TEXT,
    held        INTEGER NOT NULL,
    tries       INTEGER NOT NULL DEFAULT 0,
    next_ms     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, tracker, account, video_id)
);
CREATE INDEX tracker_outbox_session ON tracker_outbox (session) WHERE session IS NOT NULL;

CREATE TABLE tracker_state (
    profile_id  TEXT NOT NULL,
    tracker     TEXT NOT NULL,
    account     TEXT NOT NULL,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (profile_id, tracker, account, key)
);
"#;

/// Profiles (plans/m11-profiles.md): the cosmetic register gains an avatar
/// and a position; access (the PIN) and deletion are registers of their
/// own, so an edit to one never carries a stale copy of another. A profile
/// is usable once its lock register exists (`pin_hash` NULL: no PIN); a row
/// in `profile_gone` removes it for good. `pin_attempts` and `device_state`
/// (the active profile) stay on this device. The unused V1 columns go, so
/// nothing writes access into the cosmetic register. Raw imports must name
/// their profile, so its removal takes them too; older ones belong to the
/// only profile when there is one, and are dropped otherwise (no owner can
/// be told, and they can hold addon keys).
const V8: &str = r#"
CREATE TABLE import_raw_v8 (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id   TEXT NOT NULL,
    source       TEXT NOT NULL,
    imported_ms  INTEGER NOT NULL,
    payload      TEXT NOT NULL
);
INSERT INTO import_raw_v8 (id, profile_id, source, imported_ms, payload)
    SELECT r.id, p.id, r.source, r.imported_ms, r.payload FROM import_raw r, profiles p
    WHERE (SELECT COUNT(*) FROM profiles) = 1;
DROP TABLE import_raw;
ALTER TABLE import_raw_v8 RENAME TO import_raw;
ALTER TABLE profiles DROP COLUMN kids;
ALTER TABLE profiles DROP COLUMN pin_hash;
ALTER TABLE profiles ADD COLUMN avatar TEXT NOT NULL DEFAULT '';
ALTER TABLE profiles ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

CREATE TABLE profile_lock (
    profile_id  TEXT PRIMARY KEY,
    pin_hash    TEXT,
    hlc         TEXT NOT NULL
);

CREATE TABLE profile_gone (
    profile_id  TEXT PRIMARY KEY,
    hlc         TEXT NOT NULL
);

CREATE TABLE pin_attempts (
    profile_id  TEXT PRIMARY KEY,
    failures    INTEGER NOT NULL
);

CREATE TABLE device_state (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
"#;

// The owner was named after its id. Never renamed (still the zero hlc), it
// takes the name people see; the hlc stays zero, so any real rename synced
// from another device still wins.
const V9: &str = r#"
UPDATE profiles SET name = 'Main'
    WHERE id = 'default' AND name = 'default' AND substr(hlc, 1, 16) = '0000000000000000';
"#;

/// Kids mode (plans/m11-kids.md 3.1): the kids flag is its own register
/// (coll 9) and the allowlist one register per title (coll 10). Every
/// profile here gets a standard access register at the all-zero hlc, queued,
/// so another device never shows it as still syncing; any real record wins.
/// `access_generation` (device-wide) rises with every change to either
/// table, inside the transaction that makes it, so a policy decision and the
/// generation read with it name the same snapshot.
const V10: &str = r#"
CREATE TABLE profile_access (
    profile_id  TEXT PRIMARY KEY,
    kids        INTEGER NOT NULL,
    hlc         TEXT NOT NULL
);

CREATE TABLE allowed (
    profile_id  TEXT NOT NULL,
    item_id     TEXT NOT NULL,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    names       TEXT NOT NULL DEFAULT '[]',
    poster      TEXT,
    allowed     INTEGER NOT NULL,
    added_ms    INTEGER NOT NULL,
    hlc         TEXT NOT NULL,
    PRIMARY KEY (profile_id, item_id)
);
CREATE INDEX allowed_counted ON allowed (profile_id, allowed, added_ms);

INSERT INTO profile_access (profile_id, kids, hlc)
    SELECT id, 0, '00000000000000000000000000000000' FROM profiles
    WHERE id NOT IN (SELECT profile_id FROM profile_gone);
INSERT INTO sync_outbox (coll, key) SELECT 9, profile_id FROM profile_access;

INSERT INTO device_state (key, value) VALUES ('access_generation', '0')
    ON CONFLICT(key) DO NOTHING;
CREATE TRIGGER access_gen_access_insert AFTER INSERT ON profile_access BEGIN
    UPDATE device_state SET value = CAST(value AS INTEGER) + 1 WHERE key = 'access_generation';
END;
CREATE TRIGGER access_gen_access_update AFTER UPDATE ON profile_access BEGIN
    UPDATE device_state SET value = CAST(value AS INTEGER) + 1 WHERE key = 'access_generation';
END;
CREATE TRIGGER access_gen_access_delete AFTER DELETE ON profile_access BEGIN
    UPDATE device_state SET value = CAST(value AS INTEGER) + 1 WHERE key = 'access_generation';
END;
CREATE TRIGGER access_gen_allowed_insert AFTER INSERT ON allowed BEGIN
    UPDATE device_state SET value = CAST(value AS INTEGER) + 1 WHERE key = 'access_generation';
END;
CREATE TRIGGER access_gen_allowed_update AFTER UPDATE ON allowed BEGIN
    UPDATE device_state SET value = CAST(value AS INTEGER) + 1 WHERE key = 'access_generation';
END;
CREATE TRIGGER access_gen_allowed_delete AFTER DELETE ON allowed BEGIN
    UPDATE device_state SET value = CAST(value AS INTEGER) + 1 WHERE key = 'access_generation';
END;
"#;

#[cfg(test)]
mod tests {
    use super::migrations;
    use rusqlite::Connection;

    /// V10 on a V9 database: every profile still here is standard at the
    /// all-zero hlc and queued; a gone one gets nothing; the generation
    /// starts and rises with access changes.
    #[test]
    fn v10_makes_every_profile_standard_and_queues_it() {
        let mut c = Connection::open_in_memory().unwrap();
        migrations().to_version(&mut c, 9).unwrap();
        c.execute_batch(
            "INSERT INTO profiles (id, name, hlc) VALUES
               ('default', 'Main', '0000000000000000000000000000000a'),
               ('kid', 'Kid', '01a14b5de9630000000000000000000a'),
               ('x', 'X', '01a14b5de9630000000000000000000a');
             INSERT INTO profile_gone (profile_id, hlc) VALUES ('x', '01a14b5de9640000000000000000000a');",
        )
        .unwrap();
        migrations().to_latest(&mut c).unwrap();
        let rows: Vec<(String, bool, String)> = c
            .prepare("SELECT profile_id, kids, hlc FROM profile_access ORDER BY profile_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let zero = "0".repeat(32);
        assert_eq!(
            rows,
            [
                ("default".to_owned(), false, zero.clone()),
                ("kid".to_owned(), false, zero)
            ]
        );
        let queued: Vec<String> = c
            .prepare("SELECT key FROM sync_outbox WHERE coll = 9 ORDER BY key")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(queued, ["default", "kid"]);
        let generation = |c: &Connection| -> i64 {
            c.query_row(
                "SELECT CAST(value AS INTEGER) FROM device_state WHERE key = 'access_generation'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        let before = generation(&c);
        c.execute(
            "INSERT INTO allowed (profile_id, item_id, kind, name, allowed, added_ms, hlc)
             VALUES ('kid', 'tt1', 'movie', 'One', 1, 1, '1')",
            [],
        )
        .unwrap();
        assert!(generation(&c) > before);
    }

    /// V9 names a never-renamed owner and leaves a deliberate name alone.
    #[test]
    fn the_owner_takes_its_name_only_while_never_renamed() {
        for (hlc, want) in [
            ("0000000000000000000000000000000a", "Main"),
            ("01a14b5de9630000000000000000000a", "default"),
        ] {
            let mut c = Connection::open_in_memory().unwrap();
            migrations().to_version(&mut c, 8).unwrap();
            c.execute(
                "INSERT INTO profiles (id, name, hlc) VALUES ('default', 'default', ?1)",
                [hlc],
            )
            .unwrap();
            migrations().to_latest(&mut c).unwrap();
            let (name, kept): (String, String) = c
                .query_row(
                    "SELECT name, hlc FROM profiles WHERE id = 'default'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(name, want, "{hlc}");
            assert_eq!(kept, hlc, "the hlc never moves");
        }
    }
}
