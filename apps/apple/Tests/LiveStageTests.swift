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

@MainActor @Test func tryAgainClearsTheFailureAtOnce() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let record = LiveRecord<LibrarySnapshot>("no-such-view", core: core.client)
    record.start()
    #expect(await eventually { record.failure != nil })
    record.retry()
    // Before the reopen answers: no old words left up, so a press is seen and a repeat failure is new.
    #expect(record.failure == nil)
    #expect(record.stage == .loading)
}

@Test func theTitlesTryAgainClearsItsFailureToo() throws {
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Title/TitleModel.swift")
    let retry = try body(of: "func retry()", in: String(contentsOf: file, encoding: .utf8))
    #expect(retry.contains("failure = nil"), "the title's Try again keeps the old failure up")
}

@MainActor @Test func aBackgroundReopenKeepsTheFailureUpUntilItAnswers() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let record = LiveRecord<LibrarySnapshot>("no-such-view", core: core.client)
    record.start()
    #expect(await eventually { record.failure != nil })
    // ProfileSession reopens its view every 2 s while it fails: the root keeps saying why.
    record.reopen()
    #expect(record.failure != nil)
    #expect(record.stage != .loading)
}

@Test func profileSessionReopensInTheBackgroundNeverRetries() throws {
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Profiles/ProfileSession.swift")
    let session = try String(contentsOf: file, encoding: .utf8)
    #expect(session.contains("self.view.reopen()"))
    #expect(!session.contains("view.retry()"), "the background loop would clear the failure every 2 s")
}
