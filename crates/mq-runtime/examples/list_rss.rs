//! Peak memory while one Find addons list is read (plans/plugins-and-marketplace.md
//! 7.1, review B2), in a fresh process per way:
//!   cargo run -p mq-runtime --example list_rss -- <list.json> tree   # the whole body as one tree
//!   cargo run -p mq-runtime --example list_rss -- <list.json> raw    # `read_list`, one entry at a time
//! Prints one JSON line with the body's size and the process's peak resident size.

use mq_runtime::views::addon_catalog::{entries, read_list};

fn peak_bytes() -> u64 {
    // SAFETY: getrusage fills the struct it is given.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    // macOS reports bytes, Linux and Android kilobytes.
    if cfg!(target_os = "macos") {
        usage.ru_maxrss as u64
    } else {
        usage.ru_maxrss as u64 * 1024
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (path, way) = (&args[1], args[2].as_str());
    let body = std::fs::read(path).expect("the list file");
    let before = peak_bytes();
    let shown = match way {
        "tree" => {
            let tree: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
            entries(tree["addons"].as_array().expect("addons")).len()
        }
        _ => read_list(&body, 2000).expect("a list").0.len(),
    };
    let peak = peak_bytes();
    println!(
        "{{\"way\": \"{way}\", \"body_bytes\": {}, \"entries\": {shown}, \"peak_rss_bytes\": {peak}, \"grew_bytes\": {}}}",
        body.len(),
        peak.saturating_sub(before)
    );
}
