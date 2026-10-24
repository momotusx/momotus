//! Container probe for remote media.
//!
//! [`probe`] reads MKV and MP4 headers through a [`RangeReader`] inside a byte
//! and time budget and returns a [`StreamProbe`] whose fields each carry a
//! provenance. Reads are block aligned so they can share a [`ByteCache`] with
//! the loopback server. [`opensubtitles_hash`] computes the classic movie hash.

mod cache;
mod codec;
mod file;
mod http;
mod io;
mod mkv;
mod mp4;

use bytes::Bytes;
use futures_util::future::BoxFuture;
use std::time::Duration;

pub use cache::{BLOCK_SIZE, ByteCache};
pub use file::FileRangeReader;
pub use http::HttpRangeReader;
pub use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("server ignored the range request")]
    RangeUnsupported,
    #[error("source length unknown")]
    UnknownLength,
    #[error("source shorter than expected")]
    Truncated,
    #[error("http: {0}")]
    Http(String),
}

/// Random access to a byte source. Futures are boxed so the trait stays
/// usable as `&dyn RangeReader`.
#[allow(clippy::len_without_is_empty)]
pub trait RangeReader: Send + Sync {
    /// Total length in bytes.
    fn len(&self) -> BoxFuture<'_, Result<u64, ProbeError>>;
    /// Up to `len` bytes at `offset`; shorter only at the end of the source.
    fn read_at(&self, offset: u64, len: u64) -> BoxFuture<'_, Result<Bytes, ProbeError>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeBudget {
    pub max_bytes: u64,
    pub max_time: Duration,
}

impl Default for ProbeBudget {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024,
            max_time: Duration::from_millis(1500),
        }
    }
}

/// Where a value came from. This crate produces `ContainerHeader` and
/// `ParsedName`; the playback session merges `AddonHint` and `Sampled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provenance {
    ContainerHeader,
    Sampled,
    AddonHint,
    ParsedName,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field<T> {
    pub value: Option<T>,
    pub provenance: Provenance,
}

impl<T> Default for Field<T> {
    fn default() -> Self {
        Self::unknown()
    }
}

impl<T> Field<T> {
    pub const fn unknown() -> Self {
        Self {
            value: None,
            provenance: Provenance::Unknown,
        }
    }

    pub const fn new(value: T, provenance: Provenance) -> Self {
        Self {
            value: Some(value),
            provenance,
        }
    }

    pub(crate) fn header(value: Option<T>) -> Self {
        value.map_or_else(Self::unknown, |v| Self::new(v, Provenance::ContainerHeader))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Container {
    Matroska,
    WebM,
    Mp4,
    QuickTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
    Vp9,
    Vp8,
    Mpeg2,
    Mpeg4,
    Vvc,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioCodec {
    Aac,
    Ac3,
    Eac3,
    TrueHd,
    Dts,
    Flac,
    Opus,
    Vorbis,
    Mp3,
    Mp2,
    Pcm,
    Alac,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleCodec {
    Srt,
    /// ASS and SSA; libass renders both.
    Ass,
    WebVtt,
    Pgs,
    VobSub,
    DvbSub,
    MovText,
    Ttml,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hdr {
    Hdr10,
    Hlg,
    DolbyVision,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VideoInfo {
    pub codec: Field<VideoCodec>,
    /// Codec profile name, for example "Main 10" or "High".
    pub profile: Field<String>,
    /// Codec level, for example "5.1".
    pub level: Field<String>,
    pub width: Field<u32>,
    pub height: Field<u32>,
    pub fps: Field<f64>,
    pub bit_depth: Field<u8>,
    /// Known and empty means SDR.
    pub hdr: Field<Vec<Hdr>>,
    pub dv_profile: Field<u8>,
    pub dv_bl_compat_id: Field<u8>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AudioTrack {
    pub codec: Field<AudioCodec>,
    pub channels: Field<u8>,
    pub language: Field<String>,
    pub title: Field<String>,
    pub default: Field<bool>,
    pub forced: Field<bool>,
    pub atmos_hint: Field<bool>,
    /// Described for blind and low-vision viewers (audio description): the
    /// container's flag, or the track's name ("Audio Description").
    pub audio_description: Field<bool>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SubtitleTrack {
    pub codec: Field<SubtitleCodec>,
    pub language: Field<String>,
    pub title: Field<String>,
    pub default: Field<bool>,
    pub forced: Field<bool>,
    pub hearing_impaired: Field<bool>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attachment {
    pub filename: Field<String>,
    pub mime: Field<String>,
}

/// An MP4 or MOV video sample entry of type `hev1` (HEVC) or `dvhe` (Dolby
/// Vision): parameter sets that may come in band. Apple's AVFoundation
/// decodes these only as `hvc1` and `dvh1` entries: an `hev1` file plays
/// its sound with no picture. The same entry served under the other type
/// decodes when its `hvcC` carries the parameter sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hev1Entry {
    /// File offset of the entry's four-byte type.
    pub fourcc_at: u64,
    /// The type Apple decodes: `hvc1` for `hev1`, `dvh1` for `dvhe`.
    pub serve_as: [u8; 4],
    /// The `hvcC` holds VPS, SPS and PPS: served as `serve_as` it plays on
    /// Apple. Without them it plays there in no form.
    pub parameter_sets: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSpan {
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StreamProbe {
    pub container: Field<Container>,
    pub duration_ms: Field<u64>,
    pub video: Option<VideoInfo>,
    pub audio: Vec<AudioTrack>,
    pub subtitles: Vec<SubtitleTrack>,
    pub attachments: Vec<Attachment>,
    /// MKV `Cues` or MP4 `moov`: the region a player reads to seek.
    pub seek_index: Field<ByteSpan>,
    /// Every video track's `hev1` and `dvhe` sample entries (MP4 and MOV
    /// only).
    pub hev1: Vec<Hev1Entry>,
    pub budget_exhausted: bool,
    pub cancelled: bool,
    /// Error that stopped the probe early, for example `RangeUnsupported`.
    pub error: Option<ProbeError>,
    pub bytes_read: u64,
    pub elapsed: Duration,
}

/// Probes the container headers. Never fails: missing facts stay `Unknown`
/// and the flags say why the probe stopped.
pub async fn probe(
    reader: &dyn RangeReader,
    budget: ProbeBudget,
    cancel: &CancellationToken,
) -> StreamProbe {
    let started = tokio::time::Instant::now();
    let mut src = io::Src::new(reader, budget, cancel);
    let mut out = StreamProbe::default();
    src.load_len().await;
    if let Some(head) = src.read(0, 16).await {
        if mkv::sniff(&head) {
            mkv::probe(&mut src, &mut out).await;
        } else if mp4::sniff(&head) {
            mp4::probe(&mut src, &mut out).await;
        }
    }
    out.budget_exhausted = src.exhausted;
    out.cancelled = src.cancelled;
    out.error = src.error;
    out.bytes_read = src.bytes_read;
    out.elapsed = started.elapsed();
    out
}

/// Whether a file's first bytes are a video container's: Matroska or
/// WebM, MP4 or MOV, MPEG-TS, AVI. A download whose source answered with a
/// page (a dead debrid link, a captive portal) is refused by this
/// (plans/offline-downloads.md 4.7).
pub fn looks_like_video(head: &[u8]) -> bool {
    let ts = head.len() > 188 && head[0] == 0x47 && head[188] == 0x47;
    let avi = head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"AVI ";
    mkv::sniff(head) || mp4::sniff(head) || ts || avi
}

/// The `hev1` and `dvhe` sample entries of an MP4 or MOV file; none for any
/// other container. Only the MP4 path runs, so a Matroska file costs one
/// read.
pub async fn hev1_entries(
    reader: &dyn RangeReader,
    budget: ProbeBudget,
    cancel: &CancellationToken,
) -> Vec<Hev1Entry> {
    let mut src = io::Src::new(reader, budget, cancel);
    let mut out = StreamProbe::default();
    src.load_len().await;
    if src.read(0, 16).await.is_some_and(|head| mp4::sniff(&head)) {
        mp4::probe(&mut src, &mut out).await;
    }
    out.hev1
}

/// OpenSubtitles hash: file size plus the wrapping sum of the little-endian
/// u64 words of the first and last 64 KiB, as 16 hex digits.
pub async fn opensubtitles_hash(reader: &dyn RangeReader) -> Result<(String, u64), ProbeError> {
    const CHUNK: u64 = 64 * 1024;
    let size = reader.len().await?;
    if size < CHUNK {
        return Err(ProbeError::Truncated);
    }
    let head = reader.read_at(0, CHUNK).await?;
    let tail = reader.read_at(size - CHUNK, CHUNK).await?;
    if head.len() as u64 != CHUNK || tail.len() as u64 != CHUNK {
        return Err(ProbeError::Truncated);
    }
    let hash = head
        .as_chunks::<8>()
        .0
        .iter()
        .chain(tail.as_chunks::<8>().0)
        .fold(size, |acc, w| acc.wrapping_add(u64::from_le_bytes(*w)));
    Ok((format!("{hash:016x}"), size))
}
