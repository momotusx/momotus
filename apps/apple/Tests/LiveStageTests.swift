import Testing

// What Library and Calendar show from their record: skeletons, the failure, or both.

@Test func aListShowsSkeletonsUntilItsFirstSnapshotOrFailure() {
    #expect(LiveStage(hasSnapshot: false, failure: nil) == .loading)
    #expect(LiveStage(hasSnapshot: true, failure: nil) == .ready)
}

@Test func aFailureBeforeAnySnapshotIsThePage() {
    let stage = LiveStage(hasSnapshot: false, failure: "Offline")
    #expect(stage == .failed("Offline"))
    #expect(stage.failure == "Offline")
}

@Test func aFailureOverAKeptSnapshotIsNotHiddenByIt() {
    let stage = LiveStage(hasSnapshot: true, failure: "Offline")
    #expect(stage == .failedKept("Offline"))
    #expect(stage.failure == "Offline")
    #expect(LiveStage.ready.failure == nil)
    #expect(LiveStage.loading.failure == nil)
}
