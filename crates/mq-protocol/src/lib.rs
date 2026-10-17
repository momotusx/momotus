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
