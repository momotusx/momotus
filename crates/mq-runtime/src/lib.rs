//! Core runtime: owns the shared services and exposes live views to UIs.
//!
//! Each view is a typed object. Records are delivered as whole snapshots,
//! lists as index-based diffs coalesced to one batch per frame.

#[doc(hidden)]
pub mod archive;
mod core;
mod covers;
mod debrid;
pub mod defaults;
pub mod downloads;
pub mod languages;
pub mod learn;
pub mod live;
pub mod plugins;
mod profiles;
pub mod ratings;
pub mod segments;
mod services;
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
    TmdbHost, TraktConfig, TraktImporting, TraktLoginSnapshot, TraktLoginView, TraktPlayback,
    TraktReport, TraktState, TraktStatus, TraktUnmapped, TraktView,
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
pub use views::discover::{
    CatalogChoice, DiscoverChoice, DiscoverSnapshot, DiscoverState, DiscoverView, GenreChoice,
    Platform, Scope, SortChoice,
};
pub use views::downloads::{
    DownloadActivity, DownloadStorage, DownloadTitle, DownloadsSnapshot, DownloadsView,
};
pub use views::genres::{GenreTile, GenresSnapshot, GenresView, KindGenres};
pub use views::home::{HomeCatalog, HomeRow, HomeUpdate, HomeView, RowState, Tile};
pub use views::import::{ImportSnapshot, ImportSummary, ImportView};
pub use views::kids::{
    KidsKeys, KidsKeysSnapshot, KidsKeysView, KidsTitle, KidsTitlesSnapshot, KidsTitlesView,
};
pub use views::library::{KindCount, LibrarySnapshot, LibrarySort, LibraryView};
pub use views::pairing::{PairingSnapshot, PairingView};
pub use views::playback::{
    Attempt, NextEpisode, PlayTarget, PlaybackPhase, PlaybackSnapshot, PlaybackView, StartProgress,
    StartStage, SubtitleNote, SubtitleProblem, SubtitleTrack,
};
pub use views::profiles::{ProfileInfo, ProfilesSnapshot, ProfilesView, SessionState, WipePending};
pub use views::search::{SearchSnapshot, SearchView};
pub use views::settings::{DiscoverPrefs, HomeRows, Prefs, SettingsView};
pub use views::sign_in::{SignInSnapshot, SignInView};
pub use views::title::{Episode, TitleHeader, TitleUpdate, TitleView};
pub use views::versions::{
    BlockedReason, BlockedVersions, EmptyAction, EmptyKind, EmptyVersions, Pick, PickHandle,
    RowOrigin, SectionQuality, SourceRows, TitleFacts, VersionRow, VersionSection, VersionStatus,
    VersionsConditions, VersionsParams, VersionsSnapshot, VersionsView,
};
