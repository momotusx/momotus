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
    /// The decoder plays 10-bit H.264 (Hi10P), which has no hardware path on
    /// Apple's chips. True unless a device says otherwise.
    pub ten_bit_h264: bool,
    /// Formats the audio output accepts as passthrough.
    pub passthrough: Vec<Audio>,
    /// Audio formats the primary engine decodes itself.
    pub audio_decoders: Vec<Audio>,
    /// Containers the primary engine opens, as lowercase file extensions.
    pub containers: Vec<String>,
    /// A second engine (libmpv) exists for what the primary cannot play.
    pub has_fallback_engine: bool,
    /// The primary engine shows HEVC in MP4 or MOV only from a sample
    /// entry that carries its parameter sets (Apple's AVFoundation): an
    /// `hev1` entry without them plays as sound alone.
    pub hvc1_only: bool,
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
            ten_bit_h264: true,
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
            hvc1_only: false,
        }
    }

    /// Whether the primary engine opens `container`, a file extension.
    pub fn opens(&self, container: &str) -> bool {
        self.containers
            .iter()
            .any(|known| known.eq_ignore_ascii_case(container))
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
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
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
    /// The torrent engine on swarms under the wanted seeders, learned from
    /// those swarms' own samples (buffer-and-bitrate research R2): a thin
    /// swarm sets its own pace, whatever the line. With no sample yet it is
    /// the prior for an 18-seeder swarm, scaled per seeder for each row.
    pub p2p_thin: Speed,
    pub free_bytes: Option<u64>,
    /// `R`: what idle torrents hold, which a playback may delete.
    pub reclaimable_bytes: Option<u64>,
    /// `C`: what the windows playing will still take.
    pub committed_bytes: u64,
    /// On mobile data: a window keeps the device preset ahead, so its row
    /// says no minutes (plans/streaming-window.md 3.3).
    pub metered: bool,
    /// Each torrent folder's bytes by info hash: a row's own folder is room
    /// for its window, never for it whole (plans/streaming-window.md 2.1).
    #[serde(skip)]
    pub held: std::collections::HashMap<String, mq_engine_sched::HeldBytes>,
    /// This build has the torrent engine.
    pub engine: bool,
    /// The profile has a debrid account.
    pub debrid: bool,
    /// Info hashes (lowercase) whose swarm sent almost nothing when this
    /// device last tried them: they may not start, whatever the seeders an
    /// addon lists.
    pub dead_swarms: Vec<String>,
    /// The size limit is one the user set (or an older app's quality set),
    /// not the default: the line says "the size you chose".
    pub limit_chosen: bool,
    /// Digests (`key_digest`) of the stream keys whose file showed no
    /// picture on this device (an `hev1` entry without parameter sets on
    /// an `hvc1_only` engine).
    pub no_picture: Vec<String>,
    /// Digests (`key_digest`) of the stream keys this device's player could
    /// not decode when it last played them.
    pub did_not_play: Vec<String>,
    /// Digests (`target_digest`) of the stream keys whose torrent file was
    /// not the version listed, for the target it was played for.
    pub wrong_file: Vec<String>,
    /// The container of the file each version opened here, by
    /// `target_digest` for the target it opened for: it stands over the
    /// listing's, which often names none.
    pub containers: Vec<(String, String)>,
    /// The versions are ranked to download, not to play now
    /// (plans/offline-downloads.md 4.5): no speed rule, the size limit on
    /// every row, the whole file plus 1 GiB must fit, and among versions as
    /// good the smaller file first.
    pub download: bool,
    /// The language the title was made in (ISO 639-1), when the user chose
    /// no audio languages: a release in it, or naming none, is the
    /// original and ranks above a dub (`RankKey::original`).
    pub original_language: Option<String>,
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
            // The lowest rate a 17 to 22 seeder swarm failed to keep up
            // with in the race (Sintel at 1.36 Mbit/s stalled 92 s of 120).
            p2p_thin: Speed {
                mbps: 1.4,
                samples: 0,
            },
            free_bytes: None,
            engine: false,
            debrid: false,
            dead_swarms: Vec::new(),
            limit_chosen: false,
            no_picture: Vec::new(),
            did_not_play: Vec::new(),
            wrong_file: Vec::new(),
            containers: Vec::new(),
            reclaimable_bytes: None,
            committed_bytes: 0,
            metered: false,
            held: Default::default(),
            download: false,
            original_language: None,
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

/// How a version plays, as the viewer groups them: through the torrent
/// engine, through a streaming service, or from a plain link
/// (plans/plugins-and-marketplace.md 4.1).
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Torrent,
    Debrid,
    Direct,
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
/// Read back from JSON only by a kept copy that restores the skipped
/// fields itself (an offline download, plans/offline-downloads.md 4.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// `size_bytes` is the played file's (`behaviorHints.videoSize`), never
    /// a pack's to share among its episodes.
    #[serde(skip)]
    pub size_is_file: bool,
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

#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Playability {
    NeedsTranscode,
    NeedsFallbackEngine,
    /// It plays natively if its unstated container is one of the few this
    /// device opens (an mp4-only player): a stated mp4 goes first (code
    /// review r1 P3-4).
    Unstated,
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
}

/// Lexicographic sort key (plans/m13-stream-choice.md 3.5): earlier fields
/// always win, so no score can lift a stream past a better field. Higher is
/// better.
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankKey {
    /// False only when the release names its languages and none is wanted:
    /// a ready file in a language the user does not speak is no pick.
    pub language: bool,
    /// Not a cam, telesync, telecine or screener copy.
    pub legit: bool,
    /// It can start at all: not a swarm under 5 seeders, not a fetch. A
    /// row that may never start loses to every row that can, however it
    /// plays (code review r2 P2-3).
    pub viable: bool,
    /// No year in the name, or one within 3 years of the title's or the
    /// episode's.
    pub year_ok: bool,
    /// The release is in the title's original language, names no
    /// language, or the original is not known: a dub ranks below the
    /// original whenever both can start (`Conditions::original_language`).
    pub original: bool,
    /// The bitrate is unknown or within the safe speed of its kind: at
    /// most 0.7 of the speed, that is a speed at least 1.43 times the
    /// bitrate.
    pub sustains: bool,
    /// How far the bitrate is over the row's speed (x = bitrate / speed):
    /// 3 when it sustains (x at most 0.7) or the bitrate is unknown, 2 when
    /// the speed covers it with no margin (x at most 1), 1 up to 1.5 times
    /// the speed, 0 beyond. Among rows that cannot keep up, the one that
    /// pauses least ranks first, before resolution (research R1).
    pub pace: u8,
    /// In pace band 0, the share of the bitrate the row's speed covers, in
    /// thousandths (1000 / x): the row least over its speed first, so a
    /// file 10 times over stays ahead of one 25 times over (race 3). 0 in
    /// the other bands, which `pace` orders.
    pub covered: u16,
    pub playability: Playability,
    /// A torrent within 1.1 x the size limit; a ready file always is.
    pub size_ok: bool,
    /// How it starts, as far as it decides over resolution: ready 3 (2
    /// when not preferred), a torrent at the wanted seeders 2, one at 5 or
    /// none stated 1, under 5 0. Past the wanted seeders a swarm starts as
    /// well as with more, so the better version wins (pick audit: 16 of 16
    /// swarms with 4 to 333 live seeders started, in no order of seeders).
    pub tier: u8,
    pub binge: bool,
    pub effective_height: u32,
    /// Ready 4 (3 when not preferred); a torrent 3 at 4 x the wanted
    /// seeders, 2 at the wanted seeders, 1 at 5 or none stated, 0 under 5.
    /// Among versions as good, more seeders first.
    pub start: u8,
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
        /// `have_mbps` is the small-swarm figure: the swarm has fewer
        /// seeders than wanted (research R2).
        small_swarm: bool,
    },
    WashedOut,
    /// The swarm sent almost nothing when this device last tried it.
    Stalled,
    /// The file's video showed no picture on this device: it plays as
    /// sound alone.
    NoPicture,
    /// This device's player could not decode it when it last tried.
    DidNotPlay,
    #[serde(rename_all = "camelCase")]
    Large {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        size_bytes: u64,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        limit_bytes: u64,
        /// The user set the limit: "Larger than the size you chose";
        /// otherwise "Large file" (3.7).
        chosen: bool,
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
    /// A torrent too big for the free space that plays in a window
    /// (plans/streaming-window.md 3.3), in place of `Quick` or `Should`.
    #[serde(rename_all = "camelCase")]
    Buffer {
        seeders: Option<u32>,
        #[cfg_attr(feature = "ts", ts(type = "number | null"))]
        size_bytes: Option<u64>,
        /// Minutes of it the window holds ahead, rounded down, at most 10;
        /// none on mobile data, where the engine keeps the device preset.
        ahead_min: Option<u32>,
        /// The window's size.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        budget_bytes: u64,
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
