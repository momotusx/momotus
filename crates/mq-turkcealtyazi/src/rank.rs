//! Which Turkish rows fit the playing video, best first, and the label each
//! shows (plans/m13-turkcealtyazi.md 3.4).

use crate::page::{Kind, Lang, Row};
use mq_relparse::Cut;
use std::cmp::Reverse;

/// The most tracks one title offers.
pub const MAX_TRACKS: usize = 8;

/// What is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Movie,
    Episode { season: u32, episode: u32 },
}

/// What the playing release says about itself, from its file name, and its
/// frame rate when the probe read one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Playing<'a> {
    pub group: Option<&'a str>,
    pub cut: Option<Cut>,
    pub fps: Option<f32>,
}

/// One Turkish subtitle to offer.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub row: Row,
    /// Starts with the release text: what tells two files apart.
    pub label: String,
}

/// The Turkish rows that fit `target`, ranked against `playing`: the
/// playing group first, then the same cut, then the same frame rate, then
/// an episode's own file before a few episodes before a season pack, then
/// the more downloaded, then the newer.
pub fn tracks(rows: &[Row], target: Target, playing: &Playing) -> Vec<Track> {
    let mut fit: Vec<&Row> = rows
        .iter()
        .filter(|r| r.lang == Lang::Turkish && fits(&r.kind, target))
        .collect();
    fit.sort_by_key(|r| {
        (
            !same_group(&r.release, playing.group),
            mq_relparse::parse(&r.release).cut != playing.cut,
            !same_fps(r.fps, playing.fps),
            spread(&r.kind),
            Reverse(r.downloads),
            Reverse(r.altid),
        )
    });
    fit.into_iter()
        .take(MAX_TRACKS)
        .map(|r| Track {
            label: label(r),
            row: r.clone(),
        })
        .collect()
}

fn fits(kind: &Kind, target: Target) -> bool {
    match (kind, target) {
        // A film in several parts needs every part: not offered.
        (Kind::Movie { discs }, Target::Movie) => *discs == 1,
        (
            Kind::Episode { season, episode },
            Target::Episode {
                season: s,
                episode: e,
            },
        ) => (*season, *episode) == (s, e),
        (
            Kind::Episodes { season, episodes },
            Target::Episode {
                season: s,
                episode: e,
            },
        ) => *season == s && episodes.contains(&e),
        (Kind::Pack { season }, Target::Episode { season: s, .. }) => *season == s,
        _ => false,
    }
}

/// An episode's own file before a few episodes before a season pack.
fn spread(kind: &Kind) -> u8 {
    match kind {
        Kind::Episodes { .. } => 1,
        Kind::Pack { .. } => 2,
        _ => 0,
    }
}

/// Whether `text` (a release line or a file name) names `group` as a word.
/// Words are cut at every mark (`x264-CtrlHD` gives `CtrlHD`) and only at
/// spaces, slashes, dots and brackets (`CaLLioPE_MoNTEDiaZ`, `D-Z0N3` stay
/// whole), so a group with a dash or an underscore in it matches too.
pub(crate) fn same_group(text: &str, group: Option<&str>) -> bool {
    group.is_some_and(|group| {
        let fine = text.split(|c: char| !c.is_alphanumeric());
        let coarse = text.split(|c: char| c.is_whitespace() || "/.,()[]|+".contains(c));
        fine.chain(coarse)
            .any(|word| word.eq_ignore_ascii_case(group))
    })
}

fn same_fps(row: Option<f32>, playing: Option<f32>) -> bool {
    matches!((row, playing), (Some(a), Some(b)) if (a - b).abs() < 0.01)
}

/// "TOPAZ, 23.976 fps, episodes 3 to 6, by Iverson".
fn label(row: &Row) -> String {
    let mut parts = vec![row.release.clone()];
    if let Some(fps) = row.fps {
        let fps = format!("{fps:.3}");
        parts.push(format!(
            "{} fps",
            fps.trim_end_matches('0').trim_end_matches('.')
        ));
    }
    match &row.kind {
        Kind::Pack { season } => parts.push(format!("season {season} pack")),
        Kind::Episodes { episodes, .. } => parts.push(episodes_words(episodes)),
        _ => {}
    }
    if let Some(by) = &row.translator {
        parts.push(format!("by {by}"));
    }
    parts.retain(|p| !p.is_empty());
    parts.join(", ")
}

/// "episodes 3 to 6" for a run, "episodes 2, 7, 8 and 10" otherwise.
fn episodes_words(episodes: &[u32]) -> String {
    let run = episodes.windows(2).all(|w| w[1] == w[0] + 1);
    match episodes {
        [first, .., last] if run && episodes.len() > 2 => format!("episodes {first} to {last}"),
        [one] => format!("episode {one}"),
        [rest @ .., last] => {
            let rest: Vec<String> = rest.iter().map(u32::to_string).collect();
            format!("episodes {} and {last}", rest.join(", "))
        }
        [] => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episode_words_read_as_a_run_or_a_list() {
        assert_eq!(episodes_words(&[3, 4, 5, 6]), "episodes 3 to 6");
        assert_eq!(episodes_words(&[2, 7, 8, 10]), "episodes 2, 7, 8 and 10");
        assert_eq!(episodes_words(&[16, 17]), "episodes 16 and 17");
        assert_eq!(episodes_words(&[5]), "episode 5");
    }
}
