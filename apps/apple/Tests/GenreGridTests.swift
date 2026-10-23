import Foundation
import Testing

// Browse by genre on the iPhone's Search (plans/design-v3-genre-tiles.md).

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

@Test func threeTilesShareTheWidthAtTwoToThree() throws {
    // The page's content inside the token's phone inset: the frame's 108.7 at 390.
    #expect(abs(GenreGridRules.tile(width: 390 - 2 * Theme.Tile.inset).width - 108.67) < 0.01)
    // Every phone page sits on the token's inset: the gutter is the token, and Search reads the gutter.
    #expect(Metrics.compact.gutter == Theme.Tile.inset)
    #expect(try source("Theme/ThemeSupport.swift").contains("gutter: Theme.Tile.inset"))
    #expect(try source("Search/SearchScreen.swift").contains("private var margin: CGFloat { metrics.gutter }"))
    // The frame's 108.7 by 163 at 390 (350 inside 20 margins), then the other phones.
    for (width, side) in [(350.0, 108.67), (335.0, 103.67), (390.0, 122.0)] {
        let size = GenreGridRules.tile(width: width)
        #expect(abs(size.width - side) < 0.01 && abs(size.height - side * 1.5) < 0.02)
    }
}

@Test func theGridOffersOnlyKindsWithGenres() throws {
    let json = """
    {"kinds": [{"kind": "movie", "label": "Movies", "genres": [{"genre": "Drama", "art": "https://x/d.jpg"}, {"genre": "Anime", "art": null}]},
               {"kind": "channel", "label": "Channels", "genres": []},
               {"kind": "series", "label": "Series", "genres": [{"genre": "Crime", "art": null}]}]}
    """
    let s = try JSONDecoder().decode(GenresSnapshot.self, from: Data(json.utf8))
    #expect(GenreGridRules.kinds(s).map(\.label) == ["Movies", "Series"])
    #expect(s.kinds[0].genres[1].art == nil)
    // A kids profile: no kinds, and the section is not drawn.
    #expect(GenreGridRules.kinds(try JSONDecoder().decode(GenresSnapshot.self, from: Data(#"{"kinds": []}"#.utf8))).isEmpty)
}

@Test func theGenreGridIsWiredAsTheSpecSays() throws {
    let parts = try source("Search/SearchParts.swift")
    let screen = try source("Search/SearchScreen.swift")
    let grid = try body(of: "struct GenreGrid: View", in: parts)
    // Nothing at all when empty; the kind control only for two or more kinds; six blank tiles while loading.
    #expect(grid.contains("if kinds?.isEmpty != true {"))
    #expect(grid.contains("KindControl.for(count: kinds.count) != .none"))
    #expect(grid.contains("ForEach(0..<6, id: \\.self)"))
    // The name over the art, two lines at most, on the ramp's floor.
    let tile = try body(of: "private struct GenreTileView: View", in: parts)
    #expect(tile.contains("ArtImage(url: tile.art)") && tile.contains(".lineLimit(2)"))
    #expect(tile.contains(".init(color: .black.opacity(GenreGridRules.rampFloor), location: 38 / 90)"))
    // The iPhone only, not drawn when the core's grid failed; a tap takes the genre chip's path.
    #expect(screen.contains("if !metrics.wide, genres.failure == nil {"))
    #expect(screen.contains("links?.discover = DiscoverRequest(kind: kind, genre: genre)") && screen.contains("openTab(.discover, link: true)"))
    #expect(!parts.contains(".source"))
}
