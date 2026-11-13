import Foundation
import Testing

// A switch from a live Home (QA r5, R5-25): the core refuses it while any view of the old profile is open, so nothing
// the old screens leave open may outlive the switch. Incognito's entry is the probe: with views open the core answers
// `viewsOpen`, with none `incognitoFailed` (no scope on a fresh core).
@Suite(.serialized) @MainActor struct SwitchViewsTests {
    private func device() async throws -> (ProfileSession, RealCore) {
        let core = RealCore()
        #expect(await core.ready())
        let sync = SyncModel(core: core.client, records: FakeKeychain().record,
                             defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        let session = ProfileSession(core: core.client, sync: sync)
        #expect(await eventually { session.snapshot != nil })
        return (session, core)
    }

    @Test func aSwitchClosesWhatTheOldScreensStillHold() async throws {
        let (session, core) = try await device()
        // A model something still holds (a closure, a menu): its view is open when the switch asks.
        let held = LiveRecord<AddonsSnapshot>("addons", core: core.client)
        held.start()
        #expect(await eventually { held.snapshot != nil })
        #expect(!(await session.enterIncognito(pin: nil, fromChooser: true)))
        #expect(session.switchProblem?.code == "incognitoFailed", "the switch was refused for the views")
        // The picker's Back: the profile was open before the switch, so the failed picker offers the way back.
        #expect(session.canGoBack)
        session.cancelSwitch()
        #expect(!session.canGoBack && session.open)
        withExtendedLifetime(held) {}
    }

    @Test func theKeptViewsAreTheCoresOwn() throws {
        // The core's `only_the_picker_open` keeps these three open across a switch.
        let bridge = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../../crates/mq-bridge/src/lib.rs")
        let rust = try String(contentsOf: bridge, encoding: .utf8)
        #expect(rust.contains(#".filter(|name| !matches!(*name, "profiles" | "debrid" | "downloads"))"#))
        #expect(ProfileSession.keptViews == ["profiles", "debrid", "downloads"])
    }
}
