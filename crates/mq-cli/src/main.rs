//! Headless driver for the core: exercises the same code paths the apps use
//! and prints one JSON line per event with timings.

// The run row of `play-magnet` is one large `json!`.
#![recursion_limit = "256"]

use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use mq_addons::{AddonEvent, Deadlines};
use mq_protocol::{ResourcePath, decode_streams};
use mq_runtime::{
    Core, CoreConfig, HomeView, ImportSnapshot, ImportView, PlayTarget, PlaybackPhase,
    PlaybackView, RowState, SearchView, StoreId, StremioEndpoints, TitleView, VersionsParams,
    VersionsView,
};
use mq_streams::{DeviceCaps, Target};
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};

mod magnet;
mod play;
mod sync;
mod versions;

use versions::{Device, versions};

#[derive(Parser)]
#[command(name = "mq", about = "Momotus core driver")]
struct Cli {
    /// Data directory for the library database.
    #[arg(long, env = "MQ_DATA", default_value = ".mq-data")]
    data: PathBuf,
    /// The profile to open on first run only: once this data directory has
    /// switched profiles it opens as the last one (or the owner, `default`,
    /// when that one is gone).
    #[arg(long, default_value = "default")]
    profile: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, clap::ValueEnum, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum Arm {
    Recommended,
    First,
}

#[derive(Subcommand)]
enum Command {
    /// Install an addon by manifest URL (http, https or stremio://).
    Install { url: String },
    /// List installed addons.
    Addons,
    /// Load Home and print each row as it settles.
    Home {
        #[arg(long, default_value_t = 15)]
        timeout_secs: u64,
    },
    /// Query every stream addon for a title, e.g. `movie tt0111161` or `series tt0944947:1:1`.
    Streams { kind: String, id: String },
    /// End-to-end encrypted sync with a relay (development flow).
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    /// Import a Stremio account: prints a code to enter at the link page, then
    /// imports addons, library, progress and watched marks.
    Import {
        #[arg(long, default_value_t = 600)]
        timeout_secs: u64,
    },
    /// Search every addon catalog that supports it and print the rows.
    Search { query: String },
    /// Pick the recommended version of a title and play it in mpv, e.g.
    /// `movie tt1727587` or `series tt0944947:1:1`. Debrid keys come from
    /// MQ_RD_TOKEN, MQ_TORBOX_KEY, MQ_ALLDEBRID_KEY or MQ_PREMIUMIZE_KEY.
    Play {
        kind: String,
        id: String,
        #[arg(long)]
        headless: bool,
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        /// Which version: the recommendation, or the first the addons
        /// listed (the live race's two arms, plans/m13-stream-choice.md 6.4).
        #[arg(long, value_enum, default_value_t = Arm::Recommended)]
        pick: Arm,
        /// Start from an empty torrent folder and delete it after the run.
        #[arg(long)]
        fresh_cache: bool,
    },
    /// Open Versions for a title as an app does, with no playback: the pick
    /// when every addon answered (what the apps play) and after the live
    /// seeder counts, then every row with its parsed facts and the seeders
    /// its addon listed. Scrapes trackers only; fetches no piece.
    Versions {
        kind: String,
        id: String,
        /// The screen's lines.
        #[arg(long, default_value_t = 1080)]
        height: u32,
        /// The screen shows HDR10 and HLG.
        #[arg(long)]
        hdr: bool,
        /// The screen also shows Dolby Vision and HDR10+.
        #[arg(long, requires = "hdr")]
        dv: bool,
        /// The device decodes H.264 alone smoothly (no HEVC, AV1 or VP9).
        #[arg(long)]
        h264_only: bool,
        /// The measured HTTP and torrent speed, in Mbit/s (default: the
        /// priors).
        #[arg(long)]
        mbps: Option<f32>,
        /// The free space, in GB (default: the disk's).
        #[arg(long)]
        free_gb: Option<f64>,
        /// The device's language (ISO 639-1).
        #[arg(long, default_value = "en")]
        language: String,
    },
    /// Stream a torrent through the engine and the loopback server: time to
    /// metadata, first byte, Ready (head and tail verified), first frame in
    /// mpv, and seeks. Takes a magnet URI or an info hash.
    PlayMagnet {
        magnet: String,
        /// File index (default: the largest video).
        #[arg(long)]
        file: Option<usize>,
        #[arg(long)]
        headless: bool,
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        /// Seek to 25, 50 and 75% and time each.
        #[arg(long)]
        seek: bool,
        /// librqbit's stock picker instead of DWH (A/B runs).
        #[arg(long)]
        stock: bool,
        /// No player: time bytes at the head and the seek points instead.
        #[arg(long)]
        bytes: bool,
        /// Keep torrent data here (default: a temporary folder, deleted).
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Serve no pieces to other peers.
        #[arg(long)]
        no_upload: bool,
        /// A peer to dial (a seeder on this machine for a controlled run):
        /// the address policy allows it and the DHT stays off.
        #[arg(long)]
        peer: Option<std::net::SocketAddr>,
        /// No player: timed reads at 25, 50 and 75%, the whole file, then
        /// 64 MiB back at 10%, all through the loopback, each whole piece
        /// checked against the torrent's hashes.
        #[arg(long, conflicts_with = "bytes")]
        walk: bool,
        /// The walk's whole read, in bytes per second (default: as fast as
        /// the bytes come).
        #[arg(long, requires = "walk")]
        walk_rate: Option<u64>,
        /// The walk's whole read ends after this many seconds (a fill-rate
        /// run on a large file).
        #[arg(long, requires = "walk")]
        walk_for: Option<u64>,
        /// The walk's whole read starts at this byte offset (a resume).
        #[arg(long, requires = "walk")]
        resume_at: Option<u64>,
        /// The engine's disk budget in bytes (default: the engine's).
        #[arg(long)]
        cache_budget: Option<u64>,
        /// The session's download limit in bytes per second.
        #[arg(long)]
        download_limit: Option<u32>,
        /// The file's rate in bytes per second (its size over its duration),
        /// as the app gives it: the scheduler's windows become seconds of
        /// it and 120 s are kept ahead (default: none, the device preset).
        #[arg(long)]
        rate: Option<u64>,
        /// The walk-only window override in bytes (plans/streaming-window.md
        /// 4): the torrent holds at most this much of the file, whatever
        /// the free space.
        #[arg(long, requires = "rate")]
        window: Option<u64>,
        /// The window without its open-pieces limit (the fill rows' second
        /// arm).
        #[arg(long, requires = "window")]
        no_open_limit: bool,
        /// Bench only (built with `test-hooks`): the data folder's punch
        /// probe fails, so files are kept as piece files.
        #[arg(long)]
        no_punch: bool,
        /// Bench only (built with `test-hooks`): the free space the engine
        /// reads, in bytes.
        #[arg(long)]
        free: Option<u64>,
    },
    /// Download one file of a torrent whole, as an offline download does
    /// (plans/offline-downloads.md 4.6): no reader, upload off, and report
    /// the time to metadata, the bytes and the mean rate.
    DownloadMagnet {
        magnet: String,
        /// File index (default: the largest video).
        #[arg(long)]
        file: Option<usize>,
        /// Keep torrent data here (default: a temporary folder, deleted).
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// A peer to dial (a seeder on this machine): the DHT stays off.
        #[arg(long)]
        peer: Option<std::net::SocketAddr>,
        /// Give up after this many seconds.
        #[arg(long, default_value_t = 3600)]
        timeout: u64,
    },
    /// Scrape info hashes on a fresh engine (the app's first scrape) and
    /// report each tracker's stages.
    Scrape {
        hashes: Vec<String>,
        /// Wait this long after the engine starts.
        #[arg(long, default_value_t = 0)]
        wait_ms: u64,
        /// Warm the default trackers from the start (a title's page opened).
        #[arg(long)]
        warm: bool,
        /// An addon's stream answer (JSON): its swarms are asked, each with
        /// its stream's own trackers, as Versions asks them.
        #[arg(long)]
        sources: Option<PathBuf>,
    },
    /// Probe an http(s) stream, play it in mpv and report time to first frame.
    PlayUrl {
        url: String,
        #[arg(long)]
        headless: bool,
        #[arg(long, default_value_t = 5)]
        seconds: u64,
    },
}

#[derive(Subcommand)]
enum SyncAction {
    /// Create an account on the relay with this device as its first member.
    Init {
        server: String,
        #[arg(long)]
        invite: Option<String>,
        #[arg(long, default_value = "cli")]
        name: String,
    },
    /// Push local changes, pull and apply remote ones.
    Now,
    /// Join an account from another device: prints a pairing code to approve.
    Pair {
        server: String,
        #[arg(long, default_value = "cli")]
        name: String,
    },
    /// Approve a device showing this pairing code.
    Approve { qr: String },
    /// Make a recovery kit (shown once).
    Kit,
    /// Join the account of a recovery kit.
    Recover {
        server: String,
        kit: String,
        #[arg(long, default_value = "cli")]
        name: String,
    },
    /// List the account's devices and kits, with the roster fingerprint.
    Devices,
    /// Remove one device (by member id) and rotate the key.
    Remove { member: String },
    /// Report a device lost: drops it and what it vouched for, except
    /// `--keep` ids, and every kit any of them made.
    Lost {
        member: String,
        #[arg(long)]
        keep: Vec<String>,
    },
    /// Send what is queued, leave the account, forget the sync setup.
    Signout {
        #[arg(long)]
        force: bool,
    },
    /// After a fork: take the relay's roster (dropping again what this
    /// device's roster dropped, plus `--drop` ids).
    UseServerRoster {
        #[arg(long)]
        drop: Vec<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    // The engine alone: no core, so peak RSS is the engine's.
    if let Command::PlayMagnet {
        magnet,
        file,
        headless,
        seconds,
        seek,
        stock,
        bytes,
        data_dir,
        no_upload,
        peer,
        walk,
        walk_rate,
        walk_for,
        resume_at,
        cache_budget,
        download_limit,
        rate,
        window,
        no_open_limit,
        no_punch,
        free,
    } = cli.command
    {
        let options = magnet::MagnetOptions {
            file,
            headless,
            seconds,
            seek,
            stock,
            bytes,
            data_dir,
            upload: !no_upload,
            peer,
            walk,
            walk_rate,
            walk_for: walk_for.map(Duration::from_secs),
            resume_at,
            cache_budget,
            download_limit,
            rate,
            window,
            no_open_limit,
            no_punch,
            free,
        };
        match magnet::play_magnet(&magnet, options).await {
            Ok(run) => emit(run),
            Err(e) => fail(&e),
        }
        return;
    }
    if let Command::DownloadMagnet {
        magnet,
        file,
        data_dir,
        peer,
        timeout,
    } = &cli.command
    {
        let run = magnet::download_magnet(
            magnet,
            *file,
            data_dir.clone(),
            *peer,
            Duration::from_secs(*timeout),
        );
        match run.await {
            Ok(run) => emit(run),
            Err(e) => fail(&e),
        }
        return;
    }
    if let Command::Scrape {
        hashes,
        wait_ms,
        warm,
        sources,
    } = &cli.command
    {
        match magnet::scrape(hashes, *wait_ms, *warm, sources.as_deref()).await {
            Ok(run) => emit(run),
            Err(e) => fail(&e),
        }
        return;
    }
    // The engine keeps torrents in `<data>/torrents` (Core::open).
    let fresh = match &cli.command {
        Command::Play {
            fresh_cache: true, ..
        } => Some(cli.data.join("torrents")),
        _ => None,
    };
    if let Some(dir) = &fresh {
        let _ = std::fs::remove_dir_all(dir);
    }
    let core = match Core::open(CoreConfig {
        data_dir: cli.data.clone(),
        profile_id: cli.profile.clone(),
        device_id: 0,
    }) {
        Ok(core) => core,
        Err(e) => fail(&format!("cannot open core: {e}")),
    };
    let result = match cli.command {
        Command::Install { url } => install(&core, &url).await,
        Command::Addons => addons(&core),
        Command::Home { timeout_secs } => home(&core, Duration::from_secs(timeout_secs)).await,
        Command::Streams { kind, id } => streams(&core, kind, id).await,
        Command::Search { query } => search(&core, &query).await,
        Command::Sync { action } => match action {
            SyncAction::Init {
                server,
                invite,
                name,
            } => sync::init(&cli.data, &server, invite.as_deref(), &name).await,
            SyncAction::Now => sync::now(&core, &cli.data).await,
            SyncAction::Pair { server, name } => sync::pair(&cli.data, &server, &name).await,
            SyncAction::Approve { qr } => sync::approve(&core, &cli.data, &qr).await,
            SyncAction::Kit => sync::kit(&core, &cli.data).await,
            SyncAction::Recover { server, kit, name } => {
                sync::recover(&cli.data, &server, &kit, &name).await
            }
            SyncAction::Devices => sync::devices(&core, &cli.data).await,
            SyncAction::Remove { member } => sync::remove(&core, &cli.data, &member).await,
            SyncAction::Lost { member, keep } => sync::lost(&core, &cli.data, &member, &keep).await,
            SyncAction::Signout { force } => sync::sign_out(&core, &cli.data, force).await,
            SyncAction::UseServerRoster { drop } => {
                sync::use_server_roster(&core, &cli.data, &drop).await
            }
        },
        Command::Import { timeout_secs } => import(&core, Duration::from_secs(timeout_secs)).await,
        Command::Play {
            kind,
            id,
            headless,
            seconds,
            pick,
            ..
        } => play(&core, kind, id, headless, seconds, pick).await,
        Command::Versions {
            kind,
            id,
            height,
            hdr,
            dv,
            h264_only,
            mbps,
            free_gb,
            language,
        } => {
            let device = Device {
                height,
                hdr,
                dv,
                h264_only,
                mbps,
                free_gb,
                language,
            };
            versions(&core, kind, id, device).await
        }
        Command::PlayMagnet { .. } | Command::Scrape { .. } | Command::DownloadMagnet { .. } => {
            unreachable!("handled before the core opens")
        }
        Command::PlayUrl {
            url,
            headless,
            seconds,
        } => play::play_url(
            &url,
            play::PlayOptions {
                headless,
                seconds,
                start_ms: None,
                seeks: Vec::new(),
            },
        )
        .await
        .map(emit),
    };
    if let Some(dir) = &fresh {
        let _ = std::fs::remove_dir_all(dir);
    }
    if let Err(e) = result {
        fail(&e);
    }
}

fn fail(message: &str) -> ! {
    emit(json!({ "event": "error", "message": message }));
    std::process::exit(1);
}

pub(crate) fn emit(value: serde_json::Value) {
    println!("{value}");
}

fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

async fn install(core: &Core, url: &str) -> Result<(), String> {
    let started = Instant::now();
    let addon = core.install_addon(url).await.map_err(|e| e.to_string())?;
    emit(json!({
        "event": "installed",
        "id": addon.id(),
        "name": addon.manifest.name,
        "resources": addon.manifest.resources,
        "ms": ms(started.elapsed()),
    }));
    Ok(())
}

fn addons(core: &Core) -> Result<(), String> {
    for addon in core.installed_addons().map_err(|e| e.to_string())? {
        emit(
            json!({ "event": "addon", "id": addon.id(), "name": addon.manifest.name, "url": addon.transport_url.as_str() }),
        );
    }
    Ok(())
}

async fn home(core: &Core, timeout: Duration) -> Result<(), String> {
    let started = Instant::now();
    let view = HomeView::open(core);
    let (snapshot, mut updates) = view.subscribe();
    emit(json!({ "event": "home_open", "rows": snapshot.rows.len(), "ms": ms(started.elapsed()) }));
    let mut reported = std::collections::HashSet::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        for row in view.snapshot().rows {
            let settled = !matches!(row.state, RowState::Loading | RowState::Slow);
            if settled && reported.insert(row.row_id.clone()) {
                emit(json!({
                    "event": "row",
                    "title": row.title,
                    "source": row.source,
                    "state": row.state,
                    "tiles": row.tiles.len(),
                    "first": row.tiles.first().map(|t| t.name.clone()),
                    "ms": ms(started.elapsed()),
                }));
            }
        }
        let rows = view.snapshot().rows;
        if !rows.is_empty() && reported.len() >= rows.len() {
            break;
        }
        match tokio::time::timeout_at(deadline, updates.next()).await {
            Ok(Some(_)) => {}
            _ => break,
        }
    }
    let hero = view
        .snapshot()
        .header
        .heroes
        .iter()
        .map(|h| json!({ "name": h.tile.name, "reason": h.reason }))
        .collect::<Vec<_>>();
    emit(json!({ "event": "home_done", "hero": hero, "ms": ms(started.elapsed()) }));
    Ok(())
}

async fn streams(core: &Core, kind: String, id: String) -> Result<(), String> {
    let addons = core.installed_addons().map_err(|e| e.to_string())?;
    let path = ResourcePath {
        resource: "stream".into(),
        r#type: kind,
        id,
        extra: vec![],
    };
    let started = Instant::now();
    let mut fanout = core.addons().fan_out(&addons, &path, Deadlines::default());
    let mut total = 0usize;
    while let Some(event) = fanout.next().await {
        match event {
            AddonEvent::Arrived {
                addon_id,
                body,
                from_cache,
                latency,
                ..
            } => {
                let decoded = decode_streams(&body).map_err(|e| e.to_string());
                let (count, skipped, kinds) = match &decoded {
                    Ok(r) => (r.streams.len(), r.skipped, source_kinds(&r.streams)),
                    Err(_) => (0, 0, serde_json::Value::Null),
                };
                total += count;
                emit(json!({
                    "event": "streams", "addon": addon_id, "count": count, "skipped": skipped,
                    "sources": kinds, "cache": from_cache, "latency_ms": ms(latency), "ms": ms(started.elapsed()),
                    "error": decoded.err(),
                }));
            }
            AddonEvent::Slow { addon_ids, .. } => {
                emit(json!({ "event": "slow", "addons": addon_ids, "ms": ms(started.elapsed()) }))
            }
            AddonEvent::Failed {
                addon_id, error, ..
            } => emit(
                json!({ "event": "failed", "addon": addon_id, "error": error.to_string(), "ms": ms(started.elapsed()) }),
            ),
            AddonEvent::Done => break,
        }
    }
    emit(json!({ "event": "streams_done", "total": total, "ms": ms(started.elapsed()) }));
    Ok(())
}

fn source_kinds(streams: &[mq_protocol::WireStream]) -> serde_json::Value {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for s in streams {
        let kind = serde_json::to_value(s.source())
            .ok()
            .and_then(|v| v.get("source").and_then(|k| k.as_str()).map(str::to_owned))
            .unwrap_or_else(|| "unknown".into());
        *counts.entry(kind).or_default() += 1;
    }
    json!(counts)
}

/// Title, Versions and Playback views in sequence, as the apps drive them,
/// then mpv on the loopback URL. Every step reports its time since start.
async fn play(
    core: &Core,
    kind: String,
    id: String,
    headless: bool,
    seconds: u64,
    arm: Arm,
) -> Result<(), String> {
    let started = Instant::now();
    core.set_debrid_keys(&StoreId::keys_from_env());
    let mut parts = id.split(':');
    let item_id = parts.next().unwrap_or_default().to_owned();
    let season = parts.next().and_then(|s| s.parse().ok());
    let episode = parts.next().and_then(|s| s.parse().ok());

    let title = TitleView::open(core, &kind, &item_id);
    let header = until(Duration::from_secs(10), || {
        let header = title.snapshot().header;
        header.details.is_some().then_some(header)
    })
    .await;
    let details = header.and_then(|h| h.details);
    emit(
        json!({ "event": "title", "name": details.as_ref().map(|d| &d.name), "ms": ms(started.elapsed()) }),
    );

    let versions = VersionsView::open(
        core,
        VersionsParams {
            kind: kind.clone(),
            video_id: id.clone(),
            target: Target {
                title: details.as_ref().map(|d| d.name.clone()).unwrap_or_default(),
                year: None,
                season,
                episode,
            },
            caps: DeviceCaps::basic_1080p(),
            prefs: core.prefs().streams,
            last_binge_group: None,
            facts: Default::default(),
        },
    );
    until(Duration::from_secs(15), || {
        versions.snapshot().done.then_some(())
    })
    .await;
    let versions_ms = ms(started.elapsed());
    // The race reads Versions once the live seeder counts landed (about a
    // second after `done`, two after a cold first scrape), as a viewer
    // reading the list sees them.
    until(Duration::from_secs(15), || {
        (!versions.counting()).then_some(())
    })
    .await;
    let counted_ms = ms(started.elapsed());
    let snapshot = versions.snapshot();
    let key = match arm {
        Arm::Recommended => snapshot.recommended.as_ref().map(|r| r.key.clone()),
        Arm::First => versions.first_listed(),
    }
    .ok_or("no playable version")?;
    let pick = snapshot
        .recommended
        .iter()
        .chain(&snapshot.rows)
        .find(|r| r.key == key)
        .cloned()
        .ok_or("no playable version")?;
    emit(json!({
        "event": "versions", "arm": arm, "rows": snapshot.rows.len() + 1, "hidden": snapshot.hidden.count,
        "pick": { "quality": pick.quality, "via": pick.via, "release": pick.release_name,
                  "reasons": pick.reasons, "failover": pick.failover_count,
                  "line": pick.line, "sizeBytes": pick.size_bytes, "bitrateMbps": pick.bitrate_mbps },
        "versionsMs": versions_ms, "countedMs": counted_ms, "nobodySharing": snapshot.nobody_sharing,
        "ms": ms(started.elapsed()),
    }));

    let opened = Instant::now();
    let target = PlayTarget {
        kind,
        item_id,
        video_id: id,
        name: details.as_ref().map(|d| d.name.clone()),
        poster: details.and_then(|d| d.poster),
        season,
        episode,
    };
    // The recommendation plays as the apps play a pick (moving on, the
    // early cut); Torrentio's first plays as a plain client would, its
    // sources only.
    let playback = match arm {
        Arm::Recommended => {
            PlaybackView::open_picked(core, versions.pick(&key), target, &versions.plays())
        }
        Arm::First => PlaybackView::open(core, versions.candidates(&key), target),
    };
    // The race's start timeout (plans/m13-stream-choice.md 6.4): 120 s.
    // The version it moved on to shows while it starts; the core clears
    // it once that version plays.
    let moved = std::cell::RefCell::new(None::<String>);
    let ready = until(Duration::from_secs(120), || {
        let s = playback.snapshot();
        if let Some(t) = &s.trying {
            *moved.borrow_mut() = Some(t.key.clone());
        }
        (s.phase != PlaybackPhase::Resolving).then_some(s)
    })
    .await;
    let ready_ms = ms(opened.elapsed());
    // One summary line per run for the race (6.4): first frame from Play.
    let race = |first_frame: Option<u64>,
                played: &serde_json::Value,
                snap: &mq_runtime::PlaybackSnapshot| {
        let attempts = &snap.attempts;
        let seeders = match &pick.status {
            mq_runtime::views::versions::VersionStatus::Torrent { seeds } => *seeds,
            _ => None,
        };
        json!({
            "event": "race", "arm": arm, "key": key, "release": pick.release_name,
            "sizeBytes": pick.size_bytes, "seeders": seeders, "bitrateMbps": pick.bitrate_mbps,
            "line": pick.line, "readyMs": ready_ms, "firstFrameMs": first_frame.map(|f| ready_ms + f),
            "stalls": played["stalls"], "stallMs": played["stall_ms"], "attempts": attempts.len(),
            "failures": attempts.iter().map(|a| a.error.clone()).collect::<Vec<_>>(),
            "played": snap.key, "moved": moved.borrow().clone(),
            "versionsMs": versions_ms, "countedMs": counted_ms,
            "nobodySharing": snapshot.nobody_sharing, "ok": first_frame.is_some(),
        })
    };
    let Some(ready) = ready else {
        emit(race(None, &json!({}), &playback.snapshot()));
        return Err("playback did not resolve in 120 s".into());
    };
    emit(json!({
        "event": "resolved", "phase": ready.phase, "attempts": ready.attempts,
        "resume_ms": ready.resume_ms, "ms": ms(started.elapsed()),
    }));
    let Some(url) = ready.url.clone() else {
        emit(race(None, &json!({}), &ready));
        return Err("no source could be opened".into());
    };
    let played = play::play_url(
        &url,
        play::PlayOptions {
            headless,
            seconds,
            start_ms: ready.resume_ms,
            seeks: Vec::new(),
        },
    )
    .await?;
    emit(race(played["first_frame_ms"].as_u64(), &played, &ready));
    playback.report_progress(ready.resume_ms.unwrap_or(0) + seconds * 1000, 0, false);
    let subs = until(Duration::from_secs(10), || {
        let s = playback.snapshot();
        (!s.subtitles_pending).then_some(s.subtitles)
    })
    .await
    .unwrap_or_default();
    let mut langs: Vec<&str> = Vec::new();
    for t in &subs {
        if !langs.contains(&t.lang.as_str()) {
            langs.push(&t.lang);
        }
    }
    emit(
        json!({ "event": "subtitles", "count": subs.len(), "languages": langs.len(), "first": langs.iter().take(8).collect::<Vec<_>>() }),
    );
    // What a player gets for the first tracks: UTF-8 WebVTT from the loopback.
    let loopback = mq_http::RequestOptions {
        allow_private: true,
        ..Default::default()
    };
    let mut files = Vec::new();
    for t in subs.iter().take(5) {
        let started = Instant::now();
        files.push(match core.http().get(&t.url, &loopback).await {
            Ok(r) => {
                let text = String::from_utf8_lossy(&r.body);
                json!({
                    "lang": t.lang, "webvtt": text.starts_with("WEBVTT"),
                    "cues": text.matches(" --> ").count(), "bytes": r.body.len(),
                    "type": r.header("content-type"), "ms": ms(started.elapsed()),
                })
            }
            Err(e) => json!({ "lang": t.lang, "error": e.to_string() }),
        });
    }
    emit(json!({ "event": "subtitle_files", "files": files }));
    emit(json!({ "event": "done", "played": played, "total_ms": ms(started.elapsed()) }));
    Ok(())
}

/// Polls `f` every 10 ms until it returns a value or `within` passes.
async fn until<T>(within: Duration, f: impl Fn() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(value) = f() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn search(core: &Core, query: &str) -> Result<(), String> {
    let started = Instant::now();
    let view = SearchView::open(core);
    view.set_query(query);
    let settled = until(Duration::from_secs(15), || {
        let s = view.snapshot();
        let busy = s
            .rows
            .iter()
            .any(|r| matches!(r.state, RowState::Loading | RowState::Slow));
        (s.query == query && !s.rows.is_empty() && !busy).then_some(s)
    })
    .await
    .ok_or("no results in 15 s")?;
    for row in settled.rows {
        emit(json!({
            "event": "row", "title": row.title, "source": row.source, "state": row.state,
            "results": row.tiles.iter().take(5).map(|t| &t.name).collect::<Vec<_>>(),
            "count": row.tiles.len(),
        }));
    }
    emit(json!({ "event": "search_done", "ms": ms(started.elapsed()), "debounce_ms": 250 }));
    Ok(())
}

async fn import(core: &Core, timeout: Duration) -> Result<(), String> {
    let started = Instant::now();
    let view = ImportView::open(core, StremioEndpoints::default());
    let (_, mut updates) = view.subscribe();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let state = view.snapshot();
        emit(json!({ "event": "import", "state": state, "ms": ms(started.elapsed()) }));
        match state {
            ImportSnapshot::Done { .. } => return Ok(()),
            ImportSnapshot::Failed { reason } => return Err(reason),
            ImportSnapshot::Expired => return Err("the code expired".into()),
            _ => {}
        }
        match tokio::time::timeout_at(deadline, updates.next()).await {
            Ok(Some(_)) => {}
            _ => return Err("import timed out".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn the_profile_flag_says_it_only_picks_the_first_run() {
        let help = super::Cli::command().render_help().to_string();
        assert!(help.contains("first run only"), "{help}");
    }
}
