//! Core runtime: owns the shared services and exposes live views to UIs.
//!
//! Each view is a typed object. Records are delivered as whole snapshots,
//! lists as index-based diffs coalesced to one batch per frame.

mod core;
pub mod live;
pub mod views;

pub use crate::core::{Core, CoreConfig, CoreError};
pub use mq_debrid::StoreId;
pub use mq_import::Endpoints as StremioEndpoints;
pub use views::addons::{AddonInfo, AddonsSnapshot, AddonsView};
pub use views::home::{HomeRow, HomeUpdate, HomeView, RowState, Tile};
pub use views::import::{ImportSnapshot, ImportSummary, ImportView};
pub use views::library::{KindCount, LibrarySnapshot, LibrarySort, LibraryView};
pub use views::playback::{
    Attempt, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView, SubtitleTrack,
};
pub use views::search::{SearchSnapshot, SearchView};
pub use views::title::{Episode, TitleHeader, TitleUpdate, TitleView};
pub use views::versions::{VersionRow, VersionsParams, VersionsSnapshot, VersionsView};
