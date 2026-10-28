import Testing

// What the title page shows while it has no details.

@Test func aFailedOpenLeavesNoHeaderAndShowsTheFailureNotLoadingForever() {
    #expect(titleFallback(header: nil, failure: "Could not reach the core") == .failed("Could not reach the core"))
}

@Test func theHeadersOwnFailureWinsAndLoadingIsOnlyWhileNothingFailed() {
    #expect(titleFallback(header: .failed(reason: "No metadata"), failure: "Other") == .failed("No metadata"))
    #expect(titleFallback(header: nil, failure: nil) == .loading)
    #expect(titleFallback(header: .loading, failure: nil) == .loading)
    // A command that failed beside a header still loading is not the page's failure.
    #expect(titleFallback(header: .loading, failure: "Could not save") == .loading)
}

private func header(_ state: TitleState) -> TitleHeader {
    TitleHeader(
        state: state, details: nil, inLibrary: false, seasons: [], selectedSeason: nil, playTarget: nil, nextAirs: nil,
        watched: false, librarySettable: nil, genresLinked: nil)
}

@Test func tryAgainDropsAHeaderThatFailedSoThePageShowsLoadingAgain() {
    #expect(headerAfterRetry(header(.failed(reason: "No metadata"))) == nil)
    #expect(headerAfterRetry(nil) == nil)
    #expect(titleFallback(header: headerAfterRetry(header(.failed(reason: "No metadata"))).map(\.state), failure: nil) == .loading)
}
