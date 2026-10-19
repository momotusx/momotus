import Foundation
import Testing

// Trakt on Apple (plans/m7-trakt.md section 3): the words against the shared
// file, the card's rules, and the record's round trip through the real
// TraktModel on a real core pointed at a fake Trakt (a debug core reads
// MQ_DEV_TRAKT; the core library must be built with apps/apple/scripts/build-core.sh --dev).

private typealias W = TraktWords

private func status(_ json: String) throws -> TraktStatus {
    try JSONDecoder().decode(TraktStatus.self, from: Data(json.utf8))
}

@Suite struct TraktTests {
    @Test func theWordsAreTheSharedOnes() throws {
        let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared/trakt-words.json")
        let shared = try JSONDecoder().decode([String: String].self, from: Data(contentsOf: url))
        #expect(W.table == shared)
    }

    @Test func eachStateHasItsWordsAndItsWayOn() throws {
        let clock: (Int64) -> String = { _ in "9:41" }
        let connected = try status(#"{"state":"connected","user":"ana","pendingWrites":0,"unmapped":[]}"#)
        #expect(TraktRules.card(connected, clock: clock).line == "Connected as ana")
        #expect(TraktRules.card(connected, clock: clock).actions == [.syncNow, .signOut])
        let failing = try status(#"{"state":"failing","sinceMs":1,"reason":"no answer","lastError":"502","pendingWrites":2,"unmapped":[]}"#)
        #expect(TraktRules.card(failing, clock: clock).line.hasPrefix("Trakt has not answered since 9:41: no answer."))
        #expect(TraktRules.card(failing, clock: clock).detail == "Last error: 502")
        let reconnect = try status(#"{"state":"reconnect","reason":"the sign-in was revoked","pendingWrites":0,"unmapped":[]}"#)
        #expect(TraktRules.card(reconnect, clock: clock).actions == [.signInAgain, .signOut])
        #expect(TraktRules.card(try status(#"{"state":"off","pendingWrites":0,"unmapped":[]}"#), clock: clock).actions == [.connect])
        #expect(TraktRules.card(try status(#"{"state":"thisDeviceOff","pendingWrites":0,"unmapped":[]}"#), clock: clock).actions == [.signInHere])
        // A build without Trakt offers nothing to press.
        #expect(TraktRules.card(try status(#"{"state":"notConfigured","pendingWrites":0,"unmapped":[]}"#), clock: clock).actions.isEmpty)
        #expect(TraktRules.rowLine(nil, clock: clock) == W.rowOff)
        #expect(TraktRules.rowLine(connected, clock: clock) == "Connected as ana")
        #expect(TraktRules.pendingLine(0) == nil && TraktRules.pendingLine(1) == W.pendingOne && TraktRules.pendingLine(3) == "3 changes waiting to go to Trakt")
    }

    @Test func aSignInSaysWhereItStandsAndOffersANewCodeOnlyWhereItHelps() throws {
        let decode = { (json: String) in try JSONDecoder().decode(TraktLoginSnapshot.self, from: Data(json.utf8)) }
        let code = try decode(#"{"phase":"code","userCode":"AB12","url":"https://trakt.tv/activate","qr":"https://trakt.tv/activate","expiresMs":600000}"#)
        #expect(code == .code(userCode: "AB12", url: "https://trakt.tv/activate", qr: "https://trakt.tv/activate", expiresMs: 600_000))
        #expect(TraktRules.loginLine(try decode(#"{"phase":"done","user":"ana"}"#)) == "Connected as ana")
        for retry in [#"{"phase":"expired"}"#, #"{"phase":"denied"}"#, #"{"phase":"failed","reason":"x"}"#] {
            #expect(TraktRules.loginRetry(try decode(retry)))
        }
        let notConfigured = try decode(#"{"phase":"notConfigured"}"#)
        #expect(!TraktRules.loginRetry(notConfigured) && !TraktRules.loginRetry(code))
        #expect(TraktRules.timeLeft(expiresMs: 245_000, nowMs: 0) == "4:05")
        #expect(TraktRules.timeLeft(expiresMs: 0, nowMs: 5_000) == "0:00")
        #expect(TraktRules.spokenCode("AB12") == "Code A B 1 2")
    }

    @Test func eachEndOfASignInIsSaidOnce() throws {
        let decode = { (json: String) in try JSONDecoder().decode(TraktLoginSnapshot.self, from: Data(json.utf8)) }
        #expect(TraktRules.loginAnnouncement(try decode(#"{"phase":"done","user":"ana"}"#)) == "Connected as ana")
        #expect(TraktRules.loginAnnouncement(try decode(#"{"phase":"expired"}"#)) == W.expired)
        #expect(TraktRules.loginAnnouncement(try decode(#"{"phase":"denied"}"#)) == W.denied)
        #expect(TraktRules.loginAnnouncement(try decode(#"{"phase":"failed","reason":"no answer"}"#)) == "The sign-in did not finish: no answer.")
        #expect(TraktRules.loginAnnouncement(try decode(#"{"phase":"notConfigured"}"#)) == W.notConfigured)
        // Starting says nothing; the code is said by itself.
        #expect(TraktRules.loginAnnouncement(try decode(#"{"phase":"starting"}"#)) == nil)
        #expect(TraktRules.loginAnnouncement(try decode(#"{"phase":"code","userCode":"A","url":"u","qr":"q","expiresMs":1}"#)) == nil)
        #expect(TraktRules.loginAnnouncement(nil) == nil)
    }

    @Test func aPauseOrAPlayReachesTraktAndBufferingDoesNot() {
        #expect(PlayerLayout.traktPaused(paused: true, buffering: false, was: false) == true)
        #expect(PlayerLayout.traktPaused(paused: false, buffering: false, was: true) == false)
        // Buffering is neither a pause nor a play; no change says nothing.
        #expect(PlayerLayout.traktPaused(paused: false, buffering: true, was: false) == nil)
        #expect(PlayerLayout.traktPaused(paused: false, buffering: true, was: true) == nil)
        #expect(PlayerLayout.traktPaused(paused: true, buffering: false, was: true) == nil)
    }

    @Test func aNewKitIsHeldByTheIdTheCoreAnswers() throws {
        let made = try JSONDecoder().decode(MadeKit.self, from: Data(#"{"kit":"AAAA-BBBB","id":"k9","record":{"revision":1}}"#.utf8))
        #expect(made.held == HeldKit(id: "k9", kit: "AAAA-BBBB"))
        // A core that answers no id still hands over the kit: its id is unknown, the kit is not lost.
        let older = try JSONDecoder().decode(MadeKit.self, from: Data(#"{"kit":"CCCC","record":{"revision":1}}"#.utf8))
        #expect(older.held == HeldKit(id: "", kit: "CCCC"))
    }

    @Test func thePlayerMarkSaysAWarningOnScreen() {
        #expect(TraktRules.playbackMark(.on)! == (false, W.playerOn))
        #expect(TraktRules.playbackMark(.unmapped)! == (true, W.playerUnmapped))
        #expect(TraktRules.playbackMark(.thisDeviceOff)! == (true, W.playerThisDeviceOff))
        #expect(TraktRules.playbackMark(.off) == nil && TraktRules.playbackMark(nil) == nil)
    }

    @Test func theScreensNeverReadTheRecord() throws {
        let full = try status(#"{"state":"connected","pendingWrites":0,"unmapped":[],"record":{"accessToken":"secret"}}"#)
        #expect(full.record != nil)
        #expect(full.withoutRecord().record == nil)
        #expect(!"\(full.withoutRecord())".contains("secret"))
        #expect(try status(#"{"state":"off","pendingWrites":0,"unmapped":[],"record":null}"#).record == nil)
    }
}

/// A Keychain the test controls, by profile.
final class FakeTraktKeystore: @unchecked Sendable {
    var held: [String: JSONValue] = [:]
    var storeFails = false
    var deleteFails = false

    var keystore: TraktKeystore {
        TraktKeystore(
            load: { [self] in held[$0] },
            store: { [self] profile, record in
                guard !storeFails else { return false }
                held[profile] = record
                return true
            },
            delete: { [self] profile in
                guard !deleteFails else { return false }
                held[profile] = nil
                return true
            }
        )
    }
}

/// The fake every core in this process reaches: `MQ_DEV_TRAKT` is read when a core opens.
@MainActor enum TraktFake {
    static let shared: FakeTrakt = {
        let fake = try! FakeTrakt()
        setenv("MQ_DEV_TRAKT", fake.url, 1)
        return fake
    }()
}

/// Whether the linked core carries the loopback Trakt hook (a --dev build): the wiring tests need it.
enum DevCore {
    static let linked: Bool = {
        let lib = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .appending(path: "../Core/MarqueeCore.xcframework/macos-arm64/libmarquee_core.a")
        guard let data = try? Data(contentsOf: lib, options: .alwaysMapped) else { return false }
        return data.range(of: Data("MQ_DEV_TRAKT".utf8)) != nil
    }()
}

@Suite(.serialized, .enabled(if: DevCore.linked, "needs the --dev core: apps/apple/scripts/build-core.sh --dev"))
@MainActor struct TraktWiringTests {
    /// A core whose profile connects through the fake's sign-in, with a model that stored the record.
    private func connected(_ keys: FakeTraktKeystore = FakeTraktKeystore(), defaults: UserDefaults? = nil) async throws -> (RealCore, TraktModel) {
        let fake = TraktFake.shared
        let core = RealCore()
        #expect(await core.ready())
        let model = TraktModel(core: core.client, keystore: keys.keystore, defaults: try defaults ?? fresh())
        await model.begin(profile: "default")
        #expect(await eventually(10) { model.status?.state == .off })
        let login = LiveRecord<TraktLoginSnapshot>("traktLogin", core: core.client)
        login.start()
        #expect(await eventually(10) { if case .code(let code, _, _, _)? = login.snapshot { code == FakeTrakt.userCode } else { false } })
        fake.approve()
        #expect(await eventually(15) { model.status?.state == .connected })
        withExtendedLifetime(login) {}
        return (core, model)
    }

    @Test func aRecordIsStoredBeforeItIsAcknowledgedAndNeverShown() async throws {
        let keys = FakeTraktKeystore()
        let (core, model) = try await connected(keys)
        let record = try #require(keys.held["default"])
        #expect(model.status?.user == FakeTrakt.user)
        #expect(model.status?.record == nil)
        let stored = String(decoding: try JSONEncoder().encode(record), as: UTF8.self)
        #expect(TraktFake.shared.secrets.contains { stored.contains($0) })
        #expect(!TraktFake.shared.secrets.contains { "\(String(describing: model.status))".contains($0) })
        #expect(!model.memoryOnly)
        withExtendedLifetime(core) {}
    }

    @Test func aNewCoreGetsTheStoredRecordAndADeviceHoldingNoneSendsNothing() async throws {
        let keys = FakeTraktKeystore()
        let (first, _) = try await connected(keys)
        // A new core (a restart, another launch): the stored record goes before the view opens.
        let core = RealCore()
        #expect(await core.ready())
        let model = TraktModel(core: core.client, keystore: keys.keystore, defaults: try fresh())
        await model.begin(profile: "default")
        #expect(await eventually(10) { model.status?.state == .connected })
        // A model that holds no record for the profile never ends the connection with setTrakt(null).
        let empty = TraktModel(core: core.client, keystore: FakeTraktKeystore().keystore, defaults: try fresh())
        model.end()
        await empty.begin(profile: "default")
        #expect(await eventually(10) { empty.status != nil })
        try await Task.sleep(for: .milliseconds(300))
        #expect(empty.status?.state == .connected)
        withExtendedLifetime(first) {}
    }

    @Test func anAcknowledgementFromAProfileThatStoppedIsNeverSent() async throws {
        let keys = FakeTraktKeystore()
        let (first, _) = try await connected(keys)
        let record = try #require(keys.held["default"])
        // A fresh core, not connected: a record offered by a session that ended meanwhile.
        let core = RealCore()
        #expect(await core.ready())
        let fresh = FakeTraktKeystore()
        let model = TraktModel(core: core.client, keystore: fresh.keystore, defaults: try self.fresh())
        await model.begin(profile: "default")
        #expect(await eventually(10) { model.status?.state == .off })
        let old = try #require(model.session)
        await model.begin(profile: "kid")
        let offered = String(decoding: try JSONEncoder().encode(record), as: UTF8.self)
        model.received(try status(#"{"state":"off","pendingWrites":0,"unmapped":[],"record":\#(offered)}"#), in: old)
        // Stored for the profile it came from, never handed to the one that runs now.
        #expect(fresh.held["default"] == record && fresh.held["kid"] == nil)
        try await Task.sleep(for: .milliseconds(500))
        #expect(model.status?.state == .off)
        withExtendedLifetime(first) {}
    }

    @Test func aKeychainThatRefusesKeepsTheSignInForThisSessionAndSaysSo() async throws {
        let keys = FakeTraktKeystore()
        keys.storeFails = true
        let (core, model) = try await connected(keys)
        #expect(model.memoryOnly)
        #expect(keys.held.isEmpty)
        withExtendedLifetime(core) {}
    }

    @Test func signingOutDeletesTheRecordAndARefusalIsSaid() async throws {
        let keys = FakeTraktKeystore()
        let (core, model) = try await connected(keys)
        keys.deleteFails = true
        try await model.signOut()
        #expect(model.deleteFailed && keys.held["default"] != nil)
        #expect(await eventually(10) { model.status?.state == .off })
        keys.deleteFails = false
        model.deleteAgain()
        #expect(!model.deleteFailed && keys.held["default"] == nil)
        withExtendedLifetime(core) {}
    }

    @Test func aDeletedProfilesRecordGoesFromTheKeychain() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let keys = FakeTraktKeystore()
        let trakt = TraktModel(core: core.client, keystore: keys.keystore, defaults: try fresh())
        let session = ProfileSession(core: core.client, sync: SyncModel(core: core.client, records: FakeKeychain().record,
                                                                         defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)"))), trakt: trakt)
        #expect(await eventually { session.snapshot != nil })
        try await session.perform("createProfile", ["name": "Kid", "avatar": ProfileRules.formatAvatar(glyph: "cat", color: "teal")])
        #expect(await eventually { session.snapshot?.profiles.count == 2 })
        let kid = try #require(session.snapshot?.profiles.first { !$0.isOwner })
        keys.held[kid.id] = .string("a stored sign-in")
        keys.held["default"] = .string("the owner's")
        try await session.perform("deleteProfile", ["profile": kid.id])
        #expect(keys.held[kid.id] == nil)
        #expect(keys.held["default"] != nil)
    }

    @Test func backgroundIsAnsweredOnceScrobblesAreSent() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let start = ContinuousClock.now
        #expect(await core.client.lifecycleBackground())
        #expect(ContinuousClock.now - start < .seconds(5))
        // A core that cannot answer: false, at once, so the app is never held on it.
        let file = FileManager.default.temporaryDirectory.appending(path: "mq-test-\(UUID().uuidString)")
        try Data().write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }
        let dead = CoreClient(directory: file.appending(path: "data", directoryHint: .isDirectory))
        #expect(await !dead.lifecycleBackground(within: .seconds(2)))
    }

    @Test func aSignOutWhoseDeleteWasRefusedIsNeverUndone() async throws {
        let keys = FakeTraktKeystore()
        let store = try fresh()
        let (first, model) = try await connected(keys, defaults: store)
        keys.deleteFails = true
        try await model.signOut()
        #expect(model.deleteFailed && keys.held["default"] != nil)
        #expect(await eventually(10) { model.status?.state == .off })
        // A switch and back, or the next launch: the kept record is never sent again.
        let again = TraktModel(core: first.client, keystore: keys.keystore, defaults: store)
        model.end()
        await again.begin(profile: "default")
        #expect(again.deleteFailed)
        try await Task.sleep(for: .milliseconds(500))
        #expect(again.status?.state == .off)
        // The delete is tried again at the next begin, and once it works the warning goes.
        keys.deleteFails = false
        await again.begin(profile: "default")
        #expect(!again.deleteFailed && keys.held["default"] == nil)
        #expect(store.stringArray(forKey: TraktModel.signedOutKey) == [])
    }

    private func fresh() throws -> UserDefaults {
        try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)"))
    }
}
