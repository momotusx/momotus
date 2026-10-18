//! Its own test binary: tracing caches whether a log line is wanted per call
//! site, process-wide, so another test's `scoped()` calls running alongside
//! could hide these lines from this test's subscriber.

use mq_http::test_dns::{self, Interface};
use mq_http::{ClientConfig, HttpClient};
use std::sync::{Arc, Mutex};

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

#[test]
fn interface_reads_log_counts_and_one_failure() {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let sink = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || Sink(sink.clone()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let interface = |addr: &str, mask: &str, point_to_point: bool| Interface {
        addr: addr.parse().unwrap(),
        mask: Some(mask.parse().unwrap()),
        loopback: false,
        point_to_point,
    };
    let list = vec![
        interface("2001:db8:1:2::abcd", "ffff:ffff:ffff:ffff::", false),
        interface("198.18.0.1", "255.255.0.0", true),
    ];
    test_dns::set_interfaces(&http, Ok(list));
    // The second read finds the same list: no new info line.
    let _ = (http.scoped(&[]), http.scoped(&[]));
    test_dns::set_interfaces(&http, Err(()));
    let _ = (http.scoped(&[]), http.scoped(&[]), http.scoped(&[]));
    let logs = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    // Counts only: two own addresses, one prefix (the TUN's is not one), one
    // point-to-point interface.
    assert!(
        logs.contains("own=2 prefixes=1 tunnels=1") && logs.contains("interface snapshot"),
        "{logs}"
    );
    assert_eq!(logs.matches("interface snapshot own=").count(), 1, "{logs}");
    assert_eq!(
        logs.matches("keeping the last snapshot").count(),
        1,
        "{logs}"
    );
    assert!(
        !logs.contains("2001:db8") && !logs.contains("198.18"),
        "{logs}"
    );
}
