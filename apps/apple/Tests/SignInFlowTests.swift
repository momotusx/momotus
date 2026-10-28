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
        await flow.savePasskey(userName: "Robin (Momotus)")
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

/// The relay's email answers, as scripted, with what was asked.
@MainActor final class ScriptedMail {
    var starts: [String] = []
    var takes: [String] = []
    var start: (String) throws -> Void = { _ in }
    var take: Result<SignInSnapshot, CoreError> = .failure(CoreError("wrong", code: "wrongCode"))
    /// A take that waits, so a second submit meets the first one in flight.
    var slow = false
    /// What a Link another flow's `finish` was handed.
    var answer: SignInSnapshot?

    var calls: SignInFlow.Mail {
        SignInFlow.Mail(
            start: { [self] _, email in
                try start(email)
                starts.append(email)
                return EmailStart(flow: "f\(starts.count)", expiresMs: 0)
            },
            take: { [self] flow, code in
                takes.append("\(flow):\(code)")
                if slow { try? await Task.sleep(for: .milliseconds(100)) }
                return try take.get()
            })
    }
}

// Sign in with email (plans/accounts-email.md 4): the two stages, the answers
// table, Try again and the held ticket's Continue, with the relay scripted.
@Suite(.serialized) @MainActor struct EmailSignInTests {
    private func open(_ mail: ScriptedMail, sheets: ScriptedSheets = ScriptedSheets(),
                      finish: (@MainActor (SignInSnapshot) async -> Void)? = nil) async throws -> (SignInFlow, RealCore) {
        let core = RealCore()
        #expect(await core.ready())
        let sync = SyncModel(core: core.client, records: FakeKeychain().record,
                             defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        return (SignInFlow(server: nobody, sync: sync, sheets: sheets.calls, mail: mail.calls, finish: finish), core)
    }

    /// The address stage, a code sent to `address`.
    private func sent(_ flow: SignInFlow, _ address: String = "ada@example.com") async {
        flow.openEmail()
        flow.address = address
        await flow.sendCode()
    }

    @Test func theAddressThenTheCodeAndBackKeepsTheAddressWithoutANewMail() async throws {
        let mail = ScriptedMail()
        let (flow, core) = try await open(mail)
        flow.openEmail()
        #expect(flow.stage == .email && flow.provider == "email")
        flow.address = "  ada@example.com "
        await flow.sendCode()
        #expect(flow.stage == .code && flow.sentTo == "ada@example.com" && flow.mailFlow == "f1" && flow.sentAt != nil)
        #expect(!flow.resent, "a first code is no resend")
        flow.toAddress()
        #expect(flow.stage == .email && flow.address == "  ada@example.com ")
        // The same address while its code may still work: the code stage again, no mail.
        await flow.sendCode()
        #expect(flow.stage == .code && mail.starts == ["ada@example.com"])
        // Another address: a new mail.
        flow.toAddress()
        flow.address = "bob@example.com"
        await flow.sendCode()
        #expect(mail.starts == ["ada@example.com", "bob@example.com"] && flow.sentTo == "bob@example.com")
        withExtendedLifetime(core) {}
    }

    @Test func aMalformedAddressIsSaidOnItsStageByTheCore() async throws {
        // The core's own email calls: badAddress comes before any call to the relay.
        let core = RealCore()
        #expect(await core.ready())
        let sync = SyncModel(core: core.client, records: FakeKeychain().record,
                             defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        let flow = SignInFlow(server: nobody, sync: sync, sheets: ScriptedSheets().calls)
        flow.openEmail()
        flow.address = "not an address"
        await flow.sendCode()
        #expect(flow.stage == .email && flow.troubleCode == "badAddress")
        #expect(flow.trouble?.text == AccountWords.shared.badAddress)
        withExtendedLifetime(core) {}
    }

    @Test func aFailedSendStaysOnItsStageAndTryAgainSendsAgain() async throws {
        let mail = ScriptedMail()
        let sheets = ScriptedSheets()
        mail.start = { _ in throw CoreError("wait", code: "emailWait") }
        let (flow, core) = try await open(mail, sheets: sheets)
        await sent(flow)
        #expect(flow.stage == .email && flow.troubleCode == "emailWait", "fail() keeps the email stage")
        mail.start = { _ in throw CoreError("down", code: "mailUnavailable") }
        await flow.tryAgain(name: "Mac")
        #expect(flow.stage == .email && flow.trouble?.actions == ["Try again", "Other ways to sign in"])
        mail.start = { _ in }
        await flow.tryAgain(name: "Mac")
        #expect(flow.stage == .code && mail.starts == ["ada@example.com"])
        // Never the provider flow: no sheet was asked for.
        #expect(sheets.passkeys.isEmpty && flow.provider == "email")
        withExtendedLifetime(core) {}
    }

    @Test func aWrongCodeKeepsTheStageAndClearsTheField() async throws {
        let mail = ScriptedMail()
        let (flow, core) = try await open(mail)
        await sent(flow)
        flow.typeCode("123 45", name: "Mac")
        #expect(flow.code == "12345" && mail.takes.isEmpty)
        await flow.submitCode(name: "Mac")
        #expect(mail.takes.isEmpty, "not 6 digits: nothing sent")
        flow.typeCode("123456", name: "Mac")
        await flow.submitCode(name: "Mac")
        #expect(flow.stage == .code && flow.troubleCode == "wrongCode" && flow.code.isEmpty && flow.mailFlow == "f1")
        mail.take = .failure(CoreError("shape", code: "badCode"))
        flow.typeCode("654321", name: "Mac")
        await flow.submitCode(name: "Mac")
        #expect(flow.stage == .code && flow.troubleCode == "wrongCode")
        withExtendedLifetime(core) {}
    }

    @Test func anEndedCodeSaysCodeExpiredWithItsActions() async throws {
        for refused in ["ticketExpired", "wrongFlow"] {
            let mail = ScriptedMail()
            mail.take = .failure(CoreError(refused, code: refused))
            let (flow, core) = try await open(mail)
            await sent(flow)
            flow.typeCode("123456", name: "Mac")
            await flow.submitCode(name: "Mac")
            #expect(flow.stage == .code && flow.troubleCode == "codeExpired", "\(refused)")
            #expect(flow.trouble?.text == AccountWords.shared.codeExpired)
            #expect(flow.trouble?.actions == ["Send a new code", "Use another address"])
            // The field is kept and read-only: the code can no longer be used.
            flow.typeCode("999999", name: "Mac")
            #expect(flow.code == "123456" && flow.mailFlow == nil)
            // Send a new code: a new flow, the field open again, codeResent said.
            await flow.resendCode()
            #expect(flow.mailFlow == "f2" && flow.code.isEmpty && flow.resent && flow.trouble == nil)
            withExtendedLifetime(core) {}
        }
    }

    @Test func aRefusedSignInKeepsItsOwnWordsWithTheCodeActions() async throws {
        let mail = ScriptedMail()
        mail.take = .failure(CoreError("refused", code: "signInRefused"))
        let (flow, core) = try await open(mail)
        await sent(flow)
        flow.typeCode("123456", name: "Mac")
        await flow.submitCode(name: "Mac")
        #expect(flow.stage == .code && flow.troubleCode == "signInRefused")
        #expect(flow.trouble?.text == AccountWords.shared.signInRefused)
        #expect(flow.trouble?.actions == AccountWords.shared.codeExpiredActions)
        withExtendedLifetime(core) {}
    }

    @Test func noAnswerOnTheCodeStageStaysAndTryAgainSendsTheSameCode() async throws {
        let mail = ScriptedMail()
        let sheets = ScriptedSheets()
        mail.take = .failure(CoreError("network", code: nil))
        let (flow, core) = try await open(mail, sheets: sheets)
        await sent(flow)
        flow.typeCode("123456", name: "Mac")
        await flow.submitCode(name: "Mac")
        #expect(flow.stage == .code && flow.troubleCode == "unreachable" && flow.code == "123456")
        mail.take = .failure(CoreError("busy", code: "busy"))
        await flow.tryAgain(name: "Mac")
        #expect(mail.takes == ["f1:123456", "f1:123456"] && flow.troubleCode == "busy" && flow.stage == .code)
        // A resend that fails on the code stage: Try again sends again, not the code.
        mail.start = { _ in throw CoreError("down", code: "mailUnavailable") }
        await flow.resendCode()
        #expect(flow.stage == .code && flow.troubleCode == "mailUnavailable")
        mail.start = { _ in }
        await flow.tryAgain(name: "Mac")
        #expect(mail.starts.count == 2 && mail.takes.count == 2 && flow.resent)
        #expect(sheets.passkeys.isEmpty, "never the provider flow")
        withExtendedLifetime(core) {}
    }

    @Test func theSixthDigitSubmitsOnce() async throws {
        let mail = ScriptedMail()
        mail.slow = true
        let (flow, core) = try await open(mail)
        await sent(flow)
        flow.typeCode("123-456", name: "Mac")
        // Go pressed while the sixth digit's submit is with the relay: one call.
        await flow.submitCode(name: "Mac")
        for _ in 0..<50 where flow.sending { try await Task.sleep(for: .milliseconds(20)) }
        #expect(mail.takes == ["f1:123456"])
        withExtendedLifetime(core) {}
    }

    @Test func theCodesAnswerEndsAsAProvidersOrInTheLinkCall() async throws {
        let mail = ScriptedMail()
        mail.take = .success(.new(ticket: "t9"))
        let (flow, core) = try await open(mail)
        await sent(flow)
        flow.typeCode("123456", name: "Mac")
        for _ in 0..<50 where flow.stage == .code { try await Task.sleep(for: .milliseconds(20)) }
        // No passkey here: Create your Momotus account, as after a provider.
        #expect(flow.stage == .new(ticket: "t9") && flow.mailFlow == nil)

        let linked = ScriptedMail()
        let (link, linkCore) = try await open(mail, finish: { linked.answer = $0 })
        await sent(link)
        link.typeCode("123456", name: "Mac")
        await link.submitCode(name: "Mac")
        for _ in 0..<50 where linked.answer == nil { try await Task.sleep(for: .milliseconds(20)) }
        #expect(linked.answer == .new(ticket: "t9"))
        withExtendedLifetime((core, linkCore)) {}
    }

    @Test func theHeldTicketsContinueSendsANewCodeToTheAddressInMemory() async throws {
        let mail = ScriptedMail()
        let (flow, core) = try await open(mail)
        await sent(flow)
        // The ticket ran out later, back on Sign in: Continue from the trouble line.
        flow.back()
        await flow.press("email", name: "Mac")
        #expect(flow.stage == .code && mail.starts == ["ada@example.com", "ada@example.com"])
        #expect(flow.mailFlow == "f2" && flow.resent && flow.address == "ada@example.com")
        withExtendedLifetime(core) {}

        // The address no longer in memory (a restart): the address stage, empty.
        let (fresh, freshCore) = try await open(ScriptedMail())
        await fresh.press("email", name: "Mac")
        #expect(fresh.stage == .email && fresh.address.isEmpty)
        withExtendedLifetime(freshCore) {}
    }
}
