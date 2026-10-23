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
    // Reading a menu, the last press 29 s ago, keeps it and the controls under it open.
    #expect(PlayerLayout.menuHolds(openedAt: opened, now: opened + .seconds(15)))
    #expect(PlayerLayout.menuHolds(openedAt: opened, now: opened + .seconds(29)))
    // Dismissed by a tap outside, which iOS does not report: the hold lets go after half a minute.
    #expect(!PlayerLayout.menuHolds(openedAt: opened, now: opened + .seconds(30)))
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

@Test func sourcesThatFailedAlikeAreSaidOnceInFirstOrderWithNoAddonName() throws {
    let gone = "The link no longer exists (404)"
    let slow = "The source did not answer in time"
    let attempts = [
        Attempt(key: "a", via: "Torrentio", error: slow),
        Attempt(key: "b", via: "Torrentio", error: gone),
        Attempt(key: "c", via: "Local", error: gone),
        Attempt(key: "d", via: "Torrentio", error: gone),
    ]
    let lines = PlayerLayout.failureLines(attempts)
    #expect(lines == [slow, "\(gone) · 3 sources"])
    #expect(!lines.joined().contains("Torrentio") && !lines.joined().contains("Local"))
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
        "This subtitle could not be read.",
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
    // No frame watch (the page away, a window without its own): no verdict on the version.
    #expect(!PlayerLayout.pictureMissing(shown: false, advanced: 30, playingFor: 30, watched: false))
    // The version's own failure on the page; in the window it fails over without the device's record.
    #expect(PlayerLayout.pictureFailureKind(floating: false) == "decode")
    #expect(PlayerLayout.pictureFailureKind(floating: true) != "decode")
}

@Test func aSourceThatLoadsOutOfSightWaitsForItsPicture() {
    // On the page only while the app is in front; in the window only with its own watch.
    #expect(PlayerLayout.pictureWatched(floating: false, windowWatches: false, onScreen: true))
    #expect(!PlayerLayout.pictureWatched(floating: false, windowWatches: false, onScreen: false), "the background is no place for a verdict")
    #expect(PlayerLayout.pictureWatched(floating: true, windowWatches: true, onScreen: false))
    #expect(!PlayerLayout.pictureWatched(floating: true, windowWatches: false, onScreen: false))
    // 30 s played in the background without a frame: back in front, the clocks start again.
    let start = ContinuousClock.now
    let away = PlayerLayout.pictureClocks(watched: false, position: 30, loadStart: 0, playingSince: start, now: start + .seconds(30))
    #expect(away.loadStart == 30)
    #expect(away.playingSince == start + .seconds(30))
    #expect(!PlayerLayout.pictureMissing(shown: false, advanced: 31 - away.loadStart, playingFor: 1), "a verdict a second after coming back")
    // In sight the clocks run on, and 4 s without a frame still fails the version.
    let seen = PlayerLayout.pictureClocks(watched: true, position: 30, loadStart: 0, playingSince: start, now: start + .seconds(30))
    #expect(seen.loadStart == 0 && seen.playingSince == start)
    // Paused away: nothing starts playing.
    #expect(PlayerLayout.pictureClocks(watched: false, position: 5, loadStart: 0, playingSince: nil, now: start).playingSince == nil)
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
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 0, pending: true, ready: true) == ("Searching", false))
    // One is enough to show the count; the mark stays while the core lists.
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 87, pending: true, ready: true) == ("Off, 87 found", true))
    // Done: the count alone, or None found.
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 87, pending: false, ready: true) == ("Off, 87 found", false))
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 0, pending: false, ready: true) == ("None found", false))
    // A picked track keeps its name, with the mark while more may come.
    #expect(PlayerLayout.subtitleValue(picked: "Turkish", found: 3, pending: true, ready: true) == ("Turkish", true))
    #expect(PlayerLayout.subtitleValue(picked: "Turkish", found: 3, pending: false, ready: true) == ("Turkish", false))
    // No stream yet (resolving, or no snapshot): the core has not asked, so not None found.
    #expect(PlayerLayout.subtitleValue(picked: nil, found: 0, pending: false, ready: false) == ("Searching", false))
    #expect(PlayerLayout.subtitleValue(picked: "German", found: 2, pending: false, ready: false) == ("German", true))
}

@Test func onlyADecoderThatCannotDecodeTheFileCountsAsDecode() {
    let busy = NSError(domain: AVFoundationErrorDomain, code: AVError.Code.decoderTemporarilyUnavailable.rawValue)
    #expect(playerFailure(busy).kind == "other")
    let cannot = NSError(domain: AVFoundationErrorDomain, code: AVError.Code.decodeFailed.rawValue)
    #expect(playerFailure(cannot).kind == "decode")
    #expect(playerFailure(NSError(domain: AVFoundationErrorDomain, code: AVError.Code.decoderNotFound.rawValue)).kind == "decode")
}

@MainActor @Test func theStartSubtitleTurnsOnOnceUntilTheViewerPicks() throws {
    let turkish = sub(1, "tur", "tr", "Turkish", "TurkceAltyazi.org", source: "TurkceAltyazi.org")
    let english = sub(2, "eng", "en", "English", "OpenSubtitles v3")
    // None from the core, or its track not listed yet: nothing.
    #expect(PlayChoices().startSubtitle(nil, tracks: [english]) == nil)
    #expect(PlayChoices().startSubtitle(turkish.url, tracks: [english]) == nil)
    let choices = PlayChoices()
    #expect(choices.startSubtitle(turkish.url, tracks: [english, turkish]) == turkish)
    // Already applied: nothing turns on again.
    #expect(choices.startSubtitle(turkish.url, tracks: [english, turkish]) == nil)
    // The viewer picked: nothing turns on.
    let picked = PlayChoices()
    picked.chose(english)
    #expect(picked.startSubtitle(turkish.url, tracks: [english, turkish]) == nil)
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
    // An iPad in a narrow window (the compact OSD, its transport and actions in the foot block):
    // the shown controls reach the middle, so the line sits just over them.
    #expect(PlayerLayout.statusOverControls(controlsShown: true, windowHeight: 600, controlsHeight: 290) == 298)
    // A Mac window: the controls stay low, the line stays centred.
    #expect(PlayerLayout.statusOverControls(controlsShown: true, windowHeight: 1000, controlsHeight: 200) == nil)
    // Controls hidden, or no height yet: centred.
    #expect(PlayerLayout.statusOverControls(controlsShown: false, windowHeight: 600, controlsHeight: 290) == nil)
    #expect(PlayerLayout.statusOverControls(controlsShown: true, windowHeight: 0, controlsHeight: 290) == nil)
    // The iPhone's foot block (scrubber and actions, 85 pt) never reaches the middle: the transport
    // is there instead, so the spinner turns in its Play circle and the words sit 8 pt over it.
    #expect(PlayerLayout.statusOverControls(controlsShown: true, windowHeight: 402, controlsHeight: 85) == nil)
    for (height, safeBottom) in [(CGFloat(402), CGFloat(21)), (874, 34)] {
        let padding = PlayerLayout.statusOverTransport(windowHeight: height, safeBottom: safeBottom)
        let wordsBottom = height - safeBottom - padding
        #expect(wordsBottom == height / 2 - PhoneFrame.transportHalfHeight - 8, "\(height): the words' foot over the transport")
    }
    #expect(PlayerLayout.statusOverTransport(windowHeight: 402, safeBottom: 21) == 222)
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
    // Two seconds late in the file: Earlier by 2 s shows the line at 8 s.
    let earlier = -20 * PlayerLayout.subtitleStep
    #expect(WebVTT.text(at: PlayerLayout.cueTime(position: 8.5, delay: earlier), in: cues) == "Hello")
    #expect(WebVTT.text(at: PlayerLayout.cueTime(position: 8.5, delay: 0), in: cues) == nil)
    // Later holds a line back.
    #expect(WebVTT.text(at: PlayerLayout.cueTime(position: 10.5, delay: 1), in: cues) == nil)
    // Signed, never "earlier" or "later" (the owner).
    #expect(PlayerLayout.delayWords(0) == "0.0 seconds")
    #expect(PlayerLayout.delayWords(1.5) == "Plus 1.5 seconds")
    #expect(PlayerLayout.delayWords(-0.5) == "Minus 0.5 seconds")
    #expect(PlayerLayout.delayWords(-2) == "Minus 2.0 seconds")
}

@Test func theTimingStepperMovesAsStremiosDoes() {
    // A press is 0.1 s; a hold repeats from 350 ms, every 100 ms, 0.3 s a tick in the first second,
    // then 0.4 and 0.5, and 0.6 from the third second on.
    #expect(PlayerLayout.subtitleStep == 0.1)
    #expect(PlayerLayout.firstRepeatMs == 350 && PlayerLayout.repeatMs == 100 && PlayerLayout.delaySettleMs == 700)
    #expect([350, 999, 1000, 2500, 3000, 9000].map { PlayerLayout.holdStep(heldMs: $0) }.map { ($0 * 10).rounded() / 10 } == [0.3, 0.3, 0.4, 0.5, 0.6, 0.6])
    // Snapped to tenths, within a minute either way, never a negative zero.
    #expect(PlayerLayout.nudge(0.25, by: 0.1) == 0.4)
    #expect(PlayerLayout.nudge(59.8, by: 0.6) == 60)
    #expect(PlayerLayout.nudge(-59.9, by: -0.6) == -60)
    #expect(PlayerLayout.nudge(0.1, by: -0.1).sign == .plus)
    #expect(PlayerLayout.delayText(0.3) == "+0.3 s")
    #expect(PlayerLayout.delayText(-1.2) == "\u{2212}1.2 s")
    #expect(PlayerLayout.delayText(0) == "0.0 s")
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

@Test func aFilmBroughtBackFromPictureInPicturePlaysOn() {
    // Playing at restore, paused once the window closed (AVKit): it plays again.
    #expect(PlayerLayout.resumesAfterRestore(playingAtRestore: true, playingNow: false))
    // Paused in the window: it stays paused; already playing: nothing to do.
    #expect(!PlayerLayout.resumesAfterRestore(playingAtRestore: false, playingNow: false))
    #expect(!PlayerLayout.resumesAfterRestore(playingAtRestore: true, playingNow: true))
}

@Test func thePhonePlayerTurnsAcrossOnAPlaysFirstAppearOnly() {
    // A new play opens across.
    #expect(PlayerLayout.opensAcross(lastAcross: nil))
    // Back from Versions or the window: the way it was when its page left.
    #expect(!PlayerLayout.opensAcross(lastAcross: false), "a viewer watching upright is turned across again")
    #expect(PlayerLayout.opensAcross(lastAcross: true))
}

@Test func thePhonePlayerSitsWhereTheSystemPlayerDoes() {
    // Measured on the system player, iPhone 17 upright: 402 x 874, the safe
    // area 62 on top and 34 at the foot, none at the sides.
    #expect(0 + PhoneFrame.side(safe: 0) == 10, "the bars 10 pt from the screen")
    #expect(62 + PhoneFrame.topGap == 74, "the top bar")
    #expect(874 - 34 - PhoneFrame.bottomGap == 835, "the scrubber block's foot")
    #expect(PhoneFrame.transportGap(landscape: false) == 36)
    // Across: 874 x 402, the safe area 62 at each side, 21 at the foot, none on top.
    #expect(62 + PhoneFrame.side(safe: 62) == 62, "the bars at the safe area")
    #expect(0 + PhoneFrame.topGap == 12)
    #expect(402 - 21 - PhoneFrame.bottomGap == 376)
    #expect(PhoneFrame.transportGap(landscape: true) == 68)
    // The scrubber's capsule and its times' inset.
    #expect(PhoneFrame.barHeight == 38)
    #expect(PhoneFrame.barInset == 11)
}

@Test func drawnSubtitlesSitLowNeverAtTheTop() {
    // Across, the picture fills the height; the foot block (scrubber and actions) is 92 pt over the safe area.
    let across = PlayerLayout.subtitleFoot(screenHeight: 402, pictureHeight: 402, safeBottom: 21, controlsShown: true, controlsHeight: 92)
    #expect(across == 273, "over the scrubber block and the home indicator")
    #expect(across > 402 / 2, "in the lower half")
    #expect(PlayerLayout.subtitleFoot(screenHeight: 402, pictureHeight: 402, safeBottom: 21, controlsShown: false, controlsHeight: 92) == 357)
    // Upright, at the picture's foot, well over the scrubber at the screen's foot.
    #expect(PlayerLayout.subtitleFoot(screenHeight: 874, pictureHeight: 226, safeBottom: 34, controlsShown: true, controlsHeight: 92) == 534)
    // A Mac window: no safe area, as before.
    #expect(PlayerLayout.subtitleFoot(screenHeight: 900, pictureHeight: 900, safeBottom: 0, controlsShown: false, controlsHeight: 200) == 860)
}

@Test func aSubtitleClearsThePhoneControlsDownToItsFloor() {
    // iPhone 17; the foot block (capsule, 6 pt, the actions row, the foot gap) is 85 pt over the safe area.
    let block: CGFloat = PhoneFrame.barHeight + 6 + 36 + PhoneFrame.bottomGap
    let shapes: [(height: CGFloat, safeBottom: CGFloat, picture: CGFloat)] = [(402, 21, 402), (874, 34, 226)]
    for shape in shapes {
        let scrubberTop = shape.height - shape.safeBottom - block
        let transport = (shape.height / 2 - PhoneFrame.transportHalfHeight)...(shape.height / 2 + PhoneFrame.transportHalfHeight)
        let pictureBottom = (shape.height + shape.picture) / 2
        for size in SubtitleSize.allCases {
            for lines in [1, 2] {
                // The drawn line's box at full size: its lines (20 pt on the phone at the system caption size) and 6 pt above and below.
                let box = CGFloat(lines) * (20 * CaptionStyle.factor(size) * 1.2) + 12
                for shown in [true, false] {
                    let placed = PhoneFrame.subtitle(
                        screenHeight: shape.height, pictureHeight: shape.picture, safeBottom: shape.safeBottom,
                        controlsShown: shown, scrubberTop: scrubberTop, boxHeight: box)
                    let top = placed.foot - box * placed.scale
                    if shown {
                        // Clear of the transport, unless it is already at its floor.
                        #expect(top >= transport.upperBound || placed.scale == PhoneFrame.subtitleFloor, "\(shape.height) \(size) \(lines) lines meets the transport")
                        #expect(placed.foot <= scrubberTop - PhoneFrame.subtitleGap, "\(shape.height) \(size) \(lines) lines meets the scrubber")
                    }
                    #expect(placed.scale >= PhoneFrame.subtitleFloor && placed.scale <= 1)
                    if shape.height > shape.picture * 2 {
                        // Upright, the black band under the picture holds it: just under the picture, full size.
                        #expect(placed.foot == pictureBottom + PhoneFrame.bandGap + box, "\(size) \(lines) lines not in the band")
                        #expect(placed.scale == 1)
                    }
                }
            }
        }
    }
    // Across, no band: the foot sits 8 pt over the capsule; one Medium line keeps its size.
    let across = PhoneFrame.subtitle(screenHeight: 402, pictureHeight: 402, safeBottom: 21, controlsShown: true, scrubberTop: 296, boxHeight: 36)
    #expect(across.foot == 288)
    #expect(across.scale == 1)
    // Upright two Medium lines go under the picture (550), controls up or not.
    for shown in [true, false] {
        let upright = PhoneFrame.subtitle(screenHeight: 874, pictureHeight: 226, safeBottom: 34, controlsShown: shown, scrubberTop: 755, boxHeight: 60)
        #expect(upright.foot == 618)
        #expect(upright.scale == 1)
    }
    // A tall picture leaves no band that holds the box: the foot inside the picture, as before.
    let tall = PhoneFrame.subtitle(screenHeight: 874, pictureHeight: 600, safeBottom: 34, controlsShown: false, scrubberTop: 755, boxHeight: 72)
    #expect(tall.foot == 711)
    #expect(tall.scale == 1)
}

@Test func aSubtitleNeverShrinksPastItsFloor() {
    // The owner's iPhone 13 Pro across, the controls up: 43 pt between the transport and the capsule.
    let twoExtraLarge = PhoneFrame.subtitle(screenHeight: 390, pictureHeight: 390, safeBottom: 21, controlsShown: true, scrubberTop: 284, boxHeight: 84)
    #expect(twoExtraLarge.foot == 276)
    #expect(twoExtraLarge.scale == PhoneFrame.subtitleFloor, "51% before the floor")
    #expect(PhoneFrame.subtitleFloor == 0.75)
    let oneMedium = PhoneFrame.subtitle(screenHeight: 390, pictureHeight: 390, safeBottom: 21, controlsShown: true, scrubberTop: 284, boxHeight: 36)
    #expect(oneMedium.scale == 1)
}

@Test func skipAndUpNextKeepTheSubtitleOverThem() {
    // Across, the controls hidden: Skip (64 pt, its foot at 357) lifts the line 8 pt over it.
    let hidden = PhoneFrame.subtitle(screenHeight: 402, pictureHeight: 402, safeBottom: 21, controlsShown: false, scrubberTop: 296, boxHeight: 60, pillTop: 293)
    #expect(hidden.foot == 285)
    #expect(hidden.scale == 1)
    #expect(PhoneFrame.subtitle(screenHeight: 402, pictureHeight: 402, safeBottom: 21, controlsShown: false, scrubberTop: 296, boxHeight: 60).foot == 347)
    // Across, the controls up: over Skip it would meet the transport, so it stays on the capsule.
    let shown = PhoneFrame.subtitle(screenHeight: 402, pictureHeight: 402, safeBottom: 21, controlsShown: true, scrubberTop: 296, boxHeight: 36, pillTop: 224)
    #expect(shown.foot == 288)
    #expect(shown.scale == 1)
    // Upright with a tall picture and the controls up, the room is there: over Skip.
    let tall = PhoneFrame.subtitle(screenHeight: 874, pictureHeight: 600, safeBottom: 34, controlsShown: true, scrubberTop: 755, boxHeight: 72, pillTop: 683)
    #expect(tall.foot == 675)
    #expect(tall.scale == 1)
    // Upright in the band, Skip lower down changes nothing.
    let band = PhoneFrame.subtitle(screenHeight: 874, pictureHeight: 226, safeBottom: 34, controlsShown: true, scrubberTop: 755, boxHeight: 60, pillTop: 683)
    #expect(band.foot == 618)
}

@Test func aSubtitlesDetailSaysTheCoresMarksAsTheWebAndAndroidDo() {
    func track(_ name: String, fits: Bool? = nil, hash: Bool? = nil, fps: Double? = nil, release: String? = nil, hi: Bool = false) -> SubtitleTrack {
        var t = sub(1, "eng", "en", "English", name, hi: hi)
        (t.fits, t.hashMatch, t.madeForFps, t.release) = (fits, hash, fps, release)
        return t
    }
    // A hash match says so before the group; a file timed for another frame rate says which, as written.
    #expect(SubtitleMenu.detail(track("OpenSubtitles v3", fits: true, hash: true)) == "Matches this file")
    #expect(SubtitleMenu.detail(track("OpenSubtitles v3", fits: true)) == "Same release group")
    #expect(SubtitleMenu.detail(track("OpenSubtitles v3", fps: 25)) == "Made for 25 fps")
    #expect(SubtitleMenu.detail(track("OpenSubtitles v3", fps: Double(Float(23.976)))) == "Made for 23.976 fps")
    // The release, cut in the middle at a dot with its group kept; said once when it is the name.
    let release = "Slow.Horses.S06E02.1080p.WEB.H264-CAKES"
    #expect(SubtitleMenu.middle(release, max: 34) == "Slow.Horses\u{2026}1080p.WEB.H264-CAKES")
    #expect(SubtitleMenu.detail(track("CAKES, WEB 1080p", fits: true, release: release)) == "OpenSubtitles v3, Same release group, \(release)")
    #expect(SubtitleMenu.detail(track(release, release: release)) == "OpenSubtitles v3")
    let raw = "Slow.Horses.S06E02.Daddy.Issues.1080p.ATVP.WEB-DL.DDP5.1.Atmos.H.264-FLUX"
    let cut = SubtitleMenu.middle(raw, max: 46)
    #expect(cut.count <= 46 && cut.hasPrefix("Slow.Horses") && cut.hasSuffix("-FLUX") && cut.contains("\u{2026}"), "\(cut)")
    #expect(SubtitleMenu.middle("short.name", max: 46) == "short.name")
}

@Test func aTracksTimingStartsWhereTheCoreKeptItAndSettlesAsOneCommand() throws {
    var track = sub(1, "eng", "en", "English", "Walk Clips")
    #expect(PlayerLayout.startDelay(nil) == 0)
    #expect(PlayerLayout.startDelay(track) == 0)
    track.delayMs = 300
    #expect(PlayerLayout.startDelay(track) == 0.3)
    // To the tenth the stepper shows: 40 ms is 0.0 s, with no Reset.
    track.delayMs = 40
    #expect(PlayerLayout.startDelay(track) == 0)
    track.delayMs = 260
    #expect(PlayerLayout.startDelay(track) == 0.3)
    track.delayMs = 300
    // Earlier in this play wins: the core keeps one track's delay, the play each track's.
    #expect(PlayerLayout.startDelay(track, kept: [track.url: -1.5]) == -1.5)
    #expect(PlayerLayout.startDelay(track, kept: ["other": 2]) == 0.3)
    let command = PlayerLayout.delayCommand(url: track.url, seconds: -1.2)
    #expect(command["type"] as? String == "subtitleDelay")
    #expect(command["url"] as? String == track.url)
    #expect(command["ms"] as? Int == -1200)
    // The core's own field, as it sends it.
    let decoded = try JSONDecoder().decode(SubtitleTrack.self, from: Data(#"{"url": "u", "lang": "eng", "label": null, "source": "Walk", "forced": false, "hearingImpaired": false, "hashMatch": true, "madeForFps": 25.0, "delayMs": -500}"#.utf8))
    #expect(decoded.hashMatch == true && decoded.madeForFps == 25 && decoded.delayMs == -500)
}

@MainActor @Test func theStartSubtitleFollowsTheCoreWhenItsFileCannotBeRead() throws {
    // Two broken tracks, then a good one: the core moves `autoSubtitle` on each time, with no note, and
    // the player turns each new one on in the last one's place while the viewer has not chosen.
    let broken1 = sub(1, "eng", "en", "English", "Subs A")
    let broken2 = sub(2, "eng", "en", "English", "Subs B")
    let good = sub(3, "eng", "en", "English", "Subs C")
    let choices = PlayChoices()
    var tracks = [broken1, broken2, good]
    var step = choices.subtitleStep(attempt: 1, auto: broken1.url, tracks: tracks)
    #expect(step.turnOn == broken1 && !step.drop)
    tracks.removeFirst()
    step = choices.subtitleStep(attempt: 1, auto: broken2.url, tracks: tracks)
    #expect(step.turnOn == broken2 && !step.drop)
    tracks.removeFirst()
    step = choices.subtitleStep(attempt: 1, auto: good.url, tracks: tracks)
    #expect(step.turnOn == good && !step.drop)
    // It stays: the same auto again turns nothing on.
    #expect(choices.subtitleStep(attempt: 1, auto: good.url, tracks: tracks).turnOn == nil)
    // None left: the one shown goes off, and the neutral line names no source.
    let gone = PlayChoices()
    _ = gone.subtitleStep(attempt: 1, auto: broken1.url, tracks: [broken1])
    let last = gone.subtitleStep(attempt: 1, auto: nil, tracks: [])
    #expect(last.drop && last.turnOn == nil)
    let failed = try phase("ready", #","startSubtitleFailed":true"#)
    // The shared player.subtitlesFailed word, which lands with the core's subtitle hand-over.
    #expect(PlayerLayout.startSubtitleFailedWords(failed) == StreamChoice.words.player.subtitlesFailed)
    #expect(PlayerLayout.startSubtitleFailedWords(try phase("ready")) == nil)
    // The viewer's own pick is never moved or turned off by the core's start subtitle.
    let viewer = PlayChoices()
    _ = viewer.subtitleStep(attempt: 1, auto: broken1.url, tracks: [broken1, good])
    viewer.chose(broken1)
    let after = viewer.subtitleStep(attempt: 1, auto: good.url, tracks: [good])
    #expect(after.turnOn == nil && !after.drop)
    #expect(!viewer.subtitleStep(attempt: 1, auto: nil, tracks: []).drop)
    // A server move renames the same track: it stays on as it was, not turned on again.
    let moved = PlayChoices()
    _ = moved.subtitleStep(attempt: 1, auto: good.url, tracks: [good])
    moved.reopened()
    let renamed = sub(4, "eng", "en", "English", "Subs C")
    #expect(moved.subtitleStep(attempt: 2, auto: renamed.url, tracks: [renamed]).turnOn == nil)
}

/// Review apple-aether P2-2: a resume at 12:00 on the second engine, whose clock begins at 0, fails before its first
/// frame: the start stays pending through the engine's ticks, and the next version starts at 12:00.
@Test func aResumeOnTheEngineSurvivesItsFailure() {
    let pending: Double? = 720
    var position = 720.0
    for tick in [0, 0.1, 0.25] {
        if let now = ticked(time: tick, pending: pending) { position = now }
    }
    #expect(stoppedPosition(pendingStart: pending, position: position) == 720)
}

/// Review apple-aether r2 P2: the wiring of the resume on the engine, read from the source (the model needs a live
/// player): the start stays pending from the open to the first frame, and the engine's ticks pass through `ticked`.
@Test func theEngineKeepsTheStartPendingUntilItsFirstFrame() throws {
    let text = try playerModel()
    #expect(try body(of: "private func openOnEngine(", in: text).contains("pendingStart = start > 1 ? start : nil"))
    let first = try body(of: "private func engineFirstFrame(", in: text)
    let cleared = try #require(first.range(of: "pendingStart = nil"))
    let shown = try #require(first.range(of: "firstFrame()"))
    #expect(cleared.lowerBound < shown.lowerBound, "cleared before the first frame is told")
    #expect(try body(of: "private func engineTick(", in: text).contains("ticked(time: time, pending: pendingStart)"))
}
