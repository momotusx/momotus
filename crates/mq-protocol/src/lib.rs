//! Stremio addon protocol for Marquee.
//!
//! Manifest, meta and request planning come from stremio-core (MIT). Stream and
//! subtitle responses use our own wire types so protocol extensions survive.

pub mod wire;

pub use stremio_core::types::addon::{
    Descriptor, Manifest, ResourcePath, ResourceRequest, ResourceResponse,
};
pub use stremio_core::types::resource::{MetaItem, MetaItemPreview};
pub use wire::{
    ClientResolve, Source, StreamHints, StreamsResponse, SubtitlesResponse, WireError, WireStream,
    WireSubtitle, decode_streams, decode_subtitles, normalize_info_hash,
};

/// An addon's answer to a `resource` request ("catalog", "meta", ...), read
/// as stremio-core reads it (a null list is empty, an item that does not
/// decode is skipped) and as leniently as addons in the wild need: an object
/// without the list the resource asks for is an empty list (Trakt Tv answers
/// `{}` for a list it has nothing in), and keys of other resources beside the
/// asked one are ignored instead of refusing the answer. A meta answer
/// without `meta`, and a body that is not an object, read as before.
pub fn decode_response(body: &[u8], resource: &str) -> Result<ResourceResponse, serde_json::Error> {
    let root: serde_json::Value = serde_json::from_slice(body)?;
    let key = match resource {
        "catalog" => "metas",
        "meta" => "meta",
        "stream" => "streams",
        "subtitles" => "subtitles",
        "addon_catalog" => "addons",
        _ => return serde_json::from_value(root),
    };
    let serde_json::Value::Object(mut answer) = root else {
        return serde_json::from_value(root);
    };
    let value = match answer.remove(key) {
        Some(value) => value,
        // A meta has no empty form: the answer is read as stremio-core reads it.
        None if key == "meta" => return serde_json::from_value(serde_json::Value::Object(answer)),
        None => serde_json::Value::Null,
    };
    serde_json::from_value(serde_json::json!({ key: value }))
}
