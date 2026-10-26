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

    /// The failure kind an engine failure reaches the core with. A refused software host is `decode`: the device
    /// cannot decode this picture, so the core skips that version's other sources and remembers the refusal
    /// (playback.rs). One the engine rerouted there because AVPlayer waited on bytes that did not come (`waited`, a
    /// slow first segment, bench AS41) is `other`, so a slow swarm is never remembered as a picture this device cannot
    /// show. Any other engine failure is `other`.
    static func failureKind(refused: Bool, waited: Bool = false) -> String {
        refused && !waited ? "decode" : "other"
    }

    /// Whether the failure the engine absorbed when it rerouted to its software host (`softwarePathEscalations`) was
    /// AVPlayer waiting on bytes rather than refusing the picture: CoreMedia's -12889 (no media segment in time),
    /// -12888 (a playlist that stopped growing), or no code (the engine's item death at a frozen position after its
    /// -12889 revives). Any other code, and the engine's live-join verdict, is about the media.
    static func waited(domain: String?, code: Int?) -> Bool {
        domain == "CoreMediaErrorDomain" && [nil, -12889, -12888].contains(code)
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

    /// What a play does when the phone runs hot (plans/iphone-mkv.md phase 3; IM-W3: a 4K Dolby Vision remux on the
    /// engine reached thermal serious at 13.7 min on iphone-a, while 1080p HEVC 10-bit held nominal for 30 min).
    enum HeatStep: Equatable {
        case keep
        case move(String)
        case pause
    }

    /// At thermal serious or worse on the iPhone, an engine play above 1080p (the phone's screen class) moves to the best
    /// listed version at 1080p or under, in the list's order; with none listed, it pauses. Anything else keeps playing.
    static func heatStep(hot: Bool, on platform: Platform, engine: Bool, playing: String?, rows: [(key: String, quality: String)]) -> HeatStep {
        // The core's quality words (versions.rs, `quality_word`).
        let height = { (quality: String?) in ["4K": 2160, "1440p": 1440, "1080p": 1080, "720p": 720, "SD": 480][quality ?? ""] }
        guard hot, platform == .phone, engine, let playing,
              let now = height(rows.first { $0.key == playing }?.quality), now > 1080 else { return .keep }
        let cooler = rows.first { $0.key != playing && (height($0.quality) ?? .max) <= 1080 }
        return cooler.map { .move($0.key) } ?? .pause
    }

    static var enabledHere: Bool { enabled(on: platform) && admitsThisDevice }

    /// The phone's switch covers iPhones with 6 GB or more (iphone-a's class; 8 GB phones too), not iPads and not smaller
    /// phones (plans/iphone-mkv.md; the lead, 2026-10-25). The model must read as an iPhone, so a failed read, an
    /// iPad or an iPhone build on a Mac closes it. The bound sits between 4 GB (about 4.3e9 bytes less the kernel's
    /// share) and 6 GB (about 6.4e9 less it); iphone-a's own `physicalMemory` goes in the first gate row.
    static func admits(memoryBytes: UInt64, model: String) -> Bool {
        model.hasPrefix("iPhone") && memoryBytes >= 5_000_000_000
    }

    /// This device, on the iPhone build, read once; every Mac.
    private static let admitsThisDevice: Bool = {
        #if os(iOS)
        admits(memoryBytes: ProcessInfo.processInfo.physicalMemory, model: machine)
        #else
        true
        #endif
    }()

    #if os(iOS)
    /// The hardware model (`iPhone14,2`, `iPad13,4`), read without the main actor.
    static var machine: String {
        var info = utsname()
        uname(&info)
        return withUnsafeBytes(of: info.machine) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
    }
    #endif

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
