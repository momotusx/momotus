//! Local library: SQLite in WAL mode, one writer connection, a small pool of
//! readers, and table-level change notifications for live views.
//!
//! Storage units match the sync registers: progress is one atomic row per
//! (profile, video), watched is one boolean per (profile, video). Every synced
//! row carries an `hlc` string that sorts in causal order.

mod archive;
mod downloads;
pub mod hlc;
mod plugins;
mod policy;
mod profiles;
mod schema;
mod store;
mod sync;
mod tracker;

pub use archive::ArchiveFilm;
pub use downloads::DownloadRecord;
pub use hlc::{Hlc, HlcClock};

pub use plugins::{MAX_REPOS, PluginCode, PluginRepoRow};
pub use policy::{AllowedList, AllowedRow, Authorization, MAX_ALLOWED, Policy};
pub use profiles::{DEFAULTS_GIVEN, ProfileRow};
pub use store::{
    AddonRow, CachedResponse, ContinueItem, FINISHED_RATIO, IMDB_ID_ROWS, Library, LibraryEntry,
    LibraryError, LibraryItemRow, ProgressRow, Table, sweep_temporaries,
};
pub use sync::{
    DEFAULTS_ERA, OWNER, OWNER_NAME, OutboxPage, SignedEntry, Stamp, SyncCursor, SyncRecord, coll,
    valid_avatar,
};
pub use tracker::{
    ImportReport, ImportedPlay, ImportedResume, Intent, IntentOp, NewIntent, TrackerKey,
};
