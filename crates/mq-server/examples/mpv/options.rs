//! mpv's options for the core's playbacks (plans/m3-desktop.md 4.3 and
//! plans/c21-c23-loopback.md 3.3), for the examples that print or use them.

use std::net::SocketAddr;

/// On every instance (4.3), but `access-references`, which each file sets.
/// `ytdl=no` and `load-unsafe-playlists=no` are containment's layer 0.
pub const HARDENED: [&str; 13] = [
    "--no-config",
    "--ytdl=no",
    "--load-scripts=no",
    "--osc=no",
    "--input-default-bindings=no",
    "--input-vo-keyboard=no",
    "--load-stats-overlay=no",
    "--load-auto-profiles=no",
    "--ordered-chapters=no",
    "--sub-auto=no",
    "--audio-file-auto=no",
    "--cover-art-auto=no",
    "--load-unsafe-playlists=no",
];

/// A manifest playback's per-file options (3.3): references on, only lavf
/// (no fallback to mpv's own playlist parser), and lavf's whitelist.
pub const MANIFEST: [&str; 3] = [
    "--access-references=yes",
    "--demuxer=lavf",
    "--demuxer-lavf-o=protocol_whitelist=[http,tcp,crypto]",
];

/// The variables the app removes from its environment before any thread
/// starts (3.3): FFmpeg connects directly to a host they name.
pub const CLEARED: [&str; 2] = ["no_proxy", "NO_PROXY"];

/// Every instance's proxy options: the core, and lavf's options passed down.
pub fn proxy(core: SocketAddr) -> [String; 2] {
    [
        format!("--http-proxy=http://{core}"),
        "--demuxer-lavf-propagate-opts=yes".to_owned(),
    ]
}
