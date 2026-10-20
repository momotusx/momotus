//! Simulated comparison of DWH against two baselines.
//!
//! ```text
//! cargo run --release -p mq-engine-sched --example sim -- [--seeds 200] [--threads 4] [--out PATH]
//! ```
//!
//! Writes one JSON line per run and prints a markdown summary table.

mod baselines;
mod swarm;

use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use swarm::{
    HEALTHY, Kind, MIB, PLAY_BETWEEN_SEEKS, Profile, RunResult, SEEK_CAP_S, SPARSE, TTP_CAP_S,
    run_for,
};

const PIECES: [u32; 3] = [256 * 1024, 4 << 20, 16 << 20];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let seeds: u64 = arg("--seeds").map_or(200, |s| s.parse().expect("--seeds N"));
    let threads: usize = arg("--threads").map_or(4, |s| s.parse().expect("--threads N"));
    let out = arg("--out").unwrap_or_else(|| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../bench/torrent/sim-results.jsonl"
        )
        .into()
    });
    // Seconds played before each seek, and one scheduler only (`--sched dwh`).
    let play_us = arg("--play-s").map_or(PLAY_BETWEEN_SEEKS, |s| {
        s.parse::<u64>().expect("--play-s N") * 1_000_000
    });
    let only = arg("--sched");
    let profiles: [&Profile; 2] = [&HEALTHY, &SPARSE];

    let mut jobs = Vec::new();
    for pi in 0..profiles.len() {
        for piece in PIECES {
            for kind in Kind::ALL {
                if only.as_deref().is_some_and(|o| o != kind.name()) {
                    continue;
                }
                for seed in 0..seeds {
                    jobs.push((pi, piece, kind, seed));
                }
            }
        }
    }
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<RunResult>>> = Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(pi, piece, kind, seed)) = jobs.get(i) else {
                        break;
                    };
                    let r = run_for(profiles[pi], piece, seed, kind, play_us);
                    slots.lock().expect("lock")[i] = Some(r);
                }
            });
        }
    });
    let results: Vec<RunResult> = slots
        .into_inner()
        .expect("lock")
        .into_iter()
        .map(|r| r.expect("every job ran"))
        .collect();

    let mut jsonl = String::new();
    for (&(pi, piece, kind, seed), r) in jobs.iter().zip(&results) {
        let seeks: Vec<String> = r.seeks.iter().map(|s| num(*s)).collect();
        writeln!(
            jsonl,
            "{{\"profile\":\"{}\",\"piece_kib\":{},\"sched\":\"{}\",\"seed\":{},\"ttp_s\":{},\"seeks_s\":[{}],\"rebuffer_s\":{:.3},\"played_s\":{:.3},\"ahead_p10_s\":{},\"payload_mib\":{:.2},\"dup_mib\":{:.3},\"requests\":{},\"cancels\":{}}}",
            profiles[pi].name,
            piece / 1024,
            kind.name(),
            seed,
            num(r.ttp),
            seeks.join(","),
            r.rebuffer_s,
            r.played_s,
            num(Some(pct(&sorted(&r.ahead_s), 0.1)).filter(|v| !v.is_nan())),
            r.payload as f64 / MIB as f64,
            r.dup as f64 / MIB as f64,
            r.requests,
            r.cancels,
        )
        .expect("write");
    }
    if let Some(dir) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(dir).expect("create output dir");
    }
    std::fs::write(&out, jsonl).expect("write results");
    eprintln!("wrote {} runs to {out}", results.len());

    println!("{}", summary(&profiles, &jobs, &results, seeds));
}

fn sorted(v: &[f64]) -> Vec<f64> {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v
}

fn num(v: Option<f64>) -> String {
    v.map_or("null".into(), |v| format!("{v:.3}"))
}

/// Nearest-rank percentile; capped runs count at the cap.
fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn cell(v: f64, cap: f64) -> String {
    if v.is_nan() {
        "n/a".into()
    } else if v >= cap {
        format!("cap {cap:.0}")
    } else {
        format!("{v:.2}")
    }
}

type Job = (usize, u32, Kind, u64);

fn summary(profiles: &[&Profile], jobs: &[Job], results: &[RunResult], seeds: u64) -> String {
    let mut s = String::new();
    writeln!(
        s,
        "Seconds, {seeds} seeded runs per row, 5 seeks per run. \"cap\" = hit the {TTP_CAP_S:.0} s start or {SEEK_CAP_S:.0} s seek cap (counted at the cap).\n"
    )
    .expect("write");
    writeln!(
        s,
        "| swarm | piece | scheduler | playable p50 | p90 | p99 | seek p50 | p90 | p99 | capped start / seeks | rebuffer | ahead p10 | ahead p50 | dup bytes |"
    )
    .expect("write");
    writeln!(
        s,
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
    )
    .expect("write");
    for (pi, profile) in profiles.iter().enumerate() {
        for piece in PIECES {
            for kind in Kind::ALL {
                let rs: Vec<&RunResult> = jobs
                    .iter()
                    .zip(results)
                    .filter(|(j, _)| j.0 == pi && j.1 == piece && j.2 == kind)
                    .map(|(_, r)| r)
                    .collect();
                if rs.is_empty() {
                    continue;
                }
                let mut ttp: Vec<f64> = rs.iter().map(|r| r.ttp.unwrap_or(TTP_CAP_S)).collect();
                let mut seek: Vec<f64> = rs
                    .iter()
                    .flat_map(|r| r.seeks.iter().map(|s| s.unwrap_or(SEEK_CAP_S)))
                    .collect();
                ttp.sort_by(f64::total_cmp);
                seek.sort_by(f64::total_cmp);
                let capped_ttp = rs.iter().filter(|r| r.ttp.is_none()).count();
                let capped_seek = rs
                    .iter()
                    .flat_map(|r| &r.seeks)
                    .filter(|s| s.is_none())
                    .count();
                let played: f64 = rs.iter().map(|r| r.played_s + r.rebuffer_s).sum();
                let rebuffer: f64 = rs.iter().map(|r| r.rebuffer_s).sum();
                let ahead: Vec<f64> = rs.iter().flat_map(|r| r.ahead_s.iter().copied()).collect();
                let ahead = sorted(&ahead);
                let payload: u64 = rs.iter().map(|r| r.payload).sum();
                let dup: u64 = rs.iter().map(|r| r.dup).sum();
                let piece_label = if piece < 1 << 20 {
                    format!("{} KiB", piece / 1024)
                } else {
                    format!("{} MiB", piece >> 20)
                };
                writeln!(
                    s,
                    "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} / {} | {:.1}% | {} | {} | {:.2}% |",
                    profile.name,
                    piece_label,
                    kind.name(),
                    cell(pct(&ttp, 0.5), TTP_CAP_S),
                    cell(pct(&ttp, 0.9), TTP_CAP_S),
                    cell(pct(&ttp, 0.99), TTP_CAP_S),
                    cell(pct(&seek, 0.5), SEEK_CAP_S),
                    cell(pct(&seek, 0.9), SEEK_CAP_S),
                    cell(pct(&seek, 0.99), SEEK_CAP_S),
                    capped_ttp,
                    capped_seek,
                    100.0 * rebuffer / played.max(1e-9),
                    cell(pct(&ahead, 0.1), f64::INFINITY),
                    cell(pct(&ahead, 0.5), f64::INFINITY),
                    100.0 * dup as f64 / payload.max(1) as f64,
                )
                .expect("write");
            }
        }
    }
    s
}
