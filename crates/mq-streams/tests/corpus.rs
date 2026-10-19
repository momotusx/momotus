//! Golden picks over the recorded corpus (plans/m13-stream-choice.md 6.1):
//! every title of both Torrentio recordings, ranked for each device profile.
//! The table of picks is `tests/corpus/picks.txt`; a change to any pick fails
//! this test and lists the changed lines. When the change is meant, write the
//! table again with `UPDATE_PICKS=1 cargo test -p mq-streams --test corpus`
//! and read the picks in `git diff`.
//!
//! Each line also carries the plan's expectation (`tests/corpus/expected.json`).
//! Pending ones are reported, not asserted: a person confirms a pick before
//! it becomes an assertion, and a pick never changes to match the code.

use mq_protocol::decode_streams;
use mq_relparse::{Hdr, VideoCodec};
use mq_streams::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus");

#[derive(Deserialize)]
struct Corpus {
    titles: Vec<Title>,
}

#[derive(Deserialize)]
struct Title {
    id: String,
    name: String,
    streams: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    corpus: String,
    id: String,
    profile: String,
    plan: String,
    pick: Option<String>,
    blocked: Option<String>,
    pending: bool,
}

/// A device and its user (6.1). The engine, account, speed and free space
/// reach the ranking with the M13 model.
struct Profile {
    name: &'static str,
    caps: DeviceCaps,
    prefs: StreamPrefs,
}

fn android(height: u32, hdr: Vec<Hdr>, av1: bool) -> DeviceCaps {
    let mut decoders = vec![VideoCodec::H264, VideoCodec::H265];
    if av1 {
        decoders.push(VideoCodec::Av1);
    }
    DeviceCaps {
        display_height: height,
        display_hdr: hdr,
        decoders,
        ten_bit_hevc: true,
        containers: [
            "mkv", "mp4", "m4v", "webm", "ts", "m2ts", "mov", "m3u8", "mpd",
        ]
        .map(str::to_owned)
        .to_vec(),
        has_fallback_engine: false,
        ..DeviceCaps::basic_1080p()
    }
}

fn profiles() -> Vec<Profile> {
    let prefs = StreamPrefs {
        languages: vec!["en".into()],
        ..StreamPrefs::default()
    };
    let stick = android(1080, vec![], false);
    let tv = android(2160, vec![Hdr::Hdr10], true);
    let apple = DeviceCaps {
        display_height: 2160,
        display_hdr: vec![Hdr::Hdr10, Hdr::DolbyVision],
        decoders: vec![VideoCodec::H264, VideoCodec::H265],
        containers: ["mp4", "m4v", "mov", "m3u8"].map(str::to_owned).to_vec(),
        has_fallback_engine: false,
        ..DeviceCaps::basic_1080p()
    };
    let p = |name, caps: &DeviceCaps| Profile {
        name,
        caps: caps.clone(),
        prefs: prefs.clone(),
    };
    vec![
        p("A", &stick),
        p("B", &tv),
        // Best quality: no size limit, 20 seeders (with the M13 preferences).
        p("C", &tv),
        p("D", &android(1440, vec![Hdr::Hdr10], false)),
        p("E", &apple),
        p("F", &stick),
        p("G", &tv),
    ]
}

/// Profiles each recording is checked on (6.1).
fn profiles_of(corpus: &str) -> &'static [&'static str] {
    match corpus {
        "shipped" => &["A", "B", "C", "D", "E", "F", "G"],
        _ => &["A", "B", "C"],
    }
}

fn read<T: for<'de> Deserialize<'de>>(file: &str) -> T {
    let text = std::fs::read_to_string(Path::new(DIR).join(file)).unwrap();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{file}: {e}"))
}

/// The target as the app knows it: the title's name and, for an episode,
/// its season and number from the id.
fn target(t: &Title) -> Target {
    let mut parts = t.id.split(':').skip(1).map(|n| n.parse::<u32>().unwrap());
    Target {
        title: t.name.clone(),
        year: None,
        season: parts.next(),
        episode: parts.next(),
    }
}

fn parsed(t: &Title) -> Vec<ParsedStream> {
    let body = serde_json::json!({ "streams": t.streams });
    let resp = decode_streams(body.to_string().as_bytes()).unwrap();
    resp.streams
        .iter()
        .enumerate()
        .map(|(i, w)| normalize(w, "torrentio", "Torrentio", 0, i as u32))
        .collect()
}

/// "1080p 3.3 GB 271 seeds bt:922d55b4:1" for a row.
fn describe(s: &ParsedStream) -> String {
    let res = serde_json::to_value(s.release.resolution)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "?".into());
    let size = s.size_bytes.map_or("size ?".into(), |b| {
        let gb = b as f64 / (1u64 << 30) as f64;
        if gb < 0.1 {
            format!("{gb:.2} GB")
        } else {
            format!("{gb:.1} GB")
        }
    });
    let seeds = s.seeders.map_or("seeds ?".into(), |n| format!("{n} seeds"));
    let key = s
        .key
        .strip_prefix("bt:")
        .map(|k| {
            let (hash, idx) = k.split_once(':').unwrap_or((k, ""));
            format!("bt:{}:{idx}", &hash[..8.min(hash.len())])
        })
        .unwrap_or_else(|| s.key.clone());
    format!("{res} {size} {seeds} {key}")
}

fn hidden_counts(r: &Ranked) -> String {
    let mut by: BTreeMap<String, u32> = BTreeMap::new();
    for h in &r.hidden {
        *by.entry(
            serde_json::to_value(h.reason)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
        )
        .or_default() += 1;
    }
    if by.is_empty() {
        return "hidden 0".into();
    }
    let parts: Vec<String> = by.iter().map(|(k, n)| format!("{k} {n}")).collect();
    format!("hidden {}", parts.join(", "))
}

/// One line per corpus, title and profile, and the expectations that fail.
fn table() -> (String, Vec<String>, (u32, u32)) {
    let expected: serde_json::Value = read("expected.json");
    let expected: Vec<Expected> = serde_json::from_value(expected["expected"].clone()).unwrap();
    let profiles = profiles();
    let mut out = String::new();
    let mut failures = Vec::new();
    let (mut met, mut pending) = (0, 0);
    for corpus in ["shipped", "default"] {
        let data: Corpus = read(&format!("{corpus}.json"));
        for (n, t) in data.titles.iter().enumerate() {
            let streams = parsed(t);
            let target = target(t);
            for p in profiles
                .iter()
                .filter(|p| profiles_of(corpus).contains(&p.name))
            {
                let input = RankInput {
                    caps: &p.caps,
                    prefs: &p.prefs,
                    target: &target,
                    last_binge_group: None,
                    kids: None,
                };
                let ranked = rank(streams.clone(), &input);
                let pick = ranked.best().map(|r| &r.stream);
                let got = pick.map_or("no pick".into(), describe);
                let want = expected
                    .iter()
                    .find(|e| e.corpus == corpus && e.id == t.id && e.profile == p.name);
                let verdict = match want {
                    None => String::new(),
                    Some(e) => {
                        let meets = match (&e.blocked, &e.pick) {
                            (Some(_), _) => pick.is_none(),
                            (None, Some(key)) => pick.is_some_and(|s| &s.key == key),
                            (None, None) => false,
                        };
                        if e.pending {
                            pending += 1;
                        }
                        if meets {
                            met += 1;
                        } else if !e.pending {
                            failures.push(format!(
                                "{corpus} #{} {} on {}: plan {}, got {got}",
                                n + 1,
                                t.name,
                                p.name,
                                e.plan
                            ));
                        }
                        let state = if e.pending { "pending" } else { "asserted" };
                        if meets {
                            format!(" | plan {} ({state}, met)", e.plan)
                        } else {
                            format!(" | plan {} ({state}, not met)", e.plan)
                        }
                    }
                };
                writeln!(
                    out,
                    "{corpus:<7} {:>2} {:<24} {} | {got} | {}{verdict}",
                    n + 1,
                    t.name,
                    p.name,
                    hidden_counts(&ranked)
                )
                .unwrap();
            }
        }
    }
    (out, failures, (met, pending))
}

#[test]
fn corpus_picks_match_the_golden_table() {
    let (now, failures, (met, pending)) = table();
    let file = Path::new(DIR).join("picks.txt");
    if std::env::var_os("UPDATE_PICKS").is_some() {
        std::fs::write(&file, &now).unwrap();
    }
    let before = std::fs::read_to_string(&file).unwrap_or_default();
    println!("plan expectations met: {met} ({pending} pending)");
    let old: BTreeMap<&str, &str> = before
        .lines()
        .map(|l| (l.split(" | ").next().unwrap_or(l), l))
        .collect();
    let changed: Vec<String> = now
        .lines()
        .filter(|l| old.get(l.split(" | ").next().unwrap_or(l)) != Some(l))
        .map(|l| {
            let head = l.split(" | ").next().unwrap_or(l);
            format!("  was: {}\n  now: {l}", old.get(head).unwrap_or(&"(none)"))
        })
        .collect();
    assert!(
        changed.is_empty() && before.lines().count() == now.lines().count(),
        "{} picks changed (UPDATE_PICKS=1 writes the table):\n{}",
        changed.len(),
        changed.join("\n")
    );
    assert!(
        failures.is_empty(),
        "asserted picks not met:\n{}",
        failures.join("\n")
    );
}
