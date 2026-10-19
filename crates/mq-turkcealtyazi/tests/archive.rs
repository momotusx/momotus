//! Picking the subtitle out of the archives TurkceAltyazi.org sends,
//! recorded 2026-10-19 (tests/corpus, trimmed to a few cues each with the
//! real entry names and encodings), and the hostile archives the reader
//! must refuse without a panic. Memory under bombs and random bytes is
//! measured in tests/archive_alloc.rs.

mod common;

use common::{Options, corpus, find, zip, zip_with};
use mq_turkcealtyazi::{ArchiveFault, MAX_ARCHIVE, MAX_ENTRY, Want, pick};

/// The first cue's start in each recorded pack entry (from `unzip -p`).
const PACK_STARTS: [&str; 7] = [
    "00:01:08,135",
    "00:00:19,386",
    "00:00:44,611",
    "00:00:03,170",
    "00:00:03,637",
    "00:00:51,351",
    "00:00:01,201",
];
const CUE: &[u8] = b"1\n00:00:01,000 --> 00:00:02,000\nhi\n";
const STORED: Options = Options {
    stored: true,
    descriptor: false,
};

fn starts(bytes: &[u8], cue: &str) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.trim_start_matches('\u{feff}')
        .starts_with(&format!("1\r\n{cue} -->"))
}

fn single(archive: &[u8]) -> Result<Vec<u8>, ArchiveFault> {
    pick(archive, Want::Single, None)
}

#[test]
fn a_single_file_and_a_movie_come_out_as_they_are() {
    let episode = single(&corpus("episode.zip")).unwrap();
    // Windows-1254, CRLF: decoding is to_vtt's work.
    assert!(episode.starts_with(b"1\r\n00:02:06,573 --> 00:02:08,487\r\nBenim ad\xfdm Walter"));
    let movie = single(&corpus("movie.zip")).unwrap();
    assert!(movie.starts_with(b"1\r\n"));
    assert!(movie.windows(5).any(|w| w == b" --> "));
}

#[test]
fn the_episode_is_picked_out_of_a_season_pack() {
    let pack = corpus("pack.zip");
    for (i, cue) in PACK_STARTS.iter().enumerate() {
        let episode = i as u32 + 1;
        let got = pick(&pack, Want::Episode { season: 1, episode }, None).unwrap();
        assert!(
            got.starts_with(b"\xef\xbb\xbf"),
            "UTF-8 with BOM, kept for to_vtt"
        );
        assert!(starts(&got, cue), "S01E{episode:02}");
    }
    for (season, episode) in [(1, 8), (2, 5)] {
        assert_eq!(
            pick(&pack, Want::Episode { season, episode }, None),
            Err(ArchiveFault::NoEpisode { season, episode })
        );
    }
}

#[test]
fn a_few_episodes_with_translator_notes_beside_them() {
    // Four SRTs and four notes named like "9x5 çeviri notu.txt" (UTF-8 bytes
    // without the ZIP's UTF-8 flag): a note is never taken.
    let multi = corpus("multi.zip");
    for (episode, cue) in [
        (3, "00:02:04,994"),
        (4, "00:00:03,436"),
        (5, "00:00:50,821"),
        (6, "00:00:48,281"),
    ] {
        let got = pick(&multi, Want::Episode { season: 9, episode }, None).unwrap();
        assert!(starts(&got, cue), "S09E{episode:02}");
    }
    assert_eq!(
        pick(
            &multi,
            Want::Episode {
                season: 9,
                episode: 7
            },
            None
        ),
        Err(ArchiveFault::NoEpisode {
            season: 9,
            episode: 7
        })
    );
}

#[test]
fn among_fitting_files_the_playing_group_then_srt_wins() {
    let two = zip(&[
        (
            "Movie.2008.1080p.BluRay-ALPHA.srt",
            b"1\n00:00:01,000 --> 00:00:02,000\nalpha\n",
        ),
        (
            "Movie.2008.1080p.WEB-BETA.srt",
            b"1\n00:00:01,000 --> 00:00:02,000\nbeta\n",
        ),
        ("Movie.2008.1080p.WEB-BETA.ass", b"[Events]\n"),
    ]);
    assert!(
        pick(&two, Want::Single, Some("beta"))
            .unwrap()
            .ends_with(b"beta\n")
    );
    assert!(single(&two).unwrap().ends_with(b"alpha\n"));
    let notes = zip(&[("notu.txt", b"x"), ("film.sub", b"{0}{25}microdvd")]);
    assert_eq!(single(&notes), Err(ArchiveFault::NoSubtitle));
}

#[test]
fn caps_count_bytes_read_never_a_header() {
    let big = vec![b'a'; MAX_ENTRY + 1];
    let over = zip(&[("big.srt", &big)]);
    assert_eq!(single(&over), Err(ArchiveFault::TooLarge));
    // The same entry with both headers claiming 1024 bytes: still refused.
    let mut lying = over.clone();
    let claim = 1024u32.to_le_bytes();
    let local = find(&lying, b"PK\x03\x04");
    lying[local + 22..local + 26].copy_from_slice(&claim);
    let central = find(&lying, b"PK\x01\x02");
    lying[central + 24..central + 28].copy_from_slice(&claim);
    assert_eq!(single(&lying), Err(ArchiveFault::TooLarge));
    // At the cap exactly, deflated: fine.
    let at = &big[..MAX_ENTRY];
    assert_eq!(single(&zip(&[("at.srt", at)])).unwrap().len(), MAX_ENTRY);
    // Stored, a 4 MB file cannot fit a 4 MB archive with its headers.
    let stored = zip_with(&[(b"at.srt", at)], STORED);
    assert_eq!(single(&stored), Err(ArchiveFault::TooLarge));
    let mb = zip_with(&[(b"mb.srt", &big[..1024 * 1024])], STORED);
    assert_eq!(single(&mb).unwrap().len(), 1024 * 1024);
}

#[test]
fn what_is_not_a_small_zip_is_refused() {
    assert_eq!(
        single(b"<html>Just a moment...</html>"),
        Err(ArchiveFault::NotZip)
    );
    let mut huge = b"PK\x03\x04".to_vec();
    huge.resize(MAX_ARCHIVE + 1, 0);
    assert_eq!(single(&huge), Err(ArchiveFault::TooLarge));
    let names: Vec<String> = (0..201).map(|i| format!("{i}.srt")).collect();
    let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), CUE)).collect();
    assert_eq!(single(&zip(&files)), Err(ArchiveFault::TooManyEntries));
}

#[test]
fn every_truncation_of_a_real_archive_is_refused() {
    for name in ["episode.zip", "pack.zip", "multi.zip", "movie.zip"] {
        let whole = corpus(name);
        let want = if name == "pack.zip" {
            Want::Episode {
                season: 1,
                episode: 1,
            }
        } else if name == "multi.zip" {
            Want::Episode {
                season: 9,
                episode: 5,
            }
        } else {
            Want::Single
        };
        assert!(pick(&whole, want, None).is_ok(), "{name} whole");
        for len in 0..whole.len() {
            assert!(
                pick(&whole[..len], want, None).is_err(),
                "{name} cut at {len}"
            );
        }
    }
}

#[test]
fn only_the_end_record_that_ends_the_file_counts() {
    let good = zip(&[("a.srt", CUE)]);
    // A second end record inside the archive's own comment: ignored.
    let mut commented = good.clone();
    let comment =
        b"PK\x05\x06\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00";
    let len = commented.len();
    commented[len - 2..].copy_from_slice(&(comment.len() as u16).to_le_bytes());
    commented.extend(comment);
    assert_eq!(single(&commented).unwrap(), CUE);
    // Junk after the end: no end record reaches the end of the file.
    let mut junk = good.clone();
    junk.extend(b"PK\x05\x06 trailing junk");
    assert_eq!(single(&junk), Err(ArchiveFault::NotZip));
    // No end record at all.
    let end = find(&good, b"PK\x05\x06");
    assert_eq!(single(&good[..end]), Err(ArchiveFault::NotZip));
}

#[test]
fn offsets_and_sizes_outside_the_file_are_refused() {
    let good = zip(&[("a.srt", CUE)]);
    let end = find(&good, b"PK\x05\x06");
    let central = find(&good, b"PK\x01\x02");
    let len = good.len() as u32;
    let patched = |at: usize, value: u32| {
        let mut bent = good.clone();
        bent[at..at + 4].copy_from_slice(&value.to_le_bytes());
        single(&bent)
    };
    let unreadable = Err(ArchiveFault::Unreadable);
    // A directory that does not end where the end record starts: no ZIP.
    let not_zip = Err(ArchiveFault::NotZip);
    assert_eq!(
        patched(end + 16, len + 100),
        not_zip,
        "the directory past the end"
    );
    assert_eq!(patched(end + 16, 1), not_zip, "the directory elsewhere");
    assert_eq!(
        patched(end + 12, 3),
        not_zip,
        "a directory of the wrong size"
    );
    assert_eq!(
        patched(central + 42, len + 100),
        unreadable,
        "a local header past the end"
    );
    assert_eq!(
        patched(central + 20, len),
        unreadable,
        "data running past the end"
    );
    let mut name = good.clone();
    name[central + 28..central + 30].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(single(&name), unreadable, "a name running past the end");
    let mut count = good.clone();
    count[end + 10..end + 12].copy_from_slice(&5u16.to_le_bytes());
    assert_eq!(single(&count), unreadable, "more entries than records");
}

#[test]
fn a_local_header_that_disagrees_is_caught_by_the_crc() {
    // A longer name in the local header moves where the data starts.
    let mut bent = zip_with(&[(b"a.srt", CUE)], STORED);
    let local = find(&bent, b"PK\x03\x04");
    bent[local + 26] += 2;
    assert_eq!(single(&bent), Err(ArchiveFault::Unreadable));
    // A changed byte of the data.
    let mut flipped = zip_with(&[(b"a.srt", CUE)], STORED);
    let at = flipped.windows(2).position(|w| w == b"hi").unwrap();
    flipped[at] = b'H';
    assert_eq!(single(&flipped), Err(ArchiveFault::Unreadable));
}

#[test]
fn entries_with_a_data_descriptor_read_from_the_central_directory() {
    for stored in [false, true] {
        let archive = zip_with(
            &[(b"a.srt", CUE)],
            Options {
                stored,
                descriptor: true,
            },
        );
        assert_eq!(single(&archive).unwrap(), CUE, "stored: {stored}");
    }
}

#[test]
fn encrypted_and_zip64_entries_are_refused() {
    let good = zip_with(&[(b"a.srt", CUE)], STORED);
    let central = find(&good, b"PK\x01\x02");
    let end = find(&good, b"PK\x05\x06");
    let unreadable = Err(ArchiveFault::Unreadable);
    let mut locked = good.clone();
    locked[central + 8] |= 1;
    assert_eq!(single(&locked), unreadable);
    for at in [central + 20, central + 24, central + 42] {
        let mut wide = good.clone();
        wide[at..at + 4].fill(0xff);
        assert_eq!(single(&wide), unreadable, "ZIP64 marker at {at}");
    }
    // In the end record: a ZIP64 directory offset leaves no matching end
    // record; a ZIP64 entry count is refused as such.
    let mut offset = good.clone();
    offset[end + 16..end + 20].fill(0xff);
    assert_eq!(single(&offset), Err(ArchiveFault::NotZip));
    let mut count = good.clone();
    count[end + 10..end + 12].fill(0xff);
    assert_eq!(single(&count), unreadable);
}

#[test]
fn names_are_only_matched_never_used_as_paths() {
    // Nothing is written to disk: a path in a name is only text to match.
    for name in [
        &b"../../etc/Show.S01E02.srt"[..],
        b"/tmp/Show.S01E02.srt",
        b"C:\\x\\Show.S01E02.srt",
        b"\xff\xfe Show.S01E02.srt",
    ] {
        let archive = zip_with(&[(name, CUE)], Options::default());
        let got = pick(
            &archive,
            Want::Episode {
                season: 1,
                episode: 2,
            },
            None,
        );
        assert_eq!(got.unwrap(), CUE, "{}", String::from_utf8_lossy(name));
    }
}
