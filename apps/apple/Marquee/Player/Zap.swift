import Accessibility
import SwiftUI

/// A zap pressed on a place (plans/zap.md 2.2): pushed on that place's stack, so Back goes where Zap was pressed.
struct ZapRoute: Hashable, Identifiable {
    /// Discover's selected kind; nil mixes films and series.
    let kind: String?
    let id = UUID()
}

/// Starts a zap on this place. The shell gives none to a kids profile, so its buttons hide.
struct ZapAction {
    let open: @MainActor (String?) -> Void
    /// Discover tells the shell its selected kind, which the Mac toolbar's Zap sends while Discover shows.
    var kindShown: @MainActor (String?) -> Void = { _ in }

    @MainActor func callAsFunction(kind: String? = nil) { open(kind) }
}

extension View {
    /// A place's zap, pushed over the page it shows.
    func zapRoutes(_ route: Binding<ZapRoute?>, core: CoreClient) -> some View {
        navigationDestination(item: route) { ZapScreen(kind: $0.kind, core: core) }
    }
}

/// Where a zap play came from: the core's pick (What is this?'s names) and the kind that was asked, kept on the
/// play's model so a film brought back from Picture in Picture still knows them.
struct ZapOrigin {
    let pick: ZapPick
    let kind: String?
}

/// What the player hands its controls and end card in a zap: the pick's names and the ways on.
struct ZapHost {
    let pick: ZapPick
    /// Zap again: from a press, or by itself for a play that did not start.
    let again: @MainActor (ZapRules.Ask) -> Void
    /// Start from the beginning: this video as a plain play at 0.
    let fromStart: @MainActor () -> Void
    let openTitle: @MainActor () -> Void
}

/// The zap screen's next zap, as an environment value.
struct ZapAgain {
    let run: @MainActor (ZapRules.Ask) -> Void

    @MainActor func callAsFunction(_ ask: ZapRules.Ask) { run(ask) }
}

extension EnvironmentValues {
    @Entry var zap: ZapAction? = nil
    @Entry var zapHost: ZapHost? = nil
    /// The zap screen's next zap, which takes the player's place; none over a film brought back from Picture in Picture.
    @Entry var zapAgain: ZapAgain? = nil
}

/// The round glass Zap button on the iPhone and iPad: Home's hero and Discover's title row.
struct ZapButton: View {
    /// Discover's selected kind; Home sends none.
    var kind: String?
    @Environment(\.zap) private var zap

    var body: some View {
        if let zap {
            Button { zap(kind: kind) } label: {
                Glyph("zap", size: 20)
                    .foregroundStyle(Theme.Palette.text)
                    .frame(width: 40, height: 40)
                    .glass(Circle())
                    // A 44 pt target around the 40 pt circle, as the player's round buttons.
                    .frame(width: Theme.minTarget, height: Theme.minTarget)
                    .contentShape(Circle())
            }
            .buttonStyle(.round)
            .accessibilityLabel("Zap")
        }
    }
}

/// A zap (plans/zap.md 2.2): the loading screen with no title while the core picks, then the player in its place. A
/// play that fails before its first frame is replaced here by the next zap, until `ZapRules.maxTries`.
struct ZapScreen: View {
    let kind: String?
    let core: CoreClient
    @Environment(\.dismiss) private var dismiss
    @Environment(Immersion.self) private var immersion
    @State private var phase = Phase.asking(tries: 0)
    /// Bumped per ask, so a press asks again even when the tries stay 0.
    @State private var asks = 0
    /// Bumped per play, so each play gets a player of its own.
    @State private var plays = 0

    enum Phase {
        case asking(tries: Int)
        case playing(PlayRoute, VersionsModel, ZapPick)
        /// The core answered no pick: its code, and the words for it.
        case missed(code: String?, words: String)
        case nothingPlayed
    }

    var body: some View {
        content
            .task(id: asks) { await ask() }
            // The player's own counts come and go with each play; these hold the bars away and the turn across
            // between plays, so an auto zap neither shows the tab bar nor turns the phone upright.
            .onAppear {
                immersion.enter()
                #if os(iOS)
                PlayerOrientation.enter(across: true)
                #endif
            }
            .onDisappear {
                immersion.leave()
                #if os(iOS)
                PlayerOrientation.leave()
                #endif
            }
            .preferredColorScheme(.dark)
            .navigationBarBackButtonHidden()
            #if os(iOS)
            .toolbar(.hidden, for: .navigationBar, .tabBar)
            .persistentSystemOverlays(.hidden)
            #elseif os(macOS)
            .toolbarVisibility(.hidden, for: .windowToolbar)
            #endif
    }

    @ViewBuilder private var content: some View {
        switch phase {
        case .asking:
            loading
        case .playing(let route, let versions, let pick):
            PlayerScreen(route: route, versions: versions, core: core, zap: ZapOrigin(pick: pick, kind: kind))
                .id(plays)
                .environment(\.zapAgain, ZapAgain { zap($0) })
        case .missed(let code, let words):
            quiet(words, again: ZapRules.missOffersAgain(code: code))
        case .nothingPlayed:
            quiet("Nothing played.", again: true)
        }
    }

    /// The player's loading screen with no title: nothing is named before What is this?.
    private var loading: some View {
        ZStack(alignment: .topLeading) {
            Color.black.ignoresSafeArea()
            ProgressView()
                .controlSize(.large)
                .tint(.white)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            #if !os(tvOS)
            // Back at the top left off the TV (design/DECISION.md, Loading); the remote's Back cancels on TV.
            Button { dismiss() } label: {
                Glyph("chevron-left", size: 22)
                    .foregroundStyle(.white)
                    .frame(width: 40, height: 40)
                    .glass(Circle())
                    .frame(width: Theme.minTarget, height: Theme.minTarget)
            }
            .buttonStyle(.round)
            .keyboardShortcut(.cancelAction)
            .accessibilityLabel("Back")
            .padding(Theme.Space.s)
            #endif
        }
    }

    /// "Nothing played." or a miss: text on the page color with its ways on (design/DECISION.md, States).
    private func quiet(_ text: String, again: Bool) -> some View {
        VStack(spacing: Theme.Space.m) {
            Text(text)
                .display(Theme.type.title2)
                .multilineTextAlignment(.center)
                .accessibilityAddTraits(.isHeader)
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 12) { quietActions(again: again) }
                VStack(spacing: 10) { quietActions(again: again) }
            }
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.l)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Theme.Palette.bg.ignoresSafeArea())
        .onAppear { AccessibilityNotification.Announcement(text).post() }
    }

    @ViewBuilder private func quietActions(again: Bool) -> some View {
        if again {
            Button { zap(.press) } label: { Label("Zap again", glyph: "zap") }
                .buttonStyle(PillButtonStyle(prominent: true))
        }
        Button("Back") { dismiss() }
            .buttonStyle(PillButtonStyle(prominent: !again))
            #if !os(tvOS)
            .keyboardShortcut(.cancelAction)
            #endif
    }

    private func ask() async {
        guard case .asking(let tries) = phase else { return }
        do {
            let pick = try await core.call("zap", kind.map { ["kind": $0] } ?? [:], as: ZapPick.self)
            // Back while the core picked: no player, and no versions asked, after the viewer left.
            guard !Task.isCancelled else { return }
            // The player opens at once and starts the pick once versions settle, as the hero's Play does.
            let versions = VersionsModel(target: ZapRules.target(pick, tries: tries), core: core)
            versions.start()
            show(.playing(versions.pending(), versions, pick))
        } catch is CancellationError {
        } catch {
            guard !Task.isCancelled else { return }
            let code = (error as? CoreError)?.code
            phase = .missed(code: code, words: ZapRules.missLine(code: code) ?? ErrorWords.of(error))
        }
    }

    /// The next zap in this screen's place; once the tries ran out, "Nothing played." instead.
    private func zap(_ ask: ZapRules.Ask) {
        // One auto zap per play: a failure that reports again while the next zap is asked counts once.
        guard ZapRules.counts(ask, playing: playing) else { return }
        guard let tries = ZapRules.nextTries(ask, after: tries) else {
            phase = .nothingPlayed
            return
        }
        phase = .asking(tries: tries)
        asks += 1
    }

    private var playing: Bool {
        if case .playing = phase { true } else { false }
    }

    private var tries: Int {
        switch phase {
        case .asking(let tries): tries
        case .playing(let route, _, _): route.target.zapTries
        case .missed, .nothingPlayed: 0
        }
    }

    private func show(_ playing: Phase) {
        plays += 1
        phase = playing
    }
}

/// Zap's dial in the player's bar, a segmented control: it changes only on a press (never on focus), saves the
/// profile's value and zaps again.
struct ZapDialControl: View {
    /// The profile's dial; nil until its prefs are there, when no segment takes a press.
    let selected: ZapDial?
    /// The bar shows words beside its icons: the segments take the bar's text size, else the phone's.
    let words: Bool
    /// Saves the dial and zaps again once it saved; the dial takes no other press until then.
    let choose: (ZapDial) async -> Void
    @State private var saving = false

    var body: some View {
        HStack(spacing: 0) {
            ForEach(ZapDial.allCases, id: \.self) { dial in
                let on = dial == selected
                Button {
                    guard !on else { return }
                    saving = true
                    Task {
                        await choose(dial)
                        saving = false
                    }
                } label: {
                    Text(ZapRules.word(dial))
                        .textFont(words ? Theme.type.callout : 14, weight: .semibold, relativeTo: .callout)
                        .fixedSize()
                        .foregroundStyle(on ? Theme.Palette.onPrimary : .white)
                        .padding(.horizontal, words ? 14 : 8)
                        .frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)
                        .background(Capsule().fill(on ? Theme.Palette.primary : .clear))
                        .contentShape(Capsule())
                }
                .buttonStyle(FocusPlainStyle(shape: Capsule()))
                .accessibilityLabel(ZapRules.spoken(dial))
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .background(Capsule().fill(.white.opacity(0.16)))
        .fixedSize()
        .disabled(selected == nil || saving)
        .opacity(selected == nil ? 0.5 : 1)
    }
}
