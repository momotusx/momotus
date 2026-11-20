//! Native: tokio's and std's own items.

pub use std::time::Instant as StdInstant;
pub use tokio::task::{
    AbortHandle, JoinError, JoinHandle, JoinSet, spawn, spawn_blocking, yield_now,
};
pub use tokio::time::error::Elapsed;
pub use tokio::time::{Instant, Interval, interval, sleep, sleep_until, timeout, timeout_at};

pub mod task {
    pub use tokio::task::Id;
}

/// Milliseconds since the Unix epoch by the wall clock (0 before it).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Whether a task can be spawned from here (inside a tokio runtime).
pub fn in_runtime() -> bool {
    tokio::runtime::Handle::try_current().is_ok()
}

/// Runs `f` off the caller's thread; nothing waits for it.
pub fn background(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}

/// How many CPU-bound jobs run at once to good effect: the device's CPUs
/// (1 when it cannot tell).
pub fn parallelism() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
}

#[cfg(test)]
mod tests {
    #[test]
    fn in_runtime_follows_tokio() {
        assert!(!super::in_runtime());
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(rt.block_on(async { super::in_runtime() }));
    }

    #[test]
    fn now_ms_is_the_wall_clock() {
        assert!(super::now_ms() > 1_700_000_000_000);
    }

    #[test]
    fn background_runs_off_thread() {
        let (tx, rx) = std::sync::mpsc::channel();
        super::background(move || tx.send(std::thread::current().id()).unwrap());
        assert_ne!(rx.recv().unwrap(), std::thread::current().id());
    }
}
