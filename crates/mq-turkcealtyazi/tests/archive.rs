//! Picking the subtitle out of the archives TurkceAltyazi.org sends,
//! recorded 2026-10-19 (tests/corpus, trimmed to a few cues each with the
//! real entry names and encodings), and the hostile archives the reader
//! must refuse without a panic. Memory under bombs and random bytes is
//! measured in tests/archive_alloc.rs.

mod common;

use common::{Options, corpus, find, zip, zip_with};
use mq_relparse::Cut;
use mq_turkcealtyazi::{ArchiveFault, MAX_ARCHIVE, MAX_ENTRY, Playing, Want, pick};

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
    pick(archive, Want::Single, &Playing::default())
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
        let got = pick(
            &pack,
            Want::FromSeveral { season: 1, episode },
            &Playing::default(),
        )
        .unwrap();
        assert!(
            got.starts_with(b"\xef\xbb\xbf"),
            "UTF-8 with BOM, kept for to_vtt"
        );
        assert!(starts(&got, cue), "S01E{episode:02}");
    }
    for (season, episode) in [(1, 8), (2, 5)] {
        assert_eq!(
            pick(
                &pack,
                Want::FromSeveral { season, episode },
                &Playing::default()
            ),
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
        let got = pick(
            &multi,
            Want::FromSeveral { season: 9, episode },
            &Playing::default(),
        )
        .unwrap();
        assert!(starts(&got, cue), "S09E{episode:02}");
    }
    assert_eq!(
        pick(
            &multi,
            Want::FromSeveral {
                season: 9,
                episode: 7
            },
            &Playing::default()
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
        pick(
            &two,
            Want::Single,
            &Playing {
                group: Some("beta"),
                ..Playing::default()
            }
        )
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
            Want::FromSeveral {
                season: 1,
                episode: 1,
            }
        } else if name == "multi.zip" {
            Want::FromSeveral {
                season: 9,
                episode: 5,
            }
        } else {
            Want::Single
        };
        assert!(
            pick(&whole, want, &Playing::default()).is_ok(),
            "{name} whole"
        );
        for len in 0..whole.len() {
            assert!(
                pick(&whole[..len], want, &Playing::default()).is_err(),
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
            Want::FromSeveral {
                season: 1,
                episode: 2,
            },
            &Playing::default(),
        );
        assert_eq!(got.unwrap(), CUE, "{}", String::from_utf8_lossy(name));
    }
}

/// A one-cue file whose text is `says`.
fn says(says: &str) -> Vec<u8> {
    format!("1\n00:00:01,000 --> 00:00:02,000\n{says}\n").into_bytes()
}

fn from_several(archive: &[u8], season: u32, episode: u32) -> Result<Vec<u8>, ArchiveFault> {
    pick(
        archive,
        Want::FromSeveral { season, episode },
        &Playing::default(),
    )
}

#[test]
fn turkish_names_say_the_season_and_the_episode() {
    let (e1, e2, e7) = (says("e1"), says("e2"), says("e7"));
    let pack = zip(&[
        ("2. Sezon 1. Bölüm.srt", &e1),
        ("2. Sezon 7. Bölüm.srt", &e7),
        ("2. Sezon 2. Bölüm.srt", &e2),
    ]);
    // The release parser alone reads "2. Sezon 7" as episode 2 of no season.
    assert_eq!(from_several(&pack, 2, 7), Ok(e7.clone()));
    assert_eq!(from_several(&pack, 2, 2), Ok(e2.clone()));
    assert_eq!(
        from_several(&pack, 3, 7),
        Err(ArchiveFault::NoEpisode {
            season: 3,
            episode: 7
        })
    );
    let numbered = zip(&[("Bölüm 04.srt", &e1), ("Bölüm 05.srt", &e7)]);
    assert_eq!(from_several(&numbered, 1, 5), Ok(e7.clone()));
    // The same names in Windows-1254 bytes, as the site's archives keep them.
    let legacy = zip_with(
        &[
            (b"2. Sezon 1. B\xf6l\xfcm.srt", &e1),
            (b"2. Sezon 7. B\xf6l\xfcm.srt", &e7),
        ],
        Options::default(),
    );
    assert_eq!(from_several(&legacy, 2, 7), Ok(e7));
    assert_eq!(from_several(&legacy, 2, 1), Ok(e1));
}

#[test]
fn a_pack_whose_names_all_read_as_one_episode_is_refused() {
    let names = [("Dizi.E01.TOPAZ.srt", CUE), ("Dizi.E01.DIMENSION.srt", CUE)];
    assert_eq!(from_several(&zip(&names), 1, 1), Err(ArchiveFault::Unclear));
    // A single episode's row may hold its episode twice: the group picks.
    let two = zip(&[
        ("Dizi.E01.TOPAZ.srt", &says("topaz")[..]),
        ("Dizi.E01.DIMENSION.srt", &says("dimension")[..]),
    ]);
    let topaz = Playing {
        group: Some("TOPAZ"),
        ..Playing::default()
    };
    let episode = Want::Episode {
        season: 1,
        episode: 1,
    };
    assert_eq!(pick(&two, episode, &topaz), Ok(says("topaz")));
}

#[test]
fn a_single_episode_row_takes_its_own_episode_from_the_archive() {
    let e6 = Want::Episode {
        season: 1,
        episode: 6,
    };
    let none = Playing::default();
    let both = zip(&[
        ("Show.S01E05.srt", &says("e5")[..]),
        ("Show.S01E06.srt", &says("e6")[..]),
    ]);
    assert_eq!(pick(&both, e6, &none), Ok(says("e6")));
    // No name says an episode: the best file is the row's.
    let plain = zip(&[("altyazi.srt", CUE), ("altyazi.ass", b"[Events]\n")]);
    assert_eq!(pick(&plain, e6, &none), Ok(CUE.to_vec()));
    // One file is the row's, whatever its name says: the site's numbering
    // and the release's may differ.
    let other = zip(&[("Show.S01E05.srt", CUE)]);
    assert_eq!(pick(&other, e6, &none), Ok(CUE.to_vec()));
    // Several that all say other episodes: none of them.
    let others = zip(&[("Show.S01E05.srt", CUE), ("Show.S01E07.srt", CUE)]);
    assert_eq!(
        pick(&others, e6, &none),
        Err(ArchiveFault::NoEpisode {
            season: 1,
            episode: 6
        })
    );
}

#[test]
fn dotted_turkish_names_and_a_number_that_stands_alone() {
    let (e5, e6) = (says("e5"), says("e6"));
    for (five, six, season) in [
        ("Show.S02.Bolum.05.srt", "Show.S02.Bolum.06.srt", 2),
        ("Sezon.2.Bolum.5.srt", "Sezon.2.Bolum.6.srt", 2),
        (
            "Kurtlar.Vadisi.Bolum.05.srt",
            "Kurtlar.Vadisi.Bolum.06.srt",
            1,
        ),
        ("Show_2_Sezon_5_Bölüm.srt", "Show_2_Sezon_6_Bölüm.srt", 2),
    ] {
        let pack = zip(&[(five, &e5), (six, &e6)]);
        assert_eq!(from_several(&pack, season, 5), Ok(e5.clone()), "{five}");
        assert_eq!(from_several(&pack, season, 6), Ok(e6.clone()), "{six}");
    }
    // S01E05 is read before a later number: "Bölüm 1080p" is no episode.
    let hd = zip(&[
        ("Show S01E05 Bölüm 1080p.srt", &e5[..]),
        ("Show S01E06 Bölüm 1080p.srt", &e6[..]),
    ]);
    assert_eq!(from_several(&hd, 1, 5), Ok(e5.clone()));
    // A pack that also counts the series' episodes: S01E05 says which.
    let counted = zip(&[
        ("Show.S01E05.Bolum.12.srt", &e5[..]),
        ("Show.S01E06.Bolum.12.srt", &e6[..]),
    ]);
    assert_eq!(from_several(&counted, 1, 6), Ok(e6));
}

#[test]
fn one_file_in_two_formats_is_not_unclear() {
    let list = zip(&[
        ("Show.S05E16E17.srt", CUE),
        ("Show.S05E16E17.ass", &b"[Events]\n"[..]),
    ]);
    assert_eq!(from_several(&list, 5, 17), Ok(CUE.to_vec()));
}

#[test]
fn an_unreadable_text_file_hides_nothing_behind_it() {
    let mut archive = zip_with(
        &[(b"a-notu.txt", b"Iyi seyirler"), (b"b-altyazi.txt", CUE)],
        STORED,
    );
    // The first file's bytes no longer match its CRC.
    let data = find(&archive, b"PK\x03\x04") + 30 + b"a-notu.txt".len();
    archive[data] ^= 0xff;
    assert_eq!(single(&archive), Ok(CUE.to_vec()));
}

#[test]
fn the_playing_cut_picks_among_the_files() {
    let cuts = zip(&[
        (
            "Movie.2008.Extended.1080p.BluRay.srt",
            &says("extended")[..],
        ),
        (
            "Movie.2008.Theatrical.1080p.BluRay.srt",
            &says("theatrical")[..],
        ),
    ]);
    let with = |cut| {
        pick(
            &cuts,
            Want::Single,
            &Playing {
                cut,
                ..Playing::default()
            },
        )
    };
    assert_eq!(with(Some(Cut::Extended)), Ok(says("extended")));
    assert_eq!(with(Some(Cut::Theatrical)), Ok(says("theatrical")));
    // A release that names no cut is the theatrical one.
    assert_eq!(with(None), Ok(says("theatrical")));
}

#[test]
fn mac_shadows_are_skipped_and_subrip_saved_as_text_is_read() {
    let shadow = zip(&[
        ("._Show.S01E05.srt", &b"\x00\x05\x16\x07 Mac metadata"[..]),
        ("Show.S01E05.srt", CUE),
    ]);
    assert_eq!(single(&shadow), Ok(CUE.to_vec()));
    let text = zip(&[
        ("9x5 çeviri notu.txt", &b"Iyi seyirler"[..]),
        ("Show.S01E05.txt", CUE),
    ]);
    assert_eq!(single(&text), Ok(CUE.to_vec()));
    let notes = zip(&[("notu.txt", &b"Iyi seyirler"[..])]);
    assert_eq!(single(&notes), Err(ArchiveFault::NoSubtitle));
    // A real subtitle always wins over a text file.
    let both = zip(&[
        ("Show.S01E05.txt", &says("text")[..]),
        ("Show.S01E05.srt", CUE),
    ]);
    assert_eq!(single(&both), Ok(CUE.to_vec()));
}

#[test]
fn an_archive_must_start_as_a_zip() {
    // A whole archive behind a stub, its offsets moved to match (as a
    // self-extractor writes it): readable, and still not taken.
    let plain = zip(&[("Show.S01E05.srt", CUE)]);
    let stub = b"<html>";
    let mut shifted = stub.to_vec();
    shifted.extend(&plain);
    let shift = |bytes: &mut Vec<u8>, at: usize| {
        let value = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        bytes[at..at + 4].copy_from_slice(&(value + stub.len() as u32).to_le_bytes());
    };
    let central = find(&shifted, b"PK\x01\x02");
    shift(&mut shifted, central + 42);
    let end = find(&shifted, b"PK\x05\x06");
    shift(&mut shifted, end + 16);
    assert_eq!(single(&shifted), Err(ArchiveFault::NotZip));
}
