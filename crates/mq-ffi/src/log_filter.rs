//! The `MARQUEE_LOG` filter both platform loggers use. The HTTP stack's own
//! crates log every host they connect to at debug level, so they stay at
//! `warn` unless the spec names them: a debug build's log never lists the
//! addresses a playback reached (plans/c21-c23-loopback.md 3.4).

use tracing::Level;
use tracing_subscriber::filter::Targets;

/// Crates that name hosts and addresses in their debug lines.
const TRANSPORT: [&str; 5] = ["hyper", "hyper_util", "reqwest", "h2", "rustls"];

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
}
