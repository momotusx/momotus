//! A session opened straight through librqbit, skipping `Engine::start`, in a
//! fresh process: its HTTP client still finds a TLS provider (core backlog
//! C11 moved librqbit's reqwest to the process's provider). Its own file, so
//! no earlier test in the same process installs one first.

use mq_engine::librqbit::{Session, SessionOptions};

#[tokio::test]
async fn a_session_opened_directly_builds_its_client() {
    let dir = tempfile::tempdir().unwrap();
    // No DHT, trackers, discovery or listener: nothing reaches the network.
    let opts = SessionOptions {
        dht: None,
        disable_trackers: true,
        disable_local_service_discovery: true,
        persistence: None,
        listen: None,
        ..Default::default()
    };
    let session = Session::new_with_opts(dir.path().into(), opts).await;
    assert!(session.is_ok(), "{:?}", session.err());
}
