//! `cargo run -p mq-sync-server --example mock-oidc [PORT]`: the mock OpenID
//! provider the relay's tests use, served on loopback for a person to click
//! through (a subject picker, Cancel and Stall). Point an identity file's
//! `oidc` entry at the URL it prints (client id `marquee`, any secret).

#[path = "../tests/support/mock_oidc.rs"]
mod mock_oidc;

#[tokio::main]
async fn main() {
    let port = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(8790);
    let mock = mock_oidc::Mock::start(port).await;
    println!("{{\"event\":\"mock_oidc_ready\",\"url\":\"{}\"}}", mock.url);
    std::future::pending::<()>().await;
}
