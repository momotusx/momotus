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

@Test func upNextCountsMinutesPastNinetySecondsAndEpisodeCodesAreSpoken() {
    #expect(startsIn(seconds: 12) == "Starts in 12 s")
    #expect(startsIn(seconds: 90) == "Starts in 90 s")
    #expect(startsIn(seconds: 214) == "Starts in 4 min")
    #expect(spokenEpisode(season: 2, episode: 3) == "Season 2, Episode 3")
}

@Test func episodeCodesInTheCoreLinesAreSaidAsWords() {
    #expect(spokenCodes("S2:E3 \u{B7} 36 min left") == "Season 2, Episode 3 \u{B7} 36 min left")
    #expect(spokenCodes("Resume S10:E12") == "Resume Season 10, Episode 12")
    #expect(spokenCodes("Next: S1:E2") == "Next: Season 1, Episode 2")
    // Nothing to change in a line with no code, or one inside a word.
    #expect(spokenCodes("36 min left") == "36 min left")
    #expect(spokenCodes("MS1:E2") == "MS1:E2")
}

@Test func aLongSynopsisSplitsIntoWholeSentencesWithNothingLost() {
    let text = "One sentence here. Another one follows! A third asks why? " + String(repeating: "word ", count: 80) + "end."
    let blocks = readingBlocks(text, size: 50)
    // Every word arrives, in order.
    #expect(blocks.joined(separator: " ").split(separator: " ") == text.split(separator: " "))
    // Short sentences share a block up to the size; a long one goes in pieces of whole words.
    #expect(blocks.first == "One sentence here. Another one follows!")
    #expect(blocks.allSatisfy { $0.count <= 50 })
    #expect(blocks.count > 3)
    #expect(readingBlocks("").isEmpty)
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

@Test func anEpisodeStillToAirCannotBePlayed() throws {
    let now = try #require(releaseDate("2026-10-20"))
    // Its day is after today: "Airs Oct 21", not a button.
    #expect(airsLater("2026-10-21", now: now))
    // Out today or before, or with no date, it plays.
    #expect(!airsLater("2026-10-20", now: now))
    #expect(!airsLater("2026-09-01", now: now))
    #expect(!airsLater(nil, now: now))
}
