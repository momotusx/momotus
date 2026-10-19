//! The phase 7 containment gate's runs that need no network
//! (plans/c21-c23-loopback.md, "Phase 7 gate"): a canary on `[::1]`, a
//! fixture origin on 127.0.0.1 serving F1 to F6 and N1 to N4, the core, and
//! mpv started headless for each run. Prints the versions it ran and its
//! preconditions, then one JSON line per run, checks each run against the
//! gate and exits 1 when one fails (2 when a precondition is missing, so a
//! part of the gate went untested).
//! `cargo run -p mq-server --example containment -- [mpv]`; needs mpv and
//! the ffmpeg command on the path (the benign media is made by ffmpeg).

#[path = "mpv/options.rs"]
mod options;

use mq_http::{ClientConfig, HttpClient, Url};
use mq_server::{Format, Lease, StreamServer, Upstream};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The user's environment in every run: a typical `no_proxy`, which names
/// the canary (`::1`, and `localhost` for the variant that names it so).
/// The app removes it (3.3); the controls of the old design keep it.
const USER_NO_PROXY: &str = "localhost,127.0.0.1,::1,.local";
const IV: &str = "0x00000000000000000000000000000001";
/// FFmpeg's HLS guard (2.4).
const GUARD: &str = "Opening this file was forbidden for security reasons";
/// The log evidence of each layer that can stop a probe: mpv's log first,
/// then the core's.
const LAYERS: [(&str, &str); 8] = [
    ("guard", GUARD),
    ("hls_probe", "Not detecting m3u8/hls"),
    ("concat_safe_mode", "Unsafe file name"),
    (
        "unsafe_playlist_entry",
        "Refusing to load potentially unsafe URL",
    ),
    ("no_demuxer", "Failed to recognize file format"),
    ("forbidden_403", "HTTP error 403"),
    ("not_on_whitelist", "not on whitelist"),
    ("hls_segment_failed", "Failed to open segment"),
];
const CORE_LAYERS: [(&str, &str); 2] = [
    ("host_check", "refused a request for another host"),
    ("blocked_address", "blocked address"),
];

/// What the canary saw: HTTP request lines, and accepts on the raw ports.
#[derive(Clone, Default)]
struct Canary {
    requests: Arc<Mutex<Vec<String>>>,
    https: Arc<AtomicU64>,
    tcp: Arc<AtomicU64>,
    crypto: Arc<AtomicU64>,
}

#[derive(Clone, Copy)]
struct Ports {
    http: SocketAddr,
    https: SocketAddr,
    tcp: SocketAddr,
    crypto: SocketAddr,
}

impl Canary {
    fn snapshot(&self) -> (usize, u64, u64, u64) {
        (
            self.requests.lock().unwrap().len(),
            self.https.load(Ordering::SeqCst),
            self.tcp.load(Ordering::SeqCst),
            self.crypto.load(Ordering::SeqCst),
        )
    }
}

async fn counter(count: Arc<AtomicU64>) -> SocketAddr {
    let listener = TcpListener::bind("[::1]:0").await.expect("canary binds");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            drop(socket);
        }
    });
    addr
}

async fn canary() -> (Canary, Ports) {
    let canary = Canary::default();
    let listener = TcpListener::bind("[::1]:0").await.expect("canary binds");
    let http = listener.local_addr().unwrap();
    let requests = canary.requests.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let requests = requests.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let line = head.lines().next().unwrap_or_default().to_owned();
                requests.lock().unwrap().push(line);
                let reply =
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    let ports = Ports {
        http,
        https: counter(canary.https.clone()).await,
        tcp: counter(canary.tcp.clone()).await,
        crypto: counter(canary.crypto.clone()).await,
    };
    (canary, ports)
}

type Pages = Arc<Mutex<HashMap<String, (&'static str, Vec<u8>)>>>;

/// The fixture origin: whole bodies by path, 404 otherwise.
async fn origin(pages: Pages) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let pages = pages.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 4096];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let target = head.split_whitespace().nth(1).unwrap_or("/");
                let path = target.split('?').next().unwrap_or("/").to_owned();
                let page = pages.lock().unwrap().get(&path).cloned();
                let reply = match page {
                    Some((kind, body)) => {
                        let mut out = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        out.extend_from_slice(&body);
                        out
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = socket.write_all(&reply).await;
            });
        }
    });
    base
}

/// Two one-second segments, clear and AES-128, and their key, made by ffmpeg.
fn media() -> HashMap<&'static str, Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    std::fs::write(d.join("key.bin"), b"0123456789abcdef").unwrap();
    let iv = IV.trim_start_matches("0x");
    std::fs::write(
        d.join("keyinfo"),
        format!("key.bin\n{}\n{iv}\n", d.join("key.bin").display()),
    )
    .unwrap();
    for (prefix, extra) in [
        ("seg", vec![]),
        (
            "enc",
            vec![
                "-hls_key_info_file".to_owned(),
                d.join("keyinfo").display().to_string(),
            ],
        ),
    ] {
        let status = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(["-f", "lavfi", "-i", "testsrc=size=160x90:rate=10"])
            .args(["-f", "lavfi", "-i", "sine=frequency=440"])
            .args(["-t", "2", "-c:v", "libx264", "-g", "10", "-c:a", "aac"])
            .args(["-f", "hls", "-hls_time", "1", "-hls_list_size", "0"])
            .args(&extra)
            .arg("-hls_segment_filename")
            .arg(d.join(format!("{prefix}%d.ts")))
            .arg(d.join(format!("{prefix}.m3u8")))
            .status()
            .expect("ffmpeg runs");
        assert!(status.success(), "ffmpeg made no {prefix} segments");
    }
    let mut out = HashMap::new();
    for name in ["seg0.ts", "seg1.ts", "enc0.ts", "enc1.ts", "key.bin"] {
        out.insert(name, std::fs::read(d.join(name)).unwrap());
    }
    out
}

fn media_playlist(lines: &[String]) -> String {
    let mut out = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n");
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("#EXT-X-ENDLIST\n");
    out
}

fn seg(uri: &str) -> String {
    format!("#EXTINF:1.0,\n{uri}")
}

fn key(uri: &str) -> String {
    format!("#EXT-X-KEY:METHOD=AES-128,URI=\"{uri}\",IV={IV}")
}

/// F1 to F6, with the benign parts at `clear`, `segs` and `key_uri`.
fn fixtures(
    ports: &Ports,
    clear: &str,
    segs: [&str; 4],
    key_uri: &str,
) -> Vec<(&'static str, String)> {
    let c = ports.http;
    let [_, seg1, enc0, enc1] = segs;
    vec![
        (
            "f1",
            format!("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100000\nhttp://{c}/f1.m3u8\n"),
        ),
        (
            "f2",
            format!(
                "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"probe\",DEFAULT=YES,AUTOSELECT=YES,URI=\"http://{c}/f2.m3u8\"\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=100000,AUDIO=\"a\"\n{clear}\n"
            ),
        ),
        (
            "f3",
            media_playlist(&[key(&format!("http://{c}/f3.key")), seg(enc0), seg(enc1)]),
        ),
        (
            "f4",
            media_playlist(&[seg(&format!("http://{c}/f4.ts")), seg(seg1)]),
        ),
        (
            "f5",
            media_playlist(&[key(key_uri), seg(&format!("http://{c}/f5.ts")), seg(enc1)]),
        ),
        (
            "f6",
            format!(
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100000\nhttps://{}/f6.m3u8\n",
                ports.https
            ),
        ),
    ]
}

/// N1 to N3, one per scheme, and `http` once more by the name `localhost`:
/// (name, nested file, the scheme's counter). N1 is a valid MPD padded
/// after its declaration.
fn nested(ports: &Ports) -> Vec<(String, String, &'static str)> {
    let localhost = format!("localhost:{}", ports.http.port());
    let mut out = Vec::new();
    for (scheme, host, counter, tag) in [
        ("http", ports.http.to_string(), "http", "http"),
        ("http", localhost, "http", "http-localhost"),
        ("tcp", ports.tcp.to_string(), "tcp", "tcp"),
        (
            "crypto+tcp",
            ports.crypto.to_string(),
            "crypto",
            "crypto-tcp",
        ),
    ] {
        let url = |n: &str| format!("{scheme}://{host}/{n}-{tag}");
        let mpd = format!(
            "<?xml version=\"1.0\"?>{}<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" profiles=\"urn:mpeg:dash:profile:isoff-live:2011\" \
             type=\"static\" mediaPresentationDuration=\"PT2S\" minBufferTime=\"PT1S\"><Period><AdaptationSet mimeType=\"video/mp2t\">\
             <Representation id=\"1\" bandwidth=\"1\"><BaseURL>{}/</BaseURL><SegmentList duration=\"1\"><SegmentURL media=\"s.ts\"/>\
             </SegmentList></Representation></AdaptationSet></Period></MPD>\n",
            " ".repeat(2048),
            url("n1")
        );
        out.push((format!("n1-{tag}"), mpd, counter));
        out.push((
            format!("n2-{tag}"),
            format!("ffconcat version 1.0\nfile '{}'\n", url("n2")),
            counter,
        ));
        out.push((
            format!("n3-{tag}"),
            media_playlist(&[seg(&url("n3"))]),
            counter,
        ));
    }
    out
}

/// How a run's mpv sees the world.
#[derive(Clone, Copy, PartialEq)]
enum Env {
    /// The user's, `no_proxy` included.
    User,
    /// The app's: the user's without `no_proxy` (3.3).
    App,
}

/// What one run left behind.
struct Seen {
    http: Vec<String>,
    https: u64,
    tcp: u64,
    crypto: u64,
    log: String,
    core: String,
    took: Duration,
    killed: bool,
    /// mpv's exit code; `None` when it was killed or died by a signal.
    exit: Option<i32>,
}

impl Seen {
    fn reached(&self) -> bool {
        !self.http.is_empty() || self.https + self.tcp + self.crypto > 0
    }

    fn lines(&self, needle: &str) -> usize {
        self.log.lines().filter(|l| l.contains(needle)).count()
    }

    fn core_lines(&self, needle: &str) -> usize {
        self.core.matches(needle).count()
    }

    /// The layers whose evidence is in the logs.
    fn stopped_by(&self) -> Vec<&'static str> {
        let mpv = LAYERS.iter().filter(|(_, n)| self.lines(n) > 0);
        let core = CORE_LAYERS.iter().filter(|(_, n)| self.core_lines(n) > 0);
        mpv.chain(core).map(|(layer, _)| *layer).collect()
    }
}

/// One mpv run, headless, killed at `limit`.
fn mpv(
    bin: &str,
    url: &str,
    options: &[String],
    env: Env,
    limit: Duration,
) -> (String, Duration, bool, Option<i32>) {
    let log = tempfile::NamedTempFile::new().unwrap();
    let started = Instant::now();
    let mut command = Command::new(bin);
    command
        .args([
            "--idle=no",
            "--vo=null",
            "--ao=null",
            "--end=2",
            "--network-timeout=5",
        ])
        .args(["--msg-level=all=debug"])
        .args(options::HARDENED)
        .arg(format!("--log-file={}", log.path().display()))
        .args(options)
        .arg(url)
        .env_remove("http_proxy")
        .env_remove("HTTP_PROXY")
        .env_remove("https_proxy")
        .env_remove("HTTPS_PROXY")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for name in options::CLEARED {
        match env {
            Env::User => command.env(name, USER_NO_PROXY),
            Env::App => command.env_remove(name),
        };
    }
    let mut child = command.spawn().expect("mpv runs");
    let mut killed = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > limit && !killed {
            let _ = child.kill();
            killed = true;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let took = started.elapsed();
    let log = std::fs::read_to_string(log.path()).unwrap_or_default();
    (log, took, killed, status.code().filter(|_| !killed))
}

/// The protected run's options (3.3): every instance's, then a manifest's.
fn protected(core: SocketAddr) -> Vec<String> {
    let mut out: Vec<String> = options::proxy(core).into();
    out.extend(options::MANIFEST.map(String::from));
    out
}

/// The design before the 2026-10-19 review: no `demuxer=lavf`, and the
/// app's environment kept `no_proxy`.
fn old_design(core: SocketAddr) -> Vec<String> {
    let mut out: Vec<String> = options::proxy(core).into();
    out.extend(
        options::MANIFEST
            .iter()
            .filter(|o| **o != "--demuxer=lavf")
            .map(|o| o.to_string()),
    );
    out
}

/// The backstop run's options: only the proxy and lavf's whitelist (no
/// `demuxer=lavf`, so mpv's playlist fallback runs too).
fn backstop(core: SocketAddr) -> Vec<String> {
    vec![
        "--access-references=yes".to_owned(),
        format!("--http-proxy=http://{core}"),
        "--demuxer-lavf-o=protocol_whitelist=[http,tcp,crypto]".to_owned(),
    ]
}

fn json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn list(items: &[&str]) -> String {
    let items: Vec<String> = items.iter().map(|i| json(i)).collect();
    format!("[{}]", items.join(", "))
}

/// The gate's verdicts so far.
struct Gate {
    date: String,
    failed: Vec<String>,
    untested: Vec<String>,
}

impl Gate {
    fn check(&mut self, run: &str, fixture: &str, ok: bool, rule: &str) {
        if !ok {
            let line = format!("{run} {fixture}: {rule}");
            eprintln!("FAILED {line}");
            self.failed.push(line);
        }
    }

    /// Prints one run's line.
    fn record(&self, run: &str, fixture: &str, seen: &Seen, untested: Option<&str>) {
        let exit = seen.exit.map_or("null".to_owned(), |c| c.to_string());
        let untested = untested.map_or("null".to_owned(), json);
        println!(
            "{{\"date\": {}, \"phase\": \"7\", \"run\": {}, \"fixture\": {}, \"canary_http\": {}, \"canary_https_accepts\": {}, \
             \"canary_tcp_accepts\": {}, \"canary_crypto_accepts\": {}, \"stopped_by\": {}, \"guard_lines\": {}, \"forbidden_403_lines\": {}, \
             \"not_on_whitelist_lines\": {}, \"video_out\": {}, \"core_foreign_host_refusals\": {}, \"core_blocked_address\": {}, \
             \"mpv_exit\": {exit}, \"seconds\": {:.1}, \"killed\": {}, \"untested\": {untested}}}",
            json(&self.date),
            json(run),
            json(fixture),
            json(&seen.http.join(" | ")),
            seen.https,
            seen.tcp,
            seen.crypto,
            list(&seen.stopped_by()),
            seen.lines(GUARD),
            seen.lines("HTTP error 403"),
            seen.lines("not on whitelist"),
            seen.lines("VO: [null]"),
            seen.core_lines("refused a request for another host"),
            seen.core_lines("blocked address"),
            seen.took.as_secs_f64(),
            seen.killed,
        );
    }
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A command's standard output, or nothing.
fn output(bin: &str, args: &[&str]) -> String {
    Command::new(bin)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// The first line of `text` that holds `key`, after it, trimmed.
fn after(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| l.split_once(key).map(|(_, v)| v.trim().to_owned()))
        .unwrap_or_default()
}

#[tokio::main]
async fn main() {
    let bin = std::env::args().nth(1).unwrap_or_else(|| "mpv".into());
    // Debug: the core's foreign-Host refusal is a debug line.
    let logs = Arc::new(Mutex::new(Vec::new()));
    let sink = logs.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(move || Sink(sink.clone()))
        .init();
    let core_log = || String::from_utf8_lossy(&logs.lock().unwrap()).into_owned();

    // Versions and preconditions, from the mpv under test.
    let mut gate = Gate {
        date: output("date", &["-u", "+%F"]).trim().to_owned(),
        failed: Vec::new(),
        untested: Vec::new(),
    };
    let version = output(&bin, &["--no-config", "--version"]);
    let probe = output(
        &bin,
        &[
            "--no-config",
            "--idle=no",
            "--vo=null",
            "--ao=null",
            "--end=0.1",
            "--term-playing-msg=LAVF=${demuxer-lavf-list}",
            "av://lavfi:anullsrc",
        ],
    );
    let demuxers = after(&probe, "LAVF=");
    let has = |d: &str| demuxers.split(',').any(|x| x == d);
    let missing: Vec<&str> = ["dash", "hls", "concat"]
        .into_iter()
        .filter(|d| !has(d))
        .collect();
    let curl = output(&bin, &["--no-config", "--list-options"])
        .to_lowercase()
        .contains("curl");
    println!(
        "{{\"date\": {}, \"phase\": \"7\", \"check\": \"versions and preconditions\", \"mpv\": {}, \"ffmpeg\": {}, \
         \"libavformat\": {}, \"missing_demuxers\": {}, \"curl_option\": {curl}}}",
        json(&gate.date),
        json(version.split_whitespace().nth(1).unwrap_or_default()),
        json(&after(&version, "FFmpeg version:")),
        json(&after(&version, "libavformat")),
        list(&missing),
    );
    gate.check("precondition", "mpv", !curl, "no curl option");
    gate.check(
        "precondition",
        "lavf",
        has("hls") && has("concat"),
        "hls and concat demuxers",
    );
    let n1 = (!has("dash")).then_some("no dash demuxer in this mpv's libavformat");
    if let Some(why) = n1 {
        gate.untested.push(format!("N1: {why}"));
    }
    let untested = |fixture: &str| n1.filter(|_| fixture.contains("n1-"));

    let (canary, ports) = canary().await;
    let pages: Pages = Arc::default();
    let base = origin(pages.clone()).await;
    let page = |path: &str, kind: &'static str, body: Vec<u8>| {
        pages.lock().unwrap().insert(path.to_owned(), (kind, body));
    };
    let media = media();
    for (name, body) in &media {
        let kind = if name.ends_with(".ts") {
            "video/mp2t"
        } else {
            "application/octet-stream"
        };
        page(&format!("/{name}"), kind, body.clone());
    }
    const HLS: &str = "application/vnd.apple.mpegurl";
    page(
        "/clear.m3u8",
        HLS,
        media_playlist(&[seg("seg0.ts"), seg("seg1.ts")]).into_bytes(),
    );
    let names = ["seg0.ts", "seg1.ts", "enc0.ts", "enc1.ts"];
    for (name, text) in fixtures(&ports, "clear.m3u8", names, "key.bin") {
        page(&format!("/{name}.m3u8"), HLS, text.into_bytes());
    }
    let nested = nested(&ports);
    for (name, body, _) in &nested {
        let body = body.clone().into_bytes();
        page(
            &format!("/{name}.top"),
            "application/octet-stream",
            body.clone(),
        );
        page(&format!("/{name}.ts"), "video/mp2t", body);
        let wrap = media_playlist(&[seg(&format!("{name}.ts"))]);
        page(&format!("/wrap-{name}.m3u8"), HLS, wrap.into_bytes());
    }
    let n4 = b"ffconcat version 1.0\nfile 'n4.ts'\n".to_vec();
    page("/n4.ts", "video/mp2t", n4);
    let wrap = media_playlist(&[seg("n4.ts")]);
    page("/wrap-n4.m3u8", HLS, wrap.into_bytes());

    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let server = StreamServer::start(http).await.unwrap();
    let core = server.addr();
    let scope = Url::parse(&format!("{base}/")).ok();
    let upstream = |url: String| Upstream::Http {
        url,
        headers: Vec::new(),
        addon_url: scope.clone(),
    };
    let limit = Duration::from_secs(20);
    let run = |name: &str, fixture: &str, url: &str, options: &[String], env: Env| {
        let before = canary.snapshot();
        let log_before = core_log().len();
        let (log, took, killed, exit) = mpv(&bin, url, options, env, limit);
        // CONTAINMENT_LOGS=<dir> keeps each run's mpv log.
        if let Some(dir) = std::env::var_os("CONTAINMENT_LOGS") {
            let file = format!("{name}-{fixture}.log").replace([' ', ':', '/', '='], "_");
            let _ = std::fs::write(std::path::Path::new(&dir).join(file), &log);
        }
        let after = canary.snapshot();
        Seen {
            http: canary.requests.lock().unwrap()[before.0..].to_vec(),
            https: after.1 - before.1,
            tcp: after.2 - before.2,
            crypto: after.3 - before.3,
            log,
            core: core_log()[log_before..].to_owned(),
            took,
            killed,
            exit,
        }
    };
    let open = |name: String| {
        server.open_for(
            upstream(format!("{base}/{name}")),
            "v.mkv",
            limit,
            &[Format::Hls],
        )
    };
    let mut leases: Vec<Lease> = Vec::new();

    // Precondition: the benign playlist plays through the core, its own
    // URLs going through the core as the proxy (no `no_proxy`).
    let lease = open("clear.m3u8".into()).await.unwrap();
    let seen = run(
        "precondition",
        "clear",
        &lease.url,
        &protected(core),
        Env::App,
    );
    gate.record(
        "precondition: benign HLS through open",
        "clear",
        &seen,
        None,
    );
    let played = seen.lines("VO: [null]") > 0 && seen.exit == Some(0);
    gate.check(
        "precondition",
        "clear",
        played && !seen.reached(),
        "plays, reaches nothing",
    );
    leases.push(lease);

    // Control, F1 to F6: the raw fixture, no protection; each reaches.
    let refs = ["--access-references=yes".to_owned()];
    for (name, _) in fixtures(&ports, "", ["", "", "", ""], "") {
        let seen = run(
            "control",
            name,
            &format!("{base}/{name}.m3u8"),
            &refs,
            Env::User,
        );
        gate.record("control", name, &seen, None);
        let own = seen.http.iter().any(|l| l.contains(&format!("/{name}")));
        let reached = if name == "f6" { seen.https > 0 } else { own };
        gate.check("control", name, reached, "its probe reaches the canary");
    }
    // Control, nested: top level, then as an HLS segment, no protection.
    // N3's http entries reach the canary through mpv's playlist fallback,
    // and N1's where FFmpeg has a dash demuxer (the fallback opens the MPD
    // as a file of its own); FFmpeg and mpv refuse every other one first.
    for (name, _, _) in &nested {
        for (run_name, url) in [
            ("control nested top-level", format!("{base}/{name}.top")),
            (
                "control nested as segment",
                format!("{base}/wrap-{name}.m3u8"),
            ),
        ] {
            let seen = run(run_name, name, &url, &refs, Env::User);
            gate.record(run_name, name, &seen, untested(name));
            if untested(name).is_none() && !name.starts_with("n1-http") {
                let fallback = name.starts_with("n3-http");
                let rule = if fallback {
                    "reaches the canary"
                } else {
                    "reaches nothing"
                };
                gate.check(run_name, name, seen.reached() == fallback, rule);
            }
        }
    }
    // The design before review: `no_proxy` kept, no `demuxer=lavf`. A
    // child the user's `no_proxy` names goes around the proxy; each fix
    // alone closes it.
    for name in ["n3-http", "n3-http-localhost"] {
        let wrap = format!("wrap-{name}.m3u8");
        for (run_name, options, env, escapes) in [
            (
                "old design: no_proxy kept",
                old_design(core),
                Env::User,
                true,
            ),
            (
                "demuxer=lavf alone, no_proxy kept",
                protected(core),
                Env::User,
                false,
            ),
            ("no_proxy cleared alone", old_design(core), Env::App, false),
        ] {
            let lease = open(wrap.clone()).await.unwrap();
            let seen = run(run_name, &wrap, &lease.url, &options, env);
            gate.record(run_name, &wrap, &seen, None);
            let rule = if escapes {
                "reaches the canary"
            } else {
                "reaches nothing"
            };
            gate.check(run_name, &wrap, seen.reached() == escapes, rule);
            leases.push(lease);
        }
    }
    // Protected: F1 to F6 and N1 to N4 through open, with 3.3's options and
    // environment; nothing reaches the canary.
    let protect: Vec<String> = fixtures(&ports, "", ["", "", "", ""], "")
        .into_iter()
        .map(|(n, _)| format!("{n}.m3u8"))
        .chain(nested.iter().map(|(n, _, _)| format!("wrap-{n}.m3u8")))
        .chain(["wrap-n4.m3u8".to_owned()])
        .collect();
    for name in &protect {
        let lease = match open(name.clone()).await {
            Ok(lease) => lease,
            Err(words) => {
                println!(
                    "{{\"date\": {}, \"phase\": \"7\", \"run\": \"protected\", \"fixture\": {}, \"open_refused\": {}}}",
                    json(&gate.date),
                    json(name),
                    json(&words)
                );
                continue;
            }
        };
        let seen = run("protected", name, &lease.url, &protected(core), Env::App);
        gate.record("protected", name, &seen, untested(name));
        gate.check("protected", name, !seen.reached(), "reaches nothing");
        if name.starts_with('f') {
            let blocked = seen.core_lines("blocked address") > 0;
            gate.check("protected", name, blocked, "the core refuses its probe");
        } else if name == "wrap-n4.m3u8" {
            let errs = !seen.killed && seen.exit != Some(0) && seen.took < Duration::from_secs(10);
            gate.check("protected", name, errs, "an error within 10 s");
        } else if untested(name).is_none() {
            let shown = !seen.stopped_by().is_empty();
            gate.check("protected", name, shown, "the log shows what stopped it");
        }
        leases.push(lease);
    }
    // Backstop: F1 to F6 and the nested formats raw through register, with
    // only the proxy and the whitelist; the benign parts are the core's own
    // registered URLs, so they pass the Host check.
    let reg = |path: &str, name: &str| server.register(upstream(format!("{base}{path}")), name);
    let s = names.map(|n| reg(&format!("/{n}"), n));
    let k = reg("/key.bin", "key.bin");
    let segs = [
        s[0].url.as_str(),
        s[1].url.as_str(),
        s[2].url.as_str(),
        s[3].url.as_str(),
    ];
    page(
        "/bk/clear.m3u8",
        HLS,
        media_playlist(&[seg(segs[0]), seg(segs[1])]).into_bytes(),
    );
    let clear = reg("/bk/clear.m3u8", "clear.m3u8");
    let mut backstops: Vec<(String, Lease)> = Vec::new();
    for (name, text) in fixtures(&ports, &clear.url, segs, &k.url) {
        page(&format!("/bk/{name}.m3u8"), HLS, text.into_bytes());
        let lease = reg(&format!("/bk/{name}.m3u8"), &format!("{name}.m3u8"));
        backstops.push((name.to_owned(), lease));
    }
    for (name, _, _) in &nested {
        let segment = reg(&format!("/{name}.ts"), &format!("{name}.ts"));
        let wrap = media_playlist(&[seg(&segment.url)]);
        page(&format!("/bk/wrap-{name}.m3u8"), HLS, wrap.into_bytes());
        let lease = reg(&format!("/bk/wrap-{name}.m3u8"), "wrap.m3u8");
        backstops.push((format!("wrap-{name}"), lease));
        leases.push(segment);
    }
    for (name, lease) in &backstops {
        let seen = run("backstop", name, &lease.url, &backstop(core), Env::App);
        gate.record("backstop", name, &seen, untested(name));
        gate.check("backstop", name, !seen.reached(), "reaches nothing");
        let refused = seen.lines("HTTP error 403") > 0
            && seen.core_lines("refused a request for another host") > 0;
        if (name.starts_with('f') && name != "f6") || name.starts_with("wrap-n3-http") {
            gate.check(
                "backstop",
                name,
                refused,
                "the core's Host check refuses its probe",
            );
        } else if untested(name).is_none() {
            let shown = !seen.stopped_by().is_empty();
            gate.check("backstop", name, shown, "the log shows what stopped it");
        }
    }

    let verdict = if !gate.failed.is_empty() {
        1
    } else if !gate.untested.is_empty() {
        2
    } else {
        0
    };
    let failed: Vec<&str> = gate.failed.iter().map(String::as_str).collect();
    let untested: Vec<&str> = gate.untested.iter().map(String::as_str).collect();
    println!(
        "{{\"date\": {}, \"phase\": \"7\", \"check\": \"gate verdict\", \"failed\": {}, \"untested\": {}, \"exit\": {verdict}}}",
        json(&gate.date),
        list(&failed),
        list(&untested),
    );
    drop((s, k, clear, leases, backstops));
    std::process::exit(verdict);
}
