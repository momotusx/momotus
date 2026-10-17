//! Live tests against the real provider APIs. All are `#[ignore]`d and read
//! credentials from the environment; none is required for a normal test run.
//!
//! ```text
//! MQ_RD_TOKEN=...        Real-Debrid private API token or OAuth access token
//! MQ_TORBOX_KEY=...      TorBox API key
//! MQ_ALLDEBRID_KEY=...   AllDebrid API key
//! MQ_PREMIUMIZE_KEY=...  Premiumize API key
//! MQ_LIVE_HASH=...       (resolve test only) info hash already cached on the account
//!
//! cargo test -p mq-debrid --test live -- --ignored --nocapture
//! ```
//!
//! `user` and `check` are read-only. `live_resolve` ADDS a torrent to the
//! account of the first provider whose variable is set.

use mq_debrid::{
    AllDebrid, DeviceLogin, Premiumize, RealDebrid, ResolveRequest, Resolver, ResolverConfig,
    Store, TorBox,
};
use mq_http::{ClientConfig, HttpClient};

/// Big Buck Bunny, a long-lived public torrent.
const PUBLIC_HASH: &str = "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c";

fn http() -> HttpClient {
    HttpClient::new(ClientConfig::default()).unwrap()
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

async fn exercise(store: impl Store) {
    let user = store.user().await.expect("user");
    eprintln!(
        "{:?}: name={} premium={} until={:?}",
        store.id(),
        user.name,
        user.is_premium,
        user.premium_until
    );
    let status = store.check(&[PUBLIC_HASH.to_owned()]).await.expect("check");
    eprintln!("{:?}: check -> {status:?}", store.id());
    assert_eq!(status.len(), 1);
}

#[tokio::test]
#[ignore = "needs MQ_RD_TOKEN"]
async fn live_real_debrid() {
    let Some(token) = var("MQ_RD_TOKEN") else {
        return eprintln!("MQ_RD_TOKEN not set");
    };
    exercise(RealDebrid::new(http(), token)).await;
}

#[tokio::test]
#[ignore = "needs MQ_TORBOX_KEY"]
async fn live_torbox() {
    let Some(key) = var("MQ_TORBOX_KEY") else {
        return eprintln!("MQ_TORBOX_KEY not set");
    };
    exercise(TorBox::new(http(), key)).await;
}

#[tokio::test]
#[ignore = "needs MQ_ALLDEBRID_KEY"]
async fn live_alldebrid() {
    let Some(key) = var("MQ_ALLDEBRID_KEY") else {
        return eprintln!("MQ_ALLDEBRID_KEY not set");
    };
    exercise(AllDebrid::new(http(), key)).await;
}

#[tokio::test]
#[ignore = "needs MQ_PREMIUMIZE_KEY"]
async fn live_premiumize() {
    let Some(key) = var("MQ_PREMIUMIZE_KEY") else {
        return eprintln!("MQ_PREMIUMIZE_KEY not set");
    };
    exercise(Premiumize::new(http(), key)).await;
}

/// Starts a Real-Debrid device login and prints the code. No credentials needed.
#[tokio::test]
#[ignore = "network"]
async fn live_real_debrid_device_code() {
    let login = DeviceLogin::start(&http()).await.expect("start");
    eprintln!(
        "go to {} and enter {} (poll every {:?}, valid {:?})",
        login.verification_url, login.user_code, login.interval, login.expires_in
    );
    assert!(!login.user_code.is_empty());
    assert!(matches!(
        login.poll().await.expect("poll"),
        mq_debrid::LoginPoll::Pending
    ));
}

#[tokio::test]
#[ignore = "adds a torrent to the account; needs MQ_LIVE_HASH and a provider variable"]
async fn live_resolve() {
    let Some(hash) = var("MQ_LIVE_HASH") else {
        return eprintln!("MQ_LIVE_HASH not set");
    };
    let mut resolver = Resolver::new(ResolverConfig::default());
    if let Some(token) = var("MQ_RD_TOKEN") {
        resolver =
            resolver.add_store(move |p| RealDebrid::new(http(), token.clone()).with_priority(p));
    } else if let Some(key) = var("MQ_TORBOX_KEY") {
        resolver = resolver.add_store(move |p| TorBox::new(http(), key.clone()).with_priority(p));
    } else if let Some(key) = var("MQ_ALLDEBRID_KEY") {
        resolver =
            resolver.add_store(move |p| AllDebrid::new(http(), key.clone()).with_priority(p));
    } else if let Some(key) = var("MQ_PREMIUMIZE_KEY") {
        resolver =
            resolver.add_store(move |p| Premiumize::new(http(), key.clone()).with_priority(p));
    } else {
        return eprintln!("no provider variable set");
    }
    let req = ResolveRequest::new(hash);
    let link = resolver.resolve(req).await.expect("resolve");
    let host = url_host(&link.url);
    eprintln!(
        "resolved {} ({:?} bytes) on {host}",
        link.filename, link.size
    );
    assert!(link.url.starts_with("https://"));
}

fn url_host(url: &str) -> String {
    url.split('/').nth(2).unwrap_or("?").to_owned()
}
