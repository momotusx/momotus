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
        M::up(V11),
        M::up(V12),
        M::up_with_hook(V13, hash_addon_links),
        M::up(V14),
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

/// Titles taken off Continue Watching and Next Up (journey reviews
/// 2026-10-20): one register per title (coll 11), its hlc the moment of the
/// removal. A progress register of the title newer than it shows the title
/// again.
const V11: &str = r#"
CREATE TABLE dismissed (
    profile_id  TEXT NOT NULL,
    item_id     TEXT NOT NULL,
    hlc         TEXT NOT NULL,
    PRIMARY KEY (profile_id, item_id)
);
"#;

/// IMDb and Rotten Tomatoes scores looked up for titles (MDBList,
/// plans/research/ratings.md): one row per IMDb id and MDBList kind
/// ("movie", "show"), device-wide. A null score is one the source did not
/// have; `fetched_ms` dates the answer, which sets when it is asked again.
const V12: &str = r#"
CREATE TABLE ratings (
    imdb_id     TEXT NOT NULL,
    kind        TEXT NOT NULL,
    imdb        REAL,
    rt          INTEGER,
    fetched_ms  INTEGER NOT NULL,
    PRIMARY KEY (imdb_id, kind)
);
CREATE INDEX ratings_age ON ratings (fetched_ms);
"#;

/// The addon list merges per addon (plans/accounts-social-sync.md 13).
/// Each addon becomes two registers keyed by a hash of its link: its state
/// in `addons` (a removal kept as a tombstone with neither link nor
/// manifest, `installed` 0) and its place in `addon_order`, with gaps
/// (`POSITION_STEP`). The table is rebuilt for its new key; the hook fills
/// the hashes (`hash_addon_links`), then queues every addon as its two
/// registers. Whole lists (coll 1) are no longer sent. `addon_list` stays,
/// unsynced, as the mark that a profile had a list.
const V13: &str = r#"
CREATE TABLE addons_v13 (
    profile_id     TEXT NOT NULL,
    url_hash       TEXT NOT NULL,
    transport_url  TEXT NOT NULL,
    manifest_json  TEXT NOT NULL,
    enabled        INTEGER NOT NULL DEFAULT 1,
    allow_private  INTEGER NOT NULL DEFAULT 0,
    installed      INTEGER NOT NULL DEFAULT 1,
    hlc            TEXT NOT NULL,
    PRIMARY KEY (profile_id, url_hash)
);
INSERT INTO addons_v13 (profile_id, url_hash, transport_url, manifest_json, enabled, allow_private, hlc)
    SELECT profile_id, transport_url, transport_url, manifest_json, enabled, allow_private, hlc FROM addons;
CREATE TABLE addon_order (
    profile_id  TEXT NOT NULL,
    url_hash    TEXT NOT NULL,
    position    INTEGER NOT NULL,
    hlc         TEXT NOT NULL,
    PRIMARY KEY (profile_id, url_hash)
);
INSERT INTO addon_order (profile_id, url_hash, position, hlc)
    SELECT profile_id, transport_url,
           4294967296 * ROW_NUMBER() OVER (PARTITION BY profile_id ORDER BY position, transport_url), hlc
    FROM addons;
DROP TABLE addons;
ALTER TABLE addons_v13 RENAME TO addons;
DELETE FROM sync_outbox WHERE coll = 1;
"#;

/// V13's hook: each link's hash in place of the link the SQL put in its
/// key, in both tables, then every addon queued as its two registers.
fn hash_addon_links(tx: &rusqlite::Transaction) -> rusqlite_migration::HookResult {
    let links: Vec<(String, String)> = tx
        .prepare("SELECT profile_id, transport_url FROM addons")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (profile, url) in &links {
        let hash = crate::sync::url_hash(url);
        for table in ["addons", "addon_order"] {
            tx.execute(
                &format!(
                    "UPDATE {table} SET url_hash = ?3 WHERE profile_id = ?1 AND url_hash = ?2"
                ),
                rusqlite::params![profile, url, hash],
            )?;
        }
    }
    for coll in [12, 13] {
        tx.execute(
            &format!(
                "INSERT INTO sync_outbox (coll, key) SELECT {coll}, profile_id || char(31) || url_hash FROM {}",
                if coll == 12 { "addons" } else { "addon_order" }
            ),
            [],
        )?;
    }
    Ok(())
}

/// The looked-up scores go (2026-10-23): ratings come from the addons
/// alone, the IMDb score from the meta addon's own rating and the ratings
/// band from RPDB's covers, so nothing is looked up or kept.
const V14: &str = r#"
DROP INDEX ratings_age;
DROP TABLE ratings;
"#;

#[cfg(test)]
mod tests {
    use super::migrations;
    use rusqlite::Connection;

    /// V14 on a V13 database with looked-up scores: the ratings table and
    /// its index go, and every other table keeps its schema and its rows.
    #[test]
    fn v14_drops_the_ratings_cache_and_touches_nothing_else() {
        let mut c = Connection::open_in_memory().unwrap();
        migrations().to_version(&mut c, 13).unwrap();
        c.execute_batch(
            "INSERT INTO profiles (id, name, hlc) VALUES ('default', 'Main', '0000000000000000000000000000000a');
             INSERT INTO library_items (profile_id, item_id, kind, name, added_ms, hlc)
               VALUES ('default', 'tt1', 'movie', 'One', 1, '1');
             INSERT INTO ratings (imdb_id, kind, imdb, rt, fetched_ms) VALUES ('tt1', 'movie', 8.8, 87, 1);",
        )
        .unwrap();
        let schema = |c: &Connection| -> Vec<(String, String, Option<String>)> {
            c.prepare(
                "SELECT type, name, sql FROM sqlite_master
                 WHERE tbl_name <> 'ratings' ORDER BY type, name",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
        };
        let rows = |c: &Connection| -> Vec<String> {
            let mut all = Vec::new();
            for (kind, table, _) in schema(c) {
                if kind != "table" || table.starts_with("sqlite_") {
                    continue;
                }
                let mut stmt = c.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
                let n = stmt.column_count();
                let found = stmt
                    .query_map([], |r| {
                        let cells: Vec<String> = (0..n)
                            .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                            .collect();
                        Ok(format!("{table}: {}", cells.join(", ")))
                    })
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                all.extend(found);
            }
            all
        };
        let (schema_before, rows_before) = (schema(&c), rows(&c));
        assert!(rows_before.iter().any(|r| r.starts_with("library_items: ")));
        migrations().to_latest(&mut c).unwrap();
        let left: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE tbl_name = 'ratings'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "the ratings table and its index are gone");
        assert_eq!(schema(&c), schema_before);
        assert_eq!(rows(&c), rows_before);
    }

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

    /// V13 on a V12 database: each addon keeps its hlc as its state
    /// register and gets its place, with gaps, as an order register; every
    /// addon is queued as both; whole lists are no longer queued; the list
    /// mark stays.
    #[test]
    fn v13_makes_each_addon_two_registers() {
        let mut c = Connection::open_in_memory().unwrap();
        migrations().to_version(&mut c, 12).unwrap();
        c.execute_batch(
            "INSERT INTO addons (profile_id, transport_url, manifest_json, position, hlc) VALUES
               ('p', 'https://b', '{}', 0, 'h1'), ('p', 'https://a', '{}', 1, 'h1'), ('q', 'https://c', '{}', 0, 'h2');
             INSERT INTO addon_list (profile_id, hlc) VALUES ('p', 'h1'), ('q', 'h2'), ('emptied', 'h3');
             INSERT INTO sync_outbox (coll, key) VALUES (1, 'p'), (2, 'p' || char(31) || 'tt1');",
        )
        .unwrap();
        migrations().to_latest(&mut c).unwrap();
        let hashed: Vec<(String, String)> = c
            .prepare("SELECT transport_url, url_hash FROM addons")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            hashed.iter().all(|(u, h)| *h == crate::sync::url_hash(u)),
            "{hashed:?}"
        );
        let rows: Vec<(String, String, i64, String)> = c
            .prepare(
                "SELECT a.profile_id, a.transport_url, o.position, a.hlc FROM addons a
                 JOIN addon_order o USING (profile_id, url_hash)
                 WHERE a.installed = 1 AND o.hlc = a.hlc
                 ORDER BY a.profile_id, o.position",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let row = |p: &str, u: &str, pos, h: &str| (p.to_owned(), u.to_owned(), pos, h.to_owned());
        assert_eq!(
            rows,
            [
                row("p", "https://b", 1 << 32, "h1"),
                row("p", "https://a", 2 << 32, "h1"),
                row("q", "https://c", 1 << 32, "h2")
            ]
        );
        let positions: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('addons') WHERE name IN ('position', 'url')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(positions, 0, "places live in addon_order only");
        let queued: Vec<(i64, String)> = c
            .prepare("SELECT coll, key FROM sync_outbox ORDER BY coll, key")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let k = |p: &str, u: &str| format!("{p}\u{1f}{u}");
        let mut want = vec![(2, k("p", "tt1"))];
        for coll in [12, 13] {
            for (p, u) in [("p", "https://a"), ("p", "https://b"), ("q", "https://c")] {
                want.push((coll, k(p, &crate::sync::url_hash(u))));
            }
        }
        want.sort();
        assert_eq!(queued, want);
        let marks: i64 = c
            .query_row("SELECT COUNT(*) FROM addon_list", [], |r| r.get(0))
            .unwrap();
        assert_eq!(marks, 3, "the list marks stay, an emptied list's too");
    }
}
