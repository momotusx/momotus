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

    /// The switch per platform: the Mac on since the end of phase B (walked, bench AS18 to AS38; the owner,
    /// 2026-10-23), the iPhone in the last commit of plans/iphone-mkv.md after its gate, walk and played bar, tvOS
    /// off in v1 (unwalked; the owner, 2026-10-21).
    static func switchOn(_ platform: Platform) -> Bool {
        switch platform {
        case .mac: true
        case .phone: false
        case .tv: false
        }
    }

    /// Where the engine is on: its platform's switch, in own builds only. A store build, and one whose Info.plist names
    /// no build, keeps it off on every platform: the engine decodes HEVC, DTS and TrueHD in-process under copyright
    /// licences only (plans/apple-second-engine.md 6). Its frameworks stay linked there, unused; a store archive is
    /// made with `MARQUEE_CORE_BUILD=store`.
    static func enabled(on platform: Platform, build: Any? = Bundle.main.object(forInfoDictionaryKey: "MarqueeCoreBuild")) -> Bool {
        switchOn(platform) && ownBuild(build)
    }

    /// The build field says own, and nothing else does: a missing or empty field is not an own build.
    static func ownBuild(_ field: Any?) -> Bool {
        (field as? String) == "own"
    }

    /// Whether the engine may decode video in software here. The caps hide software codecs by their names, but the
    /// engine picks its software host from the stream too (interlaced or 4:2:2 H.264, MVC, anything the hardware
    /// refuses): where this is false such a session is refused (`refuses`). The Mac admits it (walked, bench AS31,
    /// AS38); the iPhone admits none in v1 (plans/iphone-mkv.md).
    static func allowsSoftwareVideo(on platform: Platform) -> Bool {
        platform == .mac
    }

    /// A session the engine sent to its software host, where the platform admits no software video.
    static func refuses(softwareHost: Bool, on platform: Platform) -> Bool {
        softwareHost && !allowsSoftwareVideo(on: platform)
    }

    /// The failure kind an engine failure reaches the core with. A refused software host is `decode`: the core then
    /// skips that version's other sources and remembers the refusal for this device, so the next play does not try
    /// it again (playback.rs); any other engine failure is `other`.
    static func failureKind(refused: Bool) -> String {
        refused ? "decode" : "other"
    }

    /// A new source while Picture in Picture is up loads in place: the engine hands the item over in its window (a
    /// stop first would close the window and end the film). Otherwise the old session stops first.
    static func stopsBeforeLoad(pictureInPictureActive: Bool) -> Bool {
        !pictureInPictureActive
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
    /// Video the engine decodes in software that a walk admitted per plan 2.5, per platform, in the core's names: on
    /// the Mac Xvid / DivX (MPEG-4 Part 2) at SD (bench AS31: an XviD AVI at 4.5% of one core). The iPhone waits for
    /// its gate row; MPEG-2 and VC-1 for theirs.
    static func engineDecoders(on platform: Platform) -> [String] {
        platform == .mac ? ["xvid"] : []
    }
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

    /// AVPlayer could not present a source (a decode or other error, or sound with no picture, which is `decode`)
    /// before its first frame: the second engine takes it once, at the same position, before the core is told. Never
    /// on a switched-off platform, never after the first frame, never from the second engine itself, and never for a
    /// source that did not answer (`network`, `timeout`): the engine would wait on it too (plan 2.3).
    static func fallsBack(enabled: Bool, from backend: Backend, firstFrameShown: Bool, kind: String) -> Bool {
        enabled && backend == .avPlayer && !firstFrameShown && ["decode", "other"].contains(kind)
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

    /// The hardware decoders, and the engine's software ones where it is on.
    static func decoders(hardware: [String], enabled: Bool, on platform: Platform = platform) -> [String] {
        enabled ? hardware + engineDecoders(on: platform) : hardware
    }

    static func audioDecoders(enabled: Bool) -> [String] {
        enabled ? avPlayerAudio + engineAudio : avPlayerAudio
    }
}
