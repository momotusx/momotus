//! `mq play-magnet`: the torrent engine and the loopback server alone, with
//! a timing per stage. Players and the byte probe both read through the
//! loopback URL, so the numbers include the HTTP leg.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mq_engine::{Engine, EngineConfig, FileHint};
use mq_http::{ClientConfig, HttpClient, Priority, RequestOptions};
use mq_server::{StreamServer, Upstream};
use serde_json::{Value, json};

use crate::play::{PlayOptions, play_url};

pub struct MagnetOptions {
    pub file: Option<usize>,
    pub headless: bool,
    pub seconds: u64,
    /// Seek to 25, 50 and 75% and time each.
    pub seek: bool,
    /// librqbit's own picker instead of DWH.
    pub stock: bool,
    /// No player: time the first byte and 4 MiB at the head and at each seek
    /// point through the loopback (for non-video files).
    pub bytes: bool,
    /// Data directory; a fresh temporary one (deleted after the run) when
    /// unset.
    pub data_dir: Option<PathBuf>,
    /// Serve pieces to other peers.
    pub upload: bool,
    /// A peer to dial, allowed by the policy; the DHT stays off with it.
    pub peer: Option<std::net::SocketAddr>,
}

const SEEKS: [f64; 3] = [25.0, 50.0, 75.0];
const METADATA_TIMEOUT: Duration = Duration::from_secs(120);
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const MIB: u64 = 1024 * 1024;
/// The engine's reads are a profile's; this tool plays outside any.
const CLI: &str = "cli";

fn ms(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

/// Peak resident set of this process only (the engine, the loopback server
/// and the probe; mpv is a separate process and not counted).
fn peak_rss_bytes() -> Option<u64> {
    // SAFETY: getrusage only writes the struct we pass.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return None;
    }
    let max = u64::try_from(usage.ru_maxrss).ok()?;
    // macOS reports bytes, Linux kilobytes.
    Some(if cfg!(target_os = "macos") {
        max
    } else {
        max * 1024
    })
}

/// `(info hash, trackers)` from a magnet URI or a bare info hash.
fn parse_magnet(input: &str) -> Result<(String, Vec<String>), String> {
    if !input.starts_with("magnet:") {
        return Ok((input.to_owned(), Vec::new()));
    }
    let url = url::Url::parse(input).map_err(|e| format!("bad magnet: {e}"))?;
    let mut hash = None;
    let mut trackers = Vec::new();
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "xt" => hash = v.strip_prefix("urn:btih:").map(str::to_owned),
            "tr" => trackers.push(v.into_owned()),
            _ => {}
        }
    }
    Ok((hash.ok_or("magnet without urn:btih")?, trackers))
}

/// Time to the first body byte and to `want` bytes of `bytes=offset-`.
async fn timed_read(http: &HttpClient, url: &str, offset: u64, want: u64) -> Value {
    let opts = RequestOptions {
        priority: Priority::Playback,
        timeout: READY_TIMEOUT,
        // A torrent read may wait long for its first piece: the header wait
        // gets the read's own limit, not a stream's default.
        hop_headers: Some(READY_TIMEOUT),
        allow_private: true,
        ..Default::default()
    };
    let started = Instant::now();
    let mut response = match http
        .stream(url, Some(format!("bytes={offset}-")), &opts)
        .await
    {
        Ok(r) => r,
        Err(e) => return json!({ "offset": offset, "error": e.to_string() }),
    };
    let (mut got, mut first, mut per_mib) = (0u64, None, Vec::new());
    while got < want {
        match tokio::time::timeout(READY_TIMEOUT, response.next_chunk()).await {
            Ok(Some(Ok(chunk))) => {
                got += chunk.len() as u64;
                first.get_or_insert_with(|| ms(started));
                while (per_mib.len() as u64 + 1) * MIB <= got.min(want) {
                    per_mib.push(ms(started));
                }
            }
            _ => break,
        }
    }
    json!({
        "offset": offset,
        "first_byte_ms": first,
        "mib4_ms": (got >= want).then(|| ms(started)),
        "per_mib_ms": per_mib,
    })
}

/// `mq scrape`: one scrape on a fresh engine, as the app's first: its own
/// temporary folder, the production address policy, the DHT and default
/// trackers on, `wait_ms` after the start, the trackers warmed from the
/// start with `warm` (a title's page opened first). Per hash its count; per
/// tracker the stages it reached and the one the budget cut.
pub async fn scrape(hashes: &[String], wait_ms: u64, warm: bool) -> Result<Value, String> {
    let dir = std::env::temp_dir().join(format!("mq-scrape-{}", std::process::id()));
    let result = scrape_in(&dir, hashes, wait_ms, warm).await;
    let _ = std::fs::remove_dir_all(&dir);
    result
}

async fn scrape_in(
    dir: &std::path::Path,
    hashes: &[String],
    wait_ms: u64,
    warm: bool,
) -> Result<Value, String> {
    let http = HttpClient::new(ClientConfig::default()).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let config = EngineConfig {
        upload: false,
        ..EngineConfig::new(dir, http.torrent_scope(&[]))
    };
    let engine = Engine::start(config).await.map_err(|e| e.to_string())?;
    let engine_start_ms = ms(started);
    if warm {
        let engine = engine.clone();
        tokio::spawn(async move { engine.warm_trackers().await });
    }
    tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    let asked: Vec<(&str, &[String])> = hashes.iter().map(|h| (h.as_str(), &[][..])).collect();
    let at = Instant::now();
    let counts = engine.scrape(&asked).await;
    let scrape_ms = ms(at);
    let trackers: Vec<Value> = engine
        .last_scrape()
        .into_iter()
        .map(|t| {
            let cut = match (t.answered_ms, t.failed) {
                (Some(_), _) | (_, Some(_)) => None,
                _ if !t.tracker.starts_with("udp") => Some("http"),
                _ if t.resolved_ms.is_none() => Some("resolve"),
                _ if t.connected_ms.is_none() => Some("connect"),
                _ => Some("scrape"),
            };
            json!({
                "tracker": t.tracker, "resolved_ms": t.resolved_ms,
                "connected_ms": t.connected_ms, "reused": t.reused, "answered_ms": t.answered_ms,
                "failed": t.failed, "cut": cut,
            })
        })
        .collect();
    Ok(json!({
        "event": "scrape",
        "engine_start_ms": engine_start_ms,
        "wait_ms": wait_ms,
        "warm": warm,
        "scrape_ms": scrape_ms,
        "answered": counts.iter().filter(|c| c.is_some()).count(),
        "counts": hashes.iter().zip(&counts).map(|(h, c)| json!({
            "hash": h, "seeders": c.map(|c| c.seeders), "leechers": c.map(|c| c.leechers),
        })).collect::<Vec<_>>(),
        "trackers": trackers,
    }))
}

pub async fn play_magnet(input: &str, o: MagnetOptions) -> Result<Value, String> {
    let (hash, trackers) = parse_magnet(input)?;
    let temporary = o.data_dir.is_none();
    let dir = o
        .data_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("mq-magnet-{}", std::process::id())));
    let result = run(&hash, &trackers, &dir, o).await;
    if temporary {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

async fn run(
    hash: &str,
    trackers: &[String],
    dir: &std::path::Path,
    o: MagnetOptions,
) -> Result<Value, String> {
    let started = Instant::now();
    let http = HttpClient::new(ClientConfig::default()).map_err(|e| e.to_string())?;
    let local: Vec<std::net::IpAddr> = o.peer.iter().map(|p| p.ip()).collect();
    let config = EngineConfig {
        dwh: !o.stock,
        upload: o.upload,
        discovery: o.peer.is_none(),
        ..EngineConfig::new(dir, http.torrent_scope(&local))
    };
    let engine = Engine::start(config).await.map_err(|e| e.to_string())?;
    let peers: Vec<std::net::SocketAddr> = o.peer.into_iter().collect();
    let t = tokio::time::timeout(METADATA_TIMEOUT, engine.add_magnet(hash, trackers, &peers))
        .await
        .map_err(|_| "no metadata in time".to_owned())?
        .map_err(|e| e.to_string())?;
    let metadata_ms = ms(started);
    let hint = o.file.map_or(FileHint::Largest, FileHint::Index);
    let file = engine
        .select_file(t, CLI, &hint)
        .await
        .map_err(|e| e.to_string())?;
    let selected = Instant::now();
    let reader = engine
        .open_reader(t, CLI, file)
        .await
        .map_err(|e| e.to_string())?;
    let info = reader.file().clone();
    crate::emit(json!({
        "event": "metadata", "ms": metadata_ms, "file": info.path, "len": info.len,
        "files": engine.files(t).map(|f| f.len()).unwrap_or(0),
    }));

    let server = StreamServer::start(http.clone())
        .await
        .map_err(|e| e.to_string())?;
    let name = info
        .path
        .rsplit('/')
        .next()
        .unwrap_or(&info.path)
        .to_owned();
    let lease = server.register(
        Upstream::Torrent {
            claim: engine.claim(t, CLI).map_err(|e| e.to_string())?,
            file,
        },
        &name,
    );
    let url = lease.url.clone();
    // A player's first request (time to first byte and to 4 MiB through the
    // loopback) runs beside the readiness wait. A player starts at Ready, as
    // in the app; the byte probe needs no container tail and starts its seeks
    // after the head read (Ready is then recorded if it came by the end).
    let mut head = reader.range(0, None).await.map_err(|e| e.to_string())?;
    let first = tokio::spawn({
        let (http, url) = (http.clone(), url.clone());
        async move { timed_read(&http, &url, 0, 4 * MIB).await }
    });
    // The task keeps the head range open (its demand stays) until it ends.
    let mut ready_task = Some(tokio::spawn(async move {
        let ready = tokio::time::timeout(READY_TIMEOUT, head.ready()).await;
        (ready.ok().and_then(Result::ok).map(|()| ms(selected)), head)
    }));
    let (mut ready_ms, mut peers_at_ready, mut kept_head) = (None, None, None);
    if !o.bytes {
        let task = ready_task.take().expect("set above");
        let (ready, head) = task.await.map_err(|e| e.to_string())?;
        let peers = engine.stats(t).await.map_err(|e| e.to_string())?.peers;
        crate::emit(json!({ "event": "ready", "ms": ready, "peers": peers }));
        ready_ms = Some(ready.ok_or("the head and tail were not verified in time")?);
        (peers_at_ready, kept_head) = (Some(peers), Some(head));
    }

    let played = if o.bytes {
        let mut seeks = Vec::new();
        for pct in SEEKS {
            let offset = (info.len as f64 * pct / 100.0) as u64;
            let mut seek = timed_read(&http, &url, offset, 4 * MIB).await;
            if let (Ok(s), Some(obj)) = (engine.stats(t).await, seek.as_object_mut()) {
                obj.insert(
                    "after".into(),
                    json!({ "peers": s.peers, "snubs": s.snubs, "hedges": s.hedges,
                            "cancels": s.cancels, "dup_bytes": s.dup_bytes,
                            "down_bytes_per_s": s.down_bytes_per_s }),
                );
            }
            seeks.push(seek);
        }
        json!({ "seeks": seeks })
    } else {
        let options = PlayOptions {
            headless: o.headless,
            seconds: o.seconds,
            start_ms: None,
            seeks: if o.seek { SEEKS.to_vec() } else { Vec::new() },
        };
        play_url(&url, options).await?
    };
    if let Some(task) = ready_task.take() {
        if task.is_finished() {
            ready_ms = task.await.ok().and_then(|(ready, _)| ready);
        } else {
            task.abort();
        }
    }
    drop(kept_head);
    let first = first.await.unwrap_or_default();
    let stats = engine.stats(t).await.map_err(|e| e.to_string())?;
    let dialed = engine.connections();
    // The lease's claim ends first: the engine keeps a claimed torrent.
    drop(lease);
    let _ = engine.remove(t, false).await;
    Ok(json!({
        "event": "run",
        "file": info.path,
        "len": info.len,
        "mode": if stats.dwh { "dwh" } else { "stock" },
        "metadata_ms": metadata_ms,
        "ttfb_ms": first.get("first_byte_ms"),
        "head_4mib_ms": first.get("mib4_ms"),
        "ready_ms": ready_ms,
        "engine_ready_ms": stats.ready_ms,
        "first_frame_ms": played.get("first_frame_ms"),
        "probe_ms": played.get("probe_ms"),
        "seeks": played.get("seeks"),
        "peers_at_ready": peers_at_ready,
        "peers": stats.peers,
        "dialed_tcp": dialed.0,
        "dialed_utp": dialed.1,
        "downloaded_bytes": stats.downloaded_bytes,
        "requests": stats.requests,
        "cancels": stats.cancels,
        "hedges": stats.hedges,
        "dup_bytes": stats.dup_bytes,
        "total_ms": ms(started),
        "peak_rss_bytes": peak_rss_bytes(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_magnets_and_bare_hashes() {
        let (h, tr) = parse_magnet(
            "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=wss%3A%2F%2Ftracker.btorrent.xyz",
        )
        .unwrap();
        assert_eq!(h, "08ada5a7a6183aae1e09d831df6748d566095a10");
        assert_eq!(
            tr,
            ["udp://explodie.org:6969", "wss://tracker.btorrent.xyz"]
        );
        assert_eq!(parse_magnet("abc").unwrap().0, "abc");
        assert!(parse_magnet("magnet:?dn=x").is_err());
    }
}
