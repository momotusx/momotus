//! The recorded files through `to_vtt`, the converter every played
//! subtitle goes through (plans/m13-turkcealtyazi.md 5.2): Windows-1254 and
//! UTF-8 with a BOM, as the site's archives hold them.

mod common;

use common::corpus;
use mq_server::to_vtt;
use mq_turkcealtyazi::{Playing, Want, pick};

fn vtt(archive: &str, want: Want) -> String {
    let file = pick(&corpus(archive), want, &Playing::default()).unwrap();
    to_vtt(&file, "tur").unwrap()
}

#[test]
fn the_episode_file_reads_in_turkish() {
    let episode = Want::Episode {
        season: 1,
        episode: 1,
    };
    let text = vtt("episode.zip", episode);
    assert!(
        text.starts_with(
            "WEBVTT\n\n1\n00:02:06.573 --> 00:02:08.487\nBenim adım Walter Hartwell White.\n"
        ),
        "{text}"
    );
}

#[test]
fn the_movie_file_decodes_from_windows_1254() {
    let text = vtt("movie.zip", Want::Single);
    assert!(text.contains("Transfer çağrısı: Alındı."), "{text}");
    assert!(text.contains("Her şey"), "{text}");
}

#[test]
fn the_pack_entry_loses_its_bom() {
    let e01 = Want::FromSeveral {
        season: 1,
        episode: 1,
    };
    let text = vtt("pack.zip", e01);
    assert!(
        text.starts_with("WEBVTT\n\n1\n00:01:08.135 -->"),
        "{text:?}"
    );
    assert!(!text.contains('\u{feff}'));
}
