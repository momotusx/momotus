//! Learning from the user's picks (plans/m13-stream-choice.md 3.9): one
//! counter per kind of choice and screen class, synced with the profile.

use mq_streams::StreamPrefs;
use serde::{Deserialize, Serialize};

/// What a pick that differs from the recommendation says the user wants.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LearnedChoice {
    /// A file ready on the streaming service over a torrent.
    Ready,
    /// More lines.
    Height,
    /// HDR the screen shows.
    Hdr,
    /// A bigger file.
    Size,
}

/// The counters of one screen class, each in -5..5. A counter stamped with
/// an older epoch than the profile's reads as zero.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Learned {
    pub ready: i8,
    pub height: i8,
    pub hdr: i8,
    pub size: i8,
    /// Picks counted in this class.
    pub picks: u32,
    pub epoch: u32,
    /// The last move, for the decay of 1 per 60 days.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub at_ms: u64,
    /// Choices the user undid: they no longer learn.
    pub muted: Vec<LearnedChoice>,
}

/// Counters by screen class (3.3): 2160, 1080, 720 and 480 lines. Each is
/// its own synced leaf (`learned.uhd`), so two devices learning on
/// different screens both keep their counts.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LearnedByClass {
    pub uhd: Learned,
    pub fhd: Learned,
    pub hd: Learned,
    pub sd: Learned,
}

/// The one-time note when a counter crosses 3: what changed, for which
/// screen class, and the preferences now in effect there. Undo mutes it.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedNote {
    pub screen_class: u32,
    pub choice: LearnedChoice,
    /// Toward more (bigger files, more lines, ready files, HDR).
    pub up: bool,
    pub prefs: StreamPrefs,
}
