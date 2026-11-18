//! The core's runtime surface: tasks, timers and clocks (plans/webos.md 3.1).
//!
//! Natively every item is tokio's or std's own, re-exported, so native
//! behaviour (and `start_paused` tests) is exactly what it was. On wasm the
//! same names run on `wasm_bindgen_futures::spawn_local` with timers from the
//! host (`mqHost.setTimer`, answered through the `fire` export), since
//! tokio's runtime, `std::time::Instant`, `SystemTime::now` and threads do
//! nothing or panic on `wasm32-unknown-unknown`.
//!
//! Core-lite crates take these from here, never from tokio or std directly;
//! `tests/pins.rs` holds them to it.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(all(target_arch = "wasm32", feature = "selftest"))]
mod selftest;
