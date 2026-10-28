//! Golden JSON of what the core sends, under `apps/shared/fixtures/`: the
//! app tests (Android, web, Apple) load these files instead of building
//! their own. A test calls `golden(name, &snapshot)` at the state it pins;
//! the file must equal the snapshot as serialized, with this run's
//! loopback ports, tokens and clock read as fixed values. With
//! `MQ_GOLDEN=write` the file is written instead. A state both builds
//! reach is pinned from the store build (no torrent engine), where the
//! snapshot says so (`conditions.engine`); `golden_torrent` pins the ones
//! only the engine reaches. Writing them all takes both runs:
//! `cargo test -p mq-runtime --test stream_choice --test kids_play`, with
//! and without `--features torrent`.

use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

/// A state pinned from the store build; nothing in a torrent build.
#[allow(dead_code)]
pub fn golden(name: &str, snapshot: &impl Serialize) {
    if !cfg!(feature = "torrent") {
        pin(name, snapshot);
    }
}

/// A state only the torrent engine reaches.
#[allow(dead_code)]
pub fn golden_torrent(name: &str, snapshot: &impl Serialize) {
    if cfg!(feature = "torrent") {
        pin(name, snapshot);
    }
}

fn pin(name: &str, snapshot: &impl Serialize) {
    let mut value = serde_json::to_value(snapshot).unwrap();
    steady(&mut value);
    let text = serde_json::to_string_pretty(&value).unwrap() + "\n";
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/shared/fixtures")
        .join(format!("{name}.json"));
    if std::env::var("MQ_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let kept = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; write it with MQ_GOLDEN=write", path.display()));
    assert!(
        kept == text,
        "{} differs from what the core sends; MQ_GOLDEN=write rewrites it:\n{text}",
        path.display()
    );
}

/// This run's own values read as fixed ones: a loopback port as 1, a
/// 64-hex token as zeros, a clock time as 0, this machine's free space as
/// unknown (every build reads it, plans/offline-downloads.md 4.9).
fn steady(value: &mut Value) {
    match value {
        Value::String(s) => *s = steady_text(s),
        Value::Array(items) => items.iter_mut().for_each(steady),
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                if CLOCK.contains(&key.as_str()) && field.is_number() {
                    *field = 0.into();
                } else if key == "freeBytes" {
                    *field = Value::Null;
                } else {
                    steady(field);
                }
            }
        }
        _ => {}
    }
}

/// Fields that hold a time read from this run's clock.
const CLOCK: &[&str] = &["startBudgetEndsAt"];

fn steady_text(text: &str) -> String {
    const HOST: &str = "127.0.0.1:";
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(HOST) {
        out.push_str(&rest[..at + HOST.len()]);
        rest = &rest[at + HOST.len()..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 {
            out.push('1');
            rest = &rest[digits..];
        }
    }
    out.push_str(rest);
    let chars: Vec<char> = out.chars().collect();
    let mut steady = String::new();
    let mut i = 0;
    while i < chars.len() {
        let run = chars[i..]
            .iter()
            .take_while(|c| c.is_ascii_hexdigit())
            .count();
        if run == 64 {
            steady.push_str(&"0".repeat(64));
            i += 64;
        } else {
            steady.extend(&chars[i..i + run.max(1)]);
            i += run.max(1);
        }
    }
    steady
}
