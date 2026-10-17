//! Retained heap and warm-up time (ignored by default). Run with:
//! `cargo test -p mq-relparse --release --test memory -- --ignored --nocapture`
//!
//! A counting global allocator tracks live bytes, so the numbers are heap actually held, not
//! resident-set size. The corpus itself is loaded before the baseline is taken.

mod common;

use mq_relparse::{parse, parse_addon_stream};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn live_mb() -> f64 {
    LIVE.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0)
}

#[test]
#[ignore = "memory report: run with --release --ignored --nocapture"]
fn retained_heap() {
    let corpus = common::load();
    let strings = common::release_strings(&corpus);
    let base = live_mb();

    let t = Instant::now();
    mq_relparse::warm_up();
    let warm = t.elapsed();
    let after_warm = live_mb() - base;

    for s in &strings {
        std::hint::black_box(parse(s));
    }
    for c in &corpus {
        for r in &c.records {
            std::hint::black_box(parse_addon_stream(&r.name, &r.text, r.file.as_deref()));
        }
    }
    let after_corpus = live_mb() - base;
    println!("warm_up: {warm:?}");
    println!("retained after warm_up: {after_warm:.2} MB");
    println!(
        "retained after the full corpus ({} strings): {after_corpus:.2} MB",
        strings.len()
    );
}
