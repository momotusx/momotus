//! The phase 7 containment gate's runs that need no network
//! (plans/c21-c23-loopback.md, "Phase 7 gate"): a canary on `[::1]`, a
//! fixture origin on 127.0.0.1 serving F1 to F6 and N1 to N4, the core, and
//! mpv started headless for each run. Prints one JSON line per run.
//! `cargo run -p mq-server --example containment -- [mpv]`; needs mpv and
//! the ffmpeg command on the path (the benign media is made by ffmpeg).

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

/// The environment every run has: a typical `no_proxy` the canary does not
/// match.
const NO_PROXY: &str = "localhost,127.0.0.1,.local";
const IV: &str = "0x00000000000000000000000000000001";
/// FFmpeg's HLS guard (2.4).
const GUARD: &str = "Opening this file was forbidden for security reasons";
/// The desktop app's options on every mpv instance (m3-desktop 4.3), but
/// `access-references`, which each run sets.
const HARDENED: [&str; 12] = [
    "--ytdl=no",
    "--load-scripts=no",
    "--osc=no",
    "--input-default-bindings=no",
    "--input-vo-keyboard=no",
    "--load-stats-overlay=no",
    "--load-auto-profiles=no",
    "--ordered-chapters=no",
    "--sub-auto=no",
    "--audio-file-auto=no",
    "--cover-art-auto=no",
    "--load-unsafe-playlists=no",
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

/// N1 to N3, one per scheme: (name, nested file, the scheme's counter).
fn nested(ports: &Ports) -> Vec<(String, String, &'static str)> {
    let mut out = Vec::new();
    for (scheme, host, counter) in [
        ("http", ports.http, "http"),
        ("tcp", ports.tcp, "tcp"),
        ("crypto+tcp", ports.crypto, "crypto"),
    ] {
        let tag = scheme.replace('+', "-");
        let url = |n: &str| format!("{scheme}://{host}/{n}-{tag}");
        let mpd = format!(
            "{}<?xml version=\"1.0\"?><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" profiles=\"urn:mpeg:dash:profile:isoff-live:2011\" \
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

/// One mpv run, headless, killed at `limit`; returns its log and how long it took.
fn mpv(bin: &str, url: &str, options: &[String], limit: Duration) -> (String, Duration, bool) {
    let log = tempfile::NamedTempFile::new().unwrap();
    let started = Instant::now();
    let mut child = Command::new(bin)
        .args([
            "--no-config",
            "--idle=no",
            "--vo=null",
            "--ao=null",
            "--end=2",
            "--network-timeout=5",
        ])
        .args(["--msg-level=all=debug"])
        .args(HARDENED)
        .arg(format!("--log-file={}", log.path().display()))
        .args(options)
        .arg(url)
        .env_remove("http_proxy")
        .env_remove("HTTP_PROXY")
        .env_remove("https_proxy")
        .env_remove("HTTPS_PROXY")
        .env("no_proxy", NO_PROXY)
        .env("NO_PROXY", NO_PROXY)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("mpv runs");
    let mut killed = false;
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > limit {
            let _ = child.kill();
            killed = true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let took = started.elapsed();
    (
        std::fs::read_to_string(log.path()).unwrap_or_default(),
        took,
        killed,
    )
}

fn protected(core: SocketAddr) -> Vec<String> {
    vec![
        "--access-references=yes".into(),
        format!("--http-proxy=http://{core}"),
        "--demuxer-lavf-propagate-opts=yes".into(),
        "--demuxer-lavf-o=protocol_whitelist=[http,tcp,crypto]".into(),
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

/// Prints one run's line: what reached the canary during it, and the log
/// evidence asked for.
#[allow(clippy::too_many_arguments)]
fn record(
    run: &str,
    fixture: &str,
    canary: &Canary,
    before: (usize, u64, u64, u64),
    log: &str,
    took: Duration,
    killed: bool,
    core: &str,
) {
    let after = canary.snapshot();
    let seen: Vec<String> = canary.requests.lock().unwrap()[before.0..].to_vec();
    let lines = |needle: &str| log.lines().filter(|l| l.contains(needle)).count();
    let core_count = |needle: &str| core.matches(needle).count();
    println!(
        "{{\"date\": \"2026-10-19\", \"phase\": \"7\", \"run\": {}, \"fixture\": {}, \"canary_http\": {}, \"canary_https_accepts\": {}, \
         \"canary_tcp_accepts\": {}, \"canary_crypto_accepts\": {}, \"guard_lines\": {}, \"forbidden_403_lines\": {}, \"not_on_whitelist_lines\": {}, \
         \"hls_probe_refusals\": {}, \"concat_unsafe_lines\": {}, \"unsafe_playlist_refusals\": {}, \"mpv_entries_played\": {}, \"video_out\": {}, \
         \"core_foreign_host_refusals\": {}, \"core_blocked_address\": {}, \"seconds\": {:.1}, \"killed\": {}}}",
        json(run),
        json(fixture),
        json(&seen.join(" | ")),
        after.1 - before.1,
        after.2 - before.2,
        after.3 - before.3,
        lines(GUARD),
        lines("HTTP error 403"),
        lines("not on whitelist"),
        lines("Not detecting m3u8/hls"),
        lines("Unsafe file name"),
        lines("Refusing to load potentially unsafe URL"),
        lines("[cplayer] Playing: "),
        lines("VO: [null]"),
        core_count("refused a request for another host"),
        core_count("blocked address"),
        took.as_secs_f64(),
        killed,
    );
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

#[tokio::main]
async fn main() {
    let bin = std::env::args().nth(1).unwrap_or_else(|| "mpv".into());
    let logs = Arc::new(Mutex::new(Vec::new()));
    let sink = logs.clone();
    tracing_subscriber::fmt()
        .with_writer(move || Sink(sink.clone()))
        .init();
    let core_log = || String::from_utf8_lossy(&logs.lock().unwrap()).into_owned();

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
    for (name, text) in fixtures(
        &ports,
        "clear.m3u8",
        ["seg0.ts", "seg1.ts", "enc0.ts", "enc1.ts"],
        "key.bin",
    ) {
        page(&format!("/{name}.m3u8"), HLS, text.into_bytes());
    }
    let nested = nested(&ports);
    for (name, body, _) in &nested {
        page(
            &format!("/{name}.top"),
            "application/octet-stream",
            body.clone().into_bytes(),
        );
        page(
            &format!("/{name}.ts"),
            "video/mp2t",
            body.clone().into_bytes(),
        );
        page(
            &format!("/wrap-{name}.m3u8"),
            HLS,
            media_playlist(&[seg(&format!("{name}.ts"))]).into_bytes(),
        );
    }
    page(
        "/n4.ts",
        "video/mp2t",
        b"ffconcat version 1.0\nfile 'n4.ts'\n".to_vec(),
    );
    page(
        "/wrap-n4.m3u8",
        HLS,
        media_playlist(&[seg("n4.ts")]).into_bytes(),
    );

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
    let run = |name: &str, fixture: &str, url: &str, options: &[String]| {
        let before = canary.snapshot();
        let log_before = core_log().len();
        let (log, took, killed) = mpv(&bin, url, options, limit);
        // CONTAINMENT_LOGS=<dir> keeps each run's mpv log.
        if let Some(dir) = std::env::var_os("CONTAINMENT_LOGS") {
            let file = format!("{name}-{fixture}.log").replace([' ', ':', '/'], "_");
            let _ = std::fs::write(std::path::Path::new(&dir).join(file), &log);
        }
        let core = core_log()[log_before..].to_owned();
        record(name, fixture, &canary, before, &log, took, killed, &core);
    };

    // Preconditions: the benign playlist plays through the core.
    let lease = server
        .open_for(
            upstream(format!("{base}/clear.m3u8")),
            "v.mkv",
            limit,
            &[Format::Hls],
        )
        .await
        .unwrap();
    run(
        "precondition: benign HLS through open",
        "clear",
        &lease.url,
        &protected(core),
    );

    // Control, F1 to F6: the raw fixture, no protection.
    let refs = ["--access-references=yes".to_owned()];
    for (name, _) in fixtures(&ports, "", ["", "", "", ""], "") {
        run("control", name, &format!("{base}/{name}.m3u8"), &refs);
    }
    // Control, nested: top level, then as an HLS segment, no protection.
    for (name, _, _) in &nested {
        run(
            "control nested top-level",
            name,
            &format!("{base}/{name}.top"),
            &refs,
        );
        run(
            "control nested as segment",
            name,
            &format!("{base}/wrap-{name}.m3u8"),
            &refs,
        );
    }
    // Protected: F1 to F6 and N1 to N4 through open, with 3.3's options.
    let mut leases: Vec<Lease> = Vec::new();
    let names: Vec<String> = fixtures(&ports, "", ["", "", "", ""], "")
        .into_iter()
        .map(|(n, _)| format!("{n}.m3u8"))
        .chain(nested.iter().map(|(n, _, _)| format!("wrap-{n}.m3u8")))
        .chain(["wrap-n4.m3u8".to_owned()])
        .collect();
    for name in &names {
        let opened = server.open_for(
            upstream(format!("{base}/{name}")),
            "v.mkv",
            limit,
            &[Format::Hls],
        );
        match opened.await {
            Ok(lease) => {
                run("protected", name, &lease.url, &protected(core));
                leases.push(lease);
            }
            Err(words) => println!(
                "{{\"date\": \"2026-10-19\", \"phase\": \"7\", \"run\": \"protected\", \"fixture\": {}, \"open_refused\": {}}}",
                json(name),
                json(&words)
            ),
        }
    }
    // Backstop: F1 to F6 raw through register; the benign parts are the
    // core's own registered URLs, so they pass the Host check.
    let reg = |path: &str, name: &str| server.register(upstream(format!("{base}{path}")), name);
    let s = ["seg0.ts", "seg1.ts", "enc0.ts", "enc1.ts"].map(|n| reg(&format!("/{n}"), n));
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
    let backstop: Vec<String> = [
        "--access-references=yes".to_owned(),
        format!("--http-proxy=http://{core}"),
        "--demuxer-lavf-o=protocol_whitelist=[http,tcp,crypto]".into(),
        "--demuxer-lavf-format=hls".into(),
    ]
    .into();
    for (name, text) in fixtures(&ports, &clear.url, segs, &k.url) {
        page(&format!("/bk/{name}.m3u8"), HLS, text.into_bytes());
        let lease = reg(&format!("/bk/{name}.m3u8"), &format!("{name}.m3u8"));
        run("backstop", name, &lease.url, &backstop);
    }
    drop((s, k, clear, leases));
}
