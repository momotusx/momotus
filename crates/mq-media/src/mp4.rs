//! ISO BMFF (MP4, MOV). The top level is walked box by box, so `moov` is
//! found at the head or after `mdat`. Inside `moov` the sample tables are
//! skipped by size and only small boxes are read.

use crate::codec::{RawVideo, language};
use crate::io::Src;
use crate::{
    AudioCodec, AudioTrack, ByteSpan, Container, Field, Hev1Entry, StreamProbe, SubtitleCodec,
    SubtitleTrack, VideoCodec, VideoInfo,
};
use bytes::Bytes;

type Kind = [u8; 4];

const TOP_LEVEL: [&Kind; 8] = [
    b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"pnot", b"styp",
];
const CONTAINERS: [&Kind; 5] = [b"trak", b"edts", b"mdia", b"minf", b"stbl"];
/// Box walks stop here so malformed files cannot loop for long.
const MAX_BOXES: usize = 256;

pub(crate) fn sniff(head: &[u8]) -> bool {
    head.get(4..8)
        .is_some_and(|t| TOP_LEVEL.iter().any(|k| &k[..] == t))
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

struct BoxHeader {
    kind: Kind,
    len: u64,
    /// Total size including the header; `None` runs to the end of the file.
    size: Option<u64>,
}

fn box_header(b: &[u8]) -> Option<BoxHeader> {
    let kind: Kind = b.get(4..8)?.try_into().ok()?;
    let (size, len) = match be32(b, 0)? {
        0 => (None, 8),
        1 => (Some(be64(b, 8)?), 16),
        n => (Some(u64::from(n)), 8),
    };
    if size.is_some_and(|s| s < len) {
        return None;
    }
    Some(BoxHeader { kind, len, size })
}

/// Children of an in-memory box body.
struct Boxes<'a>(&'a [u8]);

impl<'a> Iterator for Boxes<'a> {
    type Item = (Kind, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let h = box_header(self.0)?;
        let total = h
            .size
            .map_or(self.0.len(), |s| s.min(self.0.len() as u64) as usize);
        let body = self.0.get(h.len as usize..total)?;
        self.0 = &self.0[total..];
        Some((h.kind, body))
    }
}

/// A box body located in the file without reading it.
struct Leaf {
    kind: Kind,
    start: u64,
    end: u64,
}

async fn boxes(src: &mut Src<'_>, start: u64, end: u64) -> Vec<Leaf> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos < end && out.len() < MAX_BOXES {
        let Some(h) = src.read(pos, 16).await.and_then(|b| box_header(&b)) else {
            break;
        };
        let Some(next) = h.size.map_or(Some(end), |s| pos.checked_add(s)) else {
            break;
        };
        let next = next.min(end);
        out.push(Leaf {
            kind: h.kind,
            start: (pos + h.len).min(next),
            end: next,
        });
        pos = next;
    }
    out
}

/// Leaf boxes of a `trak`, descending through the plain container boxes.
async fn leaves(src: &mut Src<'_>, start: u64, end: u64) -> Vec<Leaf> {
    let mut stack = vec![(start, end)];
    let mut out = Vec::new();
    for _ in 0..32 {
        let Some((s, e)) = stack.pop() else { break };
        for leaf in boxes(src, s, e).await {
            if CONTAINERS.contains(&&leaf.kind) {
                stack.push((leaf.start, leaf.end));
            } else {
                out.push(leaf);
            }
        }
    }
    out
}

async fn read_leaf(src: &mut Src<'_>, leaves: &[Leaf], kind: &Kind, max: u64) -> Option<Bytes> {
    let leaf = leaves.iter().find(|l| &l.kind == kind)?;
    src.read(leaf.start, (leaf.end - leaf.start).min(max)).await
}

pub(crate) async fn probe(src: &mut Src<'_>, out: &mut StreamProbe) {
    out.container = Field::header(Some(Container::Mp4));
    let end = src.len.unwrap_or(u64::MAX);
    let mut pos = 0u64;
    for _ in 0..MAX_BOXES {
        if pos >= end {
            break;
        }
        let Some(b) = src.read(pos, 16).await else {
            break;
        };
        let Some(h) = box_header(&b) else { break };
        let size = h
            .size
            .or_else(|| src.len.map(|l| l - pos))
            .filter(|s| *s >= h.len);
        match &h.kind {
            b"ftyp" if b.get(8..12) == Some(b"qt  ") => {
                out.container = Field::header(Some(Container::QuickTime));
            }
            b"moov" => {
                if let Some(size) = size.and_then(|s| pos.checked_add(s).map(|_| s)) {
                    out.seek_index = Field::header(Some(ByteSpan {
                        offset: pos,
                        len: size,
                    }));
                    moov(src, pos + h.len, pos + size, out).await;
                }
                return;
            }
            _ => {}
        }
        let Some(next) = size.and_then(|s| pos.checked_add(s)) else {
            break;
        };
        pos = next;
    }
}

async fn moov(src: &mut Src<'_>, start: u64, end: u64, out: &mut StreamProbe) {
    src.prefetch(start, end - start).await;
    let kids = boxes(src, start, end).await;
    let (movie_scale, movie_ms) = read_leaf(src, &kids, b"mvhd", 32)
        .await
        .and_then(|b| mvhd(&b))
        .unwrap_or((0, None));
    let mut longest = None;
    for t in kids.iter().filter(|l| &l.kind == b"trak") {
        longest = longest.max(trak(src, t.start, t.end, movie_scale, out).await);
    }
    out.duration_ms = Field::header(movie_ms.or(longest));
}

fn ms(duration: u64, scale: u32) -> Option<u64> {
    (scale > 0 && duration > 0 && duration != u64::MAX && duration != u64::from(u32::MAX))
        .then(|| (u128::from(duration) * 1000 / u128::from(scale)) as u64)
}

fn mvhd(b: &[u8]) -> Option<(u32, Option<u64>)> {
    let (scale, duration) = match b.first()? {
        1 => (be32(b, 20)?, be64(b, 24)?),
        _ => (be32(b, 12)?, u64::from(be32(b, 16)?)),
    };
    Some((scale, ms(duration, scale)))
}

/// (timescale, duration, language)
fn mdhd(b: &[u8]) -> Option<(u32, u64, Option<String>)> {
    let (scale, duration, lang_at) = match b.first()? {
        1 => (be32(b, 20)?, be64(b, 24)?, 32),
        _ => (be32(b, 12)?, u64::from(be32(b, 16)?), 20),
    };
    Some((scale, duration, be16(b, lang_at).and_then(iso639)))
}

/// Packed ISO 639-2/T code; values below 0x400 are QuickTime Macintosh codes.
fn iso639(packed: u16) -> Option<String> {
    if packed < 0x400 {
        return None;
    }
    let code: String = [10, 5, 0]
        .iter()
        .map(|shift| char::from(((packed >> shift) & 0x1F) as u8 + 0x60))
        .collect();
    code.chars().all(|c| c.is_ascii_lowercase()).then_some(code)
}

/// Sum of edit durations in movie timescale units.
fn elst(b: &[u8]) -> Option<u64> {
    let step = if *b.first()? == 1 { 20 } else { 12 };
    let count = be32(b, 4)? as usize;
    let total = b
        .get(8..)?
        .chunks_exact(step)
        .take(count)
        .map(|e| match step {
            20 => be64(e, 0).unwrap_or(0),
            _ => u64::from(be32(e, 0).unwrap_or(0)),
        })
        .fold(0u64, u64::saturating_add);
    (total > 0).then_some(total)
}

fn stts_fps(b: &[u8], scale: u32) -> Option<f64> {
    let count = be32(b, 4)? as usize;
    let (samples, ticks) =
        b.get(8..)?
            .as_chunks::<8>()
            .0
            .iter()
            .take(count)
            .fold((0u64, 0u128), |(s, t), e| {
                let n = be32(e, 0).unwrap_or(0);
                let delta = be32(e, 4).unwrap_or(0);
                (s + u64::from(n), t + u128::from(n) * u128::from(delta))
            });
    (ticks > 0 && scale > 0).then(|| samples as f64 * f64::from(scale) / ticks as f64)
}

/// Handler names written by muxers ("SoundHandler", "Core Media Video")
/// say nothing about the track, so only other names become titles.
fn handler_name(hdlr: &[u8]) -> Option<String> {
    let raw = hdlr.get(24..)?;
    // QuickTime writes a counted string: drop the length byte.
    let raw = match raw.first() {
        Some(&c) if c < 0x20 => &raw[1..],
        _ => raw,
    };
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    let name = String::from_utf8_lossy(&raw[..end]).trim().to_owned();
    (!name.is_empty() && !name.ends_with("Handler") && !name.starts_with("Core Media"))
        .then_some(name)
}

/// First sample entry of an `stsd` box: its fourcc and body.
fn sample_entry(stsd: &[u8]) -> Option<(Kind, &[u8])> {
    Boxes(stsd.get(8..)?).next()
}

async fn trak(
    src: &mut Src<'_>,
    start: u64,
    end: u64,
    movie_scale: u32,
    out: &mut StreamProbe,
) -> Option<u64> {
    let l = leaves(src, start, end).await;
    let hdlr = read_leaf(src, &l, b"hdlr", 256).await?;
    let handler: Kind = hdlr.get(8..12)?.try_into().ok()?;
    let (scale, media_duration, lang) = read_leaf(src, &l, b"mdhd", 64)
        .await
        .and_then(|b| mdhd(&b))
        .unwrap_or((0, 0, None));
    let edits = read_leaf(src, &l, b"elst", 4096)
        .await
        .and_then(|b| elst(&b));
    let duration = edits
        .and_then(|d| ms(d, movie_scale))
        .or_else(|| ms(media_duration, scale));
    let stsd = read_leaf(src, &l, b"stsd", 1 << 20).await;
    let entry = stsd.as_deref().and_then(sample_entry);
    if &handler == b"vide"
        && let (Some(body), Some(leaf)) = (&stsd, l.iter().find(|x| &x.kind == b"stsd"))
    {
        out.hev1.extend(hev1_entries(body, leaf.start));
    }
    let title = Field::header(handler_name(&hdlr));
    let lang = language(lang);
    match &handler {
        b"vide" if out.video.is_none() => {
            let stts = read_leaf(src, &l, b"stts", 64 * 1024).await;
            let fps = match stts.as_deref().and_then(|b| stts_fps(b, scale)) {
                Some(fps) => Some(fps),
                None => read_leaf(src, &l, b"stsz", 12)
                    .await
                    .and_then(|b| be32(&b, 8))
                    .filter(|_| media_duration > 0)
                    .map(|n| f64::from(n) * f64::from(scale) / media_duration as f64),
            };
            out.video = Some(video(entry, fps));
        }
        b"soun" => out.audio.push(audio(entry, lang, title)),
        b"subt" | b"sbtl" | b"text" | b"clcp" => out.subtitles.push(SubtitleTrack {
            codec: Field::header(entry.map(|(k, _)| subtitle_codec(&k))),
            language: lang,
            title,
            ..Default::default()
        }),
        _ => {}
    }
    duration
}

fn fourcc(k: &Kind) -> String {
    String::from_utf8_lossy(k).trim().to_owned()
}

fn video_codec(k: &Kind) -> VideoCodec {
    match k {
        b"avc1" | b"avc3" => VideoCodec::H264,
        b"hvc1" | b"hev1" | b"dvh1" | b"dvhe" => VideoCodec::Hevc,
        b"av01" | b"dav1" => VideoCodec::Av1,
        b"vp09" => VideoCodec::Vp9,
        b"vp08" => VideoCodec::Vp8,
        b"mp4v" => VideoCodec::Mpeg4,
        b"mp2v" | b"m2v1" => VideoCodec::Mpeg2,
        b"vvc1" | b"vvi1" => VideoCodec::Vvc,
        k => VideoCodec::Other(fourcc(k)),
    }
}

fn audio_codec(k: &Kind) -> AudioCodec {
    match k {
        b"mp4a" => AudioCodec::Aac,
        b"ac-3" => AudioCodec::Ac3,
        b"ec-3" => AudioCodec::Eac3,
        b"mlpa" => AudioCodec::TrueHd,
        b"dtsc" | b"dtsh" | b"dtsl" | b"dtse" | b"dtsx" => AudioCodec::Dts,
        b"fLaC" => AudioCodec::Flac,
        b"Opus" => AudioCodec::Opus,
        b".mp3" => AudioCodec::Mp3,
        b"alac" => AudioCodec::Alac,
        b"lpcm" | b"sowt" | b"twos" | b"ipcm" | b"fpcm" => AudioCodec::Pcm,
        k => AudioCodec::Other(fourcc(k)),
    }
}

fn subtitle_codec(k: &Kind) -> SubtitleCodec {
    match k {
        b"tx3g" => SubtitleCodec::MovText,
        b"wvtt" => SubtitleCodec::WebVtt,
        b"stpp" => SubtitleCodec::Ttml,
        k => SubtitleCodec::Other(fourcc(k)),
    }
}

/// The `hev1` and `dvhe` entries of a video `stsd` whose body starts at
/// file offset `at`: after its version, flags and entry count, each entry
/// is a box.
fn hev1_entries(stsd: &[u8], at: u64) -> Vec<Hev1Entry> {
    let mut out = Vec::new();
    let mut pos = 8;
    while let Some(size) = be32(stsd, pos).map(|s| s as usize).filter(|s| *s >= 8) {
        let Some(body) = stsd.get(pos + 8..pos + size) else {
            break;
        };
        let serve_as = match stsd.get(pos + 4..pos + 8) {
            Some(b"hev1") => Some(*b"hvc1"),
            Some(b"dvhe") => Some(*b"dvh1"),
            _ => None,
        };
        if let Some(serve_as) = serve_as {
            let parameter_sets = Boxes(body.get(78..).unwrap_or_default())
                .find(|(k, _)| k == b"hvcC")
                .is_some_and(|(_, c)| hvcc_parameter_sets(c).unwrap_or(false));
            out.push(Hev1Entry {
                fourcc_at: at + pos as u64 + 4,
                serve_as,
                parameter_sets,
            });
        }
        pos += size;
    }
    out
}

/// The `hvcC` record's NAL arrays (ISO/IEC 14496-15 8.3.3.1) hold at least
/// one VPS, SPS and PPS (types 32, 33 and 34).
fn hvcc_parameter_sets(b: &[u8]) -> Option<bool> {
    let mut seen = [false; 3];
    let mut pos = 23;
    for _ in 0..*b.get(22)? {
        let kind = b.get(pos)? & 0x3f;
        let count = be16(b, pos + 1)?;
        pos += 3;
        for _ in 0..count {
            pos += 2 + usize::from(be16(b, pos)?);
        }
        if count > 0
            && let Some(slot) = kind
                .checked_sub(32)
                .and_then(|i| seen.get_mut(usize::from(i)))
        {
            *slot = true;
        }
    }
    Some(seen.iter().all(|s| *s))
}

/// Visual sample entry: 78 bytes of fixed fields, then child boxes.
fn video(entry: Option<(Kind, &[u8])>, fps: Option<f64>) -> VideoInfo {
    let mut raw = RawVideo {
        fps,
        ..Default::default()
    };
    if let Some((kind, body)) = entry {
        raw.codec = Some(video_codec(&kind));
        raw.width = be16(body, 24).map(u32::from);
        raw.height = be16(body, 26).map(u32::from);
        for (k, b) in Boxes(body.get(78..).unwrap_or_default()) {
            match &k {
                b"avcC" | b"hvcC" | b"av1C" => raw.config = Some(b.to_vec()),
                b"dvcC" | b"dvvC" | b"dvwC" => raw.dovi = Some(b.to_vec()),
                b"colr" if matches!(b.get(..4), Some(b"nclx" | b"nclc")) => {
                    raw.transfer = be16(b, 6).map(u64::from);
                }
                _ => {}
            }
        }
    }
    raw.finish()
}

/// Audio channels per AC-3 `acmod` (1+1 dual mono counts as two).
fn acmod_channels(acmod: u8) -> u32 {
    [2, 1, 2, 3, 3, 4, 4, 5][usize::from(acmod & 7)]
}

/// Channels added by each `chan_loc` bit of an E-AC-3 dependent substream,
/// most significant first: Lc/Rc, Lrs/Rrs, Cs, Ts, Lsd/Rsd, Lw/Rw, Vhl/Vhr,
/// Vhc, Lts/Rts (ETSI TS 102 366 annex F).
const CHAN_LOC: [u32; 9] = [2, 2, 1, 1, 2, 2, 2, 1, 2];

/// Channels of the first independent substream and the JOC (Atmos) flag
/// from the optional extension byte (ETSI TS 103 420).
fn dec3(b: &[u8]) -> Option<(u32, Option<bool>)> {
    let substreams = usize::from(*b.get(1)? & 7) + 1;
    let mut pos = 2;
    let mut channels = None;
    for _ in 0..substreams {
        let modes = *b.get(pos + 1)?;
        let deps = *b.get(pos + 2)? >> 1 & 0xF;
        let mut n = acmod_channels(modes >> 1 & 7) + u32::from(modes & 1);
        if deps > 0 {
            let loc = u16::from(*b.get(pos + 2)? & 1) << 8 | u16::from(*b.get(pos + 3)?);
            n += CHAN_LOC
                .iter()
                .enumerate()
                .filter(|(i, _)| loc >> (8 - i) & 1 != 0)
                .map(|(_, w)| w)
                .sum::<u32>();
            pos += 4;
        } else {
            pos += 3;
        }
        channels.get_or_insert(n);
    }
    Some((channels?, b.get(pos).map(|x| x & 1 != 0)))
}

/// Sound sample entry: version 0, 1 or 2 fixed fields, then child boxes.
fn audio(
    entry: Option<(Kind, &[u8])>,
    language: Field<String>,
    title: Field<String>,
) -> AudioTrack {
    let mut track = AudioTrack {
        language,
        title,
        ..Default::default()
    };
    let Some((kind, body)) = entry else {
        return track;
    };
    let codec = audio_codec(&kind);
    let version = be16(body, 8).unwrap_or(0);
    let mut channels = match version {
        2 => be32(body, 40),
        _ => be16(body, 16).map(u32::from),
    };
    track.atmos_hint = match codec {
        AudioCodec::Eac3 | AudioCodec::TrueHd | AudioCodec::Other(_) => Field::unknown(),
        _ => Field::header(Some(false)),
    };
    let children = match version {
        1 => 44,
        2 => 64,
        _ => 28,
    };
    for (k, b) in Boxes(body.get(children..).unwrap_or_default()) {
        match &k {
            b"dac3" => {
                if let Some(&modes) = b.get(1) {
                    channels = Some(acmod_channels(modes >> 3 & 7) + u32::from(modes >> 2 & 1));
                }
            }
            b"dec3" => {
                if let Some((n, joc)) = dec3(b) {
                    channels = Some(n);
                    track.atmos_hint = Field::header(joc);
                }
            }
            _ => {}
        }
    }
    track.codec = Field::header(Some(codec));
    track.channels = Field::header(
        channels
            .and_then(|c| u8::try_from(c).ok())
            .filter(|c| *c > 0),
    );
    track
}
