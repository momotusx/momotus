use mq_protocol::decode_streams;
use mq_relparse::{Hdr, VideoCodec};
use mq_streams::*;
use serde_json::json;

fn streams(addon: &str, rank: u16, body: serde_json::Value) -> Vec<ParsedStream> {
    let resp = decode_streams(body.to_string().as_bytes()).unwrap();
    resp.streams
        .iter()
        .enumerate()
        .map(|(i, w)| normalize(w, addon, addon, rank, i as u32))
        .collect()
}

/// A device with the torrent engine and no debrid account; nothing else
/// known (the speed priors, no runtime).
fn engine() -> &'static Conditions {
    static C: std::sync::LazyLock<Conditions> = std::sync::LazyLock::new(|| Conditions {
        engine: true,
        ..Conditions::default()
    });
    &C
}

/// The same device with a debrid account.
fn with_account() -> &'static Conditions {
    static C: std::sync::LazyLock<Conditions> = std::sync::LazyLock::new(|| Conditions {
        engine: true,
        debrid: true,
        ..Conditions::default()
    });
    &C
}

fn target() -> Target {
    Target {
        title: "The Last of Us".into(),
        year: Some(2023),
        season: Some(2),
        episode: Some(3),
    }
}

fn caps_4k_dv() -> DeviceCaps {
    DeviceCaps {
        display_height: 2160,
        display_hdr: vec![Hdr::DolbyVision, Hdr::Hdr10],
        decoders: vec![VideoCodec::H264, VideoCodec::H265, VideoCodec::Av1],
        ten_bit_hevc: true,
        passthrough: vec![mq_relparse::Audio::Eac3, mq_relparse::Audio::Atmos],
        has_fallback_engine: true,
        ..DeviceCaps::basic_1080p()
    }
}

fn sample() -> Vec<ParsedStream> {
    let mut all = streams(
        "torrentio",
        0,
        json!({ "streams": [
            { "name": "Torrentio\n4k DV", "title": "The.Last.of.Us.S02E03.2160p.WEB-DL.DDP5.1.Atmos.DV.H.265-FLUX\n👤 120 💾 8.4 GB ⚙️ TorrentGalaxy",
              "infoHash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "fileIdx": 0, "behaviorHints": { "bingeGroup": "torrentio|4k|FLUX" } },
            { "name": "Torrentio\n1080p", "title": "The.Last.of.Us.S02E03.1080p.WEB.h264-ETHEL\n👤 900 💾 2.1 GB ⚙️ 1337x",
              "infoHash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "fileIdx": 0 },
            { "name": "Torrentio\n4k", "title": "The.Last.of.Us.S02E04.2160p.WEB-DL.DV.H.265-FLUX\n👤 80 💾 8.0 GB",
              "infoHash": "cccccccccccccccccccccccccccccccccccccccc", "fileIdx": 0 },
            { "name": "Torrentio\nCAM", "title": "Another.Show.S02E03.720p.HDTV.x264\n👤 5 💾 0.6 GB",
              "infoHash": "dddddddddddddddddddddddddddddddddddddddd", "fileIdx": 0 }
        ]}),
    );
    all.extend(streams("debrid-addon", 1, json!({ "streams": [
        { "name": "RD 4K", "title": "The.Last.of.Us.S02E03.2160p.WEB-DL.DDP5.1.Atmos.DV.H.265-FLUX",
          "clientResolve": { "type": "debrid", "service": "realdebrid", "infoHash": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "fileIdx": 0, "isCached": true },
          "behaviorHints": { "cached": true, "service": "realdebrid", "videoSize": 8400000000u64, "bingeGroup": "torrentio|4k|FLUX" } },
        { "name": "nzb", "nzbUrl": "https://indexer.example/get.nzb", "servers": [] }
    ]})));
    all
}

#[test]
fn cached_debrid_wins_and_duplicates_become_failover() {
    let prefs = StreamPrefs::default();
    let input = RankInput {
        caps: &caps_4k_dv(),
        prefs: &prefs,
        target: &target(),
        last_binge_group: None,
        kids: None,
        conditions: with_account(),
    };
    let ranked = rank(sample(), &input);
    let best = ranked.best().unwrap();
    assert_eq!(best.stream.addon_id, "debrid-addon");
    assert_eq!(best.class, SourceClass::Ready);
    assert_eq!(best.key.start, 4);
    assert!(best.reasons.contains(&Reason::ReadyNow));
    assert!(best.reasons.len() <= 3);
    // The Torrentio copy of the same hash collapses into the debrid row.
    assert_eq!(best.failover.len(), 1);
    assert_eq!(best.failover[0].addon_id, "torrentio");
    assert_eq!(
        best.failover[0].key,
        "bt:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:0"
    );

    let hidden: Vec<_> = ranked.hidden.iter().map(|h| h.reason).collect();
    assert!(hidden.contains(&HideReason::WrongEpisode), "{hidden:?}");
    assert!(hidden.contains(&HideReason::WrongTitle), "{hidden:?}");
    assert!(
        hidden.contains(&HideReason::UnsupportedSource),
        "{hidden:?}"
    );
}

#[test]
fn effective_resolution_follows_the_screen() {
    let prefs = StreamPrefs::default();
    let caps = DeviceCaps::basic_1080p();
    let input = RankInput {
        caps: &caps,
        prefs: &prefs,
        target: &target(),
        last_binge_group: None,
        kids: None,
        conditions: engine(),
    };
    // Only torrents: 4K DV and 1080p compete on a 1080p SDR stick.
    let only_torrents: Vec<_> = sample()
        .into_iter()
        .filter(|s| s.addon_id == "torrentio")
        .collect();
    let ranked = rank(only_torrents, &input);
    let heights: Vec<_> = ranked.rows.iter().map(|r| r.key.effective_height).collect();
    assert!(heights.iter().all(|h| *h <= 1080), "{heights:?}");
    let best = ranked.best().unwrap();
    // The 1080p H.264 release plays natively; 4K DV needs the fallback engine to tone-map.
    assert!(
        best.stream.raw_description.contains("1080p"),
        "{}",
        best.stream.raw_description
    );
    assert_eq!(best.key.playability, Playability::Native);
}

#[test]
fn binge_group_and_preferences_apply() {
    let prefs = StreamPrefs {
        avoid: vec!["flux".into()],
        ..Default::default()
    };
    let input = RankInput {
        caps: &caps_4k_dv(),
        prefs: &prefs,
        target: &target(),
        last_binge_group: None,
        kids: None,
        conditions: engine(),
    };
    let ranked = rank(sample(), &input);
    assert!(
        ranked
            .rows
            .iter()
            .all(|r| !r.stream.raw_description.to_lowercase().contains("flux"))
    );
    assert!(
        ranked
            .hidden
            .iter()
            .any(|h| h.reason == HideReason::Excluded)
    );

    // Binge decides among rows that tie on size and start (plans/m13 3.5,
    // question 2): with no size limit and 20 seeders wanted, the 8.4 GB
    // FLUX (120 seeders) and the 1080p (900) both start quickly.
    let prefs = StreamPrefs {
        size_limit_bytes: None,
        min_seeders: 20,
        ..StreamPrefs::default()
    };
    let only_torrents: Vec<_> = sample()
        .into_iter()
        .filter(|s| s.addon_id == "torrentio")
        .collect();
    let input = RankInput {
        caps: &caps_4k_dv(),
        prefs: &prefs,
        target: &target(),
        last_binge_group: Some("torrentio|4k|FLUX"),
        kids: None,
        conditions: engine(),
    };
    let ranked = rank(only_torrents, &input);
    let best = ranked.best().unwrap();
    assert!(best.key.binge);
    assert!(best.reasons.contains(&Reason::SameReleaseAsLast));
}

#[test]
/// Each key field outranks every field after it, in the order of
/// plans/m13-stream-choice.md 3.5 (as built): field `i` best and the rest
/// worst beats field `i` worst and every later field best.
fn each_key_field_outranks_every_later_one() {
    let compose = |best: &dyn Fn(usize) -> bool| RankKey {
        language: best(0),
        legit: best(1),
        viable: best(2),
        year_ok: best(3),
        sustains: best(4),
        pace: if best(5) { 3 } else { 0 },
        covered: if best(6) { 1000 } else { 0 },
        playability: if best(7) {
            Playability::Native
        } else {
            Playability::NeedsTranscode
        },
        size_ok: best(8),
        tier: if best(9) { 3 } else { 0 },
        binge: best(10),
        effective_height: if best(11) { 2160 } else { 0 },
        start: if best(12) { 4 } else { 0 },
        within: if best(13) { 1000 } else { 0 },
    };
    for i in 0..14 {
        assert!(
            compose(&|k| k == i) > compose(&|k| k > i),
            "field {i} must outrank every later field"
        );
    }
}

#[test]
fn title_similarity_tolerates_tags_but_rejects_other_works() {
    assert!(title_similarity("last us", "last us") > 0.99);
    assert!(title_similarity("another show", "last us") < 0.5);
}

#[test]
fn a_4k_remux_does_not_win_on_a_1080p_screen() {
    let all = streams(
        "torrentio",
        0,
        json!({ "streams": [
            { "name": "Torrentio\n4k", "title": "The.Last.of.Us.S02E03.2160p.BluRay.REMUX.HEVC.DTS-HD.MA.TrueHD.7.1.Atmos-FGT\n👤 40 💾 33.4 GB",
              "infoHash": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "fileIdx": 0 },
            { "name": "Torrentio\n1080p", "title": "The.Last.of.Us.S02E03.1080p.BluRay.x264-DEMAND\n👤 300 💾 3.1 GB",
              "infoHash": "ffffffffffffffffffffffffffffffffffffffff", "fileIdx": 0 }
        ]}),
    );
    let prefs = StreamPrefs::default();
    let caps = DeviceCaps::basic_1080p();
    let ranked = rank(
        all,
        &RankInput {
            caps: &caps,
            prefs: &prefs,
            target: &target(),
            last_binge_group: None,
            kids: None,
            conditions: engine(),
        },
    );
    let best = ranked.best().unwrap();
    assert!(
        best.stream.raw_description.contains("1080p"),
        "{}",
        best.stream.raw_description
    );
    assert!(best.reasons.contains(&Reason::MatchesScreen));
    let remux = ranked
        .rows
        .iter()
        .find(|r| r.stream.raw_description.contains("2160p"))
        .unwrap();
    assert!(!remux.reasons.contains(&Reason::MatchesScreen));
    assert!(
        ranked.rows.iter().all(|r| r.group != Group::NeedsDownload),
        "no ready alternative exists"
    );
}

/// A browser plays H.264, VP9 and AV1 in MP4 or WebM with AAC or Opus, and
/// has no second engine: a DTS-HD MKV remux is not something it can play.
#[test]
fn a_browser_is_not_offered_what_it_cannot_decode() {
    let caps = DeviceCaps {
        decoders: vec![VideoCodec::H264, VideoCodec::Vp9, VideoCodec::Av1],
        audio_decoders: vec![mq_relparse::Audio::Aac, mq_relparse::Audio::Opus],
        containers: vec!["mp4".into(), "webm".into()],
        has_fallback_engine: false,
        ..DeviceCaps::basic_1080p()
    };
    let prefs = StreamPrefs::default();
    let input = RankInput {
        caps: &caps,
        prefs: &prefs,
        target: &target(),
        last_binge_group: None,
        kids: None,
        conditions: engine(),
    };
    let all = streams(
        "torrentio",
        0,
        json!({ "streams": [
            { "name": "Torrentio\n1080p", "title": "The.Last.of.Us.S02E03.1080p.BluRay.REMUX.AVC.DTS-HD.MA.5.1-GROUP.mkv\n👤 300 💾 13.9 GB",
              "infoHash": "dddddddddddddddddddddddddddddddddddddddd", "fileIdx": 0 },
            { "name": "Torrentio\n1080p", "title": "The.Last.of.Us.S02E03.1080p.WEB.h264.AAC-GROUP.mp4\n👤 40 💾 1.9 GB",
              "infoHash": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee", "fileIdx": 0 }
        ]}),
    );
    let ranked = rank(all, &input);
    let best = ranked.best().expect("the MP4 plays");
    assert!(
        best.stream.raw_description.contains("AAC"),
        "{}",
        best.stream.raw_description
    );
    assert_eq!(best.key.playability, Playability::Native);
    assert!(
        ranked
            .hidden
            .iter()
            .any(|h| h.reason == HideReason::Unplayable),
        "the remux is hidden as unplayable"
    );
}

/// A well-seeded release in a language the user does not speak never beats
/// one in their language (the design audit found a Russian-only file picked
/// over an English 720p).
#[test]
fn a_foreign_language_release_never_wins_on_seeds() {
    let caps = DeviceCaps::basic_1080p();
    let prefs = StreamPrefs {
        languages: vec!["en".into()],
        ..StreamPrefs::default()
    };
    let input = RankInput {
        caps: &caps,
        prefs: &prefs,
        target: &target(),
        last_binge_group: None,
        kids: None,
        conditions: engine(),
    };
    let all = streams(
        "torrentio",
        0,
        json!({ "streams": [
            { "name": "Torrentio\n1080p", "title": "The.Last.of.Us.S02E03.1080p.WEB.h264-GROUP.mkv\n👤 900 💾 1.4 GB\n🇷🇺",
              "infoHash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "fileIdx": 0 },
            { "name": "Torrentio\n720p", "title": "The.Last.of.Us.S02E03.720p.WEB.h264-GROUP.mkv\n👤 12 💾 900 MB",
              "infoHash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "fileIdx": 0 }
        ]}),
    );
    let ranked = rank(all, &input);
    let best = ranked.best().unwrap();
    assert!(
        best.stream.raw_description.contains("720p"),
        "{}",
        best.stream.raw_description
    );
}

/// A release that states no resolution ranks below one that does, among
/// rows that start as well: both have 200 or more seeders (seeder bands rank
/// above resolution, plans/m13-stream-choice.md 3.5).
#[test]
fn an_unstated_resolution_ranks_below_a_stated_one() {
    let caps = DeviceCaps::basic_1080p();
    let prefs = StreamPrefs::default();
    let input = RankInput {
        caps: &caps,
        prefs: &prefs,
        target: &target(),
        last_binge_group: None,
        kids: None,
        conditions: engine(),
    };
    let all = streams(
        "torrentio",
        0,
        json!({ "streams": [
            { "name": "Torrentio", "title": "The.Last.of.Us.S02E03.WEB.h264-GROUP.mkv\n👤 900 💾 1.4 GB",
              "infoHash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "fileIdx": 0 },
            { "name": "Torrentio\n480p", "title": "The.Last.of.Us.S02E03.480p.WEB.h264-GROUP.mkv\n👤 250 💾 400 MB",
              "infoHash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "fileIdx": 0 }
        ]}),
    );
    let ranked = rank(all, &input);
    assert!(
        ranked
            .best()
            .unwrap()
            .stream
            .raw_description
            .contains("480p")
    );
}

// Kids (plans/m11-kids.md 4.4).

fn up_target() -> Target {
    Target {
        title: "Up".into(),
        year: None,
        season: None,
        episode: None,
    }
}

fn torrent(i: u8, title: &str) -> serde_json::Value {
    json!({ "name": "Torrentio\n1080p", "title": title,
            "infoHash": format!("{:040x}", i), "fileIdx": 0 })
}

/// The releases a kid's "Up" meets: its own, another film with the word in
/// its name, a name the parser reads no title from, an adult term, words
/// that only hold one ("Essex", "Class"), and two hand-offs.
fn up_releases() -> Vec<ParsedStream> {
    streams(
        "torrentio",
        0,
        json!({ "streams": [
            torrent(1, "Up.2009.1080p.BluRay.x264-GROUP\n👤 50 💾 2 GB"),
            torrent(2, "Up.in.the.Air.2009.1080p.BluRay.x264-GROUP\n👤 50 💾 2 GB"),
            torrent(3, "1080p.BluRay.x264-GROUP\n👤 50 💾 2 GB"),
            torrent(4, "Up.2009.XXX.1080p.WEB.x264-GROUP\n👤 50 💾 2 GB"),
            torrent(5, "Up.2009.720p.BluRay.x264-GROUP\nRipped in Essex, Class of 2020"),
            { "ytId": "abcdefghijk", "title": "Up" },
            { "externalUrl": "https://example.com/up", "title": "Up" }
        ]}),
    )
}

fn kept(ranked: &Ranked) -> Vec<String> {
    let mut all: Vec<String> = ranked
        .rows
        .iter()
        .flat_map(|r| std::iter::once(&r.stream).chain(&r.failover))
        .map(|s| {
            s.raw_description
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    all.sort();
    all
}

fn hidden(ranked: &Ranked, reason: HideReason) -> usize {
    ranked.hidden.iter().filter(|h| h.reason == reason).count()
}

#[test]
fn a_kid_sees_only_releases_named_as_the_allowed_title() {
    let (caps, prefs) = (DeviceCaps::basic_1080p(), StreamPrefs::default());
    let names = ["Up".to_owned()];
    let input = RankInput {
        caps: &caps,
        prefs: &prefs,
        target: &up_target(),
        last_binge_group: None,
        kids: Some(&names),
        conditions: engine(),
    };
    let ranked = rank(up_releases(), &input);
    assert_eq!(
        kept(&ranked),
        [
            "Up.2009.1080p.BluRay.x264-GROUP",
            "Up.2009.720p.BluRay.x264-GROUP"
        ],
        "{:?}",
        ranked.hidden
    );
    assert_eq!(
        hidden(&ranked, HideReason::WrongTitle),
        2,
        "another film, no title"
    );
    assert_eq!(hidden(&ranked, HideReason::NotForKids), 1);
    assert_eq!(hidden(&ranked, HideReason::LeavesApp), 2);
}

#[test]
fn an_open_profile_ranks_as_before() {
    let (caps, prefs) = (DeviceCaps::basic_1080p(), StreamPrefs::default());
    let input = RankInput {
        caps: &caps,
        prefs: &prefs,
        target: &up_target(),
        last_binge_group: None,
        kids: None,
        conditions: engine(),
    };
    let ranked = rank(up_releases(), &input);
    assert_eq!(hidden(&ranked, HideReason::LeavesApp), 0);
    assert_eq!(hidden(&ranked, HideReason::NotForKids), 0);
    assert_eq!(
        hidden(&ranked, HideReason::WrongTitle),
        0,
        "short titles pass"
    );
}

#[test]
fn a_kid_matches_any_allowed_name_with_numbers_either_way() {
    let (caps, prefs) = (DeviceCaps::basic_1080p(), StreamPrefs::default());
    let all = streams(
        "torrentio",
        0,
        json!({ "streams": [
            torrent(1, "Frozen.2.2019.1080p.BluRay.x264-GROUP"),
            torrent(2, "Karlar.Ulkesi.II.2019.1080p.WEB.x264-GROUP"),
            torrent(3, "The.Frozen.Ground.2013.1080p.BluRay.x264-GROUP")
        ]}),
    );
    let names = ["Frozen II".to_owned(), "Karlar Ulkesi 2".to_owned()];
    let target = Target {
        title: "Frozen II".into(),
        ..up_target()
    };
    let input = RankInput {
        caps: &caps,
        prefs: &prefs,
        target: &target,
        last_binge_group: None,
        kids: Some(&names),
        conditions: engine(),
    };
    assert_eq!(
        kept(&rank(all, &input)),
        [
            "Frozen.2.2019.1080p.BluRay.x264-GROUP",
            "Karlar.Ulkesi.II.2019.1080p.WEB.x264-GROUP"
        ]
    );
    // An apostrophe the release leaves out, and a leading "the".
    let all = streams(
        "torrentio",
        0,
        json!({ "streams": [
            torrent(4, "Harry.Potter.and.the.Philosophers.Stone.2001.1080p.BluRay.x264-GROUP"),
            torrent(5, "Lion.King.2019.1080p.WEB.x264-GROUP")
        ]}),
    );
    let names = [
        "Harry Potter and the Philosopher's Stone".to_owned(),
        "The Lion King".to_owned(),
    ];
    let input = RankInput {
        kids: Some(&names),
        ..input
    };
    assert_eq!(rank(all, &input).rows.len(), 2);
}

// Stream choice (plans/m13-stream-choice.md 3, as built). Each test is the
// one its mutation in 6.3 must fail.

const GIB: f64 = (1u64 << 30) as f64;

fn film() -> Target {
    Target {
        title: "Movie".into(),
        year: None,
        season: None,
        episode: None,
    }
}

/// A Torrentio row: a release name, seeders and a size in GB.
fn seeded(i: u8, release: &str, seeds: u32, gb: f64) -> serde_json::Value {
    json!({ "name": "Torrentio", "title": format!("{release}\n👤 {seeds} 💾 {gb} GB"),
            "infoHash": format!("{:040x}", 0x100 + i as u32), "fileIdx": 0 })
}

/// A link a streaming service has ready (`cached`).
fn ready(i: u8, release: &str, gb: f64) -> serde_json::Value {
    json!({ "name": "Debrid", "title": release, "url": format!("https://cdn.example/{i}.mkv"),
            "behaviorHints": { "cached": true, "videoSize": (gb * GIB) as u64,
                               "filename": format!("{release}.mkv") } })
}

struct Case {
    caps: DeviceCaps,
    prefs: StreamPrefs,
    target: Target,
    conditions: Conditions,
}

impl Case {
    /// A 1080p stick with the engine, no account, nothing measured.
    fn stick() -> Self {
        Self {
            caps: DeviceCaps::basic_1080p(),
            prefs: StreamPrefs::default(),
            target: film(),
            conditions: engine().clone(),
        }
    }

    fn rank(&self, rows: Vec<serde_json::Value>) -> Ranked {
        let all = streams("t", 0, json!({ "streams": rows }));
        rank(
            all,
            &RankInput {
                caps: &self.caps,
                prefs: &self.prefs,
                target: &self.target,
                last_binge_group: None,
                kids: None,
                conditions: &self.conditions,
            },
        )
    }
}

fn first_line(r: &RankedRow) -> &str {
    r.stream.raw_description.lines().next().unwrap_or_default()
}

fn row<'a>(ranked: &'a Ranked, name: &str) -> &'a RankedRow {
    ranked
        .rows
        .iter()
        .find(|r| first_line(r).starts_with(name))
        .unwrap_or_else(|| panic!("no row {name}"))
}

fn kind(line: &ReasonLine) -> String {
    serde_json::to_value(line).unwrap()["kind"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Mutation 1: with no runtime (bitrate unknown) only the limit keeps a
/// 9 GB Blu-ray with 900 seeders below a 3 GB WEBRip with 300.
#[test]
fn the_size_limit_keeps_a_large_torrent_below_one_that_fits() {
    let ranked = Case::stick().rank(vec![
        seeded(1, "Movie.2020.1080p.BluRay.x264-GRP", 900, 9.0),
        seeded(2, "Movie.2020.1080p.WEBRip.x264-GRP", 300, 3.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.WEBRip.x264-GRP"
    );
    let large = row(&ranked, "Movie.2020.1080p.BluRay");
    assert_eq!(kind(&large.line), "large");
}

/// Mutations 2 and 4: a ready 4K remux at 65 Mbit/s cannot keep up with a
/// measured 25 Mbit/s (safe 17.5), so a ready 1080p wins, with no size
/// limit and with prefer ready on or off. Both play natively (E-AC-3), so
/// only `sustains` keeps the remux's height from winning.
#[test]
fn a_ready_remux_that_cannot_keep_up_loses_to_a_1080p_that_can() {
    for (limit, prefer_ready) in [(None, true), (None, false), (Some(5u64 << 30), true)] {
        let case = Case {
            caps: caps_4k_dv(),
            prefs: StreamPrefs {
                size_limit_bytes: limit,
                prefer_ready,
                ..StreamPrefs::default()
            },
            target: film(),
            conditions: Conditions {
                runtime_s: Some(120 * 60),
                http: Speed {
                    mbps: 25.0,
                    samples: 3,
                },
                ..with_account().clone()
            },
        };
        let ranked = case.rank(vec![
            ready(1, "Movie.2020.2160p.UHD.BluRay.REMUX.HEVC.DDP5.1-GRP", 54.5),
            ready(2, "Movie.2020.1080p.BluRay.x264-GRP", 7.0),
        ]);
        let best = ranked.best().unwrap();
        assert_eq!(
            first_line(best),
            "Movie.2020.1080p.BluRay.x264-GRP",
            "{limit:?} {prefer_ready}"
        );
        let remux = row(&ranked, "Movie.2020.2160p");
        assert!(!remux.key.sustains);
        assert_eq!(kind(&remux.line), "mayPause");
    }
}

/// Research R1 (plans/research/buffer-and-bitrate-decisions.md, case 1):
/// when no row keeps up, the one that pauses least wins. On a measured
/// 4 Mbit/s line a 65 Mbit/s 4K remux (16 times the line) loses to a
/// 5.4 Mbit/s 1080p (1.34 times).
#[test]
fn when_nothing_keeps_up_the_row_that_pauses_least_wins() {
    let case = Case {
        caps: caps_4k_dv(),
        prefs: StreamPrefs::default(),
        target: film(),
        conditions: Conditions {
            runtime_s: Some(120 * 60),
            http: Speed {
                mbps: 4.0,
                samples: 3,
            },
            ..with_account().clone()
        },
    };
    let ranked = case.rank(vec![
        ready(1, "Movie.2020.2160p.UHD.BluRay.REMUX.HEVC.DDP5.1-GRP", 54.5),
        ready(2, "Movie.2020.1080p.WEB-DL.x264-GRP", 4.5),
    ]);
    let (remux, web) = (
        row(&ranked, "Movie.2020.2160p"),
        row(&ranked, "Movie.2020.1080p"),
    );
    assert!(!remux.key.sustains && !web.key.sustains);
    assert_eq!((remux.key.pace, web.key.pace), (0, 1));
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.WEB-DL.x264-GRP"
    );
}

/// Research R2 (case 2): swarms under the wanted seeders are judged against
/// what small swarms give (before any sample, 1.4 Mbit/s for 18 seeders,
/// scaled per seeder), not the line. A 6-seeder 720p and a 22-seeder 1080p
/// no longer beat a 900-seeder 1080p that needs 72% of a measured
/// 10 Mbit/s; their line says they have few seeders (the small-swarm
/// figure is an estimate, plans/play-every-version.md 2g).
#[test]
fn a_thin_swarm_is_judged_against_small_swarms_not_the_line() {
    let case = Case {
        conditions: Conditions {
            runtime_s: Some(100 * 60),
            p2p: Speed {
                mbps: 10.0,
                samples: 4,
            },
            free_bytes: Some(200 << 30),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.1080p.BluRay.x264-BIG", 900, 5.0),
        seeded(2, "Movie.2020.1080p.WEB-DL.x264-MID", 22, 2.0),
        seeded(3, "Movie.2020.720p.WEB-DL.x264-LOW", 6, 1.1),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-BIG"
    );
    for name in ["Movie.2020.1080p.WEB-DL", "Movie.2020.720p"] {
        let thin = row(&ranked, name);
        assert!(!thin.key.sustains, "{name}");
        // The small-swarm figure is an estimate, scaled per seeder: it
        // ranks, but no row says "may pause" on it (play-every-version 2g).
        assert!(
            matches!(thin.line, ReasonLine::Few { .. }),
            "{name}: {:?}",
            thin.line
        );
    }
    // The healthy swarm is covered by the line with no margin.
    let healthy = row(&ranked, "Movie.2020.1080p.BluRay");
    assert_eq!(healthy.key.pace, 2, "x 0.72: covered, with no margin");
}

/// Plans/play-every-version.md 2g: with the speed only estimated (no
/// playback measured it), the same thin swarms still rank below the healthy
/// one, but no row says "may pause": a speed is shown only when measured.
#[test]
fn an_estimated_speed_never_reads_may_pause() {
    let case = Case {
        conditions: Conditions {
            runtime_s: Some(100 * 60),
            p2p: Speed {
                mbps: 10.0,
                samples: 0,
            },
            free_bytes: Some(200 << 30),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.1080p.BluRay.x264-BIG", 900, 5.0),
        seeded(2, "Movie.2020.1080p.WEB-DL.x264-MID", 22, 2.0),
        seeded(3, "Movie.2020.720p.WEB-DL.x264-LOW", 6, 1.1),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-BIG"
    );
    for name in ["Movie.2020.1080p.WEB-DL", "Movie.2020.720p"] {
        let thin = row(&ranked, name);
        assert!(!thin.key.sustains, "{name}: the estimate still ranks");
        assert!(
            !matches!(thin.line, ReasonLine::MayPause { .. }),
            "{name}: {:?}",
            thin.line
        );
    }
}

/// Race 3, Big Buck Bunny's shape: every swarm has 0 or 1 seeders, so each
/// is judged against the 0.3 Mbit/s floor and all are in pace band 0. The
/// 720p (2.9 Mbit/s, 10 times over) stays ahead of the 4K WEB-DL (7.4
/// Mbit/s, 25 times over), where resolution used to decide.
#[test]
fn inside_pace_band_zero_the_row_least_over_its_speed_wins() {
    let case = Case {
        conditions: Conditions {
            runtime_s: Some(10 * 60),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2008.2160p.WEB-DL.x265-UTR", 0, 0.52),
        seeded(2, "Movie.2008.720p.BluRay.x264-NhaNc3", 1, 0.2),
    ]);
    let (uhd, hd) = (
        row(&ranked, "Movie.2008.2160p"),
        row(&ranked, "Movie.2008.720p"),
    );
    assert_eq!((uhd.key.pace, hd.key.pace), (0, 0));
    assert!(
        hd.key.covered > uhd.key.covered,
        "{:?} {:?}",
        hd.key,
        uhd.key
    );
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2008.720p.BluRay.x264-NhaNc3"
    );
}

/// The small-swarm prior scales per seeder (1.4 Mbit/s for the race's 18):
/// among thin swarms more seeders still help. Winter Sleep's shape (196
/// min, the corpus's asserted pick): a 31-seeder 1080p of 3.1 GB (needs
/// 2.1, gets 2.4) stays ahead of an 11-seeder 720p of 1.6 GB (needs 1.1,
/// gets 0.86).
#[test]
fn more_seeders_still_help_among_thin_swarms() {
    let case = Case {
        conditions: Conditions {
            runtime_s: Some(196 * 60),
            p2p: Speed {
                mbps: 10.0,
                samples: 4,
            },
            free_bytes: Some(200 << 30),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.720p.BluRay.x264-LOW", 11, 1.6),
        seeded(2, "Movie.2020.1080p.BluRay.x264-MID", 31, 3.1),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-MID"
    );
    let paces = (
        row(&ranked, "Movie.2020.1080p").key.pace,
        row(&ranked, "Movie.2020.720p").key.pace,
    );
    assert_eq!(paces, (2, 1));
}

/// Mutation 5: a release in the user's language wins even when it may
/// pause; one in another language that keeps up does not.
#[test]
fn your_language_wins_even_when_it_may_pause() {
    let case = Case {
        prefs: StreamPrefs {
            languages: vec!["en".into()],
            ..StreamPrefs::default()
        },
        conditions: Conditions {
            runtime_s: Some(100 * 60),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.1080p.BluRay.x264-GRP", 300, 9.0),
        json!({ "name": "Torrentio", "title": "Movie.2020.1080p.BluRay.x264-RUS\n👤 900 💾 2.0 GB\n🇷🇺",
                "infoHash": format!("{:040x}", 0x200), "fileIdx": 0 }),
    ]);
    let best = ranked.best().unwrap();
    assert_eq!(first_line(best), "Movie.2020.1080p.BluRay.x264-GRP");
    assert!(!best.key.sustains, "it may pause, and still wins");
}

/// Mutation 10: a 2160p file under 4.6 Mbit/s is starved or upscaled and
/// counts as 1080p, so a 4K at a real bitrate wins on the 4K TV, though the
/// starved one has more seeders and a better source.
#[test]
fn a_starved_4k_counts_as_1080p() {
    let case = Case {
        caps: caps_4k_dv(),
        prefs: StreamPrefs {
            size_limit_bytes: None,
            ..StreamPrefs::default()
        },
        conditions: Conditions {
            runtime_s: Some(120 * 60),
            p2p: Speed {
                mbps: 25.0,
                samples: 3,
            },
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.2160p.BluRay.x265-LOW", 900, 2.6),
        seeded(2, "Movie.2020.2160p.WEB-DL.x265-OK", 300, 8.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.2160p.WEB-DL.x265-OK"
    );
    assert_eq!(
        row(&ranked, "Movie.2020.2160p.BluRay").key.effective_height,
        1080
    );
}

/// Mutation 11: a 900-seed HDTS copy loses to a real release with 60.
#[test]
fn a_cinema_copy_never_wins_on_seeds() {
    let ranked = Case::stick().rank(vec![
        seeded(1, "Movie.2020.1080p.HDTS.x264-CAM", 900, 2.0),
        seeded(2, "Movie.2020.1080p.WEB-DL.x264-WEB", 60, 2.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.WEB-DL.x264-WEB"
    );
    assert_eq!(kind(&row(&ranked, "Movie.2020.1080p.HDTS").line), "copy");
}

/// Mutation 12 (question 5: as many seeders as possible): seeders count up
/// to 1,000. Below it more seeders win, as in Shawshank's recording: a
/// stereo 1.6 GB copy with 698 over a 5.1 2.2 GB one with 239. Past it the
/// file decides: a Blu-ray with 1,000 over a WEBRip with 5,000.
#[test]
fn seeders_count_up_to_1000_then_the_file_decides() {
    let ranked = Case::stick().rank(vec![
        seeded(1, "Movie.2020.1080p.BluRay.x264.AAC.5.1-SIX", 239, 2.2),
        seeded(2, "Movie.2020.1080p.BrRip.x264-YIFY", 698, 1.6),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BrRip.x264-YIFY"
    );
    let ranked = Case::stick().rank(vec![
        seeded(3, "Movie.2020.1080p.WEBRip.x264-MANY", 5000, 2.0),
        seeded(4, "Movie.2020.1080p.BluRay.x264-DISC", 1000, 2.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-DISC"
    );
}

/// plans/streaming-window.md 3.1 and 3.2: where windows play, a torrent
/// that does not fit shows when its window holds 120 s of it ahead and
/// hides when it holds less; without windows it hides as before, with the
/// engine's 512 MiB reserve, so a small file on a nearly full device fits.
#[test]
fn a_torrent_that_does_not_fit_shows_when_its_window_holds_two_minutes() {
    let case = |free: u64, window: bool, runtime_s: u32| Case {
        conditions: Conditions {
            runtime_s: Some(runtime_s),
            free_bytes: Some(free),
            window,
            ..engine().clone()
        },
        ..Case::stick()
    };
    // 0.53 GiB in 12 min 14 s (Tears of Steel) with 1.1 GB free: the 3.2
    // table's 9.6 min ahead, shown; without windows it does not fit.
    let tos = || vec![seeded(1, "Movie.2012.1080p.WEB.x264-GRP", 300, 0.53)];
    let shown = case(1_100_000_000, true, 734).rank(tos());
    assert_eq!(shown.rows.len(), 1, "{:?}", shown.hidden);
    let whole = case(1_100_000_000, false, 734).rank(tos());
    assert_eq!(hidden(&whole, HideReason::NoSpace), 1);
    // A 60 GiB remux in 2 h with 1.5 GiB free: about 40 s ahead, hidden.
    let remux = vec![seeded(
        2,
        "Movie.2020.1080p.BluRay.REMUX.AVC-GRP",
        300,
        60.0,
    )];
    let ranked = case(3 << 29, true, 7200).rank(remux);
    assert_eq!(hidden(&ranked, HideReason::NoSpace), 1);
    // 0.13 GiB with 1.1 GB free fits whole (the 1 GiB spare hid it).
    let small = vec![seeded(3, "Movie.2010.1080p.WEB.x264-GRP", 300, 0.133)];
    assert_eq!(case(1_100_000_000, false, 888).rank(small).rows.len(), 1);
}

/// Where windows play, with a measured 60 Mbit/s line (so no remux row
/// may pause): `free` bytes free and the runtime known.
fn windowed_case(free: u64, runtime_s: Option<u32>, caps: DeviceCaps) -> Case {
    let mut case = prior_case(free, runtime_s, caps);
    case.conditions.p2p = Speed {
        mbps: 60.0,
        samples: 3,
    };
    case
}

/// The same with the P2P prior (10 Mbit/s), as 3.2 sizes the small files.
fn prior_case(free: u64, runtime_s: Option<u32>, caps: DeviceCaps) -> Case {
    Case {
        caps,
        conditions: Conditions {
            runtime_s,
            free_bytes: Some(free),
            window: true,
            ..engine().clone()
        },
        ..Case::stick()
    }
}

/// The owner's example (3.2): 7 min ahead, 6 on Apple targets, where the
/// engine's `F` also counts the 16 MiB APFS keeps of the head (419 s).
fn owners_minutes() -> u32 {
    if mq_engine_sched::APFS_HEAD_BYTES > 0 {
        6
    } else {
        7
    }
}

/// One row's line, ranked alone.
fn line_of(case: &Case, release: &str, gb: f64) -> ReasonLine {
    let ranked = case.rank(vec![seeded(1, release, 300, gb)]);
    assert_eq!(ranked.rows.len(), 1, "{release}: {:?}", ranked.hidden);
    ranked.rows[0].line.clone()
}

/// The minutes ahead a window row says, `None` on a row with no window.
fn minutes(line: &ReasonLine) -> Option<u32> {
    match line {
        ReasonLine::Buffer { ahead_min, .. } => *ahead_min,
        _ => None,
    }
}

/// Phase 3 test a and the 3.2 table (plans/streaming-window.md): each row
/// shown with its minutes ahead, or whole, or hidden; the windowed rows
/// keep their seeders and size and never say "Large file". The minutes are
/// the engine's: its `F` pins a piece more than 3.2's, so Tears of Steel
/// on the 1 GiB image's 0.84 GB says 3 min (214 to 235 s), not 4.
#[test]
fn the_window_rows_of_the_worked_table() {
    const GB: u64 = 1_000_000_000;
    let stick = DeviceCaps::basic_1080p;
    let remux = "Movie.2020.1080p.BluRay.REMUX.AVC-GRP";
    let web4k = "Movie.2020.2160p.WEB-DL.DV.H.265-GRP";
    let tos = "Movie.2012.1080p.WEB.x264-GRP";
    let (fast, prior) = (windowed_case, prior_case);
    type CaseOf = fn(u64, Option<u32>, DeviceCaps) -> Case;
    // A release, its GiB, runtime and free bytes, the device, the line's
    // speed, and the minutes it says.
    type Row = (&'static str, f64, u32, u64, DeviceCaps, CaseOf, Option<u32>);
    let cases: [Row; 9] = [
        // The owner's example: 40 GiB in 2 h 30 min with 3 GiB free.
        (
            remux,
            40.0,
            9000,
            3 << 30,
            stick(),
            fast,
            Some(owners_minutes()),
        ),
        (remux, 40.0, 9000, 8 << 30, stick(), fast, Some(10)),
        (web4k, 15.0, 7200, 5 << 29, caps_4k_dv(), fast, Some(10)),
        (web4k, 15.0, 7200, 10 << 30, caps_4k_dv(), fast, Some(10)),
        (
            "Movie.2020.1080p.WEB.x264-GRP",
            2.5,
            7200,
            2 << 30,
            stick(),
            prior,
            Some(10),
        ),
        // Tears of Steel on the Pixel (PX9) and on the 1 GiB image.
        (tos, 0.532, 734, 1_100_000_000, stick(), prior, Some(9)),
        (tos, 0.532, 734, 840_000_000, stick(), prior, Some(3)),
        // Sintel fits whole on the same Pixel; a 20 GiB remux on a Mac too.
        (
            "Movie.2010.1080p.WEB.x264-GRP",
            0.133,
            888,
            1_100_000_000,
            stick(),
            prior,
            None,
        ),
        (remux, 20.0, 7200, 400 << 30, stick(), fast, None),
    ];
    for (release, gb, runtime, free, caps, case, want) in cases {
        let mut whole = case(free, Some(runtime), caps.clone());
        whole.conditions.window = false;
        let line = line_of(&case(free, Some(runtime), caps), release, gb);
        assert_eq!(
            minutes(&line),
            want,
            "{release} {gb} GiB with {free} free: {line:?}"
        );
        match line {
            ReasonLine::Buffer {
                seeders,
                size_bytes,
                budget_bytes,
                ..
            } => {
                assert_eq!(seeders, Some(300));
                assert_eq!(size_bytes, Some((gb * GIB) as u64));
                assert!(budget_bytes < free, "{budget_bytes}");
            }
            // Whole, as today.
            other => assert_eq!(other, line_of(&whole, release, gb), "{release}"),
        }
    }
    // A 4K remux of 60 GiB in 2 h with 1.5 GiB free: about 40 s ahead.
    let ranked = windowed_case(3 << 29, Some(7200), caps_4k_dv()).rank(vec![seeded(
        2,
        "Movie.2020.2160p.BluRay.REMUX.HEVC.DV-GRP",
        300,
        60.0,
    )]);
    assert_eq!(hidden(&ranked, HideReason::NoSpace), 1);
    // With 8 GiB free the window is the cap: about 4.9 GB stays free.
    let capped = line_of(&windowed_case(8 << 30, Some(9000), stick()), remux, 40.0);
    let ReasonLine::Buffer { budget_bytes, .. } = capped else {
        panic!("{capped:?}");
    };
    assert!(
        (3_600_000_000..3_800_000_000).contains(&budget_bytes),
        "{budget_bytes}"
    );
    assert!((8 << 30) - budget_bytes > 48 * GB / 10);
}

/// Phase 3 test b: a windowed playback trimmed and left idle keeps 1 GiB of
/// the film; the same row still shows with the same minutes, the folder
/// counted as room for its window.
#[test]
fn a_trimmed_window_left_idle_keeps_its_row_and_minutes() {
    let remux = "Movie.2020.1080p.BluRay.REMUX.AVC-GRP";
    let mut case = windowed_case(2 << 30, Some(9000), DeviceCaps::basic_1080p());
    case.conditions.reclaimable_bytes = Some(1 << 30);
    case.conditions.held.insert(
        format!("{:040x}", 0x101),
        mq_engine_sched::HeldBytes {
            folder: 1 << 30,
            files: vec![1 << 30],
            idle: true,
            committed: 0,
        },
    );
    assert_eq!(
        minutes(&line_of(&case, remux, 40.0)),
        Some(owners_minutes())
    );
}

/// Phase 3 test d: a file that fits ranks and reads as it did without
/// windows.
#[test]
fn a_file_that_fits_ranks_as_without_windows() {
    let rows = || {
        vec![
            seeded(1, "Movie.2020.1080p.WEB.x264-AAA", 300, 2.0),
            seeded(2, "Movie.2020.1080p.BluRay.x264-BBB", 40, 9.0),
            seeded(3, "Movie.2020.720p.WEB.x264-CCC", 900, 1.0),
        ]
    };
    let with = windowed_case(100 << 30, Some(7200), DeviceCaps::basic_1080p());
    let mut without = windowed_case(100 << 30, Some(7200), DeviceCaps::basic_1080p());
    without.conditions.window = false;
    let (a, b) = (with.rank(rows()), without.rank(rows()));
    let lines = |r: &Ranked| {
        r.rows
            .iter()
            .map(|row| (first_line(row).to_owned(), row.line.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(lines(&a), lines(&b));
}

/// Phase 3 test f: with no runtime, the rate a window is planned at is
/// never under the file over 3 h: a 40 GiB 1080p WEB (guessed at 10
/// Mbit/s) is planned at 40 GiB over 3 h.
#[test]
fn with_no_runtime_a_window_is_planned_at_the_file_over_three_hours() {
    let size = 40u64 << 30;
    let ranked = Case::stick().rank(vec![seeded(1, "Movie.2020.1080p.WEB.x264-GRP", 300, 40.0)]);
    let web = &ranked.rows[0].stream;
    assert_eq!(window_rate(web, size, false, None), size / 10_800);
    assert_eq!(window_rate(web, size, false, Some(7200)), size / 7200);
}

/// Review r1 B P2-2: with no runtime the engine plans at the guess, which a
/// remux can stream faster than; the line counts its seconds at no less
/// than the file over 75 min, so a 2 h remux at 40 Mbit/s (1080p) or 80
/// Mbit/s (2160p) never says more minutes than its window holds at its
/// real rate.
#[test]
fn with_no_runtime_the_minutes_never_pass_what_a_remux_holds() {
    let cases = [
        (
            "Movie.2020.1080p.BluRay.REMUX.AVC-GRP",
            33.5,
            4u64 << 30,
            DeviceCaps::basic_1080p(),
        ),
        (
            "Movie.2020.2160p.BluRay.REMUX.HEVC.DV-GRP",
            67.1,
            6 << 30,
            caps_4k_dv(),
        ),
    ];
    // A line fast enough for both (150 Mbit/s), so neither may pause.
    let case = |free: u64, runtime: Option<u32>, caps: DeviceCaps| {
        let mut c = windowed_case(free, runtime, caps);
        c.conditions.p2p.mbps = 150.0;
        c
    };
    for (release, gb, free, caps) in cases {
        let real = minutes(&line_of(&case(free, Some(7200), caps.clone()), release, gb));
        let guessed = minutes(&line_of(&case(free, None, caps), release, gb));
        assert!(
            real.is_some() && guessed.is_some(),
            "{release}: {real:?} {guessed:?}"
        );
        assert!(
            guessed <= real,
            "{release}: {guessed:?} minutes said, {real:?} held"
        );
    }
}

/// Review r1 B P2-3: a season pack row lists the pack (12 GiB of ten
/// episodes); its room and its line are the episode's (1.2 GiB): it plays
/// whole where the episode fits, says the episode's size and minutes where
/// it does not, and says no minutes when the episode count is unknown.
#[test]
fn a_season_pack_row_is_judged_by_the_episode_it_plays() {
    let pack = || vec![seeded(1, "Show.S01.1080p.WEB.x264-GRP", 300, 12.0)];
    let case = |free: u64, episodes: Option<u32>| Case {
        target: Target {
            title: "Show".into(),
            year: None,
            season: Some(1),
            episode: Some(3),
        },
        // A line the pack's 38 Mbit/s keeps up with, so no warning wins.
        conditions: Conditions {
            runtime_s: Some(2700),
            episodes_in_season: episodes,
            free_bytes: Some(free),
            window: true,
            p2p: Speed {
                mbps: 60.0,
                samples: 3,
            },
            ..engine().clone()
        },
        ..Case::stick()
    };
    let episode = (12 << 30) / 10;
    // 8 GiB free: the episode fits whole, so no window is promised (the
    // pack's size still says "Large file", as the ranking weighs it).
    let whole = case(8 << 30, Some(10)).rank(pack());
    assert_ne!(
        kind(&whole.rows[0].line),
        "buffer",
        "{:?}",
        whole.rows[0].line
    );
    // 1.5 GiB free: the episode plays in a window, at its own size.
    let windowed = case(3 << 29, Some(10)).rank(pack());
    let line = &windowed.rows[0].line;
    assert!(
        matches!(line, ReasonLine::Buffer { size_bytes: Some(b), .. } if *b == episode),
        "{line:?}"
    );
    // The season's count unknown: no promise, and no hiding either.
    let unknown = case(3 << 29, None).rank(pack());
    assert_eq!(unknown.rows.len(), 1, "{:?}", unknown.hidden);
    assert_ne!(kind(&unknown.rows[0].line), "buffer");
    // A smaller pack (4 GiB) that plays whole: its list line says the
    // episode's size too, as its buffer line would (review r2 B P3-3).
    let small = vec![seeded(2, "Show.S01.1080p.WEB.x264-GRP", 300, 4.0)];
    let line = &case(8 << 30, Some(10)).rank(small).rows[0].line;
    assert!(
        matches!(line, ReasonLine::Quick { size_bytes: Some(b), .. } if *b == (4 << 30) / 10),
        "{line:?}"
    );
}

/// Review r2 B P2-1: a Torrentio row for one episode of a pack names its
/// file and gives the file's size (`videoSize`) under a "COMPLETE" title;
/// that size is never shared among the season's episodes, whether the row
/// gives both, the size alone or the file name alone. Where the 1.2 GiB
/// episode cannot play it is hidden; in a window it says 1.2 GiB.
#[test]
fn an_episode_row_under_a_pack_title_keeps_its_files_size() {
    const EPISODE: u64 = 1_288_490_188;
    let file = "Show.S01E03.1080p.WEB-GRP.mkv";
    let rows = [
        json!({ "filename": file, "videoSize": EPISODE }),
        json!({ "videoSize": EPISODE }),
        json!({ "filename": file }),
    ];
    let case = |free: u64| Case {
        target: Target {
            title: "Show".into(),
            year: None,
            season: Some(1),
            episode: Some(3),
        },
        conditions: Conditions {
            runtime_s: Some(2700),
            episodes_in_season: Some(10),
            free_bytes: Some(free),
            window: true,
            ..engine().clone()
        },
        ..Case::stick()
    };
    const RESERVE: u64 = 512 << 20;
    for hints in rows {
        let row = || {
            vec![json!({ "name": "Torrentio\n1080p",
                         "title": format!("Show.S01.COMPLETE.1080p.WEB-GRP\n{file}\n👤 300 💾 1.2 GB"),
                         "infoHash": "e".repeat(40), "fileIdx": 2, "behaviorHints": hints })]
        };
        // 130 MiB over the reserve: a tenth of the episode would fit whole;
        // the episode itself has no room even for a window.
        let short = case(RESERVE + (130 << 20)).rank(row());
        assert_eq!(
            hidden(&short, HideReason::NoSpace),
            1,
            "{hints}: {:?}",
            short.rows
        );
        // 400 MiB over: a window, with the file's size.
        let windowed = case(RESERVE + (400 << 20)).rank(row());
        let line = &windowed.rows[0].line;
        assert!(
            matches!(line, ReasonLine::Buffer { size_bytes: Some(b), .. } if *b == EPISODE),
            "{hints}: {line:?}"
        );
    }
}

/// Phase 3 tests g and h: under the default limit a windowed row passes
/// the size limit (no "Large file"); a limit the viewer chose compares the
/// file.
#[test]
fn a_windowed_row_passes_the_default_limit_not_a_chosen_one() {
    let remux = "Movie.2020.1080p.BluRay.REMUX.AVC-GRP";
    let mut case = windowed_case(8 << 30, Some(9000), DeviceCaps::basic_1080p());
    assert_eq!(kind(&line_of(&case, remux, 40.0)), "buffer");
    case.conditions.limit_chosen = true;
    let line = line_of(&case, remux, 40.0);
    assert!(
        matches!(line, ReasonLine::Large { chosen: true, .. }),
        "{line:?}"
    );
}

/// Phase 3 test j: a window playing elsewhere takes its `C`, so the second
/// row has fewer minutes ahead.
#[test]
fn a_window_playing_elsewhere_takes_its_room_from_the_minutes() {
    let remux = "Movie.2020.1080p.BluRay.REMUX.AVC-GRP";
    let mut case = windowed_case(3 << 30, Some(9000), DeviceCaps::basic_1080p());
    let alone = minutes(&line_of(&case, remux, 40.0)).unwrap();
    case.conditions.committed_bytes = 1 << 30;
    let shared = minutes(&line_of(&case, remux, 40.0)).unwrap();
    assert_eq!((alone, shared), (owners_minutes(), 3));
}

/// Phase 3 (3.3): on mobile data a windowed row says no minutes; the
/// engine keeps the device preset ahead there.
#[test]
fn on_mobile_data_a_windowed_row_says_no_minutes() {
    let remux = "Movie.2020.1080p.BluRay.REMUX.AVC-GRP";
    let mut case = windowed_case(3 << 30, Some(9000), DeviceCaps::basic_1080p());
    case.conditions.metered = true;
    let line = line_of(&case, remux, 40.0);
    assert_eq!(kind(&line), "buffer");
    assert_eq!(minutes(&line), None);
}

/// Mutation 6: a torrent larger than the free space less 512 MiB is hidden.
#[test]
fn no_room_hides_a_torrent_larger_than_the_free_space() {
    let case = Case {
        conditions: Conditions {
            free_bytes: Some(10 << 30),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.1080p.BluRay.x264-BIG", 300, 12.0),
        seeded(2, "Movie.2020.1080p.BluRay.x264-FIT", 300, 8.5),
    ]);
    assert_eq!(hidden(&ranked, HideReason::NoSpace), 1);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-FIT"
    );
}

/// Mutation 7 and 3.2: with no engine and no account a torrent and a
/// service-resolved row cannot open; a link still plays. An account makes
/// both `Unknown`, the engine alone makes the torrent a swarm.
#[test]
fn without_engine_or_account_a_torrent_needs_debrid() {
    let rows = || {
        vec![
            seeded(1, "Movie.2020.1080p.BluRay.x264-GRP", 300, 2.0),
            json!({ "name": "RD", "title": "Movie.2020.1080p.WEB-DL.x264-GRP",
                    "clientResolve": { "type": "debrid", "service": "realdebrid",
                                       "infoHash": format!("{:040x}", 0x300), "fileIdx": 0 } }),
            json!({ "name": "Link", "title": "Movie.2020.720p.WEB-DL.x264-GRP",
                    "url": "https://cdn.example/link.mp4" }),
        ]
    };
    let classes = |conditions: Conditions| {
        let case = Case {
            conditions,
            ..Case::stick()
        };
        let ranked = case.rank(rows());
        let mut classes: Vec<_> = ranked.rows.iter().map(|r| r.class).collect();
        classes.sort_by_key(|c| format!("{c:?}"));
        (classes, hidden(&ranked, HideReason::NeedsDebrid))
    };
    assert_eq!(
        classes(Conditions::default()),
        (vec![SourceClass::Ready], 2)
    );
    let account = Conditions {
        debrid: true,
        ..Conditions::default()
    };
    assert_eq!(
        classes(account),
        (
            vec![
                SourceClass::Ready,
                SourceClass::Unknown,
                SourceClass::Unknown
            ],
            0
        )
    );
    assert_eq!(
        classes(engine().clone()),
        (vec![SourceClass::Ready, SourceClass::Swarm], 1)
    );
}

/// 3.2: a service that says not cached makes a torrent a swarm with the
/// engine, else a fetch, which says it downloads first and ranks last.
#[test]
fn a_torrent_the_service_has_not_cached_is_a_swarm_or_a_fetch() {
    let rows = || {
        vec![
            json!({ "name": "TB", "title": "Movie.2020.1080p.BluRay.x264-GRP\n👤 300 💾 2 GB",
                     "infoHash": format!("{:040x}", 0x400), "fileIdx": 0,
                     "behaviorHints": { "cached": false } }),
        ]
    };
    for (engine, class, line) in [
        (true, SourceClass::Swarm, "quick"),
        (false, SourceClass::Fetch, "fetch"),
    ] {
        let case = Case {
            conditions: Conditions {
                engine,
                ..with_account().clone()
            },
            ..Case::stick()
        };
        let ranked = case.rank(rows());
        let best = ranked.best().unwrap();
        assert_eq!((best.class, kind(&best.line).as_str()), (class, line));
    }
}

/// Mutation 8 and UX r3 7: an episode in a 40 GB season pack is judged per
/// episode (127 Mbit/s over the 48 Mbit/s ceiling is a pack: 12.7 for each
/// of 10), and a movie above the ceiling keeps its own bitrate.
#[test]
fn a_season_pack_is_judged_per_episode_and_a_movie_is_not() {
    let measured = Conditions {
        runtime_s: Some(45 * 60),
        episodes_in_season: Some(10),
        p2p: Speed {
            mbps: 25.0,
            samples: 3,
        },
        ..engine().clone()
    };
    let show = Case {
        target: Target {
            title: "Show".into(),
            year: None,
            season: Some(1),
            episode: Some(3),
        },
        conditions: measured.clone(),
        ..Case::stick()
    };
    let ranked = show.rank(vec![
        seeded(1, "Show.S01.1080p.BluRay.x264-GRP", 500, 40.0),
        seeded(2, "Show.S01E03.1080p.WEB.h264-GRP", 60, 1.5),
    ]);
    let best = ranked.best().unwrap();
    assert_eq!(first_line(best), "Show.S01.1080p.BluRay.x264-GRP");
    let mbps = best.bitrate_mbps.unwrap();
    assert!((12.0..13.5).contains(&mbps), "{mbps}");
    assert!(best.key.sustains && best.key.size_ok);

    // A 3D remux film at 51 Mbit/s is no pack: it may pause.
    let film = Case {
        conditions: Conditions {
            runtime_s: Some(120 * 60),
            ..measured
        },
        ..Case::stick()
    };
    let ranked = film.rank(vec![
        seeded(1, "Movie.2020.1080p.3D.BluRay.REMUX.AVC-GRP", 500, 42.8),
        seeded(2, "Movie.2020.1080p.BluRay.x264-GRP", 300, 3.0),
    ]);
    let remux = row(&ranked, "Movie.2020.1080p.3D");
    assert!(remux.bitrate_mbps.unwrap() > 48.0);
    assert!(!remux.key.sustains);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-GRP"
    );
}

/// Mutation 9: an upscaled 2160p counts as 1080p on a 4K TV, so a native
/// 1080p Blu-ray beats it.
#[test]
fn an_upscaled_4k_counts_as_1080p() {
    let case = Case {
        caps: caps_4k_dv(),
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.2160p.AI.Upscaled.WEB-DL.x265-GRP", 300, 4.0),
        seeded(2, "Movie.2020.1080p.BluRay.x264-GRP", 300, 3.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-GRP"
    );
    assert_eq!(row(&ranked, "Movie.2020.2160p").key.effective_height, 1080);
}

/// Mutations 13 and 14: a release naming another year ranks below the
/// right one, and stays listed with the year said.
#[test]
fn another_year_is_demoted_never_hidden() {
    let case = Case {
        conditions: Conditions {
            title_year: Some(1999),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2023.1080p.BluRay.x264-NEW", 900, 2.0),
        seeded(2, "Movie.1999.1080p.WEBRip.x264-OLD", 300, 2.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.1999.1080p.WEBRip.x264-OLD"
    );
    let other = row(&ranked, "Movie.2023");
    assert_eq!(other.line, ReasonLine::OtherYear { year: 2023 });
}

/// r4 N3: the size limit never demotes a ready file below a weak torrent.
#[test]
fn a_ready_file_never_loses_to_a_weak_torrent_on_size() {
    let case = Case {
        conditions: with_account().clone(),
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.1080p.WEB-DL.x264-GRP", 5, 2.0),
        ready(2, "Movie.2020.1080p.BluRay.x264-GRP", 7.0),
    ]);
    let best = ranked.best().unwrap();
    assert_eq!(best.class, SourceClass::Ready);
    assert!(best.key.size_ok);
}

/// UX r3 6: a torrent that may not start (3 seeders) ranks below a large
/// one with 900; each says why.
#[test]
fn a_torrent_that_may_not_start_ranks_below_a_large_one() {
    let ranked = Case::stick().rank(vec![
        seeded(1, "Movie.2020.1080p.WEB-DL.x264-FEW", 3, 2.0),
        seeded(2, "Movie.2020.1080p.WEB-DL.x264-BIG", 900, 6.0),
    ]);
    let best = ranked.best().unwrap();
    assert_eq!(first_line(best), "Movie.2020.1080p.WEB-DL.x264-BIG");
    assert_eq!(kind(&best.line), "large");
    assert_eq!(
        kind(&row(&ranked, "Movie.2020.1080p.WEB-DL.x264-FEW").line),
        "veryFew"
    );
}

/// r4 P3: a real release named with another year outranks a clean cam.
#[test]
fn a_real_release_outranks_a_cam_of_the_right_year() {
    let case = Case {
        conditions: Conditions {
            title_year: Some(2020),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Movie.2020.HDCAM.x264-GRP", 900, 1.5),
        seeded(2, "Movie.2014.1080p.BluRay.x264-GRP", 300, 2.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2014.1080p.BluRay.x264-GRP"
    );
}

/// UX r3 5: the worst news comes first: a 3-seed 4K over the limit says it
/// may not start, not that it is large.
#[test]
fn the_worst_news_comes_first() {
    let case = Case {
        caps: caps_4k_dv(),
        ..Case::stick()
    };
    let ranked = case.rank(vec![seeded(1, "Movie.2020.2160p.WEB-DL.x265-GRP", 3, 8.0)]);
    assert_eq!(
        ranked.best().unwrap().line,
        ReasonLine::VeryFew { seeders: 3 }
    );
}

/// HDR10+ and Dolby Vision carry an HDR10 base: an HDR10 screen shows them.
#[test]
fn hdr10_plus_plays_on_an_hdr10_screen() {
    let case = Case {
        caps: DeviceCaps {
            display_height: 2160,
            display_hdr: vec![Hdr::Hdr10],
            ..caps_4k_dv()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![seeded(
        1,
        "Movie.2020.2160p.WEB-DL.HDR10+.x265-GRP",
        300,
        4.0,
    )]);
    let best = ranked.best().unwrap();
    assert_eq!(best.key.playability, Playability::Native);
    assert_ne!(kind(&best.line), "washedOut");
}

/// A release naming the work in two languages matches either name.
#[test]
fn a_title_named_in_two_languages_matches_either() {
    let case = Case {
        target: Target {
            title: "Game of Thrones".into(),
            year: None,
            season: Some(1),
            episode: Some(1),
        },
        ..Case::stick()
    };
    // The file name carries the Russian title only (the default recording).
    let mut row = seeded(
        1,
        "Игра престолов / Game of Thrones / Сезон: 1 / Серии: 1-10 из 10 [2011 WEB-DL 1080p]",
        123,
        2.0,
    );
    row["behaviorHints"] = json!({ "filename": "Игра престолов.S01E01.WEB-DL.1080p.mkv" });
    let ranked = case.rank(vec![row]);
    assert_eq!(
        hidden(&ranked, HideReason::WrongTitle),
        0,
        "{:?}",
        ranked.hidden
    );
}

/// Past the wanted seeders a swarm starts as well as with more (pick audit:
/// 16 of 16 legal swarms with 4 to 333 live seeders started, in no order of
/// seeders), so on a 4K TV a 4K with 116 beats a 1080p with 711; under the
/// wanted seeders the seeders decide, as before; between versions equal
/// on the screen, the 4 x band still does; a ready file still comes first.
#[test]
fn past_the_wanted_seeders_the_better_version_wins() {
    let fast = Speed {
        mbps: 100.0,
        samples: 5,
    };
    let tv = Case {
        caps: caps_4k_dv(),
        conditions: Conditions {
            runtime_s: Some(138 * 60),
            http: fast,
            p2p: fast,
            ..engine().clone()
        },
        ..Case::stick()
    };
    let uhd = seeded(1, "Movie.2020.2160p.BluRay.DV.HDR10.x265-GRP", 116, 5.4);
    let fhd = seeded(2, "Movie.2020.1080p.WEB-DL.x265-GRP", 711, 2.1);
    let ranked = tv.rank(vec![uhd.clone(), fhd.clone()]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.2160p.BluRay.DV.HDR10.x265-GRP"
    );
    let thin = seeded(1, "Movie.2020.2160p.BluRay.DV.HDR10.x265-GRP", 30, 5.4);
    let ranked = tv.rank(vec![thin, fhd.clone()]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.WEB-DL.x265-GRP"
    );
    let ready = ready(3, "Movie.2020.1080p.WEB-DL.x264-RDY", 2.0);
    let ranked = tv.rank(vec![uhd, ready]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.WEB-DL.x264-RDY"
    );
    // On a 720p screen both count as 720p: the 4 x band picks.
    let small = Case {
        caps: DeviceCaps {
            display_height: 720,
            ..DeviceCaps::basic_1080p()
        },
        ..tv
    };
    let ranked = small.rank(vec![
        seeded(4, "Movie.2020.720p.WEB-DL.x264-GRP", 89, 1.1),
        seeded(5, "Movie.2020.1080p.BluRay.x264-GRP", 774, 2.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-GRP"
    );
}

/// A season pack's file named for its episode alone, or for a hash, is the
/// show the torrent's own name says (pick audit, Torrentio on 2026-10-22);
/// a torrent named for another work still hides.
#[test]
fn a_pack_file_named_for_its_episode_matches_the_torrents_name() {
    let case = Case {
        target: Target {
            title: "Game of Thrones".into(),
            year: None,
            season: Some(1),
            episode: Some(1),
        },
        ..Case::stick()
    };
    let named = |i, torrent: &str, file: &str| {
        let mut row = seeded(i, &format!("{torrent}\n{file}"), 106, 0.4);
        row["behaviorHints"] = json!({ "filename": file });
        row
    };
    let ranked = case.rank(vec![
        named(
            1,
            "Game of Thrones Season 1 S01 720p BluRay x264 MrLss",
            "S01E01- Winter Is Coming.mkv",
        ),
        named(
            2,
            "Game of Thrones S01E01 1080p WEB-DL x264-NTb",
            "3424e951679a42e7ad891930319ce1df[EZTVx.to].mkv",
        ),
        named(
            3,
            "The Island of Dr. Moreau (1996) 720p BluRay",
            "S01E01- Winter Is Coming.mkv",
        ),
    ]);
    assert_eq!(ranked.rows.len(), 2, "{:?}", ranked.hidden);
    assert_eq!(hidden(&ranked, HideReason::WrongTitle), 1);
}

/// 3.3: phones count as their class.
#[test]
fn a_display_counts_as_its_screen_class() {
    for (height, class) in [
        (2160, 2160),
        (1440, 1080),
        (1179, 1080),
        (720, 720),
        (576, 480),
    ] {
        assert_eq!(screen_class(height), class, "{height}");
    }
}

/// Code review r2 P1-1: a trailer, an embedded player and a link out are
/// hidden, since playback opens none of them; before, they ranked as ready
/// and topped every torrent.
#[test]
fn a_trailer_or_a_link_out_is_hidden() {
    let ranked = Case::stick().rank(vec![
        json!({ "name": "Trailer", "ytId": "dQw4w9WgXcQ" }),
        json!({ "name": "Site", "externalUrl": "https://example.com/watch" }),
        json!({ "name": "Embed", "playerFrameUrl": "https://example.com/embed" }),
        seeded(1, "Movie.2020.1080p.WEB-DL.x264-GRP", 60, 2.0),
    ]);
    assert_eq!(hidden(&ranked, HideReason::UnsupportedSource), 3);
    assert_eq!(ranked.rows.len(), 1);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.WEB-DL.x264-GRP"
    );
}

/// Code review r2 P2-3: a row that may never start loses to every row that
/// can, however it plays: 2 seeders playing natively against 500 that need
/// the second engine (DTS-HD MA on a stick that decodes no DTS).
#[test]
fn a_dead_row_never_wins_on_how_it_plays() {
    let ranked = Case::stick().rank(vec![
        seeded(1, "Movie.2025.1080p.WEB-DL.DDP5.1.H.264-FEW", 2, 2.0),
        seeded(
            2,
            "Movie.2025.1080p.BluRay.x265.10bit.DTS-HD.MA-MANY",
            500,
            4.0,
        ),
    ]);
    let best = ranked.best().unwrap();
    assert_eq!(
        first_line(best),
        "Movie.2025.1080p.BluRay.x265.10bit.DTS-HD.MA-MANY"
    );
    assert_eq!(best.key.playability, Playability::NeedsFallbackEngine);
    let few = row(&ranked, "Movie.2025.1080p.WEB-DL");
    assert_eq!(few.key.playability, Playability::Native);
    assert!(!few.key.viable);
}

/// Code review r1 P2-2: under 5 seeders a torrent may not start whatever
/// the user asked for, so a dead 4K never beats a live 1080p on a 4K TV;
/// exactly 5, or none stated, may start.
#[test]
fn under_five_seeders_never_starts_whatever_the_wanted_count() {
    for min_seeders in [0, 3] {
        let case = Case {
            caps: caps_4k_dv(),
            prefs: StreamPrefs {
                min_seeders,
                ..StreamPrefs::default()
            },
            ..Case::stick()
        };
        let ranked = case.rank(vec![
            seeded(1, "Movie.2020.2160p.WEB-DL.DDP5.1.H.265-DEAD", 0, 4.0),
            seeded(2, "Movie.2020.2160p.WEB-DL.DDP5.1.H.265-FOUR", 4, 4.0),
            seeded(3, "Movie.2020.1080p.WEB-DL.DDP5.1.H.264-LIVE", 100, 2.0),
        ]);
        assert_eq!(
            first_line(ranked.best().unwrap()),
            "Movie.2020.1080p.WEB-DL.DDP5.1.H.264-LIVE",
            "min {min_seeders}"
        );
        for dead in [
            "Movie.2020.2160p.WEB-DL.DDP5.1.H.265-DEAD",
            "Movie.2020.2160p.WEB-DL.DDP5.1.H.265-FOUR",
        ] {
            let r = row(&ranked, dead);
            assert!(
                !r.key.viable && r.key.start == 0,
                "{dead} at min {min_seeders}"
            );
        }
    }
    let ranked = Case::stick().rank(vec![
        seeded(1, "Movie.2020.1080p.WEB-DL.x264-FIVE", 5, 2.0),
        json!({ "name": "Torrentio", "title": "Movie.2020.1080p.WEB-DL.x264-NONE\n💾 2 GB",
                "infoHash": format!("{:040x}", 0x200), "fileIdx": 0 }),
    ]);
    for name in [
        "Movie.2020.1080p.WEB-DL.x264-FIVE",
        "Movie.2020.1080p.WEB-DL.x264-NONE",
    ] {
        let r = row(&ranked, name);
        assert!(r.key.viable && r.key.start == 1, "{name}");
    }
}

/// Code review r2 P2-4: free space is judged on what the engine fetches,
/// one episode of a pack: a 40 GB season (4 GB an episode) fits 20 GB.
#[test]
fn a_season_pack_needs_room_for_one_episode() {
    let case = Case {
        target: Target {
            title: "Show".into(),
            year: None,
            season: Some(1),
            episode: Some(5),
        },
        conditions: Conditions {
            runtime_s: Some(45 * 60),
            episodes_in_season: Some(10),
            free_bytes: Some(20 << 30),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![seeded(1, "Show.S01.1080p.BluRay.x264-GRP", 500, 40.0)]);
    assert_eq!(
        hidden(&ranked, HideReason::NoSpace),
        0,
        "{:?}",
        ranked.hidden
    );
    assert_eq!(ranked.rows.len(), 1);
}

/// Code review r2 P3: a number in the title's own name is no year, so
/// Blade Runner 2049 (2017) and Wonder Woman 1984 (2020) are not demoted.
#[test]
fn a_number_in_the_title_is_no_year() {
    for (title, year, release) in [
        (
            "Blade Runner 2049",
            2017,
            "Blade.Runner.2049.1080p.BluRay.x264-GRP",
        ),
        (
            "Wonder Woman 1984",
            2020,
            "Wonder.Woman.1984.1080p.WEB-DL.DDP5.1.H.264-GRP",
        ),
    ] {
        let case = Case {
            target: Target {
                title: title.into(),
                ..film()
            },
            conditions: Conditions {
                title_year: Some(year),
                ..engine().clone()
            },
            ..Case::stick()
        };
        let ranked = case.rank(vec![seeded(1, release, 300, 2.0)]);
        let best = ranked
            .best()
            .unwrap_or_else(|| panic!("{title}: {:?}", ranked.hidden));
        assert!(best.key.year_ok, "{title}");
    }
}

/// Code review r2 P3: a release named only in the original language that
/// names the title's year is kept ("Kis Uykusu 2014" for Winter Sleep); one
/// naming another year still hides.
#[test]
fn a_release_named_in_its_own_language_with_the_year_is_kept() {
    let case = Case {
        target: Target {
            title: "Winter Sleep".into(),
            ..film()
        },
        conditions: Conditions {
            title_year: Some(2014),
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        seeded(1, "Kis Uykusu 2014 BluRay 720p DTS x264-CHD", 31, 4.0),
        seeded(2, "Cidade De Deus (2002) 1080p BluRay x264", 80, 2.0),
    ]);
    assert_eq!(ranked.rows.len(), 1, "{:?}", ranked.hidden);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Kis Uykusu 2014 BluRay 720p DTS x264-CHD"
    );
    assert_eq!(hidden(&ranked, HideReason::WrongTitle), 1);
}

/// Code review r1 P3-4: on an mp4-only player a stated mp4 goes before a
/// file whose container is not stated; a player that opens mkv plays both.
#[test]
fn a_stated_mp4_goes_before_an_unstated_container_on_an_mp4_player() {
    let mut unstated = seeded(1, "Show.S02E03.1080p.WEB-DL.H.264-CZ", 300, 1.7);
    unstated["behaviorHints"] = json!({ "filename": "Show.S02E03.1080p.WEB-DL.H.264-CZ" });
    let mut mp4 = seeded(2, "Show.S02E03.1080p.WEB.h264-GRP", 60, 1.6);
    mp4["behaviorHints"] = json!({ "filename": "Show.S02E03.1080p.WEB.h264-GRP.mp4" });
    // The unstated one starts quickly (300), the mp4 should (60).
    let mut case = Case {
        target: Target {
            title: "Show".into(),
            year: None,
            season: Some(2),
            episode: Some(3),
        },
        caps: DeviceCaps {
            containers: ["mp4", "m4v", "mov", "m3u8"].map(str::to_owned).to_vec(),
            has_fallback_engine: false,
            ..DeviceCaps::basic_1080p()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![unstated.clone(), mp4.clone()]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Show.S02E03.1080p.WEB.h264-GRP"
    );
    assert_eq!(
        row(&ranked, "Show.S02E03.1080p.WEB-DL").key.playability,
        Playability::Unstated
    );

    case.caps = DeviceCaps::basic_1080p();
    let ranked = case.rank(vec![unstated, mp4]);
    assert_eq!(
        row(&ranked, "Show.S02E03.1080p.WEB-DL").key.playability,
        Playability::Native
    );
}

/// Dead-swarm failover: a swarm that sent almost nothing on this device
/// is not started first again, whatever seeders its addon lists; it says
/// it stalled.
#[test]
fn a_swarm_that_stalled_here_is_not_started_first() {
    let stalled = seeded(1, "Movie.2020.1080p.BluRay.x264-DEAD", 900, 2.0);
    let hash = stalled["infoHash"].as_str().unwrap().to_owned();
    let case = Case {
        conditions: Conditions {
            dead_swarms: vec![hash],
            ..engine().clone()
        },
        ..Case::stick()
    };
    let ranked = case.rank(vec![
        stalled,
        seeded(2, "Movie.2020.1080p.WEB-DL.x264-LIVE", 60, 2.0),
    ]);
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.WEB-DL.x264-LIVE"
    );
    let dead = row(&ranked, "Movie.2020.1080p.BluRay");
    assert!(!dead.key.viable);
    assert_eq!(dead.line, ReasonLine::Stalled);
}

/// A file that showed no picture on an hvc1-only player (an hev1 entry
/// without parameter sets) is not started first there and says why; a
/// player that shows it ranks it as before.
#[test]
fn a_file_that_showed_no_picture_here_is_not_started_first() {
    let rows = || {
        vec![
            seeded(1, "Movie.2020.1080p.WEB-DL.HEVC-BARE", 900, 4.0),
            seeded(2, "Movie.2020.1080p.BluRay.x264-FINE", 60, 2.0),
        ]
    };
    let mut case = Case::stick();
    let bare = row(&case.rank(rows()), "Movie.2020.1080p.WEB-DL")
        .stream
        .key
        .clone();
    case.conditions = Conditions {
        no_picture: vec![mq_streams::key_digest(&bare)],
        ..engine().clone()
    };
    let shows = case.rank(rows());
    assert_eq!(
        first_line(shows.best().unwrap()),
        "Movie.2020.1080p.WEB-DL.HEVC-BARE"
    );
    case.caps.hvc1_only = true;
    let ranked = case.rank(rows());
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-FINE"
    );
    let bare = row(&ranked, "Movie.2020.1080p.WEB-DL");
    assert!(!bare.key.viable);
    assert_eq!(bare.line, ReasonLine::NoPicture);
}

/// A version this device's player could not decode last time is not
/// started first again, on any device, and says why (Android walk NV-F2).
#[test]
fn a_version_the_player_could_not_decode_is_not_started_first() {
    let rows = || {
        vec![
            seeded(1, "Movie.2020.2160p.WEB-DL.x265-EIGHT", 900, 4.0),
            seeded(2, "Movie.2020.1080p.BluRay.x264-FINE", 60, 2.0),
        ]
    };
    let mut case = Case::stick();
    let failed = row(&case.rank(rows()), "Movie.2020.2160p")
        .stream
        .key
        .clone();
    case.conditions = Conditions {
        did_not_play: vec![mq_streams::key_digest(&failed)],
        ..engine().clone()
    };
    let ranked = case.rank(rows());
    assert_eq!(
        first_line(ranked.best().unwrap()),
        "Movie.2020.1080p.BluRay.x264-FINE"
    );
    let failed = row(&ranked, "Movie.2020.2160p");
    assert!(!failed.key.viable);
    assert_eq!(failed.line, ReasonLine::DidNotPlay);
}

/// 3.7: a large file says whether the limit is one the user chose.
#[test]
fn a_large_file_says_whether_the_limit_was_chosen() {
    let rows = || {
        vec![
            seeded(1, "Movie.2020.1080p.BluRay.x264-BIG", 900, 9.0),
            seeded(2, "Movie.2020.1080p.WEBRip.x264-FIT", 300, 3.0),
        ]
    };
    let large = |ranked: &Ranked| row(ranked, "Movie.2020.1080p.BluRay").line.clone();
    let default = Case::stick().rank(rows());
    assert!(matches!(
        large(&default),
        ReasonLine::Large { chosen: false, .. }
    ));
    let case = Case {
        conditions: Conditions {
            limit_chosen: true,
            ..engine().clone()
        },
        ..Case::stick()
    };
    assert!(matches!(
        large(&case.rank(rows())),
        ReasonLine::Large { chosen: true, .. }
    ));
}

/// Plans/play-every-version.md 2d: the size limit hides over-limit rows
/// only while something else is left; when it would leave nothing they
/// come back, ranked as with no limit, and the other gates still hold.
#[test]
fn the_size_limit_lets_rows_back_rather_than_leave_nothing() {
    let caps = DeviceCaps::basic_1080p();
    let torrents = || -> Vec<ParsedStream> {
        sample()
            .into_iter()
            .filter(|s| s.addon_id == "torrentio")
            .collect()
    };
    let ranked = |limit_gb: u64| {
        let prefs = StreamPrefs {
            max_size_bytes: Some(limit_gb << 30),
            ..StreamPrefs::default()
        };
        let input = RankInput {
            caps: &caps,
            prefs: &prefs,
            target: &target(),
            last_binge_group: None,
            kids: None,
            conditions: engine(),
        };
        rank(torrents(), &input)
    };
    let names = |r: &Ranked| -> Vec<String> {
        r.rows
            .iter()
            .map(|r| r.stream.release.group.clone().unwrap_or_default())
            .collect()
    };
    // 3 GB: the 2.1 GB file plays, the 8.4 GB one stays hidden.
    let some = ranked(3);
    assert_eq!(names(&some), ["ETHEL"]);
    assert_eq!(hidden(&some, HideReason::TooLarge), 2);
    // 1 GB: nothing fits, so both of this episode come back; another
    // episode and another title stay hidden.
    let none_fit = ranked(1);
    let mut back = names(&none_fit);
    back.sort();
    assert_eq!(back, ["ETHEL", "FLUX"]);
    assert_eq!(hidden(&none_fit, HideReason::TooLarge), 0);
    assert_eq!(hidden(&none_fit, HideReason::WrongEpisode), 1);
    assert_eq!(hidden(&none_fit, HideReason::WrongTitle), 1);
}
