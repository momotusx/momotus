//! Release-name parser (a port of the PTT handler table) and addon stream text extractors.
//!
//! Only linear-time engines (`regex` and `regex-automata`) are used, so hostile addon text
//! cannot cause ReDoS.
//! See the crate README for attribution (NOTICE), accuracy and speed numbers.
//!
//! ```
//! use mq_relparse::{Resolution, parse, parse_addon_stream};
//!
//! let r = parse("Show.Name.S01E02.2160p.WEB-DL.DDP5.1.Atmos.HDR10+.DV.H.265-GRP.mkv");
//! assert_eq!(r.title, "Show Name");
//! assert_eq!(r.resolution, Resolution::R2160p);
//! assert_eq!((r.seasons, r.episodes), (vec![1], vec![2]));
//!
//! let f = parse_addon_stream(
//!     "[RD+] Torrentio\n4k HDR",
//!     "Movie.2020.2160p.BluRay.x265-GRP\n👤 92 💾 54.3 GB ⚙️ 1337x",
//!     Some("Movie.2020.2160p.BluRay.x265-GRP.mkv"),
//! );
//! assert_eq!(f.seeders, Some(92));
//! assert_eq!(f.source_site.as_deref(), Some("1337x"));
//! assert_eq!(f.service.as_deref(), Some("realdebrid"));
//! assert_eq!(f.cached, Some(true));
//! ```

mod addon;
mod convert;
mod custom;
mod date;
mod engine;
mod gate;
#[cfg(test)]
mod golden;
mod lean;
mod model;
mod needle;
mod rules;
mod title;
mod transform;

pub use model::{AddonStreamFacts, Audio, Hdr, ParsedRelease, Quality, Resolution, VideoCodec};

pub use addon::parse_addon_stream;

/// Inputs longer than this are cut (at a char boundary) before parsing.
const MAX_INPUT_BYTES: usize = 2048;

/// Build the handler table and compile the patterns the first titles need (tens of
/// milliseconds), so the first real [`parse`] call does not pay for it. Safe to call from a
/// background thread at startup; calling it is optional.
pub fn warm_up() {
    for sample in [
        "The.Matrix.1999.1080p.BluRay.x264.DTS-GRP",
        "Show.Name.S01E02.2160p.WEB-DL.DDP5.1.Atmos.HDR.H.265-GRP.mkv",
        "[Group] Anime Title - 05 [1080p HEVC 10bit] [ABCD1234].mkv",
    ] {
        parse(sample);
    }
}

/// Parse a torrent or release name (a file name works too).
pub fn parse(name: &str) -> ParsedRelease {
    let (ctx, end) = engine::run_all(&rules::TABLE, prepare(name));
    convert::finish(&ctx, end)
}

/// PTT replaces runs of underscores by a single space before anything else.
fn prepare(name: &str) -> String {
    let name = custom::slice(name, 0, MAX_INPUT_BYTES);
    let mut out = String::with_capacity(name.len());
    let mut prev_underscore = false;
    for c in name.chars() {
        if c == '_' {
            if !prev_underscore {
                out.push(' ');
            }
            prev_underscore = true;
        } else {
            out.push(c);
            prev_underscore = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typical_movie_release() {
        let r = parse("The.Matrix.1999.1080p.BluRay.DTS.x264-GRP");
        assert_eq!(r.title, "The Matrix");
        assert_eq!(r.year, Some(1999));
        assert_eq!(r.resolution, Resolution::R1080p);
        assert_eq!(r.quality, Quality::BluRay);
        assert_eq!(r.codec, VideoCodec::H264);
        assert_eq!(r.audio, [Audio::Dts]);
        assert_eq!(r.group.as_deref(), Some("GRP"));
    }

    #[test]
    fn episode_with_hdr_and_atmos() {
        let r = parse("Show.Name.S01E02.2160p.WEB-DL.DDP5.1.Atmos.HDR10+.DV.H.265-GRP.mkv");
        assert_eq!(
            (r.seasons.as_slice(), r.episodes.as_slice()),
            (&[1][..], &[2][..])
        );
        assert_eq!(r.quality, Quality::WebDl);
        assert_eq!(r.channels.as_deref(), Some("5.1"));
        assert_eq!(r.audio, [Audio::Atmos, Audio::Eac3]);
        assert!(r.hdr.contains(&Hdr::Hdr10Plus) && r.hdr.contains(&Hdr::DolbyVision));
        assert_eq!(r.container.as_deref(), Some("mkv"));
    }

    #[test]
    fn additions_beyond_ptt() {
        assert_eq!(
            parse("Movie.2008.1080p.BluRay.REMUX.VC-1.TrueHD-GRP").codec,
            VideoCodec::Vc1
        );
        assert_eq!(
            parse("Movie.2019.2160p.WEB-DL.VP9.HDR-GRP").codec,
            VideoCodec::Vp9
        );
        assert_eq!(
            parse("Show.S01E01.2160p.WEB-DL.HLG.HEVC-GRP").hdr,
            [Hdr::Hlg]
        );
    }

    #[test]
    fn restored_master_four_k_is_not_2160p() {
        let r = parse("The Godfather (1972) RM4K (1080p BluRay x265 10bit Tigole)");
        assert_eq!(r.resolution, Resolution::R1080p);
        assert_eq!(r.episodes, Vec::<u32>::new());
        assert_eq!(
            parse("Movie 2020 HD4K BluRay").resolution,
            Resolution::R2160p
        );
    }

    #[test]
    fn latino_spanish_maps_to_es() {
        assert_eq!(parse("Movie.2020.1080p.WEB-DL.Latino").languages, ["es"]);
    }

    #[test]
    fn long_input_is_cut_on_a_char_boundary() {
        let s = "é".repeat(5000);
        let _ = parse(&s);
    }

    #[test]
    fn result_survives_a_serde_round_trip() {
        let r = parse("Show.Name.S01E02.1080p.WEB-DL.x264-GRP");
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<ParsedRelease>(&json).unwrap(), r);
        assert!(json.contains("\"1080p\""));
    }
}
