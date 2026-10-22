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

/// The keys an empty catalog answer may carry besides nothing: caching hints.
const CACHE_KEYS: [&str; 3] = ["cacheMaxAge", "staleRevalidate", "staleError"];

/// An addon's answer to a "catalog" or "meta" request, read as stremio-core
/// reads it (a null list is empty, an item that does not decode is skipped),
/// with one deliberate departure: a catalog answer that is an object with no
/// keys but caching hints (`{}`) is an empty list, where stremio-core
/// refuses it. Node addons answer `res.json({ metas })` with `metas`
/// undefined as `{}` (Trakt Tv when its own call to Trakt fails). Any other
/// shape without `metas` (`{"error": ...}`, a list for another resource)
/// stays an error, so a row still fails and is asked again.
pub fn decode_response(body: &[u8], resource: &str) -> Result<ResourceResponse, serde_json::Error> {
    let root: serde_json::Value = serde_json::from_slice(body)?;
    let empty = resource == "catalog"
        && root
            .as_object()
            .is_some_and(|answer| answer.keys().all(|k| CACHE_KEYS.contains(&k.as_str())));
    if empty {
        return Ok(ResourceResponse::Metas { metas: Vec::new() });
    }
    serde_json::from_value(root)
}
