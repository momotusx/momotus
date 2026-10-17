//! `mq-sync-server --db PATH [--listen 127.0.0.1:8787] [--invite CODE]`

use mq_sync_server::{Config, router};
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let mut db = PathBuf::from("mq-sync.sqlite");
    let mut listen: SocketAddr = ([127, 0, 0, 1], 8787).into();
    let mut config = Config::default();
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().unwrap_or_else(|| usage(&flag));
        match flag.as_str() {
            "--db" => db = value.into(),
            "--listen" => listen = value.parse().unwrap_or_else(|_| usage(&flag)),
            "--invite" => config.invite = Some(value),
            _ => usage(&flag),
        }
    }
    let app = router(&db, config).expect("open database");
    let listener = tokio::net::TcpListener::bind(listen).await.expect("bind");
    println!("{{\"event\":\"sync_server_ready\",\"url\":\"http://{listen}\"}}");
    axum::serve(listener, app).await.expect("serve");
}

fn usage(flag: &str) -> ! {
    eprintln!(
        "bad argument {flag}; usage: mq-sync-server --db PATH [--listen ADDR] [--invite CODE]"
    );
    std::process::exit(2);
}
