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
    };
    let ranked = rank(sample(), &input);
    let best = ranked.best().unwrap();
    assert_eq!(best.stream.addon_id, "debrid-addon");
    assert_eq!(best.key.tier, 4);
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

    let prefs = StreamPrefs::default();
    let only_torrents: Vec<_> = sample()
        .into_iter()
        .filter(|s| s.addon_id == "torrentio")
        .collect();
    let input = RankInput {
        caps: &caps_4k_dv(),
        prefs: &prefs,
        target: &target(),
        last_binge_group: Some("torrentio|4k|FLUX"),
    };
    let ranked = rank(only_torrents, &input);
    let best = ranked.best().unwrap();
    assert!(best.key.binge);
    assert!(best.reasons.contains(&Reason::SameReleaseAsLast));
}

#[test]
fn rank_key_orders_tiers_before_scores() {
    let low_tier_high_score = RankKey {
        tier: 2,
        binge: true,
        playability: Playability::Native,
        effective_height: 2160,
        within: 1000,
    };
    let high_tier_low_score = RankKey {
        tier: 3,
        binge: false,
        playability: Playability::NeedsTranscode,
        effective_height: 480,
        within: 0,
    };
    assert!(high_tier_low_score > low_tier_high_score);
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
