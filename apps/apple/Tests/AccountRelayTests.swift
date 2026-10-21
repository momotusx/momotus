import Foundation
import Testing

// The brokered sign-in end to end on this Mac (plans/accounts.md 7): the
// app's model on a real core, a local relay with an identity file and the
// mock OpenID provider (`cargo run -p mq-sync-server --example mock-oidc`).
// Runs only with MARQUEE_SIGNIN_RELAY set (xcodebuild passes it as
// TEST_RUNNER_MARQUEE_SIGNIN_RELAY) and reports skipped without it. The auth
// session's browser is played by URLSession, which follows the provider's
// picker and stops at the return; the other sheets are scripted. The suite
// signs in 13 times, 26 of the 30 sign-in calls a minute the relay allows one
// address: start a run a minute after the last.

private let relayAddress = ProcessInfo.processInfo.environment["MARQUEE_SIGNIN_RELAY"]

/// Follows a page without redirects, so the `marquee://auth` return is seen, not followed.
private final class NoRedirect: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest) async -> URLRequest? { nil }
}

/// The browser: the provider's picker, "Sign in as alice", the relay's callback, the return.
private func browse(_ url: String, as subject: String) async throws -> String {
    let session = URLSession(configuration: .ephemeral, delegate: NoRedirect(), delegateQueue: nil)
    var next = try #require(URL(string: url))
    for _ in 0..<6 {
        let (body, answer) = try await session.data(from: next)
        let http = try #require(answer as? HTTPURLResponse)
        if let location = http.value(forHTTPHeaderField: "Location") {
            if location.hasPrefix("marquee://auth#") { return String(location.dropFirst("marquee://auth#".count)) }
            next = try #require(URL(string: location, relativeTo: next)?.absoluteURL)
            continue
        }
        // The picker page: its "Sign in as alice" link, choosing this subject instead.
        let page = String(decoding: body, as: UTF8.self)
        let marker = "\">Sign in as alice</a>"
        let end = try #require(page.range(of: marker)).lowerBound
        let start = try #require(page[..<end].range(of: "href=\"", options: .backwards)).upperBound
        let link = String(page[start..<end]).replacingOccurrences(of: "&amp;", with: "&")
            .replacingOccurrences(of: "choose=alice", with: "choose=\(subject)")
        next = try #require(URL(string: link, relativeTo: next)?.absoluteURL)
    }
    throw CoreError("no return", code: "noRedirect")
}

@Suite(.serialized, .enabled(if: relayAddress != nil, "needs MARQUEE_SIGNIN_RELAY: a local relay with the mock provider"))
@MainActor struct AccountRelayTests {
    @Test func aBrokeredSignInMakesAnAccountThenFindsIt() async throws {
        let relay = try #require(relayAddress)
        let core = RealCore()
        #expect(await core.ready())
        let keychain = FakeKeychain()
        let defaults = try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)"))
        let sync = SyncModel(core: core.client, records: keychain.record, defaults: defaults)

        let providers = try await sync.relayInfo(server: relay)
        #expect(providers == ["mock"])

        // New: no account for this identity yet; its ticket makes one, linked.
        let subject = "walk-\(UUID().uuidString.prefix(8))"
        let first = try await signIn(sync, relay, as: subject)
        guard case .new(let ticket) = first else { Issue.record("not new: \(first)"); return }
        try await sync.createAccount(server: relay, invite: "", name: "Mac test", ticket: ticket)
        #expect(sync.joined && keychain.held != nil)
        let links = try await sync.identities()
        #expect(links.map(\.provider) == ["mock"])

        // Found: the same identity again finds that account (it opens nothing by itself).
        let again = try await signIn(sync, relay, as: subject)
        guard case .found(_, let account) = again else { Issue.record("not found: \(again)"); return }
        let devices = try await sync.loadDevices()
        // The same id: the device list writes it base64url (a passkey's user.id), the sign-in as hex.
        let bytes = try #require(devices.account.flatMap(PasskeyContract.userID))
        #expect(bytes.map { String(format: "%02x", $0) }.joined() == account)
    }

    @Test func aSavedPasskeyOpensTheAccountOnASecondDevice() async throws {
        let relay = try #require(relayAddress)
        let subject = "walk-\(UUID().uuidString.prefix(8))"
        // The first device: an account from a sign-in, then a passkey (its PRF output, as a sheet would give it).
        let phone = RealCore()
        #expect(await phone.ready())
        let first = SyncModel(core: phone.client, records: FakeKeychain().record, defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        guard case .new(let ticket) = try await signIn(first, relay, as: subject) else { Issue.record("not new"); return }
        try await first.createAccount(server: relay, invite: "", name: "Phone", ticket: ticket)
        let prf = PasskeyContract.challenge()
        let id = try await first.savePasskey(prf: prf, label: AccountRules.passkeyLabel(device: "Phone"))
        let saved = try #require(first.devices?.members.first { $0.id == id })
        #expect(saved.kit && saved.passkey && saved.name == "Passkey, saved on Phone")

        // A second device: the same passkey opens it, with no new kit asked for after it.
        let tablet = RealCore()
        #expect(await tablet.ready())
        let second = SyncModel(core: tablet.client, records: FakeKeychain().record, defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        try await second.recover(server: relay, prf: prf, name: "Tablet")
        #expect(second.joined && !second.recovered)
        let list = try await second.loadDevices()
        #expect(list.members.contains { $0.thisDevice && $0.viaPasskey })
        // The passkey now opened the account on a device: a second way in, said as Last used.
        let kit = try #require(list.members.first { $0.id == id })
        #expect(AccountRules.lastUsed(kit, members: list.members)?.hasPrefix("Last used: Tablet, ") == true)
        #expect(!AccountRules.needsSecondWayIn(list.members))

        // Another passkey opens nothing: kitUnknown, the QR next.
        let stranger = RealCore()
        #expect(await stranger.ready())
        let third = SyncModel(core: stranger.client, records: FakeKeychain().record, defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        await #expect { try await third.recover(server: relay, prf: PasskeyContract.challenge(), name: "Stranger") } throws: {
            ($0 as? CoreError)?.code == "kitUnknown"
        }

        // The phone hears it: a new device joined, worded by its key.
        first.request()
        #expect(await eventually(10) { first.lastReport?.announcements.contains { $0.kind == "device" } == true })
        let joined = try #require(first.lastReport?.announcements.first { $0.kind == "device" })
        #expect(AccountRules.text(joined, members: first.devices?.members ?? []) == "A new device joined: Tablet.")
        try await first.ackAnnouncements([joined.id])
        #expect(first.lastReport?.announcements.contains { $0.id == joined.id } == false)
    }

    // MARK: The sign-in flow (4.1, 5), its sheets scripted, its browser played

    @Test func theFlowMakesAnAccountAndItsPasskeyAfterAFailedSave() async throws {
        let relay = try #require(relayAddress)
        let (phone, core) = try await member()
        let sheets = scripted(as: subject())
        sheets.create = .failure(.noPRF)
        let flow = SignInFlow(server: relay, sync: phone, sheets: sheets.calls)
        await flow.continueWith("mock", name: "Phone")
        guard case .new(let ticket) = flow.stage else { Issue.record("not new: \(flow.stage)"); return }
        #expect(sheets.passkeys == [true])
        await flow.create(ticket: ticket, name: "Phone", userName: "Robin (Marquee)")
        // The new passkey gave no PRF output: 4.1.6.
        #expect(phone.joined && flow.stage == .noSecondWayIn && sheets.created == 1)
        // Try again: saved, and the first-run steps go on.
        sheets.create = .success(PasskeyContract.challenge())
        await flow.savePasskey(userName: "Robin (Marquee)")
        #expect(flow.stage == .done)
        #expect(phone.devices?.members.contains { $0.passkey } == true)
        #expect(try await phone.identities().map(\.provider) == ["mock"])
        withExtendedLifetime(core) {}
    }

    @Test func aPasskeyThatOpensNothingOnNewKeepsTheSignIn() async throws {
        let relay = try #require(relayAddress)
        let (tablet, core) = try await member()
        // The passkey opens nothing: Create next, the sign-in kept.
        let sheets = scripted(as: subject())
        sheets.passkey = .success(stranger())
        let flow = SignInFlow(server: relay, sync: tablet, sheets: sheets.calls)
        await flow.continueWith("mock", name: "Tablet")
        guard case .new = flow.stage else { Issue.record("not new: \(flow.stage)"); return }
        #expect(sheets.passkeys == [true] && !tablet.joined)
        withExtendedLifetime(core) {}
    }

    @Test func aPasskeyOnNewOpensItsAccountAndLinksTheSignIn() async throws {
        let relay = try #require(relayAddress)
        let (phone, phoneCore) = try await account(relay, as: subject())
        let prf = PasskeyContract.challenge()
        try await phone.savePasskey(prf: prf, label: AccountRules.passkeyLabel(device: "Phone"))
        let (tablet, tabletCore) = try await member()
        let sheets = scripted(as: subject())
        sheets.passkey = .success(.init(prf: prf, user: try accountID(phone)))
        let flow = SignInFlow(server: relay, sync: tablet, sheets: sheets.calls)
        await flow.continueWith("mock", name: "Tablet")
        // One sheet: the probe's answer opened the account, and the new sign-in is linked to it.
        #expect(flow.stage == .joined && tablet.joined && sheets.passkeys == [true])
        #expect(try await tablet.identities().count == 2)
        withExtendedLifetime((phoneCore, tabletCore)) {}
    }

    @Test func aSignInHeldOverAnotherWayInIsLinkedOrOfferedOnAccount() async throws {
        let relay = try #require(relayAddress)
        let alice = subject()
        let (phone, phoneCore) = try await account(relay, as: alice)
        let prf = PasskeyContract.challenge()
        try await phone.savePasskey(prf: prf, label: AccountRules.passkeyLabel(device: "Phone"))

        // A new sign-in, then "I already use Marquee on another device": linked once the join lands.
        let (tablet, tabletCore) = try await member()
        let flow = SignInFlow(server: relay, sync: tablet, sheets: scripted(as: subject()).calls)
        await flow.continueWith("mock", name: "Tablet")
        guard case .new(let ticket) = flow.stage else { Issue.record("not new: \(flow.stage)"); return }
        flow.joinAnotherWay(ticket: ticket)
        // The QR's join, played by the passkey here.
        try await tablet.recover(server: relay, prf: prf, name: "Tablet")
        await flow.linkHeld()
        #expect(try await tablet.identities().count == 2)

        // found, with a passkey here made for another account: the code next, the sign-in kept.
        let (laptop, laptopCore) = try await member()
        let again = scripted(as: alice)
        again.passkey = .success(stranger())
        let held = SignInFlow(server: relay, sync: laptop, sheets: again.calls)
        await held.continueWith("mock", name: "Laptop")
        guard case .cannotOpen(let found?) = held.stage else { Issue.record("no ticket kept: \(held.stage)"); return }
        #expect(again.passkeys == [true] && !laptop.joined)
        // Alice's sign-in held while this device joins another account: Account offers to move it.
        held.joinAnotherWay(ticket: found)
        guard case .new(let other) = try await signIn(laptop, relay, as: subject()) else { Issue.record("not new"); return }
        try await laptop.createAccount(server: relay, invite: "", name: "Laptop", ticket: other)
        await held.linkHeld()
        #expect(laptop.heldLink == HeldLink(ticket: found, provider: "mock"))
        withExtendedLifetime((phoneCore, tabletCore, laptopCore)) {}
    }

    @Test func savingAPasskeyLeavesAnotherAccountsPasskeyAlone() async throws {
        let relay = try #require(relayAddress)
        let (phone, phoneCore) = try await account(relay, as: subject())
        let theirs = PasskeyContract.challenge()
        try await phone.savePasskey(prf: theirs, label: AccountRules.passkeyLabel(device: "Phone"))
        let (tablet, tabletCore) = try await account(relay, as: subject())
        let ours = PasskeyContract.challenge()
        try await tablet.savePasskey(prf: ours, label: AccountRules.passkeyLabel(device: "Tablet"))

        // The probe offers the other account's passkey (as after Start over): a new one is made, no kitTaken.
        let sheets = ScriptedSheets()
        sheets.passkey = .success(.init(prf: theirs, user: try accountID(phone)))
        sheets.create = .success(PasskeyContract.challenge())
        #expect(try await PasskeySave.here(sync: tablet, sheets: sheets.calls, userName: "Robin (Marquee)") == .saved)
        #expect(sheets.created == 1)
        // This account's own passkey: it already works here, nothing saved.
        sheets.passkey = .success(.init(prf: ours, user: try accountID(tablet)))
        #expect(try await PasskeySave.here(sync: tablet, sheets: sheets.calls, userName: "Robin (Marquee)") == .alreadyHere)
        #expect(sheets.created == 1)
        withExtendedLifetime((phoneCore, tabletCore)) {}
    }

    // MARK: Helpers

    private func subject() -> String { "walk-\(UUID().uuidString.prefix(8))" }

    /// A passkey no account knows.
    private func stranger() -> SignInSheets.Passkey {
        SignInSheets.Passkey(prf: PasskeyContract.challenge(), user: Data(repeating: 9, count: 16))
    }

    /// A fresh device: its core, and the app's model on it.
    private func member() async throws -> (SyncModel, RealCore) {
        let core = RealCore()
        #expect(await core.ready())
        let sync = SyncModel(core: core.client, records: FakeKeychain().record,
                             defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)")))
        return (sync, core)
    }

    /// A device in a new account, made with `subject`'s sign-in.
    private func account(_ relay: String, as subject: String) async throws -> (SyncModel, RealCore) {
        let (sync, core) = try await member()
        guard case .new(let ticket) = try await signIn(sync, relay, as: subject) else { throw CoreError("not new", code: "test") }
        try await sync.createAccount(server: relay, invite: "", name: "Device", ticket: ticket)
        return (sync, core)
    }

    /// The account's id, as a passkey made for it carries it (`user.id`).
    private func accountID(_ sync: SyncModel) throws -> Data {
        try #require(sync.devices?.account.flatMap(PasskeyContract.userID))
    }

    /// Sheets with the provider's page played as `subject`.
    private func scripted(as subject: String) -> ScriptedSheets {
        let sheets = ScriptedSheets()
        sheets.browser = { url in try await browse(url.absoluteString, as: subject) }
        return sheets
    }

    private func signIn(_ sync: SyncModel, _ relay: String, as subject: String) async throws -> SignInSnapshot {
        let view = sync.signInView(server: relay, provider: "mock")
        view.start()
        var url: String?
        for _ in 0..<100 {
            if case .browser(let link, _) = view.snapshot { url = link; break }
            try await Task.sleep(for: .milliseconds(50))
        }
        let fragment = try await browse(try #require(url), as: subject)
        try await sync.returned(view, fragment: fragment)
        for _ in 0..<200 {
            switch view.snapshot {
            case .found?, .new?, .failed?: return view.snapshot!
            default: try await Task.sleep(for: .milliseconds(50))
            }
        }
        throw CoreError("no answer", code: "unreachable")
    }
}
