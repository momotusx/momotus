#if !os(tvOS)
import SwiftUI

/// Sign in, first run's first step on a phone, tablet or Mac when the build
/// has a hosted relay (plans/accounts.md 4.1): Continue with each provider
/// the relay lists, Other ways to sign in, and Not now. Then what `found` and
/// `new` lead to. Built from the app's own pieces. The Mac draws it as a stage (`SignInStage`, variant 4 of
/// 2026-11-04): the first run's still, the step at the lower left with its ways in one row, and the phone's live
/// QR at the lower right while the step offers a join.
struct SignInScreen: View {
    @State private var flow: SignInFlow
    /// Not now, or Use my own sync server: today's doors.
    let notNow: () -> Void
    /// Joined an account that existed: Home.
    let joined: () -> Void
    /// Made a new account: the first-run steps go on.
    let created: () -> Void
    /// The line under the heading (Account's later sign-in: the library goes into the account).
    var lead: String?
    @State private var showOther = false
    @State private var showPairing = false
    @State private var showKit = false
    @State private var showMakeKit = false
    /// Other ways to sign in, asked for on an email stage: shown after its pop.
    @State private var otherAfterPop = false
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @Environment(CoreClient.self) private var core
    @Environment(\.metrics) private var metrics
    #if os(macOS)
    /// The first run's still behind the stage.
    @State private var still: FirstRunStill?
    #endif

    private typealias W = AccountWords
    private var words: AccountWords { .shared }

    init(server: String, sync: SyncModel, lead: String? = nil, notNow: @escaping () -> Void, joined: @escaping () -> Void, created: @escaping () -> Void) {
        _flow = State(initialValue: SignInFlow(server: server, sync: sync))
        self.lead = lead
        self.notNow = notNow
        self.joined = joined
        self.created = created
    }

    var body: some View {
        layout
        // The native flow is renewed while this runs: it stops when the screen closes.
        .task(id: flow.attempt) { await flow.open() }
        .onChange(of: flow.stage) { _, stage in
            if stage == .joined { joined() }
            if stage == .done { created() }
        }
        // A join another way (the QR) lands: the held sign-in goes to its account. The stage's own code (the Mac)
        // is that way too, and the join goes on as the step's joined.
        .onChange(of: sync.joined) { _, now in
            guard now else { return }
            let byCorner = cornerJoined
            Task {
                if byCorner {
                    await flow.joinedByCode()
                    joined()
                } else {
                    await flow.linkHeld()
                }
            }
        }
        .navigationDestination(isPresented: $showOther) { otherWays }
        .navigationDestination(isPresented: $showPairing) {
            PairingScreen(core: core, done: joined, autostart: flow.server, lede: words.phoneScanHow)
        }
        .navigationDestination(isPresented: $showKit) { SyncJoinScreen(kit: true, server: flow.server) }
        .navigationDestination(isPresented: $showMakeKit) { RecoveryKitScreen() }
        // Sign in with email as a push, so the system Back and the edge swipe work (accounts-email.md 4.8).
        .navigationDestination(isPresented: Binding(get: { flow.onEmail }, set: { shown in if !shown, flow.onEmail { flow.back() } })) {
            EmailSignIn(flow: flow) { otherAfterPop = true; flow.back() }
        }
        // Other ways from the email screen: pushed once its pop has settled, or iOS drops the push.
        .task(id: flow.stage) {
            guard otherAfterPop, flow.stage == .choose else { return }
            // Taken before the wait: a cancelled wait never leaves it set for a later return.
            otherAfterPop = false
            guard await settled() else { return }
            showOther = true
        }
    }

    #if os(macOS)
    private var layout: some View {
        SignInStage {
            VStack(alignment: .leading, spacing: 0) {
                Wordmark(size: 22)
                Text(isNew ? words.createAccount : words.signInTitle)
                    .textFont(52, weight: .bold, relativeTo: .largeTitle)
                    .foregroundStyle(.white)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.top, 14)
                if let lead {
                    Text(lead)
                        .textFont(17, relativeTo: .body)
                        .foregroundStyle(Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.top, 8)
                }
                if let trouble = flow.trouble, !flow.onEmail { troubleLine(trouble).padding(.top, Theme.Space.m) }
                stage.padding(.top, 28)
            }
        } corner: {
            if cornerShows { PhoneCorner(server: flow.server) }
        }
        .background { GeometryReader { page in FilmStill(still: still, wide: true, insets: page.safeAreaInsets) } }
        .background(Color.black)
        .task { still = await StillOnce.shared.get { [core] in try await core.call("firstRunStill", as: FirstRunStill?.self) } }
        // Escape is Not now on the first step, Cancel on the steps after a sign-in.
        .onExitCommand {
            switch flow.stage {
            case .choose: notNow()
            case .new, .cannotOpen, .startOver: flow.back()
            default: break
            }
        }
    }

    /// The steps that offer a join by another device show the code: the first, `new` and `cannotOpen`.
    private var cornerShows: Bool {
        guard !sync.keyUnreadable else { return false }
        switch flow.stage {
        case .choose, .new, .cannotOpen: return !flow.onEmail
        default: return false
        }
    }

    /// The corner's code joined this device (not the pushed pairing page's).
    private var cornerJoined: Bool {
        guard !showPairing, case .joined? = sync.pairing?.snapshot else { return false }
        return true
    }
    #else
    private var layout: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            HStack(alignment: .firstTextBaseline) {
                Wordmark(size: 22)
                Spacer()
            }
            Text(isNew ? words.createAccount : words.signInTitle)
                .display(metrics.wide ? 44 : Theme.type.title1 + 4)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            if let trouble = flow.trouble, !flow.onEmail { troubleLine(trouble) }
            stage
        }
    }

    private var cornerJoined: Bool { false }
    #endif

    /// A step's ways: one row on the Mac's stage, wrapping when the window is narrow; a column elsewhere.
    @ViewBuilder private func ways<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        #if os(macOS)
        FlowRow(spacing: Theme.Space.xs) { content() }
        #else
        VStack(alignment: .leading, spacing: Theme.Space.s) { content() }
        #endif
    }

    /// The Mac's stage draws Google's and Facebook's buttons by their own rules, and offers the phone's way in on
    /// the first step whatever the relay lists (the code is in the corner too).
    #if os(macOS)
    private static let stageWays = true
    #else
    private static let stageWays = false
    #endif

    // MARK: Stages

    /// No account uses the sign-in yet: the heading names the step, as on Android and the web.
    private var isNew: Bool { if case .new = flow.stage { true } else { false } }

    @ViewBuilder private var stage: some View {
        switch flow.stage {
        case .choose, .email, .code: choose
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
            ways {
                ForEach(flow.providers, id: \.self) { id in providerButton(id) }
                if flow.offersEmail {
                    // Its own row under the provider buttons; the primary when it is the only way listed.
                    Button { flow.openEmail() } label: {
                        Label(words.continueWithEmail, glyph: "mail").columnWide()
                    }
                    .buttonStyle(PillButtonStyle(prominent: flow.providers.isEmpty))
                }
                // No provider on this sync service: Create, or join by a phone's scan (plans/tv-qr-login.md 5.4); under
                // email when email is the way in.
                if flow.providers.isEmpty, !flow.unreachable {
                    Button { Task { await flow.createHere(name: DeviceName.current, userName: userName) } } label: {
                        Label(words.createAccount, glyph: "plus").columnWide()
                    }
                    .buttonStyle(PillButtonStyle(prominent: !flow.offersEmail))
                }
                if flow.providers.isEmpty || Self.stageWays, !flow.unreachable {
                    Button { showPairing = true } label: { Label(words.alreadyUse, glyph: "qr-code").columnWide() }
                        .buttonStyle(PillButtonStyle())
                }
            }
            #if os(macOS)
            HStack(spacing: 28) {
                Button { showOther = true } label: { Label(words.otherWays, glyph: "key-round") }
                Button(words.notNow, action: notNow)
                    .foregroundStyle(Theme.Palette.muted)
            }
            .buttonStyle(.textLink)
            .textFont(15, weight: .semibold, relativeTo: .callout)
            .foregroundStyle(Theme.Palette.text)
            .padding(.top, 6)
            #else
            Button { showOther = true } label: { Label(words.otherWays, glyph: "key-round") }
                .buttonStyle(PillButtonStyle())
            Button(words.notNow, action: notNow)
                .buttonStyle(.textLink)
                .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
            #endif
        }
    }

    /// Continue with a provider: Google and Facebook in their own buttons on the Mac's stage, the pill elsewhere.
    @ViewBuilder private func providerButton(_ id: String) -> some View {
        let title = AccountRules.fill(words.continueWith, ["provider": AccountRules.providerName(id)])
        if Self.stageWays, ProviderButtonStyle.draws(id) {
            Button { Task { await press(id) } } label: { Text(title) }
                .buttonStyle(ProviderButtonStyle(provider: id))
        } else {
            Button { Task { await press(id) } } label: { Label(title, glyph: "door-open").columnWide() }
                .buttonStyle(PillButtonStyle(prominent: id == flow.providers.first))
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
            ways {
                Button { flow.joinAnotherWay(ticket: ticket); showPairing = true } label: { Label(words.scanFromDevice, glyph: "qr-code") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                Button { showKit = true } label: { Label(words.kitUnknownPasskeyActions[0], glyph: "life-buoy") }
                    .buttonStyle(PillButtonStyle())
            }
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
            Text(words.noAccountYet)
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            ways {
                Button { Task { await flow.create(ticket: ticket, name: DeviceName.current, userName: userName) } } label: {
                    Label(words.createAccount, glyph: "plus")
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                Button { flow.joinAnotherWay(ticket: ticket); showPairing = true } label: { Label(words.alreadyUse, glyph: "qr-code") }
                    .buttonStyle(PillButtonStyle())
            }
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
        await flow.press(id, name: DeviceName.current)
    }

    /// The passkey's `user.name`: "<owner profile name> (Marquee)" (3.1).
    private var userName: String {
        let owner = profiles.snapshot?.profiles.first(where: \.isOwner)?.name ?? DeviceName.current
        return "\(owner) (Momotus)"
    }
}

/// Sign in with email, the address stage (plans/accounts-email.md 4.2): one
/// field and Send code. The code stage is pushed over it, so Back comes here
/// with the address kept. Also Account's Link another, with the flow's `finish`.
struct EmailSignIn: View {
    let flow: SignInFlow
    /// Other ways to sign in, from `emailLimit` and `mailUnavailable`.
    let otherWays: () -> Void
    @FocusState private var focused: Bool
    @AccessibilityFocusState private var heard: Bool
    private var words: AccountWords { .shared }

    var body: some View {
        EmailPage {
            ScreenTitle(text: words.emailTitle)
            EmailFieldBox(label: words.emailLabel, focused: focused) {
                // Read-only while sending (not disabled), so focus stays.
                TextField(words.emailLabel, text: Binding(get: { flow.address }, set: { if !flow.sending { flow.address = $0 } }), prompt: Text(verbatim: ""))
                    .textContentType(.emailAddress)
                    #if os(iOS)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .submitLabel(.send)
                    #endif
                    .focused($focused)
                    .accessibilityFocused($heard)
                    .onSubmit { flow.sendCode() }
            }
            EmailTrouble(flow: flow, otherWays: otherWays)
            Button { flow.sendCode() } label: { Label(words.emailSend, glyph: "mail") }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(flow.sending)
        }
        .navigationDestination(isPresented: Binding(get: { flow.stage == .code }, set: { shown in if !shown, flow.stage == .code { flow.toAddress() } })) {
            EmailCode(flow: flow, otherWays: otherWays)
        }
        // Each arrival at this stage (and back from the code): its field, its title said.
        .task(id: flow.stage) {
            guard flow.stage == .email, await settled() else { return }
            // A held ticket's Continue goes on to the code stage only now, its push after this one.
            flow.arrived()
            guard flow.stage == .email else { return }
            focused = true
            heard = true
            AccessibilityNotification.Announcement(words.emailTitle).post()
        }
    }
}

/// The code stage (4.3): the code's field, digits only and cut at 6, sent once
/// at the sixth digit or by Go; Send a new code after 60 s; Use another address.
struct EmailCode: View {
    let flow: SignInFlow
    let otherWays: () -> Void
    @FocusState private var focused: Bool
    @AccessibilityFocusState private var heard: Bool
    private var words: AccountWords { .shared }

    var body: some View {
        EmailPage {
            ScreenTitle(text: words.codeTitle)
            Text(AccountRules.fill(words.codeSentTo, ["email": flow.sentTo]))
                .textFont(Theme.type.body, relativeTo: .body)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
            if flow.resent { Said(tone: .ok, text: words.codeResent) }
            EmailFieldBox(label: words.codeLabel, focused: focused) {
                if flow.mailFlow == nil {
                    // The code can no longer be used: kept and readable, but no field to type
                    // in, for touch, keyboard and VoiceOver alike.
                    Text(verbatim: flow.code.isEmpty ? " " : flow.code)
                        .font(.mono(Theme.type.body))
                        .foregroundStyle(Theme.Palette.muted)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .accessibilityLabel(words.codeLabel)
                        .accessibilityValue(flow.code)
                } else {
                    TextField(words.codeLabel, text: Binding(get: { flow.code }, set: { flow.typeCode($0, name: DeviceName.current) }), prompt: Text(verbatim: ""))
                        .textContentType(.oneTimeCode)
                        #if os(iOS)
                        .keyboardType(.numberPad)
                        #endif
                        .font(.mono(Theme.type.body))
                        .focused($focused)
                        .accessibilityFocused($heard)
                        .onSubmit { flow.submitCode(name: DeviceName.current) }
                }
            }
            EmailTrouble(flow: flow, otherWays: otherWays)
            Text(words.codeMissing)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            resend
            Button(words.codeOtherAddress) { flow.toAddress() }
                .buttonStyle(.textLink)
                .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
        }
        .task(id: flow.stage) {
            guard flow.stage == .code else { return }
            await arriveAtStage(words.codeTitle) { focused = true; heard = true }
        }
        // A wrong code clears the field: focus goes back to it.
        .onChange(of: flow.troubleCode) { _, code in if code == "wrongCode" { focused = true } }
        // Send a new code gives the field back on the same stage: focus goes to it.
        .onChange(of: flow.mailFlow) { _, now in if now != nil { focused = true } }
        .onChange(of: flow.resent) { _, now in if now { AccessibilityNotification.Announcement(words.codeResent).post() } }
    }

    /// Send a new code, off for 60 s after the last mail, counted from its time.
    /// The countdown label is no live region: said once when it turns on.
    private var resend: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let left = AccountRules.resendIn(sentAt: flow.sentAt, now: context.date)
            Button { flow.resendCode() } label: {
                Label(left > 0 ? AccountRules.fill(words.codeResendIn, ["seconds": "\(left)"]) : words.codeResend, glyph: "refresh-cw")
            }
            .buttonStyle(PillButtonStyle())
            .disabled(left > 0 || flow.sending)
            .onChange(of: left == 0) { _, on in if on { AccessibilityNotification.Announcement(words.codeResend).post() } }
        }
    }
}

/// An email stage's line under the field, with the buttons its answer offers
/// (4.5). Try again repeats the stage's own call.
private struct EmailTrouble: View {
    let flow: SignInFlow
    let otherWays: () -> Void
    private var words: AccountWords { .shared }

    var body: some View {
        if let trouble = flow.trouble {
            VStack(alignment: .leading, spacing: Theme.Space.xs) {
                Said(tone: .bad, text: trouble.text)
                FlowRow(spacing: Theme.Space.xs) {
                    switch flow.troubleCode {
                    case "codeExpired", "signInRefused":
                        Button(words.codeExpiredActions[0]) { flow.resendCode() }
                            .buttonStyle(PillButtonStyle(prominent: true))
                        Button(words.codeExpiredActions[1]) { flow.toAddress() }
                            .buttonStyle(PillButtonStyle())
                    case "emailLimit":
                        Button(words.emailLimitActions[0], action: otherWays).buttonStyle(PillButtonStyle())
                    case "mailUnavailable":
                        Button(words.mailUnavailableActions[0]) { flow.tryAgain(name: DeviceName.current) }
                            .buttonStyle(PillButtonStyle(prominent: true))
                        Button(words.mailUnavailableActions[1], action: otherWays).buttonStyle(PillButtonStyle())
                    case "wrongCode", "badAddress", "emailWait":
                        // Fixed in the field, or by waiting: no button of its own.
                        EmptyView()
                    default:
                        // unreachable, busy and anything unforeseen: the stage's own call again.
                        Button(words.unreachableActions[0]) { flow.tryAgain(name: DeviceName.current) }
                            .buttonStyle(PillButtonStyle())
                    }
                }
                .disabled(flow.sending)
            }
            .onAppear { AccessibilityNotification.Announcement(trouble.text).post() }
            .onChange(of: trouble) { _, now in AccessibilityNotification.Announcement(now.text).post() }
        }
    }
}

/// An email stage's page, as the other sign-in pages.
private struct EmailPage<Content: View>: View {
    @ViewBuilder let content: Content
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) { content }
                .frame(maxWidth: 760, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, metrics.gutter)
                .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .scrollDismissesKeyboard(.interactively)
        .background(Theme.Palette.bg)
        .topScrim()
    }
}

/// Waits for a push or pop to settle; false when the waiting task was cancelled.
@MainActor private func settled() async -> Bool {
    // ponytail: a pushed field takes focus, and the next push or pop is kept, only after
    // the animation; 0.4 s covers it.
    try? await Task.sleep(for: .milliseconds(400))
    return !Task.isCancelled
}

/// A stage arrived: its title said, then focus to its field once the push has settled.
@MainActor private func arriveAtStage(_ title: String, focus: () -> Void) async {
    guard await settled() else { return }
    focus()
    AccessibilityNotification.Announcement(title).post()
}

/// A labelled field in the app's field chrome, its input set by the stage.
private struct EmailFieldBox<Field: View>: View {
    let label: String
    let focused: Bool
    @ViewBuilder let field: Field

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(caps: label)
                .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
                .accessibilityHidden(true)
            field
                .textFieldStyle(.plain)
                .font(.text(Theme.type.body))
                .autocorrectionDisabled()
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .fieldChrome(focused: focused)
        }
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
        #if os(macOS)
        // The Mac's stage says it under its heading.
        SignInScreen(server: server, sync: sync, lead: AccountWords.shared.libraryAdded, notNow: { dismiss() }, joined: { dismiss() }, created: { dismiss() })
        #else
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
        #endif
    }
}
#endif

#if os(macOS)
/// The stage's corner (`SignInStage`): "Sign in with your phone", how, and the live code, asked as the step opens
/// and renewed by the core while it shows, on a dark panel that keeps its words at 4.5:1 over any still; its own line
/// while the code is asked for or the join runs, and why it stopped with Start again. Leaving the step ends it.
private struct PhoneCorner: View {
    let server: String
    @Environment(SyncModel.self) private var sync

    private var words: AccountWords { .shared }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(words.tvSignIn)
                .textFont(17, weight: .bold, relativeTo: .headline)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            phase
        }
        .padding(20)
        .frame(width: SignInStage<EmptyView, EmptyView>.cornerWidth, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Theme.Radius.l).fill(.black.opacity(0.62)))
        .accessibilityElement(children: .contain)
        .task { start() }
        // Opens it again after a core restart.
        .task(id: sync.pairing?.taskId) { sync.pairing?.start() }
        .onDisappear { if !joined { sync.endPairing() } }
    }

    @ViewBuilder private var phase: some View {
        if let failure = sync.pairing?.failure {
            Label(PairingWords.problem(failure), glyph: "triangle-alert")
                .textFont(14, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
            Button { sync.endPairing(); start() } label: { Label("Start again", glyph: "refresh-cw") }
                .buttonStyle(PillButtonStyle())
        } else {
            switch sync.pairing?.snapshot {
            case .waiting(let qr, _)?:
                HStack(alignment: .top, spacing: 18) {
                    // A new code takes the old one's place, at the same size.
                    QRCode(text: qr, pairing: true)
                        .frame(width: 140, height: 140)
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                    Text(words.tvSignInHow)
                        .textFont(14, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.text)
                        .fixedSize(horizontal: false, vertical: true)
                }
            case .joining?, .joined?:
                Label(words.approveSigningIn, glyph: "clock")
                    .textFont(14, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
            default:
                Label("Asking the sync server for a code", glyph: "clock")
                    .textFont(14, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
            }
        }
    }

    private var joined: Bool {
        if case .joined? = sync.pairing?.snapshot { return true }
        return false
    }

    private func start() {
        guard sync.pairing == nil, !sync.joined, !sync.keyUnreadable else { return }
        UserDefaults.standard.set(server, forKey: SyncRecord.serverKey)
        sync.startPairing(server: server, name: DeviceName.current)
    }
}
#endif

#if !os(tvOS)
private extension View {
    /// A way's label fills the column's width; on the Mac's stage the ways share a row at their own widths.
    @ViewBuilder func columnWide() -> some View {
        #if os(macOS)
        self
        #else
        frame(maxWidth: .infinity)
        #endif
    }
}
#endif
