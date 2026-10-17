//! Headless driver for the core: exercises the same code paths the apps use
//! and prints one JSON line per event with timings.

use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use mq_addons::{AddonEvent, Deadlines};
use mq_protocol::{ResourcePath, decode_streams};
use mq_runtime::{Core, CoreConfig, HomeView, RowState};
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let core = match Core::open(CoreConfig {
        data_dir: cli.data.clone(),
        profile_id: cli.profile.clone(),
        device_id: 1,
    }) {
        Ok(core) => core,
        Err(e) => fail(&format!("cannot open core: {e}")),
    };
    let result = match cli.command {
        Command::Install { url } => install(&core, &url).await,
        Command::Addons => addons(&core),
        Command::Home { timeout_secs } => home(&core, Duration::from_secs(timeout_secs)).await,
        Command::Streams { kind, id } => streams(&core, kind, id).await,
    };
    if let Err(e) = result {
        fail(&e);
    }
}

fn fail(message: &str) -> ! {
    emit(json!({ "event": "error", "message": message }));
    std::process::exit(1);
}

fn emit(value: serde_json::Value) {
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
