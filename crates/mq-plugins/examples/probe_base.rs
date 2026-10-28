//! The size probe's baseline (Cargo.toml): the probe's thread, channel and
//! tokio runtime without the engine and the parser.

use std::ffi::{CStr, c_char};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// # Safety
/// `script` and `html` are NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_probe(script: *const c_char, html: *const c_char) -> usize {
    // SAFETY: the caller's promise above.
    let (script, html) = unsafe { (CStr::from_ptr(script), CStr::from_ptr(html)) };
    let (tx, mut rx) = mpsc::unbounded_channel::<(String, oneshot::Sender<String>)>();
    std::thread::spawn(move || {
        while let Some((call, reply)) = rx.blocking_recv() {
            let _ = reply.send(format!("{call:?}"));
        }
    });
    let script = script.to_string_lossy().into_owned();
    let out = std::thread::Builder::new()
        .stack_size(4 * 1024 * 1024)
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("a runtime");
            rt.block_on(async move {
                let (reply, answer) = oneshot::channel();
                let _ = tx.send((script, reply));
                tokio::time::timeout(Duration::from_secs(1), answer).await
            })
        })
        .expect("a thread")
        .join();
    format!("{out:?}").len() + html.to_bytes().len()
}
