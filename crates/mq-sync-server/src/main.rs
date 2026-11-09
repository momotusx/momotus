//! `mq-sync-server --db PATH [--listen 127.0.0.1:8787] [--invite CODE] [--open]
//! [--new-generation] [--trust-proxy] [--public-host HOST] [--identity FILE]
//! [--mail FILE] [--reports FILE] [--clients-per-address N] [--mail-gap-ms MS]`
//!
//! `--reports`: take error and crash reports at `POST /v1/reports` into
//! their own database (plans/error-reports.md 7). Without it the route is
//! absent; a file that does not open leaves it absent and sync running.
//!
//! `--trust-proxy`: behind a reverse proxy (needed for TLS), rate limits go
//! by the client address the proxy appends to `X-Forwarded-For`.
//!
//! `--public-host`: the host devices use in the relay's URL (for example
//! `sync.example.com`); defaults to the listen address. Devices sign it when
//! they ask for a fresh token.
//!
//! `--identity`: sign-in with Apple, Google, Facebook or OpenID providers
//! (`identity.example.json` names every field). It needs `--public-host`
//! (nonces and callbacks are bound to it) and refuses `--open`: sign-up
//! then takes a sign-in, or the invite.
//!
//! `--mail`: sign-in by an emailed code (plans/accounts-email.md), from an
//! Amazon SES credential file (`accessKeyId`, `secretAccessKey`, `region`,
//! `from`). It needs `--identity`. A file that is not there leaves email
//! sign-in off; one that does not parse refuses to start.
//!
//! `--clients-per-address N` (every per-client sign-in and mail budget times
//! N; 1) and `--mail-gap-ms` (between two codes to one mailbox; 60000) are
//! for tests that run many clients from one address; production keeps the
//! defaults.

use mq_sync_server::{Config, IdentityFile, router};
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    // Warnings and errors to stderr (journald under systemd, which adds the
    // time); stdout keeps only the one-line ready event.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::INFO)
        .init();
    let mut db = PathBuf::from("mq-sync.sqlite");
    let mut listen: SocketAddr = ([127, 0, 0, 1], 8787).into();
    let mut config = Config::default();
    let mut open = false;
    let mut identity = None;
    let mut mail = None;
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
                    "--identity" => identity = Some(PathBuf::from(value)),
                    "--mail" => mail = Some(PathBuf::from(value)),
                    "--reports" => config.reports = Some(PathBuf::from(value)),
                    // Tests only (the web e2e): never in the relay's unit.
                    "--clients-per-address" => {
                        config.clients_per_address = value.parse().unwrap_or_else(|_| usage(&flag))
                    }
                    "--mail-gap-ms" => {
                        config.mail_gap_ms = value.parse().unwrap_or_else(|_| usage(&flag))
                    }
                    _ => usage(&flag),
                }
            }
        }
    }
    if let Some(path) = identity {
        if open {
            refuse(
                "--open and --identity together: with sign-in, sign-up takes a sign-in or the invite",
            );
        }
        if config.public_host.is_none() {
            refuse("--identity needs --public-host: nonces and callbacks are bound to it");
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| refuse(&format!("cannot read {}: {e}", path.display())));
        let file: IdentityFile = serde_json::from_str(&text).unwrap_or_else(|e| {
            refuse(&format!("{} is not an identity file: {e}", path.display()))
        });
        config.identity = Some(file);
    }
    if let Some(path) = mail {
        let Some(identity) = config.identity.as_mut() else {
            refuse("--mail needs --identity: the mailed code signs in like a provider");
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                identity.email = Some(serde_json::from_str(&text).unwrap_or_else(|e| {
                    refuse(&format!(
                        "{} is not a mail credential file: {e}",
                        path.display()
                    ))
                }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!("no mail credential file: email sign-in is off");
            }
            Err(e) => refuse(&format!("cannot read {}: {e}", path.display())),
        }
    }
    // Reachable from other machines with no invite and no sign-in means
    // anyone can sign up.
    if !listen.ip().is_loopback() && config.invite.is_none() && config.identity.is_none() && !open {
        refuse(&format!(
            "refusing open sign-up on {listen}: pass --invite CODE, --identity FILE, or --open to allow it"
        ));
    }
    // As a URL host: an IPv6 address goes in brackets.
    let host = SocketAddr::new(listen.ip(), 0).to_string();
    let host = host.trim_end_matches(":0").to_owned();
    config.public_host.get_or_insert(host);
    let app = router(&db, config).unwrap_or_else(|e| refuse(&e));
    let listener = tokio::net::TcpListener::bind(listen).await.expect("bind");
    println!("{{\"event\":\"sync_server_ready\",\"url\":\"http://{listen}\"}}");
    mq_sync_server::serve(listener, app).await.expect("serve");
}

fn refuse(why: &str) -> ! {
    eprintln!("{why}");
    std::process::exit(2);
}

fn usage(flag: &str) -> ! {
    refuse(&format!(
        "bad argument {flag}; usage: mq-sync-server --db PATH [--listen ADDR] [--invite CODE] [--open] [--new-generation] [--trust-proxy] [--public-host HOST] [--identity FILE] [--mail FILE] [--reports FILE]"
    ));
}
