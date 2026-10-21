//! Which Turkish rows fit the playing video, best first, and the label each
//! shows (plans/m13-turkcealtyazi.md 3.4).

use crate::page::{Kind, Lang, Row, Source};
pub use mq_relparse::fit::Playing;
use mq_relparse::fit::{Family, Made, misses, same_fps};
use std::cmp::Reverse;

/// The most tracks one title offers.
pub const MAX_TRACKS: usize = 8;

/// What is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Movie,
    Episode { season: u32, episode: u32 },
}

/// One Turkish subtitle to offer.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub row: Row,
    /// Starts with the release text: what tells two files apart.
    pub label: String,
}

/// The Turkish rows that fit `target`, ranked against `playing` by the
/// shared release match (`mq_relparse::fit`: the playing group, the
/// measured frame rate, the source family, the cut), then, with the frame
/// rate unknown, the most common one among the fitting rows, then an
/// episode's own file before a few episodes before a season pack, then the
/// more downloaded, then the newer.
pub fn tracks(rows: &[Row], target: Target, playing: &Playing) -> Vec<Track> {
    let mut fit: Vec<&Row> = rows
        .iter()
        .filter(|r| r.lang == Lang::Turkish && fits(&r.kind, target))
        .collect();
    // The stand-in for an unknown frame rate only follows the cut.
    let usual = usual_fps(&fit);
    // Parsed once per row: a page has up to 2000.
    fit.sort_by_cached_key(|r| {
        (
            misses(&r.made(), playing),
            playing.fps.is_none() && !same_fps(r.fps, usual),
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

impl Row {
    /// What the row says of the release its file was made for: the release
    /// line, the site's source mark, the cut the line names, the frame rate.
    pub fn made(&self) -> Made<'_> {
        Made {
            text: &self.release,
            family: self.source.and_then(source_family),
            cut: mq_relparse::parse(&self.release).cut,
            fps: self.fps,
        }
    }
}

/// The frame rate most fitting rows have; the first seen on a tie.
fn usual_fps(rows: &[&Row]) -> Option<f32> {
    let mut counts: Vec<(f32, usize)> = Vec::new();
    for fps in rows.iter().filter_map(|r| r.fps) {
        match counts
            .iter_mut()
            .find(|(seen, _)| same_fps(Some(*seen), Some(fps)))
        {
            Some((_, n)) => *n += 1,
            None => counts.push((fps, 1)),
        }
    }
    // `max_by_key` keeps the last of equals: reversed, the first seen wins.
    counts
        .into_iter()
        .rev()
        .max_by_key(|(_, n)| *n)
        .map(|(fps, _)| fps)
}

/// `HD` and `HDRip` say nothing of where a file came from.
fn source_family(source: Source) -> Option<Family> {
    Some(match source {
        Source::WebRip | Source::WebDl => Family::Web,
        Source::BdRip | Source::BluRay => Family::Disc,
        Source::DvdRip => Family::Dvd,
        Source::DvdScr | Source::R5 => Family::Screener,
        Source::Cam => Family::Cam,
        Source::Hdts => Family::Telesync,
        Source::Hd | Source::HdRip => return None,
    })
}

/// "TOPAZ DVDRip, 23.976 fps, episodes 3 to 6, by Iverson".
fn label(row: &Row) -> String {
    let mut release = row.release.clone();
    if let Some(source) = row.source.map(Source::name)
        && !release.to_lowercase().contains(&source.to_lowercase())
    {
        release = format!("{release} {source}").trim().to_owned();
    }
    let mut parts = vec![release];
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
