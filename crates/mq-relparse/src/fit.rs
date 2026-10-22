//! How well a subtitle made for one release fits the release playing
//! (plans/player-polish.md 2.2 and 2g): the one rule every subtitle source
//! ranks by, the built-in TurkceAltyazi.org source and the addons alike.

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
    /// A source we trust says it was made for this very file (OpenSubtitles'
    /// hash match).
    pub hash: bool,
    /// It only covers foreign dialogue or signs.
    pub forced: bool,
    /// It is for the hard of hearing.
    pub hearing: bool,
}

impl<'a> Made<'a> {
    /// All `text` says: its source family and cut, no frame rate.
    pub fn read(text: &'a str) -> Self {
        let parsed = crate::parse(text);
        Self {
            text,
            family: family(parsed.quality),
            cut: parsed.cut,
            ..Self::default()
        }
    }
}

/// What a subtitle misses of the playing release, in the order that counts
/// (plans/player-polish.md 2g): two compare as their ranks, each field above
/// all the ones after it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Misses {
    /// It is forced: never a language's best.
    pub forced: bool,
    /// It is one part of a film split over discs ("CD2"): its times start
    /// at that part.
    pub part: bool,
    /// No trusted hash match.
    pub hash: bool,
    /// It does not name the playing group.
    pub group: bool,
    /// Exactly one of the two is a capture (CAM, telesync): never in step
    /// with the other.
    pub capture: bool,
    /// Exactly one of the two rates is PAL (25 or 50): a speed change.
    pub rate: bool,
    pub cut: bool,
    /// 0 the same source family, 1 a near one (WEB and disc), 2 another or
    /// unknown.
    pub source: u8,
    pub hearing: bool,
    /// The playing rate is measured and the subtitle's is not the same one
    /// (23.976 and 24 count as one).
    pub fps: bool,
}

/// What `made` misses of `playing`.
pub fn misses(made: &Made, playing: &Playing) -> Misses {
    let mine = family(playing.source);
    let played = rate(playing.fps);
    let given = rate(made.fps);
    Misses {
        forced: made.forced,
        part: is_part(made.text),
        hash: !made.hash,
        group: !same_group(made.text, playing.group),
        capture: is_capture(made.family) != is_capture(mine),
        rate: matches!((played, given), (Some(a), Some(b)) if a.pal != b.pal),
        cut: !same_cut(made.cut, playing.cut),
        source: match (made.family, mine) {
            (Some(a), Some(b)) if a == b => 0,
            (Some(a), Some(b)) if near(a, b) => 1,
            _ => 2,
        },
        hearing: made.hearing,
        fps: played.is_some() && !matches!((played, given), (Some(a), Some(b)) if a.same(b)),
    }
}

/// A frame rate the subtitle tools know, with its class: PAL (25, 50) runs
/// faster than film and NTSC (23.976, 24, 29.97, 30 and their doubles), whose
/// time-based subtitles share one speed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    pub fps: f32,
    pub pal: bool,
}

impl Rate {
    /// Within 0.15 percent, so 23.976 and 24 are one, 24 and 25 are not.
    fn same(self, other: Rate) -> bool {
        (self.fps / other.fps - 1.0).abs() <= 0.0015
    }
}

/// `fps` when it is a rate subtitles are made for; a field rate, a variable
/// rate or a typo is unknown.
pub fn rate(fps: Option<f32>) -> Option<Rate> {
    const KNOWN: [(f32, bool); 10] = [
        (23.976, false),
        (24.0, false),
        (25.0, true),
        (29.97, false),
        (30.0, false),
        (47.952, false),
        (48.0, false),
        (50.0, true),
        (59.94, false),
        (60.0, false),
    ];
    let fps = fps.filter(|f| f.is_finite() && *f > 0.0)?;
    KNOWN
        .iter()
        .find(|(known, _)| (fps - known).abs() <= 0.02)
        .map(|&(fps, pal)| Rate { fps, pal })
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

/// Whether `text` names a disc of a film split over several ("CD2",
/// "2of2"); the built-in source refuses those outright.
pub fn is_part(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|word| {
        let w = word.to_ascii_lowercase();
        let cd = w.len() == 3 && w.starts_with("cd") && w.as_bytes()[2].is_ascii_digit();
        let of = w
            .split_once("of")
            .and_then(|(a, b)| Some((a.parse::<u8>().ok()?, b.parse::<u8>().ok()?)))
            .is_some_and(|(part, all)| all > 1 && part <= all);
        cd || of
    })
}

fn is_capture(family: Option<Family>) -> bool {
    matches!(family, Some(Family::Cam | Family::Telesync))
}

/// A WEB file and a disc are often cut from one master: close, not equal.
fn near(a: Family, b: Family) -> bool {
    matches!(
        (a, b),
        (Family::Web, Family::Disc) | (Family::Disc, Family::Web)
    )
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

    fn rated(text: &str, fps: f32, playing: f32) -> Misses {
        let playing = Playing {
            fps: Some(playing),
            ..PLAYING
        };
        misses(
            &Made {
                fps: Some(fps),
                ..Made::read(text)
            },
            &playing,
        )
    }

    /// The group, then a capture, then a PAL rate, then the cut, then the
    /// source family.
    #[test]
    fn misses_rank_the_group_then_capture_rate_cut_and_source() {
        let own = of("Film.2020.1080p.WEB-DL.DDP5.1.H.264-FLUX");
        assert_eq!(
            own,
            Misses {
                hash: true,
                ..Misses::default()
            }
        );
        let rip = of("Film.2020.1080p.WEBRip.x264-OTHER");
        let disc = of("Film.2020.1080p.BluRay.x264-OTHER");
        let dvd = of("Film.2020.DVDRip.x264-OTHER");
        let extended = of("Film.2020.Extended.1080p.WEB-DL.x264-FLUX");
        assert!(own < extended && extended < rip && rip < disc && disc < dvd);
        assert!(
            of("Film.2020.Extended.1080p.BluRay-FLUX") < rip,
            "the group outranks all"
        );
        let cam = of("Film.2020.CAM.x264-OTHER");
        assert!(
            cam.capture && dvd < cam,
            "a capture is under any digital source"
        );
        let pal = rated("Film.2020.1080p.WEBRip-OTHER", 25.0, 23.976);
        assert!(
            pal.rate && rip < pal && pal < cam,
            "PAL drifts, a capture never fits"
        );
        let extended_rip = of("Film.2020.Extended.1080p.WEBRip.x264-OTHER");
        assert!(rip < extended_rip && extended_rip < pal);
    }

    /// 23.976, 24, 29.97 and 30 run at one speed for time-based subtitles;
    /// only PAL against them is a conflict, and an odd rate is unknown.
    #[test]
    fn only_pal_against_film_rates_is_a_conflict() {
        let text = "Film.2020.1080p.BluRay-OTHER";
        assert!(!rated(text, 24.0, 23.976).rate);
        assert!(!rated(text, 24.0, 23.976).fps, "23.976 and 24 are one");
        assert!(
            !rated(text, 29.97, 23.976).rate,
            "NTSC DVD subtitles keep film time"
        );
        assert!(
            rated(text, 29.97, 23.976).fps,
            "but they are not the same rate"
        );
        assert!(rated(text, 25.0, 24.0).rate);
        assert!(rated(text, 23.976, 50.0).rate);
        assert!(!rated(text, 25.0, 50.0).rate);
        assert!(
            !rated(text, 25.0, 41.3).rate,
            "an odd measured rate is unknown"
        );
        assert!(
            !rated(text, 25.0, 41.3).fps,
            "and nobody misses an unknown one"
        );
        assert!(!of("x").fps, "no rate measured: nobody misses it");
        assert_eq!(rate(Some(23.98)).map(|r| r.fps), Some(23.976));
        assert!(rate(Some(f32::NAN)).is_none());
    }

    /// Forced and split files never lead; a trusted hash leads everything
    /// else; the hard of hearing follow a plain track that fits as well.
    #[test]
    fn forced_and_parts_last_then_the_hash_first() {
        let text = "Film.2020.1080p.WEB-DL.H.264-FLUX";
        let made = Made::read(text);
        let plain = misses(&made, &PLAYING);
        let hashed = misses(
            &Made {
                hash: true,
                ..Made::read("Film.2020.DVDRip-X")
            },
            &PLAYING,
        );
        assert!(hashed < plain, "the file's own hash beats the group");
        let forced = misses(
            &Made {
                forced: true,
                hash: true,
                ..made
            },
            &PLAYING,
        );
        assert!(plain < forced && of("Film.2020.CAM-X") < forced);
        let part = of("Film.2020.1080p.WEB-DL.H.264-FLUX.CD2");
        assert!(part.part && of("Film.2020.CAM-X") < part && part < forced);
        let hearing = misses(
            &Made {
                hearing: true,
                ..made
            },
            &PLAYING,
        );
        assert!(plain < hearing && hearing < of("Film.2020.1080p.BluRay-FLUX"));
    }

    #[test]
    fn parts_are_disc_markers_only() {
        assert!(is_part("The_Shawshank_Redemption_(1994).CD1.aFLDp"));
        assert!(is_part("The Shawshank Redemption SBC AC3 aFLDp CD2.eng"));
        assert!(is_part("Film.2020.DVDRip.2of2"));
        assert!(!is_part("Film.2020.1of1.WEB"));
        assert!(!is_part(
            "Harry.Potter.and.the.Deathly.Hallows.Part.2.2011.1080p"
        ));
        assert!(!is_part("Film.2020.CDROM"));
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
