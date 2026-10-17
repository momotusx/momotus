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
