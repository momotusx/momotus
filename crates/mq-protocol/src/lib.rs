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
/// Each item's `imdbRating` reads as its IMDb rating (`imdb_rating_link`), and its
/// text has its HTML entities decoded (`decode_meta_text`).
pub fn decode_catalog(body: &[u8]) -> Result<ResourceResponse, serde_json::Error> {
    let mut root: serde_json::Value = serde_json::from_slice(body)?;
    if is_empty_answer(&root) {
        return Ok(ResourceResponse::Metas { metas: Vec::new() });
    }
    if let Some(metas) = root.get_mut("metas").and_then(|m| m.as_array_mut()) {
        metas.iter_mut().for_each(imdb_rating_link);
        metas.iter_mut().for_each(decode_meta_text);
    }
    serde_json::from_value(root)
}

/// An addon's answer to a meta request, read as stremio-core reads it, with
/// the meta's `imdbRating` read as its IMDb rating (`imdb_rating_link`) and its
/// text with its HTML entities decoded (`decode_meta_text`).
pub fn decode_meta(body: &[u8]) -> Result<ResourceResponse, serde_json::Error> {
    let mut root: serde_json::Value = serde_json::from_slice(body)?;
    if let Some(meta) = root.get_mut("meta") {
        imdb_rating_link(meta);
        decode_meta_text(meta);
    }
    serde_json::from_value(root)
}

/// A meta's `imdbRating` ("7.5", or 7.5) as an IMDb link (`category`
/// "imdb", its name the rating), the form every reader takes the rating
/// from: stremio-core turns the field into a link only when the item has
/// no `links`, so an item with links of its own (genres) would lose it. An
/// item with an IMDb link already, or with no rating from 0 to 10, is left
/// as it is. Only an IMDb id's rating is IMDb's: under another id (Anime
/// Kitsu's `kitsu:` items carry Kitsu's own score there) the field is
/// dropped, so neither this nor stremio-core's own conversion makes it an
/// IMDb link.
fn imdb_rating_link(item: &mut serde_json::Value) {
    use serde_json::{Value, json};
    let Some(item) = item.as_object_mut() else {
        return;
    };
    let Some(id) = item.get("id").and_then(Value::as_str).map(str::to_owned) else {
        return;
    };
    if !id.starts_with("tt") {
        item.remove("imdbRating");
        return;
    }
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
    let url = format!("https://www.imdb.com/title/{id}/");
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

/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;` and numeric
/// references, decoded once (`&amp;lt;` reads `&lt;`); any other `&` stays.
pub fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let named = after.find(';').filter(|&end| end <= 8);
        match named.and_then(|end| Some((entity(&after[..end])?, end))) {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some(c)
}

/// The text an addon's item shows (name, description, genres, each link's name, each
/// video's title and overview) with its HTML entities decoded once: Cinemeta sends `O&apos;Dea`, and no app draws
/// markup, so every reader gets plain text from here (the one place item text enters the core).
fn decode_meta_text(item: &mut serde_json::Value) {
    use serde_json::Value;
    fn text(value: &mut Value) {
        if let Value::String(s) = value
            && s.contains('&')
        {
            *s = decode_entities(s);
        }
    }
    fn each(value: Option<&mut Value>, f: impl Fn(&mut Value)) {
        if let Some(Value::Array(list)) = value {
            list.iter_mut().for_each(f);
        }
    }
    let Some(item) = item.as_object_mut() else {
        return;
    };
    for key in ["name", "description"] {
        item.get_mut(key).map(text);
    }
    each(item.get_mut("genres"), text);
    each(item.get_mut("links"), |link| {
        link.get_mut("name").map(text);
    });
    each(item.get_mut("videos"), |video| {
        for key in ["title", "name", "overview"] {
            video.get_mut(key).map(text);
        }
    });
}
