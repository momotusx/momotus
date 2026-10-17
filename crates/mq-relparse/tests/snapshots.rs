//! Snapshot tests over the harvested addon corpus. Every parser change shows its diff here.
//! Regenerate with `INSTA_UPDATE=always cargo test -p mq-relparse --test snapshots`.

mod common;

use mq_relparse::{AddonStreamFacts, ParsedRelease, parse_addon_stream};
use std::fmt::Write;

/// Every n-th record keeps the snapshots reviewable while staying spread over all ids.
const STRIDE: usize = 9;

fn release_summary(r: &ParsedRelease) -> String {
    let mut s = format!("title={:?}", r.title);
    let mut put = |k: &str, v: String| {
        let _ = write!(s, " {k}={v}");
    };
    if let Some(y) = r.year {
        put("year", y.to_string());
    }
    put("res", format!("{:?}", r.resolution));
    put("q", format!("{:?}", r.quality));
    put("codec", format!("{:?}", r.codec));
    if let Some(b) = r.bit_depth {
        put("bits", b.to_string());
    }
    if !r.hdr.is_empty() {
        put("hdr", format!("{:?}", r.hdr));
    }
    if !r.audio.is_empty() {
        put("audio", format!("{:?}", r.audio));
    }
    if let Some(c) = &r.channels {
        put("ch", c.clone());
    }
    if !r.languages.is_empty() {
        put("lang", r.languages.join(","));
    }
    if !r.seasons.is_empty() {
        put("seasons", format!("{:?}", r.seasons));
    }
    if !r.episodes.is_empty() {
        put("eps", format!("{:?}", r.episodes));
    }
    if let Some(g) = &r.group {
        put("group", format!("{g:?}"));
    }
    if let Some(c) = &r.container {
        put("container", c.clone());
    }
    let flags = [
        ("complete", r.complete),
        ("remux", r.remux),
        ("proper", r.proper),
        ("repack", r.repack),
        ("extended", r.extended),
        ("hc", r.hardcoded_subs),
        ("dubbed", r.dubbed),
        ("3d", r.three_d),
        ("upscaled", r.upscaled),
    ];
    for (k, on) in flags {
        if on {
            put(k, "1".into());
        }
    }
    s
}

fn facts_summary(f: &AddonStreamFacts) -> String {
    let mut s = String::new();
    let _ = write!(
        s,
        "seeders={:?} size={:?} site={:?} cached={:?} service={:?} | {}",
        f.seeders,
        f.size_bytes,
        f.source_site,
        f.cached,
        f.service,
        release_summary(&f.release)
    );
    s
}

fn render(addon: &str) -> String {
    let corpus = common::load();
    let c = corpus
        .iter()
        .find(|c| c.addon == addon)
        .expect("corpus file");
    let mut out = String::new();
    for r in c.records.iter().step_by(STRIDE) {
        let facts = parse_addon_stream(&r.name, &r.text, r.file.as_deref());
        let _ = writeln!(out, "{} {:?}\n    {}", r.id, r.name, facts_summary(&facts));
    }
    out
}

#[test]
fn torrentio() {
    insta::assert_snapshot!(render("torrentio"));
}

#[test]
fn torrentsdb() {
    insta::assert_snapshot!(render("torrentsdb"));
}

#[test]
fn comet() {
    insta::assert_snapshot!(render("comet"));
}
