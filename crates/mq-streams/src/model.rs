//! Inputs and outputs of stream ranking.

use mq_protocol::Source;
use mq_relparse::{Audio, Hdr, ParsedRelease, Resolution, VideoCodec};
use serde::Serialize;

/// What this device can decode and show. Produced by each platform.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceCaps {
    /// Vertical resolution of the output (1080 on a 1080p TV stick).
    pub display_height: u32,
    /// HDR formats the display chain can show natively.
    pub display_hdr: Vec<Hdr>,
    /// Codecs with a decoder that plays them smoothly at `display_height`.
    pub decoders: Vec<VideoCodec>,
    pub ten_bit_hevc: bool,
    /// Formats the audio output accepts as passthrough.
    pub passthrough: Vec<Audio>,
    /// A second engine (libmpv) exists for what the primary cannot play.
    pub has_fallback_engine: bool,
}

impl DeviceCaps {
    /// A conservative default for a 1080p SDR stick with no passthrough.
    pub fn basic_1080p() -> Self {
        Self {
            display_height: 1080,
            display_hdr: vec![],
            decoders: vec![VideoCodec::H264, VideoCodec::H265],
            ten_bit_hevc: true,
            passthrough: vec![],
            has_fallback_engine: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum QualityPref {
    #[default]
    Best,
    Balanced,
    DataSaver,
}

/// The user's profile preferences for picking streams.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StreamPrefs {
    pub quality: QualityPref,
    pub max_resolution: Option<Resolution>,
    pub max_size_bytes: Option<u64>,
    /// ISO 639-1 audio language preferences, most wanted first.
    pub languages: Vec<String>,
    /// Lowercase terms that hide a stream when its name contains them.
    pub avoid: Vec<String>,
}

/// What the user wants to watch, for wrong-content detection.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Target {
    pub title: String,
    pub year: Option<u16>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

/// One stream after normalization: addon hints merged with parsed facts.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedStream {
    /// Identity used for dedupe and as the UI row key.
    pub key: String,
    pub addon_id: String,
    pub addon_name: String,
    pub addon_rank: u16,
    pub index: u32,
    pub source: Source,
    pub release: ParsedRelease,
    pub size_bytes: Option<u64>,
    pub seeders: Option<u32>,
    pub cached: Option<bool>,
    pub service: Option<String>,
    pub source_site: Option<String>,
    pub binge_group: Option<String>,
    pub filename: Option<String>,
    /// Request headers the origin needs (`proxyHeaders.request`).
    #[serde(skip)]
    pub headers: Vec<(String, String)>,
    /// Subtitles the stream itself carries (`subtitles` on the stream object).
    #[serde(skip)]
    pub subtitles: Vec<mq_protocol::WireSubtitle>,
    pub raw_name: String,
    pub raw_description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Playability {
    NeedsTranscode,
    NeedsFallbackEngine,
    Native,
}

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    ReadyNow,
    PlaysDirectly,
    MatchesScreen,
    SameReleaseAsLast,
    YourLanguage,
    SmallerFaster,
}

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    Best,
    SmallerFaster,
    OtherLanguages,
    NeedsDownload,
}

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HideReason {
    WrongTitle,
    WrongEpisode,
    Excluded,
    TooLarge,
    Unplayable,
    UnsupportedSource,
}

/// Lexicographic sort key: earlier fields always win, so no within-tier
/// bonus can lift a stream past a better tier. Higher is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankKey {
    pub tier: u8,
    pub binge: bool,
    pub playability: Playability,
    pub effective_height: u32,
    pub within: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedRow {
    pub stream: ParsedStream,
    pub key: RankKey,
    pub reasons: Vec<Reason>,
    pub group: Group,
    /// Same release from other addons or services, tried in order on failure.
    /// Whole streams, since two addons can return the same torrent key.
    pub failover: Vec<ParsedStream>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HiddenRow {
    pub stream: ParsedStream,
    pub reason: HideReason,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ranked {
    pub rows: Vec<RankedRow>,
    pub hidden: Vec<HiddenRow>,
}

impl Ranked {
    pub fn best(&self) -> Option<&RankedRow> {
        self.rows.first()
    }
}
