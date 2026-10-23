import Foundation
import Testing

// A title page's genre chip opens Discover with one select of the kind and the genre; the core
// opens the first catalog listing it (C40; plans/design-v3-phase7-apple.md 4.5).

private func snapshot(kind: String?, catalog: String?, genres: [String], kinds: [String] = ["movie", "series"], missingGenre: String? = nil) throws -> DiscoverSnapshot {
    let json: [String: Any] = [
        "missingGenre": missingGenre ?? NSNull(),
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

private func fields(_ command: [String: any Sendable]?) -> [String: String]? {
    command?.mapValues { "\($0)" }
}

@Test func aGenreChipSendsTheKindAndTheGenreWithNoCatalog() throws {
    let kinds = try snapshot(kind: "movie", catalog: "cinemeta|movie|top", genres: []).kinds
    let command = DiscoverRequest(kind: "series", genre: "Thriller").selectCommand(kinds: kinds)
    #expect(fields(command) == ["type": "select", "kind": "series", "genre": "Thriller"])
    // Discover has no such kind: no select, Discover opens as it is.
    #expect(DiscoverRequest(kind: "anime", genre: "Drama").selectCommand(kinds: kinds) == nil)
}

@Test func aGenreChipIsSentOnceWhenDiscoverHasASnapshot() throws {
    var pending: DiscoverRequest? = DiscoverRequest(kind: "series", genre: "Thriller")
    var sent: [[String: String]] = []
    let send: ([String: any Sendable]) -> Void = { sent.append(fields($0)!) }
    // No snapshot yet: the request waits.
    DiscoverRequest.follow(&pending, snapshot: nil, send: send)
    #expect(sent.isEmpty && pending != nil)
    // A first snapshot with no kinds yet: still waiting (review-apple-player P3 3).
    DiscoverRequest.follow(&pending, snapshot: try snapshot(kind: nil, catalog: nil, genres: [], kinds: []), send: send)
    // The view has no subscription yet: still waiting (a command would be dropped).
    DiscoverRequest.follow(&pending, snapshot: try snapshot(kind: "movie", catalog: "cinemeta|movie|top", genres: []), canSend: false, send: send)
    #expect(sent.isEmpty && pending != nil)
    #expect(sent.isEmpty && pending != nil)
    let first = try snapshot(kind: "movie", catalog: "cinemeta|movie|top", genres: [])
    DiscoverRequest.follow(&pending, snapshot: first, send: send)
    #expect(sent == [["type": "select", "kind": "series", "genre": "Thriller"]])
    #expect(pending == nil)
    // The core's answer, and any later snapshot, sends nothing more.
    DiscoverRequest.follow(&pending, snapshot: try snapshot(kind: "series", catalog: "cinemeta|series|top", genres: ["Thriller"]), send: send)
    #expect(sent.count == 1)
    // A kind Discover lacks: cleared, nothing sent.
    pending = DiscoverRequest(kind: "anime", genre: "Drama")
    DiscoverRequest.follow(&pending, snapshot: first, send: send)
    #expect(sent.count == 1 && pending == nil)
}

@Test func aGenreNoCatalogListsShowsOneQuietLine() throws {
    let missing = try snapshot(kind: "movie", catalog: "cinemeta|movie|top", genres: ["Drama"], missingGenre: "Anime")
    #expect(missing.missingGenre == "Anime")
    #expect(DiscoverWords.missingGenre(missing.missingGenre) == "No Anime here")
    #expect(DiscoverWords.missingGenre(try snapshot(kind: "movie", catalog: nil, genres: []).missingGenre) == nil)
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
