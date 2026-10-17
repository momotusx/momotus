//! Local library: SQLite in WAL mode, one writer connection, a small pool of
//! readers, and table-level change notifications for live views.
//!
//! Storage units match the sync registers: progress is one atomic row per
//! (profile, video), watched is one boolean per (profile, video). Every synced
//! row carries an `hlc` string that sorts in causal order.

pub mod hlc;
mod schema;
mod store;

pub use hlc::{Hlc, HlcClock};

pub use store::{
    AddonRow, ContinueItem, Library, LibraryError, LibraryItemRow, ProgressRow, Table,
};
