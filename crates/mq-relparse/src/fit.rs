//! How well a subtitle made for one release fits the release playing
//! (plans/player-polish.md 2.2): the one rule every subtitle source ranks
//! by, the built-in TurkceAltyazi.org source and the addons alike.

use crate::{Cut, Quality};

/// What the playing release says about itself, from its file name, and its
/// frame rate when the probe read one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Playing<'a> {
    pub group: Option<&'a str>,
    /// Its source (`Quality::Unknown` when the name gives none).
    pub source: Quality,
    pub cut: Option<Cut>,
    pub fps: Option<f32>,
}

/// What a subtitle says of the release it was made for.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Made<'a> {
    /// Its release line or file name.
    pub text: &'a str,
    pub family: Option<Family>,
    pub cut: Option<Cut>,
    pub fps: Option<f32>,
}

impl<'a> Made<'a> {
    /// All `text` says: its source family and cut, no frame rate.
    pub fn read(text: &'a str) -> Self {
        let parsed = crate::parse(text);
        Self {
            text,
            family: family(parsed.quality),
            cut: parsed.cut,
            fps: None,
        }
    }
}

/// What a subtitle misses of the playing release, in the order that
/// counts: two compare as their ranks, fewer misses first, the group most.
/// A measured frame rate outranks the source family (a wrong rate drifts
/// whatever the source).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Misses {
    /// It does not name the playing group.
    pub group: bool,
    /// The playing frame rate is measured and it is not the subtitle's.
    pub fps: bool,
    /// Its source family is not the playing one's, or either is unknown.
    pub source: bool,
    pub cut: bool,
}

/// What `made` misses of `playing`.
pub fn misses(made: &Made, playing: &Playing) -> Misses {
    let source = family(playing.source);
    Misses {
        group: !same_group(made.text, playing.group),
        fps: playing.fps.is_some() && !same_fps(made.fps, playing.fps),
        source: source.is_none() || made.family != source,
        cut: !same_cut(made.cut, playing.cut),
    }
}

/// Whether `text` (a release line or a file name) names `group` as a word.
/// Words are cut at every mark (`x264-CtrlHD` gives `CtrlHD`) and only at
/// spaces, slashes, dots and brackets (`CaLLioPE_MoNTEDiaZ`, `D-Z0N3` stay
/// whole), so a group with a dash or an underscore in it matches too.
pub fn same_group(text: &str, group: Option<&str>) -> bool {
    group.is_some_and(|group| {
        let fine = text.split(|c: char| !c.is_alphanumeric());
        let coarse = text.split(|c: char| c.is_whitespace() || "/.,()[]|+".contains(c));
        fine.chain(coarse)
            .any(|word| word.eq_ignore_ascii_case(group))
    })
}

pub fn same_fps(a: Option<f32>, b: Option<f32>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if (a - b).abs() < 0.01)
}

/// Most releases are the theatrical cut without saying so.
pub fn same_cut(a: Option<Cut>, b: Option<Cut>) -> bool {
    let plain = |c: Option<Cut>| c.filter(|c| *c != Cut::Theatrical);
    plain(a) == plain(b)
}

/// Sources whose files share their timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Web,
    Disc,
    Dvd,
    Screener,
    Cam,
    Telesync,
}

/// A WEB-DL's timing is a WEBRip's, a BluRay's a Remux's; HDTV says nothing.
pub fn family(quality: Quality) -> Option<Family> {
    Some(match quality {
        Quality::WebRip | Quality::WebDl => Family::Web,
        Quality::BluRay | Quality::Remux => Family::Disc,
        Quality::Dvd => Family::Dvd,
        Quality::Screener => Family::Screener,
        Quality::Cam => Family::Cam,
        Quality::Telesync => Family::Telesync,
        Quality::Hdtv | Quality::Unknown => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYING: Playing = Playing {
        group: Some("FLUX"),
        source: Quality::WebDl,
        cut: None,
        fps: None,
    };

    fn of(text: &str) -> Misses {
        misses(&Made::read(text), &PLAYING)
    }

    /// The group first, then the source family, then the cut; a measured
    /// frame rate between the group and the source.
    #[test]
    fn misses_rank_the_group_then_the_rate_then_the_source_then_the_cut() {
        let own = of("Film.2020.1080p.WEB-DL.DDP5.1.H.264-FLUX");
        assert_eq!(own, Misses::default());
        let rip = of("Film.2020.1080p.WEBRip.x264-OTHER");
        let disc = of("Film.2020.1080p.BluRay.x264-OTHER");
        let extended = of("Film.2020.Extended.1080p.WEB-DL.x264-FLUX");
        assert!(own < extended && extended < rip && rip < disc);
        assert!(
            of("Film.2020.Extended.1080p.BluRay-FLUX") < rip,
            "the group outranks all"
        );
        let measured = Playing {
            fps: Some(23.976),
            ..PLAYING
        };
        let at = |fps, text| {
            misses(
                &Made {
                    fps,
                    ..Made::read(text)
                },
                &measured,
            )
        };
        let rate = at(Some(23.976), "Film.2020.1080p.BluRay-OTHER");
        let source = at(Some(25.0), "Film.2020.1080p.WEBRip-OTHER");
        assert!(rate < source, "a measured rate outranks the source");
        assert!(at(None, "x").fps, "an unknown rate misses a measured one");
        assert!(!of("x").fps, "no rate measured: nobody misses it");
    }

    #[test]
    fn a_group_is_a_word_of_the_text() {
        assert!(same_group("Film.2020.x264-CtrlHD", Some("ctrlhd")));
        assert!(same_group("Film 2020 D-Z0N3", Some("D-Z0N3")));
        assert!(!same_group("Film.2020.x264-CtrlHDX", Some("CtrlHD")));
        assert!(!same_group("Film.2020", None));
        assert!(same_cut(Some(Cut::Theatrical), None));
        assert!(!same_cut(Some(Cut::Extended), None));
    }
}
