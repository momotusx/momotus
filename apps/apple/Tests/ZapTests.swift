import Foundation
import Testing

// Zap's rules on Apple (plans/zap.md 2.2, 2.3): the tries, the hidden name, and the zap fields every route copy keeps.

private let episodePick = #"""
    {"kind":"series","itemId":"tt0903747","videoId":"tt0903747:2:5","season":2,"episode":5,"name":"Breaking Bad",
     "poster":"https://img/p.jpg","episodeName":"Breakage","startMs":1260000,"from":"near"}
    """#

private func pick(_ json: String = episodePick) throws -> ZapPick {
    try JSONDecoder().decode(ZapPick.self, from: Data(json.utf8))
}

@Test func theCoresPickDecodesWithItsEpisode() throws {
    let p = try pick()
    #expect(p.itemId == "tt0903747" && p.videoId == "tt0903747:2:5")
    #expect(p.season == 2 && p.episode == 5 && p.episodeName == "Breakage" && p.startMs == 1_260_000)
    let film = try pick(#"{"kind":"movie","itemId":"tt0111161","videoId":"tt0111161","season":null,"episode":null,"name":"The Shawshank Redemption","poster":null,"episodeName":null,"startMs":3000000,"from":"far"}"#)
    #expect(film.season == nil && film.episodeName == nil)
}

/// Every press starts at 0 tries; only a play that did not start adds one, and the third such play ends on
/// "Nothing played." instead of a fourth zap.
@Test func triesStartAtZeroFromEveryPressAndRunOutAtThree() {
    #expect(ZapRules.nextTries(.press, after: 0) == 0)
    #expect(ZapRules.nextTries(.press, after: 2) == 0)
    #expect(ZapRules.nextTries(.auto, after: 0) == 1)
    #expect(ZapRules.nextTries(.auto, after: 1) == 2)
    #expect(ZapRules.nextTries(.auto, after: 2) == nil)
}

@Test func theNameHidesInAZapUntilWhatIsThis() {
    #expect(ZapRules.nameShown(zapAt: nil, revealed: false))
    #expect(!ZapRules.nameShown(zapAt: 1_260_000, revealed: false))
    #expect(ZapRules.nameShown(zapAt: 1_260_000, revealed: true))
}

/// The pick's target carries the real name for the core, and the zap fields beside `resumeMs`, so another
/// version, the pending pick and the late pick (all copies of the route's target) keep them.
@MainActor @Test func everyRouteCopyKeepsTheZapFields() throws {
    let target = ZapRules.target(try pick(), tries: 2)
    #expect(target.title == "Breaking Bad" && target.resumeMs == nil)
    #expect(target.zapAt == 1_260_000 && target.zapTries == 2)
    #expect(target.season == 2 && target.episode == 5)

    let pending = PlayRoute(versionsSub: 7, key: "", target: target, tapped: .now)
    var late = PlayRoute(versionsSub: 7, key: "b", target: pending.target, tapped: .now)
    late.fromStart = pending.fromStart
    #expect(late.target.zapAt == 1_260_000 && late.target.zapTries == 2)

    let playing = PlayRoute(versionsSub: 7, key: "a", target: target, tapped: .now)
    let other = try #require(PlayChoices().switchVersion(from: playing, to: "b"))
    #expect(other.target.zapAt == 1_260_000 && other.target.zapTries == 2)
}

@Test func startFromTheBeginningIsAPlainPlay() throws {
    var target = ZapRules.target(try pick(), tries: 1)
    target.runtime = "47 min"
    let plain = ZapRules.plain(target)
    #expect(plain.zapAt == nil && plain.zapTries == 0)
    #expect(plain.videoId == target.videoId && plain.title == target.title && plain.runtime == "47 min")
}

/// Before the first frame a failure, no pick, a view that did not open or the core's wait for another version is
/// one try; after it nothing zaps by itself.
@Test func onlyAPlayThatNeverStartedZapsOn() {
    for status in [PlayerStatus.failed, .noPick, .cannotOpen] {
        #expect(ZapRules.failed(firstFrame: false, status: status, chooseAnother: false))
        #expect(!ZapRules.failed(firstFrame: true, status: status, chooseAnother: false))
    }
    #expect(ZapRules.failed(firstFrame: false, status: .waiting(words: true), chooseAnother: true))
    #expect(!ZapRules.failed(firstFrame: false, status: .waiting(words: true), chooseAnother: false))
    #expect(!ZapRules.failed(firstFrame: true, status: .waiting(words: false), chooseAnother: true))
    #expect(!ZapRules.failed(firstFrame: false, status: .none, chooseAnother: false))
}

@Test func zapsWordsAreTheSpecs() {
    #expect(ZapRules.missLine(code: "noAddons") == VersionWords.emptyLine("noSource"))
    #expect(ZapRules.missLine(code: "nothingCameUp") == "Nothing came up.")
    #expect(ZapRules.missLine(code: "failed") == nil)
    #expect(!ZapRules.missOffersAgain(code: "noAddons") && ZapRules.missOffersAgain(code: "nothingCameUp"))
    #expect(ZapDial.allCases.map(ZapRules.spoken) == ["Same: what you watch", "Near: titles like yours", "Far: what you never open"])
    #expect(ZapRules.episodeLine(season: 2, episode: 5, name: "Breakage") == "S2:E5 · Breakage")
    #expect(ZapRules.episodeLine(season: 2, episode: 5, name: nil) == "S2:E5")
    #expect(ZapRules.episodeLine(season: nil, episode: nil, name: "x") == nil)
}

private func source(_ path: String) throws -> String {
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    return try String(contentsOf: app.appending(path: path), encoding: .utf8)
}

/// A zap brought back from Picture in Picture has lost its zap screen: zap mode and What is this? come from the
/// play's own target, whose `zapAt` survives the restore.
@Test func aZapBroughtBackFromPictureInPictureStaysAZap() throws {
    let target = ZapRules.target(try pick(), tries: 1)
    let named = ZapRules.pick(target)
    #expect(named.name == "Breaking Bad" && named.season == 2 && named.episode == 5 && named.startMs == 1_260_000)
    #expect(ZapRules.episodeLine(season: named.season, episode: named.episode, name: named.episodeName) == "S2:E5")
    let model = try source("Player/PlayerModel.swift")
    #expect(model.contains("var zapPick: ZapPick? { zap ? zapOrigin?.pick ?? ZapRules.pick(route.target) : nil }"))
    let screen = try source("Player/PlayerScreen.swift")
    #expect(screen.contains("private var zapping: Bool { model.zap }"), "zap mode hangs on the zap screen, which Picture in Picture closes")
    #expect(screen.contains("        stage\n        .environment(\\.zapHost, zapHost)"), "the controls and end card get no zap host without the zap screen")
    #expect(screen.contains("zap?(kind: model.zapOrigin?.kind)"), "Zap again does nothing in a film brought back")
    let root = try source("App/RootView.swift")
    let cover = try #require(root.range(of: "fullScreenCover(item: Bindable(floating).shown)"))
    #expect(root[cover.lowerBound...].prefix(600).contains(".environment(\\.zap, zapAction(tab))"), "the brought-back film cannot zap again")
}

/// A zap that played and then failed offers the next zap, Try again and Back: never another version or the offer,
/// whose release names carry the title.
@Test func aZapsFailureCardOffersNoOtherVersion() throws {
    #expect(ZapRules.failureActions == [.zapAgain, .retry, .back])
    #expect(PlayerLayout.failureLabel(.zapAgain, offer: "Breaking.Bad.S02E05.1080p") == "Zap again")
    let screen = try source("Player/PlayerScreen.swift")
    #expect(screen.contains("let order = zapping ? ZapRules.failureActions : order"), "a zap's card offers another version")
    #expect(screen.contains("zapAgain: { zapHost?.again(.press) }"))
}

/// The subtitle picker's detail line drops the release text while the name is hidden.
@Test func aHiddenNameHidesTheSubtitlesReleaseText() throws {
    var track = SubtitleTrack(
        url: "http://127.0.0.1:1/sub/1.vtt", lang: "eng", label: nil, source: "OpenSubtitles", forced: false, hearingImpaired: true,
        language: "en", languageName: "English", name: "CAKES, WEB 1080p")
    track.release = "Breaking.Bad.S02E05.Breakage.1080p.WEB.x264-CAKES"
    #expect(SubtitleMenu.detail(track)?.contains("Breaking.Bad") == true)
    #expect(SubtitleMenu.detail(track, release: false) == "For the hard of hearing")
    let picker = try source("Player/SubtitlePicker.swift")
    #expect(picker.contains("SubtitleMenu.detail(t, max: 40, release: model.nameShown)"), "the picker shows the release text in a zap")
}

/// Back while the core picks, or a failure reporting twice: no player after the viewer left, one auto zap per play.
@Test func aLeftOrRepeatedZapOpensNothing() throws {
    #expect(ZapRules.counts(.press, playing: false))
    #expect(ZapRules.counts(.auto, playing: true))
    #expect(!ZapRules.counts(.auto, playing: false))
    let zap = try source("Player/Zap.swift")
    #expect(zap.contains("as: ZapPick.self)\n            // Back while the core picked: no player, and no versions asked, after the viewer left.\n            guard !Task.isCancelled else { return }"),
            "a pick that lands after Back opens a player")
    #expect(zap.contains("guard ZapRules.counts(ask, playing: playing) else { return }"))
    // A miss's Zap again is a press: its tries start at 0.
    #expect(zap.contains("Button { zap(.press) } label: { Label(\"Zap again\", glyph: \"zap\") }"))
}

/// Open title on the zap screen: the player ends without leaving the page, and the shell's push takes the zap off
/// the place in the same update as it pushes the title, so a dismiss never races the push (the Mac with a title page
/// open under the zap) and Back from the title never lands on a zap that replays from its start.
@Test func openTitleSwapsTheZapForTheTitleInOneUpdate() throws {
    let screen = try source("Player/PlayerScreen.swift")
    #expect(screen.contains("            if zapAgain != nil { end() } else { close() }\n            openTitle(route)"),
            "Open title dismisses the zap screen in the same turn as the push")
    let root = try source("App/RootView.swift")
    let push = try #require(root.range(of: "private func push(_ route: TitleRoute, on item: AppTab) {"))
    #expect(root[push.upperBound...].prefix(80).contains("zaps[item] = nil"), "the zap stays on the stack under the title")
}

/// Zap again on the zap screen runs the player's close path before the next zap replaces it: a zap past its hold
/// reports its progress and schedules a sync.
@Test func zapAgainClosesThePlayerFirst() throws {
    let screen = try source("Player/PlayerScreen.swift")
    #expect(screen.contains("            if let zapAgain {\n                end()\n                zapAgain(ask)"), "the old player is dropped without close()")
    let end = try #require(screen.range(of: "private func end() {"))
    let body = screen[end.upperBound...].prefix(300)
    #expect(body.contains("model.close()") && body.contains("sync.schedule()"))
}

/// The dial waits for the profile's prefs, and zaps only once its new value is saved; a failed save keeps the old
/// dial and this play. It takes no second press while a save is in flight.
@Test func theDialZapsOnlyAfterItsValueSaved() throws {
    let model = try source("Player/PlayerModel.swift")
    #expect(model.contains("var zapDial: ZapDial? { prefs.snapshot.map { $0.zapDial ?? .near } }"), "the dial reads Near before the prefs load")
    #expect(model.contains(#"func setZapDial(_ dial: ZapDial) async -> Bool { await prefs.perform("setPrefs", ["patch": ["zapDial": dial.rawValue]]) }"#),
            "the dial zaps before its value is saved")
    let controls = try source("Player/PlayerControls.swift")
    #expect(controls.contains("if await model.setZapDial(dial) { zapHost.again(.press) }"), "a failed save still zaps")
    let zap = try source("Player/Zap.swift")
    #expect(zap.contains(".disabled(selected == nil || saving)"), "the dial takes a press before its prefs or during a save")
}

/// Start from the beginning before the pick lands would let the pick bring zap mode back: it waits for the pick.
@Test func startFromTheBeginningWaitsForThePick() throws {
    #expect(ZapRules.startsOver(zap: true, waitsForPick: false))
    #expect(!ZapRules.startsOver(zap: true, waitsForPick: true))
    #expect(!ZapRules.startsOver(zap: false, waitsForPick: false))
    let model = try source("Player/PlayerModel.swift")
    #expect(model.contains("guard ZapRules.startsOver(zap: zap, waitsForPick: route.waitsForPick) else { return }"))
    let controls = try source("Player/PlayerControls.swift")
    #expect(controls.contains(".disabled(!ZapRules.startsOver(zap: model.zap, waitsForPick: model.route.waitsForPick))"))
}

/// The version the core moves on to is a release name: it stays out of the waiting line while a zap hides the title.
@Test func theNextVersionsNameHidesInAZap() throws {
    let json = #"""
        {"phase":"resolving","url":null,"key":"k1","filename":null,"sizeBytes":null,"resumeMs":null,
         "attempts":[],"subtitles":[],"subtitlesPending":false,"next":null,"bingeGroup":null,
         "trying":{"key":"k2","label":"Breaking.Bad.S02E05.1080p"}}
        """#
    let moving = try JSONDecoder().decode(PlaybackSnapshot.self, from: Data(json.utf8))
    #expect(PlayerLayout.waitingWords(moving, notice: nil) == "Trying the next version: Breaking.Bad.S02E05.1080p")
    #expect(PlayerLayout.waitingWords(moving, notice: nil, nameShown: false) == "Trying the next version")
    let screen = try source("Player/PlayerScreen.swift")
    #expect(screen.contains("findingPick: model.route.waitsForPick,\n            nameShown: model.nameShown))"), "the player never passes the hidden name")
}

/// A file's own track title can carry the release name: while the name is hidden each in-file audio and subtitle
/// track shows its language only, else its place; What is this? brings the file's names back.
@Test func theFilesTrackTitlesHideInAZap() throws {
    let names = ["Movie.Name.2019.eng", "Movie.Name.2019.ger.DTS", "Commentary"]
    let languages: [String?] = ["eng", "de", nil]
    #expect(TrackPicker.shownNames(names, languages: languages, audio: true, nameShown: true) == names)
    let hidden = TrackPicker.shownNames(names, languages: languages, audio: true, nameShown: false)
    #expect(hidden[0] == "English" && hidden[1] == "German")
    #expect(hidden[2] == TrackPicker.placeName(index: 2, count: 3, audio: true))
    #expect(TrackPicker.shownNames(["Movie.Name.2019.forced"], languages: [], audio: false, nameShown: false) == ["Subtitles"])
    let model = try source("Player/PlayerModel.swift")
    #expect(model.contains("TrackPicker.shownNames(audioTitles, languages: audioTrackLanguages, audio: true, nameShown: nameShown)"),
            "the audio strip and picker show the file's track titles in a zap")
    #expect(model.contains("TrackPicker.shownNames(textTitles, languages: textTrackLanguages, audio: false, nameShown: nameShown)"),
            "the subtitle picker and the Mac menu show the file's track titles in a zap")
}

/// Esc on the Mac leaves the zap's loading and quiet screens, as their Back does.
@Test func escapeLeavesTheZapsOwnScreens() throws {
    let zap = try source("Player/Zap.swift")
    #expect(zap.components(separatedBy: ".keyboardShortcut(.cancelAction)").count - 1 == 2, "Esc does nothing on a zap screen")
}

/// SwiftUI's wiring is not run here: these pin the lines that hand the rules to the player and the shell.
@Test func thePlayerAndTheShellWireZapThroughTheRules() throws {
    let model = try source("Player/PlayerModel.swift")
    #expect(model.contains(#"if let zapAt = target.zapAt { params["zapAt"] = zapAt }"#), "the playback view never hears the zap's start")
    #expect(model.contains("var nameShown: Bool { ZapRules.nameShown(zapAt: route.target.zapAt, revealed: revealed) }"))
    let screen = try source("Player/PlayerScreen.swift")
    #expect(screen.contains(".onChange(of: zapping && model.zapFailed) { _, failed in if failed { zapHost?.again(.auto) } }"),
            "a zap that did not start shows a card instead of zapping on")
    #expect(screen.contains("PlayerControls(model: model, versions: zapping ? nil : model.versions"), "Versions shows in a zap")
    #expect(screen.contains("episodes: zapping || model.route.target.season == nil ? nil"), "Episodes shows in a zap")
    let controls = try source("Player/PlayerControls.swift")
    #expect(controls.contains(#"Text(model.nameShown ? target.title : "Zap")"#), "the top line names a zap")
    #expect(controls.contains("if model.nameShown, let season = target.season, let episode = target.episode {"), "the S:E line shows in a zap")
    let zap = try source("Player/Zap.swift")
    #expect(zap.contains("guard let tries = ZapRules.nextTries(ask, after: tries) else {"))
    let root = try source("App/RootView.swift")
    #expect(root.contains("guard profiles.snapshot?.current?.kids != true else { return nil }"), "a kids profile gets Zap")
    #expect(root.contains("return { zap(kind: tab == .discover ? discoverKind : nil) }"), "the Mac's Zap sends a kind off Discover")
}
