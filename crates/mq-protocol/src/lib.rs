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
///
/// Each item's `imdbRating` reads as its IMDb rating (`imdb_rating_link`).
pub fn decode_catalog(body: &[u8]) -> Result<ResourceResponse, serde_json::Error> {
    let mut root: serde_json::Value = serde_json::from_slice(body)?;
    if is_empty_answer(&root) {
        return Ok(ResourceResponse::Metas { metas: Vec::new() });
    }
    if let Some(metas) = root.get_mut("metas").and_then(|m| m.as_array_mut()) {
        metas.iter_mut().for_each(imdb_rating_link);
    }
    serde_json::from_value(root)
}

/// An addon's answer to a meta request, read as stremio-core reads it, with
/// the meta's `imdbRating` read as its IMDb rating (`imdb_rating_link`).
pub fn decode_meta(body: &[u8]) -> Result<ResourceResponse, serde_json::Error> {
    let mut root: serde_json::Value = serde_json::from_slice(body)?;
    if let Some(meta) = root.get_mut("meta") {
        imdb_rating_link(meta);
    }
    serde_json::from_value(root)
}

/// A meta's `imdbRating` ("7.5", or 7.5) as an IMDb link (`category`
/// "imdb", its name the rating), the form every reader takes the rating
/// from: stremio-core turns the field into a link only when the item has
/// no `links`, so an item with links of its own (genres) would lose it. An
/// item with an IMDb link already, or with no rating from 0 to 10, is left
/// as it is.
fn imdb_rating_link(item: &mut serde_json::Value) {
    use serde_json::{Value, json};
    let Some(item) = item.as_object_mut() else {
        return;
    };
    let rating = match item.get("imdbRating") {
        Some(Value::String(s)) => s.trim().to_owned(),
        Some(Value::Number(n)) => n.as_f64().map_or_else(String::new, |f| format!("{f:.1}")),
        _ => return,
    };
    if !rating
        .parse::<f64>()
        .is_ok_and(|r| (0.0..=10.0).contains(&r))
    {
        return;
    }
    let is_imdb = |l: &Value| {
        l.get("category")
            .and_then(Value::as_str)
            .is_some_and(|c| c.eq_ignore_ascii_case("imdb"))
    };
    let url = match item.get("id").and_then(Value::as_str) {
        Some(id) if id.starts_with("tt") => format!("https://www.imdb.com/title/{id}/"),
        _ => "https://www.imdb.com/".to_owned(),
    };
    let link = json!({"name": rating, "category": "imdb", "url": url});
    match item.get_mut("links") {
        Some(Value::Array(links)) if links.iter().any(is_imdb) => {}
        Some(Value::Array(links)) => links.push(link),
        None | Some(Value::Null) => {
            item.insert("links".into(), json!([link]));
        }
        Some(_) => {}
    }
}
