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
