//! The `MARQUEE_LOG` filter both platform loggers use. The HTTP stack's own
//! crates log every host they connect to at debug level, and the torrent
//! engine's crates log every tracker, peer and DHT node, so they stay at
//! `warn` unless the spec names them: a debug build's log never lists the
//! addresses a playback reached (plans/c21-c23-loopback.md 3.4,
//! plans/c27-torrent-policy.md 5).

use tracing::Level;
use tracing_subscriber::filter::Targets;

/// Crates that name hosts and addresses in their debug lines.
const TRANSPORT: [&str; 8] = [
    "hyper",
    "hyper_util",
    "reqwest",
    "h2",
    "rustls",
    "librqbit",
    "librqbit_dht",
    "librqbit_tracker_comms",
];

/// The filter for `spec`, with the transport crates held at `warn` unless the
/// spec mentions one of them; `None` when the spec does not parse.
pub(crate) fn targets(spec: &str) -> Option<Targets> {
    let mut filter = spec.parse::<Targets>().ok()?;
    if !TRANSPORT.iter().any(|name| spec.contains(name)) {
        for name in TRANSPORT {
            filter = filter.with_target(name, Level::WARN);
        }
    }
    Some(filter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transport_stays_at_warn_unless_asked_for() {
        let debug = targets("debug").unwrap();
        assert!(debug.would_enable("mq_server::ahead", &Level::DEBUG));
        assert!(!debug.would_enable("hyper_util::client::legacy::connect::http", &Level::DEBUG));
        assert!(!debug.would_enable("reqwest::connect", &Level::DEBUG));
        assert!(debug.would_enable("hyper_util::client", &Level::WARN));
        let asked = targets("debug,hyper_util=trace").unwrap();
        assert!(asked.would_enable("hyper_util::client::legacy::connect::http", &Level::DEBUG));
    }

    /// The targets a debug build logged tracker URLs, peer and node
    /// addresses under in the C27 live check.
    #[test]
    fn the_torrent_engine_stays_at_warn_unless_asked_for() {
        let debug = targets("debug").unwrap();
        for target in [
            "librqbit_tracker_comms::tracker_comms",
            "librqbit::dht_utils",
            "librqbit_dht::dht",
        ] {
            assert!(!debug.would_enable(target, &Level::DEBUG), "{target}");
            assert!(debug.would_enable(target, &Level::WARN), "{target}");
        }
        assert!(debug.would_enable("mq_engine::reader", &Level::DEBUG));
        let asked = targets("debug,librqbit_dht=debug").unwrap();
        assert!(asked.would_enable("librqbit_dht::dht", &Level::DEBUG));
    }
}
