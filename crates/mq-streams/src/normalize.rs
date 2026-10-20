//! Turns a wire stream into a `ParsedStream`: structured addon hints first,
//! then facts parsed from the stream text and the file name.

use crate::model::ParsedStream;
use mq_protocol::{Source, WireStream};

pub fn normalize(
    wire: &WireStream,
    addon_id: &str,
    addon_name: &str,
    addon_rank: u16,
    index: u32,
) -> ParsedStream {
    let name = wire.name.clone().unwrap_or_default();
    let description = wire.description.clone().unwrap_or_default();
    let hints = &wire.behavior_hints;
    let filename = hints.filename.clone().or_else(|| {
        wire.client_resolve
            .as_ref()
            .and_then(|c| c.filename.clone())
    });
    let facts = mq_relparse::parse_addon_stream(&name, &description, filename.as_deref());
    let source = wire.source();
    let cached = hints
        .cached
        .or_else(|| wire.client_resolve.as_ref().and_then(|c| c.is_cached))
        .or(facts.cached);
    let service = hints
        .service
        .clone()
        .or_else(|| wire.client_resolve.as_ref().and_then(|c| c.service.clone()))
        .or(facts.service.clone());
    ParsedStream {
        key: stream_key(
            &source,
            &facts.release.title,
            hints.video_size.or(facts.size_bytes),
        ),
        addon_id: addon_id.to_owned(),
        addon_name: addon_name.to_owned(),
        addon_rank,
        index,
        source,
        size_bytes: hints.video_size.or(facts.size_bytes),
        seeders: hints.seeders.or(facts.seeders),
        cached,
        service,
        source_site: hints.indexer.clone().or(facts.source_site.clone()),
        binge_group: hints.binge_group.clone(),
        filename,
        headers: hints
            .proxy_headers
            .as_ref()
            .map(|p| {
                let mut h: Vec<_> = p.request.clone().into_iter().collect();
                h.sort();
                h
            })
            .unwrap_or_default(),
        subtitles: wire.subtitles.clone(),
        // The caller knows which install answered.
        addon_url: None,
        release: facts.release,
        raw_name: name,
        raw_description: description,
    }
}

/// What a device keeps of a stream key: `h` and 32 hex digits of its
/// SHA-256. A key can be a whole URL, and an addon's URL can hold an
/// account's API key (code review 2026-10-20 P2-5).
pub fn key_digest(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(key.as_bytes());
    let hex: String = hash[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("h{hex}")
}

/// Torrent identity is hash and file; URLs are their own identity; anything
/// else falls back to title plus size.
fn stream_key(source: &Source, title: &str, size: Option<u64>) -> String {
    match source {
        Source::Torrent {
            info_hash,
            file_idx,
            ..
        } => {
            format!(
                "bt:{info_hash}:{}",
                file_idx.map_or("auto".to_owned(), |i| i.to_string())
            )
        }
        Source::ClientResolve(c) => match (&c.info_hash, c.file_idx) {
            (Some(hash), idx) => format!(
                "bt:{}:{}",
                hash.to_ascii_lowercase(),
                idx.map_or("auto".to_owned(), |i| i.to_string())
            ),
            (None, _) => format!("rel:{}:{}", title.to_lowercase(), size.unwrap_or(0)),
        },
        Source::Http { url } => format!("url:{url}"),
        _ => format!("rel:{}:{}", title.to_lowercase(), size.unwrap_or(0)),
    }
}
