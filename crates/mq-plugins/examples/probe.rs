//! The size probe with the engine and the parser (Cargo.toml): every part a
//! plugin call reaches is kept by the linker.

use mq_plugins::dom::{self, Caps};
use mq_plugins::engine::{self, Call, HostReply, Info, Media};
use std::ffi::{CStr, c_char};

/// # Safety
/// `script` and `html` are NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mq_probe(script: *const c_char, html: *const c_char) -> usize {
    // SAFETY: the caller's promise above.
    let (script, html) = unsafe { (CStr::from_ptr(script), CStr::from_ptr(html)) };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<engine::HostRequest>();
    std::thread::spawn(move || {
        while let Some(request) = rx.blocking_recv() {
            let _ = request
                .reply
                .send(HostReply::Failed(format!("{:?}", request.call)));
        }
    });
    let script = script.to_string_lossy().into_owned();
    let call = Call {
        script,
        plugin_id: String::new(),
        tmdb_id: String::new(),
        media: Media::Movie,
        season: None,
        episode: None,
        info: Info::default(),
    };
    let out = engine::call(call, Default::default(), Default::default(), tx).map(|t| t.join());
    let doc = dom::parse(&html.to_string_lossy(), Caps::default(), || false);
    format!("{out:?} {:?}", doc.stopped).len() + doc.nodes.len()
}
