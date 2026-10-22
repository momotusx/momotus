import Foundation
import Testing

// The item's legible text timed by the app (plans/apple-file-subtitle-timing.md, decision A), with the delivery
// orders the spikes measured (bench/player/subtitle-sync.jsonl AFS1 to AFS4).

private func log(_ deliveries: [(Double, String)]) -> LegibleLog {
    var log = LegibleLog()
    for (time, text) in deliveries { log.deliver(text.isEmpty ? [] : [text], at: time) }
    return log
}

@Test func whatShowsIsTheLastDeliveryAtOrBeforeTheTime() {
    // A cue at its start, an empty array at its end, delivered ahead (AFS1).
    let file = log([(0, "line 0"), (1.5, ""), (2, "line 1"), (3.5, ""), (4, "line 2")])
    #expect(file.text(at: 0.5) == "line 0")
    #expect(file.text(at: 1.7) == nil)
    #expect(file.text(at: 9) == "line 2")
    // A timing either way reads the same log: 1 s earlier shows line 1 at 1.2 s, 1 s later line 0 at 2.2 s.
    #expect(file.text(at: PlayerLayout.cueTime(position: 1.2, delay: -1)) == "line 1")
    #expect(file.text(at: PlayerLayout.cueTime(position: 2.2, delay: 1)) == "line 0")
}

@Test func twoCuesAtOneTimeShowTogetherAndARedeliveryAddsNothing() {
    // mov_text gives two deliveries at 20.00 (AFS4); a seek back delivers them again.
    var file = log([(20, "line at 20"), (20, "four second line"), (22, "line at 22")])
    #expect(file.text(at: 21) == "line at 20\nfour second line")
    file.deliver(["line at 20"], at: 20)
    file.deliver(["four second line"], at: 20)
    #expect(file.text(at: 21) == "line at 20\nfour second line")
}

@Test func aTwoLineCueRedeliveredShowsOnce() {
    // A two-line cue is one string with its line break (AFS6); a redelivery after a seek must not draw it twice.
    var file = LegibleLog()
    file.deliver(["Where are you going?\nHome."], at: 30)
    file.deliver([], at: 32)
    file.seek()
    file.deliver(["Where are you going?\nHome."], at: 30)
    #expect(file.text(at: 31) == "Where are you going?\nHome.")
    // A cue that repeats one of its own lines keeps both.
    file.deliver(["- Yes\n- Yes"], at: 40)
    file.deliver(["- Yes\n- Yes"], at: 40)
    #expect(file.text(at: 40.5) == "- Yes\n- Yes")
}

@Test func anEmptyDeliveryNeverErasesACueAtItsOwnTime() {
    // Abutting cues: the last one's end and the next one's start share a time, in either order (AFS6 saw one delivery).
    var file = log([(10, "first")])
    file.deliver(["second"], at: 12)
    file.deliver([], at: 12)
    #expect(file.text(at: 12.5) == "second")
    var other = log([(10, "first")])
    other.deliver([], at: 12)
    other.deliver(["second"], at: 12)
    #expect(other.text(at: 12.5) == "second")
    #expect(other.text(at: 11) == "first")
}

@Test func aForwardSeekWithALaterTimingShowsNoStaleLine() {
    // At +2.0 s while playing at 100 s the log ends with a cue whose end was never delivered (the frontier, 60 s ahead).
    var file = log([(150, "old"), (151.5, ""), (159.5, "frontier")])
    file.seek()
    #expect(file.text(at: PlayerLayout.cueTime(position: 1001, delay: 2)) == nil)
    #expect(file.text(at: PlayerLayout.cueTime(position: 1001, delay: 60)) == nil)
    // Facts before the frontier stay.
    #expect(file.text(at: 150.5) == "old")
}

@Test func aSeekIntoACueShowsItOnceTheOutputSaysSoAsMP4AndHLSDo() {
    // An open cue (its end not delivered yet) must not show through a jump.
    var file = log([(10, "long A")])
    file.seek()
    #expect(file.text(at: 30.2) == nil, "nothing from before the seek")
    // MP4 redelivers the cue at the landing (AFS4).
    file.deliver(["in progress"], at: 30)
    #expect(file.text(at: 30.2) == "in progress")
    // HLS redelivers it at its own start, before the landing.
    var hls = log([(10, "long A")])
    hls.seek()
    hls.deliver(["line 16"], at: 30.6)
    hls.deliver([], at: 32.1)
    #expect(hls.text(at: 31.2) == "line 16")
    // A delivery after the landing first leaves the landing empty (no cue was on there).
    var none = log([(10, "long A")])
    none.seek()
    none.deliver(["next"], at: 42)
    #expect(none.text(at: 40.5) == nil)
    #expect(none.text(at: 42.5) == "next")
    none.reset()
    #expect(none.text(at: 42.5) == nil)
}

@Test func edgesAreFiniteShiftedAndInAWindowAhead() {
    let file = log([(1, "a"), (2, ""), (50, "b"), (51, ""), (200, "c")])
    #expect(file.edges(delay: 0.5, from: 0, ahead: 70) == [1.5, 2.5, 50.5, 51.5])
    #expect(file.edges(delay: -2, from: 0, ahead: 70) == [0, 48, 49])
    #expect(PlayerLayout.cueEdges([WebVTT.Cue(start: 1, end: .infinity, text: "x")], delay: 0) == [1])
}

@MainActor @Test func aFileTrackKeyIsForgottenOnAnotherVersion() {
    let outbox = DelayOutbox(clock: StepperClock(now: { .zero }, sleep: { _ in await Task.yield() })) { _ in true }
    outbox.set(url: "file:0", seconds: 1.5, sent: false)
    outbox.set(url: "http://a/1.vtt", seconds: 0.4)
    outbox.forget(prefix: PlayerLayout.fileTrackPrefix)
    #expect(outbox.kept["file:0"] == nil)
    #expect(outbox.kept["http://a/1.vtt"] == 0.4)
}

@MainActor @Test func aFileTracksValueIsKeptForThePlayAndNeverSent() async {
    var sent = 0
    var said: [Double] = []
    let outbox = DelayOutbox(clock: StepperClock(now: { .zero }, sleep: { _ in await Task.yield() })) { _ in sent += 1; return true }
    outbox.settled = { said.append($0) }
    outbox.set(url: "file:0", seconds: 1.5, sent: false)
    for _ in 0..<50 { await Task.yield() }
    outbox.flush()
    #expect(sent == 0)
    #expect(outbox.kept["file:0"] == 1.5)
    #expect(said == [1.5], "the settled value is still said")
}

@Test func onlyCaptionsInTheVideoTakeNoTiming() {
    #expect(PlayerLayout.fileTrackTimeable(mediaType: "sbtl"))
    #expect(PlayerLayout.fileTrackTimeable(mediaType: "text"))
    #expect(!PlayerLayout.fileTrackTimeable(mediaType: "clcp"))
    #expect(PlayerLayout.fileTrackKey(2) == "file:2")
}

@Test func aFileTrackGetsThePictureInPictureNote() {
    #expect(PlayerLayout.pipSubtitleNote(addonSubtitle: false, fileSubtitle: true, told: false)
        == "Subtitles from the file do not show in Picture in Picture")
    #expect(PlayerLayout.pipSubtitleNote(addonSubtitle: false, fileSubtitle: true, told: true) == nil)
    #expect(PlayerLayout.pipSubtitleNote(addonSubtitle: false, fileSubtitle: false, told: false) == nil)
}

@Test func theFileTimingRulesHold() {
    // The file's timing applies with no addon track on (a forced line beside an addon track keeps its time) and to a
    // timeable track only.
    #expect(PlayerLayout.fileDelay(addonOn: false, timeable: true, delay: 1.5) == 1.5)
    #expect(PlayerLayout.fileDelay(addonOn: true, timeable: true, delay: 1.5) == 0)
    #expect(PlayerLayout.fileDelay(addonOn: false, timeable: false, delay: 1.5) == 0)
    // A track starts where it was left this play, else none: another version or source never inherits a timing.
    #expect(PlayerLayout.startFileDelay(textTrack: 1, kept: ["file:1": 1.5, "file:0": -0.4]) == 1.5)
    #expect(PlayerLayout.startFileDelay(textTrack: 2, kept: ["file:1": 1.5]) == 0)
    #expect(PlayerLayout.startFileDelay(textTrack: nil, kept: ["file:1": 1.5]) == 0)
    // The overlay: the file's line, then the addon track's.
    #expect(PlayerLayout.overlayText(file: "forced", addon: "addon") == "forced\naddon")
    #expect(PlayerLayout.overlayText(file: nil, addon: "addon") == "addon")
    #expect(PlayerLayout.overlayText(file: "file", addon: nil) == "file")
    #expect(PlayerLayout.overlayText(file: nil, addon: nil) == nil)
}
