# AetherEngine, vendored

Upstream: https://github.com/superuser404notfound/AetherEngine, tag 7.36.0. Licence: LGPL-3.0 with the
App Store / DRM exception (LICENSE). Why it is here: plans/apple-second-engine.md.

What is kept: `Sources/AetherEngine` and `LICENSE` as upstream ships them; `Package.swift` is trimmed to
the engine library, with its dependencies at the exact versions 7.36.0 resolved (FFmpegBuild 3.7.1,
LibDovi 2.1.0, LibTrueHD 2.3.0, LibDTSX 1.0.1).

Patches (each marked "Marquee patch N" in the source):

1. `Sources/AetherEngine/Network/HLSLocalServer.swift`: the loopback HLS server binds 127.0.0.1, not
   0.0.0.0, so no other host on the network can reach a stream it serves. Upstream binds all interfaces
   so an AirPlay receiver can fetch the stream; Marquee does not offer AirPlay on this path. Upgrade
   path: bind the LAN address only while external playback is active.
2. `Sources/AetherEngine/Native/NativeAVPlayerHost.swift` and `Sources/AetherEngine/Audio/AudioAVPlayerHost.swift`:
   both players the engine makes have `allowsExternalPlayback = false`, so Control Center cannot hand the
   video to a receiver patch 1 would refuse; audio routes (AirPlay speakers) are output routes and still play.
3. Every input open sets `protocol_whitelist` to an empty list. FFmpeg allows every enabled protocol when the
   whitelist is unset (`file` included in this build) and none when it is empty; a custom-reader open, which
   every Marquee source is, never consults it. So a crafted source probed as HLS cannot name a `file:` segment.
   A local-file open fails; Marquee has none. The opens, all listed (a script test fails on any other,
   `apps/apple/scripts/vendor.test.ts`):

   | Site | How |
   | --- | --- |
   | `Demuxer/Demuxer.swift`, `openLocal` (`avformat_open_input(&ctx, urlString, nil, &opts)`) | `applyDemuxerOptions` |
   | `Demuxer/Demuxer.swift`, `openWithProvider` (`avformat_open_input(&ctxPtr, nil, inputFormat, &opts)`) | `applyDemuxerOptions` |
   | `Decoder/SubtitleDecoder.swift`, sidecar over a reader (`avformat_open_input(&ctxPtr, nil, nil, &noProtocols)`) | its own empty whitelist |
   | `Decoder/SubtitleDecoder.swift`, sidecar by URL (`avformat_open_input(&ctx, urlString, nil, &noProtocols)`) | its own empty whitelist; Marquee never calls the sidecar API |

To move to a newer tag: copy its `Sources/AetherEngine` and `LICENSE`, take the pins from its
`Package.resolved`, apply the patches above again, and walk the player before it lands.
