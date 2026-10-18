//! Serves one HTTP(S) URL through the loopback proxy and prints the local
//! URL, for timing players against it: `cargo run -p mq-server --example
//! serve -- <url> [name]`. The URL's own host is in scope, as an install's
//! is, so a loopback or LAN literal plays.

use mq_http::{ClientConfig, HttpClient};
use mq_server::{StreamServer, Upstream};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next().expect("usage: serve <url> [name]");
    let name = args.next().unwrap_or_else(|| "video.mp4".into());
    let http = HttpClient::new(ClientConfig::default()).expect("http client");
    let server = StreamServer::start(http).await.expect("bind");
    let upstream = Upstream::Http {
        addon_url: url.parse().ok(),
        url,
        headers: Vec::new(),
    };
    println!("{}", server.register(upstream, &name));
    std::future::pending::<()>().await;
}
