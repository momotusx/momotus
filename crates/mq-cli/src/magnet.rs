//! `mq play-magnet`: the torrent engine and the loopback server alone, with
//! a timing per stage. Players and the byte probe both read through the
//! loopback URL, so the numbers include the HTTP leg.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use mq_engine::{Engine, EngineConfig, FileHint, PieceCheck, TorrentHandle};
use mq_http::{ClientConfig, HttpClient, Priority, RequestOptions};
use mq_server::{StreamServer, Upstream};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
    /// No player: the walk (`walk`), its whole read's rate in bytes per
    /// second, and the offset that read starts at.
    pub walk: bool,
    pub walk_rate: Option<u64>,
    pub walk_for: Option<Duration>,
    pub resume_at: Option<u64>,
    pub cache_budget: Option<u64>,
    pub download_limit: Option<u32>,
    /// The file's rate in bytes per second, for the engine.
    pub rate: Option<u64>,
    /// The walk-only window override, in bytes.
    pub window: Option<u64>,
    /// No open-pieces limit in the window.
    pub no_open_limit: bool,
    /// Bench only: files kept as piece files, and the free space set.
    pub no_punch: bool,
    pub free: Option<u64>,
}

/// The P2P speed the CLI plans a window with (the app's prior).
const FILL_ESTIMATE: u64 = 5_000_000;

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

fn read_options() -> RequestOptions {
    RequestOptions {
        priority: Priority::Playback,
        timeout: READY_TIMEOUT,
        // A torrent read may wait long for its first piece: the header wait
        // gets the read's own limit, not a stream's default.
        hop_headers: Some(READY_TIMEOUT),
        allow_private: true,
        ..Default::default()
    }
}

/// Bytes allocated under `dir`: blocks, so a hole counts nothing.
fn allocated(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => allocated(&e.path()),
            Ok(m) => m.blocks() * 512,
            Err(_) => 0,
        })
        .sum()
}

/// Nearest-rank percentile.
fn percentile(values: &[u64], p: f64) -> Option<u64> {
    let mut v = values.to_vec();
    v.sort_unstable();
    let rank = (p * v.len() as f64).ceil() as usize;
    v.get(rank.max(1) - 1).copied()
}

/// `key` of each object in the array `list`.
fn each(list: Option<&Value>, key: &str) -> Vec<u64> {
    list.and_then(Value::as_array)
        .map(|l| l.iter().filter_map(|v| v[key].as_u64()).collect())
        .unwrap_or_default()
}

/// `want` bytes of `bytes=offset-` through the loopback, at most `rate`
/// bytes per second when set and for at most `stop` when set, each whole
/// piece checked. A stall is a wait of over 1 s for the next bytes after
/// the first. The fill is the bytes of each whole second from the first
/// byte, p10 and p50.
async fn read_checked(
    http: &HttpClient,
    url: &str,
    (offset, want): (u64, u64),
    (rate, stop): (Option<u64>, Option<Duration>),
    check: &mut PieceCheck,
) -> Value {
    check.seek(offset);
    let started = Instant::now();
    let mut response = match http
        .stream(url, Some(format!("bytes={offset}-")), &read_options())
        .await
    {
        Ok(r) => r,
        Err(e) => return json!({ "offset": offset, "error": e.to_string() }),
    };
    let (mut got, mut first, mut stalls, mut stall_ms) = (0u64, None, 0u32, 0u64);
    let (mut sha, mut mib10) = (Sha256::new(), None);
    let mut per_s: Vec<u64> = Vec::new();
    while got < want && stop.is_none_or(|s| started.elapsed() < s) {
        if let Some(rate) = rate.filter(|r| *r > 0) {
            let due = Duration::from_secs_f64(got as f64 / rate as f64);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                tokio::time::sleep(wait).await;
            }
        }
        let asked = Instant::now();
        let Ok(Some(Ok(chunk))) = tokio::time::timeout(READY_TIMEOUT, response.next_chunk()).await
        else {
            break;
        };
        let waited = ms(asked);
        if first.is_some() && waited > 1000 {
            stalls += 1;
            stall_ms += waited;
        }
        let take = chunk
            .len()
            .min(usize::try_from(want - got).unwrap_or(usize::MAX));
        check.feed(&chunk[..take]);
        sha.update(&chunk[..take]);
        got += take as u64;
        if got >= 10 * MIB {
            mib10.get_or_insert_with(|| ms(started));
        }
        let first = *first.get_or_insert_with(|| ms(started));
        let second = (ms(started) - first) as usize / 1000;
        if per_s.len() <= second {
            per_s.resize(second + 1, 0);
        }
        per_s[second] += take as u64;
    }
    // The last second is partial.
    per_s.pop();
    json!({
        "offset": offset, "bytes": got, "first_byte_ms": first, "mib10_ms": mib10,
        "ms": (got >= want).then(|| ms(started)), "stalls": stalls, "stall_ms": stall_ms,
        "sha256": sha.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "fill_secs": per_s.len(),
        "fill_p10_bytes_per_s": percentile(&per_s, 0.1),
        "fill_p50_bytes_per_s": percentile(&per_s, 0.5),
    })
}

/// The torrent's verified and received bytes each second, into `out`,
/// until aborted.
async fn swarm_rate(engine: Engine, t: TorrentHandle, out: Arc<std::sync::Mutex<Vec<(u64, u64)>>>) {
    let mut last = None;
    loop {
        if let Ok(s) = engine.stats(t).await {
            let now = (s.downloaded_bytes, s.received_bytes);
            if let Some((v, r)) = last {
                let mut out = out.lock().unwrap_or_else(|e| e.into_inner());
                out.push((now.0.saturating_sub(v), now.1.saturating_sub(r)));
            }
            last = Some(now);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// `--walk`: reads at 25, 50 and 75% (4 MiB each), the whole file from
/// `from` at `rate` (for at most `stop`), then 64 MiB back at 10% and
/// 50 MiB at 50%. During
/// the whole read the swarm's rate is sampled each second: what the engine
/// verified and what it received.
async fn walk(
    (http, url): (&HttpClient, &str),
    (engine, t): (&Engine, TorrentHandle),
    len: u64,
    (from, rate, stop): (u64, Option<u64>, Option<Duration>),
    mut check: PieceCheck,
) -> Value {
    let at = |pct: f64| (len as f64 * pct / 100.0) as u64;
    let mut forward = Vec::new();
    for pct in SEEKS {
        let span = (at(pct), (4 * MIB).min(len - at(pct)));
        forward.push(read_checked(http, url, span, (None, None), &mut check).await);
    }
    let from = from.min(len);
    let rates = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sampler = tokio::spawn(swarm_rate(engine.clone(), t, Arc::clone(&rates)));
    let whole = read_checked(http, url, (from, len - from), (rate, stop), &mut check).await;
    sampler.abort();
    let rates = rates.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let verified: Vec<u64> = rates.iter().map(|r| r.0).collect();
    let mean = |v: &[u64]| (!v.is_empty()).then(|| v.iter().sum::<u64>() / v.len() as u64);
    let received: Vec<u64> = rates.iter().map(|r| r.1).collect();
    let back = (at(10.0), (64 * MIB).min(len - at(10.0)));
    let back = read_checked(http, url, back, (None, None), &mut check).await;
    let mid = (at(50.0), (50 * MIB).min(len - at(50.0)));
    let mid = read_checked(http, url, mid, (None, None), &mut check).await;
    json!({
        "forward": forward, "whole": whole, "back": [back], "mid": mid,
        "checked": check.checked, "wrong": check.wrong,
        "swarm": {
            "secs": rates.len(),
            "verified_p10_bytes_per_s": percentile(&verified, 0.1),
            "verified_p50_bytes_per_s": percentile(&verified, 0.5),
            "verified_mean_bytes_per_s": mean(&verified),
            "received_mean_bytes_per_s": mean(&received),
        },
    })
}

/// Time to the first body byte and to `want` bytes of `bytes=offset-`.
async fn timed_read(http: &HttpClient, url: &str, offset: u64, want: u64) -> Value {
    let opts = read_options();
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
pub async fn scrape(
    hashes: &[String],
    wait_ms: u64,
    warm: bool,
    sources: Option<&std::path::Path>,
) -> Result<Value, String> {
    let mut asked: Vec<(String, Vec<String>)> =
        hashes.iter().map(|h| (h.clone(), Vec::new())).collect();
    if let Some(file) = sources {
        let body = std::fs::read(file).map_err(|e| e.to_string())?;
        let answer = mq_protocol::decode_streams(&body).map_err(|e| e.to_string())?;
        for wire in &answer.streams {
            if let mq_protocol::Source::Torrent {
                info_hash,
                trackers,
                ..
            } = wire.source()
                && !asked.iter().any(|(h, _)| *h == info_hash)
            {
                asked.push((info_hash, trackers));
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("mq-scrape-{}", std::process::id()));
    let result = scrape_in(&dir, &asked, wait_ms, warm).await;
    let _ = std::fs::remove_dir_all(&dir);
    result
}

async fn scrape_in(
    dir: &std::path::Path,
    hashes: &[(String, Vec<String>)],
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
    let asked: Vec<(&str, &[String])> = hashes
        .iter()
        .map(|(h, own)| (h.as_str(), own.as_slice()))
        .collect();
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
        "counts": hashes.iter().map(|(h, _)| h).zip(&counts).map(|(h, c)| json!({
            "hash": h, "seeders": c.map(|c| c.seeders), "leechers": c.map(|c| c.leechers),
        })).collect::<Vec<_>>(),
        "trackers": trackers,
    }))
}

/// `mq download-magnet`: one file of a torrent downloaded whole with no
/// reader, upload off, as an offline download (plans/offline-downloads.md
/// 4.6). Reports the time to metadata, the bytes, the seconds and the mean
/// rate, for bench/downloads.
pub async fn download_magnet(
    input: &str,
    file: Option<usize>,
    data_dir: Option<PathBuf>,
    peer: Option<std::net::SocketAddr>,
    timeout: Duration,
) -> Result<Value, String> {
    let (hash, trackers) = parse_magnet(input)?;
    let temporary = data_dir.is_none();
    let dir = data_dir.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("mq-download-{}", std::process::id()))
    });
    let started = Instant::now();
    let result = async {
        let http = HttpClient::new(ClientConfig::default()).map_err(|e| e.to_string())?;
        let local: Vec<std::net::IpAddr> = peer.iter().map(|p| p.ip()).collect();
        let config = EngineConfig {
            upload: false,
            discovery: peer.is_none(),
            ..EngineConfig::new(&dir, http.torrent_scope(&local))
        };
        let engine = Engine::start(config).await.map_err(|e| e.to_string())?;
        let peers: Vec<std::net::SocketAddr> = peer.into_iter().collect();
        let t = tokio::time::timeout(
            METADATA_TIMEOUT,
            engine.add_magnet(&hash, &trackers, &peers),
        )
        .await
        .map_err(|_| "no metadata in time".to_owned())?
        .map_err(|e| e.to_string())?;
        let metadata_ms = ms(started);
        let hint = file.map_or(FileHint::Largest, FileHint::Index);
        let index = engine
            .select_file(t, "", &hint)
            .await
            .map_err(|e| e.to_string())?;
        let handle = engine.download(t, index).await.map_err(|e| e.to_string())?;
        let begun = Instant::now();
        let mut first_byte_ms = None;
        let progress = loop {
            let p = handle.progress().await;
            if p.have > 0 && first_byte_ms.is_none() {
                first_byte_ms = Some(ms(begun));
            }
            if p.have >= p.len || begun.elapsed() >= timeout || p.failed.is_some() {
                break p;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        };
        let secs = begun.elapsed().as_secs_f64();
        handle.stop().await;
        Ok::<_, String>(json!({
            "event": "download",
            "hash": hash,
            "file": index,
            "metadata_ms": metadata_ms,
            "first_piece_ms": first_byte_ms,
            "bytes": progress.have,
            "len": progress.len,
            "whole": progress.have >= progress.len,
            "seconds": (secs * 10.0).round() / 10.0,
            "mean_mb_s": ((progress.have as f64 / 1e6 / secs.max(0.001)) * 100.0).round() / 100.0,
            "failed": progress.failed,
        }))
    }
    .await;
    if temporary {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
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
    // A folder given is kept for the next run (a relaunch); a temporary
    // one goes after this run.
    let keep_data = o.data_dir.is_some();
    let http = HttpClient::new(ClientConfig::default()).map_err(|e| e.to_string())?;
    let local: Vec<std::net::IpAddr> = o.peer.iter().map(|p| p.ip()).collect();
    let mut config = EngineConfig {
        dwh: !o.stock,
        upload: o.upload,
        discovery: o.peer.is_none(),
        ..EngineConfig::new(dir, http.torrent_scope(&local))
    };
    if let Some(bytes) = o.cache_budget {
        config.cache_budget_bytes = bytes;
    }
    config.limits.download_bps = o.download_limit.and_then(std::num::NonZeroU32::new);
    if o.no_punch || o.free.is_some() {
        #[cfg(not(feature = "test-hooks"))]
        return Err("--no-punch and --free need a build with test-hooks".into());
        #[cfg(feature = "test-hooks")]
        if o.no_punch {
            use mq_engine::librqbit::sync_log::{Fail, fail};
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            fail(dir, Fail::PunchRefused, usize::MAX);
        }
    }
    let engine = Engine::start(config).await.map_err(|e| e.to_string())?;
    #[cfg(feature = "test-hooks")]
    mq_engine::test_hooks::set_free(&engine, o.free);
    // The data folder's allocated bytes, sampled every 250 ms.
    let max_allocated = Arc::new(AtomicU64::new(0));
    let sampler = tokio::spawn({
        let (dir, max) = (dir.to_owned(), Arc::clone(&max_allocated));
        async move {
            loop {
                max.fetch_max(allocated(&dir), Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    });
    let peers: Vec<std::net::SocketAddr> = o.peer.into_iter().collect();
    let t = tokio::time::timeout(METADATA_TIMEOUT, engine.add_magnet(hash, trackers, &peers))
        .await
        .map_err(|_| "no metadata in time".to_owned())?
        .map_err(|e| e.to_string())?;
    let metadata_ms = ms(started);
    // What the scheduler holds, sampled beside the folder: a folder past
    // it is the file system's (APFS's refills), not the scheduler's.
    // The lowest free space beside it: how close the run came to the reserve.
    let (max_held, min_free) = (
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(u64::MAX)),
    );
    let held_sampler = tokio::spawn({
        let (engine, max, min) = (engine.clone(), Arc::clone(&max_held), Arc::clone(&min_free));
        async move {
            loop {
                if let Ok(s) = engine.stats(t).await {
                    max.fetch_max(s.held_bytes.unwrap_or(0), Ordering::Relaxed);
                }
                if let Some(free) = engine.free() {
                    min.fetch_min(free, Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    });
    let hint = o.file.map_or(FileHint::Largest, FileHint::Index);
    let file = engine
        .select_file(t, CLI, &hint)
        .await
        .map_err(|e| e.to_string())?;
    let selected = Instant::now();
    if let Some(rate) = o.rate {
        engine
            .set_rate(t, file, rate, false)
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut window_row = None;
    if let (Some(limit), Some(rate)) = (o.window, o.rate) {
        let inputs = engine
            .window_inputs(t, file, engine.free().unwrap_or(0), rate, FILL_ESTIMATE)
            .await
            .map_err(|e| e.to_string())?;
        let mut plan = mq_engine::window_within(engine.device_class(), inputs, limit);
        if o.no_open_limit {
            (plan.open_floor, plan.open_cap) = (1 << 40, 1 << 40);
        }
        let w = mq_engine::Window {
            plan,
            bitrate: rate,
            metered: false,
            resume_at: o.resume_at,
        };
        engine.window(t, file, w).await.map_err(|e| e.to_string())?;
        window_row = Some(json!({
            "budget": plan.budget, "behind": plan.behind, "slack": plan.slack,
            "ahead": plan.ahead, "floor": plan.floor, "free": inputs.free,
            "room": inputs.room(),
        }));
    }
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
    // A player's first request (time to first byte, 4 and 10 MiB through the
    // loopback) runs beside the readiness wait. A player starts at Ready, as
    // in the app; the byte probe needs no container tail and starts its seeks
    // after the head read (Ready is then recorded if it came by the end).
    let mut head = reader.range(0, None).await.map_err(|e| e.to_string())?;
    let first = tokio::spawn({
        let (http, url) = (http.clone(), url.clone());
        async move { timed_read(&http, &url, 0, 10 * MIB).await }
    });
    // The task keeps the head range open (its demand stays) until it ends.
    let mut ready_task = Some(tokio::spawn(async move {
        let ready = tokio::time::timeout(READY_TIMEOUT, head.ready()).await;
        (ready.ok().and_then(Result::ok).map(|()| ms(selected)), head)
    }));
    let (mut ready_ms, mut peers_at_ready) = (None, None);
    if !o.bytes && !o.walk {
        let task = ready_task.take().expect("set above");
        let (ready, head) = task.await.map_err(|e| e.to_string())?;
        // The head range ends at Ready, as the app's does: kept open, it took
        // the focus whenever the player had no read open and fetched the
        // window's evicted head again.
        drop(head);
        let peers = engine.stats(t).await.map_err(|e| e.to_string())?.peers;
        crate::emit(json!({ "event": "ready", "ms": ready, "peers": peers }));
        ready_ms = Some(ready.ok_or("the head and tail were not verified in time")?);
        peers_at_ready = Some(peers);
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
    } else if o.walk {
        let check = engine.piece_check(t, file).map_err(|e| e.to_string())?;
        let span = (o.resume_at.unwrap_or(0), o.walk_rate, o.walk_for);
        walk((&http, &url), (&engine, t), info.len, span, check).await
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
    let first = first.await.unwrap_or_default();
    let stats = engine.stats(t).await.map_err(|e| e.to_string())?;
    let dialed = engine.connections();
    sampler.abort();
    held_sampler.abort();
    let allocated_bytes = allocated(dir);
    let max_allocated_bytes = max_allocated.load(Ordering::Relaxed).max(allocated_bytes);
    // Seeks: the walk's time to first byte, or mpv's to its next frame.
    let (forward, back) = if o.walk {
        (
            each(played.get("forward"), "first_byte_ms"),
            each(played.get("back"), "first_byte_ms"),
        )
    } else {
        (each(played.get("seeks"), "ms"), Vec::new())
    };
    let walked = played.get("whole");
    let stalls = walked.unwrap_or(&played);
    let walk_row = o.walk.then(|| {
        json!({
            "rate": o.walk_rate, "from": o.resume_at.unwrap_or(0),
            "bytes": walked.and_then(|w| w.get("bytes")), "ms": walked.and_then(|w| w.get("ms")),
            "for_s": o.walk_for.map(|d| d.as_secs()),
            "fill_secs": walked.and_then(|w| w.get("fill_secs")),
            "fill_p10_bytes_per_s": walked.and_then(|w| w.get("fill_p10_bytes_per_s")),
            "fill_p50_bytes_per_s": walked.and_then(|w| w.get("fill_p50_bytes_per_s")),
            "back": played.get("back"), "forward": played.get("forward"),
            "mid": played.get("mid"), "sha256": walked.and_then(|w| w.get("sha256")),
            "mib10_ms": walked.and_then(|w| w.get("mib10_ms")),
            "checked": played.get("checked"), "wrong": played.get("wrong"),
            "swarm": played.get("swarm"),
        })
    });
    // The lease's claim ends first: the engine keeps a claimed torrent.
    drop(lease);
    let _ = engine.remove(t, keep_data).await;
    Ok(json!({
        "event": "run",
        "file": info.path,
        "len": info.len,
        "mode": if stats.dwh { "dwh" } else { "stock" },
        "metadata_ms": metadata_ms,
        "ttfb_ms": first.get("first_byte_ms"),
        "head_4mib_ms": first.get("per_mib_ms").and_then(|m| m.get(3)),
        "head_10mib_ms": first.get("mib4_ms"),
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
        "refetched_bytes": stats.refetched_bytes,
        "rate_bytes_per_s": stats.bytes_per_s,
        "readahead_bytes": stats.readahead_bytes,
        "window": window_row,
        "held_bytes": stats.held_bytes,
        "max_held_bytes": stats.held_bytes.map(|h| h.max(max_held.load(Ordering::Relaxed))),
        "min_free_bytes": Some(min_free.load(Ordering::Relaxed)).filter(|&f| f < u64::MAX),
        "evict_rounds": stats.evict_rounds,
        "sched_seeks": stats.seeks,
        "max_allocated_bytes": max_allocated_bytes,
        "allocated_bytes": allocated_bytes,
        "seek_forward_p50_ms": percentile(&forward, 0.5),
        "seek_forward_p90_ms": percentile(&forward, 0.9),
        "seek_back_p50_ms": percentile(&back, 0.5),
        "seek_back_p90_ms": percentile(&back, 0.9),
        "stalls": stalls.get("stalls"),
        "stall_ms": stalls.get("stall_ms"),
        "walk": walk_row,
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

    #[test]
    fn nearest_rank_percentiles() {
        assert_eq!(percentile(&[], 0.5), None);
        assert_eq!(percentile(&[7], 0.9), Some(7));
        let v = [50, 10, 40, 20, 30, 100, 90, 80, 70, 60];
        assert_eq!(percentile(&v, 0.5), Some(50));
        assert_eq!(percentile(&v, 0.9), Some(90));
        assert_eq!(percentile(&v[..3], 0.5), Some(40));
    }
}
