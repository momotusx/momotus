import Foundation
import Testing

// The Home hero carousel's clock (plans/appletv-polish.md 4.1) and the owner's B2.

@MainActor
@Test func theDwellPagesOnAndOnePageNeverDoes() {
    let pager = HeroPagerModel(count: 3, dwell: 8)
    #expect(!pager.tick(4, running: true))
    #expect(abs(pager.progress - 0.5) < 0.001)
    #expect(pager.tick(4, running: true))
    #expect(pager.progress == 0)
    #expect(pager.next(after: 2) == 0)
    let one = HeroPagerModel(count: 1, dwell: 8)
    #expect(!one.tick(60, running: true))
}

@MainActor
@Test func aHeldClockDoesNotFillAndAPageByHandStopsIt() {
    let pager = HeroPagerModel(count: 3, dwell: 8)
    _ = pager.tick(2, running: true)
    #expect(!pager.tick(60, running: false))
    #expect(abs(pager.progress - 0.25) < 0.001)
    pager.input()
    #expect(pager.progress == 0)
    pager.manual()
    #expect(!pager.tick(60, running: true))
    #expect(pager.stopped)
}

@MainActor
@Test func keysPageWithNoWrapAndATrailerWritesTheSameValue() {
    let pager = HeroPagerModel(count: 3, dwell: 8)
    #expect(pager.step(from: 2, by: 1) == nil)
    #expect(pager.step(from: 0, by: -1) == nil)
    #expect(pager.step(from: 1, by: 1) == 2)
    pager.trailer(0.4)
    #expect(abs(pager.progress - 0.4) < 0.001)
    pager.trailer(3)
    #expect(pager.progress == 1)
}

@MainActor
@Test func theOwnersB2() {
    // design/tokens.json motion: a page stays 8 s, slides in 650 ms, its art at 0.85 of its words.
    #expect(HeroPagerModel(count: 2).dwell == 8)
    #expect(Theme.Motion.heroDwell == 8)
    #expect(Theme.Motion.heroParallax == 0.85)
}

@MainActor
@Test func theDwellNeverCutsAPlayingTrailer() {
    #expect(!HeroPagerModel.runs(visible: true, active: true, reduceMotion: false, trailerPlaying: true, pages: 3))
    #expect(HeroPagerModel.runs(visible: true, active: true, reduceMotion: false, trailerPlaying: false, pages: 3))
    // Twenty seconds of ticks while a trailer plays: no page change.
    let pager = HeroPagerModel(count: 3, dwell: 8)
    let running = HeroPagerModel.runs(visible: true, active: true, reduceMotion: false, trailerPlaying: true, pages: 3)
    for _ in 0..<600 { #expect(!pager.tick(1.0 / 30, running: running)) }
}

@MainActor
@Test func aPointerOnTheHeroHoldsTheClockWhereItIs() {
    let held = HeroPagerModel.runs(visible: true, active: true, reduceMotion: false, trailerPlaying: false, held: true, pages: 3)
    #expect(!held)
    #expect(!HeroPagerModel.runs(visible: true, active: true, reduceMotion: true, trailerPlaying: false, pages: 3))
    #expect(!HeroPagerModel.runs(visible: false, active: true, reduceMotion: false, trailerPlaying: false, pages: 3))
    let pager = HeroPagerModel(count: 3, dwell: 8)
    _ = pager.tick(6, running: true)
    // Thirty seconds under the pointer: no page, and the fill stays at 6 of 8.
    for _ in 0..<900 { #expect(!pager.tick(1.0 / 30, running: held)) }
    #expect(abs(pager.progress - 0.75) < 0.001)
    // The pointer leaves: the last 2 s run out and the page changes.
    #expect(!pager.tick(1.9, running: true))
    #expect(pager.tick(0.2, running: true))
}

@MainActor
@Test func theHeroPagesAndPlaysOnlyWhileItIsThePageOnScreen() {
    // Review r1, C1: Home is kept alive behind Discover and under the player.
    #expect(HeroPagerModel.live(visible: true, tabShown: true, covered: false))
    #expect(!HeroPagerModel.live(visible: true, tabShown: false, covered: false))
    #expect(!HeroPagerModel.live(visible: true, tabShown: true, covered: true))
    #expect(!HeroPagerModel.live(visible: false, tabShown: true, covered: false))
    let behindDiscover = HeroPagerModel.live(visible: true, tabShown: false, covered: false)
    #expect(!HeroPagerModel.runs(visible: behindDiscover, active: true, reduceMotion: false, trailerPlaying: false, pages: 3))
}

@MainActor
@Test func voiceOverOrSwitchControlStopsTheTimedPages() {
    // Review r1, W3.
    #expect(!HeroPagerModel.runs(visible: true, active: true, reduceMotion: false, trailerPlaying: false, assistive: true, pages: 3))
    #expect(HeroPagerModel.runs(visible: true, active: true, reduceMotion: false, trailerPlaying: false, assistive: false, pages: 3))
}

@MainActor
@Test func aLateEndOfTheTrailerBeforeIsDropped() {
    // Review r1, W6: the page echoes each play's generation; the stop of the one before can land after a new play.
    #expect(TrailerMessages.accepts(["event": "end", "why": "stopped", "gen": 4], generation: 4))
    #expect(!TrailerMessages.accepts(["event": "end", "why": "stopped", "gen": 3], generation: 4))
    #expect(!TrailerMessages.accepts(["event": "playing"], generation: 4))
}

@Test func aPreviewPlaysOnlyWhenEveryConditionHoldsAndLowPowerIsNoneOfThem() {
    let ok = TrailerSignals(enabled: true, capable: true, visible: true, reducedMotion: false, metered: false,
                            lowPower: false, screenReader: false, systemAutoplay: true, hasTrailer: true)
    #expect(ok.allowed)
    var saver = ok
    saver.lowPower = true
    #expect(saver.allowed)
    #expect(saver.blocks.isEmpty)
    let blocked: [WritableKeyPath<TrailerSignals, Bool>: Bool] = [
        \.enabled: false, \.capable: false, \.visible: false, \.reducedMotion: true, \.metered: true,
        \.screenReader: true, \.systemAutoplay: false, \.hasTrailer: false,
    ]
    for (key, value) in blocked {
        var signals = ok
        signals[keyPath: key] = value
        #expect(!signals.allowed)
        #expect(signals.blocks.count == 1)
    }
}
