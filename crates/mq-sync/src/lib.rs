//! End-to-end encrypted sync of library registers through a blind relay.

mod engine;
pub mod pair;
pub mod seal;
pub mod wire;

pub use engine::{
    SyncConfig, SyncError, SyncReport, add_device, create_account, pull_changes, push_changes,
    sync_once,
};
