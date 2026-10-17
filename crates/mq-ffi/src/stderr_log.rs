//! Core logs on Apple platforms go to stderr, only when `MARQUEE_LOG` is set
//! (the `tracing` targets syntax, as on Android: `mq_server=debug,info`).
//! Xcode's console shows them; a simulator run keeps them with
//! `SIMCTL_CHILD_MARQUEE_LOG=... xcrun simctl launch --stderr=<file> ...`.
//! Errors already redact URLs and keys (`mq_http::redact_url`).

use std::sync::Once;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::prelude::*;

/// Installs the stderr subscriber once per process, if asked for.
pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(filter) = std::env::var("MARQUEE_LOG")
            .ok()
            .and_then(|spec| spec.parse::<Targets>().ok())
        else {
            return;
        };
        let layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(false);
        let _ = tracing_subscriber::registry()
            .with(layer.with_filter(filter))
            .try_init();
    });
}
