import Foundation
import Testing

// A title page's genre chip opens Discover on the title's kind, then the genre once that kind's
// snapshot lists it (plans/design-v3-phase3-apple.md 4.2).

private func snapshot(kind: String?, catalog: String?, genres: [String], kinds: [String] = ["movie", "series"]) throws -> DiscoverSnapshot {
    let json: [String: Any] = [
        "kinds": kinds.map { [$0, $0.capitalized] },
        "kind": kind ?? NSNull(),
        "catalogs": catalog.map { [["id": $0, "name": "Popular", "source": "Cinemeta"]] } ?? [],
        "catalog": catalog ?? NSNull(),
        "genres": genres,
        "genre": NSNull(),
        "genreRequired": false,
        "items": [],
        "state": ["state": "ready"],
    ]
    return try JSONDecoder().decode(DiscoverSnapshot.self, from: JSONSerialization.data(withJSONObject: json))
}

@Test func aGenreChipWaitsForItsKindThenTakesTheGenreAsTheCatalogSpellsIt() throws {
    let request = DiscoverRequest(kind: "series", genre: "thriller")
    // Before the switch: the movie snapshot, even one listing the genre, only waits.
    #expect(request.step(try snapshot(kind: "movie", catalog: "cinemeta|movie|top", genres: ["Thriller"])) == .wait)
    // The kind arrived but its catalog has not loaded.
    #expect(request.step(try snapshot(kind: "series", catalog: nil, genres: [])) == .wait)
    #expect(request.step(try snapshot(kind: "series", catalog: "cinemeta|series|top", genres: ["Drama", "Thriller"])) == .select("Thriller"))
    #expect(request.step(nil) == .wait)
}

@Test func aGenreTheCatalogDoesNotListLeavesTheKindWithNoGenre() throws {
    let request = DiscoverRequest(kind: "series", genre: "Reality-TV")
    #expect(request.step(try snapshot(kind: "series", catalog: "cinemeta|series|top", genres: ["Drama"])) == .none)
    // Discover has no such kind at all: nothing to wait for.
    #expect(DiscoverRequest(kind: "anime", genre: "Drama").step(try snapshot(kind: "movie", catalog: "cinemeta|movie|top", genres: ["Drama"])) == .none)
}

// A chip on a title opened from Search or Discover: the place is already on screen with the title
// pushed over it, and only its first page reads the request (review P1-1).
@Test func aChipOnATitleOpenedFromSearchOrDiscoverOpensThatPlaceOnItsFirstPage() {
    #expect(AppTab.search.opensAtRoot(from: .search, link: true))
    #expect(AppTab.discover.opensAtRoot(from: .discover, link: true))
    // From another place too: a title pushed there earlier would hide the answer.
    #expect(AppTab.search.opensAtRoot(from: .home, link: true))
    // A plain tab press on the Mac keeps the place where it was.
    #expect(!AppTab.search.opensAtRoot(from: .search, link: false))
    #expect(!AppTab.library.opensAtRoot(from: .home, link: false))
}

// A Calendar link names its episode: two routes to one title differ by it, so the stack pushes
// the second one (plan 5.1).
@Test func aTitleRouteNamingAnEpisodeIsAnotherRoute() {
    let plain = TitleRoute(kind: "series", id: "tt0903747", name: "Breaking Bad")
    #expect(plain != TitleRoute(kind: "series", id: "tt0903747", name: "Breaking Bad", season: 2, videoId: "tt0903747:2:3"))
    #expect(TitleRoute(kind: "series", id: "tt0903747", name: "", season: 2, videoId: "tt0903747:2:3")
        != TitleRoute(kind: "series", id: "tt0903747", name: "", season: 2, videoId: "tt0903747:2:4"))
    #expect(plain == TitleRoute(kind: "series", id: "tt0903747", name: "Breaking Bad"))
}
