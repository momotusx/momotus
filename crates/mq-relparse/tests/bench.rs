//! Micro-benchmark (ignored by default). Run with:
//! `cargo test -p mq-relparse --release --test bench -- --ignored --nocapture`
//!
//! Reports the mean over whole passes of the harvested corpus. The best pass is the number to
//! trust on a busy machine; the median shows what a loaded machine gives.

mod common;

use mq_relparse::parse;
use std::time::Instant;

const PASSES: usize = 15;

#[test]
#[ignore = "benchmark: run with --release --ignored --nocapture"]
fn parse_speed() {
    let strings = common::release_strings(&common::load());
    let t0 = Instant::now();
    std::hint::black_box(parse("The.Matrix.1999.1080p.BluRay.x264-GRP"));
    println!("first call (builds the handler table): {:?}", t0.elapsed());
    for s in &strings {
        std::hint::black_box(parse(s));
    }
    let mut pass_means = Vec::new();
    let mut best: Vec<u128> = Vec::new();
    for _ in 0..PASSES {
        let mut per_call = Vec::with_capacity(strings.len());
        let start = Instant::now();
        for s in &strings {
            let t = Instant::now();
            std::hint::black_box(parse(s));
            per_call.push(t.elapsed().as_nanos());
        }
        pass_means.push(start.elapsed().as_secs_f64() * 1e6 / strings.len() as f64);
        if best.is_empty() || per_call.iter().sum::<u128>() < best.iter().sum::<u128>() {
            best = per_call;
        }
    }
    pass_means.sort_by(f64::total_cmp);
    best.sort_unstable();
    let pick = |q: f64| best[((best.len() - 1) as f64 * q) as usize] as f64 / 1000.0;
    println!(
        "titles: {}  mean/title: best pass {:.1} us, median pass {:.1} us",
        strings.len(),
        pass_means[0],
        pass_means[PASSES / 2]
    );
    println!(
        "best pass percentiles: p50 {:.1} us  p90 {:.1} us  p99 {:.1} us",
        pick(0.5),
        pick(0.9),
        pick(0.99)
    );
}
