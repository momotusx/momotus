//! Golden test over PTT's own test tables (`testdata/ptt_cases.json`).
//!
//! Expected values are PTT vocabulary; they are mapped through the same label tables the
//! parser uses, so the comparison checks detection, not naming.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::convert::{audio_from, codec_from, hdr_from, quality_from, resolution_from};
use crate::model::{Audio, ParsedRelease, Quality, Resolution, VideoCodec};
use crate::parse;

#[derive(Deserialize)]
struct Case {
    input: String,
    source: String,
    full: bool,
    expected: Map<String, Value>,
}

const FIELDS: [&str; 20] = [
    "title",
    "year",
    "resolution",
    "quality",
    "codec",
    "bit_depth",
    "hdr",
    "audio",
    "channels",
    "languages",
    "seasons",
    "episodes",
    "complete",
    "group",
    "container",
    "proper",
    "repack",
    "hardcoded",
    "dubbed",
    "3d",
];
const EXTRA_FIELDS: [&str; 2] = ["upscaled", "edition"];

fn ptt_audio(a: Audio) -> &'static str {
    match a {
        Audio::Aac => "AAC",
        Audio::Ac3 => "Dolby Digital",
        Audio::Eac3 => "Dolby Digital Plus",
        Audio::Dts | Audio::DtsHd => "DTS Lossy",
        Audio::DtsHdMa | Audio::DtsX => "DTS Lossless",
        Audio::TrueHd => "TrueHD",
        Audio::Atmos => "Atmos",
        Audio::Flac => "FLAC",
        Audio::Opus => "OPUS",
        Audio::Mp3 => "MP3",
        Audio::Pcm => "PCM",
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn dedup(items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in items {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out
}

fn ints(v: &Value) -> Vec<u32> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default()
}

fn best_channel(list: &[String]) -> Option<String> {
    ["7.1", "5.1", "2.0", "stereo", "mono"]
        .into_iter()
        .find(|c| list.iter().any(|l| l == c))
        .map(str::to_string)
}

/// `Ok` when our value matches the expected one; `Err` carries "expected vs got".
fn check(field: &str, exp: &Value, got: &ParsedRelease, case: &Case) -> Result<(), String> {
    let null = exp.is_null();
    let eq = |ok: bool, e: String, g: String| {
        if ok {
            Ok(())
        } else {
            Err(format!("{e} != {g}"))
        }
    };
    match field {
        "title" => eq(
            exp.as_str() == Some(&got.title),
            exp.to_string(),
            got.title.clone(),
        ),
        "year" => eq(
            exp.as_u64().map(|y| y as u16) == got.year,
            exp.to_string(),
            format!("{:?}", got.year),
        ),
        "resolution" => {
            let want = exp.as_str().map_or(Resolution::Unknown, resolution_from);
            eq(
                want == got.resolution,
                format!("{want:?}"),
                format!("{:?}", got.resolution),
            )
        }
        "quality" => {
            let want = exp.as_str().map_or(Quality::Unknown, quality_from);
            eq(
                want == got.quality,
                format!("{want:?}"),
                format!("{:?}", got.quality),
            )
        }
        "codec" => {
            let want = exp.as_str().map_or(VideoCodec::Unknown, codec_from);
            eq(
                want == got.codec,
                format!("{want:?}"),
                format!("{:?}", got.codec),
            )
        }
        "bit_depth" => {
            let want = exp
                .as_str()
                .and_then(|s| s.strip_suffix("bit")?.parse::<u8>().ok());
            eq(
                want == got.bit_depth,
                format!("{want:?}"),
                format!("{:?}", got.bit_depth),
            )
        }
        "hdr" => {
            let mut want: Vec<_> = strings(exp).iter().filter_map(|s| hdr_from(s)).collect();
            let mut have = got.hdr.clone();
            want.sort_by_key(|h| format!("{h:?}"));
            have.sort_by_key(|h| format!("{h:?}"));
            eq(want == have, format!("{want:?}"), format!("{have:?}"))
        }
        "audio" => {
            let want: Vec<String> = dedup(strings(exp))
                .into_iter()
                .filter(|a| audio_from(a).is_some() || a.starts_with("DTS"))
                .collect();
            let have = dedup(
                got.audio
                    .iter()
                    .map(|a| ptt_audio(*a).to_string())
                    .collect(),
            );
            eq(want == have, format!("{want:?}"), format!("{have:?}"))
        }
        "channels" => {
            let want = best_channel(&strings(exp));
            eq(
                want == got.channels,
                format!("{want:?}"),
                format!("{:?}", got.channels),
            )
        }
        "languages" => {
            let want = dedup(
                strings(exp)
                    .into_iter()
                    .map(|l| if l == "la" { "es".into() } else { l })
                    .collect(),
            );
            eq(
                want == got.languages,
                format!("{want:?}"),
                format!("{:?}", got.languages),
            )
        }
        "seasons" => eq(
            ints(exp) == got.seasons,
            format!("{:?}", ints(exp)),
            format!("{:?}", got.seasons),
        ),
        "episodes" => eq(
            ints(exp) == got.episodes,
            format!("{:?}", ints(exp)),
            format!("{:?}", got.episodes),
        ),
        "complete" => eq(
            !null == got.complete,
            exp.to_string(),
            got.complete.to_string(),
        ),
        "group" => eq(
            exp.as_str() == got.group.as_deref(),
            exp.to_string(),
            format!("{:?}", got.group),
        ),
        "container" => {
            let ext = case.expected.get("extension").and_then(Value::as_str);
            let ok = match (&got.container, exp.as_str()) {
                (None, None) => true,
                (Some(c), Some(e)) => c == e,
                (Some(c), None) => Some(c.as_str()) == ext,
                (None, Some(_)) => false,
            };
            eq(ok, exp.to_string(), format!("{:?}", got.container))
        }
        "proper" => eq(!null == got.proper, exp.to_string(), got.proper.to_string()),
        "repack" => eq(!null == got.repack, exp.to_string(), got.repack.to_string()),
        "hardcoded" => eq(
            !null == got.hardcoded_subs,
            exp.to_string(),
            got.hardcoded_subs.to_string(),
        ),
        "dubbed" => eq(!null == got.dubbed, exp.to_string(), got.dubbed.to_string()),
        "3d" => eq(
            !null == got.three_d,
            exp.to_string(),
            got.three_d.to_string(),
        ),
        "upscaled" => eq(
            !null == got.upscaled,
            exp.to_string(),
            got.upscaled.to_string(),
        ),
        "edition" => eq(
            (exp.as_str() == Some("Extended Edition")) == got.extended,
            exp.to_string(),
            got.extended.to_string(),
        ),
        _ => Ok(()),
    }
}

#[derive(Default)]
struct Tally {
    pass: usize,
    total: usize,
    failures: Vec<String>,
}

fn fields_to_check(case: &Case) -> Vec<&'static str> {
    FIELDS
        .iter()
        .chain(EXTRA_FIELDS.iter())
        .copied()
        .filter(|f| case.expected.contains_key(*f) || (case.full && FIELDS.contains(f)))
        .collect()
}

/// Compare every case; prints per-field rates and returns (passed, total).
fn run(cases: &[Case]) -> (usize, usize) {
    let mut by_field: BTreeMap<&str, Tally> = BTreeMap::new();
    for case in cases {
        let got = parse(&case.input);
        for field in fields_to_check(case) {
            let exp = case.expected.get(field).cloned().unwrap_or(Value::Null);
            let t = by_field.entry(field).or_default();
            t.total += 1;
            match check(field, &exp, &got, case) {
                Ok(()) => t.pass += 1,
                Err(e) => t
                    .failures
                    .push(format!("[{}] {:?}: {e}", case.source, case.input)),
            }
        }
    }
    let (pass, total) = by_field
        .values()
        .fold((0, 0), |a, t| (a.0 + t.pass, a.1 + t.total));
    for (f, t) in &by_field {
        println!(
            "{f:<11} {}/{} ({:.1}%)",
            t.pass,
            t.total,
            100.0 * t.pass as f64 / t.total as f64
        );
    }
    println!(
        "TOTAL {pass}/{total} ({:.2}%)",
        100.0 * pass as f64 / total as f64
    );
    if std::env::var_os("RELPARSE_SHOW_FAILURES").is_some() {
        for (f, t) in &by_field {
            t.failures.iter().for_each(|m| println!("FAIL {f}: {m}"));
        }
    }
    (pass, total)
}

#[test]
fn ptt_golden_cases() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../testdata/ptt_cases.json")).unwrap();
    let (pass, total) = run(&cases);
    assert!(
        pass * 100 >= total * 95,
        "golden pass rate below 95%: {pass}/{total}"
    );
}

/// Dev aid: agreement with upstream PTT output (same case format) given as `RELPARSE_ORACLE=<file>`.
#[test]
fn oracle_agreement() {
    let Some(path) = std::env::var_os("RELPARSE_ORACLE") else {
        return;
    };
    let cases: Vec<Case> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    run(&cases);
}
