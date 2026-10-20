//! End-to-end encrypted sync of library registers through a blind relay.

mod engine;
pub mod identity;
pub mod pair;
pub mod roster;
pub mod seal;
pub mod wire;

pub use engine::{
    Blocked, DeviceKits, Devices, ForkPreview, LostPreview, MemberInfo, PAIRING_MS, SyncConfig,
    SyncError, SyncReport, create_account, create_account_with, delete_account, devices,
    fork_preview, leave_here, lost_preview, pull_changes, push_changes, remove_devices,
    report_lost, sign_out, sync_once, use_server_roster,
};
