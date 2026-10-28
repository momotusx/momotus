import Foundation
import Testing

// Trailers play inside the app (plans/trailer-inline.md): never handed to YouTube or a browser, seeks that stay in
// the trailer, and the title page's lines that keep its header trailer to the hero's rules.

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func noTitlePageHandsATrailerToAnotherApp() throws {
    let dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Title")
    let files = try FileManager.default.contentsOfDirectory(atPath: dir.path).filter { $0.hasSuffix(".swift") }
    #expect(!files.isEmpty)
    for file in files {
        let text = try source("Title/\(file)")
        #expect(!text.contains("openURL") && !text.contains("NSWorkspace.shared.open") && !text.contains("UIApplication.shared.open"),
                "\(file) opens a link")
        #expect(!text.contains("trailer.url") && !text.contains("$0.url"), "\(file) reads a trailer's watch page")
    }
}

@Test func aSeekStaysInsideTheTrailerShortOfItsClose() {
    #expect(trailerSeekTarget(30, 150, by: 10) == 40)
    #expect(trailerSeekTarget(30, 150, by: -10) == 20)
    #expect(trailerSeekTarget(4, 150, by: -10) == 0)
    #expect(trailerSeekTarget(145, 150, by: 10) == 148)
    #expect(trailerSeekTarget(0, 1, by: 10) == 0)
}

@Test func theHeaderTrailerFollowsTheHerosSignalsAndLetsGo() throws {
    let title = try source("Title/TitleScreen.swift")
    #expect(title.contains("TrailerSignals.now(visible: true, reducedMotion: reduceMotion, hasTrailer: true).allowed"))
    #expect(title.contains("live: pageVisible && heroVisible && !fullTrailer"))
    #expect(title.contains(".onChange(of: pageVisible) { _, now in if !now, !fullTrailer { trailerPlayer.release() } }"))
    #expect(title.contains(".onDisappear { trailerPlayer.release() }"))
    // One player at a time: Home's goes before the title's plays.
    #expect(title.contains("TrailerPlayer.shared.release()"))
    // Full screen has its own sound: it never writes the session's.
    let full = try source("Title/FullTrailer.swift")
    #expect(!full.contains("TrailerState.shared"))
}
