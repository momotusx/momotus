//! Matroska and WebM (RFC 8794 EBML, RFC 9559 Matroska). Level 1 elements
//! are located by walking the Segment until the first Cluster and through
//! SeekHead; only the small ones are read whole.

use crate::codec::{RawVideo, flag_or_name, language, name_has};
use crate::io::Src;
use crate::{
    Attachment, AudioCodec, AudioTrack, ByteSpan, Container, Field, Provenance, StreamProbe,
    SubtitleCodec, SubtitleTrack, VideoCodec,
};
use std::collections::HashMap;

const EBML: u32 = 0x1A45_DFA3;
const DOC_TYPE: u32 = 0x4282;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const SEEK: u32 = 0x4DBB;
const SEEK_ID: u32 = 0x53AB;
const SEEK_POSITION: u32 = 0x53AC;
const INFO: u32 = 0x1549_A966;
const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
const DURATION: u32 = 0x4489;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_TYPE: u32 = 0x83;
const FLAG_DEFAULT: u32 = 0x88;
const FLAG_FORCED: u32 = 0x55AA;
const FLAG_HEARING_IMPAIRED: u32 = 0x55AB;
const FLAG_VISUAL_IMPAIRED: u32 = 0x55AC;
const DEFAULT_DURATION: u32 = 0x23_E383;
const NAME: u32 = 0x536E;
const LANGUAGE: u32 = 0x22_B59C;
const LANGUAGE_BCP47: u32 = 0x22_B59D;
const CODEC_ID: u32 = 0x86;
const CODEC_PRIVATE: u32 = 0x63A2;
const CODEC_NAME: u32 = 0x25_8688;
const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const COLOUR: u32 = 0x55B0;
const BITS_PER_CHANNEL: u32 = 0x55B2;
const TRANSFER_CHARACTERISTICS: u32 = 0x55BA;
const AUDIO: u32 = 0xE1;
const CHANNELS: u32 = 0x9F;
const BLOCK_ADDITION_MAPPING: u32 = 0x41E4;
const BLOCK_ADD_ID_TYPE: u32 = 0x41E7;
const BLOCK_ADD_ID_EXTRA_DATA: u32 = 0x41ED;
const ATTACHMENTS: u32 = 0x1941_A469;
const ATTACHED_FILE: u32 = 0x61A7;
const FILE_NAME: u32 = 0x466E;
const FILE_MEDIA_TYPE: u32 = 0x4660;
const CUES: u32 = 0x1C53_BB6B;
const CLUSTER: u32 = 0x1F43_B675;

/// Element walks stop here so malformed files cannot loop for long.
const MAX_ELEMENTS: usize = 1024;

pub(crate) fn sniff(head: &[u8]) -> bool {
    head.starts_with(&EBML.to_be_bytes())
}

#[derive(Debug, Clone, Copy)]
struct Header {
    id: u32,
    /// `None` for the reserved "unknown size" value.
    size: Option<u64>,
    len: u64,
}

impl Header {
    fn end(&self, pos: u64) -> Option<u64> {
        pos.checked_add(self.len)?.checked_add(self.size?)
    }
}

/// EBML variable-size integer; IDs keep their length marker, sizes drop it.
fn vint(b: &[u8], keep_marker: bool) -> Option<(u64, usize)> {
    let first = *b.first()?;
    let len = first.leading_zeros() as usize + 1;
    let rest = b.get(1..len).filter(|_| len <= 8)?;
    let mut v = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first) & (0xFF >> len)
    };
    for &x in rest {
        v = v << 8 | u64::from(x);
    }
    Some((v, len))
}

fn header(b: &[u8]) -> Option<Header> {
    let (id, id_len) = vint(b, true).filter(|(_, l)| *l <= 4)?;
    let (size, size_len) = vint(b.get(id_len..)?, false)?;
    let unknown = size == (1u64 << (7 * size_len)) - 1;
    Some(Header {
        id: id as u32,
        size: (!unknown).then_some(size),
        len: (id_len + size_len) as u64,
    })
}

/// Children of an in-memory master element; a truncated last child is
/// yielded as far as it goes.
struct Children<'a>(&'a [u8]);

impl<'a> Iterator for Children<'a> {
    type Item = (u32, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let h = header(self.0)?;
        let rest = self.0.get(h.len as usize..)?;
        let n = h
            .size
            .map_or(rest.len(), |s| s.min(rest.len() as u64) as usize);
        let (body, tail) = rest.split_at(n);
        self.0 = tail;
        Some((h.id, body))
    }
}

fn uint(b: &[u8]) -> Option<u64> {
    (b.len() <= 8).then(|| b.iter().fold(0u64, |v, &x| v << 8 | u64::from(x)))
}

fn float(b: &[u8]) -> Option<f64> {
    match b.len() {
        4 => Some(f64::from(f32::from_be_bytes(b.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(b.try_into().ok()?)),
        _ => None,
    }
}

fn string(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn child(b: &[u8], id: u32) -> Option<&[u8]> {
    Children(b).find(|(i, _)| *i == id).map(|(_, b)| b)
}

async fn head(src: &mut Src<'_>, pos: u64) -> Option<Header> {
    header(&src.read(pos, 12).await?)
}

/// Body of the element at `pos` when its ID is `id`.
async fn element(src: &mut Src<'_>, pos: u64, id: u32) -> Option<bytes::Bytes> {
    let h = head(src, pos).await.filter(|h| h.id == id)?;
    src.read(pos.checked_add(h.len)?, h.size?).await
}

pub(crate) async fn probe(src: &mut Src<'_>, out: &mut StreamProbe) {
    let Some(ebml) = head(src, 0).await.filter(|h| h.id == EBML) else {
        return;
    };
    let webm = match element(src, 0, EBML).await {
        Some(b) => child(&b, DOC_TYPE).is_some_and(|d| string(d) == "webm"),
        None => false,
    };
    out.container = Field::header(Some(if webm {
        Container::WebM
    } else {
        Container::Matroska
    }));
    let Some(seg_pos) = ebml.end(0) else { return };
    let Some(seg) = head(src, seg_pos).await.filter(|h| h.id == SEGMENT) else {
        return;
    };
    let data = seg_pos + seg.len;
    let end = seg.end(seg_pos).or(src.len).unwrap_or(u64::MAX);

    // Level 1 positions: a walk up to the first Cluster, then SeekHead
    // entries for what follows the clusters (usually Cues at the tail).
    let mut index = Index {
        data,
        ..Index::default()
    };
    let mut pos = data;
    for _ in 0..MAX_ELEMENTS {
        // With a SeekHead locating Tracks, skip the round trip it would take
        // to step over large Attachments just to find the first Cluster.
        if pos >= end || index.knows_tracks() && !src.cached(pos, 12) {
            break;
        }
        let Some(h) = head(src, pos).await else { break };
        match h.id {
            CLUSTER => break,
            SEEK_HEAD => {
                index.seek_heads.push(pos);
                index.parse_seek_heads(src).await;
            }
            id => index.found(id, pos),
        }
        let Some(next) = h.end(pos) else { break };
        pos = next;
    }
    index.parse_seek_heads(src).await;
    let at = index.at;

    if let Some(&p) = at.get(&INFO)
        && let Some(b) = element(src, p, INFO).await
    {
        info(&b, out);
    }
    if let Some(&p) = at.get(&TRACKS)
        && let Some(b) = element(src, p, TRACKS).await
    {
        for (id, entry) in Children(&b) {
            if id == TRACK_ENTRY {
                track(entry, out);
            }
        }
    }
    // Cues first: one tail read that helps every seek, while a lazy font
    // walk can take a round trip per file.
    if let Some(&p) = at.get(&CUES) {
        cues(src, p, out).await;
    }
    if let Some(&p) = at.get(&ATTACHMENTS) {
        attachments(src, p, out).await;
    }
}

#[derive(Default)]
struct Index {
    /// Segment data start; SeekPosition is relative to it.
    data: u64,
    at: HashMap<u32, u64>,
    seek_heads: Vec<u64>,
    parsed: usize,
}

impl Index {
    fn found(&mut self, id: u32, pos: u64) {
        self.at.entry(id).or_insert(pos);
    }

    fn knows_tracks(&self) -> bool {
        self.parsed > 0 && self.at.contains_key(&TRACKS)
    }

    /// Parses pending SeekHeads; an entry may point at another SeekHead.
    async fn parse_seek_heads(&mut self, src: &mut Src<'_>) {
        while let Some(&p) = self.seek_heads.get(self.parsed).filter(|_| self.parsed < 4) {
            self.parsed += 1;
            let Some(b) = element(src, p, SEEK_HEAD).await else {
                continue;
            };
            for (id, rel) in seeks(&b) {
                let Some(abs) = self.data.checked_add(rel) else {
                    continue;
                };
                if id != SEEK_HEAD {
                    self.found(id, abs);
                } else if !self.seek_heads.contains(&abs) {
                    self.seek_heads.push(abs);
                }
            }
        }
    }
}

fn seeks(b: &[u8]) -> impl Iterator<Item = (u32, u64)> + '_ {
    Children(b)
        .filter(|(id, _)| *id == SEEK)
        .filter_map(|(_, s)| {
            let id = uint(child(s, SEEK_ID)?)? as u32;
            Some((id, uint(child(s, SEEK_POSITION)?)?))
        })
}

fn info(b: &[u8], out: &mut StreamProbe) {
    let scale = child(b, TIMESTAMP_SCALE)
        .and_then(uint)
        .unwrap_or(1_000_000);
    let ms = child(b, DURATION)
        .and_then(float)
        .map(|d| d * scale as f64 / 1e6)
        .filter(|ms| ms.is_finite() && *ms >= 0.0);
    out.duration_ms = Field::header(ms.map(|ms| ms.round() as u64));
}

fn video_codec(id: &str) -> VideoCodec {
    match id {
        "V_MPEG4/ISO/AVC" => VideoCodec::H264,
        "V_MPEGH/ISO/HEVC" => VideoCodec::Hevc,
        "V_AV1" => VideoCodec::Av1,
        "V_VP9" => VideoCodec::Vp9,
        "V_VP8" => VideoCodec::Vp8,
        "V_MPEG1" | "V_MPEG2" => VideoCodec::Mpeg2,
        "V_MPEGI/ISO/VVC" => VideoCodec::Vvc,
        id if id.starts_with("V_MPEG4/ISO/") => VideoCodec::Mpeg4,
        id => VideoCodec::Other(id.into()),
    }
}

fn audio_codec(id: &str) -> AudioCodec {
    match id {
        "A_EAC3" => AudioCodec::Eac3,
        "A_TRUEHD" => AudioCodec::TrueHd,
        "A_FLAC" => AudioCodec::Flac,
        "A_OPUS" => AudioCodec::Opus,
        "A_VORBIS" => AudioCodec::Vorbis,
        "A_MPEG/L3" => AudioCodec::Mp3,
        "A_MPEG/L2" | "A_MPEG/L1" => AudioCodec::Mp2,
        "A_ALAC" => AudioCodec::Alac,
        id if id.starts_with("A_AAC") => AudioCodec::Aac,
        id if id.starts_with("A_AC3") => AudioCodec::Ac3,
        id if id.starts_with("A_DTS") => AudioCodec::Dts,
        id if id.starts_with("A_PCM") => AudioCodec::Pcm,
        id => AudioCodec::Other(id.into()),
    }
}

fn subtitle_codec(id: &str) -> SubtitleCodec {
    match id {
        "S_TEXT/UTF8" | "S_TEXT/ASCII" => SubtitleCodec::Srt,
        "S_TEXT/ASS" | "S_TEXT/SSA" | "S_ASS" | "S_SSA" => SubtitleCodec::Ass,
        "S_TEXT/WEBVTT" => SubtitleCodec::WebVtt,
        "S_HDMV/PGS" => SubtitleCodec::Pgs,
        "S_VOBSUB" => SubtitleCodec::VobSub,
        "S_DVBSUB" => SubtitleCodec::DvbSub,
        id => SubtitleCodec::Other(id.into()),
    }
}

#[derive(Default)]
struct Entry<'a> {
    kind: u64,
    codec_id: String,
    private: Option<&'a [u8]>,
    name: Option<String>,
    codec_name: Option<String>,
    language: Option<String>,
    bcp47: Option<String>,
    default: Option<bool>,
    forced: Option<bool>,
    hearing_impaired: Option<bool>,
    visual_impaired: Option<bool>,
    default_duration: Option<u64>,
    video: Option<&'a [u8]>,
    audio: Option<&'a [u8]>,
    dovi: Option<&'a [u8]>,
}

fn track(b: &[u8], out: &mut StreamProbe) {
    let mut e = Entry::default();
    for (id, v) in Children(b) {
        match id {
            TRACK_TYPE => e.kind = uint(v).unwrap_or(0),
            CODEC_ID => e.codec_id = string(v),
            CODEC_PRIVATE => e.private = Some(v),
            CODEC_NAME => e.codec_name = Some(string(v)),
            NAME => e.name = Some(string(v)),
            LANGUAGE => e.language = Some(string(v)),
            LANGUAGE_BCP47 => e.bcp47 = Some(string(v)),
            FLAG_DEFAULT => e.default = uint(v).map(|f| f != 0),
            FLAG_FORCED => e.forced = uint(v).map(|f| f != 0),
            FLAG_HEARING_IMPAIRED => e.hearing_impaired = uint(v).map(|f| f != 0),
            FLAG_VISUAL_IMPAIRED => e.visual_impaired = uint(v).map(|f| f != 0),
            DEFAULT_DURATION => e.default_duration = uint(v),
            VIDEO => e.video = Some(v),
            AUDIO => e.audio = Some(v),
            BLOCK_ADDITION_MAPPING => {
                let kind = child(v, BLOCK_ADD_ID_TYPE).and_then(uint).unwrap_or(0) as u32;
                if matches!(&kind.to_be_bytes(), b"dvcC" | b"dvvC" | b"dvwC") {
                    e.dovi = child(v, BLOCK_ADD_ID_EXTRA_DATA);
                }
            }
            _ => {}
        }
    }
    // RFC 9559 defaults: Language "eng", FlagDefault 1, FlagForced 0.
    let lang = language(
        e.bcp47
            .clone()
            .or(e.language.clone())
            .or(Some("eng".into())),
    );
    let default = Field::header(Some(e.default.unwrap_or(true)));
    let name = e.name.as_deref();
    match e.kind {
        1 if out.video.is_none() => out.video = Some(video(&e)),
        2 => {
            let codec = audio_codec(&e.codec_id);
            let names = [name, e.codec_name.as_deref()];
            let atmos = match codec {
                AudioCodec::Eac3 | AudioCodec::TrueHd | AudioCodec::Other(_) => {
                    if names.iter().any(|n| name_has(*n, "atmos")) {
                        Field::new(true, Provenance::ParsedName)
                    } else {
                        Field::unknown()
                    }
                }
                _ => Field::header(Some(false)),
            };
            let channels = e
                .audio
                .map(|a| child(a, CHANNELS).and_then(uint).unwrap_or(1));
            out.audio.push(AudioTrack {
                codec: Field::header(Some(codec)),
                channels: Field::header(channels.and_then(|c| u8::try_from(c).ok())),
                language: lang,
                title: Field::header(e.name.clone()),
                default,
                forced: Field::header(Some(e.forced.unwrap_or(false))),
                atmos_hint: atmos,
                audio_description: match e.visual_impaired {
                    Some(true) => Field::new(true, Provenance::ContainerHeader),
                    _ if described(name) => Field::new(true, Provenance::ParsedName),
                    other => Field::header(other),
                },
            });
        }
        0x11 => out.subtitles.push(SubtitleTrack {
            codec: Field::header(Some(subtitle_codec(&e.codec_id))),
            language: lang,
            title: Field::header(e.name.clone()),
            default,
            forced: flag_or_name(Some(e.forced.unwrap_or(false)), name, "forced"),
            hearing_impaired: flag_or_name(e.hearing_impaired, name, "sdh"),
        }),
        _ => {}
    }
}

/// A track named for audio description: "Audio Description", "Described",
/// "Descriptive Audio"; never a "description" alone ("Director's
/// description commentary").
fn described(name: Option<&str>) -> bool {
    let Some(name) = name else { return false };
    let words: Vec<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let pair = |a: &str, b: &str| words.windows(2).any(|w| w[0] == a && w[1] == b);
    pair("audio", "description")
        || pair("descriptive", "audio")
        || words
            .iter()
            .any(|w| w == "described" || w == "audiodescription")
}

fn video(e: &Entry<'_>) -> crate::VideoInfo {
    let v = e.video.unwrap_or_default();
    let colour = child(v, COLOUR).unwrap_or_default();
    RawVideo {
        codec: Some(video_codec(&e.codec_id)),
        config: e.private.map(<[u8]>::to_vec),
        width: child(v, PIXEL_WIDTH)
            .and_then(uint)
            .and_then(|w| u32::try_from(w).ok()),
        height: child(v, PIXEL_HEIGHT)
            .and_then(uint)
            .and_then(|h| u32::try_from(h).ok()),
        fps: e
            .default_duration
            .filter(|d| *d > 0)
            .map(|d| 1e9 / d as f64),
        bit_depth: child(colour, BITS_PER_CHANNEL)
            .and_then(uint)
            .filter(|b| (1..=16).contains(b))
            .map(|b| b as u8),
        transfer: child(colour, TRANSFER_CHARACTERISTICS).and_then(uint),
        dovi: e.dovi.map(<[u8]>::to_vec),
    }
    .finish()
}

/// Attachment names and types without downloading the file data, unless the
/// whole element fits the budget (players read fonts at open anyway).
async fn attachments(src: &mut Src<'_>, pos: u64, out: &mut StreamProbe) {
    let Some(h) = head(src, pos).await.filter(|h| h.id == ATTACHMENTS) else {
        return;
    };
    let (Some(start), Some(end)) = (pos.checked_add(h.len), h.end(pos)) else {
        return;
    };
    src.prefetch(start, end - start).await;
    let mut p = start;
    for _ in 0..MAX_ELEMENTS {
        if p >= end {
            break;
        }
        let Some(f) = head(src, p).await else { break };
        let Some(next) = f.end(p) else { break };
        if f.id == ATTACHED_FILE {
            let file = attached_file(src, p + f.len, next).await;
            if file.filename.value.is_some() || file.mime.value.is_some() {
                out.attachments.push(file);
            }
        }
        p = next;
    }
}

async fn attached_file(src: &mut Src<'_>, start: u64, end: u64) -> Attachment {
    let mut file = Attachment::default();
    let mut p = start;
    for _ in 0..16 {
        if p >= end {
            break;
        }
        let Some(c) = head(src, p).await else { break };
        let Some(next) = c.end(p) else { break };
        if matches!(c.id, FILE_NAME | FILE_MEDIA_TYPE)
            && next - p <= 4096
            && let Some(b) = src.read(p + c.len, next - p - c.len).await
        {
            let value = Field::header(Some(string(&b)));
            match c.id {
                FILE_NAME => file.filename = value,
                _ => file.mime = value,
            }
        }
        p = next;
    }
    file
}

/// Records where Cues live and warms them when they fit the budget, so the
/// player's first seek does not wait for a tail read.
async fn cues(src: &mut Src<'_>, pos: u64, out: &mut StreamProbe) {
    let Some(h) = head(src, pos).await.filter(|h| h.id == CUES) else {
        return;
    };
    let Some(end) = h.end(pos) else { return };
    out.seek_index = Field::header(Some(ByteSpan {
        offset: pos,
        len: end - pos,
    }));
    src.prefetch(pos, end - pos).await;
}

#[cfg(test)]
mod described_tests {
    use super::described;

    /// Core backlog C35, review note 3: the words of audio description, never
    /// a "description" alone.
    #[test]
    fn a_track_name_says_audio_description_in_its_words() {
        for name in [
            "English Audio Description",
            "Described",
            "Descriptive Audio",
            "ENG AudioDescription",
        ] {
            assert!(described(Some(name)), "{name}");
        }
        for name in [
            "Director's description commentary",
            "Description",
            "Commentary",
            "English",
        ] {
            assert!(!described(Some(name)), "{name}");
        }
        assert!(!described(None));
    }
}
