import Testing

// Home's rules, as Android's RouteTest has them.

private func row(_ state: RowState, _ tiles: String...) -> HomeRow {
    HomeRow(
        rowId: "r\(tiles.count)", title: "Popular", source: "Cinemeta", state: state,
        tiles: tiles.map {
            Tile(
                id: $0, kind: "movie", name: $0, poster: nil, background: nil, logo: nil, posterShape: "poster",
                releaseInfo: nil, progress: nil, videoId: nil, rating: nil)
        })
}

@Test func heroPlayPlaysAVideoOrAMovieAndNothingElse() {
    func tile(_ kind: String, videoId: String?) -> Tile {
        Tile(
            id: "tt1", kind: kind, name: "A", poster: nil, background: nil, logo: nil, posterShape: "poster",
            releaseInfo: nil, progress: nil, videoId: videoId, rating: nil)
    }
    #expect(tile("movie", videoId: nil).playVideoId == "tt1")
    #expect(tile("series", videoId: "tt1:2:3").playVideoId == "tt1:2:3")
    // A series with no video named: its page decides what plays.
    #expect(tile("series", videoId: nil).playVideoId == nil)
}

@Test func tilesKeepTheirIdentityWhenTheyMove() {
    let a = row(.ready, "a").tiles[0], b = row(.ready, "b").tiles[0]
    #expect([a, b].keyed.map(\.key) == ["movie:a", "movie:b"])
    // After a play Continue Watching reorders: the keys move with their tiles.
    #expect([b, a].keyed.map(\.key) == ["movie:b", "movie:a"])
    // A tile twice in one strip still has two keys.
    #expect([a, a].keyed.map(\.key) == ["movie:a", "movie:a#1"])
}

@Test func tvHomeEntersOnContinueWatchingThenTheHeroThenTheFirstRowWithTiles() {
    let watching = HomeRow(rowId: "continue-watching", title: "Continue Watching", source: "", state: .ready, tiles: row(.ready, "a").tiles)
    let popular = HomeRow(rowId: "popular", title: "Popular", source: "Cinemeta", state: .ready, tiles: row(.ready, "b").tiles)
    let failed = HomeRow(rowId: "broken", title: "Broken", source: "X", state: .failed(reason: "500"), tiles: [])
    #expect(HomeStart.target(rows: [watching, popular], hero: true) == .row("continue-watching"))
    #expect(HomeStart.target(rows: [popular], hero: true) == .hero)
    // No hero: the first row with tiles, past an empty or failed lane.
    #expect(HomeStart.target(rows: [failed, popular], hero: false) == .row("popular"))
    // Continue Watching that is not the first row does not lead.
    #expect(HomeStart.target(rows: [popular, watching], hero: true) == .hero)
    #expect(HomeStart.target(rows: [failed], hero: false) == nil)
}

@Test func homeOffersOneTryAgainOnlyWhenEveryLaneEndedWithNothingAndOneFailed() {
    let failed = row(.failed(reason: "offline"))
    #expect([failed, failed].allFailed)
    #expect([failed, row(.empty)].allFailed)
    #expect(![failed, row(.loading)].allFailed, "a lane still on its way")
    #expect(![failed, row(.slow)].allFailed, "a slow lane is still on its way")
    #expect(![failed, row(.ready, "a")].allFailed, "a lane with tiles")
    #expect(![row(.failed(reason: "late"), "a")].allFailed, "tiles that arrived before the failure")
    #expect(![row(.empty)].allFailed, "empty, not failed")
    #expect(![HomeRow]().allFailed)
}
