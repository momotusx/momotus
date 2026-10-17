//! End-to-end encrypted sync of library registers through a blind relay.

mod engine;
pub mod pair;
pub mod roster;
pub mod seal;
pub mod wire;

pub use engine::{
    Devices, LostPreview, MemberInfo, PAIRING_MS, SyncConfig, SyncError, SyncReport,
    create_account, devices, lost_preview, pull_changes, push_changes, remove_devices, report_lost,
    sign_out, sync_once, use_server_roster,
};
