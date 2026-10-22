import Foundation
import Testing

// Covers carry RPDB's ratings band (the core's Tile::rated_cover, 1a642161): the tile draws the cover and falls
// back to the addon's poster when RPDB has no image; everything that is not a cover uses the addon's poster.

private func tile(_ poster: String?, fallback: String?) throws -> Tile {
    let posterJSON = poster.map { "\"\($0)\"" } ?? "null"
    let fallbackJSON = fallback.map { "\"\($0)\"" } ?? "null"
    let json = """
    {"id": "tt1375666", "kind": "movie", "name": "Inception", "poster": \(posterJSON), "posterFallback": \(fallbackJSON),
     "background": null, "logo": null, "posterShape": "poster", "releaseInfo": "2010", "progress": null, "videoId": null,
     "rating": "8.8", "ratings": {"imdb": "8.8", "rottenTomatoes": "87%"}}
    """
    return try JSONDecoder().decode(Tile.self, from: Data(json.utf8))
}

@Test func aCoverKeepsTheAddonsPosterForEverythingElse() throws {
    let rpdb = "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt1375666.jpg?fallback=true"
    let own = "https://images.metahub.space/poster/medium/tt1375666/img"
    let cover = try tile(rpdb, fallback: own)
    #expect(cover.posterFallback == own)
    // A backdrop stand-in, the player's art and the core's play target take the addon's poster, never the band.
    #expect(cover.plainPoster == own)
    // A tile that is not RPDB's has no fallback and is its own plain poster; an older core sends no field.
    #expect(try tile(own, fallback: nil).plainPoster == own)
    let old = try JSONDecoder().decode(Tile.self, from: Data(#"{"id": "x", "kind": "movie", "name": "X", "poster": "p", "background": null, "logo": null, "posterShape": "poster", "releaseInfo": null, "progress": null, "videoId": null, "rating": null}"#.utf8))
    #expect(old.posterFallback == nil && old.plainPoster == "p")
    // VoiceOver reads the name, the year and IMDb's score.
    #expect(posterLabel([cover.name, cover.releaseInfo], cover.ratings) == "Inception, 2010, IMDb 8.8")
}

@Test func theFallbackIsAskedOnlyWhenTheCoverFails() async {
    var asked: [String] = []
    // RPDB answers 404 for an id it does not know: the addon's poster shows.
    let missing = await ArtLoader.firstLoaded(["rpdb", "own"]) { url -> String? in
        asked.append(url)
        return url == "rpdb" ? nil : url
    }
    #expect(missing == "own" && asked == ["rpdb", "own"])
    asked = []
    let found = await ArtLoader.firstLoaded(["rpdb", "own"]) { url -> String? in
        asked.append(url)
        return url
    }
    #expect(found == "rpdb" && asked == ["rpdb"], "no second request when the cover loads")
    let none = await ArtLoader.firstLoaded([nil, "own"]) { url -> String? in url }
    #expect(none == "own")
}

@Test func theArtCacheHoldsCoversOnDisk() {
    // About 120 KB a cover with a 45-day max-age: room for about 2,500, kept across launches.
    #expect(ArtLoader.cacheDisk >= 300 << 20)
    #expect(ArtLoader.cache.diskCapacity == ArtLoader.cacheDisk)
    #expect(ArtLoader.cache.memoryCapacity == ArtLoader.cacheMemory)
    // In the app's own caches folder: the Mac app runs outside a sandbox, where ~/Library/Caches is every app's.
    #expect(ArtLoader.cacheFolder.lastPathComponent == "art")
    #expect(ArtLoader.cacheFolder.deletingLastPathComponent().standardizedFileURL != URL.cachesDirectory.standardizedFileURL)
}

@Test func posterTilesDrawTheCoverWithItsFallbackAndNoChip() throws {
    // Read from the sources: the tiles are SwiftUI views over a live core.
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    func read(_ path: String) throws -> String { try String(contentsOf: app.appending(path: path), encoding: .utf8) }
    for path in ["Home/PosterTile.swift", "Common/Browse.swift"] {
        let text = try read(path)
        #expect(text.contains("ArtImage(url: tile.poster, fallbackURL: tile.posterFallback, fallback: tile.name)"), "\(path)")
        #expect(!text.contains("RatingChip"), "\(path)")
    }
    #expect(try read("Home/HeroView.swift").contains("hero?.tile.background ?? hero?.tile.plainPoster"))
    #expect(try read("Home/ContinueCard.swift").contains("tile.background ?? tile.plainPoster"))
    #expect(try read("Home/HomeScreen.swift").contains("poster: tile.plainPoster"))
    #expect(try read("Theme/Components.swift").contains("ArtLoader.firstLoaded([url, fallbackURL])"))
    #expect(try read("Theme/ArtLoader.swift").contains("config.urlCache = cache"))
}
