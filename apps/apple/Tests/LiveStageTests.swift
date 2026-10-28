import Foundation
import Testing

// What Library and Calendar show from their record: skeletons, the failure, or both.

@Test func aListShowsSkeletonsUntilItsFirstSnapshotOrFailure() {
    #expect(LiveStage(hasSnapshot: false, failure: nil) == .loading)
    #expect(LiveStage(hasSnapshot: true, failure: nil) == .ready)
}

@Test func aFailureBeforeAnySnapshotIsThePage() {
    let stage = LiveStage(hasSnapshot: false, failure: "Offline")
    #expect(stage == .failed("Offline"))
    #expect(stage.failure == "Offline")
}

@Test func aFailureOverAKeptSnapshotIsNotHiddenByIt() {
    let stage = LiveStage(hasSnapshot: true, failure: "Offline")
    #expect(stage == .failedKept("Offline"))
    #expect(stage.failure == "Offline")
    #expect(LiveStage.ready.failure == nil)
    #expect(LiveStage.loading.failure == nil)
}

@Test func calendarPinsAFailureOverItsKeptSnapshotWhereItIsSeen() throws {
    // The page has scrolled to today, so a line inside the list sits above the viewport. Read from the
    // sources: it is a view.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Library/CalendarScreen.swift")
    let screen = try String(contentsOf: file, encoding: .utf8)
    let pinned = try body(of: ".safeAreaInset(edge: .top, spacing: 0)", in: screen)
    #expect(pinned.contains(".failedKept"), "the failure over a kept snapshot left the pinned inset")
}
