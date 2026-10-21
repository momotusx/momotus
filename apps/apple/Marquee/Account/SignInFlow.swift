import Foundation
import Observation

/// The sync service this build signs in to: the hosted relay the build
/// carries (`MARQUEE_SYNC_SERVER`), or a debug run's `-MQSyncServer`. None:
/// first run shows today's doors and the ways in keep their server field.
enum AccountServer {
    @MainActor static var `default`: String? {
        #if DEBUG
        if let walk = UserDefaults.standard.string(forKey: "MQSyncServer"), !walk.isEmpty { return walk }
        #endif
        let built = Bundle.main.object(forInfoDictionaryKey: "MarqueeSyncServer") as? String
        return built?.isEmpty == false ? built : nil
    }
}

#if !os(tvOS)
/// Signing in on a phone, tablet or Mac (plans/accounts.md 4.1, 5): the
/// providers the relay lists, Continue with Apple as one sheet with a
/// passkey, the brokered sign-in for the others, then what `found` and
/// `new` lead to. The core finds the account; only a passkey, a scan from a
/// signed-in device or a recovery kit opens it.
@MainActor @Observable
final class SignInFlow {
    enum Stage: Equatable {
        /// The sign-in buttons.
        case choose
        case working
        /// The identity has an account this device cannot open yet: the
        /// pairing QR, a recovery kit, or I can't use any of these.
        case cannotOpen(ticket: String?)
        /// No account for this identity: create one, or join the account another device holds.
        case new(ticket: String, passkeyHere: Bool)
        /// "I can't use any of these": a new account in place of the old one, after its warning.
        case startOver(ticket: String)
        /// The account was just made: save a passkey as its way in.
        case created
        /// No passkey after creating (none, cancelled, no PRF): 4.1.6.
        case noSecondWayIn
        /// Joined an account that existed: Home after its first pull.
        case joined
        /// Made a new account and its first way in, or Later: the first-run steps go on.
        case done
    }

    private(set) var stage: Stage = .choose
    private(set) var providers: [String] = []
    /// What the last step said, in words, with its buttons.
    private(set) var trouble: AccountRules.Trouble?
    private(set) var troubleCode: String?
    /// The provider of the sign-in that runs or ran, for its words.
    private(set) var provider: String?
    /// The relay could not be asked for its providers.
    private(set) var unreachable = false
    /// A ticket linked once this device joined another way (the pairing door, a passkey).
    private(set) var linkAfterJoin: String?

    let server: String
    @ObservationIgnored private let sync: SyncModel
    @ObservationIgnored private var native: NativeStart?
    @ObservationIgnored private var renewal: Task<Void, Never>?
    @ObservationIgnored private var brokered: LiveRecord<SignInSnapshot>?

    init(server: String, sync: SyncModel) {
        self.server = server
        self.sync = sync
    }

    deinit {
        renewal?.cancel()
    }

    // MARK: Opening

    /// The relay's providers, and Apple's native flow ready before the button
    /// is pressed, renewed before its 10 minutes end (4.3).
    func open() async {
        do {
            providers = AccountRules.orderedProviders(try await sync.relayInfo(server: server))
            unreachable = false
        } catch {
            unreachable = true
            return
        }
        guard providers.contains("apple") else { return }
        renewal?.cancel()
        renewal = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                self.native = try? await self.sync.nativeSignInStart(server: self.server, provider: "apple")
                // ponytail: renewed every 9 minutes; the relay keeps a flow 10.
                try? await Task.sleep(for: .seconds(9 * 60))
            }
        }
    }

    func back() {
        stage = .choose
        trouble = nil
        troubleCode = nil
    }

    // MARK: Providers

    /// Continue with Apple: one sheet, the Apple ID or a Marquee passkey.
    func continueWithApple(name: String) async {
        provider = "apple"
        stage = .working
        trouble = nil
        do {
            let start: NativeStart
            if let native { start = native } else { start = try await sync.nativeSignInStart(server: server, provider: "apple") }
            // Used once: the next press starts a new flow.
            native = nil
            switch try await SignInSheets.continueWithApple(nonce: start.nonce) {
            case .passkey(let prf):
                await recover(prf: prf, name: name, link: nil)
            case .apple(let token, _):
                let answer = try await sync.nativeSignIn(flow: start.flow, idToken: token)
                await signedIn(answer, name: name)
            }
        } catch {
            fail(error)
        }
        renewNative()
    }

    /// Google, Facebook or another provider: the relay's page in the
    /// browser session, its return handed to the view.
    func continueWith(_ id: String, name: String) async {
        provider = id
        stage = .working
        trouble = nil
        let view = sync.signInView(server: server, provider: id)
        brokered = view
        view.start()
        defer { brokered = nil }
        do {
            guard case .browser(let url, _) = await next(view, where: { if case .starting = $0 { false } else { true } }) else {
                return await signedIn(view.snapshot ?? .failed(code: "unreachable"), name: name)
            }
            guard let address = URL(string: url) else { throw CoreError("not a link", code: "unreachable") }
            let fragment = try await SignInSheets.browser(address)
            try await sync.returned(view, fragment: fragment)
            let answer = await next(view, where: { switch $0 { case .found, .new, .failed, .receipt: true; default: false } })
            await signedIn(answer, name: name)
        } catch {
            fail(error)
        }
    }

    /// Use a passkey (Other ways): any Marquee passkey, then the account it opens.
    func usePasskey(name: String) async {
        provider = nil
        stage = .working
        trouble = nil
        do {
            let prf = try await SignInSheets.passkey(immediate: false)
            await recover(prf: prf, name: name, link: nil)
        } catch {
            if case SignInSheets.Failure.cancelled = error {
                say("passkeyCancelled")
                stage = .choose
            } else {
                fail(error)
            }
        }
    }

    // MARK: Found and new

    /// `found`: a passkey already here opens it; none, or one that opens
    /// nothing, leads to the QR. `new`: create, or join another device's account.
    private func signedIn(_ answer: SignInSnapshot, name: String) async {
        switch answer {
        case .found(let ticket, _):
            // 4.1.3: ask for a passkey here only, no QR to another phone.
            if let prf = try? await SignInSheets.passkey(immediate: true) {
                await recover(prf: prf, name: name, link: nil)
            } else {
                stage = .cannotOpen(ticket: ticket)
            }
        case .new(let ticket):
            let here = (try? await SignInSheets.passkey(immediate: true)) != nil
            stage = .new(ticket: ticket, passkeyHere: here)
        case .failed(let code):
            if code == "cancelled" { stage = .choose } else { say(code); stage = .choose }
        default:
            stage = .choose
        }
    }

    /// The passkey here on `new`: it recovers the account it opens, and the
    /// identity is then linked with the ticket held.
    func usePasskeyHere(ticket: String, name: String) async {
        stage = .working
        do {
            let prf = try await SignInSheets.passkey(immediate: false)
            await recover(prf: prf, name: name, link: ticket)
        } catch {
            fail(error)
        }
    }

    private func recover(prf: Data, name: String, link: String?) async {
        do {
            try await sync.recover(server: server, prf: prf, name: name)
            if let link { try? await sync.linkIdentity(ticket: link) }
            stage = .joined
        } catch let error as CoreError where error.code == "kitUnknown" {
            say("kitUnknown")
            stage = .cannotOpen(ticket: link)
        } catch {
            fail(error)
        }
    }

    /// "I can't use any of these": Start over, after its warning.
    func goStartOver(ticket: String) {
        trouble = nil
        stage = .startOver(ticket: ticket)
    }

    /// "I already use Marquee on another device": the pairing QR; the ticket
    /// is linked once the join is stored (`linkHeld`).
    func joinAnotherWay(ticket: String?) {
        linkAfterJoin = ticket
    }

    /// The join another way has landed: the held identity goes to this account.
    func linkHeld() async {
        guard let ticket = linkAfterJoin, sync.joined else { return }
        linkAfterJoin = nil
        do {
            try await sync.linkIdentity(ticket: ticket)
        } catch {
            // ponytail: a link that fails here is offered again from Account's Link another.
            say((error as? CoreError)?.code ?? "")
        }
    }

    // MARK: Creating

    /// Create your Marquee account: the account with the ticket, then its passkey.
    func create(ticket: String, name: String, userName: String) async {
        stage = .working
        trouble = nil
        do {
            try await sync.createAccount(server: server, invite: "", name: name, ticket: ticket)
            stage = .created
            await savePasskey(userName: userName)
        } catch {
            fail(error)
            stage = .new(ticket: ticket, passkeyHere: false)
        }
    }

    /// Start over after its warning: a new account takes the identity from the old one.
    func startOver(ticket: String, name: String, userName: String) async {
        stage = .working
        trouble = nil
        do {
            try await sync.createAccount(server: server, invite: "", name: name, ticket: ticket, move: true)
            stage = .created
            await savePasskey(userName: userName)
        } catch {
            fail(error)
            stage = .startOver(ticket: ticket)
        }
    }

    /// The new account's passkey (4.1.4); no PRF or a closed sheet goes to 4.1.6.
    func savePasskey(userName: String) async {
        stage = .working
        do {
            let list = try await sync.loadDevices()
            guard let account = list.account.flatMap(PasskeyContract.userID) else { throw SignInSheets.Failure.noRelyingParty }
            let prf = try await SignInSheets.createPasskey(account: account, userName: userName)
            try await sync.savePasskey(prf: prf, label: AccountRules.passkeyLabel(device: DeviceName.current))
            stage = .done
        } catch {
            stage = .noSecondWayIn
        }
    }

    /// Later, on 4.1.6: the first-run steps go on without a second way in.
    func later() {
        stage = .done
    }

    // MARK: Helpers

    /// Starts a fresh native flow for the next press.
    private func renewNative() {
        guard providers.contains("apple") else { return }
        Task { [weak self] in
            guard let self else { return }
            self.native = try? await self.sync.nativeSignInStart(server: self.server, provider: "apple")
        }
    }

    /// The view's next snapshot that matches; a view that failed to open, or
    /// a minute with no answer, is `unreachable`.
    private func next(_ view: LiveRecord<SignInSnapshot>, where matches: (SignInSnapshot) -> Bool) async -> SignInSnapshot {
        // ponytail: polled every 100 ms; the view's own push would wake it at once.
        for _ in 0..<600 {
            if let now = view.snapshot, matches(now) { return now }
            if view.failure != nil { break }
            try? await Task.sleep(for: .milliseconds(100))
        }
        return .failed(code: "unreachable")
    }

    private func say(_ code: String) {
        troubleCode = code
        trouble = AccountRules.trouble(code, provider: provider)
    }

    private func fail(_ error: any Error) {
        if case SignInSheets.Failure.cancelled = error {
            stage = .choose
            return
        }
        let code = (error as? CoreError)?.code ?? "unreachable"
        say(code)
        if trouble == nil { trouble = AccountRules.Trouble(text: error.localizedDescription, actions: []) }
        stage = .choose
    }
}
#endif
