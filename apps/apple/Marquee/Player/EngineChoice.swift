import Foundation

/// The player engine a source plays on (plans/apple-second-engine.md 2.2).
enum Backend: String, Sendable {
    case avPlayer
    case aether
}

/// Where the second engine (AetherEngine) is on, and which sources it plays. One switch per platform turns
/// on the caps, the routing, the fallback and the `playedOn` mark together; where it is off, every source plays
/// on AVPlayer and fails as before.
enum EngineChoice {
    enum Platform: CaseIterable, Sendable {
        case mac, phone, tv
    }

    /// The switch per platform: the Mac at the end of phase B, the iPhone at the end of phase C after its walk and
    /// resource gate, tvOS off in v1 (unwalked; the owner, 2026-10-21).
    static func enabled(on platform: Platform) -> Bool {
        switch platform {
        case .mac: false
        case .phone: false
        case .tv: false
        }
    }

    static var platform: Platform {
        #if os(macOS)
        .mac
        #elseif os(tvOS)
        .tv
        #else
        .phone
        #endif
    }

    static var enabledHere: Bool { enabled(on: platform) }

    /// What AVPlayer opens: its files, and HLS playlists.
    static let avPlayerContainers = ["mp4", "m4v", "mov", "m3u8"]
    /// What the second engine adds (2.5); `wmv`, `flv` and `ogg` only once a walk plays one.
    static let engineContainers = ["mkv", "webm", "ts", "m2ts", "avi"]
    /// Audio AVPlayer decodes, in the core's names.
    static let avPlayerAudio = ["aac", "ac3", "eac3", "atmos", "flac", "mp3", "pcm"]
    /// Audio AVPlayer does not decode, in the core's names; the engine bridges it.
    static let engineAudio = ["dts", "dts_hd", "dts_hd_ma", "dts_x", "true_hd", "opus"]

    /// The engine for a source, from what the core says about the opened file: the second engine for a container
    /// AVPlayer does not open, for audio it does not decode with no other track known, and for a version that
    /// played there before (`engine`); AVPlayer for everything else, an unknown container included, so nothing
    /// that plays on AVPlayer today changes path.
    static func backend(container: String?, audio: [String], engine: String?, enabled: Bool) -> Backend {
        guard enabled else { return .avPlayer }
        if engine == Backend.aether.rawValue { return .aether }
        if let container, !avPlayerContainers.contains(container.lowercased()) { return .aether }
        // Atmos rides on TrueHD or E-AC-3: it says nothing about decoding.
        let tracks = audio.filter { $0 != "atmos" }
        if !tracks.isEmpty, tracks.allSatisfy(engineAudio.contains) { return .aether }
        return .avPlayer
    }

    /// AVPlayer could not present a source (an error, or sound with no picture) before its first frame: the second
    /// engine takes it once, at the same position, before the core is told. Never on a switched-off platform, never
    /// after the first frame, never from the second engine itself.
    static func fallsBack(enabled: Bool, from backend: Backend, firstFrameShown: Bool) -> Bool {
        enabled && backend == .avPlayer && !firstFrameShown
    }

    /// The engine's open, to its first frame, gets what the start budget has left (plans/apple-second-engine.md
    /// 2.3), at least 1 s; 90 s, the whole budget, when the core sends none (a Try again run, which the core ends
    /// itself, or an older core).
    static func openWithin(budgetEndsAtMs: Int64?, nowMs: Int64 = Int64(Date().timeIntervalSince1970 * 1000)) -> Duration {
        guard let end = budgetEndsAtMs else { return .seconds(90) }
        return .milliseconds(max(1000, end - nowMs))
    }

    /// The containers and audio the core is told this device plays.
    static func containers(enabled: Bool) -> [String] {
        enabled ? avPlayerContainers + engineContainers : avPlayerContainers
    }

    static func audioDecoders(enabled: Bool) -> [String] {
        enabled ? avPlayerAudio + engineAudio : avPlayerAudio
    }
}
