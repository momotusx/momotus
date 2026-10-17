//! Core runtime: owns the shared services and exposes live views to UIs.
//!
//! Each view is a typed object. Records are delivered as whole snapshots,
//! lists as index-based diffs coalesced to one batch per frame.

mod core;
pub mod live;
pub mod views;

pub use crate::core::{Core, CoreConfig, CoreError};
pub use mq_debrid::StoreId;
pub use views::home::{HomeRow, HomeUpdate, HomeView, RowState, Tile};
pub use views::playback::{Attempt, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView};
pub use views::title::{Episode, TitleHeader, TitleUpdate, TitleView};
pub use views::versions::{VersionRow, VersionsParams, VersionsSnapshot, VersionsView};
