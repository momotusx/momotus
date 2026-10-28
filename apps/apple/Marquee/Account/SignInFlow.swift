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
        /// No account for this identity, and no passkey here opened one:
        /// create one, or join the account another device holds.
        case new(ticket: String)
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
        /// Sign in with email: the address (plans/accounts-email.md 4).
        case email
        /// Enter the code mailed to `sentTo`.
        case code
    }

    /// The email calls: the core's, unless a test scripts them.
    struct Mail {
        var start: @MainActor (_ server: String, _ email: String) async throws -> EmailStart
        var take: @MainActor (_ flow: String, _ code: String) async throws -> SignInSnapshot
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
    /// Bumped by Try again: the screen's task opens the flow again.
    private(set) var attempt = 0
    /// A sign-in linked once this device joined another way (the pairing door).
    private(set) var linkAfterJoin: HeldLink?

    // Email, kept in memory only while this flow lives (E12).
    /// The relay lists `email`: the "Continue with email" row.
    private(set) var offersEmail = false
    /// The address field, as typed.
    var address = ""
    /// The code field: digits only, cut at 6 (`typeCode`).
    private(set) var code = ""
    /// The address the newest code went to, and when: the resend timer counts from it.
    private(set) var sentTo = ""
    private(set) var sentAt: Date?
    /// A resend worked: `codeResent` is said.
    private(set) var resent = false
    /// A start or a code is with the relay: the field read-only, the button busy.
    private(set) var sending = false
    /// The newest flow while its code may still work; nil once used, ended or refused.
    private(set) var mailFlow: String?
    /// On the address or the code stage.
    var onEmail: Bool { stage == .email || stage == .code }

    let server: String
    @ObservationIgnored private let sync: SyncModel
    @ObservationIgnored private let sheets: SignInSheets.Calls
    @ObservationIgnored private let mail: Mail
    /// Account's Link another: the code's answer goes here instead of `signedIn`.
    @ObservationIgnored private let finish: (@MainActor (SignInSnapshot) async -> Void)?
    @ObservationIgnored private var native: NativeStart?
    @ObservationIgnored private var brokered: LiveRecord<SignInSnapshot>?
    /// Try again on the code stage repeats the last call: a send, else the code.
    @ObservationIgnored private var lastWasSend = false

    init(server: String, sync: SyncModel, sheets: SignInSheets.Calls = .init(), mail: Mail? = nil,
         finish: (@MainActor (SignInSnapshot) async -> Void)? = nil) {
        self.server = server
        self.sync = sync
        self.sheets = sheets
        self.mail = mail ?? Mail(start: { try await sync.emailSignInStart(server: $0, email: $1) },
                                 take: { try await sync.emailSignIn(flow: $0, code: $1) })
        self.finish = finish
    }

    // MARK: Opening

    /// The relay's providers, then Apple's native flow ready before the button
    /// is pressed, renewed before its 10 minutes end (4.3) for as long as the
    /// screen's task runs: it ends when the screen closes.
    func open() async {
        do {
            let listed = try await sync.relayInfo(server: server)
            providers = AccountRules.orderedProviders(listed)
            offersEmail = listed.contains("email")
            unreachable = false
        } catch {
            unreachable = true
            return
        }
        guard providers.contains("apple") else { return }
        while !Task.isCancelled {
            native = try? await sync.nativeSignInStart(server: server, provider: "apple")
            // ponytail: renewed every 9 minutes; the relay keeps a flow 10.
            try? await Task.sleep(for: .seconds(9 * 60))
        }
    }

    /// Try again when the relay did not answer: the screen's task opens it again.
    func retry() {
        attempt += 1
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
            switch try await sheets.apple(start.nonce) {
            case .passkey(let prf):
                await recover(prf: prf, name: name, link: nil, otherwise: .cannotOpen(ticket: nil))
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
            let fragment = try await sheets.browser(address)
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
            let passkey = try await sheets.passkey(false)
            await recover(prf: passkey.prf, name: name, link: nil, otherwise: .cannotOpen(ticket: nil))
        } catch {
            if case SignInSheets.Failure.cancelled = error {
                say("passkeyCancelled")
                stage = .choose
            } else {
                fail(error)
            }
        }
    }

    /// A provider's sign-in, from its button or from the trouble line's
    /// Continue after a held ticket ran out. Email there is the code stage
    /// with the address still in memory and a new code (accounts-email.md 4.6).
    func press(_ id: String, name: String) async {
        switch id {
        case "email": await emailAgain()
        case "apple": await continueWithApple(name: name)
        default: await continueWith(id, name: name)
        }
    }

    // MARK: Email (plans/accounts-email.md 4)

    /// Continue with email: the address stage, the address kept from before.
    func openEmail() {
        provider = "email"
        trouble = nil
        troubleCode = nil
        stage = .email
    }

    /// Back from the code stage, or Use another address: the address stage, prefilled.
    func toAddress() {
        trouble = nil
        troubleCode = nil
        code = ""
        stage = .email
    }

    /// Send code. The same address again while its code may still work goes
    /// back to the code stage without a new mail.
    func sendCode() async {
        let typed = address.trimmingCharacters(in: .whitespacesAndNewlines)
        if typed == sentTo, mailFlow != nil, let sentAt, Date.now.timeIntervalSince(sentAt) < 10 * 60 {
            toCode()
            return
        }
        await start(typed, resend: false)
    }

    /// Send a new code to the address the last one went to.
    func resendCode() async {
        await start(sentTo, resend: true)
    }

    /// Typed into the code field: digits only, cut at 6; the sixth digit submits once.
    /// Read-only while sending, and once the code can no longer be used.
    func typeCode(_ typed: String, name: String) {
        guard !sending, mailFlow != nil else { return }
        let before = code
        code = AccountRules.codeDigits(typed)
        if code.count == 6, code != before { Task { await submitCode(name: name) } }
    }

    /// The code to the relay, once at a time; its answer ends as a provider's does.
    func submitCode(name: String) async {
        guard !sending, stage == .code, code.count == 6, let flow = mailFlow else { return }
        sending = true
        defer { sending = false }
        lastWasSend = false
        trouble = nil
        troubleCode = nil
        let answer: SignInSnapshot
        do {
            answer = try await mail.take(flow, code)
        } catch {
            return codeFailed((error as? CoreError)?.code ?? "unreachable", error)
        }
        if case .failed(let refused) = answer { return codeFailed(refused, nil) }
        mailFlow = nil
        if let finish { await finish(answer) } else { await signedIn(answer, name: name) }
    }

    /// Try again on an email stage repeats that stage's own call, never the provider flow.
    func tryAgain(name: String) async {
        switch stage {
        case .email: await sendCode()
        case .code: if lastWasSend { await resendCode() } else { await submitCode(name: name) }
        default: retry()
        }
    }

    private func toCode() {
        trouble = nil
        troubleCode = nil
        code = ""
        stage = .code
    }

    private func start(_ typed: String, resend: Bool) async {
        guard !sending else { return }
        sending = true
        defer { sending = false }
        lastWasSend = true
        trouble = nil
        troubleCode = nil
        resent = false
        do {
            let started = try await mail.start(server, typed)
            mailFlow = started.flow
            sentTo = typed
            sentAt = .now
            toCode()
            resent = resend
        } catch {
            fail(error)
        }
    }

    /// The held ticket ran out: a new code to the address in memory; with none
    /// (the app was restarted), the address stage, empty.
    private func emailAgain() async {
        openEmail()
        guard !sentTo.isEmpty else { return }
        address = sentTo
        mailFlow = nil
        toCode()
        await resendCode()
    }

    /// The code stage's answers table: the stage stays, its words and actions change.
    private func codeFailed(_ refused: String, _ error: (any Error)?) {
        switch refused {
        case "wrongCode", "badCode":
            say("wrongCode")
            code = ""
        case "ticketExpired", "wrongFlow":
            say("codeExpired")
            mailFlow = nil
        case "signInRefused":
            troubleCode = refused
            trouble = AccountRules.Trouble(text: AccountRules.words.signInRefused, actions: AccountRules.words.codeExpiredActions)
            mailFlow = nil
        case "emailLimit":
            say(refused)
            mailFlow = nil
        default:
            say(refused)
            if trouble == nil { trouble = AccountRules.Trouble(text: error?.localizedDescription ?? AccountRules.words.unreachable, actions: []) }
        }
    }

    // MARK: Found and new

    /// What a sign-in answered. `found`: a passkey already here, made for the
    /// found account, opens it; none, another account's (a device may hold
    /// passkeys for two), or one that opens nothing, leads to the QR (4.1.3). `new`: a
    /// passkey here opens the account it belongs to in that one sheet, and the
    /// identity is linked to it (4.1.4); none, a closed sheet, or one that
    /// opens nothing leads to Create, the sign-in kept.
    func signedIn(_ answer: SignInSnapshot, name: String) async {
        switch answer {
        case .found(let ticket, let account):
            // Asked for a passkey on this device only, no QR to another phone.
            if let here = try? await sheets.passkey(true), PasskeyContract.isFor(here.user, account: account) {
                await recover(prf: here.prf, name: name, link: nil, otherwise: .cannotOpen(ticket: ticket))
            } else {
                stage = .cannotOpen(ticket: ticket)
            }
        case .new(let ticket):
            if let here = try? await sheets.passkey(true) {
                await recover(prf: here.prf, name: name, link: ticket, otherwise: .new(ticket: ticket))
            } else {
                stage = .new(ticket: ticket)
            }
        case .failed(let code):
            if code != "cancelled" { say(code) }
            stage = .choose
        default:
            stage = .choose
        }
    }

    /// A passkey's PRF output opens its account here; then the held sign-in is
    /// linked to it. One that opens nothing (`kitUnknown`) goes to `otherwise`.
    private func recover(prf: Data, name: String, link: String?, otherwise: Stage) async {
        stage = .working
        do {
            try await sync.recover(server: server, prf: prf, name: name)
            if let link { await self.link(HeldLink(ticket: link, provider: provider ?? "")) }
            stage = .joined
        } catch let error as CoreError where error.code == "kitUnknown" {
            stage = otherwise
        } catch {
            fail(error)
        }
    }

    /// Links a held sign-in to the account this device just joined. One that
    /// another account holds waits for Account's "Use it for this account instead".
    private func link(_ held: HeldLink) async {
        do {
            try await sync.linkIdentity(ticket: held.ticket)
        } catch let refusal as CoreError where refusal.code == "linkedElsewhere" {
            sync.heldLink = held
        } catch {
            // ponytail: any other failure is offered again by Account's Link another.
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
        linkAfterJoin = ticket.map { HeldLink(ticket: $0, provider: provider ?? "") }
    }

    /// The join another way has landed: the held identity goes to this account.
    func linkHeld() async {
        guard let held = linkAfterJoin, sync.joined else { return }
        linkAfterJoin = nil
        await link(held)
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
            stage = .new(ticket: ticket)
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
            let prf = try await sheets.create(account, userName)
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

    /// A failed step in words. An email stage keeps its stage (accounts-email.md
    /// 4.4); any other goes back to the sign-in buttons.
    private func fail(_ error: any Error) {
        if case SignInSheets.Failure.cancelled = error {
            if !onEmail { stage = .choose }
            return
        }
        let code = (error as? CoreError)?.code ?? "unreachable"
        say(code)
        if trouble == nil { trouble = AccountRules.Trouble(text: error.localizedDescription, actions: []) }
        if !onEmail { stage = .choose }
    }
}
#endif

#if !os(tvOS)
/// Save a passkey here, on Account (3.1): a passkey already on this device
/// that was made for this account is tried first ("Your passkey already works
/// on this device", nothing saved). One made for another account (its
/// `user.id`) is left alone and a new one is made, so it never ends in
/// `kitTaken`, also after Start over.
enum PasskeySave {
    enum Outcome: Equatable { case saved, alreadyHere }

    @MainActor
    static func here(sync: SyncModel, sheets: SignInSheets.Calls = .init(), userName: String) async throws -> Outcome {
        let list = try await sync.loadDevices()
        guard let account = list.account.flatMap(PasskeyContract.userID) else { throw CoreError("no account id", code: "unreachable") }
        let label = AccountRules.passkeyLabel(device: DeviceName.current)
        if let here = try? await sheets.passkey(true), here.user == account {
            do {
                try await sync.savePasskey(prf: here.prf, label: label)
                return .saved
            } catch let refusal as CoreError where refusal.code == "kitHere" {
                return .alreadyHere
            } catch let refusal as CoreError where refusal.code == "kitDropped" {
                // Removed once, it never comes back: a new one below.
            }
        }
        try await sync.savePasskey(prf: try await sheets.create(account, userName), label: label)
        return .saved
    }
}
#endif
