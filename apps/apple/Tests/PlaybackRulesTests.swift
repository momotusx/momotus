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
