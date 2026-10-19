//! Probe a stream, then play it in mpv and measure time to first frame
//! through mpv's JSON IPC (`playback-restart` marks the first shown frame).

use mq_http::{ClientConfig, HttpClient};
use mq_media::{ByteCache, CancellationToken, HttpRangeReader, ProbeBudget};
use serde_json::{Value, json};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;

pub struct PlayOptions {
    pub headless: bool,
    pub seconds: u64,
    /// Resume position.
    pub start_ms: Option<u64>,
    /// Seek to these percentages after the first frame and time each.
    pub seeks: Vec<f64>,
}

pub async fn play_url(url: &str, options: PlayOptions) -> Result<Value, String> {
    let started = Instant::now();
    let http = HttpClient::new(ClientConfig::default()).map_err(|e| e.to_string())?;
    let reader = HttpRangeReader::new(http, url, vec![], Arc::new(ByteCache::new(8 * 1024 * 1024)))
        .allow_private(url.starts_with("http://127.0.0.1:"));
    let probe = mq_media::probe(&reader, ProbeBudget::default(), &CancellationToken::new()).await;
    let probe_ms = started.elapsed().as_millis() as u64;
    let video = probe.video.as_ref().map(|v| format!("{v:?}"));
    super::emit(json!({
        "event": "probe",
        "ms": probe_ms,
        "bytes": probe.bytes_read,
        "budget_exhausted": probe.budget_exhausted,
        "video": video,
        "audio_tracks": probe.audio.len(),
        "subtitle_tracks": probe.subtitles.len(),
        "error": probe.error.map(|e| e.to_string()),
    }));

    let socket = std::env::temp_dir().join(format!("mq-mpv-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let mut args = vec![
        format!("--input-ipc-server={}", socket.display()),
        "--ytdl=no".into(),
        "--load-scripts=no".into(),
        "--config=no".into(),
        "--really-quiet".into(),
    ];
    // With seeks mpv quits after the last one; `--length` would end playback
    // once a seek jumps past it.
    if options.seeks.is_empty() {
        args.push(format!("--length={}", options.seconds));
    }
    if let Some(ms) = options.start_ms {
        args.push(format!("--start={:.3}", ms as f64 / 1000.0));
    }
    if options.headless {
        args.extend(["--vo=null".into(), "--ao=null".into()]);
    }
    args.push(url.to_owned());
    let mpv_started = Instant::now();
    let mut child = Command::new("mpv")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("cannot start mpv: {e}"))?;

    let stream = connect(&socket, Duration::from_secs(5)).await?;
    let (read, mut write) = stream.into_split();
    // `paused-for-cache` counts stalls (plans/m13-stream-choice.md 6.4).
    for (id, name) in [(1, "video-params"), (2, "paused-for-cache")] {
        let observe = json!({ "command": ["observe_property", id, name] }).to_string() + "\n";
        write
            .write_all(observe.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut lines = BufReader::new(read).lines();
    let mut first_frame_ms = None;
    let mut file_loaded_ms = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let line = match tokio::time::timeout(Duration::from_secs(30), lines.next_line()).await {
            Ok(Ok(Some(line))) => line,
            _ => break,
        };
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match event.get("event").and_then(Value::as_str) {
            Some("file-loaded") => file_loaded_ms = Some(mpv_started.elapsed().as_millis() as u64),
            Some("playback-restart") => {
                first_frame_ms = Some(mpv_started.elapsed().as_millis() as u64);
                break;
            }
            Some("end-file") => break,
            _ => {}
        }
    }
    // Stalls while it plays: each time mpv pauses to fill its cache, until
    // the file ends or the playback ran far past its length.
    let (mut stalls, mut stall_ms, mut stalled_at) = (0u32, 0u64, None::<Instant>);
    let mut ended = false;
    if first_frame_ms.is_some() && options.seeks.is_empty() {
        let cap = Instant::now() + Duration::from_secs(options.seconds + 120);
        while Instant::now() < cap {
            let left = cap.saturating_duration_since(Instant::now());
            let Ok(Ok(Some(line))) = tokio::time::timeout(left, lines.next_line()).await else {
                break;
            };
            let event = serde_json::from_str::<Value>(&line).unwrap_or_default();
            match event.get("event").and_then(Value::as_str) {
                Some("property-change") if event["name"] == "paused-for-cache" => {
                    match (event["data"].as_bool(), stalled_at) {
                        (Some(true), None) => {
                            stalls += 1;
                            stalled_at = Some(Instant::now());
                        }
                        (Some(false), Some(at)) => {
                            stall_ms += at.elapsed().as_millis() as u64;
                            stalled_at = None;
                        }
                        _ => {}
                    }
                }
                Some("end-file") => {
                    ended = true;
                    break;
                }
                _ => {}
            }
        }
        if let Some(at) = stalled_at {
            stall_ms += at.elapsed().as_millis() as u64;
        }
        if !ended {
            let quit = json!({ "command": ["quit"] }).to_string() + "\n";
            let _ = write.write_all(quit.as_bytes()).await;
        }
    }
    // Seek latency: from the seek command to mpv's next `playback-restart`
    // (the first frame shown at the new position).
    let mut seeks = Vec::new();
    for pct in options.seeks.iter().filter(|_| first_frame_ms.is_some()) {
        let cmd = json!({ "command": ["seek", pct, "absolute-percent"] }).to_string() + "\n";
        if write.write_all(cmd.as_bytes()).await.is_err() {
            break;
        }
        let sent = Instant::now();
        let mut restarted = None;
        while sent.elapsed() < SEEK_TIMEOUT {
            let Ok(Ok(Some(line))) = tokio::time::timeout(SEEK_TIMEOUT, lines.next_line()).await
            else {
                break;
            };
            let event = serde_json::from_str::<Value>(&line).unwrap_or_default();
            match event.get("event").and_then(Value::as_str) {
                Some("playback-restart") => {
                    restarted = Some(sent.elapsed().as_millis() as u64);
                    break;
                }
                Some("end-file") => break,
                _ => {}
            }
        }
        seeks.push(json!({ "pct": pct, "ms": restarted }));
        if restarted.is_none() {
            break;
        }
    }
    if !options.seeks.is_empty() {
        let quit = json!({ "command": ["quit"] }).to_string() + "\n";
        let _ = write.write_all(quit.as_bytes()).await;
    }
    let _ = child.wait().await;
    let _ = std::fs::remove_file(&socket);
    Ok(json!({
        "event": "played",
        "probe_ms": probe_ms,
        "file_loaded_ms": file_loaded_ms,
        "first_frame_ms": first_frame_ms,
        "stalls": stalls,
        "stall_ms": stall_ms,
        "ended": ended,
        "seeks": seeks,
        "total_ms": started.elapsed().as_millis() as u64,
    }))
}

const SEEK_TIMEOUT: Duration = Duration::from_secs(60);

async fn connect(path: &std::path::Path, within: Duration) -> Result<UnixStream, String> {
    let deadline = Instant::now() + within;
    loop {
        match UnixStream::connect(path).await {
            Ok(stream) => return Ok(stream),
            Err(_) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            Err(e) => return Err(format!("mpv ipc not available: {e}")),
        }
    }
}
