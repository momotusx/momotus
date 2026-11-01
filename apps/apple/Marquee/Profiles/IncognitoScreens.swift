import SwiftUI

/// Incognito's intro, on every entry (plans/batch-10-incognito.md 5.1): full screen on the page colour, the mark, what
/// is not kept and what can still be seen, then Continue (TV focus) and Leave Incognito. Back on TV leaves.
struct IncognitoIntro: View {
    @Environment(ProfileSession.self) private var profiles
    @Environment(DebridSession.self) private var debrid
    @Environment(\.metrics) private var metrics
    @FocusState private var focus: Bool

    private var form: Incognito.Form {
        #if os(tvOS)
        .tv
        #elseif os(macOS)
        .mac
        #else
        .phone
        #endif
    }

    private var tv: Bool { form == .tv }

    var body: some View {
        let intro = Incognito.intro(debrid: !(debrid.view.snapshot?.accounts.isEmpty ?? true), form: form)
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: tv ? Theme.Space.l : Theme.Space.m) {
                ProfileAvatar(Incognito.id, name: Incognito.name, size: tv ? 128 : 96)
                Text(Incognito.heading)
                    .display(metrics.wide ? 44 : Theme.type.title1 + 4)
                    .foregroundStyle(Theme.Palette.text)
                    .accessibilityAddTraits(.isHeader)
                group("Not kept", intro.notKept)
                group("Still kept or seen", intro.seen)
                Text(intro.ends)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                buttons.padding(.top, Theme.Space.s)
            }
            .frame(maxWidth: tv ? 1100 : 560, alignment: .leading)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.xl)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        #if os(tvOS)
        .defaultFocus($focus, true)
        .onExitCommand { profiles.leaveIncognito() }
        #endif
    }

    private func group(_ title: String, _ lines: [String]) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Text(title)
                .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            ForEach(lines, id: \.self) { line in
                Text(line)
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var buttons: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: Theme.Space.s) { continueButton; leaveButton }
            VStack(alignment: .leading, spacing: Theme.Space.s) { continueButton; leaveButton }
        }
    }

    private var continueButton: some View {
        Button("Continue") { profiles.continueIntro() }
            .buttonStyle(PillButtonStyle(prominent: true))
            .focused($focus)
            #if !os(tvOS)
            .keyboardShortcut(.defaultAction)
            #endif
    }

    private var leaveButton: some View {
        Button { profiles.leaveIncognito() } label: { Label(Incognito.leave, glyph: "log-out") }
            .buttonStyle(PillButtonStyle())
    }
}

/// The ended line over a single profile's Home (5.4), a polite notice with OK: OK tells the core it was read, so the
/// line stays until then, across a reload too.
struct IncognitoEndedNotice: View {
    let ok: () -> Void

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            Text(Incognito.ended)
                .textFont(Theme.type.body, weight: .semibold)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
            Button("OK", action: ok).buttonStyle(PillButtonStyle())
        }
        .padding(.horizontal, Theme.Space.m)
        .padding(.vertical, Theme.Space.s)
        .flatCard()
        .accessibilityElement(children: .contain)
        .onAppear { AccessibilityNotification.Announcement(Incognito.ended).post() }
    }
}

#if os(iOS)
/// What the app switcher shows of an Incognito session (5.5): the page colour and the mark, nothing of the screen.
struct IncognitoCover: View {
    var body: some View {
        Theme.Palette.bg
            .ignoresSafeArea()
            .overlay { ProfileAvatar(Incognito.id, name: Incognito.name, size: 96) }
            .accessibilityHidden(true)
    }
}

/// Puts `IncognitoCover` over every window of the app, above sheets and the player, while `Incognito.covers` says so:
/// the system's app switcher snapshot is taken after the app leaves, so the cover must be on screen by then.
struct IncognitoCovering: ViewModifier {
    let on: Bool
    let playerOnScreen: Bool
    @Environment(\.scenePhase) private var phase
    @State private var windows: [UIWindow] = []

    func body(content: Content) -> some View {
        content.onChange(of: phase) { _, phase in
            if on, Incognito.covers(phase, playerOnScreen: playerOnScreen) { cover() } else { uncover() }
        }
    }

    private func cover() {
        guard windows.isEmpty else { return }
        windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.map { scene in
            let window = UIWindow(windowScene: scene)
            window.windowLevel = .alert + 1
            window.rootViewController = UIHostingController(rootView: IncognitoCover())
            window.isHidden = false
            return window
        }
    }

    private func uncover() {
        windows.forEach { $0.isHidden = true }
        windows = []
    }
}
#endif
