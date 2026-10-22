import Foundation
import Testing

// The item's legible text timed by the app (plans/apple-file-subtitle-timing.md, decision A), with the delivery
// orders the spikes measured (bench/player/subtitle-sync.jsonl AFS1 to AFS4).

private func log(_ deliveries: [(Double, String)]) -> LegibleLog {
    var log = LegibleLog()
    for (time, text) in deliveries { log.deliver(text, at: time) }
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
    file.deliver("line at 20", at: 20)
    file.deliver("four second line", at: 20)
    #expect(file.text(at: 21) == "line at 20\nfour second line")
    file.deliver("", at: 20)
    #expect(file.text(at: 21) == nil)
}

@Test func aSeekIntoACueShowsItOnceTheOutputSaysSoAsMP4AndHLSDo() {
    // An open cue (its end not delivered yet) must not show through a jump.
    var file = log([(10, "long A")])
    file.seek(to: 30)
    #expect(file.text(at: 30.2) == nil, "nothing from before the seek")
    // MP4 redelivers the cue at the landing (AFS4).
    file.deliver("in progress", at: 30)
    #expect(file.text(at: 30.2) == "in progress")
    // HLS redelivers it at its own start, before the landing.
    var hls = log([(10, "long A")])
    hls.seek(to: 31)
    hls.deliver("line 16", at: 30.6)
    hls.deliver("", at: 32.1)
    #expect(hls.text(at: 31.2) == "line 16")
    // A delivery after the landing first leaves the landing empty (no cue was on there).
    var none = log([(10, "long A")])
    none.seek(to: 40)
    none.deliver("next", at: 42)
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

@Test func theItemsOutputIsMadeWithItMutingAndTheFileTimingRuleHolds() throws {
    // Read from the source: the model needs a live player.
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player/PlayerModel.swift")
    let text = try String(contentsOf: file, encoding: .utf8)
    #expect(text.contains("legible.suppressesPlayerRendering = true"))
    #expect(text.contains("legible.advanceIntervalForDelegateInvocation = PlayerLayout.maxSubtitleDelay"))
    #expect(text.contains("item.add(legible)"))
    // One output per item: never added anywhere else.
    #expect(text.components(separatedBy: ".add(").count == 2)
    #expect(text.contains("var fileDelay: Double { subtitle == nil && textTrackTimeable ? subtitleDelay : 0 }"))
    #expect(text.contains("subtitleDelay = outbox.kept[PlayerLayout.fileTrackKey(index)] ?? 0"))
    #expect(text.contains("legibleLog.seek(to: target.seconds)"))
    // Another track starts with nothing delivered and where it was left; another version forgets the file's keys.
    #expect(text.contains("legibleLog.reset()\n        subtitleDelay = outbox.kept[PlayerLayout.fileTrackKey(index)] ?? 0"))
    #expect(text.contains("outbox.forget(prefix: PlayerLayout.fileTrackPrefix)"))
}
