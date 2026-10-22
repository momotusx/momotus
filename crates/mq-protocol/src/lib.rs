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

/// The keys an empty answer may carry besides nothing: caching hints.
const CACHE_KEYS: [&str; 3] = ["cacheMaxAge", "staleRevalidate", "staleError"];

/// Whether an addon's answer is an object with no keys but caching hints
/// (`{}`): Node addons send `res.json({ metas: undefined })` that way, and
/// Trakt Tv's old deployment did whenever its own call to Trakt failed.
/// Such an answer is never cached.
pub fn is_empty_answer(answer: &serde_json::Value) -> bool {
    answer
        .as_object()
        .is_some_and(|keys| keys.keys().all(|k| CACHE_KEYS.contains(&k.as_str())))
}

/// An addon's answer to a catalog request, read as stremio-core reads it (a
/// null list is empty, an item that does not decode is skipped), with one
/// deliberate departure: an empty answer (`is_empty_answer`) is an empty
/// list, where stremio-core refuses it. Any other shape without `metas`
/// (`{"error": ...}`, a list for another resource) reads as stremio-core
/// reads it, so a row still fails and is asked again.
pub fn decode_catalog(body: &[u8]) -> Result<ResourceResponse, serde_json::Error> {
    let root: serde_json::Value = serde_json::from_slice(body)?;
    if is_empty_answer(&root) {
        return Ok(ResourceResponse::Metas { metas: Vec::new() });
    }
    serde_json::from_value(root)
}
