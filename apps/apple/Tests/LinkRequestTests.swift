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
