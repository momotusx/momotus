//! Phase D's measurements (bench/plugins/d-spike.jsonl), one case per
//! process so the peak RSS is that case's own:
//!
//!   measure overhead | string | string-rope | links | divs | attrs
//!   measure attrs-unguarded <KiB>
//!
//! `attrs` is one 4 MiB tag of attributes under the default caps;
//! `attrs-unguarded` is one such tag of the given size with no attribute cap,
//! so the scan lets it through: html5ever's own cost for it.
//!
//! Prints one JSON line. The Rust heap is counted by the allocator below;
//! QuickJS allocates with its own malloc, which only the RSS sees.

use mq_plugins::dom::{self, Caps};
use mq_plugins::engine::{self, HostReply, HostRequest, Limits, Outcome};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::mpsc;

struct Counting;
static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call goes to `System`; the counters only add and subtract.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `System`.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        // SAFETY: as for `System`.
        unsafe { System.dealloc(p, layout) };
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// The process's peak RSS in bytes.
fn max_rss() -> u64 {
    // SAFETY: a zeroed rusage the call fills.
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: a valid pointer to it.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) };
    // Bytes on Apple, KiB on Linux and Android.
    let unit = if cfg!(target_vendor = "apple") {
        1
    } else {
        1024
    };
    r.ru_maxrss as u64 * unit
}

/// One call with a host that answers every fetch with an empty body.
fn run(script: &str) -> Outcome {
    let (tx, mut rx) = mpsc::unbounded_channel::<HostRequest>();
    let host = std::thread::spawn(move || {
        while let Some(request) = rx.blocking_recv() {
            let _ = request.reply.send(HostReply::Body(String::new()));
        }
    });
    let outcome = engine::call(script.into(), Limits::default(), Default::default(), tx)
        .join()
        .expect("the call's thread");
    host.join().expect("the host thread");
    outcome
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let case = std::env::args().nth(1).unwrap_or_default();
    let before = max_rss();
    let line = match case.as_str() {
        "overhead" => {
            let mut ms: Vec<f64> = (0..100)
                .map(|_| {
                    let started = Instant::now();
                    let out = run("function getStreams() { return []; }");
                    assert_eq!(out, Outcome::Done("[]".into()));
                    started.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            ms.sort_by(f64::total_cmp);
            format!(
                r#""calls":100,"p50_ms":{:.3},"p90_ms":{:.3},"max_ms":{:.3}"#,
                percentile(&ms, 0.5),
                percentile(&ms, 0.9),
                ms[99]
            )
        }
        "string" | "string-rope" => {
            let script = if case == "string" {
                "function getStreams() { return 'x'.repeat(2 ** 28).length; }"
            } else {
                // Doubling makes ropes; reading a character flattens one.
                "function getStreams() { let s = 'x'; while (s.length < 2 ** 28) s += s; return s.charCodeAt(5); }"
            };
            let started = Instant::now();
            let out = run(script);
            format!(
                r#""outcome":"{out:?}","ms":{:.1}"#,
                started.elapsed().as_secs_f64() * 1e3
            )
        }
        "links" | "divs" => {
            let html = if case == "links" {
                "<a>".repeat(4 * 1024 * 1024 / 3)
            } else {
                "<div>".repeat(1_000_000)
            };
            let start = NOW.load(Ordering::Relaxed);
            PEAK.store(start, Ordering::Relaxed);
            let started = Instant::now();
            let doc = dom::parse(&html, Caps::default(), || false);
            let ms = started.elapsed().as_secs_f64() * 1e3;
            let heap = PEAK.load(Ordering::Relaxed) - start;
            let kept = NOW.load(Ordering::Relaxed) - start;
            format!(
                r#""input_bytes":{},"stopped":"{:?}","nodes":{},"fed":{},"ms":{ms:.1},"rust_heap_peak_bytes":{heap},"tree_bytes":{kept},"tree_bytes_per_node":{},"node_struct_bytes":{}"#,
                html.len(),
                doc.stopped,
                doc.nodes.len(),
                doc.fed,
                kept / doc.nodes.len(),
                std::mem::size_of::<dom::Node>()
            )
        }
        "attrs" | "attrs-unguarded" => {
            let (bytes, caps) = if case == "attrs" {
                (4 * 1024 * 1024, Caps::default())
            } else {
                let kib: usize = std::env::args()
                    .nth(2)
                    .and_then(|k| k.parse().ok())
                    .expect("attrs-unguarded <KiB>");
                let caps = Caps {
                    attrs: usize::MAX,
                    tag: usize::MAX,
                    ..Caps::default()
                };
                (kib * 1024, caps)
            };
            let mut html = String::from("<a");
            let mut i = 0;
            while html.len() < bytes - 16 {
                html.push_str(&format!(" a{i}"));
                i += 1;
            }
            html.push('>');
            let started = Instant::now();
            let doc = dom::parse(&html, caps, || false);
            format!(
                r#""input_bytes":{},"attributes":{i},"stopped":"{:?}","fed":{},"ms":{:.1}"#,
                html.len(),
                doc.stopped,
                doc.fed,
                started.elapsed().as_secs_f64() * 1e3
            )
        }
        _ => panic!(
            "measure overhead | string | string-rope | links | divs | attrs | attrs-unguarded <KiB>"
        ),
    };
    println!(
        r#"{{"case":"{case}",{line},"rss_before_bytes":{before},"max_rss_bytes":{}}}"#,
        max_rss()
    );
}
