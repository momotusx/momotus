//! Core runtime: owns the shared services and exposes live views to UIs.
//!
//! Each view is a typed object. Records are delivered as whole snapshots,
//! lists as index-based diffs coalesced to one batch per frame.

mod core;
pub mod live;
pub mod views;

pub use crate::core::{Core, CoreConfig, CoreError};
pub use views::home::{HomeRow, HomeUpdate, HomeView, RowState, Tile};
