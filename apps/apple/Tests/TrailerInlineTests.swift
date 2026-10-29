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

@MainActor @Test func aTrailerThatIsNoYouTubeIdEndsInAnErrorAtOnce() {
    var ended: [String] = []
    let state = TrailerState()
    TrailerPlayer().play("https://www.youtube.com/watch?v=x", into: state) { ended.append($0) }
    #expect(ended == ["error"])
    #expect(!state.loading)
}

@MainActor @Test func lettingThePlayerGoTellsWhoPlaysWhy() {
    let player = TrailerPlayer()
    let state = TrailerState()
    var ended: [String] = []
    player.play("abcdefghijk", into: state) { ended.append($0) }
    #expect(state.loading && state.buffering)
    // The page left or covered: whoever plays hears it stopped, and its state is reset.
    player.release()
    #expect(ended == ["stopped"])
    #expect(!state.loading && !state.playing)
    // The content process died: lost, so the full screen shows its line and a header does not replay.
    player.play("abcdefghijk", into: state) { ended.append($0) }
    player.release(why: "lost")
    #expect(ended == ["stopped", "lost"])
    // Nothing plays: nobody to tell.
    player.release()
    #expect(ended == ["stopped", "lost"])
}

@Test func theBarHoldsADragAndSeeksAnyOtherMoveAtOnce() {
    #expect(trailerSliderMove(42, dragging: true) == (held: 42, seek: nil))
    #expect(trailerSliderMove(42, dragging: false) == (held: nil, seek: 42))
}
