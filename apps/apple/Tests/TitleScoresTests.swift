import AppKit
import Foundation
import SwiftUI
import Testing

// The title page's ratings row, genre chips and More like this (plans/title-page-ratings-genres-similar.md):
// IMDb as x/10, TMDB and Trakt as percentages, each after its mark and named once; the row as tall as it will be
// from the first draw, so nothing moves when TMDB and Trakt land; genres as words on a kids profile.

private func details(scores: [String: Any]?, ratedPoster: String? = nil) throws -> TitleDetails {
    var json: [String: Any] = [
        "id": "tt0111161", "kind": "movie", "name": "The Shawshank Redemption",
        "description": "Over the course of several years, two convicts form a friendship, seeking consolation and, eventually, redemption through basic compassion.",
        "poster": NSNull(), "background": NSNull(), "logo": NSNull(), "releaseInfo": "1994", "runtime": "142 min",
        "genres": ["Drama", "Crime"], "cast": ["Tim Robbins", "Morgan Freeman"], "directors": ["Frank Darabont"],
        "rating": "9.3", "ratings": ["imdb": "9.3"], "imdbUrl": "https://www.imdb.com/title/tt0111161/",
        "source": "Cinemeta", "trailers": [], "ratedPoster": ratedPoster ?? NSNull(),
    ]
    if let scores { json["scores"] = scores }
    return try JSONDecoder().decode(TitleDetails.self, from: JSONSerialization.data(withJSONObject: json))
}

private var pending: [String: Any] { ["tmdb": ["state": "pending"], "trakt": ["state": "pending"]] }
private var landed: [String: Any] { [
    "tmdb": ["state": "shown", "text": "87%", "url": "https://www.themoviedb.org/movie/278"],
    "trakt": ["state": "shown", "text": "92%", "url": "https://trakt.tv/movies/tt0111161"],
] }

@Test func theRatingsRowReadsAsTheCoverBandDoes() throws {
    let items = scoreItems(imdb: "9.3", scores: try details(scores: landed).scores)
    #expect(items.map(\.mark) == [.imdb, .tmdb, .trakt])
    #expect(items.map(\.text) == ["9.3/10", "87%", "92%"])
    #expect(items.map(\.spoken) == ["IMDb 9.3 out of 10", "TMDB 87 percent", "Trakt 92 percent"])
    // Each links to the title's page on its service, from the core's checked URLs.
    let linked = scoreItems(imdb: "9.3", scores: try details(scores: landed).scores, imdbUrl: "https://www.imdb.com/title/tt0111161/")
    #expect(linked.map(\.url?.absoluteString) == ["https://www.imdb.com/title/tt0111161/", "https://www.themoviedb.org/movie/278", "https://trakt.tv/movies/tt0111161"])
    // Another host, plain http, a script or nothing (a kids profile gets no URL): no link.
    for bad in ["https://evil.example/tt1", "http://www.imdb.com/title/tt1/", "javascript:alert(1)", "https://trakt.tv.evil.example/x", ""] {
        #expect(outLink(bad) == nil, "\(bad)")
    }
    #expect(outLink(nil) == nil)
    // Pending keeps a slot that says nothing; none and a missing IMDb score have no slot.
    let waiting = scoreItems(imdb: nil, scores: try details(scores: ["tmdb": ["state": "pending"], "trakt": ["state": "none"]]).scores)
    #expect(waiting == [ScoreItem(mark: .tmdb, text: nil, spoken: "")])
    // An older core sends no scores: IMDb alone.
    #expect(scoreItems(imdb: "9.3", scores: try details(scores: nil).scores).map(\.mark) == [.imdb])
}

@MainActor private final class Parts {
    var frames: [HeroPart: CGRect] = [:]
}

/// The Mac hero laid out at 1440 from `details`, once every part has reported and a pass changed nothing.
@MainActor private func heroParts(_ details: TitleDetails) -> [HeroPart: CGRect] {
    let parts = Parts()
    let hero = TitleHeroWide(facts: TitleHeroFacts(details), open: { _ in }, more: {}) { Color.clear.frame(width: 600, height: 40) }
        .heroFrame(height: 810, alignment: .bottomLeading) { EmptyView() }
        .heroProbe(.box)
        .environment(\.heroProbe, HeroProbeSink { part, frame in parts.frames[part] = frame })
        .environment(\.metrics, .regular)
    let host = NSHostingView(rootView: hero.frame(width: 1440).fixedSize(horizontal: false, vertical: true))
    host.frame = CGRect(x: 0, y: 0, width: 1440, height: 1200)
    var last: [HeroPart: CGRect] = [:]
    for _ in 0..<200 {
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        if parts.frames.count >= 5, parts.frames == last { break }
        last = parts.frames
    }
    return parts.frames
}

@MainActor @Test func nothingMovesWhenTmdbAndTraktLand() throws {
    let before = heroParts(try details(scores: pending))
    let after = heroParts(try details(scores: landed))
    let meta = try #require(before[.meta])
    // A linked score is a finger-sized target (44 pt) inside a row that stays 24 tall.
    for name in ["IMDb", "TMDB", "Trakt"] {
        let target = try #require(after[.link(name)], "no link for \(name)")
        #expect(target.height >= 44, "\(name) target \(target)")
    }
    // The meta line holds the 24 pt ratings row from the first draw.
    #expect(meta.height >= ScoreRow.height, "meta \(meta)")
    for part in [HeroPart.box, .meta, .chips, .synopsis, .actions] {
        let (a, b) = (try #require(before[part]), try #require(after[part]))
        #expect(abs(a.minX - b.minX) < 0.5 && abs(a.minY - b.minY) < 0.5 && abs(a.height - b.height) < 0.5, "\(part) moved: \(a) to \(b)")
    }
}

/// The chips ChipGroups draws, and those that are links (a button to press).
@MainActor private func chips(linksGenres: Bool) -> (drawn: Int, links: Int) {
    let parts = Parts()
    let host = NSHostingView(rootView: ChipGroups(genres: ["Drama", "Crime"], directors: ["Frank Darabont"], linked: linksGenres, open: { _ in })
        .frame(width: 480, alignment: .leading)
        .environment(\.heroProbe, HeroProbeSink { part, frame in parts.frames[part] = frame }))
    host.frame = CGRect(x: 0, y: 0, width: 480, height: 100)
    for _ in 0..<50 {
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
    let names = ["Drama", "Crime", "Frank Darabont"]
    let drawn = names.filter { (parts.frames[.chip($0)]?.width ?? 0) > 0 }.count
    let links = names.filter { (parts.frames[.link($0)]?.width ?? 0) > 0 }.count
    return (drawn, links)
}

@MainActor @Test func aKidsProfilesGenresAreWordsNotLinks() throws {
    // The core says so (`genresLinked`, false on a kids profile, which has no Discover); an older core sends none.
    let header = try JSONDecoder().decode(TitleHeader.self, from: Data(#"{"state": {"state": "loading"}, "details": null, "seasons": [], "selectedSeason": null, "inLibrary": false, "playTarget": null, "watched": false, "genresLinked": false}"#.utf8))
    #expect(!header.linksGenres)
    // Linked, each genre and name is a button; for a kids profile, whose Search finds only its list, the same chips
    // show with nothing to press.
    let linked = chips(linksGenres: true)
    #expect(linked.drawn == 3 && linked.links == 3, "linked: \(linked)")
    let words = chips(linksGenres: false)
    #expect(words.drawn == 3 && words.links == 0, "kids: \(words)")
}

@Test func theCoverCardFitsBesideTheWordsAndAboveTheRow() {
    // 40 + 480 + 40 + 240 + 40: from an 840 window, with a hero tall enough for the card over the row and foot.
    #expect(!WideLayout.coverFits(window: 839, hero: 810))
    #expect(WideLayout.coverFits(window: 840, hero: 810))
    #expect(!WideLayout.coverFits(window: 1440, hero: 400))
}

// The page tells the core it shows as soon as it is in front, and that it stopped under the player or Versions,
// in the background or behind another screen: a core that never hears it keeps TMDB and Trakt pending.
@Test func thePageShowsOnlyInFrontActiveAndUncovered() {
    #expect(titlePageShows(inFront: true, placed: true, active: true, covered: false))
    #expect(!titlePageShows(inFront: false, placed: true, active: true, covered: false))
    #expect(!titlePageShows(inFront: true, placed: false, active: true, covered: false))
    #expect(!titlePageShows(inFront: true, placed: true, active: false, covered: false))
    #expect(!titlePageShows(inFront: true, placed: true, active: true, covered: true))
}

// A badge links only to an https page on IMDb, TMDB or Trakt, whatever URL arrives.
@Test func aBadgeLinksOnlyToTheThreeServices() {
    #expect(outLink("https://www.themoviedb.org/movie/278") != nil)
    #expect(outLink("https://trakt.tv/movies/tt0111161") != nil)
    #expect(outLink("http://www.imdb.com/title/tt0111161/") == nil)
    #expect(outLink("https://example.com/title/tt0111161/") == nil)
    #expect(outLink("javascript:alert(1)") == nil)
    #expect(outLink(nil) == nil)
}
