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

@Test func aSeriesWaitingForItsNextEpisodeSaysTheDay() {
    let english = Locale(identifier: "en_US")
    // The core's day, read in UTC: never the day before west of Greenwich.
    #expect(nextAirsWords("2026-10-21", locale: english) == "Next episode airs Oct 21")
    #expect(nextAirsWords("not a day", locale: english) == nil)
}

@Test func noPlayButtonSaysWhy() {
    // The core named no episode to play and no day: never a dead Play.
    #expect(noPlayWords(hasEpisodes: true) == "The next episode has no air date yet")
    #expect(noPlayWords(hasEpisodes: false) == "This series lists no episodes to play")
}

@Test func aFailedRemovalSaysWhichTitleAndWhy() {
    #expect(removeFailedWords(name: "Sintel", reason: "The core did not answer")
        == "Sintel could not be removed from Continue Watching: The core did not answer")
}

@Test func aStartThatSettlesInAHiddenPlaceOpensNothing() {
    // The viewer went to another place while the pick settled: no player opens out of sight.
    #expect(SettledStart.of(picked: true, done: true, placeShown: false) == .nothing)
    #expect(SettledStart.of(picked: false, done: false, placeShown: false) == .nothing)
    // On screen: the pick plays; no version at all goes to Add a source; still answering, Versions.
    #expect(SettledStart.of(picked: true, done: false, placeShown: true) == .play)
    #expect(SettledStart.of(picked: false, done: true, placeShown: true) == .addSource)
    #expect(SettledStart.of(picked: false, done: false, placeShown: true) == .versions)
}

@Test func aSeriesShowsLibraryAloneAMovieBothMarks() {
    // The core keeps no watched mark for a whole series (C14, C41).
    #expect(titleMarks(kind: "series", inLibrary: true, watched: true) == [.library(on: true)])
    #expect(titleMarks(kind: "movie", inLibrary: false, watched: true) == [.library(on: false), .watched(on: true)])
}

@Test func startOverIsLiveOnlyForTheTitleInFront() {
    #expect(startOverAvailable(resume: 600_000, shown: true, covered: false))
    #expect(!startOverAvailable(resume: nil, shown: true, covered: false))
    #expect(!startOverAvailable(resume: 0, shown: true, covered: false))
    // Another tab's title, or one under the player.
    #expect(!startOverAvailable(resume: 600_000, shown: false, covered: false))
    #expect(!startOverAvailable(resume: 600_000, shown: true, covered: true))
}

private func ep(_ number: Int, _ title: String, released: String? = "2022-02-18", watched: Bool = false, progress: Double? = nil, leftMs: Int64? = nil) -> Episode {
    Episode(videoId: "tt5875444:6:\(number)", season: 6, episode: number, title: title, overview: nil, thumbnail: nil,
            released: released, watched: watched, progress: progress, leftMs: leftMs)
}

@Test func anEpisodesCaptionKeepsTheTVPagesWords() {
    let en = Locale(identifier: "en_US")
    let now = Date(timeIntervalSince1970: 1_761_000_000)  // 2025-10-20
    #expect(episodeCaption(ep(1, "Bad Dates", released: "2026-03-03"), minutes: 46, now: now, locale: en) == "Airs Mar 3")
    #expect(episodeCaption(ep(2, "Dead Weight", leftMs: 31 * 60_000), minutes: 46, now: now, locale: en) == "31 min left \u{B7} Feb 18, 2022")
    // No time left from the core: the progress against the title's runtime.
    #expect(episodeCaption(ep(3, "Hard Lines", progress: 0.5), minutes: 46, now: now, locale: en) == "23 min left \u{B7} Feb 18, 2022")
    #expect(episodeCaption(ep(4, "Last Stop"), minutes: 46, now: now, locale: en) == "46 min \u{B7} Feb 18, 2022")
    // The card says Watched, as the frames draw it; the shared words do not change.
    #expect(cardCaption(ep(5, "Close Calls", watched: true), minutes: 46, now: now, locale: en) == "Watched")
    #expect(episodeCaption(ep(5, "Close Calls", watched: true), minutes: 46, now: now, locale: en) == "46 min \u{B7} Feb 18, 2022")
}

@Test func theEpisodeSearchMatchesANumberOrANameWithoutCase() {
    let season = [ep(1, "Bad Dates"), ep(2, "Dead Weight"), ep(12, "Twelve")]
    #expect(filterEpisodes(season, query: "").count == 3)
    #expect(filterEpisodes(season, query: "2").map(\.episode) == [2])
    #expect(filterEpisodes(season, query: "dead").map(\.episode) == [2])
    #expect(filterEpisodes(season, query: " DATES ").map(\.episode) == [1])
}

@Test func informationCountsTheSelectedSeasonsEpisodes() {
    let series = informationRows(genres: ["Drama", "Thriller"], runtime: "46 min", seasons: [0, 1, 2], selectedSeason: 2, episodes: 8)
    #expect(series.map(\.key) == ["Genres", "Runtime", "Seasons", "Episodes"])
    #expect(series.last?.value == "8 episodes in Season 2")
    #expect(series[2].value == "2")
    let movie = informationRows(genres: ["Drama"], runtime: "142 min", seasons: [], selectedSeason: nil, episodes: 0)
    #expect(movie.map(\.key) == ["Genres", "Runtime"])
}

@Test func theMacPlaybackMenuStartsOverTheTitleInFront() throws {
    // The FocusedValues entry the title page publishes while `startOverAvailable`, and the menu item
    // reading it with Shift-Cmd-R (plans/design-v3-phase3-apple.md 2.1). Read from the sources: the
    // menu lives in a scene.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let commands = try String(contentsOf: app.appending(path: "App/AppCommands.swift"), encoding: .utf8)
    let title = try String(contentsOf: app.appending(path: "Title/TitleScreen.swift"), encoding: .utf8)
    #expect(commands.contains("@Entry var startOver: StartOverAction?"))
    #expect(commands.range(of: #"Button\("Start Over"\) \{ startOver\?\.run\(\) \}\s+\.keyboardShortcut\("r", modifiers: \[\.command, \.shift\]\)\s+\.disabled\(startOver == nil\)"#, options: .regularExpression) != nil)
    #expect(title.range(of: #"\.focusedSceneValue\(\\\.startOver, startOverAvailable\("#, options: .regularExpression) != nil)
}
