//! Probes of real public files over the network. Ignored by default:
//! `cargo test -p mq-media --test live -- --ignored --nocapture --test-threads=1`

use mq_http::{ClientConfig, HttpClient};
use mq_media::*;
use std::sync::Arc;
use std::time::Duration;

fn reader(url: &str) -> HttpRangeReader {
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    HttpRangeReader::new(http, url, Vec::new(), Arc::new(ByteCache::new(16 << 20)))
}

/// Probes with the default budget and, when a slow origin exhausts it,
/// again with 10 s so the assertions check parsing rather than latency.
async fn probe_url(url: &str) -> StreamProbe {
    let p = probe_with(url, ProbeBudget::default()).await;
    if !p.budget_exhausted {
        return p;
    }
    let relaxed = ProbeBudget {
        max_time: Duration::from_secs(10),
        ..Default::default()
    };
    probe_with(url, relaxed).await
}

async fn probe_with(url: &str, budget: ProbeBudget) -> StreamProbe {
    let r = reader(url);
    let p = probe(&r, budget, &CancellationToken::new()).await;
    let v = p.video.as_ref();
    println!(
        "{url} (budget {} ms)\n  container={:?} duration_ms={:?} video={:?} {}x{} fps={:?} depth={:?}\n  audio={:?} subs={} seek_index={:?}\n  bytes_read={} elapsed_ms={} exhausted={} error={:?}",
        budget.max_time.as_millis(),
        p.container.value,
        p.duration_ms.value,
        v.and_then(|v| v.codec.value.clone()),
        v.and_then(|v| v.width.value).unwrap_or(0),
        v.and_then(|v| v.height.value).unwrap_or(0),
        v.and_then(|v| v.fps.value),
        v.and_then(|v| v.bit_depth.value),
        p.audio
            .iter()
            .map(|a| (a.codec.value.clone(), a.channels.value))
            .collect::<Vec<_>>(),
        p.subtitles.len(),
        p.seek_index.value,
        p.bytes_read,
        p.elapsed.as_millis(),
        p.budget_exhausted,
        p.error,
    );
    p
}

#[tokio::test]
#[ignore = "network"]
async fn live_mp4_blender_sintel_trailer() {
    let url = "https://download.blender.org/durian/trailer/sintel_trailer-1080p.mp4";
    let p = probe_url(url).await;
    assert_eq!(p.video.unwrap().codec.value, Some(VideoCodec::H264));
    assert_eq!(p.audio[0].codec.value, Some(AudioCodec::Aac));
    // moov sits after mdat in this file.
    assert!(p.seek_index.value.unwrap().offset > 14_000_000);
}

#[tokio::test]
#[ignore = "network"]
async fn live_mkv_blender_sintel() {
    let url = "https://download.blender.org/durian/movies/Sintel.2010.1080p.mkv";
    let p = probe_url(url).await;
    assert_eq!(p.container.value, Some(Container::Matroska));
    assert_eq!(p.video.unwrap().codec.value, Some(VideoCodec::H264));
    assert!(!p.subtitles.is_empty());
}

#[tokio::test]
#[ignore = "network"]
async fn live_mkv_jellyfish() {
    let url = "https://repo.jellyfin.org/archive/jellyfish/media/jellyfish-10-mbps-hd-h264.mkv";
    let p = probe_url(url).await;
    assert_eq!(p.container.value, Some(Container::Matroska));
    assert_eq!(p.video.unwrap().codec.value, Some(VideoCodec::H264));
}

#[tokio::test]
#[ignore = "network"]
async fn live_mkv_hevc_10bit() {
    // ffprobe: hevc Main 10, level 120, yuv420p10le, transfer unknown.
    let url =
        "https://repo.jellyfin.org/archive/jellyfish/media/jellyfish-3-mbps-hd-hevc-10bit.mkv";
    let v = probe_url(url).await.video.unwrap();
    assert_eq!(v.codec.value, Some(VideoCodec::Hevc));
    assert_eq!(v.profile.value.as_deref(), Some("Main 10"));
    assert_eq!(v.level.value.as_deref(), Some("4.0"));
    assert_eq!(v.bit_depth.value, Some(10));
    assert_eq!(v.hdr, Field::unknown());
}

#[tokio::test]
#[ignore = "network"]
async fn live_mislabeled_mkv_is_sniffed_as_mp4() {
    let url =
        "https://test-videos.co.uk/vids/bigbuckbunny/mkv/1080/Big_Buck_Bunny_1080_10s_30MB.mkv";
    let p = probe_url(url).await;
    assert_eq!(p.container.value, Some(Container::Mp4));
    assert!(p.video.is_some());
}

#[tokio::test]
#[ignore = "network"]
async fn live_mp4_behind_redirect() {
    let url =
        "https://archive.org/download/BigBuckBunny_124/Content/big_buck_bunny_720p_surround.mp4";
    let p = probe_url(url).await;
    assert_eq!(p.video.unwrap().codec.value, Some(VideoCodec::H264));
}

#[tokio::test]
#[ignore = "network"]
async fn live_opensubtitles_breakdance_vector() {
    let r = reader("https://static.opensubtitles.org/addons/avi/breakdance.avi");
    let started = std::time::Instant::now();
    let (hash, size) = opensubtitles_hash(&r).await.unwrap();
    println!(
        "breakdance.avi hash={hash} size={size} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    assert_eq!((hash.as_str(), size), ("8e245d9679d31e12", 12_909_756));
}
