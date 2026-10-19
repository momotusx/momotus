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
    @State private var server = SyncJoinScreen.rememberedServer
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
                    .font(.text(Theme.type.body))
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
        let device = name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Marquee" : name.trimmingCharacters(in: .whitespacesAndNewlines)
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
                UserDefaults.standard.set(address, forKey: "syncServer")
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
        let saved = UserDefaults.standard.string(forKey: "syncServer") ?? ""
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
                    .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                    .tracking(1.6)
                    .foregroundStyle(Theme.Palette.muted)
                if secret {
                    Spacer()
                    Button(revealed ? W.hideKit : W.showKit) { revealed.toggle() }
                        .buttonStyle(.textLink)
                        .font(.text(Theme.type.callout, weight: .semibold, relativeTo: .callout))
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
                    .font(.text(Theme.type.callout, relativeTo: .callout))
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

/// Add a device: scan (iPhone and iPad) or paste the code the new device
/// shows, say which sync server it joins through and approve it, then show
/// the six digits the new device must show too. A TV says to use a phone.
struct AddDeviceScreen: View {
    @Environment(SyncModel.self) private var sync
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss
    @State private var code = ""
    @State private var scanning = false
    /// The code waiting for the step that names its sync server.
    @State private var reviewing: String?
    @State private var busy = false
    @State private var error: String?
    @State private var digits: String?
    /// The code the digits belong to: They differ ends its approval.
    @State private var approvedCode: String?
    /// They differ ended the approval: its words replace the digits.
    @State private var ended = false
    @FocusState private var typing: String?

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: W.addTitle)
                #if os(tvOS)
                Elsewhere(glyph: "qr-code", title: W.tvOnly, text: W.tvAdd)
                #else
                if let digits { approved(digits) } else if let reviewing { review(reviewing) } else { entry }
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
        #if os(iOS)
        .fullScreenCover(isPresented: $scanning) {
            QRScanner { found in
                scanning = false
                take(found)
            } cancel: {
                scanning = false
            }
        }
        #endif
    }

    private var entry: some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            Text(W.addLede)
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            Callout(glyph: "shield-alert", text: W.addWarning)
            #if os(iOS)
            Button { error = nil; scanning = true } label: { Label(W.scan, glyph: "camera") }
                .buttonStyle(PillButtonStyle(prominent: true))
            #endif
            SyncField(label: scanLabel, text: $code, prompt: W.pasteHint, typing: $typing, mono: true)
            Button { take(code) } label: { Label(W.approve, glyph: "check") }
                .buttonStyle(PillButtonStyle(prominent: !hasCamera))
                .disabled(code.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            if let error { Said(tone: .bad, text: error) }
        }
    }

    private var hasCamera: Bool {
        #if os(iOS)
        true
        #else
        false
        #endif
    }

    private var scanLabel: String { hasCamera ? W.pasteLabel : W.pasteOnly }

    /// A scanned or pasted code goes to the step that names its sync server first; never straight to approving.
    private func take(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard SyncRules.isPairingCode(trimmed) else {
            error = W.notACode
            return
        }
        typing = nil
        error = nil
        reviewing = trimmed
    }

    private func review(_ qr: String) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            // The code is for the user's own sync server (the core refuses any other): the question is the device.
            Text(W.reviewAsk)
                .display(Theme.type.title3)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
            Text(W.reviewText)
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            if let error { Said(tone: .bad, text: error) }
            FlowRow(spacing: Theme.Space.xs) {
                Button { approve(qr) } label: { Label(busy ? W.approving : W.approve, glyph: "check") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(busy)
                Button(W.cancel) { reviewing = nil; error = nil }
                    .buttonStyle(PillButtonStyle())
                    .disabled(busy)
            }
        }
    }

    private func approve(_ qr: String) {
        guard !busy else { return }
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                digits = try await sync.approvePairing(qr)
                approvedCode = qr
                ended = false
                code = ""
                reviewing = nil
            } catch {
                self.error = SyncProblem(error, action: .approve).words
            }
        }
    }

    private func approved(_ digits: String) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.m) {
            if ended {
                // The old digits and "confirm on the new device" go: they no longer apply.
                Said(tone: .warn, text: W.differEnded)
            } else {
                Text(W.approvedStep)
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.text)
                Text(digits)
                    .font(.mono(Theme.type.hero + 8))
                    .tracking(8)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityLabel("Code \(digits.map(String.init).joined(separator: " "))")
                Text(W.approvedHelp)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let error { Said(tone: .bad, text: error) }
            FlowRow(spacing: Theme.Space.xs) {
                Button { dismiss() } label: { Label(W.done, glyph: "check") }
                    .buttonStyle(PillButtonStyle(prominent: true))
                // The digits differ: whoever confirmed them is not the device in front of the user.
                if !ended {
                    Button { theyDiffer() } label: { Label(W.differ, glyph: "x") }
                        .buttonStyle(PillButtonStyle())
                        .disabled(busy)
                }
            }
        }
    }

    /// Ends the approval whether or not the device joined yet; a cancelled
    /// prompt or a failed call changes nothing, and the button stays.
    private func theyDiffer() {
        guard !busy, let qr = approvedCode else { return }
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                try await sync.cancelPairing(qr)
                ended = true
                announce(W.differEnded)
            } catch {
                self.error = SyncProblem(error, action: .differ).words
            }
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
                .font(.text(Theme.type.body))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            Text(W.kitOnce)
                .font(.text(Theme.type.callout, relativeTo: .callout))
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
                .font(.text(Theme.type.callout, relativeTo: .callout))
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
    private func letters(_ held: HeldKit, visible: Bool) -> some View {
        CaptureProof {
            Text(visible ? held.kit : W.kitHidden)
                .font(visible ? .mono(Theme.type.title3) : .text(Theme.type.body))
                .foregroundStyle(visible ? Theme.Palette.text : Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
                .padding(Theme.Space.s)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(RoundedRectangle(cornerRadius: Theme.Radius.s).fill(Theme.Palette.surface))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(visible ? "\(W.kitShown): \(held.kit.map(String.init).joined(separator: " "))" : W.kitHidden)
    }

    #if os(macOS)
    /// Hold to show, Show for 20 seconds (the OS prompt again: for a keyboard or VoiceOver
    /// user who cannot hold), and the seconds left (not announced: it changes every second).
    private func macControls(left: Int, visible: Bool) -> some View {
        FlowRow(spacing: Theme.Space.xs) {
            Button {} label: { Label(W.holdToShow, glyph: "eye") }
                .buttonStyle(HoldStyle(holding: $holding))
            Button { reveal() } label: { Label(W.showForAWhile, glyph: "clock") }
                .buttonStyle(PillButtonStyle())
                .disabled(gate.asking)
            if visible, !holding, left > 0 {
                Text(W.hiddenIn(left))
                    .font(.text(Theme.type.callout, relativeTo: .callout))
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
                .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                .tracking(1.6)
                .foregroundStyle(Theme.Palette.muted)
            if held.id.isEmpty { Said(tone: .warn, text: W.kitIdUnknown) }
            #if os(macOS)
            // No API keeps a Mac window out of every screen recording: the kit shows for a
            // while after each reveal, or while Show is held, and the page says why.
            TimelineView(.periodic(from: .now, by: 1)) { context in
                let visible = gate.shows(now: context.date, holding: holding)
                VStack(alignment: .leading, spacing: Theme.Space.s) {
                    letters(held, visible: visible)
                    macControls(left: gate.until.map { max(0, Int($0.timeIntervalSince(context.date).rounded(.up))) } ?? 0, visible: visible)
                }
            }
            Text(W.macCapture)
                .font(.text(Theme.type.callout, relativeTo: .callout))
                .foregroundStyle(Theme.Palette.muted)
                .fixedSize(horizontal: false, vertical: true)
            #else
            letters(held, visible: true)
            #endif
            Callout(glyph: "triangle-alert", text: W.kitWarning)
            Text(copied ? W.kitCopied : W.copyKitHelp)
                .font(.text(Theme.type.callout, relativeTo: .callout))
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
            .background(Capsule().fill(configuration.isPressed ? AnyShapeStyle(Theme.Palette.raised) : AnyShapeStyle(Theme.controlFill)))
            .overlay(Capsule().strokeBorder(Theme.Palette.control, lineWidth: 1))
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
                    .font(.text(Theme.type.body))
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
        .confirmationDialog(W.lostButton(drop.count) + "?", isPresented: $confirming, titleVisibility: .visible) {
            Button(W.lostButton(drop.count), role: .destructive) { report() }
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
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                ForEach(rows) { row in tick(row) }
            }
        }
        let kits = SyncRules.kitsGoing(previewKits: preview.kits, members: members, keep: keep)
        if !kits.going.isEmpty { Callout(glyph: "key-round", text: W.lostKits(kits.going.count)) }
        if !kits.unsure.isEmpty { Callout(glyph: "triangle-alert", text: W.lostKitsUnsure(kits.unsure.count)) }
        if let error { Said(tone: .bad, text: error) }
        FlowRow(spacing: Theme.Space.xs) {
            Button {
                #if os(tvOS)
                confirming = true
                #else
                report()
                #endif
            } label: { Label(W.lostButton(drop.count), glyph: "shield-alert") }
                .buttonStyle(DestructiveTextStyle())
                .disabled(busy)
            Button(W.cancel) { dismiss() }.buttonStyle(PillButtonStyle())
        }
    }

    /// One device that goes unless the user unticks it.
    private func tick(_ row: SyncRules.LostRow) -> some View {
        let going = !keep.contains(row.id)
        return Button {
            if going { keep.insert(row.id) } else { keep.remove(row.id) }
        } label: {
            HStack(alignment: .top, spacing: Theme.Space.s) {
                Glyph(going ? "circle-check" : "circle-dashed").foregroundStyle(going ? Theme.Palette.bad : Theme.Palette.muted)
                VStack(alignment: .leading, spacing: 2) {
                    Text(row.name).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                    Text(row.detail)
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(Theme.Palette.muted)
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
                        .font(.text(Theme.type.body))
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
                    .font(.text(Theme.type.body))
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
                .font(.text(Theme.type.callout, relativeTo: .callout))
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
                    .font(.text(Theme.type.callout, relativeTo: .callout))
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
                        Text(name).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                        Text(detail).font(.text(Theme.type.callout, relativeTo: .callout)).foregroundStyle(Theme.Palette.muted)
                        // A ticked device's kits are named on its row: they go with it.
                        if on, kits > 0 { Text(W.goesWith(kits)).font(.text(Theme.type.callout, weight: .medium, relativeTo: .callout)) }
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
                Text(name).font(.text(Theme.type.headline, weight: .semibold, relativeTo: .headline))
                if let tag {
                    Text(tag).font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption)).foregroundStyle(Theme.Palette.muted)
                }
            }
            Text(detail).font(.text(Theme.type.callout, relativeTo: .callout)).foregroundStyle(Theme.Palette.muted)
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
