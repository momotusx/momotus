//! Headless driver for the core: exercises the same code paths the apps use
//! and prints one JSON line per event with timings.

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

mod play;
mod sync;

#[derive(Parser)]
#[command(name = "mq", about = "Marquee core driver")]
struct Cli {
    /// Data directory for the library database.
    #[arg(long, env = "MQ_DATA", default_value = ".mq-data")]
    data: PathBuf,
    /// Profile id.
    #[arg(long, default_value = "default")]
    profile: String,
    #[command(subcommand)]
    command: Command,
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
        } => play(&core, kind, id, headless, seconds).await,
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
            },
        )
        .await
        .map(emit),
    };
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
        .hero
        .map(|h| json!({ "name": h.tile.name, "reason": h.reason }));
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
            AddonEvent::Slow { addon_ids } => {
                emit(json!({ "event": "slow", "addons": addon_ids, "ms": ms(started.elapsed()) }))
            }
            AddonEvent::Failed { addon_id, error } => emit(
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
        },
    );
    let snapshot = until(Duration::from_secs(15), || {
        let s = versions.snapshot();
        s.done.then_some(s)
    })
    .await
    .unwrap_or_else(|| versions.snapshot());
    let pick = snapshot.recommended.ok_or("no playable version")?;
    emit(json!({
        "event": "versions", "rows": snapshot.rows.len() + 1, "hidden": snapshot.hidden.count,
        "pick": { "quality": pick.quality, "via": pick.via, "release": pick.release_name,
                  "reasons": pick.reasons, "failover": pick.failover_count },
        "ms": ms(started.elapsed()),
    }));

    let playback = PlaybackView::open(
        core,
        versions.candidates(&pick.key),
        PlayTarget {
            kind,
            item_id,
            video_id: id,
            name: details.as_ref().map(|d| d.name.clone()),
            poster: details.and_then(|d| d.poster),
            season,
            episode,
        },
    );
    let ready = until(Duration::from_secs(60), || {
        let s = playback.snapshot();
        (s.phase != PlaybackPhase::Resolving).then_some(s)
    })
    .await
    .ok_or("playback did not resolve in 60 s")?;
    emit(json!({
        "event": "resolved", "phase": ready.phase, "attempts": ready.attempts,
        "resume_ms": ready.resume_ms, "ms": ms(started.elapsed()),
    }));
    let url = ready.url.ok_or("no source could be opened")?;
    let played = play::play_url(
        &url,
        play::PlayOptions {
            headless,
            seconds,
            start_ms: ready.resume_ms,
        },
    )
    .await?;
    playback.report_progress(ready.resume_ms.unwrap_or(0) + seconds * 1000, 0);
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
