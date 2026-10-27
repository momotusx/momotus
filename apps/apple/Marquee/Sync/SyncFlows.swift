import SwiftUI
#if os(iOS)
import AVFoundation
import UIKit
import UniformTypeIdentifiers
#elseif os(macOS)
import AppKit
#endif

private typealias W = SyncWords

/// Start an account (the first device) or join one with a recovery kit: both
/// take the server and this device's name. A device that already syncs asks
/// first, since a join replaces its account here.
struct SyncJoinScreen: View {
    let kit: Bool
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss
    @State private var server: String

    /// `server`: the build's hosted relay, filled in from sign-in; else the last one used.
    init(kit: Bool, server: String? = nil) {
        self.kit = kit
        _server = State(initialValue: server ?? SyncJoinScreen.rememberedServer)
    }
    @State private var name = UserDefaults.standard.string(forKey: "syncName") ?? DeviceName.current
    /// The invite code, or the kit: it stays in this page only.
    @State private var extra = ""
    @State private var busy = false
    @State private var error: String?
    @State private var replacing = false
    @FocusState private var typing: String?

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: kit ? W.recoverTitle : W.startTitle)
                Text(kit ? W.recoverLede : W.startText)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                SyncField(label: W.server, text: $server, prompt: W.serverHint, typing: $typing, url: true)
                if kit { SyncField(label: W.kit, text: $extra, prompt: W.kitHint, typing: $typing, mono: true, secret: true) }
                SyncField(label: W.device, text: $name, prompt: DeviceName.current, typing: $typing, help: W.deviceHelp)
                if !kit { SyncField(label: W.invite, text: $extra, prompt: W.inviteHint, typing: $typing) }
                if let error { Said(tone: .bad, text: error) }
                Button { submit(confirmed: false) } label: {
                    Label(busy ? (kit ? W.recovering : W.starting) : (kit ? W.recoverButton : W.startButton), glyph: kit ? "life-buoy" : "check")
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(busy)
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        // The kit leaves with the page: nothing keeps it.
        .onDisappear { extra = "" }
        .confirmationDialog(W.replaceAsk, isPresented: $replacing, titleVisibility: .visible) {
            Button(W.replaceButton, role: .destructive) { submit(confirmed: true) }
            Button(W.cancel, role: .cancel) {}
        } message: {
            Text(W.replaceText)
        }
    }

    private func submit(confirmed: Bool) {
        guard !busy else { return }
        typing = nil
        let address = server.trimmingCharacters(in: .whitespacesAndNewlines)
        let text = extra.trimmingCharacters(in: .whitespacesAndNewlines)
        // An address with a user name or password is never kept or sent.
        guard PairingWords.plainAddress(address) else { error = W.badAddress; return }
        if kit, text.isEmpty { error = W.noKitEntered; return }
        // A live account is replaced only after the user says so.
        if sync.joined, !confirmed { replacing = true; return }
        let device = name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Momotus" : name.trimmingCharacters(in: .whitespacesAndNewlines)
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                if kit {
                    try await sync.recover(server: address, kit: text, name: device)
                } else {
                    try await sync.createAccount(server: address, invite: text, name: device)
                }
                UserDefaults.standard.set(address, forKey: SyncRecord.serverKey)
                UserDefaults.standard.set(device, forKey: "syncName")
                extra = ""
                dismiss()
            } catch {
                self.error = SyncProblem(error, action: kit ? .recover : .start).words
            }
        }
    }

    /// The sync server used before, when it is a plain address.
    private static var rememberedServer: String {
        let saved = UserDefaults.standard.string(forKey: SyncRecord.serverKey) ?? ""
        return PairingWords.plainAddress(saved) ? saved : "https://"
    }
}

/// A labelled text field in the app's field chrome.
struct SyncField: View {
    let label: String
    @Binding var text: String
    let prompt: String
    var typing: FocusState<String?>.Binding
    var url = false
    var mono = false
    /// A recovery kit: hidden as it is typed, unless the user shows it.
    var secret = false
    var help: String?
    @State private var revealed = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(caps: label)
                    .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                    .tracking(1.6)
                    .foregroundStyle(Theme.Palette.muted)
                if secret {
                    Spacer()
                    Button(revealed ? W.hideKit : W.showKit) { revealed.toggle() }
                        .buttonStyle(.textLink)
                        .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.text)
                }
            }
            field
                .hiddenWhenWatched(secret)
                .focused(typing, equals: label)
                .textFieldStyle(.plain)
                .font(mono ? .mono(Theme.type.body) : .text(Theme.type.body))
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                .keyboardType(url ? .URL : .asciiCapable)
                #endif
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .fieldChrome(focused: typing.wrappedValue == label)
            if let help {
                Text(help)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
            }
        }
    }

    @ViewBuilder private var field: some View {
        if secret, !revealed {
            SecureField(prompt, text: $text)
        } else {
            TextField(prompt, text: $text)
        }
    }
}

/// The approve sheet (plans/tv-qr-login.md 5.6): a code a link brought (the
/// `/pair` universal link, `momotus://pair` from the link host's page) or this
/// phone's camera scanner, the only producer of a scan. "Sign in "<name>"?"
/// with Sign in; the core says whether that screen shares this phone's
/// network. Not near, a link asks for a scan here and a scan for the screen
/// lock (the model asks it), before anything is sent. Then Signing in, and
/// Done once the device list shows it, with Not you? Remove it. A TV says to
/// use a phone.
struct AddDeviceScreen: View {
    @Environment(SyncModel.self) private var sync
    @Environment(ProfileSession.self) private var profiles
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss
    @State private var step = ApproveStep.idle
    @State private var scanning = false
    /// The code on the sheet, and whether this phone's camera read it.
    @State private var code: (text: String, scanned: Bool)?
    @State private var device = AccountWords.shared.approveUnnamed
    @State private var member: String?
    @State private var busy = false
    @State private var waiting: Task<Void, Never>?
    /// Bumped by every new code and by leaving: an answer that comes back after
    /// the sheet moved on writes nothing (an approval sent cannot be called back).
    @State private var generation = 0

    private typealias A = AccountWords

    private var kids: Bool { profiles.snapshot?.current?.kids == true }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: A.shared.signInAnother)
                #if os(tvOS)
                Elsewhere(glyph: "qr-code", title: W.tvOnly, text: W.tvAdd)
                #else
                if kids {
                    Text(A.shared.approveKids)
                        .textFont(Theme.type.body)
                        .foregroundStyle(Theme.Palette.muted)
                        .fixedSize(horizontal: false, vertical: true)
                } else {
                    steps
                }
                #endif
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task {
            // A link's code, taken once; none opens the scanner (one tap from the shortcut).
            if kids {
                AppRouter.shared.pendingCode = nil
            } else if let linked = AppRouter.shared.pendingCode {
                AppRouter.shared.pendingCode = nil
                offer(linked, scanned: false)
            } else {
                scanning = true
            }
        }
        .onDisappear {
            generation += 1
            waiting?.cancel()
        }
        #if os(iOS)
        .fullScreenCover(isPresented: $scanning) {
            QRScanner { found in
                scanning = false
                let text = found.trimmingCharacters(in: .whitespacesAndNewlines)
                // After "scan it here" the user already said Sign in to this code: it goes on at once.
                // Another code is a new sheet, so the name answered is always the scanned screen's.
                if step == .scanHere, text == code?.text { signIn(text, scanned: true) } else { offer(text, scanned: true) }
            } cancel: {
                scanning = false
            }
        }
        #endif
    }

    @ViewBuilder private var steps: some View {
        switch step {
        case .idle:
            Text(W.addLede)
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            scanButton
        case .sheet(let ask, let form):
            HStack(spacing: Theme.Space.s) {
                Glyph(form == "tv" ? "tv" : "monitor-smartphone")
                    .foregroundStyle(Theme.Palette.text)
                Text(ask)
                    .display(Theme.type.title3)
                    .foregroundStyle(Theme.Palette.text)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .accessibilityElement(children: .combine)
            // The one defence on the near path: a full line, at any text size (r2 native P2-1).
            Text(A.shared.approveNear)
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
            FlowRow(spacing: Theme.Space.xs) {
                Button { if let code { signIn(code.text, scanned: code.scanned) } } label: { Label(A.shared.approveButton, glyph: "check") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(busy)
                Button(W.cancel) { dismiss() }
                    .buttonStyle(PillButtonStyle())
            }
        case .scanHere:
            Said(tone: .warn, text: A.shared.approveScanHere)
            scanButton
        case .signingIn:
            Said(tone: .warn, text: A.shared.approveSigningIn)
        case .done:
            Said(tone: .ok, text: AccountRules.fill(A.shared.approveDone, ["device": device]))
            FlowRow(spacing: Theme.Space.xs) {
                Button { dismiss() } label: { Label(W.done, glyph: "check") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                Button(A.shared.approveNotYou) { notYou() }
                    .buttonStyle(PillButtonStyle())
                    .disabled(busy)
            }
        case .slow:
            Said(tone: .warn, text: AccountRules.fill(A.shared.approveSlow, ["device": device]))
            Button(A.shared.announceOk) { dismiss() }
                .buttonStyle(PillButtonStyle(prominent: true))
        case .failed(let words):
            Said(tone: .bad, text: words)
            FlowRow(spacing: Theme.Space.xs) {
                scanButton
                Button(W.done) { dismiss() }
                    .buttonStyle(PillButtonStyle())
            }
        }
    }

    @ViewBuilder private var scanButton: some View {
        #if os(iOS)
        Button { scanning = true } label: { Label(W.scan, glyph: "camera") }
            .buttonStyle(PillButtonStyle(prominent: true))
        #endif
    }

    /// A code arrived: it replaces any other, and its wait.
    private func offer(_ text: String, scanned: Bool) {
        generation += 1
        let mine = generation
        waiting?.cancel()
        member = nil
        busy = false
        code = (text, scanned)
        Task {
            let next: ApproveStep
            do {
                let read = try await sync.readPairingCode(text)
                guard mine == generation else { return }
                device = read.name ?? A.shared.approveUnnamed
                next = .sheet(ask: ApproveStep.ask(read), form: read.form)
            } catch {
                next = .failed(W.notACode)
            }
            if mine == generation { step = next }
        }
    }

    private func signIn(_ text: String, scanned: Bool) {
        guard !busy else { return }
        busy = true
        code = (text, scanned)
        let mine = generation
        Task {
            defer { if mine == generation { busy = false } }
            do {
                let joined = try await sync.approvePairing(text, scanned: scanned)
                guard mine == generation else { return }
                member = joined
                step = .signingIn
                announce(A.shared.approveSigningIn)
                wait(for: joined, mine: mine)
            } catch let error as CoreError where error.code == "scanRequired" {
                guard mine == generation else { return }
                step = .scanHere
                scanning = true
            } catch {
                if mine == generation { step = .failed(ApproveStep.words(error)) }
            }
        }
    }

    /// One poll of the device list, every 2 s up to 60 s, until the new device is in it.
    private func wait(for joined: String, mine: Int) {
        waiting?.cancel()
        waiting = Task {
            for _ in 0..<30 {
                try? await Task.sleep(for: .seconds(2))
                if Task.isCancelled || mine != generation { return }
                if let list = try? await sync.loadDevices(), list.members.contains(where: { $0.id == joined }) {
                    guard mine == generation else { return }
                    step = .done
                    announce(AccountRules.fill(A.shared.approveDone, ["device": device]))
                    return
                }
            }
            if mine == generation { step = .slow }
        }
    }

    /// Not you? Remove it: the device just signed in leaves the account.
    private func notYou() {
        guard let member, !busy else { return }
        busy = true
        Task {
            defer { busy = false }
            do {
                try await sync.removeDevice(member)
                dismiss()
            } catch {
                step = .failed(SyncProblem(error, action: .remove).words)
            }
        }
    }
}

/// The approve sheet's steps, and its words for a code and a refusal.
enum ApproveStep: Equatable {
    case idle
    case sheet(ask: String, form: String?)
    case scanHere
    case signingIn
    case done
    case slow
    case failed(String)

    /// "Sign in "<name>"?", else by the screen's form.
    static func ask(_ code: PairingCode) -> String {
        if let name = code.name { return AccountRules.fill(AccountWords.shared.approveAsk, ["device": name]) }
        return code.form == "tv" ? AccountWords.shared.approveAskTv : AccountWords.shared.approveAskOther
    }

    static func words(_ error: any Error) -> String {
        switch (error as? CoreError)?.code {
        case "codeAnswered": AccountWords.shared.approveAnswered
        case "codeExpired": AccountWords.shared.approveExpired
        default: SyncProblem(error, action: .approve).words
        }
    }
}

/// Make a recovery kit, behind the OS prompt. The model holds its letters
/// in memory until "I saved it" (five minutes at most once this page is
/// left); they show right after the kit is made, and again only after the
/// OS prompt: leaving the app covers them. On screen they are left out of
/// screenshots, recordings and mirroring. Copied only to this device's
/// clipboard, for a minute. Not on a TV.
struct RecoveryKitScreen: View {
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    @Environment(\.tabShown) private var tabShown
    /// Whether the held kit may show (`KitGate`).
    @State private var gate = KitGate()
    /// Mac: Show is held down.
    @State private var holding = false
    @State private var error: String?
    @State private var copied = false

    private var shown: HeldKit? { gate.unlocked ? sync.kits.held : nil }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: W.kitTitle)
                #if os(tvOS)
                Elsewhere(glyph: "key-round", title: W.tvOnly, text: W.tvKit)
                #else
                if let shown {
                    kit(shown)
                } else if sync.kits.held != nil {
                    waiting
                } else {
                    ask
                }
                #endif
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .onAppear {
            sync.kitPage(open: true)
            #if DEBUG
            // Only the made-up preview kit ever shows without the prompt.
            if DebugHooks.kitPreview, sync.kits.held == DebugHooks.previewKit { gate.previewUnlock() }
            #endif
        }
        .onDisappear {
            gate.cover()
            sync.kitPage(open: false)
        }
        // Another tab over this one is leaving the page: the hold runs, and coming back asks again.
        .onChange(of: tabShown) { _, shown in
            if !shown { gate.cover() }
            sync.kitPage(open: shown)
        }
        // Back to the app (from the switcher, another app or the lock screen): the OS prompt again.
        .onChange(of: phase) { _, now in gate.phase(now) }
        #if os(iOS)
        // The scene phase can come late or not at all for a quick visit away.
        .onReceive(NotificationCenter.default.publisher(for: UIScene.willDeactivateNotification)) { _ in gate.deactivating() }
        #endif
        // Leaving keeps the kit (the model holds it), but while it shows the way out is I saved it.
        #if !os(macOS)
        .navigationBarBackButtonHidden(shown != nil)
        #endif
    }

    private var ask: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Text(W.kitLede)
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            Text(W.kitOnce)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
            if !sync.kits.unseen.isEmpty { Callout(glyph: "triangle-alert", text: sync.kits.expired ? W.kitExpired : W.kitUnseen) }
            if sync.kits.unknownUnseen { Callout(glyph: "triangle-alert", text: W.kitUnseenUnknown) }
            if let error { Said(tone: .bad, text: error) }
            FlowRow(spacing: Theme.Space.xs) {
                Button { make() } label: {
                    Label(sync.makingKit ? W.makingKit : sync.kits.unseen.isEmpty ? W.makeKit : W.makeNewKit, glyph: "key-round")
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(sync.makingKit)
                // Leaving while it is made keeps the kit: the hub then says it waits.
                Button(W.cancel) { dismiss() }.buttonStyle(PillButtonStyle())
            }
        }
    }

    /// A kit held and not on screen: shown again only after the OS prompt.
    private var waiting: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Callout(glyph: "key-round", text: W.kitWaiting)
            Text(W.kitAskShow)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            if let error { Said(tone: .bad, text: error) }
            FlowRow(spacing: Theme.Space.xs) {
                Button { reveal() } label: { Label(W.showKitAgain, glyph: "key-round") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(gate.asking)
                Button(W.cancel) { dismiss() }.buttonStyle(PillButtonStyle())
            }
        }
    }

    /// The kit shows once made only if the page stayed in view meanwhile; else it waits, covered.
    private func make() {
        guard !sync.makingKit, !gate.asking else { return }
        error = nil
        copied = false
        gate.ask()
        Task {
            do {
                try await sync.createRecoveryKit()
                gate.answered(ok: sync.kits.held != nil)
            } catch {
                gate.answered(ok: false)
                self.error = SyncProblem(error, action: .kit).words
            }
        }
    }

    private func reveal() {
        guard !gate.asking else { return }
        error = nil
        gate.ask()
        Task {
            do {
                try await sync.confirmShowKit()
                gate.answered(ok: true)
            } catch {
                gate.answered(ok: false)
                self.error = SyncProblem(error, action: .kit).words
            }
        }
    }

    /// The letters, out of captures where the system allows it; never selectable: the system's
    /// Copy would put them on every device of the Apple account with no expiry.
    private func letters(_ held: HeldKit, visible: Bool, hidden: String = W.kitHidden) -> some View {
        CaptureProof {
            Text(visible ? held.kit : hidden)
                .font(visible ? .mono(Theme.type.title3) : .text(Theme.type.body))
                .foregroundStyle(visible ? Theme.Palette.text : Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
                .padding(Theme.Space.s)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(RoundedRectangle(cornerRadius: Theme.Radius.s).fill(Theme.Palette.surface))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(visible ? "\(W.kitShown): \(held.kit.map(String.init).joined(separator: " "))" : hidden)
    }

    #if os(macOS)
    /// Hold to show, Show for 20 seconds (the OS prompt again: for a keyboard or VoiceOver
    /// user who cannot hold), and the seconds left (not announced: it changes every second).
    private func macControls(left: Int, visible: Bool, canHold: Bool) -> some View {
        FlowRow(spacing: Theme.Space.xs) {
            Button {} label: { Label(W.holdToShow, glyph: "eye") }
                .buttonStyle(HoldStyle(holding: $holding))
                .disabled(!canHold)
            Button { reveal() } label: { Label(W.showForAWhile, glyph: "clock") }
                .buttonStyle(PillButtonStyle())
                .disabled(gate.asking)
            if visible, !holding, left > 0 {
                Text(W.hiddenIn(left))
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .monospacedDigit()
                    .accessibilityHidden(true)
            }
        }
    }
    #endif

    private func kit(_ held: HeldKit) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Text(caps: W.kitShown)
                .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            if held.id.isEmpty { Said(tone: .warn, text: W.kitIdUnknown) }
            #if os(macOS)
            // No API keeps a Mac window out of every screen recording: the kit shows for a
            // while after each reveal, or while Hold to show is held within a minute of it,
            // and the page says why.
            TimelineView(.periodic(from: .now, by: 1)) { context in
                let visible = gate.shows(now: context.date, holding: holding)
                let canHold = gate.canHold(now: context.date)
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    letters(held, visible: visible, hidden: canHold ? W.kitHidden : W.kitHiddenLocked)
                    macControls(left: gate.until.map { max(0, Int($0.timeIntervalSince(context.date).rounded(.up))) } ?? 0, visible: visible, canHold: canHold)
                }
            }
            Text(W.macCapture)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            #else
            letters(held, visible: true)
            #endif
            Callout(glyph: "triangle-alert", text: W.kitWarning)
            Text(copied ? W.kitCopied : W.copyKitHelp)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            FlowRow(spacing: Theme.Space.xs) {
                Button { copied = Clipboard.copySecret(held.kit) } label: { Label(copied ? W.copied : W.copyKit, glyph: copied ? "check" : "copy") }
                    .buttonStyle(PillButtonStyle())
                Button {
                    sync.kitSaved()
                    dismiss()
                } label: { Label(W.savedKit, glyph: "check") }
                    .buttonStyle(PillButtonStyle(prominent: true))
            }
        }
    }
}

/// Content left out of screenshots, screen recordings and mirroring.
///
/// iOS: drawn inside the layer a secure text field draws its text in, which
/// the system blanks in every capture. If this iOS has no such layer the
/// content does not show at all (the kit's Copy still works).
/// macOS: while it is on screen its window is left out of screen capture and
/// recording (`sharingType` none).
struct CaptureProof<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        #if os(iOS)
        SecureLayer(content: content)
        #elseif os(macOS)
        content.background(UnsharedWindow())
        #else
        content
        #endif
    }
}

#if os(iOS)
private struct SecureLayer<Content: View>: UIViewRepresentable {
    let content: Content

    final class Coordinator {
        var host: UIHostingController<Content>?
        var leaving: (any NSObjectProtocol)?

        deinit {
            if let leaving { NotificationCenter.default.removeObserver(leaving) }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> UIView {
        let field = UITextField()
        field.isSecureTextEntry = true
        // The field's own canvas: the layer the system leaves out of captures.
        guard let canvas = field.subviews.first else {
            coreLog.error("kit: no secure layer on this system; the kit stays hidden")
            return UIView()
        }
        canvas.removeFromSuperview()
        canvas.subviews.forEach { $0.removeFromSuperview() }
        canvas.isUserInteractionEnabled = false
        let host = UIHostingController(rootView: content)
        host.view.backgroundColor = .clear
        host.view.translatesAutoresizingMaskIntoConstraints = false
        canvas.addSubview(host.view)
        NSLayoutConstraint.activate([
            host.view.leadingAnchor.constraint(equalTo: canvas.leadingAnchor),
            host.view.trailingAnchor.constraint(equalTo: canvas.trailingAnchor),
            host.view.topAnchor.constraint(equalTo: canvas.topAnchor),
            host.view.bottomAnchor.constraint(equalTo: canvas.bottomAnchor),
        ])
        context.coordinator.host = host
        // Leaving the front hides it at once, in the same frame: the app
        // switcher's picture never waits for the page to change.
        context.coordinator.leaving = NotificationCenter.default.addObserver(forName: UIScene.willDeactivateNotification, object: nil, queue: .main) { [weak canvas] _ in
            MainActor.assumeIsolated { canvas?.isHidden = true }
        }
        return canvas
    }

    func updateUIView(_ view: UIView, context: Context) {
        context.coordinator.host?.rootView = content
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: UIView, context: Context) -> CGSize? {
        guard let host = context.coordinator.host else { return .zero }
        let width = proposal.width ?? UIView.layoutFittingExpandedSize.width
        return CGSize(width: width, height: host.sizeThatFits(in: CGSize(width: width, height: .greatestFiniteMagnitude)).height)
    }
}
#endif

#if os(macOS)
/// A pill that reports while it is pressed: the Mac shows the kit only while Show is held.
private struct HoldStyle: ButtonStyle {
    @Binding var holding: Bool

    func makeBody(configuration: Configuration) -> some View {
        // A pill like the others, darker while held.
        configuration.label
            .labelFont(Theme.type.headline)
            .labelStyle(PillLabelStyle())
            .foregroundStyle(Theme.Palette.text)
            .padding(.horizontal, PillButtonStyle.horizontal)
            .padding(.vertical, PillButtonStyle.vertical)
            .background { if configuration.isPressed { Capsule().fill(Theme.Palette.raised) } else { SecondaryFill(shape: Capsule()) } }
            .contentShape(Capsule())
            .onChange(of: configuration.isPressed) { _, pressed in holding = pressed }
    }
}

/// Leaves its window out of screen capture and recording while it is in it
/// (best effort: Apple calls `sharingType` legacy, and a ScreenCaptureKit
/// recording can still capture the window).
private struct UnsharedWindow: NSViewRepresentable {
    final class Probe: NSView {
        private weak var held: NSWindow?

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            held?.sharingType = .readOnly
            held = window
            window?.sharingType = .none
        }
    }

    func makeNSView(context: Context) -> Probe { Probe() }

    func updateNSView(_ view: Probe, context: Context) {}
}
#endif

extension View {
    /// A kit typed in and shown: blurred while the app is not in front (its
    /// picture in the app switcher; on a Mac, a hidden window) and while the
    /// screen is recorded or mirrored.
    func hiddenWhenWatched(_ secret: Bool = true) -> some View {
        modifier(HiddenWhenWatched(secret: secret))
    }
}

private struct HiddenWhenWatched: ViewModifier {
    let secret: Bool
    @Environment(\.scenePhase) private var phase
    #if os(iOS)
    @State private var captured = UIScreen.main.isCaptured
    #else
    private let captured = false
    #endif

    func body(content: Content) -> some View {
        content
            .blur(radius: secret && (KitRules.covers(phase) || captured) ? 16 : 0)
            #if os(iOS)
            .onReceive(NotificationCenter.default.publisher(for: UIScreen.capturedDidChangeNotification)) { _ in
                captured = UIScreen.main.isCaptured
            }
            #endif
    }
}

/// Report a device lost: it goes, with the devices it added (listed from a
/// fresh device list, ticked, each one untickable) and always the kits they
/// made. Behind the OS prompt; a TV asks a second time instead.
struct ReportLostScreen: View {
    let lost: MemberInfo
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss
    @State private var loaded: (members: [MemberInfo], preview: LostPreview)?
    @State private var loadError: String?
    @State private var keep: Set<String> = []
    @State private var busy = false
    @State private var error: String?
    @State private var confirming = false

    private var name: String { SyncRules.memberName(lost) }

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: "\(W.reportLost): \(name)")
                Text(W.named(W.lostLede, name))
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                if let loadError {
                    Said(tone: .bad, text: loadError)
                    FlowRow(spacing: Theme.Space.xs) {
                        Button { Task { await load() } } label: { Label(W.retry, glyph: "refresh-cw") }.buttonStyle(PillButtonStyle(prominent: true))
                        Button(W.cancel) { dismiss() }.buttonStyle(PillButtonStyle())
                    }
                } else if let loaded {
                    content(loaded.members, loaded.preview)
                } else {
                    Said(tone: .warn, text: W.named(W.lostChecking, name))
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task { await load() }
        .confirmationDialog(AccountRules.reportLostButton(drop.count) + "?", isPresented: $confirming, titleVisibility: .visible) {
            Button(AccountRules.reportLostButton(drop.count), role: .destructive) { report() }
            Button(W.cancel, role: .cancel) {}
        } message: {
            Text(W.named(W.lostLede, name))
        }
    }

    /// The names come from a device list fetched now, never a cached one:
    /// a device added since the page opened must not go unseen.
    private func load() async {
        loadError = nil
        do {
            let list = try await sync.loadDevices()
            let preview = try await sync.lostPreview(lost.id)
            loaded = (list.members, preview)
        } catch {
            loadError = SyncProblem(error, action: .lost).words
        }
    }

    private var drop: [String] { SyncRules.lostDrop(lost: lost.id, added: loaded?.preview.devices ?? [], keep: keep) }

    @ViewBuilder private func content(_ members: [MemberInfo], _ preview: LostPreview) -> some View {
        let rows = SyncRules.lostRows(preview: preview, members: members)
        if !rows.isEmpty {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                Text(W.named(W.lostAdded, name)).display(Theme.type.title3).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
                Text(W.named(W.lostAddedHelp, name))
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                ForEach(rows) { row in tick(row, joined: joinedWithPasskey(row.id, preview, members)) }
            }
        }
        let kits = SyncRules.lostKitsGoing(preview, keep: keep)
        if !kits.isEmpty { Callout(glyph: "key-round", text: W.lostKits(kits.count)) }
        if let error { Said(tone: .bad, text: error) }
        FlowRow(spacing: Theme.Space.xs) {
            Button {
                #if os(tvOS)
                confirming = true
                #else
                report()
                #endif
            } label: { Label(AccountRules.reportLostButton(drop.count), glyph: "shield-alert") }
                .buttonStyle(DestructiveTextStyle())
                .disabled(busy)
            Button(W.cancel) { dismiss() }.buttonStyle(PillButtonStyle())
        }
    }

    /// "Joined with a passkey on <date>" for a device that joined through one:
    /// a stolen phone may hold any synced passkey (plans/accounts.md 3.2).
    private func joinedWithPasskey(_ id: String, _ preview: LostPreview, _ members: [MemberInfo]) -> String? {
        guard preview.passkeyJoined.contains(id), let member = members.first(where: { $0.id == id }) else { return nil }
        return AccountRules.fill(AccountWords.shared.joinedWithPasskey, ["date": AccountRules.date(member.addedMs)])
    }

    /// One device that goes unless the user unticks it.
    private func tick(_ row: SyncRules.LostRow, joined: String? = nil) -> some View {
        let going = !keep.contains(row.id)
        return Button {
            if going { keep.insert(row.id) } else { keep.remove(row.id) }
        } label: {
            HStack(alignment: .top, spacing: Theme.Space.s) {
                Glyph(going ? "circle-check" : "circle-dashed").foregroundStyle(going ? Theme.Palette.bad : Theme.Palette.muted)
                VStack(alignment: .leading, spacing: 2) {
                    Text(row.name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    Text(row.detail)
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.muted)
                    if let joined {
                        Text(joined)
                            .textFont(Theme.type.callout, relativeTo: .callout)
                            .foregroundStyle(Theme.Palette.muted)
                    }
                }
                Spacer(minLength: 0)
            }
            .foregroundStyle(Theme.Palette.text)
            .padding(Theme.Space.s)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        }
        .buttonStyle(.row)
        .accessibilityLabel(SyncRules.spokenRow(row.name, row.detail))
        .accessibilityValue(going ? "Removed with it" : "Kept")
        .accessibilityAddTraits(going ? .isSelected : [])
    }

    private func report() {
        guard !busy else { return }
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                try await sync.reportLost(lost.id, keep: Array(keep))
                dismiss()
            } catch {
                self.error = SyncProblem(error, action: .lost).words
            }
        }
    }
}

#if os(iOS)
/// The camera, reading QR codes until one is a pairing code. Cancel or
/// leaving stops it; a code read after that is dropped.
struct QRScanner: View {
    let found: (String) -> Void
    let cancel: () -> Void
    @State private var failed = false

    var body: some View {
        ZStack(alignment: .bottom) {
            Color.black.ignoresSafeArea()
            if failed {
                VStack(spacing: Theme.Space.m) {
                    Said(tone: .bad, text: W.noCamera)
                    Button(W.cancel, action: cancel).buttonStyle(PillButtonStyle(prominent: true))
                }
                .padding(Theme.Space.l)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                CameraView(found: found, failed: { failed = true }).ignoresSafeArea()
                VStack(spacing: Theme.Space.s) {
                    Text(W.scanning)
                        .textFont(Theme.type.body)
                        .foregroundStyle(.white)
                        .multilineTextAlignment(.center)
                    Button(W.cancel, action: cancel).buttonStyle(PillButtonStyle())
                }
                .padding(Theme.Space.l)
                .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
                .padding(Theme.Space.m)
            }
        }
    }
}

private struct CameraView: UIViewControllerRepresentable {
    let found: (String) -> Void
    let failed: () -> Void

    func makeUIViewController(context: Context) -> CameraController {
        CameraController(found: found, failed: failed)
    }

    func updateUIViewController(_ controller: CameraController, context: Context) {}

    static func dismantleUIViewController(_ controller: CameraController, coordinator: ()) {
        controller.stop()
    }
}

/// The capture session, handed to a background queue to start and stop
/// (both block); the session does its own locking.
private final class Capture: @unchecked Sendable {
    let session = AVCaptureSession()
    func start() { DispatchQueue.global(qos: .userInitiated).async { self.session.startRunning() } }
    func stop() { DispatchQueue.global(qos: .userInitiated).async { self.session.stopRunning() } }
}

final class CameraController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    private let capture = Capture()
    private var session: AVCaptureSession { capture.session }
    private let found: (String) -> Void
    private let failed: () -> Void
    /// Set by Cancel, leaving or the first code: nothing is handed on after it.
    private var stopped = false

    init(found: @escaping (String) -> Void, failed: @escaping () -> Void) {
        self.found = found
        self.failed = failed
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { fatalError("not from a storyboard") }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        AVCaptureDevice.requestAccess(for: .video) { granted in
            Task { @MainActor [weak self] in
                if granted { self?.configure() } else { self?.failed() }
            }
        }
    }

    private func configure() {
        guard !stopped, let camera = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back),
              let input = try? AVCaptureDeviceInput(device: camera), session.canAddInput(input) else {
            failed()
            return
        }
        session.addInput(input)
        let output = AVCaptureMetadataOutput()
        guard session.canAddOutput(output) else { failed(); return }
        session.addOutput(output)
        output.setMetadataObjectsDelegate(self, queue: .main)
        output.metadataObjectTypes = [.qr]
        let preview = AVCaptureVideoPreviewLayer(session: session)
        preview.videoGravity = .resizeAspectFill
        preview.frame = view.bounds
        view.layer.addSublayer(preview)
        capture.start()
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        view.layer.sublayers?.forEach { $0.frame = view.bounds }
    }

    nonisolated func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection) {
        let codes = objects.compactMap { ($0 as? AVMetadataMachineReadableCodeObject)?.stringValue }
        // The delegate queue is the main queue.
        MainActor.assumeIsolated {
            // A read that lands after Cancel or leaving is dropped.
            guard !stopped, let code = codes.first(where: SyncRules.isPairingCode) else { return }
            stop()
            found(code.trimmingCharacters(in: .whitespacesAndNewlines))
        }
    }

    func stop() {
        stopped = true
        capture.stop()
    }
}
#endif

/// After the sync server showed a different device list: both lists from
/// `forkPreview`, the user's ticks, then the server's list as the page showed
/// it (its `head`), never before the user saw it. A list that moved meanwhile
/// is shown again with nothing ticked. A device not on the server's list, or
/// one the server no longer knows, cannot take it: it can only stop here.
struct ForkReviewScreen: View {
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss
    @State private var preview: ForkPreview?
    @State private var loadError: SyncProblem?
    @State private var ticked: Set<String> = []
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: W.forkTitle)
                Text(W.forkLede)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                if let loadError {
                    Said(tone: .bad, text: loadError.words)
                    // A server that no longer knows this device's token: stopping here is the way out.
                    if loadError.code == nil, loadError.message == "http status 401" { LeaveHere() }
                    FlowRow(spacing: Theme.Space.xs) {
                        Button { Task { await load() } } label: { Label(W.retry, glyph: "refresh-cw") }.buttonStyle(PillButtonStyle(prominent: true))
                        Button(W.cancel) { dismiss() }.buttonStyle(PillButtonStyle())
                    }
                } else if let preview {
                    if preview.onServer { compare(preview) } else { offList }
                } else {
                    Said(tone: .warn, text: W.forkLoading)
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        .task { await load() }
    }

    private func load() async {
        loadError = nil
        #if DEBUG
        if let fixture = DebugHooks.forkPreview {
            preview = fixture
            return
        }
        #endif
        do {
            preview = try await sync.forkPreview()
        } catch {
            loadError = SyncProblem(error, action: .compare)
        }
    }

    private var offList: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Said(tone: .bad, text: W.forkOffList)
            Button(W.cancel) { dismiss() }.buttonStyle(PillButtonStyle())
            LeaveHere()
        }
    }

    @ViewBuilder private func compare(_ preview: ForkPreview) -> some View {
        let sides = SyncRules.forkSides(mine: preview.mine, server: preview.server)
        let all = preview.server + preview.mine
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(W.forkServer).display(Theme.type.title3).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
            Text(W.forkServerHelp)
                .textFont(Theme.type.callout, relativeTo: .callout)
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            ForEach(SyncRules.sortMembers(preview.server)) { member in
                serverRow(member, preview: preview, all: all, onlyThere: sides.serverOnly.contains(member))
            }
        }
        if sides.mineOnly.isEmpty {
            if sides.serverOnly.isEmpty { Said(tone: .idle, text: W.forkSame) }
        } else {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                Text(W.forkMine).display(Theme.type.title3).foregroundStyle(Theme.Palette.text).accessibilityAddTraits(.isHeader)
                Text(W.forkMineHelp)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                ForEach(SyncRules.sortMembers(sides.mineOnly)) { member in
                    plainRow(name: SyncRules.memberName(member),
                             detail: W.addedLine(kit: member.kit, addedMs: member.addedMs, by: SyncRules.sponsorName(member, in: all)), tag: nil)
                }
            }
        }
        if let error { Said(tone: .bad, text: error) }
        let counts = SyncRules.forkCounts(preview, ticked: ticked)
        FlowRow(spacing: Theme.Space.xs) {
            Button { go(preview) } label: {
                Label(busy ? W.forkGoing : W.forkButton(devices: counts.devices, kits: counts.kits), glyph: "monitor-smartphone")
            }
            .buttonStyle(PillButtonStyle(prominent: counts.devices + counts.kits == 0))
            .disabled(busy)
            Button(W.cancel) { dismiss() }.buttonStyle(PillButtonStyle())
        }
    }

    @ViewBuilder private func serverRow(_ member: MemberInfo, preview: ForkPreview, all: [MemberInfo], onlyThere: Bool) -> some View {
        let name = SyncRules.memberName(member) + (onlyThere ? " (\(W.onlyOnServer.lowercased()))" : "")
        let detail = W.addedLine(kit: member.kit, addedMs: member.addedMs, by: SyncRules.sponsorName(member, in: all))
        switch SyncRules.forkRow(member, in: preview, ticked: ticked) {
        case .thisDevice: plainRow(name: name, detail: detail, tag: W.thisDevice)
        case .goes: plainRow(name: name, detail: detail, tag: W.forkGoes)
        case .withTicked(let maker): plainRow(name: name, detail: detail, tag: W.named(W.goesWithTicked, maker))
        case .tickable:
            let on = ticked.contains(member.id)
            let kits = SyncRules.forkKits(of: member.id, in: preview).count
            Button {
                if on { ticked.remove(member.id) } else { ticked.insert(member.id) }
            } label: {
                HStack(alignment: .top, spacing: Theme.Space.s) {
                    Glyph(on ? "circle-check" : "circle-dashed").foregroundStyle(on ? Theme.Palette.bad : Theme.Palette.muted)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                        Text(detail).textFont(Theme.type.callout, relativeTo: .callout).foregroundStyle(Theme.Palette.muted)
                        // A ticked device's kits are named on its row: they go with it.
                        if on, kits > 0 { Text(W.goesWith(kits)).textFont(Theme.type.callout, weight: .medium, relativeTo: .callout) }
                    }
                    Spacer(minLength: 0)
                }
                .foregroundStyle(Theme.Palette.text)
                .padding(Theme.Space.s)
                .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
            }
            .buttonStyle(.row)
            .accessibilityLabel(SyncRules.spokenRow(name, detail))
            .accessibilityValue(on ? (kits > 0 ? "Removed when you continue. \(W.goesWith(kits))" : "Removed when you continue") : "Kept")
            .accessibilityAddTraits(on ? .isSelected : [])
        }
    }

    private func plainRow(name: String, detail: String, tag: String?) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: Theme.Space.xs) {
                Text(name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                if let tag {
                    Text(tag).textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption).foregroundStyle(Theme.Palette.muted)
                }
            }
            Text(detail).textFont(Theme.type.callout, relativeTo: .callout).foregroundStyle(Theme.Palette.muted)
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .accessibilityElement(children: .combine)
    }

    /// Only the list the user saw: one that moved since is refused, then shown again with nothing ticked.
    private func go(_ shown: ForkPreview) {
        guard !busy else { return }
        busy = true
        error = nil
        let drop = SyncRules.forkDrop(server: shown.server, drops: shown.drops, ticked: ticked)
        Task {
            defer { busy = false }
            do {
                try await sync.useServerRoster(drop: drop, head: shown.head)
                announce(W.forkDone)
                dismiss()
            } catch {
                let problem = SyncProblem(error, action: .serverList)
                self.error = problem.words
                if problem.code == "rosterChanged" {
                    ticked = []
                    await load()
                }
            }
        }
    }
}
