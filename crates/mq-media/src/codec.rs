//! Codec configuration records shared by the MKV and MP4 parsers:
//! avcC (ISO/IEC 14496-15 5.3.3), hvcC (14496-15 8.3.3), av1C (AV1-ISOBMFF
//! 2.3) and the Dolby Vision decoder configuration record (dvcC/dvvC/dvwC).

use crate::{Field, Hdr, Provenance, VideoCodec, VideoInfo};

/// Video facts collected from a container before codec records are decoded.
#[derive(Default)]
pub(crate) struct RawVideo {
    pub codec: Option<VideoCodec>,
    /// avcC, hvcC or av1C payload (MKV CodecPrivate or the MP4 child box).
    pub config: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub bit_depth: Option<u8>,
    /// ITU-T H.273 transfer characteristics.
    pub transfer: Option<u64>,
    pub dovi: Option<Vec<u8>>,
}

#[derive(Default)]
struct Config {
    profile: Option<String>,
    level: Option<String>,
    bit_depth: Option<u8>,
    fps: Option<f64>,
}

impl RawVideo {
    pub fn finish(self) -> VideoInfo {
        let config = self.config.as_deref().unwrap_or_default();
        let cfg = match self.codec {
            Some(VideoCodec::Hevc) => hvcc(config),
            Some(VideoCodec::H264) => avcc(config),
            Some(VideoCodec::Av1) => av1c(config),
            _ => Config::default(),
        };
        let dv = self.dovi.as_deref().and_then(dovi);
        // Millisecond-rounded frame durations give 24.000002; keep 3 decimals
        // so 23.976 and 24 stay apart and compare cleanly.
        let fps = self
            .fps
            .or(cfg.fps)
            .filter(|f| f.is_finite() && *f > 0.0)
            .map(|f| (f * 1000.0).round() / 1000.0);
        VideoInfo {
            codec: Field::header(self.codec),
            profile: Field::header(cfg.profile),
            level: Field::header(cfg.level),
            width: Field::header(self.width.filter(|w| *w > 0)),
            height: Field::header(self.height.filter(|h| *h > 0)),
            fps: Field::header(fps),
            bit_depth: Field::header(cfg.bit_depth.or(self.bit_depth)),
            hdr: Field::header(hdr(self.transfer, dv)),
            dv_profile: Field::header(dv.map(|d| d.0)),
            dv_bl_compat_id: Field::header(dv.map(|d| d.1)),
        }
    }
}

/// HDR formats from the transfer function and the Dolby Vision base layer
/// compatibility id (1 and 6 are HDR10, 4 is HLG). `None` means unknown.
fn hdr(transfer: Option<u64>, dv: Option<(u8, u8)>) -> Option<Vec<Hdr>> {
    // H.273 0, 2 and 3 are reserved or unspecified.
    let transfer = transfer.filter(|t| !matches!(t, 0 | 2 | 3));
    if transfer.is_none() && dv.is_none() {
        return None;
    }
    let mut out = Vec::new();
    if let Some((_, compat)) = dv {
        out.push(Hdr::DolbyVision);
        match compat {
            1 | 6 => out.push(Hdr::Hdr10),
            4 => out.push(Hdr::Hlg),
            _ => {}
        }
    }
    let extra = match transfer {
        Some(16) => Some(Hdr::Hdr10),
        Some(18) => Some(Hdr::Hlg),
        _ => None,
    };
    if let Some(h) = extra.filter(|h| !out.contains(h)) {
        out.push(h);
    }
    Some(out)
}

/// (profile, base layer compatibility id) from a DOVIDecoderConfigurationRecord.
fn dovi(c: &[u8]) -> Option<(u8, u8)> {
    let c = c.get(..5)?;
    Some((c[2] >> 1, c[4] >> 4))
}

fn level(major: u8, minor: u8) -> String {
    format!("{major}.{minor}")
}

fn hvcc(c: &[u8]) -> Config {
    let Some(c) = c.get(..23).filter(|c| c[0] == 1) else {
        return Config::default();
    };
    let profile = match c[1] & 0x1F {
        1 => "Main".into(),
        2 => "Main 10".into(),
        3 => "Main Still Picture".into(),
        4 => "Range Extensions".into(),
        5 => "High Throughput".into(),
        9 => "Screen Content".into(),
        n => format!("profile {n}"),
    };
    let avg = u16::from_be_bytes([c[19], c[20]]);
    Config {
        profile: Some(profile),
        level: Some(level(c[12] / 30, c[12] % 30 / 3)),
        bit_depth: Some((c[17] & 7) + 8),
        fps: (avg > 0).then(|| f64::from(avg) / 256.0),
    }
}

fn avcc(c: &[u8]) -> Config {
    let Some(head) = c.get(..6).filter(|c| c[0] == 1) else {
        return Config::default();
    };
    let idc = head[1];
    let profile = match idc {
        66 if head[2] & 0x40 != 0 => "Constrained Baseline".into(),
        66 => "Baseline".into(),
        77 => "Main".into(),
        88 => "Extended".into(),
        100 => "High".into(),
        110 => "High 10".into(),
        122 => "High 4:2:2".into(),
        244 => "High 4:4:4 Predictive".into(),
        n => format!("profile {n}"),
    };
    let bit_depth = match idc {
        66 | 77 | 88 | 100 => Some(8),
        _ => avcc_ext_bit_depth(c),
    };
    Config {
        profile: Some(profile),
        level: Some(level(head[3] / 10, head[3] % 10)),
        bit_depth,
        fps: None,
    }
}

/// High profile avcC records may end with chroma format and bit depth after
/// the parameter sets.
fn avcc_ext_bit_depth(c: &[u8]) -> Option<u8> {
    let skip = |pos: &mut usize, count: usize| -> Option<()> {
        for _ in 0..count {
            *pos += 2 + usize::from(u16::from_be_bytes([*c.get(*pos)?, *c.get(*pos + 1)?]));
        }
        Some(())
    };
    let mut pos = 6;
    skip(&mut pos, usize::from(c[5] & 0x1F))?;
    let pps = usize::from(*c.get(pos)?);
    pos += 1;
    skip(&mut pos, pps)?;
    c.get(pos + 1).map(|b| (b & 7) + 8)
}

fn av1c(c: &[u8]) -> Config {
    let Some(c) = c.get(..3).filter(|c| c[0] & 0x80 != 0) else {
        return Config::default();
    };
    let profile = c[1] >> 5;
    let idx = c[1] & 0x1F;
    let bit_depth = match (c[2] & 0x40 != 0, c[2] & 0x20 != 0) {
        (false, _) => 8,
        (true, true) if profile == 2 => 12,
        (true, _) => 10,
    };
    Config {
        profile: Some(match profile {
            0 => "Main".into(),
            1 => "High".into(),
            2 => "Professional".into(),
            n => format!("profile {n}"),
        }),
        level: (idx < 31).then(|| level(2 + (idx >> 2), idx & 3)),
        bit_depth: Some(bit_depth),
        fps: None,
    }
}

/// Whole-word, case-insensitive match in a track name, for hints such as
/// "Atmos", "SDH" or "Forced" that muxers often leave out of the flags.
pub(crate) fn name_has(name: Option<&str>, word: &str) -> bool {
    name.is_some_and(|n| {
        n.split(|c: char| !c.is_alphanumeric())
            .any(|w| w.eq_ignore_ascii_case(word))
    })
}

/// A container flag, upgraded to a name hint when the flag is false but the
/// track name says otherwise.
pub(crate) fn flag_or_name(flag: Option<bool>, name: Option<&str>, word: &str) -> Field<bool> {
    match flag {
        Some(true) => Field::new(true, Provenance::ContainerHeader),
        _ if name_has(name, word) => Field::new(true, Provenance::ParsedName),
        other => Field::header(other),
    }
}

/// ISO 639-2 or BCP 47 language; "und" means unknown.
pub(crate) fn language(lang: Option<String>) -> Field<String> {
    Field::header(lang.filter(|l| !l.is_empty() && l != "und"))
}
