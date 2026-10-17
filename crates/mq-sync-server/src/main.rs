//! `mq-sync-server --db PATH [--listen 127.0.0.1:8787] [--invite CODE] [--open]
//! [--new-generation] [--trust-proxy] [--public-host HOST]`
//!
//! `--trust-proxy`: behind a reverse proxy (needed for TLS), rate limits go
//! by the client address the proxy appends to `X-Forwarded-For`.
//!
//! `--public-host`: the host devices use in the relay's URL (for example
//! `sync.example.com`); defaults to the listen address. Devices sign it when
//! they ask for a fresh token.

use mq_sync_server::{Config, router};
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let mut db = PathBuf::from("mq-sync.sqlite");
    let mut listen: SocketAddr = ([127, 0, 0, 1], 8787).into();
    let mut config = Config::default();
    let mut open = false;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--open" => open = true,
            "--new-generation" => config.new_generation = true,
            "--trust-proxy" => config.trust_proxy = true,
            _ => {
                let value = args.next().unwrap_or_else(|| usage(&flag));
                match flag.as_str() {
                    "--db" => db = value.into(),
                    "--listen" => listen = value.parse().unwrap_or_else(|_| usage(&flag)),
                    "--invite" => config.invite = Some(value),
                    "--public-host" => config.public_host = Some(value),
                    _ => usage(&flag),
                }
            }
        }
    }
    // Reachable from other machines with no invite means anyone can sign up.
    if !listen.ip().is_loopback() && config.invite.is_none() && !open {
        eprintln!("refusing open sign-up on {listen}: pass --invite CODE, or --open to allow it");
        std::process::exit(2);
    }
    // As a URL host: an IPv6 address goes in brackets.
    let host = SocketAddr::new(listen.ip(), 0).to_string();
    let host = host.trim_end_matches(":0").to_owned();
    config.public_host.get_or_insert(host);
    let app = router(&db, config).expect("open database");
    let listener = tokio::net::TcpListener::bind(listen).await.expect("bind");
    println!("{{\"event\":\"sync_server_ready\",\"url\":\"http://{listen}\"}}");
    mq_sync_server::serve(listener, app).await.expect("serve");
}

fn usage(flag: &str) -> ! {
    eprintln!(
        "bad argument {flag}; usage: mq-sync-server --db PATH [--listen ADDR] [--invite CODE] [--open] [--new-generation] [--trust-proxy] [--public-host HOST]"
    );
    std::process::exit(2);
}
