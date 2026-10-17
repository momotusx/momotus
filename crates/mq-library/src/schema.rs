use rusqlite_migration::{M, Migrations};

pub(crate) fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(V1), M::up(V2)])
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
