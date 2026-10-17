//! `mq-sync-server --db PATH [--listen 127.0.0.1:8787] [--invite CODE] [--open]
//! [--new-generation]`

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
            _ => {
                let value = args.next().unwrap_or_else(|| usage(&flag));
                match flag.as_str() {
                    "--db" => db = value.into(),
                    "--listen" => listen = value.parse().unwrap_or_else(|_| usage(&flag)),
                    "--invite" => config.invite = Some(value),
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
    let app = router(&db, config).expect("open database");
    let listener = tokio::net::TcpListener::bind(listen).await.expect("bind");
    println!("{{\"event\":\"sync_server_ready\",\"url\":\"http://{listen}\"}}");
    axum::serve(listener, app).await.expect("serve");
}

fn usage(flag: &str) -> ! {
    eprintln!(
        "bad argument {flag}; usage: mq-sync-server --db PATH [--listen ADDR] [--invite CODE] [--open] [--new-generation]"
    );
    std::process::exit(2);
}
