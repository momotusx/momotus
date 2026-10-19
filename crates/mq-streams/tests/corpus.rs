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
    line: Option<String>,
    blocked: Option<String>,
    pending: bool,
}

/// A device and its user (6.1): what the device plays, what the user
/// prefers, and the conditions the core reads (engine, account, speed, free
/// space). The title's runtime and years join them per title.
struct Profile {
    name: &'static str,
    caps: DeviceCaps,
    prefs: StreamPrefs,
    conditions: Conditions,
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

const GB: u64 = 1 << 30;

/// The engine, an account, a measured torrent speed and the free space.
fn conditions(engine: bool, debrid: bool, p2p: Option<f32>, free_gb: u64) -> Conditions {
    let defaults = Conditions::default();
    Conditions {
        engine,
        debrid,
        p2p: p2p.map_or(defaults.p2p, |mbps| Speed { mbps, samples: 3 }),
        free_bytes: Some(free_gb * GB),
        ..defaults
    }
}

fn profiles() -> Vec<Profile> {
    let prefs = StreamPrefs {
        languages: vec!["en".into()],
        ..StreamPrefs::default()
    };
    let best = StreamPrefs {
        size_limit_bytes: None,
        min_seeders: 20,
        ..prefs.clone()
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
    let p = |name, caps: &DeviceCaps, prefs: &StreamPrefs, conditions| Profile {
        name,
        caps: caps.clone(),
        prefs: prefs.clone(),
        conditions,
    };
    vec![
        p("A", &stick, &prefs, conditions(true, false, Some(25.0), 20)),
        p("B", &tv, &prefs, conditions(true, false, None, 50)),
        p("C", &tv, &best, conditions(true, false, Some(100.0), 200)),
        p(
            "D",
            &android(1440, vec![Hdr::Hdr10], false),
            &prefs,
            conditions(true, false, Some(15.0), 8),
        ),
        p("E", &apple, &prefs, conditions(false, true, None, 50)),
        p("F", &stick, &prefs, Conditions::default()),
        p("G", &tv, &prefs, conditions(false, true, None, 50)),
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

/// The Cinemeta facts of a title (`tests/corpus/meta.json`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    id: String,
    year: Option<String>,
    runtime: Option<String>,
    episode_released: Option<String>,
    episodes_in_season: Option<u32>,
}

/// "2005–2013" is 2005; "2013-09-16T05:00:00.000Z" is 2013.
fn first_year(text: &str) -> Option<u16> {
    text.get(..4)?.parse().ok()
}

/// A profile's conditions with the title's runtime, years and season size,
/// as the app sends them with `versions` (4.3).
fn for_title(base: &Conditions, m: &Meta) -> Conditions {
    let minutes = m
        .runtime
        .as_deref()
        .and_then(|r| r.split_whitespace().next()?.parse::<u32>().ok());
    Conditions {
        runtime_s: minutes.map(|m| m * 60),
        title_year: m.year.as_deref().and_then(first_year),
        episode_year: m.episode_released.as_deref().and_then(first_year),
        episodes_in_season: m.episodes_in_season,
        ..base.clone()
    }
}

fn kind_of<T: serde::Serialize>(value: &T) -> String {
    let v = serde_json::to_value(value).unwrap();
    v.get("kind")
        .unwrap_or(&v)
        .as_str()
        .unwrap_or("?")
        .to_owned()
}

/// `CORPUS_EXPLAIN="shipped 2 A"` prints that title's top rows on that
/// profile with their key and score, to read why a pick won.
fn explain(corpus: &str, n: usize, profile: &str, ranked: &Ranked, plan: Option<&str>) {
    let Ok(want) = std::env::var("CORPUS_EXPLAIN") else {
        return;
    };
    let this = format!("{corpus} {n} {profile}");
    if !want.split(',').any(|w| w.trim() == this) {
        return;
    }
    println!("== {this}");
    if let Some(h) = ranked
        .hidden
        .iter()
        .find(|h| Some(h.stream.key.as_str()) == plan)
    {
        println!(
            "plan row hidden: {:?} | {}",
            h.reason,
            h.stream.raw_description.lines().next().unwrap_or_default()
        );
    }
    let shown = ranked
        .rows
        .iter()
        .enumerate()
        .filter(|(i, r)| *i < 3 || Some(r.stream.key.as_str()) == plan);
    for (i, r) in shown {
        let k = &r.key;
        let mark = if Some(r.stream.key.as_str()) == plan {
            "plan"
        } else {
            ""
        };
        println!(
            "#{i} {mark} {} | {:?} | lang {} legit {} year {} sustains {} {:?} viable {} size {} start {} binge {} height {} within {} | {} {:?} {:?} | {:.1} Mbit/s | {}",
            describe(&r.stream),
            r.class,
            k.language as u8,
            k.legit as u8,
            k.year_ok as u8,
            k.sustains as u8,
            k.playability,
            k.viable as u8,
            k.size_ok as u8,
            k.start,
            k.binge as u8,
            k.effective_height,
            k.within,
            kind_of(&r.line),
            r.stream.release.quality,
            r.stream.release.hdr,
            r.bitrate_mbps.unwrap_or(f32::NAN),
            r.stream.raw_description.lines().next().unwrap_or_default()
        );
    }
}

/// Profiles whose user never chose anything (3.8): the owner's rule holds
/// on them.
fn default_user(corpus: &str, profile: &str) -> bool {
    match corpus {
        "shipped" => ["A", "B", "D", "G"].contains(&profile),
        _ => ["A", "B"].contains(&profile),
    }
}

/// Code review r1 P2-1: the owner's rule, from each row's own facts, so a
/// wrong pick fails however the table was written. Among the visible rows:
/// a real release when there is one, a wanted language when there is one,
/// at most 5 GB (an episode of a pack) when a real one fits, at least 50
/// seeders when a fitting real one has them, and no row of the pick's
/// playability or better in a higher seeder band.
fn owner_rule(ranked: &Ranked, prefs: &StreamPrefs, c: &Conditions) -> Vec<String> {
    use mq_relparse::Quality::{Cam, Screener, Telesync};
    let Some(pick) = ranked.best() else {
        return Vec::new();
    };
    let real = |r: &RankedRow| !matches!(r.stream.release.quality, Cam | Telesync | Screener);
    let lang = |r: &RankedRow| {
        let l = &r.stream.release.languages;
        l.is_empty() || prefs.languages.iter().any(|w| l.contains(w))
    };
    let torrent = |r: &RankedRow| matches!(r.class, SourceClass::Swarm | SourceClass::Unknown);
    let fits = |r: &RankedRow| {
        let episode = match (r.bitrate_mbps, c.runtime_s) {
            (Some(mbps), Some(s)) => Some((mbps as f64 * s as f64 / 8.0 * 1e6) as u64),
            _ => r.stream.size_bytes,
        };
        let limit = prefs.size_limit_bytes.map(|l| l as f64 * 1.1);
        !torrent(r)
            || matches!((episode, limit), (Some(b), Some(l)) if b as f64 <= l)
            || limit.is_none()
            || episode.is_none()
    };
    let seeds = |r: &RankedRow| {
        if torrent(r) {
            r.stream.seeders.unwrap_or(0)
        } else {
            u32::MAX
        }
    };
    let band = |r: &RankedRow| {
        let (n, min) = (seeds(r), prefs.min_seeders);
        match n {
            u32::MAX => 4,
            n if n >= min * 4 => 3,
            n if n >= min => 2,
            n if n >= 5 => 1,
            _ => 0,
        }
    };
    let rows = &ranked.rows;
    let mut out = Vec::new();
    let mut need = |holds: bool, any: bool, what: &str| {
        if any && !holds {
            out.push(what.to_owned());
        }
    };
    need(
        real(pick),
        rows.iter().any(real),
        "a copy over a real release",
    );
    need(lang(pick), rows.iter().any(lang), "a language not wanted");
    let good: Vec<&RankedRow> = rows.iter().filter(|r| real(r) && lang(r)).collect();
    need(fits(pick), good.iter().any(|r| fits(r)), "over 5 GB");
    let fitting: Vec<&&RankedRow> = good.iter().filter(|r| fits(r)).collect();
    need(
        seeds(pick) >= prefs.min_seeders,
        fitting.iter().any(|r| seeds(r) >= prefs.min_seeders),
        "under 50 seeders",
    );
    let better = fitting.iter().find(|r| {
        r.key.playability >= pick.key.playability
            && r.key.year_ok
            && r.key.sustains
            && band(r) > band(pick)
    });
    if let Some(r) = better {
        out.push(format!("a higher seeder band: {}", describe(&r.stream)));
    }
    out
}

/// One line per corpus, title and profile, and the expectations that fail.
fn table() -> (String, Vec<String>, (u32, u32)) {
    let expected: serde_json::Value = read("expected.json");
    let expected: Vec<Expected> = serde_json::from_value(expected["expected"].clone()).unwrap();
    let metas: serde_json::Value = read("meta.json");
    let metas: Vec<Meta> = serde_json::from_value(metas["titles"].clone()).unwrap();
    let profiles = profiles();
    let mut out = String::new();
    let mut failures = Vec::new();
    let (mut met, mut pending) = (0, 0);
    for corpus in ["shipped", "default"] {
        let data: Corpus = read(&format!("{corpus}.json"));
        for (n, t) in data.titles.iter().enumerate() {
            let streams = parsed(t);
            let target = target(t);
            let meta = metas.iter().find(|m| m.id == t.id).expect("a meta");
            for p in profiles
                .iter()
                .filter(|p| profiles_of(corpus).contains(&p.name))
            {
                let conditions = for_title(&p.conditions, meta);
                let input = RankInput {
                    caps: &p.caps,
                    prefs: &p.prefs,
                    target: &target,
                    last_binge_group: None,
                    kids: None,
                    conditions: &conditions,
                };
                let ranked = rank(streams.clone(), &input);
                if default_user(corpus, p.name) {
                    for broken in owner_rule(&ranked, &p.prefs, &conditions) {
                        failures.push(format!(
                            "{corpus} #{} {} on {}: owner's rule: {broken}",
                            n + 1,
                            t.name,
                            p.name
                        ));
                    }
                }
                let best = ranked.best();
                let line = best.map(|r| kind_of(&r.line));
                let got = best.map_or("no pick".into(), |r| {
                    format!(
                        "{} {} {}",
                        describe(&r.stream),
                        kind_of(&r.class),
                        line.as_deref().unwrap_or("?")
                    )
                });
                let want = expected
                    .iter()
                    .find(|e| e.corpus == corpus && e.id == t.id && e.profile == p.name);
                let plan_key = want.and_then(|e| e.pick.as_deref());
                explain(corpus, n + 1, p.name, &ranked, plan_key);
                let verdict = match want {
                    None => String::new(),
                    Some(e) => {
                        // A named line must be the row's; none named is
                        // `quick` or `should`.
                        let line_ok = match (&e.line, line.as_deref()) {
                            (Some(want), Some(got)) => want == got,
                            (None, Some(got)) => matches!(got, "quick" | "should"),
                            _ => false,
                        };
                        let meets = match (&e.blocked, &e.pick) {
                            (Some(_), _) => best.is_none(),
                            (None, Some(key)) => {
                                best.is_some_and(|r| &r.stream.key == key) && line_ok
                            }
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
