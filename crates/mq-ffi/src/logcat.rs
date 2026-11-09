//! Core logs on Android go to logcat under the tag `marquee`. `MARQUEE_LOG`
//! sets the filter in the `tracing` targets syntax (`debug`,
//! `info,mq_sync=trace`); the default is `info`. The app sets it with
//! `Os.setenv` before opening the core. Errors already redact URLs and keys
//! (`mq_http::redact_url`), so nothing secret reaches the log.

use std::ffi::{CString, c_char, c_int};
use std::io::{self, Write};
use std::sync::Once;
use tracing::{Level, Metadata};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

/// Installs the logcat subscriber once per process.
pub fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let filter = std::env::var("MARQUEE_LOG")
            .ok()
            .and_then(|spec| crate::log_filter::targets(&spec))
            .unwrap_or_else(|| Targets::new().with_default(Level::INFO));
        let layer = tracing_subscriber::fmt::layer()
            .with_writer(Logcat)
            .with_ansi(false)
            .with_level(false)
            .without_time();
        // The error reports' layer beside the log's (plans/error-reports.md
        // 6.1): it keeps warn and error callsites, never a formatted line.
        let _ = tracing_subscriber::registry()
            .with(mq_runtime::reports::layer())
            .with(layer.with_filter(filter))
            .try_init();
    });
}

struct Logcat;

/// One formatted event, written to logcat when dropped.
struct Line {
    prio: c_int,
    text: Vec<u8>,
}

impl<'a> MakeWriter<'a> for Logcat {
    type Writer = Line;

    fn make_writer(&'a self) -> Line {
        Line::new(Level::INFO)
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Line {
        Line::new(*meta.level())
    }
}

impl Line {
    fn new(level: Level) -> Self {
        // android/log.h: VERBOSE 2, DEBUG 3, INFO 4, WARN 5, ERROR 6.
        let prio = match level {
            Level::TRACE => 2,
            Level::DEBUG => 3,
            Level::INFO => 4,
            Level::WARN => 5,
            Level::ERROR => 6,
        };
        Self {
            prio,
            text: Vec::new(),
        }
    }
}

impl Write for Line {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.text.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        // logcat ends the line itself, and a NUL would cut the C string.
        let text = self.text.strip_suffix(b"\n").unwrap_or(&self.text);
        let text: Vec<u8> = text
            .iter()
            .map(|&b| if b == 0 { b' ' } else { b })
            .collect();
        if let Ok(text) = CString::new(text) {
            // SAFETY: both pointers are valid NUL-terminated strings for the call.
            unsafe { __android_log_write(self.prio, c"marquee".as_ptr(), text.as_ptr()) };
        }
    }
}

#[cfg(test)]
mod tests {
    /// Runs on a device or emulator: `cargo ndk test --no-run`, push, run.
    #[test]
    fn events_reach_logcat() {
        super::init();
        tracing::warn!("logcat check {}", std::process::id());
        tracing::trace!("below the default level");
        let out = std::process::Command::new("logcat")
            .args(["-d", "-s", "marquee"])
            .output()
            .expect("logcat runs");
        let log = String::from_utf8_lossy(&out.stdout);
        assert!(log.contains(&format!("logcat check {}", std::process::id())));
        assert!(!log.contains("below the default level"));
    }
}
