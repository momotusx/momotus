import Foundation
import SwiftUI
import Testing

// The player bar's rules (plans/player-bar.md): the quick shift, what shows, the strips, the fit chain on every
// width the plan names, Episodes' way back, and the notice slot Trakt's `reconnect` shares.

/// Time that moves only when a test moves it: each sleeper wakes at its own time, in order, and runs before the
/// next one, so a hold's ticks land exactly where its rule puts them.
private final class ManualTime: @unchecked Sendable {
    private let lock = NSLock()
    private var ms = 0
    private var sleepers: [(at: Int, wake: CheckedContinuation<Void, Never>)] = []

    var now: Int { lock.withLock { ms } }

    var clock: StepperClock { StepperClock(now: { [self] in .milliseconds(now) }, sleep: { [self] in await sleep($0) }) }

    func sleep(_ duration: Duration) async {
        await withCheckedContinuation { wake in
            lock.withLock { sleepers.append((ms + Int(duration / .milliseconds(1)), wake)) }
        }
    }

    @MainActor func advance(to target: Int) async {
        await settle()
        while true {
            let due: CheckedContinuation<Void, Never>? = lock.withLock {
                guard let i = sleepers.indices.filter({ sleepers[$0].at <= target }).min(by: { sleepers[$0].at < sleepers[$1].at }) else { return nil }
                let sleeper = sleepers.remove(at: i)
                ms = sleeper.at
                return sleeper.wake
            }
            guard let due else { break }
            due.resume()
            await settle()
        }
        lock.withLock { ms = target }
    }

    /// Every sleeper wakes, so no task is left waiting when a test ends.
    @MainActor func drain() async {
        let all = lock.withLock { defer { sleepers = [] }; return sleepers.map(\.wake) }
        all.forEach { $0.resume() }
        await settle()
    }
}

/// Lets woken tasks run: a sleeper resumes off the main actor and hops back, so a little real time passes too.
@MainActor private func settle() async {
    for _ in 0..<4 {
        for _ in 0..<20 { await Task.yield() }
        try? await Task.sleep(for: .milliseconds(1))
    }
}

// MARK: the quick shift (D2, D3, section 6)

@MainActor @Test func aHoldOfTwoSecondsMovesExactlyFourAndAHalfSeconds() async {
    let time = ManualTime()
    var moves: [Double] = []
    let hold = TimingHold(rule: .quick, clock: time.clock) { moves.append($0) }
    hold.start(1)
    await time.advance(to: 2000)
    hold.stop(single: true)
    await time.drain()
    // Ticks at 400, 550, 700 and 850 ms move 0.25 s each; from 1000 ms to 1900 ms seven ticks move 0.5 s each.
    #expect(moves == Array(repeating: 0.25, count: 4) + Array(repeating: 0.5, count: 7), "\(moves)")
    #expect(moves.reduce(0, +) == 4.5)
}

@MainActor @Test func aHeldKeyMovesByTheRuleNotByTheKeyboard() async {
    // Section 6: keydown at 0 and keyup at 990 ms is exactly +1.0 s (ticks at 400, 550, 700 and 850 ms); keyup at
    // 300 ms is one tap, +0.25 s. The keyboard's own repeats never reach the hold.
    for (up, total) in [(990, 1.0), (300, 0.25)] {
        let time = ManualTime()
        var value = 0.0
        let hold = TimingHold(rule: .quick, clock: time.clock) { value = PlayerLayout.nudge(value, by: $0) }
        hold.start(1)
        await time.advance(to: up)
        hold.stop(single: true)
        await time.drain()
        #expect(value == total, "keyup at \(up) ms")
    }
    // Earlier from 0, and four taps.
    var value = 0.0
    let tap = TimingHold(rule: .quick, clock: ManualTime().clock) { value = PlayerLayout.nudge(value, by: $0) }
    tap.start(-1)
    tap.stop(single: true)
    #expect(PlayerLayout.delayText(value) == "\u{2212}0.25 s")
    value = 0
    for _ in 1...4 { tap.start(1); tap.stop(single: true) }
    #expect(PlayerLayout.delayText(value) == "+1.0 s")
}

@Test func theShiftReadsAtLeastOneDecimalAndAtMostTwo() {
    #expect(PlayerLayout.delayText(0.25) == "+0.25 s")
    #expect(PlayerLayout.delayText(0.5) == "+0.5 s")
    #expect(PlayerLayout.delayText(1) == "+1.0 s")
    #expect(PlayerLayout.delayText(0) == "0.0 s")
    #expect(PlayerLayout.delayText(-60) == "\u{2212}60.0 s")
    // Within 60 s either way, as the core keeps it.
    #expect(PlayerLayout.nudge(59.9, by: 0.25) == 60)
    // Said once it settles (D12), and named with its visible text first (section 8).
    #expect(PlayerLayout.shiftSaid(0.25) == "Subtitles 0.25 seconds later")
    #expect(PlayerLayout.shiftSaid(-1) == "Subtitles 1.0 seconds earlier")
    #expect(PlayerLayout.shiftSaid(0) == "Subtitles in sync")
    #expect(PlayerLayout.shiftValueName(0.25) == "+0.25 s, subtitles later, press to reset")
    #expect(PlayerLayout.shiftValueName(-0.5) == "\u{2212}0.5 s, subtitles earlier, press to reset")
    // At 0 a press does nothing, so the name ends there; a remote's OK always adjusts.
    #expect(PlayerLayout.shiftValueName(0) == "0.0 s, subtitles in sync")
    #expect(PlayerLayout.shiftValueName(0, tv: true) == "0.0 s, subtitles in sync, press OK to adjust")
}

// MARK: what the bar shows

private func track(_ url: String, _ tag: String) -> SubtitleTrack {
    SubtitleTrack(url: url, lang: tag, label: nil, source: "Walk", forced: false, hearingImpaired: false)
}

@Test func theShiftShowsOnlyForASubtitleThatCanBeTimed() {
    // D4: the picker's own Timing condition (`timingKey`): an addon track or a file track the app draws.
    #expect(BarRules.shiftShown(timingKey: "https://subs/1.vtt"))
    #expect(BarRules.shiftShown(timingKey: PlayerLayout.fileTrackKey(0)))
    #expect(!BarRules.shiftShown(timingKey: nil), "Off, captions in the video or a picture subtitle")
    // The model's key is the picker's: an addon track, else a timeable file track, else none.
    let model = (try? playerModel()) ?? ""
    #expect(model.contains("guard timingKey != nil else { return }"), "shiftSubtitles is gated by the same key")
}

@Test func nextShowsOnlyForASeriesWhoseCoreNamesANextEpisode() {
    let next = NextEpisode(videoId: "tt1:1:3", title: "In Perpetuity", season: 1, episode: 3, thumbnail: nil)
    #expect(BarRules.shows(season: 1, next: next) == (true, true))
    #expect(BarRules.shows(season: 1, next: nil) == (true, false), "the last episode, or the next not out yet")
    #expect(BarRules.shows(season: nil, next: nil) == (false, false), "a film")
    #expect(BarRules.nextName(next) == "Next episode, S1:E3 In Perpetuity")
}

@Test func theChipsPutTheLanguageOnFirstThenTheProfilesThenTheCores() {
    let tracks = ["fr", "de", "en", "es", "it", "pt", "tr"].enumerated().map { track("https://s/\($0.offset)", $0.element) }
    let languages = TrackPicker.languages(tracks, file: [])
    let chips = BarRules.chips(languages, on: "es", preferred: ["tr", "en"], cap: 5).map(\.language)
    #expect(chips == ["es", "tr", "en", "fr", "de"])
    #expect(BarRules.chips(languages, on: nil, preferred: [], cap: 3).map(\.language) == ["fr", "de", "en"])
    #expect(BarRules.chipCap(phone: true) == 3 && BarRules.chipCap(phone: false) == 5)
    // The other line's language is never a chip.
    var other = languages
    other[0].disabled = true
    #expect(!BarRules.chips(other, on: nil, preferred: ["fr"], cap: 5).map(\.language).contains("fr"))
}

@Test func aTapOutsideAnOpenStripOnlyClosesIt() {
    // D21: the first tap closes the strip; the controls neither hide nor seek.
    #expect(BarRules.pictureTapClosesStrip(.subtitles))
    #expect(BarRules.pictureTapClosesStrip(.audio))
    #expect(!BarRules.pictureTapClosesStrip(nil))
    // Escape and Back: the Style row first, then the strip, then the player's own way.
    #expect(BarRules.back(strip: .subtitles, style: true) == .closeStyle)
    #expect(BarRules.back(strip: .subtitles, style: false) == .closeStrip)
    #expect(BarRules.back(strip: nil, style: false) == .player)
}

@Test func skipOrUpNextClosesAnOpenStrip() {
    // D23.
    #expect(BarRules.strip(.subtitles, skipOrUpNext: true) == nil)
    #expect(BarRules.strip(.audio, skipOrUpNext: true) == nil)
    #expect(BarRules.strip(.subtitles, skipOrUpNext: false) == .subtitles)
}

@Test func episodesSelectsInPlaceForItsOwnTitleAndPushesAnyOther() {
    // Section 5, Apple Episodes: a player over its own title page pops back to it (no second copy, Versions
    // included); from Home, a place or Downloads the title is pushed after the player leaves.
    #expect(BarRules.episodes(playing: "tt1748166", page: "tt1748166") == .selectInPlace)
    #expect(BarRules.episodes(playing: "tt1748166", page: "tt0111161") == .push)
    #expect(BarRules.episodes(playing: "tt1748166", page: nil) == .push)
}

@Test func episodesLeavesThePlayerBeforeTheTitleOpens() throws {
    // The order is fixed: close (dismiss) first, then the route on the next turn of the main queue.
    let screen = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player/PlayerScreen.swift"), encoding: .utf8)
    let open = try body(of: "private func openEpisodes()", in: screen)
    let close = try #require(open.range(of: "close()"))
    let route = try #require(open.range(of: "DispatchQueue.main.async { MainActor.assumeIsolated { open(route) } }"))
    #expect(close.lowerBound < route.lowerBound)
    let title = try String(contentsOf: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Title/TitleScreen.swift"), encoding: .utf8)
    let opened = try body(of: "private func titleOpened(", in: title)
    #expect(opened.contains("(playing, showVersions, showEpisodeVersions) = (nil, false, false)"))
    #expect(opened.contains("parentOpenTitle(route)"))
    #expect(title.contains(".environment(\\.openTitle, OpenTitleAction { titleOpened($0) })"))
}

@Test func theStyleRowStepsThroughTheCoresValues() {
    #expect(BarRules.sizeText(.medium) == "100%" && BarRules.sizeText(.small) == "80%" && BarRules.sizeText(.extraLarge) == "150%")
    #expect(BarRules.sizeStep(.medium, by: 1) == .large)
    #expect(BarRules.sizeStep(.extraLarge, by: 1) == nil && BarRules.sizeStep(.small, by: -1) == nil)
    #expect(BarRules.positionText(8) == "Default" && BarRules.positionText(12) == "12%")
    #expect(BarRules.positionStep(8, by: 1) == 12 && BarRules.positionStep(8, by: -1) == 5)
    #expect(BarRules.positionStep(25, by: 1) == nil && BarRules.positionStep(5, by: -1) == nil)
    #expect(BarRules.positionStep(11, by: 1) == 16, "a value off the steps goes on from the nearest")
}

@Test func nothingTheOwnerTookOffTheBarIsBack() throws {
    // Acceptance 4: no mute, volume slider or More in the bar (the system volume and the Mac's menu commands stay).
    let player = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player")
    let bar = try ["PlayerControls.swift", "PlayerStrip.swift"].map { try String(contentsOf: player.appending(path: $0), encoding: .utf8) }
        .joined(separator: "\n").split(separator: "\n")
        .filter { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("//") && !$0.trimmingCharacters(in: .whitespaces).hasPrefix("///") }
        .joined(separator: "\n")
    #expect(bar.range(of: "volume", options: .caseInsensitive) == nil)
    #expect(!bar.contains("\"More\"") && !bar.contains("ellipsis"))
}

// MARK: the fit chain (D15 to D17)

/// The bar at its longest: a series, the shift's room, the longest values the plan names.
private let longLeft = [
    BarFit.Pill(word: "Audio", value: "English, Stereo, Commentary"), BarFit.Pill(word: "Speed", value: "1.25x"),
    BarFit.Pill(word: "Versions", value: "4K HDR Dolby Vision"),
]

private func longRight(subtitle: String) -> [BarFit.Pill] {
    let reserve = BarFit.subtitleReserve(off: "Off, 112 found", languages: ["English", "Portuguese (Brazil)", "Turkish"])
    return [BarFit.Pill(word: "Subtitles", value: subtitle, reserve: reserve), BarFit.Pill(word: "Episodes"), BarFit.Pill(word: "Next"), BarFit.Pill(word: nil)]
}

@Test func theMacBarFitsAtEveryWidthWithNothingMeetingTheTransport() {
    // The window's width less its gutters (40 pt each side on the Mac; 16 pt on a compact iPad window).
    let widths: [(String, CGFloat)] = [
        ("Mac minimum", BackdropStops.macMinWidth - 80), ("800", 720), ("1024", 944), ("1366", 1286), ("1440", 1360), ("compact", 375 - 32),
    ]
    for (name, width) in widths {
        let on = BarFit.desktop(width: width, left: longLeft, right: longRight(subtitle: "Portuguese (Brazil), Auto"))
        #expect(BarFit.fits(on, width: width, left: longLeft, right: longRight(subtitle: "Portuguese (Brazil), Auto")) || width < 400, "\(name): \(on)")
        // A subtitle turned off (and on again) changes nothing in the layout.
        let off = BarFit.desktop(width: width, left: longLeft, right: longRight(subtitle: "Off, 112 found"))
        #expect(on == off, "\(name)")
        // Every target stays whole.
        for pill in longLeft + longRight(subtitle: "English") {
            for words in [true, false] { #expect(BarFit.pillWidth(pill, words: words) >= Theme.minTarget) }
        }
        #expect(BarFit.shiftWidth(words: false) >= 2 * Theme.minTarget + BarFit.shiftValue)
        #expect(BarFit.transport >= 2 * Theme.minTarget + BarFit.play)
    }
    // The chain goes as the plan says: wide windows keep a row; the Mac's minimum puts Right under the transport;
    // a compact window puts both groups under it (where the row scrolls, its targets whole).
    #expect(BarFit.desktop(width: 1360, left: longLeft, right: longRight(subtitle: "English")).under == .none)
    #expect(BarFit.desktop(width: 640, left: longLeft, right: longRight(subtitle: "English")).under == .right)
    #expect(BarFit.desktop(width: 343, left: longLeft, right: longRight(subtitle: "English")).under == .both)
    let film = [BarFit.Pill(word: "Subtitles", value: "English"), BarFit.Pill(word: nil)]
    let plan = BarFit.desktop(width: 1360, left: longLeft, right: film)
    #expect(plan.under == .none && !plan.chevrons)
}

@Test func theIPhoneAcrossKeepsOneRowAndAScrubberOf120() {
    // The iPhone 13 Pro across: 844 pt less its 47 pt sides; a series (Subtitles, Episodes, Next) with the shift.
    let times = BarFit.phoneTimes() + 2 * PhoneFrame.barInset + 16
    let across = BarFit.phone(width: 844 - 94, left: 3, right: 3, times: times)
    #expect(!across.twoRows)
    #expect(BarFit.phoneScrubber(width: 750, left: 3, right: 3, times: times, chevrons: across.chevrons) >= BarFit.minScrubber)
    // A film keeps the words.
    #expect(BarFit.phone(width: 750, left: 3, right: 1, times: times) == BarFit.Phone())
    // A small phone across (an iPhone SE, 568 pt less its edges): two rows, never a scrubber under 120.
    let small = BarFit.phone(width: 548, left: 3, right: 3, times: times)
    #expect(small.twoRows)
    // The row count never depends on a subtitle being on: the shift's room is always counted (no input names it).
    #expect(BarFit.phone(width: 750, left: 3, right: 3, times: times) == across)
    #expect(BarFit.pill >= Theme.minTarget)
}

@Test func theIPhoneUprightUsesThreeRowsThatFit() {
    // D15 at 375 and 390 pt: Subtitles and the shift fit their own row, in words; the other row holds five icons.
    for width: CGFloat in [375, 390] {
        #expect(!BarFit.uprightChevrons(width: width), "\(width)")
        #expect(BarFit.groupWidth(Array(repeating: BarFit.pill, count: 5), gap: BarFit.phoneGap) + BarFit.phoneGap <= width)
    }
    #expect(BarFit.uprightChevrons(width: 200))
}

// MARK: the notice slot (section 5.1)

@MainActor @Test func reconnectHoldsTheSlotAndASubtitleNoteComesAfterIt() async {
    let time = ManualTime()
    var seen: [String?] = []
    let slot = NoticeSlot(sleep: { await time.sleep($0) }) { seen.append($0) }
    // Play start: the subtitle note shows, then the snapshot's `reconnect` takes the slot at once.
    slot.show("Some subtitles did not load.")
    slot.show(TraktWords.playerReconnect, seconds: TraktRules.noticeSeconds, priority: true)
    #expect(slot.line == TraktWords.playerReconnect)
    // A note that comes during the 4 s waits.
    slot.show("This subtitle could not be read.")
    await time.advance(to: 3999)
    #expect(slot.line == TraktWords.playerReconnect, "the Trakt line holds its full 4 s")
    await time.advance(to: 4000)
    #expect(slot.line == "This subtitle could not be read.", "the newest waiting line, for its own full time")
    await time.advance(to: 8999)
    #expect(slot.line == "This subtitle could not be read.")
    await time.advance(to: 9000)
    #expect(slot.line == nil)
    #expect(seen == ["Some subtitles did not load.", TraktWords.playerReconnect, "This subtitle could not be read.", nil])
    await time.drain()
}

@MainActor @Test func aNoteAlreadyUpGivesWayAndReturnsAfterTheTraktLine() async {
    let time = ManualTime()
    let slot = NoticeSlot(sleep: { await time.sleep($0) }) { _ in }
    slot.show("A subtitle source did not answer.")
    await time.advance(to: 2000)
    slot.show(TraktWords.playerReconnect, seconds: TraktRules.noticeSeconds, priority: true)
    await time.advance(to: 5999)
    #expect(slot.line == TraktWords.playerReconnect)
    await time.advance(to: 6000)
    #expect(slot.line == "A subtitle source did not answer.", "back for its full 5 s")
    await time.advance(to: 10999)
    #expect(slot.line == "A subtitle source did not answer.")
    await time.advance(to: 11000)
    #expect(slot.line == nil)
    // A line that waits for the viewer (a heat pause) stays until cleared.
    slot.show("Paused: the phone is running hot.", seconds: nil)
    await time.advance(to: 60000)
    #expect(slot.line == "Paused: the phone is running hot.")
    slot.clear("Paused: the phone is running hot.")
    #expect(slot.line == nil)
    await time.drain()
}

@Test func theModelShowsReconnectWithPriorityForFourSeconds() throws {
    let model = try playerModel()
    let trakt = try body(of: "private func showTraktNotice(", in: model)
    #expect(trakt.contains("showStatus(TraktWords.playerReconnect, seconds: TraktRules.noticeSeconds, priority: true)"))
    // Every other line keeps its 5 s and no priority.
    #expect(model.contains("private func showStatus(_ words: String, untilPlay: Bool = false, seconds: Double = 5, priority: Bool = false)"))
    #expect(!model.contains("traktNotice ="), "no separate Trakt flag")
}

// MARK: the background (D8)

@Test func theBackgroundMapsToTheCaptionStyleUnlessTheSystemInsists() {
    let base = CaptionStyle(
        face: nil, scale: 1, text: .white, highlight: .black.opacity(0.8), window: .clear, oneBoxFill: .black.opacity(0.8),
        radius: 4, edge: .none)
    // Box keeps today's look.
    let box = base.with(.box, systemInsists: false)
    #expect(box.highlight == base.highlight && box.edge == .none)
    // Outline and Shadow draw no box, only an edge.
    let outline = base.with(.outline, systemInsists: false)
    #expect(outline.highlight == .clear && outline.window == .clear && outline.oneBoxFill == .clear && outline.edge == .uniform)
    let shadow = base.with(.shadow, systemInsists: false)
    #expect(shadow.highlight == .clear && shadow.edge == .dropShadow)
    // The viewer's caption settings win over the profile.
    let insisted = base.with(.outline, systemInsists: true)
    #expect(insisted.highlight == base.highlight && insisted.edge == .none)
}
