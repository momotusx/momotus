import Foundation
import Testing

// Resume says the same fact as the episode list: the time left.

@Test func resumeSaysTheTimeLeftWhenTheRuntimeIsKnown() {
    #expect(resumeText(resumeMs: 21 * 60_000, runtime: "57 min") == "36 min left")
    #expect(resumeText(resumeMs: 70 * 60_000, runtime: "2h 9min") == "59 min left")
    // Never "0 min left": past the stated runtime it still says a minute.
    #expect(resumeText(resumeMs: 58 * 60_000, runtime: "57 min") == "1 min left")
}

@Test func resumeSaysThePositionWithoutARuntime() {
    #expect(resumeText(resumeMs: 21 * 60_000, runtime: nil) == "from 21:00")
    #expect(resumeText(resumeMs: 75 * 60_000 + 3_000, runtime: "unknown") == "from 1:15:03")
}

@Test func resumeSaysWhatTheFileHasLeftFirst() {
    // The stored duration wins over the runtime, rounded up as Continue Watching words it.
    #expect(resumeText(resumeMs: 21 * 60_000, leftMs: 60_001, runtime: "57 min") == "2 min left")
    #expect(resumeText(resumeMs: 21 * 60_000, leftMs: 60_001, runtime: nil) == "2 min left")
    #expect(resumeText(resumeMs: 21 * 60_000, leftMs: 0, runtime: "57 min") == "36 min left")
}

@Test func aCalendarDayKeepsItsDateWestOfUTC() throws {
    let english = Locale(identifier: "en_US")
    let losAngeles = try #require(TimeZone(identifier: "America/Los_Angeles"))
    // An episode that airs on the 21st reads the 21st for a UTC-8 viewer, not the 20th.
    #expect(dayHeading("2026-10-21", today: "2026-10-01", viewer: losAngeles, locale: english) == "Wednesday, October 21")
    #expect(dayHeading("2026-10-02", today: "2026-10-01", viewer: losAngeles, locale: english) == "Tomorrow")
    // The night the clocks go back in New York, tomorrow is still tomorrow.
    let newYork = try #require(TimeZone(identifier: "America/New_York"))
    #expect(dayHeading("2026-11-02", today: "2026-11-01", viewer: newYork, locale: english) == "Tomorrow")
    #expect(dayHeading("2026-11-01", today: "2026-11-01", viewer: newYork, locale: english) == "Today")
    let tokyo = try #require(TimeZone(identifier: "Asia/Tokyo"))
    #expect(dayHeading("2026-10-20", today: "2026-10-21", viewer: tokyo, locale: english) == "Yesterday")
}
