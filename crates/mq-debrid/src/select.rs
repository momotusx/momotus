//! Picks the file to play inside a torrent.
//!
//! Order: exact filename, then `SxxEyy` / `NxM` for the requested episode,
//! then the addon's `fileIdx`, then the largest playable video. Extras only
//! win when nothing else exists. A file named "sample" is skipped unless it is
//! the largest video (a title that contains the word stays playable).

use crate::util::basename;
use crate::{ResolveRequest, TorrentFile};
use regex::Regex;
use std::sync::OnceLock;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "ts", "m2ts", "webm", "wmv", "mpg", "mpeg", "flv", "ogv",
];
const EXTRA_DIRS: &[&str] = &[
    "extras",
    "extra",
    "featurettes",
    "featurette",
    "bonus",
    "bonus features",
    "behind the scenes",
    "deleted scenes",
    "trailers",
    "trailer",
    "interviews",
    "special features",
];

fn is_video(path: &str) -> bool {
    basename(path)
        .rsplit_once('.')
        .is_some_and(|(_, ext)| VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn is_sample(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(^|[^a-z0-9])sample([^a-z0-9]|$)").expect("valid regex"))
        .is_match(path)
}

fn is_extra(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let named = RE
        .get_or_init(|| {
            Regex::new(r"(?i)(^|[^a-z0-9])(trailer|featurette)([^a-z0-9]|$)").expect("valid regex")
        })
        .is_match(basename(path));
    let mut parts = path.split(['/', '\\']).collect::<Vec<_>>();
    parts.pop(); // the file name itself
    named
        || parts
            .iter()
            .any(|dir| EXTRA_DIRS.contains(&dir.to_ascii_lowercase().as_str()))
}

/// Every `(season, first episode, last episode)` a file name claims.
fn episodes(name: &str) -> Vec<(u32, u32, u32)> {
    static SXXEYY: OnceLock<Regex> = OnceLock::new();
    static NXM: OnceLock<Regex> = OnceLock::new();
    let sxxeyy = SXXEYY.get_or_init(|| {
        // Optional range tail: S01E01E02, S01E01-E02, S01E01-02. The trailing
        // class stops "-1080p" from reading as an episode range.
        Regex::new(
            r"(?i)(?:^|[^a-z0-9])s(\d{1,2})[ ._-]?e(\d{1,3})(?:(?:-?e|-)(\d{1,3})(?:[^0-9pi]|$))?",
        )
        .expect("valid regex")
    });
    let nxm = NXM.get_or_init(|| {
        Regex::new(r"(?i)(?:^|[^a-z0-9])(\d{1,2})x(\d{2,3})(?:[^0-9pi]|$)").expect("valid regex")
    });
    let num = |m: Option<regex::Match<'_>>| m.and_then(|m| m.as_str().parse::<u32>().ok());
    let mut out = Vec::new();
    for caps in sxxeyy.captures_iter(name) {
        if let (Some(s), Some(a)) = (num(caps.get(1)), num(caps.get(2))) {
            let b = num(caps.get(3)).filter(|&b| b > a).unwrap_or(a);
            out.push((s, a, b));
        }
    }
    for caps in nxm.captures_iter(name) {
        if let (Some(s), Some(e)) = (num(caps.get(1)), num(caps.get(2))) {
            out.push((s, e, e));
        }
    }
    out
}

fn largest<'a>(files: impl Iterator<Item = &'a TorrentFile>) -> Option<&'a TorrentFile> {
    files.max_by_key(|f| f.size)
}

/// Returns the file to resolve, or `None` when nothing playable matches. When
/// an episode is requested and the torrent clearly holds other episodes but
/// not that one, this returns `None` rather than the wrong episode.
pub fn pick_file<'a>(files: &'a [TorrentFile], req: &ResolveRequest) -> Option<&'a TorrentFile> {
    let max_video = files
        .iter()
        .filter(|f| is_video(&f.path))
        .map(|f| f.size)
        .max()
        .unwrap_or(0);
    let videos: Vec<&TorrentFile> = files
        .iter()
        .filter(|f| is_video(&f.path) && !(is_sample(&f.path) && f.size < max_video))
        .collect();
    let main: Vec<&TorrentFile> = videos
        .iter()
        .copied()
        .filter(|f| !is_extra(&f.path))
        .collect();
    let pool = if main.is_empty() { videos } else { main };

    if let Some(hint) = req.filename_hint.as_deref().map(basename)
        && let Some(file) = largest(
            pool.iter()
                .copied()
                .filter(|f| basename(&f.path).eq_ignore_ascii_case(hint)),
        )
    {
        return Some(file);
    }

    if let (Some(season), Some(episode)) = (req.season, req.episode) {
        let claims = |f: &TorrentFile| episodes(basename(&f.path));
        if let Some(file) = largest(pool.iter().copied().filter(|f| {
            claims(f)
                .iter()
                .any(|&(s, a, b)| s == season && (a..=b).contains(&episode))
        })) {
            return Some(file);
        }
        if pool.iter().any(|f| !claims(f).is_empty()) {
            return None;
        }
    }

    if let Some(file) = req
        .file_idx
        .and_then(|i| files.get(i as usize))
        .filter(|f| pool.iter().any(|p| p.index == f.index))
    {
        return Some(file);
    }

    largest(pool.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, u64)]) -> Vec<TorrentFile> {
        list.iter()
            .enumerate()
            .map(|(i, (path, size))| TorrentFile {
                index: i as u32 + 1,
                path: (*path).to_owned(),
                size: *size,
                selected: false,
                link: None,
            })
            .collect()
    }

    fn req() -> ResolveRequest {
        ResolveRequest::new("0".repeat(40))
    }

    fn pick<'a>(files: &'a [TorrentFile], req: &ResolveRequest) -> Option<&'a str> {
        pick_file(files, req).map(|f| f.path.as_str())
    }

    #[test]
    fn picks_largest_video_and_skips_samples_and_junk() {
        let f = files(&[
            ("Movie/Sample/movie-sample.mkv", 90),
            ("Movie/movie.sample.mp4", 80),
            ("Movie/movie.mkv", 4_000),
            ("Movie/movie.nfo", 5_000_000),
            ("Movie/cover.jpg", 600_000),
            ("Movie/RARBG.com.mp4", 1),
        ]);
        assert_eq!(pick(&f, &req()), Some("Movie/movie.mkv"));
    }

    #[test]
    fn extras_lose_to_the_feature_even_when_larger() {
        let f = files(&[
            ("Movie/Extras/Making of.mkv", 9_000),
            ("Movie/Movie.Trailer.mkv", 8_000),
            ("Movie/Movie.mkv", 4_000),
        ]);
        assert_eq!(pick(&f, &req()), Some("Movie/Movie.mkv"));
    }

    #[test]
    fn extras_are_used_when_nothing_else_exists() {
        let f = files(&[("Extras/a.mkv", 10), ("Extras/b.mkv", 20)]);
        assert_eq!(pick(&f, &req()), Some("Extras/b.mkv"));
    }

    #[test]
    fn a_title_containing_sample_stays_playable_when_it_is_the_feature() {
        let f = files(&[("The.Sample.Movie.2020.mkv", 4_000), ("sample.mkv", 10)]);
        assert_eq!(pick(&f, &req()), Some("The.Sample.Movie.2020.mkv"));
    }

    #[test]
    fn no_video_means_no_playable_file() {
        let f = files(&[("movie.nfo", 10), ("cover.jpg", 1)]);
        assert_eq!(pick(&f, &req()), None);
    }

    #[test]
    fn exact_filename_beats_everything() {
        let f = files(&[
            ("Pack/Show.S01E01.mkv", 900),
            ("Pack/Show.S01E02.mkv", 500),
            ("Pack/Show.S01E03.mkv", 700),
        ]);
        let mut r = req();
        r.filename_hint = Some("show.s01e02.MKV".into());
        r.season = Some(1);
        r.episode = Some(3);
        r.file_idx = Some(0);
        assert_eq!(pick(&f, &r), Some("Pack/Show.S01E02.mkv"));
    }

    #[test]
    fn season_pack_picks_requested_episode() {
        let f = files(&[
            ("Show.S02/Show.S02E01.1080p.mkv", 900),
            ("Show.S02/Show.S02E02.1080p.mkv", 950),
            ("Show.S02/Show.S02E10.1080p.mkv", 800),
            ("Show.S02/Sample/Show.S02E10.sample.mkv", 20),
        ]);
        let mut r = req();
        r.season = Some(2);
        r.episode = Some(10);
        assert_eq!(pick(&f, &r), Some("Show.S02/Show.S02E10.1080p.mkv"));
    }

    #[test]
    fn nxm_style_names_match() {
        let f = files(&[("Show 1x05.avi", 100), ("Show 1x06.avi", 200)]);
        let mut r = req();
        r.season = Some(1);
        r.episode = Some(5);
        assert_eq!(pick(&f, &r), Some("Show 1x05.avi"));
    }

    #[test]
    fn resolution_is_not_an_episode() {
        // "1920x1080" must not parse as season 19 episode 20.
        assert!(episodes("Movie.1920x1080.x264.mkv").is_empty());
        assert!(episodes("Show.S01E01-1080p.mkv").contains(&(1, 1, 1)));
    }

    #[test]
    fn multi_episode_files_cover_the_range() {
        let f = files(&[
            ("Show.S01E01E02.mkv", 100),
            ("Show.S01E03.mkv", 100),
            ("Show.S01E04-06.mkv", 100),
        ]);
        let mut r = req();
        r.season = Some(1);
        r.episode = Some(2);
        assert_eq!(pick(&f, &r), Some("Show.S01E01E02.mkv"));
        r.episode = Some(5);
        assert_eq!(pick(&f, &r), Some("Show.S01E04-06.mkv"));
    }

    #[test]
    fn wrong_episode_is_refused_instead_of_guessed() {
        let f = files(&[("Show.S01E01.mkv", 100), ("Show.S01E02.mkv", 100)]);
        let mut r = req();
        r.season = Some(1);
        r.episode = Some(7);
        r.file_idx = Some(0);
        assert_eq!(pick(&f, &r), None);
    }

    #[test]
    fn unmarked_names_fall_through_to_file_idx() {
        let f = files(&[
            ("[Group] Show - 01.mkv", 300),
            ("[Group] Show - 02.mkv", 100),
            ("[Group] Show - 03.mkv", 200),
        ]);
        let mut r = req();
        r.season = Some(1);
        r.episode = Some(2);
        r.file_idx = Some(1);
        assert_eq!(pick(&f, &r), Some("[Group] Show - 02.mkv"));
    }

    #[test]
    fn file_idx_pointing_at_a_sample_or_non_video_is_ignored() {
        let f = files(&[
            ("movie.sample.mkv", 10),
            ("movie.nfo", 10),
            ("movie.mkv", 500),
        ]);
        let mut r = req();
        r.file_idx = Some(0);
        assert_eq!(pick(&f, &r), Some("movie.mkv"));
        r.file_idx = Some(1);
        assert_eq!(pick(&f, &r), Some("movie.mkv"));
    }

    #[test]
    fn file_idx_beats_largest_for_movies() {
        let f = files(&[("a.mkv", 900), ("b.mkv", 100)]);
        let mut r = req();
        r.file_idx = Some(1);
        assert_eq!(pick(&f, &r), Some("b.mkv"));
    }
}
