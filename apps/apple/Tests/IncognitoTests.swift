import AppKit
import Foundation
import SwiftUI
import Testing

// Incognito on Apple (plans/batch-10-incognito.md 7.3): the face, the tile and the menu row, the lock line after Leave,
// Settings, storage and Trakt, the cover, the subtitle and art sessions, trailers, scores, and the Mac's idle sender
// and warning. Every rule here takes Incognito as an argument: the app's switch (`Incognito.on`) is never flipped, so
// no test running beside these sees it.

private func profile(_ id: String, pin: Bool = false, owner: Bool = false, kids: Bool = false) -> ProfileInfo {
    ProfileInfo(id: id, name: id.capitalized, avatar: "", position: 0, hasPin: pin, isOwner: owner, syncing: false, kids: kids)
}

private func snapshot(_ profiles: [ProfileInfo], active: String? = nil, locked: Bool = false, host: String? = nil,
                      pin: String? = nil) -> ProfilesSnapshot {
    ProfilesSnapshot(profiles: profiles, active: active ?? profiles[0].id, locked: locked, max: 6,
                     incognitoHost: host, incognitoPin: pin)
}

private let main = profile("main", owner: true)

@Test func theSnapshotReadsTheContractFieldsAndTheFaceIsIncognito() throws {
    let json = #"""
    {"profiles":[{"id":"main","name":"Main","avatar":"cat:sky","position":0,"hasPin":true,"isOwner":true,"syncing":false,"kids":false}],
     "active":"incognito","locked":false,"max":6,"incognitoHost":"main","incognitoPin":null,"incognitoEnded":false}
    """#
    let s = try JSONDecoder().decode(ProfilesSnapshot.self, from: Data(json.utf8))
    #expect(s.incognito && s.incognitoHost == "main" && s.incognitoEnded == false)
    // The app face (2.8): every place that shows the current profile shows Incognito, never the host.
    #expect(s.current == Incognito.profile)
    #expect(s.current?.name == "Incognito" && s.current?.avatar == "incognito" && s.current?.hasPin == false)
    #expect(s.current?.isOwner == false && s.current?.kids == false)
    #expect(!s.profiles.contains { $0.id == Incognito.id })
    // Outside Incognito the face is the active profile, as before.
    #expect(snapshot([main]).current == main)
}

@Test func theMarkIsTheHatGlassesOnSea() {
    #expect(ProfileRules.parseAvatar(Incognito.id) == .init(glyph: "incognito", color: "incognito"))
    // A stored avatar can never name it: real ids and avatars are the profiles' own.
    #expect(ProfileRules.parseAvatar("incognito:sky").glyph == "letter")
    let catalog = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee/Resources/Icons.xcassets")
    #expect(FileManager.default.fileExists(atPath: catalog.appending(path: "hat-glasses.imageset/hat-glasses.svg").path()))
}

@Test func thePhoneTabAndTheBarSayIncognito() {
    #expect(Incognito.tabWord(Incognito.profile) == "Incognito")
    #expect(Incognito.tabWord(main) == "Profile")
    let inside = snapshot([main], active: Incognito.id, host: "main")
    // ProfileMenu's label on every bar (the iOS tab, the Mac ornament's end, the tvOS bar).
    #expect("Profile: \(inside.current?.name ?? "")" == Incognito.spoken)
}

@Test func theTileAndTheRowOpenTheKeypadOnlyWhenTheCoreAsks() {
    // No PIN asked: the switch.
    #expect(Incognito.choice(snapshot([main])) == .switchTo)
    // The core names whose PIN: the keypad, with that profile.
    let asked = snapshot([main, profile("kid", kids: true)], active: "kid2", pin: "main")
    #expect(Incognito.choice(asked) == .pin)
    #expect(Incognito.pinProfile(asked) == main)
    #expect(Incognito.pinTitle("Main") == "Enter the PIN for Main to open Incognito")
    // Running: the tile or the row only closes.
    #expect(Incognito.choice(snapshot([main], active: Incognito.id, host: "main")) == .close)
}

@Test func theKeypadSaysWhyOnlyWhenTheKidsRuleAsked() {
    let kid = profile("kid", kids: true)
    #expect(Incognito.kidsReason(snapshot([main, kid], active: "main", pin: "main")))
    // Locked: the PIN is asked because the session is, not by the kids rule.
    #expect(!Incognito.kidsReason(snapshot([main, kid], active: "main", locked: true, pin: "main")))
    // No kids profile, or no PIN asked: no line.
    #expect(!Incognito.kidsReason(snapshot([main], pin: "main")))
    #expect(!Incognito.kidsReason(snapshot([main, kid])))
}

@Test func aKidsSessionIsOfferedNoIncognito() {
    let kid = profile("kid", kids: true)
    #expect(!Incognito.offered(snapshot([main, kid], active: "kid")))
    #expect(Incognito.offered(snapshot([main, kid], active: "main")))
    #expect(Incognito.offered(snapshot([main], active: Incognito.id, host: "main")))
}

@Test func theStartPickerIgnoresIncognito() {
    // One profile: Home, also behind a running scope's host (Incognito is never among the profiles).
    #expect(!ProfileRules.startsOnPicker(snapshot([main], active: Incognito.id, host: "main")))
    #expect(ProfileRules.startsOnPicker(snapshot([main, profile("ada")], active: Incognito.id, host: "main")))
}

@Test func theChooserAfterLeaveShowsNoLockLineForAHostWithoutAPin() {
    // A Leave from a chooser entry locks the host; with no PIN there is nothing to unlock with.
    #expect(!ProfileRules.lockLine(snapshot([main], locked: true)))
    #expect(ProfileRules.lockLine(snapshot([profile("main", pin: true, owner: true)], locked: true)))
    #expect(!ProfileRules.lockLine(snapshot([profile("main", pin: true, owner: true)])))
}

@Test func theRefusalsReadAsWordsWithNoSpaceWords() {
    #expect(ProfileRules.problem("noSpace", fallback: "x") == "Couldn't open Incognito. Try again.")
    #expect(ProfileRules.problem("incognitoFailed", fallback: "x") == "Couldn't open Incognito. Try again.")
    #expect(ProfileRules.problem("incognito", fallback: "x") == "Not available in Incognito.")
    #expect(KeypadResult.of(code: "noSpace", retryAfterMs: nil, message: "disk full") == .problem(Incognito.couldNotOpen))
}

@Test func theIntroSaysWhatIsKeptPerForm() {
    let phone = Incognito.intro(debrid: false, form: .phone)
    #expect(phone.notKept == ["What you watch and search.", "Addons and settings you change."])
    #expect(phone.seen == ["Downloads stay on this device, and every profile can see them.",
                           "Addons, your internet provider and torrent peers can still see what you play."])
    #expect(phone.ends == "It ends when you leave, close the app, or after 30 minutes away.")
    let mac = Incognito.intro(debrid: true, form: .mac)
    #expect(mac.seen[1] == "Your debrid account keeps its own history.")
    #expect(mac.ends == "It ends when you leave, close the window, or after 30 minutes without use.")
    #expect(Incognito.intro(debrid: false, form: .tv).ends == "It ends when you leave, or after 30 minutes away.")
}

@Test func settingsInIncognitoHidesTheProfilesTheAccountAndDownloads() {
    let titles = SettingsSection.shown(tv: false, downloads: true, incognito: true).map(\.title)
    #expect(titles == ["Sources", "Home", "Playback", "Subtitles and audio", "This device", "About"])
    #expect(SettingsSection.shown(tv: true, downloads: false, incognito: true).map(\.title)
        == ["Sources", "Home", "Playback", "Subtitles and audio", "About"])
    // Outside it, as before.
    #expect(SettingsSection.shown(tv: false, downloads: true).contains(.account))
}

@Test func theIncognitoFolderIsLeftOutOfBackups() throws {
    let dir = FileManager.default.temporaryDirectory.appending(path: "mq-test-\(UUID().uuidString)", directoryHint: .isDirectory)
    defer { try? FileManager.default.removeItem(at: dir) }
    CoreClient.excludeFromBackup(dataDirectory: dir)
    let folder = CoreClient.incognitoDirectory(in: dir)
    #expect(folder.lastPathComponent == "incognito")
    #expect(try folder.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup == true)
}

@Test func theCoverShowsInTheBackgroundAndWhenInactiveOnlyWithoutThePlayer() {
    #expect(Incognito.covers(.background, playerOnScreen: false))
    #expect(Incognito.covers(.background, playerOnScreen: true))
    #expect(Incognito.covers(.inactive, playerOnScreen: false))
    #expect(!Incognito.covers(.inactive, playerOnScreen: true))
    #expect(!Incognito.covers(.active, playerOnScreen: false))
}

@Test func theSubtitleAndArtFetchesKeepNothingOnDiskInIncognito() {
    let subtitles = Incognito.subtitleSession(incognito: true)
    #expect(subtitles !== URLSession.shared)
    #expect((subtitles.configuration.urlCache?.diskCapacity ?? 0) == 0)
    #expect(Incognito.subtitleSession(incognito: false) === URLSession.shared)
    let art = ArtLoader.session(incognito: true)
    #expect(art.configuration.urlCache !== ArtLoader.cache)
    #expect((art.configuration.urlCache?.diskCapacity ?? 0) == 0)
    #expect(ArtLoader.session(incognito: false).configuration.urlCache === ArtLoader.cache)
}

@MainActor @Test func noTrailerPlaysInIncognitoAndNoWebViewIsMade() {
    let player = TrailerPlayer()
    let state = TrailerState()
    var ended: [String] = []
    player.play("abcdefghijk", into: state, incognito: true) { ended.append($0) }
    #expect(ended == ["incognito"])
    #expect(!state.loading)
    #expect(player.webView == nil)
}

@MainActor @Test func theHeroAndTheTitleHeaderHoldTheirPreviewsInIncognito() {
    // They ask the signals first: Incognito holds them.
    let signals = TrailerSignals.now(visible: true, reducedMotion: false, hasTrailer: true, incognito: true)
    #expect(signals.blocks.contains("incognito"))
    #expect(!signals.allowed)
}

@Test func theTrailerButtonIsGoneInIncognito() {
    #expect(!Incognito.trailerShown(hasTrailers: true, offline: false, incognito: true))
    #expect(Incognito.trailerShown(hasTrailers: true, offline: false, incognito: false))
}

@Test func scoresStayAsPlainTextWithNoOutsideLink() {
    let scores = TitleScores(tmdb: .shown("87%", url: "https://www.themoviedb.org/movie/278"), trakt: .shown("92%", url: "https://trakt.tv/movies/x"))
    let inside = scoreItems(imdb: "8.4", scores: scores, imdbUrl: "https://www.imdb.com/title/tt0111161/", incognito: true)
    #expect(inside.map(\.text) == ["8.4/10", "87%", "92%"])
    #expect(inside.allSatisfy { $0.url == nil })
    // The spoken score names no service page.
    #expect(inside[0].spoken == "IMDb 8.4 out of 10")
    let outside = scoreItems(imdb: "8.4", scores: scores, imdbUrl: "https://www.imdb.com/title/tt0111161/", incognito: false)
    #expect(outside.allSatisfy { $0.url != nil })
    #expect(outsideLink(URL(string: "https://www.imdb.com/"), incognito: true) == nil)
}

@MainActor @Test func traktNeverBeginsInIncognito() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let defaults = try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)"))
    let model = TraktModel(core: core.client, keystore: FakeTraktKeystore().keystore, defaults: defaults)
    await model.begin(profile: Incognito.id)
    #expect(model.session == nil)
    await model.begin(profile: "default")
    #expect(model.session != nil)
    model.end()
}

@Test func theMacGoesIdleAfterAMinuteWithoutInputOrVideo() {
    let start = ContinuousClock.now
    var clock = IdleClock(now: start)
    #expect(clock.tick(at: start + .seconds(59), playing: false) == nil)
    #expect(clock.tick(at: start + .seconds(60), playing: false) == "idle")
    // Once per change.
    #expect(clock.tick(at: start + .seconds(90), playing: false) == nil)
    #expect(clock.input(at: start + .seconds(91)) == "active")
    #expect(clock.input(at: start + .seconds(92)) == nil)
    // A playing video is use: no idle under it, and the minute counts from where it stopped.
    #expect(clock.tick(at: start + .seconds(200), playing: true) == nil)
    #expect(clock.tick(at: start + .seconds(259), playing: false) == nil)
    #expect(clock.tick(at: start + .seconds(260), playing: false) == "idle")
    // Hidden or the screens asleep: idle at once, unless a video plays.
    var hidden = IdleClock(now: start)
    #expect(hidden.away(playing: true) == nil)
    #expect(hidden.away(playing: false) == "idle")
}

@MainActor @Test func theMacWakeIsSentAsActiveOnlyInIncognito() {
    var sent: [String] = []
    var running = true
    let idle = IncognitoIdle(running: { running }, warned: { false }, stay: {}, send: { sent.append($0) })
    NSWorkspace.shared.notificationCenter.post(name: NSWorkspace.didWakeNotification, object: nil)
    #expect(sent == ["active"])
    running = false
    NSWorkspace.shared.notificationCenter.post(name: NSWorkspace.didWakeNotification, object: nil)
    #expect(sent == ["active"])
    withExtendedLifetime(idle) {}
}

@MainActor @Test func anyInputAnswersTheWarningAsItsButtonDoes() {
    var sent: [String] = []
    var stayed = 0
    let idle = IncognitoIdle(running: { true }, warned: { true }, stay: { stayed += 1 }, send: { sent.append($0) })
    idle.input()
    #expect(stayed == 1)
    #expect(sent.isEmpty)
}

@MainActor @Test func theWarningShowsItsLineAndAStayButton() {
    var stayed = 0
    let host = NSHostingView(rootView: IncognitoEndsSoon(stay: { stayed += 1 }))
    host.frame = CGRect(x: 0, y: 0, width: 700, height: 120)
    host.layoutSubtreeIfNeeded()
    #expect(Incognito.endsSoon == "Incognito ends in 1 minute.")
    #expect(Incognito.stay == "Stay in Incognito")
    #expect(host.fittingSize.width > 0)
    withExtendedLifetime(host) {}
    #expect(stayed == 0)
}

// The entry and its refusals on a real core (the contract: Incognito answers incognitoFailed until the scope exists).
@Suite(.serialized) @MainActor struct IncognitoSessionTests {
    private func device() async throws -> (ProfileSession, RealCore) {
        let core = RealCore()
        #expect(await core.ready())
        let sync = SyncModel(core: core.client, records: FakeKeychain().record,
                             defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        let session = ProfileSession(core: core.client, sync: sync)
        #expect(await eventually { session.snapshot != nil })
        return (session, core)
    }

    @Test func aRefusedEntryShowsItsWordsAndOpensNothing() async throws {
        let (session, core) = try await device()
        let snapshot = try #require(session.snapshot)
        #expect(snapshot.incognitoEnded == false && snapshot.incognitoHost == nil)
        #expect(!(await session.enterIncognito(pin: nil, fromChooser: true)))
        #expect(session.switchProblem?.code == "incognitoFailed")
        #expect(ProfileRules.problem(session.switchProblem?.code, fallback: "") == Incognito.couldNotOpen)
        #expect(!session.incognito && !session.intro && !Incognito.on)
        session.cancelSwitch()
        withExtendedLifetime(core) {}
    }

    @Test func theEndedNoticeIsReadWithItsOwnOp() async throws {
        let (session, core) = try await device()
        // The OK of the notice over a single profile's Home.
        let read = try await core.client.call("ackIncognitoEnded", as: ProfilesSnapshot.self)
        #expect(read.incognitoEnded == false)
        session.ackEnded()
        withExtendedLifetime(core) {}
    }
}
