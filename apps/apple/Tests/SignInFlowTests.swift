import Foundation
import Testing

// The sign-in screens' steps (plans/accounts.md 4.1, 5) at the flow's level,
// with the system sheets scripted. A sync server nobody answers stands in
// where no answer is needed; AccountRelayTests runs the steps that need one.

private let nobody = "http://127.0.0.1:9"

/// Sheets that answer as scripted and record what was asked.
@MainActor final class ScriptedSheets {
    /// The `immediate` of each passkey sheet asked for.
    var passkeys: [Bool] = []
    var passkey: Result<SignInSheets.Passkey, SignInSheets.Failure> = .failure(.noCredential)
    var created = 0
    var create: Result<Data, SignInSheets.Failure> = .failure(.cancelled)
    var browser: @MainActor (URL) async throws -> String = { _ in throw SignInSheets.Failure.cancelled }

    var calls: SignInSheets.Calls {
        var calls = SignInSheets.Calls()
        calls.apple = { _ in throw SignInSheets.Failure.cancelled }
        calls.passkey = { [self] immediate in
            passkeys.append(immediate)
            return try passkey.get()
        }
        calls.create = { [self] _, _ in
            created += 1
            return try create.get()
        }
        calls.browser = { [self] url in try await browser(url) }
        return calls
    }
}

@Suite(.serialized) @MainActor struct SignInFlowTests {
    private func open(_ sheets: ScriptedSheets) async throws -> (SignInFlow, RealCore) {
        let core = RealCore()
        #expect(await core.ready())
        let sync = SyncModel(core: core.client, records: FakeKeychain().record,
                             defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        return (SignInFlow(server: nobody, sync: sync, sheets: sheets.calls), core)
    }

    @Test func foundWithNoPasskeyHereOffersTheCodeAndKeepsTheSignIn() async throws {
        let sheets = ScriptedSheets()
        let (flow, core) = try await open(sheets)
        await flow.signedIn(.found(ticket: "t1", account: "a1"), name: "Mac")
        #expect(flow.stage == .cannotOpen(ticket: "t1"))
        // A passkey on this device only: no QR to another phone (4.1.3).
        #expect(sheets.passkeys == [true])
        withExtendedLifetime(core) {}
    }

    @Test func foundTakesOnlyAPasskeyMadeForTheFoundAccount() async throws {
        let sheets = ScriptedSheets()
        sheets.passkey = .success(.init(prf: Data(repeating: 7, count: 32), user: Data(repeating: 0xab, count: 16)))
        let (flow, core) = try await open(sheets)
        // Another account's passkey: this device never joins that one, the code next.
        await flow.signedIn(.found(ticket: "t1", account: String(repeating: "cd", count: 16)), name: "Mac")
        #expect(flow.stage == .cannotOpen(ticket: "t1"))
        // The found account's own opens it (nobody answers here, so its recover fails in words).
        await flow.signedIn(.found(ticket: "t1", account: String(repeating: "AB", count: 16)), name: "Mac")
        #expect(flow.stage == .choose && flow.trouble != nil)
        #expect(sheets.passkeys == [true, true])
        withExtendedLifetime(core) {}
    }

    @Test func newWithNoPasskeyHereOffersCreate() async throws {
        let sheets = ScriptedSheets()
        let (flow, core) = try await open(sheets)
        await flow.signedIn(.new(ticket: "t2"), name: "Mac")
        #expect(flow.stage == .new(ticket: "t2"))
        #expect(sheets.passkeys == [true])
        withExtendedLifetime(core) {}
    }

    @Test func newWithAPasskeyHereUsesItsOneSheet() async throws {
        let sheets = ScriptedSheets()
        sheets.passkey = .success(.init(prf: Data(repeating: 7, count: 32), user: Data(repeating: 1, count: 16)))
        let (flow, core) = try await open(sheets)
        await flow.signedIn(.new(ticket: "t3"), name: "Mac")
        // The probe's own answer opens the account: one sheet, one Face ID (4.1.4).
        #expect(sheets.passkeys == [true])
        // Nobody answers here, so its recover fails, in words.
        #expect(flow.stage == .choose && flow.trouble != nil)
        withExtendedLifetime(core) {}
    }

    @Test func startOverWarnsFirstAndCancelGoesBack() async throws {
        let (flow, core) = try await open(ScriptedSheets())
        flow.goStartOver(ticket: "t4")
        #expect(flow.stage == .startOver(ticket: "t4"))
        flow.back()
        #expect(flow.stage == .choose && flow.trouble == nil)
        withExtendedLifetime(core) {}
    }

    @Test func aPasskeySaveThatCannotRunOffersTheOtherWays() async throws {
        let sheets = ScriptedSheets()
        let (flow, core) = try await open(sheets)
        // No account to read its id from: 4.1.6, Try again, a kit instead, or Later.
        await flow.savePasskey(userName: "Robin (Marquee)")
        #expect(flow.stage == .noSecondWayIn)
        #expect(sheets.created == 0)
        flow.later()
        #expect(flow.stage == .done)
        withExtendedLifetime(core) {}
    }

    @Test func aCancelledPasskeyKeepsTheWaysIn() async throws {
        let sheets = ScriptedSheets()
        sheets.passkey = .failure(.cancelled)
        let (flow, core) = try await open(sheets)
        await flow.usePasskey(name: "Mac")
        #expect(sheets.passkeys == [false])
        #expect(flow.stage == .choose && flow.troubleCode == "passkeyCancelled")
        #expect(flow.trouble?.actions == AccountWords.shared.passkeyCancelledActions)
        withExtendedLifetime(core) {}
    }

    @Test func aSignInFailureIsSaidAndACancelIsNot() async throws {
        let (flow, core) = try await open(ScriptedSheets())
        await flow.signedIn(.failed(code: "ticketExpired"), name: "Mac")
        #expect(flow.stage == .choose && flow.troubleCode == "ticketExpired")
        flow.back()
        await flow.signedIn(.failed(code: "cancelled"), name: "Mac")
        #expect(flow.stage == .choose && flow.trouble == nil)
        withExtendedLifetime(core) {}
    }

    @Test func aJoinAnotherWayHoldsTheSignInUntilTheJoin() async throws {
        let (flow, core) = try await open(ScriptedSheets())
        flow.joinAnotherWay(ticket: "t5")
        #expect(flow.linkAfterJoin == HeldLink(ticket: "t5", provider: ""))
        // Not joined yet: it stays held.
        await flow.linkHeld()
        #expect(flow.linkAfterJoin != nil)
        withExtendedLifetime(core) {}
    }

    @Test func aRelayThatDoesNotAnswerIsSaidWithTryAgain() async throws {
        let (flow, core) = try await open(ScriptedSheets())
        await flow.open()
        #expect(flow.unreachable && flow.providers.isEmpty)
        flow.retry()
        #expect(flow.attempt == 1)
        withExtendedLifetime(core) {}
    }
}
