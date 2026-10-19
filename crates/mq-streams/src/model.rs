//! Inputs and outputs of stream ranking.

use mq_protocol::Source;
use mq_relparse::{Audio, Hdr, ParsedRelease, VideoCodec};
use serde::{Deserialize, Serialize};

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
    /// Audio formats the primary engine decodes itself.
    pub audio_decoders: Vec<Audio>,
    /// Containers the primary engine opens, as lowercase file extensions.
    pub containers: Vec<String>,
    /// A second engine (libmpv) exists for what the primary cannot play.
    pub has_fallback_engine: bool,
}

impl DeviceCaps {
    /// A conservative default for a 1080p SDR stick with no passthrough:
    /// common audio decodes, DTS and TrueHD go to the fallback engine.
    pub fn basic_1080p() -> Self {
        Self {
            display_height: 1080,
            display_hdr: vec![],
            decoders: vec![VideoCodec::H264, VideoCodec::H265],
            ten_bit_hevc: true,
            passthrough: vec![],
            audio_decoders: vec![
                Audio::Aac,
                Audio::Ac3,
                Audio::Eac3,
                Audio::Mp3,
                Audio::Opus,
                Audio::Flac,
                Audio::Pcm,
            ],
            containers: ["mkv", "mp4", "m4v", "webm", "ts", "m2ts", "mov", "avi"]
                .map(str::to_owned)
                .to_vec(),
            has_fallback_engine: true,
        }
    }
}

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum QualityPref {
    #[default]
    Best,
    Balanced,
    DataSaver,
}

/// The soft size limit a user starts with: 5 GB as addons print sizes
/// (base 1024).
pub const DEFAULT_SIZE_LIMIT: u64 = 5 << 30;

/// The user's profile preferences for picking streams.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StreamPrefs {
    /// Read once and mapped to the fields below (plans/m13-stream-choice.md
    /// 4.1); the ranking does not read it.
    pub quality: QualityPref,
    /// Never pick above this many lines (720, 1080, 2160), whatever the screen.
    pub max_height: Option<u32>,
    /// The old hard limit: larger streams are hidden. No UI writes it.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub max_size_bytes: Option<u64>,
    /// ISO 639-1 audio language preferences, most wanted first.
    pub languages: Vec<String>,
    /// Lowercase terms that hide a stream when its name contains them.
    pub avoid: Vec<String>,
    /// The soft limit: a torrent above 1.1 x this ranks below the ones that
    /// fit. `None` is no limit.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub size_limit_bytes: Option<u64>,
    /// Seeders that make a torrent start well; 4 x this starts quickly.
    pub min_seeders: u32,
    /// Prefer HDR where the screen shows it.
    pub hdr: bool,
    /// A file ready on a streaming service starts before every torrent.
    pub prefer_ready: bool,
    /// Learn from the user's picks (applied by the runtime).
    pub learn: bool,
}

impl Default for StreamPrefs {
    fn default() -> Self {
        Self {
            quality: QualityPref::default(),
            max_height: None,
            max_size_bytes: None,
            languages: Vec::new(),
            avoid: Vec::new(),
            size_limit_bytes: Some(DEFAULT_SIZE_LIMIT),
            min_seeders: 50,
            hdr: true,
            prefer_ready: true,
            learn: true,
        }
    }
}

/// A connection's speed for one kind of source, in Mbit/s, and how many
/// playbacks measured it (0: the prior).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Speed {
    pub mbps: f32,
    pub samples: u32,
}

/// What the ranking knows beyond the device and the user
/// (plans/m13-stream-choice.md 3.1). The default knows nothing: the speed
/// priors, no engine, no account.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conditions {
    pub runtime_s: Option<u32>,
    pub title_year: Option<u16>,
    /// The episode's air date's year.
    pub episode_year: Option<u16>,
    pub episodes_in_season: Option<u32>,
    /// Debrid and direct links.
    pub http: Speed,
    /// The torrent engine.
    pub p2p: Speed,
    pub free_bytes: Option<u64>,
    /// This build has the torrent engine.
    pub engine: bool,
    /// The profile has a debrid account.
    pub debrid: bool,
}

impl Default for Conditions {
    fn default() -> Self {
        Self {
            runtime_s: None,
            title_year: None,
            episode_year: None,
            episodes_in_season: None,
            http: Speed {
                mbps: 15.0,
                samples: 0,
            },
            p2p: Speed {
                mbps: 10.0,
                samples: 0,
            },
            free_bytes: None,
            engine: false,
            debrid: false,
        }
    }
}

/// How a stream starts (plans/m13-stream-choice.md 3.2).
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceClass {
    /// A direct link, or a file a streaming service has ready.
    Ready,
    /// A torrent behind a debrid account with no cache answer.
    Unknown,
    /// A torrent the engine fetches from its swarm.
    Swarm,
    /// Not ready on the service, and no engine: it downloads first.
    Fetch,
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
    /// The transport URL of the install that listed this stream, which alone
    /// decides the stream's private reach. Never sent: it can carry a key.
    #[serde(skip)]
    pub addon_url: Option<url::Url>,
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
    /// Kids: a source that leaves the app (YouTube, an embedded player, an
    /// external link).
    LeavesApp,
    /// Kids: the release names an adult term.
    NotForKids,
    /// A torrent with no engine and no debrid account, or a row only a
    /// debrid account resolves.
    NeedsDebrid,
    /// A torrent larger than the free space, less 1 GiB.
    NoSpace,
}

/// Lexicographic sort key (plans/m13-stream-choice.md 3.5): earlier fields
/// always win, so no score can lift a stream past a better field. Higher is
/// better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankKey {
    /// False only when the release names its languages and none is wanted:
    /// a ready file in a language the user does not speak is no pick.
    pub language: bool,
    /// Not a cam, telesync, telecine or screener copy.
    pub legit: bool,
    /// No year in the name, or one within 3 years of the title's or the
    /// episode's.
    pub year_ok: bool,
    /// The bitrate is unknown or within the safe speed of its kind.
    pub sustains: bool,
    pub playability: Playability,
    /// It can start at all: not a swarm under 5 seeders, not a fetch.
    pub viable: bool,
    /// A torrent within 1.1 x the size limit; a ready file always is.
    pub size_ok: bool,
    /// Ready 4 (3 when not preferred); a torrent 3 at 4 x the wanted
    /// seeders, 2 at the wanted seeders, 1 at 5 or none stated, 0 under 5.
    pub start: u8,
    pub binge: bool,
    pub effective_height: u32,
    pub within: u16,
}

/// What a row tells the user, first that applies by tone (bad, warn, ok),
/// with the numbers the details show.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReasonLine {
    /// A cam, telesync or screener copy: "cam", "telesync", "screener".
    #[serde(rename_all = "camelCase")]
    Copy {
        quality: String,
    },
    #[serde(rename_all = "camelCase")]
    Fetch {
        service: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    VeryFew {
        seeders: u32,
    },
    #[serde(rename_all = "camelCase")]
    OtherYear {
        year: u16,
    },
    #[serde(rename_all = "camelCase")]
    MayPause {
        need_mbps: f32,
        have_mbps: f32,
        /// Playbacks that measured `have_mbps`; 0 is the estimate.
        samples: u32,
    },
    WashedOut,
    #[serde(rename_all = "camelCase")]
    Large {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        size_bytes: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        limit_bytes: u64,
    },
    #[serde(rename_all = "camelCase")]
    Few {
        seeders: u32,
    },
    #[serde(rename_all = "camelCase")]
    Ready {
        service: Option<String>,
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        size_bytes: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Quick {
        seeders: Option<u32>,
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        size_bytes: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Should {
        seeders: Option<u32>,
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        size_bytes: Option<u64>,
        /// The service it may be ready on (`Unknown`).
        service: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedRow {
    pub stream: ParsedStream,
    pub key: RankKey,
    pub class: SourceClass,
    pub line: ReasonLine,
    /// Size over runtime, after the pack guard; `None` when not known.
    pub bitrate_mbps: Option<f32>,
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
