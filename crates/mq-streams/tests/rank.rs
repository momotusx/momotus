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
        playability: if best(5) {
            Playability::Native
        } else {
            Playability::NeedsTranscode
        },
        size_ok: best(6),
        start: if best(7) { 4 } else { 0 },
        binge: best(8),
        effective_height: if best(9) { 2160 } else { 0 },
        within: if best(10) { 1000 } else { 0 },
    };
    for i in 0..11 {
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

/// Mutation 6: a torrent larger than the free space less 1 GiB is hidden.
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
