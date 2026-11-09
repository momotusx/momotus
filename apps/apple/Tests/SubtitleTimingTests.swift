import Foundation
import SwiftUI
import Testing

// The subtitle timing stepper's hold and its way to the core (the web's lib/delay.ts, Android's
// SubtitleTiming.kt), on a virtual clock so a loaded machine changes nothing.

/// Time that moves only when a hold or the outbox sleeps; it starts well past zero, so a hold that
/// forgot when it started reads the wrong step.
private final class VirtualTime: @unchecked Sendable {
    private let lock = NSLock()
    private var ms = 5_000

    var now: Int { lock.withLock { ms } }

    var clock: StepperClock {
        StepperClock(now: { [self] in .milliseconds(now) }, sleep: { [self] duration in
            lock.withLock { ms += Int(duration / .milliseconds(1)) }
            await Task.yield()
        })
    }
}

/// Lets the hold's and the outbox's tasks run.
@MainActor private func settle(_ rounds: Int = 50) async {
    for _ in 0..<rounds { await Task.yield() }
}

private func track(_ url: String, delayMs: Int? = nil) -> SubtitleTrack {
    var t = SubtitleTrack(url: url, lang: "eng", label: nil, source: "Walk", forced: false, hearingImpaired: false)
    t.delayMs = delayMs
    return t
}

@MainActor @Test func aPressIsOneStepAndACutHoldNone() async {
    var moves: [Double] = []
    let hold = TimingHold(clock: VirtualTime().clock) { moves.append($0) }
    hold.start(1)
    hold.stop(single: true)
    #expect(moves == [0.1])
    // Cut off (a cancelled touch, focus gone): no step, and nothing keeps ticking.
    hold.start(-1)
    hold.stop(single: false)
    await settle()
    #expect(moves == [0.1])
}

@MainActor @Test func aHoldRepeatsFrom350MsAndSpeedsUpBySecondHeld() async {
    let time = VirtualTime()
    var moves: [Double] = []
    let hold = TimingHold(clock: time.clock) { moves.append($0) }
    let start = time.now
    hold.start(1)
    while time.now - start < 3_500 { await Task.yield() }
    hold.stop(single: true)
    await settle()
    let rounded = moves.map { ($0 * 10).rounded() / 10 }
    // Ticks at 350, 450 ... 950 ms: 0.3 each; then 0.4 through the second second, 0.5 through the
    // third, 0.6 from there; the release after repeats adds no step.
    let expected = Array(repeating: 0.3, count: 7) + Array(repeating: 0.4, count: 10) + Array(repeating: 0.5, count: 10)
    #expect(Array(rounded.prefix(27)) == expected, "\(rounded)")
    #expect(rounded.count >= 32 && rounded.dropFirst(27).allSatisfy { $0 == 0.6 }, "\(rounded)")
}

@MainActor @Test func aHoldOfThirtyTicksSendsOneCommandOnceItRests() async {
    var sent: [DelayOutbox.Command] = []
    let outbox = DelayOutbox(clock: VirtualTime().clock) { sent.append($0); return true }
    for tick in 1...30 { outbox.set(url: "a", seconds: Double(tick) / 10) }
    #expect(sent.isEmpty, "nothing goes while the value moves")
    await settle()
    #expect(sent.count == 1)
    #expect(sent.first?["url"] as? String == "a" && sent.first?["ms"] as? Int == 3000)
}

@MainActor @Test func aFlushSendsAtOnceOnceAndBeforeWhatFollowsIt() async {
    // Close, another track, a failover, a server move: the value goes now, to the view still open,
    // ahead of anything sent after it (the unsubscribe), and only once.
    var log: [String] = []
    let outbox = DelayOutbox(clock: VirtualTime().clock) { log.append("delay \($0["url"] as? String ?? "") \($0["ms"] as? Int ?? 0)"); return true }
    outbox.set(url: "a", seconds: 0.5)
    outbox.flush()
    log.append("unsubscribe")
    outbox.flush()
    await settle()
    #expect(log == ["delay a 500", "unsubscribe"])
}

@MainActor @Test func aTrackChangeSendsTheOldTracksValueFirst() async {
    var sent: [String] = []
    let outbox = DelayOutbox(clock: VirtualTime().clock) { sent.append("\($0["url"] as? String ?? "") \($0["ms"] as? Int ?? 0)"); return true }
    outbox.set(url: "a", seconds: -1.2)
    outbox.flush()
    outbox.set(url: "b", seconds: 0.3)
    await settle()
    #expect(sent == ["a -1200", "b 300"])
}

@MainActor @Test func eachTrackKeepsItsValueForThePlayThenTheCoresDelayMs() {
    let outbox = DelayOutbox(clock: VirtualTime().clock) { _ in true }
    #expect(outbox.start(track("a", delayMs: 300)) == 0.3)
    outbox.set(url: "a", seconds: 1.5)
    outbox.set(url: "b", seconds: -0.4)
    // The core keeps one track's delay; the play keeps each track's.
    #expect(outbox.start(track("a", delayMs: 0)) == 1.5)
    #expect(outbox.start(track("b", delayMs: 0)) == -0.4)
    #expect(outbox.start(track("c", delayMs: 700)) == 0.7)
    #expect(outbox.start(nil) == 0)
}

@MainActor @Test func aServerMoveCarriesTheValueToTheTracksNewURL() async {
    var sent: [String] = []
    let outbox = DelayOutbox(clock: VirtualTime().clock) { sent.append($0["url"] as? String ?? ""); return true }
    outbox.set(url: "http://old/1.vtt", seconds: 0.7)
    outbox.moved(from: "http://old/1.vtt", to: "http://new/1.vtt")
    outbox.flush()
    #expect(sent == ["http://new/1.vtt"])
    #expect(outbox.start(track("http://new/1.vtt")) == 0.7)
}

@MainActor @Test func theSettledValueIsSaidOnceAndNotForVoiceOversOwnAdjust() async {
    var said: [Double] = []
    let outbox = DelayOutbox(clock: VirtualTime().clock) { _ in true }
    outbox.settled = { said.append($0) }
    outbox.set(url: "a", seconds: 0.1)
    outbox.set(url: "a", seconds: 0.2)
    await settle()
    // VoiceOver reads an adjusted value itself: saying it again would be twice.
    outbox.set(url: "a", seconds: 0.3, spoken: false)
    await settle()
    #expect(said == [0.2])
}

@MainActor @Test func aValueWithNoViewToTakeItWaitsForTheNext() async {
    // Between a server move's old view and its new one: the value is kept, not dropped.
    var open = false
    var sent: [Int] = []
    let outbox = DelayOutbox(clock: VirtualTime().clock) { command in
        guard open else { return false }
        sent.append(command["ms"] as? Int ?? 0)
        return true
    }
    outbox.set(url: "a", seconds: 0.4)
    outbox.flush()
    #expect(sent.isEmpty)
    open = true
    outbox.flush()
    #expect(sent == [400])
}

@Test func aModifierAtTheReleaseStillEndsTheHold() {
    // Shift tapped while Right is held: the release is still the stepper's, or the hold runs on.
    #expect(PlayerLayout.stepperTakes(down: false, modifiers: .shift))
    #expect(PlayerLayout.stepperTakes(down: false, modifiers: .option))
    #expect(!PlayerLayout.stepperTakes(down: true, modifiers: .command))
    #expect(PlayerLayout.stepperTakes(down: true, modifiers: []))
}

@Test func aKeyWithAModifierIsNoStep() {
    #expect(PlayerLayout.stepperKey([]))
    #expect(!PlayerLayout.stepperKey(.command))
    #expect(!PlayerLayout.stepperKey(.option))
    #expect(!PlayerLayout.stepperKey([.shift]))
    #expect(!PlayerLayout.stepperKey(.control))
}

/// A function's body in a Swift source, by brace count from its signature.
func body(of signature: String, in text: String) throws -> String {
    let start = try #require(text.range(of: signature), "\(signature) not found")
    let open = try #require(text[start.upperBound...].firstIndex(of: "{"))
    var depth = 0
    var index = open
    while index < text.endIndex {
        if text[index] == "{" { depth += 1 }
        if text[index] == "}" { depth -= 1; if depth == 0 { break } }
        index = text.index(after: index)
    }
    return String(text[open...index])
}

func playerModel() throws -> String {
    let file = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player/PlayerModel.swift")
    return try String(contentsOf: file, encoding: .utf8)
}

@Test func everyWayOutOfATrackOrAViewSendsTheValueFirst() throws {
    // The model's wiring of the outbox (read from the source: the model needs a live player).
    let text = try playerModel()
    func before(_ first: String, _ then: String, in name: String) throws {
        let b = try body(of: name, in: text)
        let one = try #require(b.range(of: first), "\(name) has no \(first)")
        let two = try #require(b.range(of: then), "\(name) has no \(then)")
        #expect(one.lowerBound < two.lowerBound, "\(name): \(first) after \(then)")
    }
    try before("flushDelay()", "outbox.start(track)", in: "func selectSubtitle(")
    try before("flushDelay()", "failOverCommand(", in: "private func fail(")
    try before("flushDelay()", "report()", in: "func close()")
    try before("flushDelay()", "sub = nil", in: "private func stop()")
    try before("flushDelay()", "reopen()", in: "func serverMoved()")
    #expect(try body(of: "func chooseSubtitle(", in: text).contains("selectSubtitle(track)"))
    #expect(try body(of: "func setSubtitleDelay(", in: text).contains("outbox.set(url: url, seconds: seconds, spoken: spoken)"))
    // Synchronous, so it beats the unsubscribe; a Task could lose to it.
    let send = try body(of: "outbox = DelayOutbox ", in: text)
    #expect(send.contains("core.commandNow(sub, command)") && !send.contains("Task {") && !send.contains("await"), "\(send)")
    // The key handlers ask with the press's phase, so a release always ends a hold.
    let controls = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player/PlayerControls.swift"), encoding: .utf8)
    #expect(controls.components(separatedBy: "PlayerLayout.stepperTakes(down: press.phase == .down, modifiers: press.modifiers)").count == 3)
    #expect(!controls.contains("guard PlayerLayout.stepperKey(press.modifiers)"))
    // After a server move the track's URL changes: the outbox follows it.
    #expect(try body(of: "private func apply(", in: text).contains("outbox.moved(from: shown.url, to: same.url)"))
}

@Test func aSubtitlePickOrATimingStepNeverSetsThePictureAgain() throws {
    // The picture stays up while tracks change (the web's and Android's rule): a pick, the file's own
    // tracks and the timing change cues or a media selection, never the player's item.
    let text = try playerModel()
    for name in ["func chooseSubtitle(", "func selectSubtitle(", "func selectText(", "func setSubtitleDelay(", "private func flushDelay(",
                 "private func legibleDelivered(", "private func linesChanged("] {
        let b = try body(of: name, in: text)
        #expect(!b.contains("replaceCurrentItem") && !b.contains("AVPlayerItem(") && !b.contains("seek("), "\(name)")
    }
}

@Test func theFullPickersTimingIsASignedValue() throws {
    // The full picker's stepper keeps the signed value (the owner, 2026-10-22). The bar's quick shift says Earlier
    // and Later (the owner's pick, 2026-11-09) and lives in PlayerStrip.swift, outside this check. The tvOS items
    // and the stepper's labels, as drawn and as VoiceOver reads them; comments may say which way a sign moves the words.
    let player = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let code = try ["PlayerControls.swift", "SubtitlePicker.swift"].map { try String(contentsOf: player.appending(path: $0), encoding: .utf8) }
        .joined(separator: "\n").split(separator: "\n")
        .filter { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("//") }.joined(separator: "\n")
    #expect(code.range(of: #"earlier|later"#, options: [.regularExpression, .caseInsensitive]) == nil)
    // The stepper's labels, and tvOS's three buttons in the picker's options.
    for label in [#"Label("\u{2212}0.1 s", glyph: "minus")"#, #"Label("+0.1 s", glyph: "plus")"#,
                  #"label: "Minus 0.1 seconds""#, #"label: "Plus 0.1 seconds""#] {
        #expect(code.contains(label), "\(label)")
    }
}

@Test func theFileSubtitleRulesAreAppliedWhereTheyBelong() throws {
    // Each rule lives in a pure function with its own test; these pin the calls that apply them, in the model's functions
    // (read from the source: the model needs a live player).
    let text = try playerModel()
    func calls(_ name: String, _ needles: [String]) throws {
        let b = try body(of: name, in: text)
        for needle in needles { #expect(b.contains(needle), "\(name) has no \(needle)") }
    }
    try calls("func switchVersion(", ["outbox.forget(prefix: PlayerLayout.fileTrackPrefix)"])
    try calls("func selectText(", ["resetLegible()", "subtitleDelay = PlayerLayout.startFileDelay(textTrack: index, kept: outbox.kept)"])
    try calls("func selectSubtitle(", ["resetLegible()"])
    try calls("func seek(to", ["legibleLog.seek()"])
    try calls("private func loadTracks(", ["subtitleDelay = PlayerLayout.startFileDelay(textTrack: textTrack, kept: outbox.kept)"])
    try calls("private func legibleDelivered(", ["guard legibleOpen else { return }"])
    try calls("private func resetLegible(", ["legibleLog.reset()", "legibleOpen = false", "self?.legibleOpen = true"])
    // The item's muting output: made in load() and nowhere else (an output added later gets no delivery, bench AFS3).
    let load = try body(of: "private func load(", in: text)
    #expect(load.contains("AVPlayerItemLegibleOutput()") && load.contains("legible.suppressesPlayerRendering = true"))
    #expect(load.contains("item.add(legible)"))
    #expect(text.components(separatedBy: "AVPlayerItemLegibleOutput()").count == 2, "one output, made once")
}

@Test func theTimingStepperSitsInTheLanguagesOptions() throws {
    // The picker's second level ends with the timing row while a track that takes one is on (plans/player-polish.md 2.4).
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player/SubtitlePicker.swift")
    let picker = try String(contentsOf: url, encoding: .utf8)
    let options = try body(of: "private func options(", in: picker)
    #expect(options.contains("} else if let key = model.timingKey, chosen == l.language {\n                        timing({ model.subtitleDelay }, track: key)"))
    // Each line's row reads its own value at each step, so a hold moves on from the last step.
    #expect(options.contains("timing({ model.secondDelay }, track: url) { model.setSecondDelay($0, spoken: $1) }"))
    #expect(try body(of: "@ViewBuilder private func timing(", in: picker).contains("SubtitleTimingStepper(value: value(), track: track, takesFocus: false)"))
}
