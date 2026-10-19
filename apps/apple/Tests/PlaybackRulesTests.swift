import AVFoundation
import Foundation
import Testing

// Skip segments and Up Next timing as plans/core-protocol.md gives them, with
// the numbers TheIntroDB returns for Breaking Bad S01E01 in a 3485 s file.

private let next = #"{"videoId":"tt0903747:1:2","title":"Cat's in the Bag...","season":1,"episode":2,"thumbnail":null}"#
private let segments = #"[{"kind":"intro","startMs":228664,"endMs":246143,"source":"TheIntroDB"},{"kind":"credits","startMs":3431000,"endMs":3485000,"source":"TheIntroDB"}]"#

private func snapshot(next: String = next, nextAtMs: String = "null", segments: String = segments) throws -> PlaybackSnapshot {
    let json = """
        {"phase":"ready","url":"http://127.0.0.1:1/v","key":"k","filename":null,"sizeBytes":null,"resumeMs":null,
         "attempts":[],"subtitles":[],"subtitlesPending":false,"next":\(next),"bingeGroup":null,
         "segments":\(segments),"nextAtMs":\(nextAtMs)}
        """
    return try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(json.utf8))
}

@Test func aSegmentIsActiveFromItsStartUpToItsEnd() throws {
    let s = try snapshot()
    #expect(s.segment(at: 228_663) == nil)
    #expect(s.segment(at: 228_664)?.label == "Skip intro")
    #expect(s.segment(at: 246_142)?.kind == "intro")
    #expect(s.segment(at: 246_143) == nil)
    #expect(s.segment(at: 3_431_000)?.label == "Skip credits")
}

@Test func onlyCreditsThatReachTheEndOfTheFileLeadIntoUpNext() throws {
    let s = try snapshot()
    let credits = try #require(s.segment(at: 3_431_000))
    let intro = try #require(s.segment(at: 230_000))
    #expect(credits.endsFile(durationMs: 3_485_000))
    #expect(credits.endsFile(durationMs: 3_485_900))
    #expect(!credits.endsFile(durationMs: 3_600_000))
    #expect(!intro.endsFile(durationMs: 3_485_000))
}

@Test func upNextFollowsTheCreditsStartWhenTheCoreKnowsIt() throws {
    let matched = try snapshot(nextAtMs: "3431000")
    #expect(!matched.offersNext(positionMs: 3_430_999, durationMs: 3_485_000, ended: false, skippedAt: nil))
    #expect(matched.offersNext(positionMs: 3_431_000, durationMs: 3_485_000, ended: false, skippedAt: nil))

    // Without it, the last 20 s.
    let own = try snapshot()
    #expect(!own.offersNext(positionMs: 3_464_999, durationMs: 3_485_000, ended: false, skippedAt: nil))
    #expect(own.offersNext(positionMs: 3_465_000, durationMs: 3_485_000, ended: false, skippedAt: nil))
    #expect(own.offersNext(positionMs: 0, durationMs: 3_485_000, ended: true, skippedAt: nil))
}

@Test func skipCreditsOpensUpNextUntilTheViewerSeeksBackBeforeThem() throws {
    let s = try snapshot()
    #expect(s.offersNext(positionMs: 3_431_000, durationMs: 3_485_000, ended: false, skippedAt: 3_431_000))
    #expect(!s.offersNext(positionMs: 3_000_000, durationMs: 3_485_000, ended: false, skippedAt: 3_431_000))
}

@Test func noUpNextWithoutANextEpisodeOrForShortFiles() throws {
    let last = try snapshot(next: "null", nextAtMs: "3431000")
    #expect(!last.offersNext(positionMs: 3_480_000, durationMs: 3_485_000, ended: true, skippedAt: nil))
    let clip = try snapshot()
    #expect(!clip.offersNext(positionMs: 59_000, durationMs: 60_000, ended: true, skippedAt: nil))
}

@Test func olderCoresWithoutSegmentsStillDecode() throws {
    let json = """
        {"phase":"resolving","url":null,"key":null,"filename":null,"sizeBytes":null,"resumeMs":null,
         "attempts":[],"subtitles":[],"subtitlesPending":true,"next":null,"bingeGroup":null}
        """
    let s = try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(json.utf8))
    #expect(s.segment(at: 0) == nil)
    #expect(s.nextAtMs == nil)
}

@Test func theCalendarOpensOnTheFirstDayOnOrAfterToday() throws {
    func calendar(_ today: String, _ dates: [String]) throws -> CalendarSnapshot {
        let days = dates.map { #"{"date":"\#($0)","items":[]}"# }.joined(separator: ",")
        let json = #"{"today":"\#(today)","days":[\#(days)],"pending":0,"failed":[],"noSchedule":[],"series":1}"#
        return try JSONDecoder().decode(CalendarSnapshot.self, from: Data(json.utf8))
    }
    #expect(try calendar("2026-10-17", ["2026-10-12", "2026-10-17", "2026-10-20"]).startDay == "2026-10-17")
    #expect(try calendar("2026-10-18", ["2026-10-12", "2026-10-17", "2026-10-20"]).startDay == "2026-10-20")
    #expect(try calendar("2026-10-30", ["2026-10-12", "2026-10-17"]).startDay == "2026-10-17")
    #expect(try calendar("2026-10-30", []).startDay == nil)
}

@Test func thePhonePlayerPlacesItsPartsByTheRealWindow() {
    // A 320 pt class phone in landscape: the transport joins the actions; from 360 it stays centered.
    #expect(PlayerLayout.joinsTransport(windowHeight: 320))
    #expect(!PlayerLayout.joinsTransport(windowHeight: 360))
    #expect(!PlayerLayout.joinsTransport(windowHeight: 0), "before the window is measured")
    // Up Next waits while the controls show on a short window, and while a track menu is open.
    #expect(!PlayerLayout.showsUpNext(due: true, controlsShown: true, menuOpen: false, windowHeight: 402))
    #expect(PlayerLayout.showsUpNext(due: true, controlsShown: false, menuOpen: false, windowHeight: 402))
    #expect(PlayerLayout.showsUpNext(due: true, controlsShown: true, menuOpen: false, windowHeight: 874))
    #expect(!PlayerLayout.showsUpNext(due: true, controlsShown: false, menuOpen: true, windowHeight: 874))
    #expect(!PlayerLayout.showsUpNext(due: false, controlsShown: false, menuOpen: false, windowHeight: 874))
    // Skip and Up Next sit just over the measured controls, else near the foot.
    #expect(PlayerLayout.overlayBottom(controlsShown: true, controlsHeight: 180) == 188)
    #expect(PlayerLayout.overlayBottom(controlsShown: false, controlsHeight: 180) == 24)
}

@Test func tvSeekingGrowsWhilePressedAgainQuickly() {
    // Quick presses one way: 10, 10, 20, 30, 60, then 120 s each.
    var run = 0
    var moved = SeekRamp.step(run)
    for _ in 1..<15 {
        run = SeekRamp.run(after: run, continuing: true)
        moved += SeekRamp.step(run)
    }
    #expect(SeekRamp.step(0) == 10 && SeekRamp.step(5) == 120 && SeekRamp.step(40) == 120)
    // Twenty minutes in fifteen presses, not 120.
    #expect(moved >= 1200)
    // A pause or the other direction starts over at 10 s.
    #expect(SeekRamp.run(after: 4, continuing: false) == 0)
}

@Test func aMenuHoldsTheControlsUntilAPickATouchOrItsLimit() {
    let opened = ContinuousClock.now
    #expect(PlayerLayout.menuHolds(openedAt: opened, now: opened))
    #expect(PlayerLayout.menuHolds(openedAt: opened, now: opened + .seconds(9)))
    // Dismissed by a tap outside, which iOS does not report: the hold lets go.
    #expect(!PlayerLayout.menuHolds(openedAt: opened, now: opened + .seconds(10)))
    // A pick or a touch cleared it.
    #expect(!PlayerLayout.menuHolds(openedAt: nil, now: opened))
}

@Test func captionsKeepTheAppFaceForTheSystemOne() {
    // The default styles name the system face privately: the app's Inter stands in.
    #expect(CaptionStyle.face(named: ".AppleSystemUIFontMedium") == nil)
    #expect(CaptionStyle.face(named: nil) == nil)
    // A face the viewer picked is used as is.
    #expect(CaptionStyle.face(named: "Menlo-Regular") == "Menlo-Regular")
}

@Test func sourcesThatFailedAlikeAreSaidOnceInFirstOrder() throws {
    let gone = "The link no longer exists (404)"
    let slow = "The source did not answer in time"
    let attempts = [
        Attempt(key: "a", via: "Torrentio", error: slow),
        Attempt(key: "b", via: "Torrentio", error: gone),
        Attempt(key: "c", via: "Local", error: gone),
        Attempt(key: "d", via: "Torrentio", error: gone),
    ]
    let grouped = FailedSources.group(attempts)
    #expect(grouped.map(\.via) == ["Torrentio", "Torrentio", "Local"])
    #expect(grouped.map(\.words) == [slow, "\(gone) · 2 sources", gone])
}

@Test func aFailoverNamesTheAttemptThatWasLoaded() {
    let command = failOverCommand(PlayerFailure(kind: "decode", error: "decoder error"), attempt: 3)
    #expect(command["type"] as? String == "failOver")
    #expect(command["error"] as? String == "decoder error")
    #expect(command["kind"] as? String == "decode")
    #expect(command["attempt"] as? Int64 == 3)
    #expect(command.count == 4)
}

@Test func anUnknownAttemptIsLeftOutOfTheFailover() {
    let command = failOverCommand(PlayerFailure(kind: "network", error: "offline"), attempt: nil)
    #expect(command["attempt"] == nil)
    #expect(command["kind"] as? String == "network")
    #expect(command.count == 3)
}

@Test func playerErrorsAreSortedIntoWhatFailed() {
    let url = { (code: Int) in NSError(domain: NSURLErrorDomain, code: code) }
    #expect(playerFailure(url(NSURLErrorTimedOut)).kind == "timeout")
    #expect(playerFailure(url(NSURLErrorCannotConnectToHost)).kind == "network")
    #expect(playerFailure(url(NSURLErrorNotConnectedToInternet)).kind == "network")
    let av = { (code: AVError.Code) in NSError(domain: AVFoundationErrorDomain, code: code.rawValue) }
    #expect(playerFailure(av(.decoderNotFound)).kind == "decode")
    #expect(playerFailure(av(.fileFormatNotRecognized)).kind == "decode")
    #expect(playerFailure(av(.unknown)).kind == "other")
    let media = NSError(domain: "CoreMediaErrorDomain", code: -12939, userInfo: [NSLocalizedDescriptionKey: "gone"])
    #expect(playerFailure(media) == PlayerFailure(kind: "other", error: "gone"))
    #expect(playerFailure(nil).kind == "other")
}

@Test func aSourceThatFailsBeforeItsStartSeekKeepsTheStart() {
    #expect(stoppedPosition(pendingStart: 900, position: 0) == 900)
    #expect(stoppedPosition(pendingStart: nil, position: 912.5) == 912.5)
}

@Test func aFailoverContinuesWhereTheFailedSourceStopped() {
    #expect(sourceStart(stoppedAt: nil, resumeMs: 30_000) == 30)
    #expect(sourceStart(stoppedAt: 312.5, resumeMs: 30_000) == 312.5)
    #expect(sourceStart(stoppedAt: nil, resumeMs: nil) == 0)
}

@Test func onlyALandedStartSeekHandsOver() {
    #expect(startLanded(finished: true, sameItem: true, pending: 900) == 900)
    // An interrupted seek, or one for an item already replaced, keeps the start pending.
    #expect(startLanded(finished: false, sameItem: true, pending: 900) == nil)
    #expect(startLanded(finished: true, sameItem: false, pending: 900) == nil)
    // A viewer's seek already cleared it.
    #expect(startLanded(finished: true, sameItem: true, pending: nil) == nil)
}

@Test func aSeekInFlightHoldsUntilItLands() {
    // B seeks to its 900 s start; the viewer seeks to 1200 s; stale ticks say 1199.5 and 0; B fails.
    var pending: Double? = 1200
    var position = 1200.0
    for stale in [1199.5, 0, 900] {
        if let now = ticked(time: stale, pending: pending) { position = now }
    }
    #expect(position == 1200)
    #expect(stoppedPosition(pendingStart: pending, position: position) == 1200)
    // Only the seek's own completion hands over.
    #expect(startLanded(finished: true, sameItem: true, pending: pending) == 1200)
    pending = nil
    #expect(ticked(time: 1203, pending: pending) == 1203)
}

// Subtitle notes (plans/m13-turkcealtyazi.md 4.2).

@Test func eachSubtitleProblemHasItsWords() {
    let words = ["unreachable", "blocked", "changed", "trackFailed", "somethingNew"].map { SubtitleNote(source: "OpenSubtitles v3", problem: $0).words }
    #expect(words == [
        "OpenSubtitles v3 did not answer. Try its subtitles again in a moment.",
        "OpenSubtitles v3 is turning requests away. Marquee will try again in a few minutes.",
        "OpenSubtitles v3 changed its pages, so Marquee cannot read them. An update will fix this.",
        "A subtitle file from OpenSubtitles v3 could not be read. Pick another one.",
        "Subtitles from OpenSubtitles v3 did not load.",
    ])
}

@Test func aNewNoteIsSaidAndAFileThatLeftGoesOff() {
    let file = SubtitleTrack(url: "http://127.0.0.1:1/sub/t/1.vtt", lang: "tur", label: nil, source: "SubSense", forced: false, hearingImpaired: false)
    let other = SubtitleTrack(url: "http://127.0.0.1:1/sub/t/2.vtt", lang: "eng", label: nil, source: "SubSense", forced: false, hearingImpaired: false)
    let slow = SubtitleNote(source: "SubSense", problem: "unreachable")
    let gone = SubtitleNote(source: "SubSense", problem: "trackFailed")
    // Did not answer: said, and the file stays in the list and stays on.
    #expect(PlayerLayout.subtitleTrouble(picked: file, old: [], new: [slow], tracks: [file, other])
        == SubtitleTrouble(say: slow.words, off: false))
    // Could not be read: it left the list, so subtitles go off.
    #expect(PlayerLayout.subtitleTrouble(picked: file, old: [slow], new: [slow, gone], tracks: [other])
        == SubtitleTrouble(say: gone.words, off: true))
    // A note already said is not said again.
    #expect(PlayerLayout.subtitleTrouble(picked: file, old: [slow], new: [slow], tracks: [file]) == nil)
    // Nothing picked: nothing to say about it.
    #expect(PlayerLayout.subtitleTrouble(picked: nil, old: [], new: [gone], tracks: []) == nil)
    // A failover empties both lists without a note: the file stays picked.
    #expect(PlayerLayout.subtitleTrouble(picked: file, old: [slow], new: [], tracks: []) == nil)
    // After it, the same note counts as new again.
    #expect(SubtitleNote.added(from: [], to: [slow]) == [slow])
}

@Test func aPictureThatNeverComesFailsTheVersionOver() {
    // The sound runs past 4 s with no frame (an HEVC file tagged hev1): this version cannot be shown here.
    #expect(PlayerLayout.pictureMissing(shown: false, advanced: 4.5, playingFor: 0))
    #expect(PlayerLayout.pictureMissing(shown: false, advanced: 0, playingFor: 4.5))
    // Still starting: a frame usually comes within a second of the sound.
    #expect(!PlayerLayout.pictureMissing(shown: false, advanced: 3, playingFor: 3))
    // A shown frame is never a missing picture, however long it plays.
    #expect(!PlayerLayout.pictureMissing(shown: true, advanced: 600, playingFor: 600))
}

@Test func focusShowsOnlyForKeyboardNavigation() {
    // A click focuses the page (the player takes the keyboard): no control rings for it.
    #expect(!FocusRule.shown(focused: true, keyboardNavigation: false))
    #expect(FocusRule.shown(focused: true, keyboardNavigation: true))
    #expect(!FocusRule.shown(focused: false, keyboardNavigation: true))
}

private func sub(_ n: Int, _ lang: String, _ language: String, _ heading: String, _ name: String, source: String = "OpenSubtitles v3", hi: Bool = false) -> SubtitleTrack {
    SubtitleTrack(url: "http://127.0.0.1:1/sub/\(n).vtt", lang: lang, label: nil, source: source, forced: false, hearingImpaired: hi,
                  language: language, languageName: heading, name: name)
}

@Test func subtitleGroupsFollowTheCoreOrderWithTheProfilesLanguagesOnTop() {
    // The owner's case, in the core's order: Turkish preferred, English as audio, then the rest by name.
    let tracks = [
        sub(1, "tur", "tr", "Turkish", "TurkceAltyazi.org", source: "TurkceAltyazi.org"),
        sub(2, "eng", "en", "English", "OpenSubtitles v3 1"),
        sub(3, "eng", "en", "English", "Tenzing.2026.1080p.WEB", hi: true),
        sub(4, "zhc", "yue", "Chinese (Cantonese)", "OpenSubtitles v3"),
        sub(5, "zht", "zh-Hant", "Chinese (Traditional)", "OpenSubtitles v3"),
        sub(6, "mne", "cnr", "Montenegrin", "OpenSubtitles v3"),
        sub(7, "pob", "pt-BR", "Portuguese (Brazil)", "OpenSubtitles v3"),
        sub(8, "spl", "es-419", "Spanish (Latin America)", "OpenSubtitles v3"),
    ]
    let groups = SubtitleMenu.groups(tracks, preferred: ["tur", "en"])
    #expect(groups.top.map(\.heading) == ["Turkish", "English"])
    #expect(groups.other.map(\.language) == ["yue", "zh-Hant", "cnr", "pt-BR", "es-419"])
    #expect(groups.top[1].tracks.map(SubtitleMenu.title) == ["OpenSubtitles v3 1", "Tenzing.2026.1080p.WEB"])
    // The line already names the source: nothing beneath it. A release line gets its source and SDH.
    #expect(SubtitleMenu.detail(tracks[1]) == nil)
    #expect(SubtitleMenu.detail(tracks[2]) == "OpenSubtitles v3, For the hard of hearing")
    // A preference in any ISO 639 spelling counts for every region of its language.
    #expect(SubtitleMenu.groups(tracks, preferred: ["por"]).top.map(\.language) == ["pt-BR"])
    #expect(SubtitleMenu.base("ger") == SubtitleMenu.base("de-AT"))
    // None of the profile's languages here: every language is on top, nothing under Other languages.
    let none = SubtitleMenu.groups(tracks, preferred: ["fin"])
    #expect(none.top.count == 7 && none.other.isEmpty)
}

@Test func subtitleTracksFromAnOlderCoreStillGroupAndName() throws {
    let old = try JSONDecoder().decode(SubtitleTrack.self, from: Data(#"{"url": "u", "lang": "tur", "label": null, "source": "SubSense", "forced": true, "hearingImpaired": false}"#.utf8))
    #expect(old.language == nil && old.name == nil)
    #expect(SubtitleMenu.heading(old) == "Turkish")
    #expect(SubtitleMenu.title(old) == "SubSense")
    #expect(SubtitleMenu.detail(old) == "Forced")
    #expect(SubtitleMenu.groups([old], preferred: ["tr"]).top.map(\.language) == ["tur"])
    let new = try JSONDecoder().decode(SubtitleTrack.self, from: Data(#"{"url": "u", "lang": "pob", "label": null, "source": "OpenSubtitles v3", "forced": false, "hearingImpaired": false, "language": "pt-BR", "languageName": "Portuguese (Brazil)", "name": "OpenSubtitles v3 2"}"#.utf8))
    #expect(SubtitleMenu.heading(new) == "Portuguese (Brazil)")
    #expect(SubtitleMenu.title(new) == "OpenSubtitles v3 2")
}
