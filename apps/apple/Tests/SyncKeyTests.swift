import AppKit
import Foundation
import Testing

// The sync key in the Keychain (round 2 reviews): a Keychain that does not
// answer is never "no account", a refused delete is said, a key left by a
// deleted install goes before the core opens, and a kit on a Mac's
// clipboard does not outlive the app.

private let account = try! JSONDecoder().decode(JSONValue.self, from: Data(#"{"revision":3,"keys":"opaque"}"#.utf8))

@Suite(.serialized) @MainActor struct SyncKeyTests {
    private func defaults() throws -> UserDefaults {
        try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)"))
    }

    @Test func aKeychainThatDoesNotAnswerIsNotNoAccount() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let keychain = FakeKeychain(account)
        keychain.readFails = true
        let sync = SyncModel(core: core.client, records: keychain.record, defaults: try defaults())
        #expect(sync.keyUnreadable && !sync.joined)
        // None of the ways in may replace the account it may hold.
        await #expect { try await sync.createAccount(server: "https://relay.invalid", invite: "", name: "Desk") } throws: {
            ($0 as? CoreError)?.code == "keyUnreadable"
        }
        await #expect { try await sync.recover(server: "https://relay.invalid", kit: "AAAA", name: "Desk") } throws: {
            ($0 as? CoreError)?.code == "keyUnreadable"
        }
        sync.startPairing(server: "https://relay.invalid", name: "Desk")
        #expect(sync.pairing == nil)
        #expect(keychain.held == account)
        // Try again once the Keychain answers: the account is there.
        keychain.readFails = false
        sync.readKeyAgain()
        #expect(!sync.keyUnreadable && sync.joined)
        #expect(SyncProblem(code: "keyUnreadable", message: "").words == SyncWords.keyUnreadable)
    }

    @Test func aKeyTheKeychainKeptAfterLeavingIsSaid() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let keychain = FakeKeychain(account)
        keychain.deleteFails = true
        let sync = SyncModel(core: core.client, records: keychain.record, defaults: try defaults())
        #expect(sync.joined && !sync.keyLeft)
        sync.forgetHere()
        #expect(!sync.joined && sync.keyLeft)
        #expect(keychain.held == account)
        sync.deleteKeyAgain()
        #expect(sync.keyLeft)
        keychain.deleteFails = false
        sync.deleteKeyAgain()
        #expect(!sync.keyLeft && keychain.held == nil)
    }

    @Test func aKeyLeftByADeletedInstallGoesBeforeTheCoreOpens() throws {
        // First launch of an install with no library: the old install's key goes.
        let keychain = FakeKeychain(account)
        let fresh = try defaults()
        #expect(keychain.record.forgetLeftover(libraryExists: false, defaults: fresh))
        #expect(keychain.held == nil)
        // Later launches never touch it, library or not.
        keychain.held = account
        #expect(!keychain.record.forgetLeftover(libraryExists: false, defaults: fresh))
        #expect(keychain.held == account)
        // A first launch of this version over a library (an update): kept.
        let updated = try defaults()
        #expect(!keychain.record.forgetLeftover(libraryExists: true, defaults: updated))
        #expect(keychain.held == account)
        // A Keychain that does not answer: nothing is deleted blind.
        keychain.readFails = true
        #expect(!keychain.record.forgetLeftover(libraryExists: false, defaults: try defaults()))
        keychain.readFails = false
        #expect(keychain.held == account)
    }

    @Test func aKitOnAMacsClipboardIsClearedAtTheNextLaunch() throws {
        let board = NSPasteboard(name: NSPasteboard.Name("mq-test-\(UUID().uuidString)"))
        defer { board.releaseGlobally() }
        let store = try defaults()
        // The app wrote a kit and stopped before its minute was up.
        let change = Clipboard.write("SECRET-KIT", to: board)
        store.set(change, forKey: Clipboard.pendingKey)
        Clipboard.clearLeftover(board, defaults: store)
        #expect(board.string(forType: .string) == nil)
        #expect(store.object(forKey: Clipboard.pendingKey) == nil)
        // Something copied since stays.
        let kit = Clipboard.write("SECRET-KIT", to: board)
        store.set(kit, forKey: Clipboard.pendingKey)
        board.clearContents()
        board.setString("a link", forType: .string)
        Clipboard.clearLeftover(board, defaults: store)
        #expect(board.string(forType: .string) == "a link")
        // Written marked concealed and transient, which clipboard managers skip.
        Clipboard.write("SECRET-KIT", to: board)
        #expect(board.types?.contains(NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")) == true)
    }
}
