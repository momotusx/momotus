//! Serves one HTTP(S) URL through the loopback server and prints the local
//! URL, for timing players against it and for the containment gate
//! (plans/c21-c23-loopback.md, phase 7): `cargo run -p mq-server --example
//! serve -- [--open] <url> [name]`. Without `--open` the URL is registered
//! raw, as the gate's backstop run needs; with it, it is opened as the apps
//! open it for a player that plays HLS, so a playlist is found and rewritten.
//! The URL's own host is in scope, as an install's is, so a loopback or LAN
//! literal plays. The second line is the whole mpv command for that URL:
//! the app's options on every instance (m3-desktop 4.3, `ytdl=no` and
//! `load-unsafe-playlists=no` included), the file's options and the
//! environment of 3.3. Leaving any of them out leaves mpv uncontained.

#[path = "mpv/options.rs"]
mod options;

use mq_http::{ClientConfig, HttpClient};
use mq_server::{StreamServer, Upstream};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let open = args.first().is_some_and(|a| a == "--open");
    if open {
        args.remove(0);
    }
    let mut args = args.into_iter();
    let url = args.next().expect("usage: serve [--open] <url> [name]");
    let name = args.next().unwrap_or_else(|| "video.mp4".into());
    let http = HttpClient::new(ClientConfig::default()).expect("http client");
    let server = StreamServer::start(http).await.expect("bind");
    let upstream = Upstream::Http {
        addon_url: url.parse().ok(),
        url,
        headers: Vec::new(),
    };
    let (lease, playlist) = if open {
        let lease = server.open(upstream, &name, Duration::from_secs(10)).await;
        let lease = lease.unwrap_or_else(|words| panic!("{words}"));
        let playlist = lease.format.is_some();
        (lease, playlist)
    } else {
        (server.register(upstream, &name), true)
    };
    println!("{}", lease.url);
    // A file plays with references off; a playlist (or a raw URL that may
    // be one) only with the core as the proxy, lavf alone and its whitelist.
    let file: &[&str] = if playlist {
        &options::MANIFEST
    } else {
        &["--access-references=no"]
    };
    let env = options::CLEARED.map(|v| format!("-u {v}")).join(" ");
    println!(
        "env {env} mpv {} {} {} '{}'",
        options::HARDENED.join(" "),
        options::proxy(server.addr()).join(" "),
        file.join(" "),
        lease.url
    );
    std::future::pending::<()>().await;
}
