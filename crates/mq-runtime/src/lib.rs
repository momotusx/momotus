//! Core runtime: owns the shared services and exposes live views to UIs.
//!
//! Each view is a typed object. Records are delivered as whole snapshots,
//! lists as index-based diffs coalesced to one batch per frame.

mod core;
mod debrid;
pub mod defaults;
pub mod learn;
pub mod live;
mod profiles;
pub mod segments;
pub mod speed;
mod trakt;
mod turkcealtyazi;
pub mod views;

pub use crate::core::{Core, CoreConfig, CoreError, TvdbMiss};
pub use crate::debrid::{
    AccountState, DebridAccount, DebridCredential, DebridLoginSnapshot, DebridLoginView,
    DebridRecord, DebridSnapshot, DebridTombstone, DebridView, RestoreAnswer,
};
pub use crate::profiles::{MAX_PROFILES, ProfileError};
pub use crate::trakt::{
    TraktConfig, TraktImporting, TraktLoginSnapshot, TraktLoginView, TraktPlayback, TraktReport,
    TraktState, TraktStatus, TraktUnmapped, TraktView,
};
pub use mq_debrid::StoreId;
pub use mq_import::Endpoints as StremioEndpoints;
pub use mq_tracking::trakt::{Hosts as TraktHosts, TraktRecord};
pub use views::addon_catalog::{
    AddonCatalogSnapshot, AddonCatalogView, AddonList, CINEMETA, Configuration, Installed,
    ListState, ListedAddon, ListedEntry, ListedError, ListedFailure, carries_token,
    link_carries_token,
};
pub use views::addons::{AddonInfo, AddonsSnapshot, AddonsView};
pub use views::calendar::{
    CalendarDay, CalendarItem, CalendarSeries, CalendarSnapshot, CalendarView,
};
pub use views::discover::{CatalogChoice, DiscoverSnapshot, DiscoverState, DiscoverView};
pub use views::home::{HomeCatalog, HomeRow, HomeUpdate, HomeView, RowState, Tile};
pub use views::import::{ImportSnapshot, ImportSummary, ImportView};
pub use views::kids::{
    KidsKeys, KidsKeysSnapshot, KidsKeysView, KidsTitle, KidsTitlesSnapshot, KidsTitlesView,
};
pub use views::library::{KindCount, LibrarySnapshot, LibrarySort, LibraryView};
pub use views::pairing::{PairingSnapshot, PairingView};
pub use views::playback::{
    Attempt, NextEpisode, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView, SubtitleNote,
    SubtitleProblem, SubtitleTrack,
};
pub use views::profiles::{ProfileInfo, ProfilesSnapshot, ProfilesView, SessionState};
pub use views::search::{SearchSnapshot, SearchView};
pub use views::settings::{HomeRows, Prefs, SettingsView};
pub use views::title::{Episode, TitleHeader, TitleUpdate, TitleView};
pub use views::versions::{
    BlockedReason, BlockedVersions, Pick, PickHandle, TitleFacts, VersionRow, VersionsConditions,
    VersionsParams, VersionsSnapshot, VersionsView,
};
