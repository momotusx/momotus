//! Raw handler results (PTT vocabulary) to the public [`ParsedRelease`].

use crate::custom::slice;
use crate::engine::{Ctx, F};
use crate::model::{Audio, Hdr, ParsedRelease, Quality, Resolution, VideoCodec};
use crate::title::clean_title;
use crate::transform::Val;

pub(crate) fn finish(ctx: &Ctx, end: usize) -> ParsedRelease {
    let quality = str_of(ctx, F::Quality);
    ParsedRelease {
        title: clean_title(slice(&ctx.title, 0, end)),
        year: int_of(ctx, F::Year).and_then(|y| u16::try_from(y).ok()),
        resolution: str_of(ctx, F::Resolution).map_or(Resolution::Unknown, resolution_from),
        quality: quality.map_or(Quality::Unknown, quality_from),
        codec: str_of(ctx, F::Codec).map_or(VideoCodec::Unknown, codec_from),
        bit_depth: str_of(ctx, F::BitDepth).and_then(bit_depth_from),
        hdr: strs_of(ctx, F::Hdr)
            .iter()
            .filter_map(|s| hdr_from(s))
            .collect(),
        audio: strs_of(ctx, F::Audio)
            .iter()
            .filter_map(|s| audio_from(s))
            .collect(),
        channels: best_channels(strs_of(ctx, F::Channels)),
        languages: languages(strs_of(ctx, F::Languages)),
        seasons: ints_of(ctx, F::Seasons),
        episodes: ints_of(ctx, F::Episodes),
        complete: ctx.has(F::Complete),
        group: str_of(ctx, F::Group).map(str::to_string),
        container: container(ctx),
        remux: matches!(quality, Some("BluRay REMUX" | "REMUX")),
        proper: ctx.has(F::Proper),
        repack: ctx.has(F::Repack),
        extended: str_of(ctx, F::Edition) == Some("Extended Edition"),
        hardcoded_subs: ctx.has(F::Hardcoded),
        dubbed: ctx.has(F::Dubbed),
        three_d: ctx.has(F::ThreeD),
        upscaled: ctx.has(F::Upscaled),
    }
}

fn str_of(ctx: &Ctx, f: F) -> Option<&str> {
    match ctx.get(f) {
        Some(Val::Str(s)) => Some(s),
        _ => None,
    }
}

fn int_of(ctx: &Ctx, f: F) -> Option<u32> {
    match ctx.get(f) {
        Some(Val::Int(n)) => Some(*n),
        _ => None,
    }
}

fn strs_of(ctx: &Ctx, f: F) -> &[String] {
    match ctx.get(f) {
        Some(Val::Strs(v)) => v,
        _ => &[],
    }
}

fn ints_of(ctx: &Ctx, f: F) -> Vec<u32> {
    match ctx.get(f) {
        Some(Val::Ints(v)) => v.clone(),
        _ => Vec::new(),
    }
}

/// Map a pixel height to the nearest enum step (None below 400 lines).
fn bucket(h: u32) -> Resolution {
    match h {
        4000.. => Resolution::R4320p,
        1900.. => Resolution::R2160p,
        1300.. => Resolution::R1440p,
        1000.. => Resolution::R1080p,
        700.. => Resolution::R720p,
        560.. => Resolution::R576p,
        400.. => Resolution::R480p,
        _ => Resolution::Unknown,
    }
}

pub(crate) fn resolution_from(label: &str) -> Resolution {
    let digits = |s: &str| s.trim_end_matches(['p', 'i', 'P', 'I']).parse::<u32>().ok();
    match label.rsplit_once('x') {
        Some((_, h)) => digits(h).map_or(Resolution::Unknown, bucket),
        None => digits(label).map_or(Resolution::Unknown, bucket),
    }
}

pub(crate) fn quality_from(label: &str) -> Quality {
    match label {
        "BluRay REMUX" | "REMUX" => Quality::Remux,
        "BluRay" | "BRRip" | "BDRip" | "UHDRip" => Quality::BluRay,
        "WEB-DL" | "WEB" => Quality::WebDl,
        "WEBRip" | "WEB-DLRip" | "WEBMux" | "HDRip" => Quality::WebRip,
        "HDTV" | "HDTVRip" | "PDTV" | "TVRip" | "SATRip" | "PPVRip" => Quality::Hdtv,
        "DVD" | "DVDRip" | "VHS" | "VHSRip" => Quality::Dvd,
        "SCR" | "R5" => Quality::Screener,
        "TeleSync" | "TeleCine" => Quality::Telesync,
        "CAM" => Quality::Cam,
        _ => Quality::Unknown,
    }
}

pub(crate) fn codec_from(label: &str) -> VideoCodec {
    match label {
        "avc" => VideoCodec::H264,
        "hevc" => VideoCodec::H265,
        "av1" => VideoCodec::Av1,
        "vp9" => VideoCodec::Vp9,
        "xvid" => VideoCodec::Xvid,
        "mpeg" => VideoCodec::Mpeg2,
        "vc1" => VideoCodec::Vc1,
        _ => VideoCodec::Unknown,
    }
}

fn bit_depth_from(label: &str) -> Option<u8> {
    label.strip_suffix("bit")?.parse().ok()
}

pub(crate) fn hdr_from(label: &str) -> Option<Hdr> {
    match label {
        "DV" => Some(Hdr::DolbyVision),
        "HDR10+" => Some(Hdr::Hdr10Plus),
        "HDR" => Some(Hdr::Hdr10),
        "SDR" => Some(Hdr::Sdr),
        "HLG" => Some(Hdr::Hlg),
        _ => None,
    }
}

pub(crate) fn audio_from(label: &str) -> Option<Audio> {
    match label {
        "AAC" => Some(Audio::Aac),
        "Dolby Digital" => Some(Audio::Ac3),
        "Dolby Digital Plus" => Some(Audio::Eac3),
        "DTS" => Some(Audio::Dts),
        "DTS-HD" => Some(Audio::DtsHd),
        "DTS-HD MA" => Some(Audio::DtsHdMa),
        "DTS:X" => Some(Audio::DtsX),
        "TrueHD" => Some(Audio::TrueHd),
        "Atmos" => Some(Audio::Atmos),
        "FLAC" => Some(Audio::Flac),
        "OPUS" => Some(Audio::Opus),
        "MP3" => Some(Audio::Mp3),
        "PCM" => Some(Audio::Pcm),
        _ => None,
    }
}

fn best_channels(found: &[String]) -> Option<String> {
    ["7.1", "5.1", "2.0", "stereo", "mono"]
        .into_iter()
        .find(|c| found.iter().any(|f| f == c))
        .map(str::to_string)
}

/// PTT's "la" (Latino) is Spanish for our purposes; ISO 639-1 "la" would be Latin.
fn languages(found: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in found {
        let code = if l == "la" { "es" } else { l.as_str() };
        if !out.iter().any(|o| o == code) {
            out.push(code.to_string());
        }
    }
    out
}

const VIDEO_EXTENSIONS: [&str; 18] = [
    "mkv", "mp4", "avi", "wmv", "mpg", "mpeg", "m4v", "mov", "webm", "flv", "ts", "m2ts", "mk3d",
    "divx", "ogm", "vob", "3gp", "mpe",
];

fn container(ctx: &Ctx) -> Option<String> {
    if let Some(c) = str_of(ctx, F::Container) {
        return Some(c.to_string());
    }
    let ext = str_of(ctx, F::Extension)?;
    VIDEO_EXTENSIONS.contains(&ext).then(|| ext.to_string())
}
