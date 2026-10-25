import Foundation
import Testing

// plans/play-every-version.md, Phase 0: the build says own or store; an own build's core carries the
// engine. A mismatch never puts developer words before the viewer: Settings, About, a log line and a
// Debug assertion tell the owner.

@Test func anOwnBuildWithoutTheEngineIsAMismatch() {
    #expect(CoreBuild.missingEngine(field: "own", hasEngine: false))
    #expect(!CoreBuild.missingEngine(field: "own", hasEngine: true))
    #expect(!CoreBuild.missingEngine(field: "store", hasEngine: false), "a store core, on purpose")
    // This test bundle links the checkout's core, which the Xcode phase holds to the engine.
    #expect(hasEngine())
}

@Test func theMismatchReachesTheOwnerNotTheViewer() throws {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    func read(_ path: String) throws -> String { try String(contentsOf: app.appending(path: path), encoding: .utf8) }
    #expect(try read("App/MarqueeApp.swift").contains("CoreBuild.check()"))
    let settings = try read("Settings/SettingsScreen.swift")
    #expect(settings.contains("if CoreBuild.missingEngine(field: CoreBuild.field, hasEngine: hasEngine()) {\n                    Text(CoreBuild.aboutLine)"))
    let check = try read("App/CoreBuild.swift")
    #expect(check.contains("coreLog.error(") && check.contains("assertionFailure(aboutLine)"))
    // The player shows no words of its own about it: the core's account state is the viewer's.
    #expect(!(try read("Player/PlayerModel.swift")).contains("CoreBuild"))
    #expect(CoreBuild.aboutLine == "This build's core has no torrent engine")
}

/// Code r3 finding 1: a Release build is a store build unless the line says MARQUEE_CORE_BUILD=own, so the launch log
/// names the build and the second engine, and a store build never passes for an engine build in a walk.
@Test func theLaunchLogNamesTheBuildAndTheEngine() throws {
    #expect(CoreBuild.launchLine(field: "own", engineOn: true) == "core build own: second engine on")
    #expect(CoreBuild.launchLine(field: "store", engineOn: false) == "core build store: second engine off")
    #expect(CoreBuild.launchLine(field: nil, engineOn: false) == "core build none: second engine off")
    let check = try body(of: "    static func check()", in: String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/App/CoreBuild.swift"), encoding: .utf8))
    #expect(check.contains("engineOn: EngineChoice.enabledHere)\n        coreLog.notice(\"\\(line, privacy: .public)\")\n        guard"), "logged before the mismatch guard, on every launch")
}
