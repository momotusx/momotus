//! Core logs on Apple platforms go to stderr, only when `MARQUEE_LOG` is set
//! (the `tracing` targets syntax, as on Android: `mq_server=debug,info`).
//! Xcode's console shows them; a simulator run keeps them with
//! `SIMCTL_CHILD_MARQUEE_LOG=... xcrun simctl launch --stderr=<file> ...`.
//! Errors already redact URLs and keys (`mq_http::redact_url`).
//!
//! The error reports' layer is always installed (plans/error-reports.md
//! 6.1): it keeps warn and error callsites, never a formatted line.

use std::sync::Once;
use tracing_subscriber::prelude::*;

/// Installs the subscriber once per process: the reports' layer always,
/// the stderr log when asked for.
pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let log = std::env::var("MARQUEE_LOG")
            .ok()
            .and_then(|spec| crate::log_filter::targets(&spec))
            .map(|filter| {
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_ansi(false)
                    .with_filter(filter)
            });
        let _ = tracing_subscriber::registry()
            .with(mq_runtime::reports::layer())
            .with(log)
            .try_init();
    });
}
