import Foundation
import Testing

// The brokered sign-in end to end on this Mac (plans/accounts.md 7): the
// app's model on a real core, a local relay with an identity file and the
// mock OpenID provider (`cargo run -p mq-sync-server --example mock-oidc`).
// Runs only with MARQUEE_SIGNIN_RELAY set (xcodebuild passes it as
// TEST_RUNNER_MARQUEE_SIGNIN_RELAY); the auth session's browser is played
// by URLSession, which follows the provider's picker and stops at the return.

private let relay = ProcessInfo.processInfo.environment["MARQUEE_SIGNIN_RELAY"]

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

@Suite(.serialized) @MainActor struct AccountRelayTests {
    @Test func aBrokeredSignInMakesAnAccountThenFindsIt() async throws {
        guard let relay else { return }
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
        guard let relay else { return }
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
