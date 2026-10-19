//! Ranking the recorded rows against a playing release, and the labels.

use mq_relparse::Cut;
use mq_turkcealtyazi::{Kind, Lang, Playing, Row, Target, TitlePage, Track, title_page, tracks};

fn rows(file: &str, url: &str, digits: &str) -> Vec<Row> {
    let path = format!("{}/tests/corpus/{file}", env!("CARGO_MANIFEST_DIR"));
    match title_page(url, digits, &std::fs::read_to_string(path).unwrap()) {
        Ok(TitlePage::Rows { rows, .. }) => rows,
        other => panic!("{other:?}"),
    }
}

fn breaking_bad() -> Vec<Row> {
    rows(
        "title-breaking-bad.html",
        "/mov/0903747/breaking-badd.html",
        "0903747",
    )
}

fn ids(tracks: &[Track]) -> Vec<u32> {
    tracks.iter().map(|t| t.row.altid).collect()
}

const S01E01: Target = Target::Episode {
    season: 1,
    episode: 1,
};

#[test]
fn an_episode_gets_its_own_file_then_the_season_packs_by_downloads() {
    let got = tracks(&breaking_bad(), S01E01, &Playing::default());
    assert_eq!(
        ids(&got),
        [108735, 373225, 345388, 250740, 84404, 134774, 725317]
    );
    assert_eq!(got[0].label, "BiA, 25 fps, by F471H");
    assert_eq!(got[1].label, "ORPHEUS, 23.976 fps, season 1 pack");
    assert!(got.iter().all(|t| t.row.lang == Lang::Turkish));
}

#[test]
fn the_playing_group_and_frame_rate_come_first() {
    let rows = breaking_bad();
    let orpheus = tracks(
        &rows,
        S01E01,
        &Playing {
            group: Some("ORPHEUS"),
            ..Playing::default()
        },
    );
    assert_eq!(orpheus[0].row.altid, 373225);
    // CtrlHD sits inside "AVC / CtrlHD / Visionx" and "x264-CtrlHD".
    let ctrlhd = tracks(
        &rows,
        S01E01,
        &Playing {
            group: Some("ctrlhd"),
            ..Playing::default()
        },
    );
    assert_eq!(ids(&ctrlhd)[..2], [345388, 134774]);
    // At 23.976 fps the 25 fps files go last, the episode's own first of them.
    let fps = tracks(
        &rows,
        S01E01,
        &Playing {
            fps: Some(23.976),
            ..Playing::default()
        },
    );
    assert_eq!(
        ids(&fps),
        [373225, 345388, 250740, 84404, 725317, 108735, 134774]
    );
}

#[test]
fn a_few_episodes_and_the_pack_for_an_episode_inside_them() {
    let simpsons = rows(
        "title-simpsons.html",
        "/mov/0096697/the-simpsons.html",
        "0096697",
    );
    let got = tracks(
        &simpsons,
        Target::Episode {
            season: 9,
            episode: 5,
        },
        &Playing::default(),
    );
    assert_eq!(ids(&got), [160120, 238276]);
    assert_eq!(
        got[0].label,
        "TOPAZ, 23.976 fps, episodes 3 to 6, by Iverson"
    );
    let e08 = tracks(
        &simpsons,
        Target::Episode {
            season: 9,
            episode: 8,
        },
        &Playing::default(),
    );
    assert_eq!(ids(&e08), [239174, 238276]);
    assert_eq!(e08[0].label, "TPZ, 23.976 fps, episodes 2, 7, 8 and 10");
}

#[test]
fn a_movie_gets_one_part_files_only() {
    let matrix = rows(
        "title-matrix.html",
        "/mov/0133093/the-matrix1.html",
        "0133093",
    );
    let got = tracks(&matrix, Target::Movie, &Playing::default());
    assert_eq!(ids(&got), [152408, 120471, 762312]);
    // RARBG has the fewest downloads; playing it, its row leads.
    let rarbg = tracks(
        &matrix,
        Target::Movie,
        &Playing {
            group: Some("RARBG"),
            ..Playing::default()
        },
    );
    assert_eq!(rarbg[0].row.altid, 762312);
    let prodji = tracks(
        &matrix,
        Target::Movie,
        &Playing {
            group: Some("PRoDJi"),
            ..Playing::default()
        },
    );
    assert_eq!(prodji[0].row.altid, 120471);
}

#[test]
fn the_same_cut_comes_first_and_no_cut_matches_no_cut() {
    let row = |altid: u32, release: &str, downloads: u32| Row {
        altid,
        path: format!("/sub/{altid}/x.html"),
        lang: Lang::Turkish,
        kind: Kind::Movie { discs: 1 },
        release: release.into(),
        fps: None,
        downloads,
        translator: None,
        hearing_impaired: false,
    };
    let rows = [
        row(1, "BluRay", 500),
        row(2, "Extended Cut BluRay", 10),
        row(3, "THEATRICAL", 50),
    ];
    let extended = tracks(
        &rows,
        Target::Movie,
        &Playing {
            cut: Some(Cut::Extended),
            ..Playing::default()
        },
    );
    assert_eq!(ids(&extended), [2, 1, 3]);
    let theatrical = tracks(
        &rows,
        Target::Movie,
        &Playing {
            cut: Some(Cut::Theatrical),
            ..Playing::default()
        },
    );
    assert_eq!(ids(&theatrical)[0], 3);
    assert_eq!(
        ids(&tracks(&rows, Target::Movie, &Playing::default())),
        [1, 3, 2]
    );
}

#[test]
fn a_group_with_a_dash_or_an_underscore_matches_whole() {
    let row = |altid: u32, release: &str, downloads: u32| Row {
        altid,
        path: format!("/sub/{altid}/x.html"),
        lang: Lang::Turkish,
        kind: Kind::Movie { discs: 1 },
        release: release.into(),
        fps: None,
        downloads,
        translator: None,
        hearing_impaired: false,
    };
    let rows = [
        row(1, "BluRay", 500),
        row(2, "D-Z0N3", 10),
        row(3, "CaLLioPE_MoNTEDiaZ", 5),
    ];
    for (group, first) in [("D-Z0N3", 2), ("CaLLioPE_MoNTEDiaZ", 3), ("MoNTEDiaZ", 3)] {
        let got = tracks(
            &rows,
            Target::Movie,
            &Playing {
                group: Some(group),
                ..Playing::default()
            },
        );
        assert_eq!(got[0].row.altid, first, "{group}");
    }
}

#[test]
fn at_most_eight_tracks_and_never_a_special_or_english() {
    let mut many = breaking_bad();
    let pack = many.iter().find(|r| r.altid == 373225).unwrap().clone();
    for i in 0..10 {
        many.push(Row {
            altid: 900_000 + i,
            ..pack.clone()
        });
    }
    assert_eq!(tracks(&many, S01E01, &Playing::default()).len(), 8);
    let specials = tracks(
        &breaking_bad(),
        Target::Episode {
            season: 5,
            episode: 1,
        },
        &Playing::default(),
    );
    assert_eq!(
        ids(&specials),
        [725388],
        "the season 5 pack, never the specials"
    );
}
