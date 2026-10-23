# AetherEngine, vendored

Upstream: https://github.com/superuser404notfound/AetherEngine, tag 7.36.0. Licence: LGPL-3.0 with the
App Store / DRM exception (LICENSE). Why it is here: plans/apple-second-engine.md.

What is kept: `Sources/AetherEngine` and `LICENSE` as upstream ships them; `Package.swift` is trimmed to
the engine library, with its dependencies at the exact versions 7.36.0 resolved (FFmpegBuild 3.7.1,
LibDovi 2.1.0, LibTrueHD 2.3.0, LibDTSX 1.0.1).

Patches (each marked "Marquee patch" in the source):

1. `Sources/AetherEngine/Network/HLSLocalServer.swift`: the loopback HLS server binds 127.0.0.1, not
   0.0.0.0, so no other host on the network can reach a stream it serves. Upstream binds all interfaces
   so an AirPlay receiver can fetch the stream; Marquee does not offer AirPlay on this path. Upgrade
   path: bind the LAN address only while external playback is active.

To move to a newer tag: copy its `Sources/AetherEngine` and `LICENSE`, take the pins from its
`Package.resolved`, apply the patches above again, and walk the player before it lands.
