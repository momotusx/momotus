//! The ZIP a subtitle downloads as (`POST /ind`): one file, a season pack,
//! or a few episodes with translator notes beside them
//! (plans/m13-turkcealtyazi.md 3.5). Only the picked entry is inflated, in
//! memory, and every cap counts bytes actually read, never a header's word.
//!
//! The site's archives are small, stored or deflated, without ZIP64 or
//! encryption, so a central-directory reader on flate2 (already in the
//! workspace) does it. The zip crate cannot join: zip 8 and 9 need flate2
//! 1.1, which stremio-core's pin on flate2 1.0 rules out, and every zip
//! below 9.0.0 carries advisories that deny.toml fails on.

use crate::rank::{Playing, same_cut, same_group};
use flate2::read::DeflateDecoder;
use regex::Regex;
use std::io::Read;
use std::sync::LazyLock;

/// The largest archive taken (the largest seen is 113 KB).
pub const MAX_ARCHIVE: usize = 4 * 1024 * 1024;
/// The largest subtitle file inflated from it.
pub const MAX_ENTRY: usize = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 200;
/// Subtitle formats `to_vtt` reads, in order of preference.
const EXTENSIONS: [&str; 4] = [".srt", ".vtt", ".ass", ".ssa"];
/// `.txt` files opened to look for SubRip timings when nothing else is a
/// subtitle; translator notes are `.txt` too.
const TXT_TRIES: usize = 4;

/// Turkish names: `2. Sezon 7. Bölüm`, `Bölüm 05`, `Sezon 2`. The vowels
/// may be another code page's bytes, read as U+FFFD.
static TR_SEASON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:(\d{1,2})\s*\.\s*sezon|sezon\s*(\d{1,2}))").expect("pattern")
});
static TR_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:(\d{1,3})\s*\.\s*b[oö\x{FFFD}]l[uü\x{FFFD}]m|b[oö\x{FFFD}]l[uü\x{FFFD}]m\s*(\d{1,3}))",
    )
    .expect("pattern")
});

/// Which file of the archive is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// A movie: the archive's only subtitle, or its best.
    Single,
    /// A single episode's row: the file naming that episode, or, when no
    /// name carries an episode, the best.
    Episode { season: u32, episode: u32 },
    /// One episode out of a pack, a list or a range: only a file naming it.
    FromSeveral { season: u32, episode: u32 },
}

/// Why an archive gave no subtitle: a fault of this file, not of the site.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArchiveFault {
    #[error("not a ZIP archive")]
    NotZip,
    #[error("more than {MAX_ENTRIES} entries")]
    TooManyEntries,
    #[error("no subtitle file inside")]
    NoSubtitle,
    #[error("no file for S{season:02}E{episode:02} inside")]
    NoEpisode { season: u32, episode: u32 },
    /// Every file reads as the same episode: the names cannot tell which
    /// file is which, so none is guessed.
    #[error("the names inside do not say which episode each file is")]
    Unclear,
    #[error("over the size limit")]
    TooLarge,
    #[error("the archive cannot be read")]
    Unreadable,
}

/// The raw bytes of the subtitle `want` names in `archive`. Among several
/// fitting files, the playing release's group wins, then its cut.
pub fn pick(archive: &[u8], want: Want, playing: &Playing) -> Result<Vec<u8>, ArchiveFault> {
    if archive.len() > MAX_ARCHIVE {
        return Err(ArchiveFault::TooLarge);
    }
    if !archive.starts_with(b"PK\x03\x04") {
        return Err(ArchiveFault::NotZip);
    }
    let files: Vec<Entry> = entries(archive)?
        .into_iter()
        // Folders, and the AppleDouble shadows a Mac adds (`._name`).
        .filter(|e| !e.name.ends_with('/') && !e.name.contains("__MACOSX"))
        .filter(|e| !base(&e.name).starts_with("._"))
        .collect();
    let mut subtitles: Vec<&Entry> = files
        .iter()
        .filter(|e| preference(&e.name).is_some())
        .collect();
    // SubRip saved as text: only when nothing else is a subtitle, and only
    // a file that holds its timings.
    let as_text = subtitles.is_empty();
    if as_text {
        subtitles = files
            .iter()
            .filter(|e| e.name.to_lowercase().ends_with(".txt"))
            .collect();
    }
    if subtitles.is_empty() {
        return Err(ArchiveFault::NoSubtitle);
    }
    let mut fitting = fitting(subtitles, want)?;
    fitting.sort_by_key(|e| {
        (
            !same_group(base(&e.name), playing.group),
            !same_cut(mq_relparse::parse(base(&e.name)).cut, playing.cut),
            preference(&e.name),
            e.name.clone(),
        )
    });
    if !as_text {
        return inflate(fitting[0]);
    }
    for entry in fitting.into_iter().take(TXT_TRIES) {
        let bytes = inflate(entry)?;
        if bytes.windows(3).any(|w| w == b"-->") {
            return Ok(bytes);
        }
    }
    Err(ArchiveFault::NoSubtitle)
}

/// The files `want` may take, never empty.
fn fitting<'e, 'a>(
    subtitles: Vec<&'e Entry<'a>>,
    want: Want,
) -> Result<Vec<&'e Entry<'a>>, ArchiveFault> {
    let (season, episode, several) = match want {
        Want::Single => return Ok(subtitles),
        Want::Episode { season, episode } => (season, episode, false),
        Want::FromSeveral { season, episode } => (season, episode, true),
    };
    let named: Vec<(Vec<u32>, Vec<u32>)> = subtitles.iter().map(|e| named(base(&e.name))).collect();
    if several
        && subtitles.len() > 1
        && !named[0].1.is_empty()
        && named.iter().all(|n| n.1 == named[0].1)
    {
        return Err(ArchiveFault::Unclear);
    }
    if !several && named.iter().all(|(_, episodes)| episodes.is_empty()) {
        return Ok(subtitles);
    }
    let fit: Vec<_> = subtitles
        .into_iter()
        .zip(&named)
        .filter(|(_, (seasons, episodes))| {
            episodes.contains(&episode) && (seasons.is_empty() || seasons.contains(&season))
        })
        .map(|(e, _)| e)
        .collect();
    if fit.is_empty() {
        return Err(ArchiveFault::NoEpisode { season, episode });
    }
    Ok(fit)
}

/// The seasons and episodes a file name gives: Turkish words first, then
/// the release parser.
fn named(name: &str) -> (Vec<u32>, Vec<u32>) {
    let number = |c: regex::Captures| c.iter().skip(1).flatten().next()?.as_str().parse().ok();
    if let Some(episode) = TR_EPISODE.captures(name).and_then(number) {
        let seasons = TR_SEASON
            .captures(name)
            .and_then(number)
            .into_iter()
            .collect();
        return (seasons, vec![episode]);
    }
    let release = mq_relparse::parse(name);
    (release.seasons, release.episodes)
}

/// One file of the archive, as the central directory and its local header
/// describe it.
struct Entry<'a> {
    name: String,
    method: u16,
    encrypted: bool,
    crc: u32,
    data: &'a [u8],
}

/// Every entry of the central directory, its data located through its
/// local header. Anything out of bounds is unreadable. The sizes come from
/// the central directory, so an entry written with a data descriptor (bit
/// 3, zero sizes in its local header) reads like any other. Names are only
/// matched, never used as a path: nothing is written to disk.
fn entries(archive: &[u8]) -> Result<Vec<Entry<'_>>, ArchiveFault> {
    let unreadable = ArchiveFault::Unreadable;
    // The end record whose comment ends exactly at the end of the file and
    // whose central directory ends exactly where it starts; a signature
    // inside a comment, or anywhere else, does not count.
    let tail = archive.len().saturating_sub(22 + 65535);
    let end = (tail..archive.len().saturating_sub(21))
        .rev()
        .find(|&at| {
            archive.get(at..at + 4) == Some(b"PK\x05\x06")
                && u16_at(archive, at + 20)
                    .is_some_and(|comment| at + 22 + comment as usize == archive.len())
                && u32_at(archive, at + 12)
                    .zip(u32_at(archive, at + 16))
                    .is_some_and(|(size, start)| start as usize + size as usize == at)
        })
        .ok_or(ArchiveFault::NotZip)?;
    let total = u16_at(archive, end + 10).ok_or(ArchiveFault::NotZip)?;
    let directory = u32_at(archive, end + 16).ok_or(ArchiveFault::NotZip)?;
    // ZIP64 marks its counts and offsets this way; the site never needs it.
    if total == u16::MAX || directory == u32::MAX {
        return Err(unreadable);
    }
    let total = total as usize;
    if total > MAX_ENTRIES {
        return Err(ArchiveFault::TooManyEntries);
    }
    let mut at = directory as usize;
    let mut out = Vec::with_capacity(total);
    for _ in 0..total {
        if archive.get(at..at + 4) != Some(b"PK\x01\x02") {
            return Err(unreadable);
        }
        let field16 = |off: usize| u16_at(archive, at + off).ok_or(ArchiveFault::Unreadable);
        let field32 = |off: usize| u32_at(archive, at + off).ok_or(ArchiveFault::Unreadable);
        let flags = field16(8)?;
        let method = field16(10)?;
        let crc = field32(16)?;
        let compressed = field32(20)?;
        let name_len = field16(28)? as usize;
        let skip = name_len + field16(30)? as usize + field16(32)? as usize;
        let local = field32(42)?;
        if [compressed, field32(24)?, local].contains(&u32::MAX) {
            return Err(unreadable);
        }
        let (compressed, local) = (compressed as usize, local as usize);
        let name = archive
            .get(at + 46..at + 46 + name_len)
            .ok_or(unreadable.clone())?;
        if archive.get(local..local + 4) != Some(b"PK\x03\x04") {
            return Err(unreadable);
        }
        let local_skip = u16_at(archive, local + 26).ok_or(unreadable.clone())? as usize
            + u16_at(archive, local + 28).ok_or(unreadable.clone())? as usize;
        let start = local + 30 + local_skip;
        out.push(Entry {
            // Names are matched on their ASCII parts (S01E05, .srt), so a
            // name in another code page needs no decoding.
            name: String::from_utf8_lossy(name).into_owned(),
            method,
            encrypted: flags & 1 == 1,
            crc,
            data: archive
                .get(start..start + compressed)
                .ok_or(unreadable.clone())?,
        });
        at += 46 + skip;
    }
    Ok(out)
}

/// The entry's bytes, read through a cap one byte past `MAX_ENTRY`, so an
/// oversized file is told apart without inflating it all, whatever its
/// header claims; checked against its CRC.
fn inflate(entry: &Entry) -> Result<Vec<u8>, ArchiveFault> {
    if entry.encrypted {
        return Err(ArchiveFault::Unreadable);
    }
    let mut bytes = Vec::new();
    let cap = MAX_ENTRY as u64 + 1;
    let read = match entry.method {
        0 => entry.data.take(cap).read_to_end(&mut bytes),
        8 => DeflateDecoder::new(entry.data)
            .take(cap)
            .read_to_end(&mut bytes),
        _ => return Err(ArchiveFault::Unreadable),
    };
    read.map_err(|_| ArchiveFault::Unreadable)?;
    if bytes.len() > MAX_ENTRY {
        return Err(ArchiveFault::TooLarge);
    }
    let mut crc = flate2::Crc::new();
    crc.update(&bytes);
    if crc.sum() != entry.crc {
        return Err(ArchiveFault::Unreadable);
    }
    Ok(bytes)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// The entry's name without its folders.
fn base(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// The format's place in `EXTENSIONS`; `None` for anything else, such as
/// a translator's note (`9x5 çeviri notu.txt`) or a MicroDVD `.sub`.
fn preference(name: &str) -> Option<usize> {
    let lower = name.to_lowercase();
    EXTENSIONS.iter().position(|ext| lower.ends_with(ext))
}
