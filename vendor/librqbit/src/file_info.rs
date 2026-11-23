use std::borrow::Cow;
use std::path::PathBuf;

use librqbit_core::torrent_metainfo::FileDetailsAttrs;

/// Marquee fork (plans/desktop-release.md 3.2 c): one component of a
/// torrent's file path as this platform stores it. On Windows a name with
/// `<>:"|?*` or a control character, a reserved device name or a trailing dot
/// or space would escape the folder (`C:`), write an alternate data stream
/// (`01: Title.flac`) or open a device (`NUL`): those characters become `_`,
/// as qBittorrent does. Elsewhere the name is kept as it is.
pub fn local_name(component: &str) -> Cow<'_, str> {
    if cfg!(windows) {
        windows_name(component)
    } else {
        Cow::Borrowed(component)
    }
}

/// [`local_name`]'s Windows rule, on every platform so its tests run
/// anywhere (mq-engine's, since this vendored crate's own do not run).
pub fn windows_name(c: &str) -> Cow<'_, str> {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let bad = |ch: char| ch.is_control() || "<>:\"|?*".contains(ch);
    let stem = c.split('.').next().unwrap_or(c).trim_end_matches(' ');
    let reserved = RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem));
    let trailing = c.ends_with(['.', ' ']);
    if !reserved && !trailing && !c.contains(bad) {
        return Cow::Borrowed(c);
    }
    let mut out: String = c.chars().map(|ch| if bad(ch) { '_' } else { ch }).collect();
    if trailing {
        let kept = out.trim_end_matches(['.', ' ']).len();
        let cut = out.len() - kept;
        out.truncate(kept);
        out.extend(std::iter::repeat_n('_', cut));
    }
    if reserved {
        let at = stem.len();
        out.insert(at, '_');
    }
    Cow::Owned(out)
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub relative_filename: PathBuf,
    pub offset_in_torrent: u64,
    pub piece_range: std::ops::Range<u32>,
    pub attrs: FileDetailsAttrs,
    pub len: u64,
}

// Iterate file pieces in the following order: first, last, everything else from start to end.
fn iter_piece_priorities(range: std::ops::Range<usize>) -> impl Iterator<Item = usize> {
    // First and last of each file first, then the rest of pieces in that file.
    let r = range;
    use std::iter::once;

    let first = once(r.start);
    let last = once(r.start + r.len().overflowing_sub(1).0); // it's ok if it repeats, doesn't matter
    let mid = r.clone().skip(1).take(r.len().overflowing_sub(2).0);

    // The take(r.len()) is to not yield start/end pieces in case of 0 and 1 lengths.
    first.chain(last).chain(mid).take(r.len())
}

impl FileInfo {
    pub fn piece_range_usize(&self) -> std::ops::Range<usize> {
        self.piece_range.start as usize..self.piece_range.end as usize
    }

    pub fn iter_piece_priorities(&self) -> impl Iterator<Item = usize> {
        iter_piece_priorities(self.piece_range_usize())
    }
}

#[cfg(test)]
mod tests {
    use super::iter_piece_priorities;

    #[test]
    fn test_iter_piece_priorities() {
        let it = |r: std::ops::Range<usize>| -> Vec<usize> { iter_piece_priorities(r).collect() };
        assert_eq!(it(0..0), Vec::<usize>::new());

        assert_eq!(it(0..1), vec![0]);
        assert_eq!(it(0..2), vec![0, 1]);
        assert_eq!(it(0..3), vec![0, 2, 1]);
        assert_eq!(it(0..4), vec![0, 3, 1, 2]);
    }
}
