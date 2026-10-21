#if !os(tvOS)
import SwiftUI

/// Sign in, first run's first step on a phone, tablet or Mac when the build
/// has a hosted relay (plans/accounts.md 4.1): Continue with each provider
/// the relay lists, Other ways to sign in, and Not now. Then what `found` and
/// `new` lead to. Built from the app's own pieces; the design pick restyles it.
struct SignInScreen: View {
    @State private var flow: SignInFlow
    /// Not now, or Use my own sync server: today's doors.
    let notNow: () -> Void
    /// Joined an account that existed: Home.
    let joined: () -> Void
    /// Made a new account: the first-run steps go on.
    let created: () -> Void
    @State private var showOther = false
    @State private var showPairing = false
    @State private var showKit = false
    @State private var showMakeKit = false
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @Environment(CoreClient.self) private var core
    @Environment(\.metrics) private var metrics

    private typealias W = AccountWords
    private var words: AccountWords { .shared }

    init(server: String, sync: SyncModel, notNow: @escaping () -> Void, joined: @escaping () -> Void, created: @escaping () -> Void) {
        _flow = State(initialValue: SignInFlow(server: server, sync: sync))
        self.notNow = notNow
        self.joined = joined
        self.created = created
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            HStack(alignment: .firstTextBaseline) {
                Wordmark(size: 22)
                Spacer()
            }
            Text(words.signInTitle)
                .display(metrics.wide ? 44 : Theme.type.title1 + 4)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            if let trouble = flow.trouble { troubleLine(trouble) }
            stage
        }
        // The native flow is renewed while this runs: it stops when the screen closes.
        .task(id: flow.attempt) { await flow.open() }
        .onChange(of: flow.stage) { _, stage in
            if stage == .joined { joined() }
            if stage == .done { created() }
        }
        // A join another way (the QR) lands: the held sign-in goes to its account.
        .onChange(of: sync.joined) { _, now in
            guard now else { return }
            Task { await flow.linkHeld() }
        }
        .navigationDestination(isPresented: $showOther) { otherWays }
        .navigationDestination(isPresented: $showPairing) {
            PairingScreen(core: core, done: joined, autostart: flow.server)
        }
        .navigationDestination(isPresented: $showKit) { SyncJoinScreen(kit: true, server: flow.server) }
        .navigationDestination(isPresented: $showMakeKit) { RecoveryKitScreen() }
    }

    // MARK: Stages

    @ViewBuilder private var stage: some View {
        switch flow.stage {
        case .choose: choose
        case .working, .created:
            ProgressView().controlSize(.large).tint(Theme.Palette.text)
        case .cannotOpen(let ticket):
            cannotOpen(ticket)
        case .new(let ticket):
            newAccount(ticket)
        case .startOver(let ticket):
            startOver(ticket)
        case .noSecondWayIn:
            noSecondWayIn
        case .joined, .done:
            ProgressView().controlSize(.large).tint(Theme.Palette.text)
        }
    }

    private var choose: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if flow.unreachable {
                Said(tone: .bad, text: words.unreachable)
                FlowRow(spacing: Theme.Space.xs) {
                    Button(words.unreachableActions[0]) { flow.retry() }
                        .buttonStyle(PillButtonStyle(prominent: true))
                    Button(words.unreachableActions[1], action: notNow)
                        .buttonStyle(PillButtonStyle())
                }
            }
            ForEach(flow.providers, id: \.self) { id in
                Button { Task { await press(id) } } label: {
                    Label(AccountRules.fill(words.continueWith, ["provider": AccountRules.providerName(id)]), glyph: "door-open")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(PillButtonStyle(prominent: id == flow.providers.first))
            }
            Button { showOther = true } label: { Label(words.otherWays, glyph: "key-round") }
                .buttonStyle(PillButtonStyle(prominent: flow.providers.isEmpty))
            Button(words.notNow, action: notNow)
                .buttonStyle(.textLink)
                .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
        }
    }

    /// Other ways to sign in: a passkey, a scan, a recovery kit, the user's own server.
    private var otherWays: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                ScreenTitle(text: words.otherWays)
                if PasskeyContract.relyingParty != nil {
                    Button { showOther = false; Task { await flow.usePasskey(name: DeviceName.current) } } label: {
                        SettingsRow(symbol: "key-round", title: words.usePasskey, detail: "")
                    }
                    .buttonStyle(.row)
                }
                Button { showOther = false; showPairing = true } label: {
                    SettingsRow(symbol: "qr-code", title: words.scanFromDevice, detail: "")
                }
                .buttonStyle(.row)
                Button { showOther = false; showKit = true } label: {
                    SettingsRow(symbol: "life-buoy", title: words.useRecoveryKit, detail: "")
                }
                .buttonStyle(.row)
                Button { showOther = false; notNow() } label: {
                    SettingsRow(symbol: "wrench", title: words.useOwnServer, detail: "")
                }
                .buttonStyle(.row)
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .background(Theme.Palette.bg)
    }

    /// Found, but no passkey here opens it: the QR on a signed-in device, a
    /// recovery kit, or Start over.
    private func cannotOpen(_ ticket: String?) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if flow.trouble == nil { Said(tone: .warn, text: words.kitUnknownPasskey) }
            Button { flow.joinAnotherWay(ticket: ticket); showPairing = true } label: { Label(words.scanFromDevice, glyph: "qr-code") }
                .buttonStyle(PillButtonStyle(prominent: true))
            Button { showKit = true } label: { Label(words.kitUnknownPasskeyActions[0], glyph: "life-buoy") }
                .buttonStyle(PillButtonStyle())
            if let ticket {
                Button(words.kitUnknownPasskeyActions[1]) { flow.goStartOver(ticket: ticket) }
                    .buttonStyle(.textLink)
                    .foregroundStyle(Theme.Palette.text)
            }
            back
        }
    }

    /// `new` once no passkey here opened an account (the probe's sheet was 4.1.4's line).
    private func newAccount(_ ticket: String) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Button { Task { await flow.create(ticket: ticket, name: DeviceName.current, userName: userName) } } label: {
                Label(words.createAccount, glyph: "plus")
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            Button { flow.joinAnotherWay(ticket: ticket); showPairing = true } label: { Label(words.alreadyUse, glyph: "qr-code") }
                .buttonStyle(PillButtonStyle())
            back
        }
    }

    private func startOver(_ ticket: String) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Said(tone: .warn, text: words.startOverWarning)
            Button { Task { await flow.startOver(ticket: ticket, name: DeviceName.current, userName: userName) } } label: {
                Label(words.createAccount, glyph: "plus")
            }
            .buttonStyle(PillButtonStyle(prominent: true))
            back
        }
    }

    /// No passkey after creating (4.1.6): Try again, a recovery kit instead, or Later.
    private var noSecondWayIn: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Said(tone: .warn, text: words.noSecondWayIn)
            FlowRow(spacing: Theme.Space.xs) {
                Button(words.noSecondWayInActions[0]) { Task { await flow.savePasskey(userName: userName) } }
                    .buttonStyle(PillButtonStyle(prominent: true))
                Button(words.noSecondWayInActions[1]) { showMakeKit = true }
                    .buttonStyle(PillButtonStyle())
                Button(words.noSecondWayInActions[2]) { flow.later() }
                    .buttonStyle(PillButtonStyle())
            }
        }
    }

    private var back: some View {
        Button(SyncWords.cancel) { flow.back() }
            .buttonStyle(.textLink)
            .foregroundStyle(Theme.Palette.muted)
    }

    /// A step's words with its buttons, when they do something here.
    private func troubleLine(_ trouble: AccountRules.Trouble) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Said(tone: .bad, text: trouble.text)
            if flow.troubleCode == "passkeyCancelled" {
                FlowRow(spacing: Theme.Space.xs) {
                    Button(words.passkeyCancelledActions[0]) { Task { await flow.usePasskey(name: DeviceName.current) } }
                        .buttonStyle(PillButtonStyle())
                    Button(words.passkeyCancelledActions[1]) { showPairing = true }
                        .buttonStyle(PillButtonStyle())
                }
            } else if flow.troubleCode == "signupLimit" {
                Button(words.signupLimitActions[0]) { showOther = true }.buttonStyle(PillButtonStyle())
            } else if flow.troubleCode == "ticketExpired", let provider = flow.provider {
                Button(words.ticketExpiredActions[0]) { Task { await press(provider) } }.buttonStyle(PillButtonStyle())
            }
        }
    }

    // MARK: Helpers

    private func press(_ id: String) async {
        if id == "apple" { await flow.continueWithApple(name: DeviceName.current) } else { await flow.continueWith(id, name: DeviceName.current) }
    }

    /// The passkey's `user.name`: "<owner profile name> (Marquee)" (3.1).
    private var userName: String {
        let owner = profiles.snapshot?.profiles.first(where: \.isOwner)?.name ?? DeviceName.current
        return "\(owner) (Marquee)"
    }
}
#endif

#if !os(tvOS)
/// Signing in later, from Account (4.1.5): the device's library goes into the account.
struct LaterSignIn: View {
    let server: String
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                Callout(glyph: "info", text: AccountWords.shared.libraryAdded)
                SignInScreen(server: server, sync: sync, notNow: { dismiss() }, joined: { dismiss() }, created: { dismiss() })
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
    }
}
#endif
