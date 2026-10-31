import Foundation
import SwiftUI
import Testing

// The second subtitle line on Apple (plans/dual-subtitles.md phase 4, sections 4, 5.1, 5.4 and 6.3): the core's line
// followed until the viewer picks, each line's own timing, the three layouts, Only when paused, and the picker's
// switch, Spoken row and disabled rows. Mutation rows DP1 to DP6 in bench/dual-subtitles/apple.jsonl.

private func track(_ n: Int, _ lang: String, _ language: String, _ name: String) -> SubtitleTrack {
    SubtitleTrack(url: "http://127.0.0.1:1/sub/\(n).vtt", lang: lang, label: nil, source: "Walk", forced: false, hearingImpaired: false,
                  language: language, languageName: name, name: "\(name) \(n)")
}

private let turkish = track(1, "tur", "tr", "Turkish")
private let english1 = track(2, "eng", "en", "English")
private let english2 = track(3, "eng", "en", "English")
private let german = track(4, "ger", "de", "German")
private let tracks = [turkish, english1, english2, german]

private func source(_ name: String) throws -> String {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Player/\(name)")
    return try String(contentsOf: url, encoding: .utf8)
}

/// DP1: the core's `autoSecondSubtitle` is a level, followed until the viewer picks; a new source or play follows again.
@MainActor @Test func theSecondLineFollowsTheCoreUntilTheViewerPicks() {
    let choices = PlayChoices()
    var sent: [[String: any Sendable]] = []
    choices.opened(sub: 7) { _, command in sent.append(command) }
    // On, kept, moved by a first-line change, then off when the core says none.
    #expect(choices.secondStep(auto: english1.url, tracks: tracks, shown: nil) == .some(english1))
    #expect(choices.secondStep(auto: english1.url, tracks: tracks, shown: english1) == nil)
    #expect(choices.secondStep(auto: german.url, tracks: tracks, shown: english1) == .some(german))
    #expect(choices.secondStep(auto: nil, tracks: tracks, shown: german) == .some(nil))
    // A url the list does not hold turns nothing on.
    #expect(choices.secondStep(auto: "http://127.0.0.1:1/sub/9.vtt", tracks: tracks, shown: nil) == nil)
    // The viewer's pick wins: the core's level no longer moves it, and the core keeps it as the profile's.
    choices.choseSecond(english2, spoken: true)
    #expect(choices.viewerChoseSecond)
    #expect(choices.secondStep(auto: german.url, tracks: tracks, shown: english2) == nil)
    #expect(sent.last?["type"] as? String == "secondSubtitleChosen")
    #expect(sent.last?["url"] as? String == english2.url && sent.last?["spoken"] as? Bool == true)
    // A picked track that left the list goes off.
    #expect(choices.secondStep(auto: nil, tracks: [turkish], shown: english2) == .some(nil))
    choices.choseSecond(nil, spoken: false)
    #expect(sent.last?["url"] is NSNull && sent.last?["spoken"] as? Bool == false)
    // Another source follows the core again; so does another play.
    _ = choices.source(1)
    _ = choices.source(2)
    #expect(!choices.viewerChoseSecond)
    #expect(choices.secondStep(auto: english1.url, tracks: tracks, shown: nil) == .some(english1))
    choices.choseSecond(nil, spoken: false)
    choices.newPlay()
    #expect(!choices.viewerChoseSecond)
    // Spoken language with no track: while the list is still out the core may yet bring one, so it is followed.
    choices.keptSpoken(pending: true)
    #expect(!choices.viewerChoseSecond)
    choices.keptSpoken(pending: false)
    #expect(choices.viewerChoseSecond)
}

@Test func theModelFollowsTheCoresSecondLineAndTimesItApart() throws {
    let model = try source("PlayerModel.swift")
    let apply = try body(of: "private func apply(", in: model)
    #expect(apply.contains("if let next = choices.secondStep(auto: snapshot.autoSecondSubtitle, tracks: snapshot.subtitles, shown: secondSubtitle) {\n            selectSecondSubtitle(next)"))
    // A server move renames the second's URL too: its value goes with it.
    #expect(apply.contains("if let shown = secondSubtitle, !snapshot.subtitles.contains(shown), let same = SubtitleMenu.same(shown, in: snapshot.subtitles) {\n            outbox.moved(from: shown.url, to: same.url)"))
    // Each line's own edges and timing: the second's delay never reaches the first's.
    #expect(try body(of: "private func watchCueEdges(", in: model).contains("PlayerLayout.cueEdges(secondCues, delay: secondDelay)"))
    let set = try body(of: "func setSecondDelay(", in: model)
    #expect(set.contains("guard let url = secondSubtitle?.url else { return }") && set.contains("secondDelay = seconds") && !set.contains("subtitleDelay"))
    let select = try body(of: "func selectSecondSubtitle(", in: model)
    #expect(select.contains("flushDelay()\n            secondDelay = outbox.start(track)"))
    #expect(try body(of: "var secondText: String?", in: model).contains("WebVTT.text(at: PlayerLayout.cueTime(position: position, delay: secondDelay), in: secondCues)"))
}

/// DP2: the outbox sends the waiting value of one line before it takes the other's, so tuning both lines inside
/// 700 ms keeps both.
@MainActor @Test func theOutboxFlushesWhenTheOtherLineIsSet() async {
    var sent: [String] = []
    let outbox = DelayOutbox(clock: StepperClock(now: { .zero }, sleep: { _ in await Task.yield() })) {
        sent.append("\($0["url"] as? String ?? "") \($0["ms"] as? Int ?? 0)")
        return true
    }
    outbox.set(url: turkish.url, seconds: 0.5)
    outbox.set(url: english1.url, seconds: -0.3)
    #expect(sent == ["\(turkish.url) 500"])
    for _ in 0..<50 { await Task.yield() }
    #expect(sent == ["\(turkish.url) 500", "\(english1.url) -300"])
    #expect(outbox.kept == [turkish.url: 0.5, english1.url: -0.3])
}

/// DP6 (and the rules beside it): drawn only beside a first line, never beside a picture first line or in the
/// first's language, and with Only when paused only while the viewer has paused.
@Test func theSecondLineIsDrawnOnlyBesideAFirstLineAndWhenPausedIfAsked() throws {
    let drawn = { (second: Bool, firstOn: Bool, picture: Bool, same: Bool, onPause: Bool, paused: Bool) in
        PlayerLayout.secondDrawn(second: second, firstOn: firstOn, pictureFirst: picture, sameLanguage: same, onPause: onPause, paused: paused)
    }
    #expect(drawn(true, true, false, false, false, false))
    #expect(!drawn(false, true, false, false, false, false))
    #expect(!drawn(true, false, false, false, false, false))
    #expect(!drawn(true, true, true, false, false, false))
    #expect(!drawn(true, true, false, true, false, false))
    #expect(!drawn(true, true, false, false, true, false))
    #expect(drawn(true, true, false, false, true, true))
    // The model keys the gate on its own `paused` (the viewer's pause, not a stall), read live by the overlay.
    let model = try source("PlayerModel.swift")
    #expect(try body(of: "var secondText: String?", in: model).contains("guard secondDrawn(paused: paused) else { return nil }"))
    #expect(model.contains("    private(set) var paused = false"))
    #expect(model.contains("onPause: secondOnPause, paused: paused)"))
    // The first line's language as drawn: the core's track's, else the file track's own code.
    #expect(try body(of: "var firstLanguage: String?", in: model).contains("textTrack.flatMap { textTrackLanguages[safe: $0] ?? nil }"))
}

/// The switch and the second line go with a picture first line, known from the track before any bitmap.
@Test func aPictureFirstLineHidesTheSwitchFromItsTrack() throws {
    // The engine names the decoder ("pgssub"); the descriptor's name reads the same.
    #expect(PlayerLayout.pictureTrack(codec: "pgssub") && PlayerLayout.pictureTrack(codec: "dvdsub") && PlayerLayout.pictureTrack(codec: "dvbsub"))
    #expect(PlayerLayout.pictureTrack(codec: "hdmv_pgs_subtitle") && PlayerLayout.pictureTrack(codec: "xsub"))
    #expect(PlayerLayout.pictureTrack(codec: "dvd_subtitle") && PlayerLayout.pictureTrack(codec: "dvb_subtitle"))
    #expect(!PlayerLayout.pictureTrack(codec: "subrip") && !PlayerLayout.pictureTrack(codec: "ass") && !PlayerLayout.pictureTrack(codec: "webvtt"))
    let model = try source("PlayerModel.swift")
    #expect(try body(of: "private func engineTracks(", in: model).contains("textPictures = text.map { PlayerLayout.pictureTrack(codec: $0.codec) }"))
    #expect(model.contains("var pictureFirst: Bool { subtitle == nil && textTrack.flatMap { textPictures[safe: $0] } == true }"))
    let picker = try source("SubtitlePicker.swift")
    #expect(picker.contains("private var lines: Bool { kind == .subtitles && model.firstOn && !model.pictureFirst }"))
}

/// DP3: VoiceOver reads the first line only; two languages read in a row are noise.
@Test func theSecondLineIsHiddenFromVoiceOver() throws {
    let block = try body(of: "private struct SubtitleBlock: View", in: try source("PlayerScreen.swift"))
    #expect(block.contains("SubtitleLine(text: second, style: style, size: size, scale: scale, boxed: !oneBox)\n                    .accessibilityHidden(true)"))
    #expect(block.contains("SubtitleLine(text: first, style: style, size: size, boxed: !oneBox)\n                    .accessibilityHidden(firstHidden)"))
    #expect(try source("PlayerScreen.swift").contains("oneBox: model.subtitleLayout == .oneBox, firstHidden: model.subtitle == nil)"))
}

/// Stacked: the second over the first at 85%, one block hung from the foot and scaled as one.
@Test func stackedIsOneBlockWithTheSecondAtEightyFivePercent() throws {
    #expect(PlayerLayout.secondScale == 0.85)
    let screen = try source("PlayerScreen.swift")
    let block = try body(of: "private struct SubtitleBlock: View", in: screen)
    #expect(block.contains("VStack(spacing: SubtitleLine.points(style, size) * 0.25 * unit) {\n            if let second {"))
    #expect(block.contains("var scale = PlayerLayout.secondScale"))
    #expect(screen.contains("first: first, second: split ? nil : second, size: model.subtitleSize,"))
    #expect(screen.contains(".onGeometryChange(for: CGFloat.self) { $0.size.height } action: { subtitleBox = $0 }\n                            .scaleEffect(place.scale, anchor: .bottom)"))
}

/// DP4: Split hangs the second line from the top, the mirror of the foot.
@Test func splitHangsTheSecondLineFromTheTop() {
    let top = { (h: CGFloat, picture: CGFloat, safeTop: CGFloat, up: Bool, bar: CGFloat) in
        PlayerLayout.subtitleTop(screenHeight: h, pictureHeight: picture, safeTop: safeTop, controlsShown: up, topBarFoot: bar)
    }
    // A full-frame TV picture: 8% of it under its top, over the 5% line.
    #expect(abs(top(1080, 1080, 0, false, 0) - 86.4) < 0.01)
    // A 2.39:1 film: under the picture's top, not on the band.
    let wide = 1920 / 2.39
    #expect(abs(top(1080, wide, 0, false, 0) - ((1080 - wide) / 2 + wide * 0.08)) < 0.01)
    // With the controls up, 8 pt under the top bar's foot; hidden, the bar counts for nothing.
    #expect(top(1080, 1080, 0, true, 150) == 158)
    #expect(abs(top(1080, 1080, 0, false, 150) - 86.4) < 0.01)
    // Never over the safe area or the 5% line.
    #expect(top(1000, 200, 120, false, 0) == 416)
    #expect(top(1000, 1000, 100, false, 0) == 100)
    // An upright phone: the band over the picture takes the line when it holds its box and 8 pt.
    let picture: CGFloat = 390 * 9 / 16
    let pictureTop = (844 - picture) / 2
    let upright = PlayerLayout.subtitleTop(
        screenHeight: 844, pictureHeight: picture, safeTop: 47, controlsShown: false, topBarFoot: 0, upright: true, boxHeight: 60)
    #expect(abs(upright - (pictureTop - 8 - 60)) < 0.01)
    let across = PlayerLayout.subtitleTop(
        screenHeight: 844, pictureHeight: picture, safeTop: 47, controlsShown: false, topBarFoot: 0, upright: false, boxHeight: 60)
    #expect(abs(across - (pictureTop + picture * 0.08)) < 0.01)
}

/// DP5: One box draws one background behind both lines: the style's window when it shows one, else the colour
/// behind the glyphs, never both.
@Test func oneBoxDrawsOneBackgroundBehindBothLines() throws {
    #expect(CaptionStyle.oneBoxUsesWindow(windowOpacity: 0.6))
    #expect(!CaptionStyle.oneBoxUsesWindow(windowOpacity: 0))
    let screen = try source("PlayerScreen.swift")
    let block = try body(of: "private struct SubtitleBlock: View", in: screen)
    #expect(block.contains(".modifier(CaptionBox(on: oneBox, style: style, fill: style.oneBoxFill))"))
    let line = try body(of: "private struct SubtitleLine: View", in: screen)
    #expect(line.contains(".background(boxed ? style.highlight : .clear)"))
    #expect(line.contains(".modifier(CaptionBox(on: boxed, style: style, fill: style.window))"))
    #expect(try source("CaptionStyle.swift").contains("oneBoxFill: oneBoxUsesWindow(windowOpacity: windowOpacity) ? window : highlight,"))
}

@Test func sameLanguageReadsEverySpellingAndNeverAnUnknownOne() {
    #expect(TrackPicker.sameLanguage("eng", "en"))
    #expect(TrackPicker.sameLanguage("en-GB", "en"))
    #expect(TrackPicker.sameLanguage("pt-BR", "por"))
    #expect(!TrackPicker.sameLanguage("en", "de"))
    #expect(!TrackPicker.sameLanguage("und", "und"))
    #expect(!TrackPicker.sameLanguage(nil, "en"))
    #expect(!TrackPicker.sameLanguage("", ""))
}

@Test func eachTabDisablesTheOtherLinesLanguageAndTheSecondTakesOnlyTheCoresTracks() {
    let file = [TrackPicker.FileTrack(index: 0, name: "English (US)", language: "en-US"), TrackPicker.FileTrack(index: 1, name: "French", language: "fra")]
    let second = TrackPicker.lineLanguages(.second, tracks: tracks, file: file, first: "tr", second: "en")
    #expect(second.map(\.language) == ["tr", "en", "de"])
    #expect(second.allSatisfy { $0.file.isEmpty })
    #expect(second.map(\.disabled) == [true, false, false])
    let first = TrackPicker.lineLanguages(.first, tracks: tracks, file: file, first: "tr", second: "en")
    #expect(first.map(\.language) == ["tr", "en", "de", "fr"])
    #expect(first.map(\.disabled) == [false, true, false, false])
    // The file's own row in the second's language is disabled with it.
    #expect(first[1].file.map(\.index) == [0])
    // No second line: nothing disabled on the first tab.
    #expect(TrackPicker.lineLanguages(.first, tracks: tracks, file: file, first: "tr", second: nil).allSatisfy { !$0.disabled })
    // A disabled language is never turned on or opened.
    #expect(TrackPicker.press(.language("tr"), languages: second, open: nil) == .none)
    #expect(TrackPicker.press(.language("de"), languages: second, open: nil) == .turnOn(.track(german)))
    #expect(TrackPicker.key(.right, focus: .language("tr"), order: [], languages: second, open: nil) == .none)
    // Its row says why: "Turkish, On the first line, 1 option".
    #expect(TrackPicker.spoken(second[0], auto: false, selected: false, other: "On the first line") == "Turkish, On the first line, 1 option")
}

/// The switch heads the panel, then search, Off, the Spoken row (Second line tab) and the languages; on the Mac it
/// is one stop that Left and Right move on.
@Test func theSwitchIsTheFirstStopAndLeftAndRightMoveOnIt() {
    let languages = TrackPicker.lineLanguages(.second, tracks: tracks, file: [], first: "tr", second: nil)
    let order = TrackPicker.order(audio: nil, open: nil, shown: languages, lines: true, spoken: true)
    #expect(order == [.line, .search, .off, .spoken, .language("tr"), .language("en"), .language("de")])
    #expect(TrackPicker.order(audio: nil, open: nil, shown: languages) == [.search, .off, .language("tr"), .language("en"), .language("de")])
    let key = { (k: TrackPicker.Key, focus: TrackPicker.Row) in
        TrackPicker.key(k, focus: focus, order: order, languages: languages, open: nil, spoken: "en")
    }
    #expect(key(.right, .line) == .line(.second))
    #expect(key(.left, .line) == .line(.first))
    #expect(key(.down, .line) == .focus(.search))
    #expect(key(.up, .search) == .focus(.line))
    #expect(key(.down, .off) == .focus(.spoken))
    #expect(key(.right, .spoken) == .open("en"))
    #expect(key(.enter, .spoken) == .spoken)
    #expect(key(.enter, .line) == TrackPicker.Act.none)
    #expect(TrackPicker.key(.right, focus: .spoken, order: order, languages: languages, open: nil) == TrackPicker.Act.none)
    let picker = (try? source("SubtitlePicker.swift")) ?? ""
    #expect(picker.contains("audio: kind == .audio ? shownAudio : nil, open: openLanguage, shown: shown, lines: lines,"))
}

@Test func theSubtitlesValueReadsBothLines() {
    #expect(TrackPicker.buttonValue(core: "Turkish", file: nil, auto: true, autoLanguage: "Turkish", second: "English") == "Turkish + English")
    #expect(TrackPicker.buttonValue(core: "Turkish", file: nil, auto: true, autoLanguage: "Turkish") == "Turkish, Auto")
    #expect(TrackPicker.buttonValue(core: nil, file: nil, auto: false, autoLanguage: nil, second: "English") == nil)
}

@Test func theWordsComeFromTheSharedFile() {
    let w = StreamChoice.words.player
    #expect(w.firstLine == "First line" && w.secondLine == "Second line")
    #expect(w.spokenLanguage == "Spoken language" && w.spokenNotKnown == "not known for this video")
    #expect(w.onFirstLine == "On the first line" && w.onSecondLine == "On the second line")
    #expect(w.layout == "Layout" && w.onlyWhenPaused == "Only when paused")
    #expect(SubtitleLayout.allCases.map(w.layoutName) == ["Stacked", "Split", "One box"])
}

@Test func theSnapshotAndThePrefsDecodeWithAndWithoutTheSecondLine() throws {
    let fixtures = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures")
    // The settings fixture predates the fields: none, read as Stacked and off.
    var prefs = try JSONSerialization.jsonObject(with: Data(contentsOf: fixtures.appending(path: "settings.json"))) as! [String: Any]
    let old = try JSONDecoder().decode(Prefs.self, from: JSONSerialization.data(withJSONObject: prefs))
    #expect(old.secondSubtitle == nil && old.subtitleLayout == nil && old.secondOnPause == nil)
    prefs["secondSubtitle"] = "audio"
    prefs["subtitleLayout"] = "oneBox"
    prefs["secondOnPause"] = true
    let now = try JSONDecoder().decode(Prefs.self, from: JSONSerialization.data(withJSONObject: prefs))
    #expect(now.secondSubtitle == "audio" && now.subtitleLayout == .oneBox && now.secondOnPause == true)
    // The playback snapshot: none from an older core, the track and the audio's language from this one.
    let reply = try JSONSerialization.jsonObject(with: Data(contentsOf: fixtures.appending(path: "playback-reply.json"))) as! [String: Any]
    var snapshot = (reply["result"] as! [String: Any])["snapshot"] as! [String: Any]
    snapshot.removeValue(forKey: "autoSecondSubtitle")
    snapshot.removeValue(forKey: "spokenLanguage")
    let before = try JSONDecoder().decode(PlaybackSnapshot.self, from: JSONSerialization.data(withJSONObject: snapshot))
    #expect(before.autoSecondSubtitle == nil && before.spokenLanguage == nil)
    snapshot["autoSecondSubtitle"] = english1.url
    snapshot["spokenLanguage"] = "en"
    let after = try JSONDecoder().decode(PlaybackSnapshot.self, from: JSONSerialization.data(withJSONObject: snapshot))
    #expect(after.autoSecondSubtitle == english1.url && after.spokenLanguage == "en")
}

/// The picker's Second line tab: Off, the Spoken row and its pick, the second's own tracks, its foot.
@Test func theSecondLineTabPicksForTheSecondLine() throws {
    let picker = try source("SubtitlePicker.swift")
    let run = try body(of: "private func run(", in: picker)
    #expect(run.contains("case .off: if second { model.chooseSecondSubtitle(nil) } else { model.chooseSubtitle(nil) }"))
    #expect(run.contains("case .turnOn(.track(let track)): if second { model.chooseSecondSubtitle(track) } else { model.chooseSubtitle(track) }"))
    #expect(run.contains("if let best = spokenLanguage?.tracks.first { model.chooseSecondSubtitle(best, spoken: true) } else { model.keepSpoken() }"))
    // A disabled Spoken row (the first line's language) takes no Return on the Mac.
    #expect(run.contains("guard !spokenDisabled else { break }\n            if let best"))
    // A remote lands on the new tab's chosen row once its languages are built; the Spoken row when it is the choice.
    #expect(picker.contains("if landOnTab {\n                    landOnTab = false\n                    setRow(chosenRow)"))
    #expect(picker.contains("private var chosenRow: Row { second && model.spokenOn ? .spoken : chosen.map(Row.language) ?? .off }"))
    #expect(picker.contains("if second, query.trimmingCharacters(in: .whitespaces).isEmpty { spokenRow }"))
    #expect(picker.contains("return model.playback?.subtitlesPending == true ? nil : w.spokenNotKnown"))
    let options = try body(of: "private func options(", in: picker)
    #expect(options.contains("selected: second ? model.secondSubtitle == t : model.subtitle == t)"))
    #expect(options.contains("layoutRow") && options.contains("Toggle(w.onlyWhenPaused, isOn: Binding(get: { model.secondOnPause }, set: { model.setSecondOnPause($0) }))"))
    let model = try source("PlayerModel.swift")
    #expect(try body(of: "func keepSpoken(", in: model).contains("setPrefs([\"secondSubtitle\": \"audio\"])"))
    #expect(model.contains("func setSubtitleLayout(_ layout: SubtitleLayout) { setPrefs([\"subtitleLayout\": layout.rawValue]) }"))
}
