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
    // Start over begins at 0; a failover after it still resumes where the last source stopped.
    #expect(sourceStart(stoppedAt: nil, resumeMs: 30_000, fromStart: true) == 0)
    #expect(sourceStart(stoppedAt: 312.5, resumeMs: 30_000, fromStart: true) == 312.5)
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

private func phase(_ phase: String, _ extra: String = "") throws -> PlaybackSnapshot {
    let json = """
        {"phase":"\(phase)","url":null,"key":"k1","filename":null,"sizeBytes":null,"resumeMs":null,
         "attempts":[],"subtitles":[],"subtitlesPending":false,"next":null,"bingeGroup":null\(extra)}
        """
    return try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(json.utf8))
}

@Test func theCoreNamesTheNextVersionWhileItResolves() throws {
    let moving = try phase("resolving", #","trying":{"key":"k2","label":"720p WEB"}"#)
    #expect(moving.trying == OtherVersion(key: "k2", label: "720p WEB"))
    #expect(PlayerLayout.waitingWords(moving, notice: "This source failed: 404. Trying the next one.") == "Trying the next version: 720p WEB")
    // Once it is ready the name has done its job: the player's own words, or the plain wait.
    let ready = try phase("ready", #","trying":{"key":"k2","label":"720p WEB"}"#)
    #expect(PlayerLayout.waitingWords(ready, notice: nil) == "Getting the stream ready")
    // An older core names nothing.
    let older = try phase("resolving")
    #expect(older.trying == nil && older.offer == nil)
    #expect(PlayerLayout.waitingWords(older, notice: "This source failed: 404. Trying the next one.") == "This source failed: 404. Trying the next one.")
    #expect(PlayerLayout.waitingWords(nil, notice: nil) == "Getting the stream ready")
}

@Test func aFailedPlaybackCanOfferTheNextVersion() throws {
    let failed = try phase("failed", #","offer":{"key":"k3","label":"1080p WEB"}"#)
    #expect(failed.offer?.key == "k3")
    #expect(failed.offer?.label == "1080p WEB")
}

@Test func theSubtitlesButtonCountsWhileTheCoreStillLists() {
    // Nothing in yet: Searching, with no mark beside the word that says it.
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 0, pending: true) == ("Searching", false))
    // One is enough to show the count; the mark stays while the core lists.
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 87, pending: true) == ("Off, 87 found", true))
    // Done: the count alone, or None found.
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 87, pending: false) == ("Off, 87 found", false))
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 0, pending: false) == ("None found", false))
    // A picked track keeps its name, with the mark while more may come.
    #expect(PlayerLayout.subtitleValue(picked: "Turkish", found: 3, pending: true) == ("Turkish", true))
    #expect(PlayerLayout.subtitleValue(picked: "Turkish", found: 3, pending: false) == ("Turkish", false))
}

@Test func theFailureCardOffersTheNextVersionThenTryAgain() {
    #expect(PlayerLayout.failureActions(offer: true, canChoose: true) == [.offer, .retry, .choose, .back])
    // No version offered: Try again is first, the one that takes focus.
    #expect(PlayerLayout.failureActions(offer: false, canChoose: true) == [.retry, .choose, .back])
    #expect(PlayerLayout.failureActions(offer: false, canChoose: false) == [.retry, .back])
}

@Test func onlyADecoderThatCannotDecodeTheFileCountsAsDecode() {
    let busy = NSError(domain: AVFoundationErrorDomain, code: AVError.Code.decoderTemporarilyUnavailable.rawValue)
    #expect(playerFailure(busy).kind == "other")
    let cannot = NSError(domain: AVFoundationErrorDomain, code: AVError.Code.decodeFailed.rawValue)
    #expect(playerFailure(cannot).kind == "decode")
    #expect(playerFailure(NSError(domain: AVFoundationErrorDomain, code: AVError.Code.decoderNotFound.rawValue)).kind == "decode")
}

@Test func theStartSubtitleTurnsOnOnceUntilTheViewerPicks() throws {
    let turkish = sub(1, "tur", "tr", "Turkish", "TurkceAltyazi.org", source: "TurkceAltyazi.org")
    let english = sub(2, "eng", "en", "English", "OpenSubtitles v3")
    #expect(SubtitleMenu.autoPick(turkish.url, settled: false, tracks: [english, turkish]) == turkish)
    // The viewer picked (or the start subtitle already applied): nothing turns on.
    #expect(SubtitleMenu.autoPick(turkish.url, settled: true, tracks: [english, turkish]) == nil)
    // None from the core, or its track not listed yet.
    #expect(SubtitleMenu.autoPick(nil, settled: false, tracks: [english]) == nil)
    #expect(SubtitleMenu.autoPick(turkish.url, settled: false, tracks: [english]) == nil)
    let auto = try phase("ready", #","autoSubtitle":"http://127.0.0.1:1/sub/1.vtt""#)
    #expect(auto.autoSubtitle == "http://127.0.0.1:1/sub/1.vtt")
    #expect(try phase("ready").autoSubtitle == nil)
}

@Test func withNoLanguagesSetTheDevicesAndEnglishGoOnTop() {
    #expect(SubtitleMenu.preferred(subtitle: [], audio: [], device: "de") == ["de", "en"])
    #expect(SubtitleMenu.preferred(subtitle: ["tur"], audio: ["en"], device: "de") == ["tur", "en"])
    #expect(SubtitleMenu.preferred(subtitle: ["tur"], audio: [], device: nil) == ["tur", "en"])
    // The iPhone walk's case: none set on an English device, English on top, the rest under Other languages.
    let tracks = [sub(1, "eng", "en", "English", "OpenSubtitles v3"), sub(2, "ara", "ar", "Arabic", "OpenSubtitles v3")]
    let groups = SubtitleMenu.groups(tracks, preferred: SubtitleMenu.preferred(subtitle: [], audio: [], device: "en"))
    #expect(groups.top.map(\.language) == ["en"] && groups.other.map(\.language) == ["ar"])
}

@Test func theWaitingLineSitsOverControlsThatReachTheMiddle() {
    // A phone in landscape: the shown controls reach the middle, so the line sits just over them.
    #expect(PlayerLayout.statusOverControls(controlsShown: true, windowHeight: 402, controlsHeight: 290) == 298)
    // A Mac window: the controls stay low, the line stays centred.
    #expect(PlayerLayout.statusOverControls(controlsShown: true, windowHeight: 1000, controlsHeight: 200) == nil)
    // Controls hidden, or no height yet: centred.
    #expect(PlayerLayout.statusOverControls(controlsShown: false, windowHeight: 402, controlsHeight: 290) == nil)
    #expect(PlayerLayout.statusOverControls(controlsShown: true, windowHeight: 0, controlsHeight: 290) == nil)
}

@Test func theFailureHeadlineSaysOnlyThisVersionWhenAnotherIsOffered() {
    #expect(PlayerLayout.failureHeadline(offer: true) == "This version could not be played")
    #expect(PlayerLayout.failureHeadline(offer: false) == "None of the sources could be played")
}

@MainActor @Test func tryAgainAndSubtitlePicksSendTheirCommands() {
    let choices = PlayChoices()
    var sent: [[String: any Sendable]] = []
    choices.opened(sub: 7) { _, command in sent.append(command) }
    let turkish = sub(1, "tur", "tr", "Turkish", "TurkceAltyazi.org", source: "TurkceAltyazi.org")
    choices.retry()
    choices.chose(turkish)
    choices.chose(nil)
    #expect(sent.count == 3)
    #expect(sent[0]["type"] as? String == "retry")
    #expect(sent[1]["type"] as? String == "subtitleChosen" && sent[1]["url"] as? String == turkish.url)
    // Off is sent too, as a null url, so the title remembers it.
    #expect(sent[2]["type"] as? String == "subtitleChosen" && sent[2]["url"] is NSNull)
    // A track in the file settles the play and sends nothing.
    choices.choseInFile()
    #expect(sent.count == 3)
}

@MainActor @Test func anotherEpisodeOrVersionGetsItsStartSubtitleAgain() {
    let choices = PlayChoices()
    let turkish = sub(1, "tur", "tr", "Turkish", "TurkceAltyazi.org", source: "TurkceAltyazi.org")
    #expect(choices.startSubtitle(turkish.url, tracks: [turkish]) == turkish)
    // Once per play.
    #expect(choices.startSubtitle(turkish.url, tracks: [turkish]) == nil)
    // The next episode (or Play {offer}) starts over.
    choices.newPlay()
    #expect(choices.startSubtitle(turkish.url, tracks: [turkish]) == turkish)
    // A pick in that play still wins over its start subtitle.
    choices.newPlay()
    choices.chose(nil)
    #expect(choices.startSubtitle(turkish.url, tracks: [turkish]) == nil)
}

@Test func theOverlaySaysWhatIsHappening() {
    #expect(PlayerLayout.status(phase: "failed", cannotOpen: false, firstFrameShown: true, waiting: false) == .failed)
    // A view that could not open gets a card, not a spinner with the error as its line.
    #expect(PlayerLayout.status(phase: nil, cannotOpen: true, firstFrameShown: false, waiting: false) == .cannotOpen)
    // Try again after a frame was shown: the play starts over, and says so.
    #expect(PlayerLayout.status(phase: "resolving", cannotOpen: false, firstFrameShown: true, waiting: false) == .waiting(words: true))
    #expect(PlayerLayout.status(phase: "ready", cannotOpen: false, firstFrameShown: false, waiting: false) == .waiting(words: true))
    // A stall mid-play: the spinner alone.
    #expect(PlayerLayout.status(phase: "ready", cannotOpen: false, firstFrameShown: true, waiting: true) == .waiting(words: false))
    #expect(PlayerLayout.status(phase: "ready", cannotOpen: false, firstFrameShown: true, waiting: false) == PlayerStatus.none)
}

@Test func theFailureCardSaysChooseAVersionAsTheOtherApps() {
    #expect(PlayerLayout.failureLabel(.choose, offer: nil) == "Choose a version")
    #expect(PlayerLayout.failureLabel(.offer, offer: "SD WEB") == "Play SD WEB")
    #expect(PlayerLayout.failureLabel(.retry, offer: nil) == "Try again")
    #expect(PlayerLayout.failureLabel(.back, offer: nil) == "Back")
}

@Test func anAcceptedTryAgainSaysSoEvenWhenItFailsAgainAtOnce() throws {
    // retries rose within the view: Try again was taken.
    #expect(PlayerLayout.retryRose(old: 0, new: 1))
    #expect(!PlayerLayout.retryRose(old: 1, new: 1))
    // A view's first snapshot, or an older core without the field: nothing to say.
    #expect(!PlayerLayout.retryRose(old: nil, new: 2))
    #expect(!PlayerLayout.retryRose(old: 1, new: nil))
    // A refail keeps the card, with the line under its headline and Try again focused.
    #expect(PlayerLayout.status(phase: "failed", cannotOpen: false, firstFrameShown: true, waiting: false) == .failed)
    #expect(PlayerLayout.failureNote(tryingAgain: true) == "Trying again")
    #expect(PlayerLayout.failureNote(tryingAgain: false) == nil)
    // Focus is always the first: Play {offer} when offered, also after a Try again.
    #expect(PlayerLayout.failureFocus([.offer, .retry, .choose, .back]) == .offer)
    #expect(PlayerLayout.failureFocus([.retry, .choose, .back]) == .retry)
    // While the play resolves again, the spinner says it.
    #expect(PlayerLayout.waitingWords(try phase("resolving"), notice: nil, tryingAgain: true) == "Trying again")
    // At least 1.5 s, unless the play is ready first.
    #expect(PlayerLayout.tryingAgainHold == .milliseconds(1500))
    #expect(!PlayerLayout.keepsTryingAgain(showing: true, phase: "ready"))
    #expect(PlayerLayout.keepsTryingAgain(showing: true, phase: "failed"))
    #expect(try phase("failed", #","retries":2"#).retries == 2)
    #expect(try phase("failed").retries == nil)
}

private func route(_ key: String, sub: UInt64 = 3) -> PlayRoute {
    let target = PlayTargetInfo(kind: "movie", itemId: "tt1727587", videoId: "tt1727587", title: "Sintel", poster: nil, resumeMs: nil)
    return PlayRoute(versionsSub: sub, key: key, target: target, tapped: .now)
}

@MainActor @Test func commandsGoToThePlaybackViewOpenedLast() {
    let choices = PlayChoices()
    var sent: [(UInt64, String)] = []
    let deliver: (UInt64, [String: any Sendable]) -> Void = { sub, command in sent.append((sub, command["type"] as? String ?? "")) }
    choices.opened(sub: 1, deliver: deliver)
    choices.retry()
    // Play {offer} or the next episode opens another view: its commands never reach the old one.
    choices.opened(sub: 2, deliver: deliver)
    choices.retry()
    choices.chose(nil)
    #expect(sent.map(\.0) == [1, 2, 2])
    #expect(sent.map(\.1) == ["retry", "retry", "subtitleChosen"])
}

@MainActor @Test func switchingVersionOrEpisodeStartsThePlayOver() {
    let choices = PlayChoices()
    let turkish = sub(1, "tur", "tr", "Turkish", "TurkceAltyazi.org", source: "TurkceAltyazi.org")
    #expect(choices.startSubtitle(turkish.url, tracks: [turkish]) == turkish)
    // Play {offer}: a new route on the same Versions view, and the start subtitle applies again.
    let next = choices.switchVersion(from: route("k1"), to: "k2")
    #expect(next?.key == "k2" && next?.versionsSub == 3)
    #expect(choices.startSubtitle(turkish.url, tracks: [turkish]) == turkish)
    // The version already playing is no switch.
    #expect(choices.switchVersion(from: route("k2"), to: "k2") == nil)
    // The next episode: the viewer's Off in the last one does not carry over.
    choices.chose(nil)
    #expect(choices.nextEpisode(route("e2")).key == "e2")
    #expect(choices.startSubtitle(turkish.url, tracks: [turkish]) == turkish)
}

@MainActor @Test func aMoveOnDropsTheOldSubtitleAndTakesTheNewStartOne() {
    let choices = PlayChoices()
    let old = sub(1, "tur", "tr", "Turkish", "TurkceAltyazi.org 4K", source: "TurkceAltyazi.org")
    let new = sub(2, "tur", "tr", "Turkish", "TurkceAltyazi.org 1080p", source: "TurkceAltyazi.org")
    #expect(!choices.source(1))
    #expect(choices.startSubtitle(old.url, tracks: [old]) == old)
    // The same source: nothing changes.
    #expect(!choices.source(1))
    // The core moved on: the old file goes and the new source's start subtitle turns on.
    #expect(choices.source(2))
    #expect(choices.startSubtitle(new.url, tracks: [new]) == new)
    // The viewer's own pick goes too (its file was the old release's): the core's
    // start subtitle for the new list brings back the viewer's language.
    choices.chose(new)
    #expect(choices.source(3))
    #expect(choices.startSubtitle(old.url, tracks: [old]) == old)
    // Off stays Off: the core sends no start subtitle for it.
    choices.chose(nil)
    #expect(choices.source(4))
    #expect(choices.startSubtitle(nil, tracks: [old]) == nil)
}

@MainActor @Test func oneSnapshotDropsOnANewSourceThenTurnsOnItsStartSubtitle() {
    let choices = PlayChoices()
    let old = sub(1, "tur", "tr", "Turkish", "TurkceAltyazi.org 4K", source: "TurkceAltyazi.org")
    let new = sub(2, "tur", "tr", "Turkish", "TurkceAltyazi.org 1080p", source: "TurkceAltyazi.org")
    let first = choices.subtitleStep(attempt: 1, auto: old.url, tracks: [old])
    #expect(!first.drop && first.turnOn == old)
    let same = choices.subtitleStep(attempt: 1, auto: old.url, tracks: [old])
    #expect(!same.drop && same.turnOn == nil)
    let moved = choices.subtitleStep(attempt: 2, auto: new.url, tracks: [new])
    #expect(moved.drop && moved.turnOn == new)
}

@Test func aBusyLineNamesTheMoveOnBeforeTryingAgain() throws {
    let moving = try phase("resolving", #","trying":{"key":"k2","label":"1080p WEB"}"#)
    #expect(PlayerLayout.waitingWords(moving, notice: nil, tryingAgain: true) == "Trying the next version: 1080p WEB")
    #expect(PlayerLayout.waitingWords(try phase("resolving"), notice: nil, tryingAgain: true) == "Trying again")
}

@Test func theCannotOpenCardPutsAnotherVersionFirst() {
    #expect(PlayerLayout.cannotOpenActions(canChoose: true) == [.choose, .retry, .back])
    #expect(PlayerLayout.cannotOpenActions(canChoose: false) == [.retry, .back])
}

@Test func torrentDataIsLeftOutOfBackups() throws {
    let data = FileManager.default.temporaryDirectory.appending(path: "mq-backup-\(UUID().uuidString)", directoryHint: .isDirectory)
    defer { try? FileManager.default.removeItem(at: data) }
    // The core's folder: \`torrents\` in the data directory.
    let torrents = CoreClient.torrentDirectory(in: data)
    #expect(torrents.lastPathComponent == "torrents" && torrents.deletingLastPathComponent().standardizedFileURL == data.standardizedFileURL)
    CoreClient.excludeTorrentsFromBackup(dataDirectory: data)
    let values = try torrents.resourceValues(forKeys: [.isExcludedFromBackupKey, .isDirectoryKey])
    #expect(values.isDirectory == true)
    #expect(values.isExcludedFromBackup == true)
}

@Test func airPlayIsNotHandedALoopbackStream() throws {
    // The core serves every stream on 127.0.0.1: a receiver would ask itself for it.
    #expect(!PlayerLayout.allowsExternalPlayback(try #require(URL(string: "http://127.0.0.1:52011/s/token/Sintel.mp4"))))
    #expect(!PlayerLayout.allowsExternalPlayback(try #require(URL(string: "http://localhost:8080/v.mp4"))))
    #expect(!PlayerLayout.allowsExternalPlayback(try #require(URL(string: "http://[::1]:8080/v.mp4"))))
    // A stream another device can reach may go to AirPlay.
    #expect(PlayerLayout.allowsExternalPlayback(try #require(URL(string: "https://cdn.example.org/v.m3u8"))))
}

@Test func pictureInPictureSaysOnceThatAddonSubtitlesStayBehind() {
    // An addon's track is drawn over the page, which leaves as the window floats.
    #expect(PlayerLayout.pipSubtitleNote(addonSubtitle: true, told: false) == "Addon subtitles do not show in Picture in Picture")
    // Once per film, and nothing to say with no addon track on.
    #expect(PlayerLayout.pipSubtitleNote(addonSubtitle: true, told: true) == nil)
    #expect(PlayerLayout.pipSubtitleNote(addonSubtitle: false, told: false) == nil)
}

@Test func subtitleTimingMovesTheCuesForThisPlay() {
    let cues = WebVTT.parse("WEBVTT\n\n00:00:10.000 --> 00:00:12.000\nHello\n")
    // Two seconds late in the file: two presses of Earlier show the line at 8 s.
    let earlier = -2 * PlayerLayout.subtitleStep * 2
    #expect(WebVTT.text(at: PlayerLayout.cueTime(position: 8.5, delay: earlier), in: cues) == "Hello")
    #expect(WebVTT.text(at: PlayerLayout.cueTime(position: 8.5, delay: 0), in: cues) == nil)
    // Later holds a line back.
    #expect(WebVTT.text(at: PlayerLayout.cueTime(position: 10.5, delay: 1), in: cues) == nil)
    #expect(PlayerLayout.delayWords(0) == "On time")
    #expect(PlayerLayout.delayWords(1.5) == "1.5 s later")
    #expect(PlayerLayout.delayWords(-0.5) == "0.5 s earlier")
    #expect(PlayerLayout.delayWords(-2) == "2 s earlier")
}

@Test func theProfilesSubtitleSizeScalesTheDrawnLines() {
    #expect(CaptionStyle.factor(.medium) == 1)
    #expect(CaptionStyle.factor(.small) < 1)
    #expect(CaptionStyle.factor(.large) > 1)
    #expect(CaptionStyle.factor(.extraLarge) > CaptionStyle.factor(.large))
    #expect(SubtitleSize.allCases.map(CaptionStyle.words) == ["Small", "Medium", "Large", "Extra large"])
}

@Test func theLanguagePickerSearchesTheCoresFullList() {
    let all = [
        LanguageChoice(code: "en", name: "English"), LanguageChoice(code: "zh-Hans", name: "Chinese (Simplified)"),
        LanguageChoice(code: "zh-Hant", name: "Chinese (Traditional)"), LanguageChoice(code: "pt-BR", name: "Portuguese (Brazil)"),
        LanguageChoice(code: "pt", name: "Portuguese"), LanguageChoice(code: "ar", name: "Arabic"),
    ]
    // Subtitles keep each tag.
    #expect(LanguageChoice.rows(all, audio: false, query: "").map(\.code) == ["en", "zh-Hans", "zh-Hant", "pt-BR", "pt", "ar"])
    // Audio takes the base code once, named without its script or region.
    #expect(LanguageChoice.rows(all, audio: true, query: "").map(\.code) == ["en", "zh", "pt", "ar"])
    #expect(LanguageChoice.rows(all, audio: true, query: "").map(\.name) == ["English", "Chinese", "Portuguese", "Arabic"])
    // A search matches a name in part, or a code whole.
    #expect(LanguageChoice.rows(all, audio: false, query: "chin").map(\.code) == ["zh-Hans", "zh-Hant"])
    #expect(LanguageChoice.rows(all, audio: false, query: "AR").map(\.code) == ["ar"])
}

@Test func subtitleLinesChangeExactlyAtTheirEdges() {
    let cues = WebVTT.parse("WEBVTT\n\n00:00:10.000 --> 00:00:12.000\nOne\n\n00:00:12.000 --> 00:00:14.500\nTwo\n\n00:00:00.200 --> 00:00:01.000\nZero\n")
    // Each start and end once, in order, where the line changes.
    #expect(PlayerLayout.cueEdges(cues, delay: 0) == [0.2, 1, 10, 12, 14.5])
    // Shifted by the timing; an edge before the film's start is never reached.
    #expect(PlayerLayout.cueEdges(cues, delay: -0.5) == [0.5, 9.5, 11.5, 14])
    #expect(PlayerLayout.cueEdges([], delay: 1).isEmpty)
}

@Test func theFilesOwnTracksTakeTheProfilesSize() {
    // AVFoundation's relative font size, in percent; Medium leaves it alone.
    #expect(CaptionStyle.relativeFontSize(.medium) == nil)
    #expect(CaptionStyle.relativeFontSize(.small) == 80)
    #expect(CaptionStyle.relativeFontSize(.large) == 125)
    #expect(CaptionStyle.relativeFontSize(.extraLarge) == 150)
}

@MainActor @Test func aServerMoveKeepsTheSubtitlePicks() {
    let old = SubtitleTrack(url: "http://127.0.0.1:52011/sub/t/1.vtt", lang: "ger", label: "Sintel.de", source: "OpenSubtitles", forced: false, hearingImpaired: false)
    let other = SubtitleTrack(url: "http://127.0.0.1:61022/sub/u/2.vtt", lang: "spa", label: "Sintel.es", source: "OpenSubtitles", forced: false, hearingImpaired: false)
    let moved = SubtitleTrack(url: "http://127.0.0.1:61022/sub/u/1.vtt", lang: "ger", label: "Sintel.de", source: "OpenSubtitles", forced: false, hearingImpaired: false)
    // The same track on the new address: everything but its URL.
    #expect(SubtitleMenu.same(old, in: [other, moved]) == moved)
    #expect(SubtitleMenu.same(old, in: [other]) == nil)
    // The viewer's pick on source 1; the view opens again on the moved server
    // and numbers its source 2: nothing is dropped, and the start subtitle
    // does not come back over the pick.
    let choices = PlayChoices()
    _ = choices.source(1)
    choices.chose(old)
    choices.reopened()
    #expect(choices.subtitleStep(attempt: 2, auto: other.url, tracks: [other, moved]).drop == false)
    #expect(choices.startSubtitle(other.url, tracks: [other, moved]) == nil)
}
