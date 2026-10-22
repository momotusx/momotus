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

    /// Once at launch: the log line and, in Debug, the assertion.
    static func check() {
        guard missingEngine(field: field, hasEngine: hasEngine()) else { return }
        coreLog.error("core build \(field, privacy: .public): \(aboutLine, privacy: .public)")
        assertionFailure(aboutLine)
    }
}
