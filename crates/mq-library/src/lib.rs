//! Local library: SQLite in WAL mode, one writer connection, a small pool of
//! readers, and table-level change notifications for live views.
//!
//! Storage units match the sync registers: progress is one atomic row per
//! (profile, video), watched is one boolean per (profile, video). Every synced
//! row carries an `hlc` string that sorts in causal order.

pub mod hlc;
mod policy;
mod profiles;
mod schema;
mod store;
mod sync;
mod tracker;

pub use hlc::{Hlc, HlcClock};

pub use policy::{AllowedList, AllowedRow, Authorization, MAX_ALLOWED, Policy};
pub use profiles::ProfileRow;
pub use store::{
    AddonRow, CachedResponse, ContinueItem, FINISHED_RATIO, Library, LibraryEntry, LibraryError,
    LibraryItemRow, ProgressRow, Table, sweep_temporaries,
};
pub use sync::{
    OWNER, OWNER_NAME, OutboxPage, SignedEntry, Stamp, SyncCursor, SyncRecord, coll, valid_avatar,
};
pub use tracker::{
    ImportReport, ImportedPlay, ImportedResume, Intent, IntentOp, NewIntent, TrackerKey,
};
