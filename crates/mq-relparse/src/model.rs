//! Public result types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Resolution {
    /// 240 to 399 lines (240p, 360p).
    #[serde(rename = "360p")]
    R360p,
    #[serde(rename = "480p")]
    R480p,
    #[serde(rename = "576p")]
    R576p,
    #[serde(rename = "720p")]
    R720p,
    #[serde(rename = "1080p")]
    R1080p,
    #[serde(rename = "1440p")]
    R1440p,
    #[serde(rename = "2160p")]
    R2160p,
    #[serde(rename = "4320p")]
    R4320p,
    #[default]
    #[serde(rename = "unknown")]
    Unknown,
}

impl Resolution {
    /// Vertical pixel count, 0 for `Unknown`.
    pub fn height(self) -> u32 {
        match self {
            Resolution::R360p => 360,
            Resolution::R480p => 480,
            Resolution::R576p => 576,
            Resolution::R720p => 720,
            Resolution::R1080p => 1080,
            Resolution::R1440p => 1440,
            Resolution::R2160p => 2160,
            Resolution::R4320p => 4320,
            Resolution::Unknown => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Cam,
    Telesync,
    Screener,
    Dvd,
    Hdtv,
    WebRip,
    WebDl,
    BluRay,
    Remux,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
    Vp9,
    Xvid,
    Mpeg2,
    Vc1,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hdr {
    Hdr10,
    Hdr10Plus,
    DolbyVision,
    Hlg,
    Sdr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audio {
    Aac,
    Ac3,
    Eac3,
    Dts,
    DtsHd,
    DtsHdMa,
    DtsX,
    TrueHd,
    Atmos,
    Flac,
    Opus,
    Mp3,
    Pcm,
}

/// Structured facts parsed from a release name.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ParsedRelease {
    /// Cleaned title, without year, season or release tags.
    pub title: String,
    pub year: Option<u16>,
    pub resolution: Resolution,
    pub quality: Quality,
    pub codec: VideoCodec,
    pub bit_depth: Option<u8>,
    pub hdr: Vec<Hdr>,
    pub audio: Vec<Audio>,
    /// Best channel layout seen: "7.1", "5.1", "2.0", "stereo" or "mono".
    pub channels: Option<String>,
    /// ISO 639-1 codes, in order of detection.
    pub languages: Vec<String>,
    pub seasons: Vec<u32>,
    pub episodes: Vec<u32>,
    /// Season pack, full series or collection.
    pub complete: bool,
    pub group: Option<String>,
    /// Lowercase container, for example "mkv", "mp4", "avi".
    pub container: Option<String>,
    pub remux: bool,
    pub proper: bool,
    pub repack: bool,
    pub extended: bool,
    pub hardcoded_subs: bool,
    pub dubbed: bool,
    pub three_d: bool,
    pub upscaled: bool,
}

/// Facts from one addon stream entry: the parsed release plus addon-formatted fields.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AddonStreamFacts {
    pub release: ParsedRelease,
    pub seeders: Option<u32>,
    pub size_bytes: Option<u64>,
    /// Indexer or tracker the addon names (for example "ThePirateBay").
    pub source_site: Option<String>,
    /// Debrid cache state when the addon states one.
    pub cached: Option<bool>,
    /// Debrid service id such as "realdebrid", "torbox".
    pub service: Option<String>,
}
