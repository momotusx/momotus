#if !os(tvOS)
import SwiftUI

/// Sign in, first run's Login sheet on a phone, tablet or Mac with a hosted relay (plans/first-screens.md 3.3, D10,
/// combo A): "Sign in" with Not now at the right and the sheet's line, Continue with each provider the relay lists
/// (Google and Facebook in their own buttons on every form), Continue with email, Use another device on the phone,
/// Other ways to sign in; on wide forms the phone's live QR beside the ways (`SignInStage`). Then what `found` and
/// `new` lead to, in the same sheet. Settings' Sign in draws the same sheet over the wall (`LaterSignIn`).
struct SignInScreen: View {
    @State private var flow: SignInFlow
    /// Not now, or Use my own sync server.
    let notNow: () -> Void
    /// Joined an account that existed, with the provider signed in with (nil: by a code or another device).
    let joined: (_ provider: String?) -> Void
    /// Made a new account: first run goes on to the profile form.
    let created: () -> Void
    /// The line under the heading.
    var lead: String?
    /// From "I already use Momotus": the phone offers Use another device first.
    var returning = false
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

    private typealias W = AccountWords
    private var words: AccountWords { .shared }

    init(server: String, sync: SyncModel, lead: String? = nil, returning: Bool = false, notNow: @escaping () -> Void,
         joined: @escaping (_ provider: String?) -> Void, created: @escaping () -> Void) {
        _flow = State(initialValue: SignInFlow(server: server, sync: sync))
        self.lead = lead
        self.returning = returning
        self.notNow = notNow
        self.joined = joined
        self.created = created
    }

    var body: some View {
        layout
        // The native flow is renewed while this runs: it stops when the screen closes.
        .task(id: flow.attempt) { await flow.open() }
        .onChange(of: flow.stage) { _, stage in
            if stage == .joined { joined(flow.provider) }
            if stage == .done { created() }
        }
        // A join another way (the QR) lands: the held sign-in goes to its account. The sheet's own code (wide forms)
        // is that way too, and the join goes on as the step's joined.
        .onChange(of: sync.joined) { _, now in
            guard now else { return }
            let byCorner = cornerJoined
            Task {
                if byCorner {
                    await flow.joinedByCode()
                    joined(nil)
                } else {
                    await flow.linkHeld()
                }
            }
        }
        // Every later stage is the same sheet over the still wall, its Back returning here (plans/first-screens.md 3.3).
        .navigationDestination(isPresented: $showOther) { otherWays.environment(\.onWall, true) }
        .navigationDestination(isPresented: $showPairing) {
            PairingScreen(core: core, done: { joined(nil) }, autostart: flow.server, lede: words.phoneScanHow).environment(\.onWall, true)
        }
        .navigationDestination(isPresented: $showKit) { SyncJoinScreen(kit: true, server: flow.server).environment(\.onWall, true) }
        .navigationDestination(isPresented: $showMakeKit) { RecoveryKitScreen().environment(\.onWall, true) }
        // Sign in with email as a push, so the system Back and the edge swipe work (accounts-email.md 4.8).
        .navigationDestination(isPresented: Binding(get: { flow.onEmail }, set: { shown in if !shown, flow.onEmail { flow.back() } })) {
            EmailSignIn(flow: flow) { otherAfterPop = true; flow.back() }.environment(\.onWall, true)
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

    private var layout: some View {
        VStack(alignment: .leading, spacing: 0) {
            SheetHead(title: isNew ? words.createAccount : words.signInTitle, line: lead,
                      link: flow.stage == .choose ? words.notNow : nil, onLink: notNow)
            if let trouble = flow.trouble, !flow.onEmail { troubleLine(trouble).padding(.top, Theme.Space.s) }
            Group {
                if metrics.wide {
                    SignInStage { stage } corner: {
                        if cornerShows { PhoneCorner(server: flow.server) }
                    }
                } else {
                    stage
                }
            }
            .padding(.top, 22)
        }
        #if os(macOS)
        // Escape is Not now on the first step, Cancel on the steps after a sign-in.
        .onExitCommand {
            switch flow.stage {
            case .choose: notNow()
            case .new, .cannotOpen, .startOver: flow.back()
            default: break
            }
        }
        #endif
    }

    /// Wide forms show the code beside the steps that offer a join: the first, `new` and `cannotOpen`.
    private var cornerShows: Bool {
        guard metrics.wide, !sync.keyUnreadable else { return false }
        switch flow.stage {
        case .choose, .new, .cannotOpen: return !flow.onEmail
        default: return false
        }
    }

    /// The corner's code joined this device (not the pushed pairing page's).
    private var cornerJoined: Bool {
        guard metrics.wide, !showPairing, case .joined? = sync.pairing?.snapshot else { return false }
        return true
    }

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
        VStack(alignment: .leading, spacing: 12) {
            if flow.unreachable {
                Said(tone: .bad, text: words.unreachable)
                FlowRow(spacing: Theme.Space.xs) {
                    Button(words.unreachableActions[0]) { flow.retry() }
                        .buttonStyle(PillButtonStyle(prominent: true))
                    Button(words.unreachableActions[1], action: notNow)
                        .buttonStyle(PillButtonStyle())
                }
            }
            // From "I already use Momotus", the phone's other-device way comes first (one more press, counted).
            if returning, offersDevice { anotherDevice }
            ForEach(flow.providers, id: \.self) { id in providerButton(id) }
            if flow.offersEmail {
                // Its own row under the provider buttons; the primary when it is the only way listed.
                Button { flow.openEmail() } label: { way(words.continueWithEmail, glyph: "mail") }
                    .buttonStyle(PillButtonStyle(prominent: flow.providers.isEmpty))
            }
            // No provider on this sync service: Create here.
            if flow.providers.isEmpty, !flow.unreachable {
                Button { Task { await flow.createHere(name: DeviceName.current, userName: userName) } } label: {
                    way(words.createAccount, glyph: "plus")
                }
                .buttonStyle(PillButtonStyle(prominent: !flow.offersEmail))
            }
            if !returning, offersDevice { anotherDevice }
            Button(words.otherWays) { showOther = true }
                .buttonStyle(.textLink)
                .textFont(17, weight: .semibold, relativeTo: .body)
                .foregroundStyle(Theme.Palette.key)
                .frame(maxWidth: .infinity, minHeight: FirstRunRules.Size.link)
                .contentShape(Rectangle())
        }
    }

    /// Use another device (the pairing code): on the phone, and on a wide sheet whose QR cannot show.
    private var offersDevice: Bool { !flow.unreachable && !cornerShows }

    private var anotherDevice: some View {
        Button { showPairing = true } label: { way(FirstRunWords.shared.useAnotherDevice, glyph: "qr-code") }
            .buttonStyle(PillButtonStyle())
    }

    /// A way's label: the sheet's width, its icon and words from the leading edge, 52 high (round2.html `.prov .btn`).
    private func way(_ title: String, glyph: String) -> some View {
        Label(title, glyph: glyph)
            .frame(maxWidth: .infinity, minHeight: FirstRunRules.Size.provider - 2 * PillButtonStyle.vertical, alignment: .leading)
    }

    /// Continue with a provider: Google and Facebook in their own buttons, the pill for the rest.
    @ViewBuilder private func providerButton(_ id: String) -> some View {
        let title = AccountRules.fill(words.continueWith, ["provider": AccountRules.providerName(id)])
        if ProviderButtonStyle.draws(id) {
            Button { Task { await press(id) } } label: {
                Text(title).frame(maxWidth: .infinity, minHeight: FirstRunRules.Size.provider - 2 * PillButtonStyle.vertical, alignment: .leading)
            }
            .buttonStyle(ProviderButtonStyle(provider: id))
        } else {
            Button { Task { await press(id) } } label: { way(title, glyph: "door-open") }
                .buttonStyle(PillButtonStyle(prominent: id == flow.providers.first))
        }
    }

    /// Other ways to sign in: a passkey, a scan, a recovery kit, the user's own server.
    private var otherWays: some View {
        SignInPage(title: words.otherWays) {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
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
        }
    }

    /// Found, but no passkey here opens it: the QR on a signed-in device, a
    /// recovery kit, or Start over.
    private func cannotOpen(_ ticket: String?) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            if flow.trouble == nil { Said(tone: .warn, text: words.kitUnknownPasskey) }
            FlowRow(spacing: Theme.Space.xs) {
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
    /// The account is created at once, as any app's provider sign-in does; Create and
    /// "I already use" stay on screen only when that create failed.
    private func newAccount(_ ticket: String) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(words.noAccountYet)
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            FlowRow(spacing: Theme.Space.xs) {
                Button { Task { await flow.create(ticket: ticket, name: DeviceName.current, userName: userName) } } label: {
                    Label(words.createAccount, glyph: "plus")
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                Button { flow.joinAnotherWay(ticket: ticket); showPairing = true } label: { Label(words.alreadyUse, glyph: "qr-code") }
                    .buttonStyle(PillButtonStyle())
            }
            back
        }
        .task(id: ticket) {
            if flow.trouble == nil { await flow.create(ticket: ticket, name: DeviceName.current, userName: userName) }
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
            .frame(minHeight: FirstRunRules.Size.link)
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
    /// From the Login sheet: the code stage is a sheet over the wall too.
    @Environment(\.onWall) private var onWall
    private var words: AccountWords { .shared }

    var body: some View {
        EmailPage(title: words.emailTitle) {
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
            EmailCode(flow: flow, otherWays: otherWays).environment(\.onWall, onWall)
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
        EmailPage(title: words.codeTitle) {
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

/// An email stage's page, as the other sign-in pages (a sheet over the wall from the Login sheet).
private struct EmailPage<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        SignInPage(title: title) { content }
            .scrollDismissesKeyboard(.interactively)
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
/// Signing in later, from Account (4.1.5): the device's library goes into the account. The same Login sheet as first
/// run's, over the still poster wall (D10), asked for as the sheet opens.
struct LaterSignIn: View {
    let server: String
    @Environment(SyncModel.self) private var sync
    @Environment(CoreClient.self) private var core
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ZStack {
            PosterWall(posters: WallOnce.shared.posters, columns: FirstRunRules.columns(FirstRunRules.form(wide: metrics.wide, tv: false)),
                       running: false, dimmed: true)
            WallSheet(sheet: .login) {
                SignInScreen(server: server, sync: sync, lead: AccountWords.shared.libraryAdded, notNow: { dismiss() },
                             joined: { _ in dismiss() }, created: { dismiss() })
            }
        }
        .task { await WallOnce.shared.get { [core] in try await core.call("firstRunWall", as: [String].self) } }
    }
}

/// The Login sheet's QR column on wide forms (round2.html `.qrc`): the live code at 200 pt, "Sign in with your
/// phone", how, and "Waiting for your phone"; asked for as the step opens and renewed by the core while it shows, its
/// own line while the code is asked for or the join runs, and why it stopped with Start again. Leaving the step ends it.
private struct PhoneCorner: View {
    let server: String
    @Environment(SyncModel.self) private var sync
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var words: FirstRunWords { .shared }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            phase
        }
        .accessibilityElement(children: .contain)
        .task { start() }
        // Opens it again after a core restart.
        .task(id: sync.pairing?.taskId) { sync.pairing?.start() }
        .onDisappear { if !joined { sync.endPairing() } }
    }

    @ViewBuilder private var phase: some View {
        if let failure = sync.pairing?.failure {
            Label(PairingWords.problem(failure), glyph: "triangle-alert")
                .textFont(15, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
            Button { sync.endPairing(); start() } label: { Label("Start again", glyph: "refresh-cw") }
                .buttonStyle(PillButtonStyle())
        } else {
            switch sync.pairing?.snapshot {
            case .waiting(let qr, _)?:
                // A new code takes the old one's place, at the same size.
                QRCode(text: qr, pairing: true)
                    .frame(width: 200, height: 200)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
                    .accessibilityLabel(AccountWords.shared.tvQrLabel)
                Text(words.qrTitle)
                    .textFont(17, weight: .bold, relativeTo: .headline)
                    .foregroundStyle(Theme.Palette.text)
                    .padding(.top, 6)
                    .accessibilityAddTraits(.isHeader)
                Text(words.qrHow)
                    .textFont(15, relativeTo: .callout)
                    .foregroundStyle(Color.white.opacity(0.64))
                    .fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 10) {
                    // Still under Reduce Motion (round2.html:486).
                    if reduceMotion { Glyph("clock", size: 16) } else { ProgressView().controlSize(.small).tint(Theme.Palette.text) }
                    Text(words.qrWaiting).textFont(15, relativeTo: .callout)
                }
                .foregroundStyle(Theme.Palette.muted)
            case .joining?, .joined?:
                Label(AccountWords.shared.approveSigningIn, glyph: "clock")
                    .textFont(15, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.text)
            default:
                Label("Asking the sync server for a code", glyph: "clock")
                    .textFont(15, relativeTo: .callout)
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
