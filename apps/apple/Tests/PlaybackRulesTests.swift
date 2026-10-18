import AVFoundation
import Foundation
import Testing

// Skip segments and Up Next timing as plans/core-protocol.md gives them, with
// the numbers TheIntroDB returns for Breaking Bad S01E01 in a 3485 s file.

private let next = #"{"videoId":"tt0903747:1:2","title":"Cat's in the Bag...","season":1,"episode":2,"thumbnail":null}"#
private let segments = #"[{"kind":"intro","startMs":228664,"endMs":246143,"source":"TheIntroDB"},{"kind":"credits","startMs":3431000,"endMs":3485000,"source":"TheIntroDB"}]"#

private func snapshot(next: String = next, nextAtMs: String = "null", segments: String = segments) throws -> PlaybackSnapshot {
    let json = """
        {"phase":"ready","url":"http://127.0.0.1:1/v","key":"k","filename":null,"sizeBytes":null,"resumeMs":null,
         "attempts":[],"subtitles":[],"subtitlesPending":false,"next":\(next),"bingeGroup":null,
         "segments":\(segments),"nextAtMs":\(nextAtMs)}
        """
    return try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(json.utf8))
}

@Test func aSegmentIsActiveFromItsStartUpToItsEnd() throws {
    let s = try snapshot()
    #expect(s.segment(at: 228_663) == nil)
    #expect(s.segment(at: 228_664)?.label == "Skip intro")
    #expect(s.segment(at: 246_142)?.kind == "intro")
    #expect(s.segment(at: 246_143) == nil)
    #expect(s.segment(at: 3_431_000)?.label == "Skip credits")
}

@Test func onlyCreditsThatReachTheEndOfTheFileLeadIntoUpNext() throws {
    let s = try snapshot()
    let credits = try #require(s.segment(at: 3_431_000))
    let intro = try #require(s.segment(at: 230_000))
    #expect(credits.endsFile(durationMs: 3_485_000))
    #expect(credits.endsFile(durationMs: 3_485_900))
    #expect(!credits.endsFile(durationMs: 3_600_000))
    #expect(!intro.endsFile(durationMs: 3_485_000))
}

@Test func upNextFollowsTheCreditsStartWhenTheCoreKnowsIt() throws {
    let matched = try snapshot(nextAtMs: "3431000")
    #expect(!matched.offersNext(positionMs: 3_430_999, durationMs: 3_485_000, ended: false, skippedAt: nil))
    #expect(matched.offersNext(positionMs: 3_431_000, durationMs: 3_485_000, ended: false, skippedAt: nil))

    // Without it, the last 20 s.
    let own = try snapshot()
    #expect(!own.offersNext(positionMs: 3_464_999, durationMs: 3_485_000, ended: false, skippedAt: nil))
    #expect(own.offersNext(positionMs: 3_465_000, durationMs: 3_485_000, ended: false, skippedAt: nil))
    #expect(own.offersNext(positionMs: 0, durationMs: 3_485_000, ended: true, skippedAt: nil))
}

@Test func skipCreditsOpensUpNextUntilTheViewerSeeksBackBeforeThem() throws {
    let s = try snapshot()
    #expect(s.offersNext(positionMs: 3_431_000, durationMs: 3_485_000, ended: false, skippedAt: 3_431_000))
    #expect(!s.offersNext(positionMs: 3_000_000, durationMs: 3_485_000, ended: false, skippedAt: 3_431_000))
}

@Test func noUpNextWithoutANextEpisodeOrForShortFiles() throws {
    let last = try snapshot(next: "null", nextAtMs: "3431000")
    #expect(!last.offersNext(positionMs: 3_480_000, durationMs: 3_485_000, ended: true, skippedAt: nil))
    let clip = try snapshot()
    #expect(!clip.offersNext(positionMs: 59_000, durationMs: 60_000, ended: true, skippedAt: nil))
}

@Test func olderCoresWithoutSegmentsStillDecode() throws {
    let json = """
        {"phase":"resolving","url":null,"key":null,"filename":null,"sizeBytes":null,"resumeMs":null,
         "attempts":[],"subtitles":[],"subtitlesPending":true,"next":null,"bingeGroup":null}
        """
    let s = try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(json.utf8))
    #expect(s.segment(at: 0) == nil)
    #expect(s.nextAtMs == nil)
}

@Test func theCalendarOpensOnTheFirstDayOnOrAfterToday() throws {
    func calendar(_ today: String, _ dates: [String]) throws -> CalendarSnapshot {
        let days = dates.map { #"{"date":"\#($0)","items":[]}"# }.joined(separator: ",")
        let json = #"{"today":"\#(today)","days":[\#(days)],"pending":0,"failed":[],"noSchedule":[],"series":1}"#
        return try JSONDecoder().decode(CalendarSnapshot.self, from: Data(json.utf8))
    }
    #expect(try calendar("2026-10-17", ["2026-10-12", "2026-10-17", "2026-10-20"]).startDay == "2026-10-17")
    #expect(try calendar("2026-10-18", ["2026-10-12", "2026-10-17", "2026-10-20"]).startDay == "2026-10-20")
    #expect(try calendar("2026-10-30", ["2026-10-12", "2026-10-17"]).startDay == "2026-10-17")
    #expect(try calendar("2026-10-30", []).startDay == nil)
}

@Test func thePhonePlayerPlacesItsPartsByTheRealWindow() {
    // A 320 pt class phone in landscape: the transport joins the actions; from 360 it stays centered.
    #expect(PlayerLayout.joinsTransport(windowHeight: 320))
    #expect(!PlayerLayout.joinsTransport(windowHeight: 360))
    #expect(!PlayerLayout.joinsTransport(windowHeight: 0), "before the window is measured")
    // Up Next waits while the controls show on a short window, and while a track menu is open.
    #expect(!PlayerLayout.showsUpNext(due: true, controlsShown: true, menuOpen: false, windowHeight: 402))
    #expect(PlayerLayout.showsUpNext(due: true, controlsShown: false, menuOpen: false, windowHeight: 402))
    #expect(PlayerLayout.showsUpNext(due: true, controlsShown: true, menuOpen: false, windowHeight: 874))
    #expect(!PlayerLayout.showsUpNext(due: true, controlsShown: false, menuOpen: true, windowHeight: 874))
    #expect(!PlayerLayout.showsUpNext(due: false, controlsShown: false, menuOpen: false, windowHeight: 874))
    // Skip and Up Next sit just over the measured controls, else near the foot.
    #expect(PlayerLayout.overlayBottom(controlsShown: true, controlsHeight: 180) == 188)
    #expect(PlayerLayout.overlayBottom(controlsShown: false, controlsHeight: 180) == 24)
}

@Test func tvSeekingGrowsWhilePressedAgainQuickly() {
    // Quick presses one way: 10, 10, 20, 30, 60, then 120 s each.
    var run = 0
    var moved = SeekRamp.step(run)
    for _ in 1..<15 {
        run = SeekRamp.run(after: run, continuing: true)
        moved += SeekRamp.step(run)
    }
    #expect(SeekRamp.step(0) == 10 && SeekRamp.step(5) == 120 && SeekRamp.step(40) == 120)
    // Twenty minutes in fifteen presses, not 120.
    #expect(moved >= 1200)
    // A pause or the other direction starts over at 10 s.
    #expect(SeekRamp.run(after: 4, continuing: false) == 0)
}

@Test func aMenuHoldsTheControlsUntilAPickATouchOrItsLimit() {
    let opened = ContinuousClock.now
    #expect(PlayerLayout.menuHolds(openedAt: opened, now: opened))
    #expect(PlayerLayout.menuHolds(openedAt: opened, now: opened + .seconds(9)))
    // Dismissed by a tap outside, which iOS does not report: the hold lets go.
    #expect(!PlayerLayout.menuHolds(openedAt: opened, now: opened + .seconds(10)))
    // A pick or a touch cleared it.
    #expect(!PlayerLayout.menuHolds(openedAt: nil, now: opened))
}

@Test func captionsKeepTheAppFaceForTheSystemOne() {
    // The default styles name the system face privately: the app's Inter stands in.
    #expect(CaptionStyle.face(named: ".AppleSystemUIFontMedium") == nil)
    #expect(CaptionStyle.face(named: nil) == nil)
    // A face the viewer picked is used as is.
    #expect(CaptionStyle.face(named: "Menlo-Regular") == "Menlo-Regular")
}

@Test func sourcesThatFailedAlikeAreSaidOnceInFirstOrder() throws {
    let gone = "The link no longer exists (404)"
    let slow = "The source did not answer in time"
    let attempts = [
        Attempt(key: "a", via: "Torrentio", error: slow),
        Attempt(key: "b", via: "Torrentio", error: gone),
        Attempt(key: "c", via: "Local", error: gone),
        Attempt(key: "d", via: "Torrentio", error: gone),
    ]
    let grouped = FailedSources.group(attempts)
    #expect(grouped.map(\.via) == ["Torrentio", "Torrentio", "Local"])
    #expect(grouped.map(\.words) == [slow, "\(gone) · 2 sources", gone])
}

@Test func aFailoverNamesTheAttemptThatWasLoaded() {
    let command = failOverCommand(PlayerFailure(kind: "decode", error: "decoder error"), attempt: 3)
    #expect(command["type"] as? String == "failOver")
    #expect(command["error"] as? String == "decoder error")
    #expect(command["kind"] as? String == "decode")
    #expect(command["attempt"] as? Int64 == 3)
    #expect(command.count == 4)
}

@Test func anUnknownAttemptIsLeftOutOfTheFailover() {
    let command = failOverCommand(PlayerFailure(kind: "network", error: "offline"), attempt: nil)
    #expect(command["attempt"] == nil)
    #expect(command["kind"] as? String == "network")
    #expect(command.count == 3)
}

@Test func playerErrorsAreSortedIntoWhatFailed() {
    let url = { (code: Int) in NSError(domain: NSURLErrorDomain, code: code) }
    #expect(playerFailure(url(NSURLErrorTimedOut)).kind == "timeout")
    #expect(playerFailure(url(NSURLErrorCannotConnectToHost)).kind == "network")
    #expect(playerFailure(url(NSURLErrorNotConnectedToInternet)).kind == "network")
    let av = { (code: AVError.Code) in NSError(domain: AVFoundationErrorDomain, code: code.rawValue) }
    #expect(playerFailure(av(.decoderNotFound)).kind == "decode")
    #expect(playerFailure(av(.fileFormatNotRecognized)).kind == "decode")
    #expect(playerFailure(av(.unknown)).kind == "other")
    let media = NSError(domain: "CoreMediaErrorDomain", code: -12939, userInfo: [NSLocalizedDescriptionKey: "gone"])
    #expect(playerFailure(media) == PlayerFailure(kind: "other", error: "gone"))
    #expect(playerFailure(nil).kind == "other")
}

@Test func aSourceThatFailsBeforeItsStartSeekKeepsTheStart() {
    #expect(stoppedPosition(pendingStart: 900, position: 0) == 900)
    #expect(stoppedPosition(pendingStart: nil, position: 912.5) == 912.5)
}

@Test func aFailoverContinuesWhereTheFailedSourceStopped() {
    #expect(sourceStart(stoppedAt: nil, resumeMs: 30_000) == 30)
    #expect(sourceStart(stoppedAt: 312.5, resumeMs: 30_000) == 312.5)
    #expect(sourceStart(stoppedAt: nil, resumeMs: nil) == 0)
}

@Test func onlyALandedStartSeekHandsOver() {
    #expect(startLanded(finished: true, sameItem: true, pending: 900) == 900)
    // An interrupted seek, or one for an item already replaced, keeps the start pending.
    #expect(startLanded(finished: false, sameItem: true, pending: 900) == nil)
    #expect(startLanded(finished: true, sameItem: false, pending: 900) == nil)
    // A viewer's seek already cleared it.
    #expect(startLanded(finished: true, sameItem: true, pending: nil) == nil)
}

@Test func aSeekInFlightHoldsUntilItLands() {
    // B seeks to its 900 s start; the viewer seeks to 1200 s; a stale tick says 0; B fails.
    var pending: Double? = 900
    var position = 0.0
    pending = 1200; position = 1200
    let stale = ticked(time: 0, pending: pending)
    if let now = stale.position { position = now }
    pending = stale.pending
    #expect(position == 1200)
    #expect(stoppedPosition(pendingStart: pending, position: position) == 1200)
    // The player reaches the target: the wait ends.
    let landed = ticked(time: 1200.2, pending: pending)
    #expect(landed.position == 1200.2)
    #expect(landed.pending == nil)
    // No seek in flight: every tick counts.
    #expect(ticked(time: 42, pending: nil).position == 42)
}

