use mq_protocol::{Source, decode_streams, decode_subtitles, normalize_info_hash};
use serde_json::json;

#[test]
fn torrentio_streams_classify_as_torrents() {
    let bytes = include_bytes!("fixtures/torrentio_movie.json");
    let resp = decode_streams(bytes).expect("decodes");
    assert_eq!(resp.skipped, 0);
    assert!(resp.streams.len() > 10);
    for stream in &resp.streams {
        match stream.source() {
            Source::Torrent { info_hash, .. } => assert_eq!(info_hash.len(), 40),
            other => panic!("expected torrent, got {other:?}"),
        }
        assert!(
            stream.description.is_some(),
            "title alias maps to description"
        );
    }
    let first = &resp.streams[0];
    assert!(first.behavior_hints.binge_group.is_some());
    assert!(first.behavior_hints.filename.is_some());
}

#[test]
fn opensubtitles_keeps_unknown_keys() {
    let bytes = include_bytes!("fixtures/opensubtitles_movie.json");
    let resp = decode_subtitles(bytes).expect("decodes");
    assert!(!resp.subtitles.is_empty());
    let sub = &resp.subtitles[0];
    assert!(!sub.lang.is_empty());
    assert!(sub.extra.contains_key("SubEncoding"));
}

#[test]
fn client_resolve_only_stream_is_kept() {
    let body = json!({ "streams": [{
        "name": "Debrid 4K",
        "title": "Show.S01E02.2160p.WEB-DL.DV.mkv",
        "clientResolve": {
            "type": "debrid", "service": "realdebrid",
            "infoHash": "E8F4C7C2D4A4BA911181D993273B43DE3292712A",
            "fileIdx": "3", "season": 1, "episode": 2, "isCached": true,
            "stream": { "raw": { "parsed": { "resolution": "2160p" } } }
        },
        "behaviorHints": { "cached": true, "service": "realdebrid", "seeders": "41" }
    }]});
    let resp = decode_streams(body.to_string().as_bytes()).unwrap();
    let stream = &resp.streams[0];
    assert_eq!(stream.behavior_hints.seeders, Some(41));
    match stream.source() {
        Source::ClientResolve(cr) => {
            assert_eq!(cr.file_idx, Some(3));
            assert_eq!(cr.is_cached, Some(true));
        }
        other => panic!("expected client resolve, got {other:?}"),
    }
}

#[test]
fn magnet_url_becomes_torrent_with_trackers() {
    let body = json!({ "streams": [{
        "url": "magnet:?xt=urn:btih:e8f4c7c2d4a4ba911181d993273b43de3292712a&tr=udp%3A%2F%2Ftracker.example%3A80"
    }]});
    let resp = decode_streams(body.to_string().as_bytes()).unwrap();
    match resp.streams[0].source() {
        Source::Torrent { trackers, .. } => {
            assert_eq!(
                trackers,
                vec!["tracker:udp://tracker.example:80".to_string()]
            )
        }
        other => panic!("expected torrent, got {other:?}"),
    }
}

#[test]
fn nulls_and_bad_items_do_not_fail_the_list() {
    let body = json!({ "streams": [
        { "url": "https://cdn.example/v.mkv", "subtitles": null, "behaviorHints": null, "fileIdx": null },
        "not an object",
        { "ytId": "abc123" },
        { "name": "nothing playable" }
    ]});
    let resp = decode_streams(body.to_string().as_bytes()).unwrap();
    assert_eq!(resp.skipped, 1);
    assert!(matches!(resp.streams[0].source(), Source::Http { .. }));
    assert!(matches!(resp.streams[1].source(), Source::YouTube { .. }));
    assert!(matches!(resp.streams[2].source(), Source::Invalid { .. }));
}

#[test]
fn base32_info_hash_normalizes_to_hex() {
    // base32 of 20 zero bytes plus a known value round trip
    assert_eq!(
        normalize_info_hash("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").as_deref(),
        Some("0000000000000000000000000000000000000000")
    );
    assert_eq!(
        normalize_info_hash("E8F4C7C2D4A4BA911181D993273B43DE3292712A").as_deref(),
        Some("e8f4c7c2d4a4ba911181d993273b43de3292712a")
    );
    assert!(normalize_info_hash("xyz").is_none());
}

#[test]
fn missing_key_is_an_error_and_null_is_empty() {
    assert!(decode_streams(br#"{"metas": []}"#).is_err());
    assert_eq!(
        decode_streams(br#"{"streams": null}"#)
            .unwrap()
            .streams
            .len(),
        0
    );
}

#[test]
fn source_serializes_without_key_collisions() {
    let body =
        json!({ "streams": [{ "clientResolve": { "type": "debrid", "service": "torbox" } }] });
    let resp = decode_streams(body.to_string().as_bytes()).unwrap();
    let value = serde_json::to_value(resp.streams[0].source()).unwrap();
    assert_eq!(value["source"], "clientResolve");
    assert_eq!(value["type"], "debrid");
}
