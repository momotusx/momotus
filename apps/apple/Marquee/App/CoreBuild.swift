import Foundation

/// Which core the build was made with, from the build itself (Info.plist `MarqueeCoreBuild`, own or
/// store), beside what the core says it carries (`hasEngine()`). Every own build carries the torrent
/// engine (plans/play-every-version.md, Phase 0), so a mismatch is a mis-built core: the viewer sees the
/// core's quiet account state, as on a store build, and the owner gets `aboutLine` in Settings, About,
/// a log line and, in Debug, a failed assertion.
enum CoreBuild {
    static var field: String { Bundle.main.object(forInfoDictionaryKey: "MarqueeCoreBuild") as? String ?? "own" }

    static let aboutLine = "This build's core has no torrent engine"

    /// An own build whose core has no engine; a store build has none on purpose.
    static func missingEngine(field: String, hasEngine: Bool) -> Bool {
        field != "store" && !hasEngine
    }

    /// The launch line: the build's field as written (none when missing) and whether the second engine is on, so a
    /// store build (every Release build without MARQUEE_CORE_BUILD=own) never passes for an engine build in a walk.
    static func launchLine(field: Any?, engineOn: Bool) -> String {
        "core build \(field as? String ?? "none"): second engine \(engineOn ? "on" : "off")"
    }

    /// Once at launch: the build line, then the mismatch line and, in Debug, the assertion.
    static func check() {
        let line = launchLine(field: Bundle.main.object(forInfoDictionaryKey: "MarqueeCoreBuild"), engineOn: EngineChoice.enabledHere)
        coreLog.notice("\(line, privacy: .public)")
        guard missingEngine(field: field, hasEngine: hasEngine()) else { return }
        coreLog.error("core build \(field, privacy: .public): \(aboutLine, privacy: .public)")
        assertionFailure(aboutLine)
    }
}
