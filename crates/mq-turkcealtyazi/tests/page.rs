//! The title page parser on pages recorded from TurkceAltyazi.org on
//! 2026-10-19 (tests/corpus, bench/m13/turkcealtyazi-research.jsonl).

use mq_turkcealtyazi::{Changed, Kind, Lang, Row, Source, TitlePage, title_page};

const BREAKING_BAD: (&str, &str, &str) = (
    "title-breaking-bad.html",
    "/mov/0903747/breaking-badd.html",
    "0903747",
);
const SIMPSONS: (&str, &str, &str) = (
    "title-simpsons.html",
    "/mov/0096697/the-simpsons.html",
    "0096697",
);
const MATRIX: (&str, &str, &str) = (
    "title-matrix.html",
    "/mov/0133093/the-matrix1.html",
    "0133093",
);
const NOT_FOUND: (&str, &str, &str) = ("title-404.html", "/mov/9999999/404-error.html", "9999999");

fn corpus(name: &str) -> String {
    let path = format!("{}/tests/corpus/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn rows((file, url, digits): (&str, &str, &str)) -> Vec<Row> {
    match title_page(url, digits, &corpus(file)) {
        Ok(TitlePage::Rows { rows, unparsed: 0 }) => rows,
        other => panic!("{file}: {other:?}"),
    }
}

fn find(rows: &[Row], altid: u32) -> &Row {
    rows.iter().find(|r| r.altid == altid).unwrap()
}

/// The rule that makes a change loud: every subtitle row of every recorded
/// page parses. A marker, flag or field the grammar misses fails here.
#[test]
fn every_row_of_every_recorded_page_parses() {
    for (page, count) in [(BREAKING_BAD, 17), (SIMPSONS, 11), (MATRIX, 5)] {
        let html = corpus(page.0);
        let anchors = html.matches(r#"href="/sub/"#).count();
        let rows = rows(page);
        assert_eq!(rows.len(), count, "{}", page.0);
        assert_eq!(rows.len(), anchors, "{}: a row went unread", page.0);
        for r in &rows {
            assert!(r.path.starts_with(&format!("/sub/{}/", r.altid)) && r.path.ends_with(".html"));
            assert!(
                !r.release.is_empty(),
                "{}: {} has no release line",
                page.0,
                r.altid
            );
        }
    }
}

#[test]
fn a_series_page_gives_episodes_packs_and_specials() {
    let rows = rows(BREAKING_BAD);
    let e01 = find(&rows, 108735);
    assert_eq!(
        e01.kind,
        Kind::Episode {
            season: 1,
            episode: 1
        }
    );
    assert_eq!(
        (
            e01.lang,
            e01.fps,
            e01.downloads,
            e01.release.as_str(),
            e01.translator.as_deref()
        ),
        (Lang::Turkish, Some(25.0), 6379, "BiA", Some("F471H"))
    );
    assert_eq!(e01.path, "/sub/108735/breaking-badd.html");
    let turkish_season_1: Vec<_> = rows
        .iter()
        .filter(|r| r.lang == Lang::Turkish && r.kind == Kind::Pack { season: 1 })
        .map(|r| r.altid)
        .collect();
    assert_eq!(
        turkish_season_1,
        [373225, 345388, 250740, 725317, 134774, 84404]
    );
    assert_eq!(find(&rows, 490870).kind, Kind::Special { season: 5 });
    assert_eq!(find(&rows, 490870).release, "All-Star Celebrity Bowling");
    assert_eq!(
        find(&rows, 398841).kind,
        Kind::Episode {
            season: 2,
            episode: 9
        }
    );
    assert_eq!(find(&rows, 121909).lang, Lang::English);
    assert!(rows.iter().all(|r| !r.hearing_impaired));
}

#[test]
fn lists_and_ranges_of_episodes_and_the_hearing_impaired_section() {
    let rows = rows(SIMPSONS);
    let episodes = |altid| match &find(&rows, altid).kind {
        Kind::Episodes { season, episodes } => (*season, episodes.clone()),
        other => panic!("{altid}: {other:?}"),
    };
    assert_eq!(episodes(160120), (9, vec![3, 4, 5, 6]), "a range");
    assert_eq!(episodes(239174), (9, vec![2, 7, 8, 10]), "a list");
    assert_eq!(episodes(793639), (33, vec![20, 21, 22]));
    assert_eq!(episodes(169405), (5, vec![5, 9, 11]));
    assert_eq!(episodes(239852), (5, vec![16, 17]));
    let range = find(&rows, 160120);
    assert_eq!((range.release.as_str(), range.downloads), ("TOPAZ", 182));
    assert_eq!(
        find(&rows, 238276).translator.as_deref(),
        Some("Iverson & HaNCeR")
    );
    // Marked by the row's own image (this recording has English ones only).
    let hearing: Vec<_> = rows
        .iter()
        .filter(|r| r.hearing_impaired)
        .map(|r| r.altid)
        .collect();
    assert_eq!(hearing, [250741, 237014, 279380]);
    assert!(
        rows.iter()
            .filter(|r| r.hearing_impaired)
            .all(|r| r.lang == Lang::English)
    );
}

#[test]
fn a_movie_page_gives_disc_counts() {
    let rows = rows(MATRIX);
    let movie = find(&rows, 152408);
    assert_eq!(movie.kind, Kind::Movie { discs: 1 });
    assert_eq!(
        (movie.downloads, movie.release.as_str()),
        (38861, "CaLLioPE_MoNTEDiaZ")
    );
    assert_eq!(find(&rows, 306655).kind, Kind::Movie { discs: 2 });
    assert_eq!(find(&rows, 762312).release, "RARBG (REMASTERED) 1080P");
    assert_eq!(find(&rows, 706049).lang, Lang::English);
}

#[test]
fn no_page_for_the_id_and_an_empty_list_are_nothing() {
    let (file, url, digits) = NOT_FOUND;
    assert_eq!(
        title_page(url, digits, &corpus(file)),
        Ok(TitlePage::Nothing)
    );
    // A real title whose list is empty.
    let (bb, bb_url, bb_digits) = BREAKING_BAD;
    assert_eq!(
        title_page(bb_url, bb_digits, &corpus(file)),
        Ok(TitlePage::Nothing)
    );
    // The site's own answer for an unknown id wins over whatever the page holds.
    assert_eq!(title_page(url, digits, &corpus(bb)), Ok(TitlePage::Nothing));
}

#[test]
fn only_a_title_page_path_is_read() {
    // The origin (https on the site's own host) is the site client's check.
    let (file, url, digits) = BREAKING_BAD;
    let html = corpus(file);
    for path in [
        "/index.php",
        "/mov/0903747/breaking-badd.php",
        "/mov/0903747/Breaking-Badd.html",
        "/mov/0903747/",
        "/sub/108735/breaking-badd.html",
    ] {
        assert_eq!(
            title_page(path, digits, &html),
            Err(Changed("the title page has a new address")),
            "{path}"
        );
    }
    // Another title's page is never read as this one's: nothing for this title.
    assert_eq!(
        title_page(&url.replace("0903747", "0903748"), digits, &html),
        Ok(TitlePage::OtherTitle)
    );
    assert_eq!(title_page(url, "903747", &html), Ok(TitlePage::OtherTitle));
}

#[test]
fn a_layout_change_is_loud_never_nothing() {
    let (file, url, digits) = BREAKING_BAD;
    let html = corpus(file);
    let read = |html: String| title_page(url, digits, &html);
    let unread = Err(Changed("most subtitle rows cannot be read"));
    // New flag classes: no row has a language we know.
    let flags = html.replace("flagtr", "flagxx").replace("flagen", "flagyy");
    assert_eq!(read(flags), unread);
    // Only the Turkish flag renamed: 15 of 17 rows unread.
    assert_eq!(read(html.replace("flagtr", "flagturk")), unread);
    // New marker markup on every row.
    let markers = html
        .replace(r#"<span class="cred">|</span>"#, "/")
        .replace("<b>Paket</b>", "<i>Paket</i>");
    assert_eq!(read(markers), unread);
    // Rows that no longer open the way they did.
    let openers = html.replace(r#"<div class="altsonsez"#, r#"<div class="altrow"#);
    assert_eq!(read(openers), unread);
    // Subtitle pages under a new path: the rows are still counted.
    assert_eq!(
        read(html.replace(r#"href="/sub/"#, r#"href="/altyazi/"#)),
        unread
    );
    // A new language cell: the links and openers still count the rows.
    let cells = html.replace(r#"<div class="aldil">"#, r#"<div class="lang">"#);
    assert_eq!(read(cells), unread);
    // Two of the three renamed at once: the third still counts the rows.
    let (link, cell, opener) = (
        (r#"href="/sub/"#, r#"href="/altyazi/"#),
        (r#"<div class="aldil">"#, r#"<div class="lang">"#),
        (r#"<div class="altsonsez"#, r#"<div class="altrow"#),
    );
    for (one, two) in [(link, cell), (link, opener), (cell, opener)] {
        let renamed = html.replace(one.0, one.1).replace(two.0, two.1);
        assert_eq!(read(renamed), unread, "{one:?} and {two:?}");
    }
    // The list itself renamed: its class in the page's CSS does not count.
    let gone = html.replace(
        r#"<div class="altyazi-list-wrapper">"#,
        r#"<div class="subtitle-list">"#,
    );
    assert!(
        gone.contains("altyazi-list-wrapper"),
        "the CSS still names it"
    );
    assert_eq!(read(gone), Err(Changed("the subtitle list is gone")));
}

#[test]
fn release_lines_are_plain_text_of_at_most_80_characters() {
    let (file, url, digits) = BREAKING_BAD;
    let long = format!("Extended &amp; {}", "x".repeat(120));
    let html = corpus(file).replacen("All-Star Celebrity Bowling", &long, 1);
    let Ok(TitlePage::Rows { rows, .. }) = title_page(url, digits, &html) else {
        panic!("a list");
    };
    let special = find(&rows, 490870);
    assert_eq!(special.release.chars().count(), 80);
    assert!(
        special.release.starts_with("Extended & xxx"),
        "{}",
        special.release
    );
}

#[test]
fn up_to_a_quarter_unread_still_lists_and_more_is_a_change() {
    let (file, url, digits) = BREAKING_BAD;
    let html = corpus(file);
    // 17 rows: 4 unread is under a quarter, 5 is over.
    let four = html.replacen("flagtr", "flagxx", 4);
    let Ok(TitlePage::Rows { rows, unparsed }) = title_page(url, digits, &four) else {
        panic!("still a list");
    };
    assert_eq!((rows.len(), unparsed), (13, 4));
    let five = html.replacen("flagtr", "flagxx", 5);
    assert_eq!(
        title_page(url, digits, &five),
        Err(Changed("most subtitle rows cannot be read"))
    );
}

#[test]
fn one_unreadable_row_is_counted_and_the_rest_still_list() {
    let (file, url, digits) = BREAKING_BAD;
    let html = corpus(file).replacen("E<b>01</b>", "E<b>Bir</b>", 1);
    let Ok(TitlePage::Rows { rows, unparsed }) = title_page(url, digits, &html) else {
        panic!("still a list");
    };
    assert_eq!((rows.len(), unparsed), (16, 1));
    assert!(rows.iter().all(|r| r.altid != 108735));
}

#[test]
fn the_source_mark_is_read_from_its_image() {
    let breaking_bad = rows(BREAKING_BAD);
    let source = |rows: &[Row], altid| find(rows, altid).source;
    assert_eq!(source(&breaking_bad, 373225), Some(Source::DvdRip));
    assert_eq!(source(&breaking_bad, 725317), Some(Source::WebDl));
    assert_eq!(source(&breaking_bad, 345388), Some(Source::Hd));
    assert_eq!(source(&breaking_bad, 108735), None);
    assert_eq!(source(&rows(SIMPSONS), 793639), Some(Source::WebRip));
    assert_eq!(source(&rows(MATRIX), 152408), Some(Source::BdRip));
    // The mark is an image: the release line stays the group's words.
    assert_eq!(find(&breaking_bad, 725317).release, "NETFLiX");
    // A code the sprite has no image for is no source; the row still lists.
    let (file, url, digits) = BREAKING_BAD;
    let html = corpus(file).replacen(r#"class="rps r2""#, r#"class="rps r99""#, 1);
    let Ok(TitlePage::Rows { rows, unparsed: 0 }) = title_page(url, digits, &html) else {
        panic!("a list");
    };
    assert_eq!(find(&rows, 373225).source, None);
}

#[test]
fn translators_are_capped_and_a_page_of_too_many_rows_is_a_change() {
    let (file, url, digits) = BREAKING_BAD;
    let html = corpus(file).replace("F471H", &"y".repeat(120));
    let Ok(TitlePage::Rows { rows, .. }) = title_page(url, digits, &html) else {
        panic!("a list");
    };
    let by = find(&rows, 108735).translator.clone().unwrap();
    assert_eq!(by.chars().count(), 80, "{by}");
    // More rows than any title has is not the site's list.
    let opener = r#"<div class="altsonsez"#;
    let flood = format!("{}{opener}", format!("{opener}\"></div>").repeat(2001));
    let html = corpus(file).replacen(opener, &flood, 1);
    assert_eq!(
        title_page(url, digits, &html),
        Err(Changed("the list has more rows than any title"))
    );
}
