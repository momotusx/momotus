//! Which file of a torrent to play.

use crate::FileInfo;

/// How the caller identifies the file to play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileHint {
    /// Index in [`crate::Engine::files`] (an addon's `fileIdx`).
    Index(usize),
    /// Episode of a season pack, matched as `S01E02` or `1x02` in the name.
    Episode { season: u32, episode: u32 },
    /// File name (an addon's `behaviorHints.filename`): exact base name, else
    /// a case-insensitive substring of the path.
    Name(String),
    /// The largest video file, else the largest file.
    Largest,
}

const VIDEO: [&str; 13] = [
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "m2ts", "mpg", "mpeg", "wmv", "flv", "ogv",
];

fn is_video(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| VIDEO.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The release parser's reading of the file name (season packs name each
/// episode file).
fn is_episode(path: &str, season: u32, episode: u32) -> bool {
    let r = mq_relparse::parse(path.rsplit('/').next().unwrap_or(path));
    r.seasons.contains(&season) && r.episodes.contains(&episode)
}

pub(crate) fn pick(files: &[FileInfo], hint: &FileHint) -> Option<usize> {
    let largest = |only_video: bool| {
        files
            .iter()
            .filter(|f| !only_video || is_video(&f.path))
            .max_by_key(|f| f.len)
            .map(|f| f.idx)
    };
    match hint {
        FileHint::Index(i) => files.iter().find(|f| f.idx == *i).map(|f| f.idx),
        FileHint::Episode { season, episode } => files
            .iter()
            .filter(|f| is_video(&f.path) && is_episode(&f.path, *season, *episode))
            .max_by_key(|f| f.len)
            .map(|f| f.idx),
        FileHint::Name(name) => {
            let lower = name.to_lowercase();
            let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_lowercase();
            files
                .iter()
                .find(|f| base(&f.path) == lower)
                .or_else(|| {
                    files
                        .iter()
                        .find(|f| f.path.to_lowercase().contains(&lower))
                })
                .map(|f| f.idx)
        }
        FileHint::Largest => largest(true).or_else(|| largest(false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[(&str, u64)]) -> Vec<FileInfo> {
        names
            .iter()
            .enumerate()
            .map(|(idx, (path, len))| FileInfo {
                idx,
                path: path.to_string(),
                len: *len,
            })
            .collect()
    }

    #[test]
    fn picks_by_index_name_episode_and_size() {
        let f = files(&[
            ("Show/Show.S01E01.1080p.mkv", 900),
            ("Show/Show.S01E02.1080p.mkv", 800),
            ("Show/sample.mkv", 50),
            ("Show/Show.S01E02.nfo", 2),
            ("Show/extras/2x10 Bonus.mp4", 700),
            ("Show/poster.jpg", 5000),
        ]);
        assert_eq!(pick(&f, &FileHint::Index(1)), Some(1));
        assert_eq!(pick(&f, &FileHint::Index(9)), None);
        let ep = |season, episode| FileHint::Episode { season, episode };
        assert_eq!(pick(&f, &ep(1, 2)), Some(1));
        assert_eq!(pick(&f, &ep(2, 10)), Some(4));
        assert_eq!(pick(&f, &ep(3, 1)), None);
        assert_eq!(pick(&f, &FileHint::Name("sample.MKV".into())), Some(2));
        assert_eq!(pick(&f, &FileHint::Name("s01e01".into())), Some(0));
        assert_eq!(pick(&f, &FileHint::Largest), Some(0));
        let no_video = files(&[("a.iso", 10), ("b.txt", 20)]);
        assert_eq!(pick(&no_video, &FileHint::Largest), Some(1));
    }
}
