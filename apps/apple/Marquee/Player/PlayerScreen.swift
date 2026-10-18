import AVFoundation
import SwiftUI

/// Full-screen playback with an OSD that hides itself while playing.
struct PlayerScreen: View {
    @State private var model: PlayerModel
    @Environment(\.dismiss) private var dismiss
    @Environment(Immersion.self) private var immersion
    @Environment(SyncModel.self) private var sync
    @State private var controls = true
    @State private var lastTouch = ContinuousClock.now
    @State private var showNextVersions = false
    /// This title's Versions, after every source of the pick failed.
    @State private var showVersions = false
    /// When an OSD menu opened; a pick or the next touch clears it.
    @State private var menuOpenedAt: ContinuousClock.Instant?
    /// The controls stay up and Up Next waits while this holds (`PlayerLayout.menuHolds`).
    private var menuOpen: Bool { PlayerLayout.menuHolds(openedAt: menuOpenedAt, now: .now) }
    /// TV: the invisible target that holds focus while the controls are hidden.
    @FocusState private var catcherFocused: Bool
    /// The real window's height, bars included (the player hides them).
    @State private var windowHeight: CGFloat = 0
    /// The bottom controls' measured height; a guess until they have shown once.
    @State private var controlsHeight: CGFloat = 200
    #if os(iOS)
    @State private var pip = PictureInPicture()
    #endif
    let core: CoreClient

    init(route: PlayRoute, versions: VersionsModel?, core: CoreClient) {
        _model = State(initialValue: PlayerModel(route: route, versions: versions, core: core))
        self.core = core
    }

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            VideoSurface(player: model.player, onFirstFrame: model.firstFrame, onLayer: attachLayer)
                .ignoresSafeArea()
                .accessibilityHidden(true)
            // Taps on the picture toggle the controls. A gesture on the whole
            // screen instead would sit over the menus (Audio, Subtitles, ...)
            // and keep them from opening.
            #if os(tvOS)
            // A remote has no picture to tap: with the controls hidden this
            // invisible target holds focus, and a click or any move brings
            // them back (Play/Pause is on the whole screen, below).
            if !controls, model.playback?.phase != "failed", !model.showEndCard {
                Button { touched(toggle: false) } label: {
                    Color.clear.frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle())
                }
                .buttonStyle(BareStyle())
                .focused($catcherFocused)
                .onMoveCommand { _ in touched(toggle: false) }
                .onAppear { if model.segment == nil && !model.showNext { catcherFocused = true } }
                .ignoresSafeArea()
                .accessibilityLabel("Show controls")
            }
            #else
            Color.clear
                .contentShape(Rectangle())
                .ignoresSafeArea()
                .onTapGesture { touched(toggle: true) }
                // With the controls hidden, VoiceOver finds one element that brings them back.
                .accessibilityElement()
                .accessibilityLabel("Show controls")
                .accessibilityAddTraits(.isButton)
                .accessibilityAction { touched(toggle: false) }
                .accessibilityHidden(controls)
            #endif
            if let text = model.subtitleText {
                GeometryReader { geo in
                    // The bottom of the picture as fitted on screen, kept clear of the controls.
                    let picture = fittedHeight(geo.size)
                    let bottom = min((geo.size.height + picture) / 2 - 16, geo.size.height - (controlsShown ? controlsHeight + 16 : 40))
                    // Its foot stays put; a larger caption style or a second line grows upward.
                    SubtitleLine(text: text)
                        .frame(width: geo.size.width, height: max(bottom - 10, 0), alignment: .bottom)
                }
                .ignoresSafeArea()
                .allowsHitTesting(false)
                .animation(Theme.Motion.panel, value: controls)
            }
            StatusOverlay(model: model, back: close, chooseVersion: model.versions == nil ? nil : { showVersions = true })
            if controlsShown {
                PlayerControls(model: model, versions: model.versions, back: close, hold: { open in
                    menuOpenedAt = open ? .now : nil
                    lastTouch = .now
                }, activity: { lastTouch = .now }, pip: pipControl, windowHeight: windowHeight, measured: { controlsHeight = $0 })
                    .transition(.opacity)
            }
            if let segment = model.segment, model.playback?.phase != "failed", !model.showEndCard {
                // Netflix places it bottom right on the controls' top line, and it
                // stays there whether or not the controls show.
                SkipButton(segment: segment, showSource: controls) { model.skip(segment) }
                    .id(segment)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
                    .padding(.trailing, Theme.Space.m)
                    .padding(.bottom, PlayerLayout.overlayBottom(controlsShown: controlsShown, controlsHeight: controlsHeight))
                    .transition(.opacity)
            }
            if PlayerLayout.showsUpNext(due: model.showNext, controlsShown: controlsShown, menuOpen: menuOpen, windowHeight: windowHeight) {
                // Just over the measured controls while they show; on a short window it waits for them to hide.
                UpNextCard(model: model, short: windowHeight > 0 && windowHeight < PlayerLayout.upNextRoom)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
                    .padding(.horizontal, Theme.Space.m)
                    .padding(.bottom, PlayerLayout.overlayBottom(controlsShown: controlsShown, controlsHeight: controlsHeight))
                    .transition(.move(edge: .trailing).combined(with: .opacity))
                    .animation(Theme.Motion.panel, value: controls)
            }
            if model.showEndCard {
                EndCard(model: model, back: close) { showNextVersions = true }
                    .transition(.opacity)
            }
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.height + $0.safeAreaInsets.top + $0.safeAreaInsets.bottom } action: { windowHeight = $0 }
        .animation(Theme.Motion.panel, value: model.showNext)
        .animation(Theme.Motion.panel, value: model.showEndCard)
        .animation(Theme.Motion.panel, value: model.segment)
        .navigationDestination(isPresented: $showVersions) {
            if let versions = model.versions {
                VersionsScreen(model: versions, heading: model.route.target.title, core: core)
            }
        }
        .navigationDestination(isPresented: $showNextVersions) {
            if let versions = model.nextVersions, let next = model.next {
                VersionsScreen(model: versions, heading: "S\(next.season):E\(next.episode) \(next.title)", core: core)
            }
        }
        #if !os(tvOS)
        .onContinuousHover { _ in touched(toggle: false) }
        #endif
        .task(id: model.taskId) { await model.run() }
        .task { await autoHide() }
        // VoiceOver: a two-finger double tap plays or pauses; the scrub gesture leaves.
        #if !os(macOS)
        .accessibilityAction(.magicTap) { model.togglePlay() }
        #endif
        .accessibilityAction(.escape) { close() }
        .onAppear {
            immersion.active = true
            #if os(iOS)
            PlayerOrientation.enter()
            #endif
        }
        .onDisappear {
            immersion.active = false
            #if os(iOS)
            PlayerOrientation.leave()
            #endif
            model.close()
        }
        .preferredColorScheme(.dark)
        .navigationBarBackButtonHidden()
        #if os(iOS)
        .toolbar(.hidden, for: .navigationBar, .tabBar)
        .statusBarHidden(!controls)
        .persistentSystemOverlays(.hidden)
        #elseif os(macOS)
        .toolbarVisibility(.hidden, for: .windowToolbar)
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.space) { model.togglePlay(); return .handled }
        .onKeyPress(.leftArrow) { model.skip(-10); return .handled }
        .onKeyPress(.rightArrow) { model.skip(10); return .handled }
        .onKeyPress(.escape) { close(); return .handled }
        #elseif os(tvOS)
        // Back one step at a time, as Leanback and the Android TV app do: an
        // open menu closes itself first, then the controls hide, then the player leaves.
        .onExitCommand {
            if controlsShown { withAnimation(Theme.Motion.panel) { controls = false } } else { close() }
        }
        // The remote's Play/Pause, wherever focus is: it toggles and shows the controls.
        .onPlayPauseCommand {
            model.togglePlay()
            touched(toggle: false)
        }
        #endif
    }

    /// iOS: Picture in Picture on the video layer; elsewhere nothing.
    private var attachLayer: (@MainActor (AVPlayerLayer) -> Void)? {
        #if os(iOS)
        { [pip] layer in pip.attach(layer) }
        #else
        nil
        #endif
    }

    private var pipControl: PipControl? {
        #if os(iOS)
        PipControl(possible: pip.possible, start: pip.start)
        #else
        nil
        #endif
    }

    /// The OSD is on screen: not hidden, and no failure or end card in its place.
    private var controlsShown: Bool {
        controls && model.playback?.phase != "failed" && !model.showEndCard
    }

    /// Height of the picture fitted into `size`, or the full height before it is known.
    private func fittedHeight(_ size: CGSize) -> CGFloat {
        let video = model.videoSize
        guard video.width > 0, video.height > 0 else { return size.height }
        return min(size.height, size.width * video.height / video.width)
    }

    private func close() {
        model.close()
        sync.schedule()
        dismiss()
    }

    private func touched(toggle: Bool) {
        lastTouch = .now
        menuOpenedAt = nil
        withAnimation(Theme.Motion.panel) { controls = toggle ? !controls : true }
    }

    /// Hides the controls after three quiet seconds of playback, never while
    /// VoiceOver or Switch Control runs: their cursor is no touch, and the
    /// controls would go from under it.
    private func autoHide() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(500))
            if controls, model.playing, !menuOpen, !assistiveTechRunning, ContinuousClock.now - lastTouch > .seconds(3) {
                withAnimation(Theme.Motion.panel) { controls = false }
            }
        }
    }

    /// Read live, as either can start while the player is up.
    private var assistiveTechRunning: Bool {
        #if os(macOS)
        NSWorkspace.shared.isVoiceOverEnabled || NSWorkspace.shared.isSwitchControlEnabled
        #else
        UIAccessibility.isVoiceOverRunning || UIAccessibility.isSwitchControlRunning
        #endif
    }
}

/// "Skip intro" and the like. It takes focus when it appears, so a remote can
/// press it without opening the controls; its data source shows on focus or
/// with the controls.
private struct SkipButton: View {
    let segment: Segment
    let showSource: Bool
    let skip: () -> Void
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .trailing, spacing: 10) {
            // A white pill with an accent edge.
            Button(action: skip) { Label(segment.label, glyph: "skip-forward") }
                .buttonStyle(PillButtonStyle(prominent: true))
                .overlay { Capsule().strokeBorder(Theme.Palette.accent, lineWidth: 2).allowsHitTesting(false) }
                .focused($focused)
            Text("Skip data: \(segment.source)")
                .font(.text(Theme.type.caption, relativeTo: .caption))
                .foregroundStyle(.white.opacity(0.85))
                .shadow(color: .black.opacity(0.7), radius: 6)
                .opacity(focused || showSource ? 1 : 0)
                .accessibilityHidden(!(focused || showSource))
        }
        .onAppear { focused = true }
    }
}

#if os(tvOS)
/// The label alone: no focus mark, for the invisible target behind hidden controls.
private struct BareStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View { configuration.label }
}
#endif

/// Subtitle text in the viewer's caption style: by default white on a dark
/// box so it reads on any picture.
private struct SubtitleLine: View {
    let text: String

    var body: some View {
        let style = CaptionStyle.current
        let size = (Theme.type.headline + 3) * style.scale
        Text(text)
            .font(style.face.map { .custom($0, size: size, relativeTo: .title3) } ?? .text(size, weight: .semibold, relativeTo: .title3))
            .foregroundStyle(style.text)
            .modifier(CaptionEdge(edge: style.edge))
            .background(style.highlight)
            .multilineTextAlignment(.center)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: style.radius).fill(style.window))
            .padding(.horizontal, Theme.Space.l)
    }
}

/// Starting, buffering, a failed source and a failed playback, in words.
private struct StatusOverlay: View {
    let model: PlayerModel
    let back: () -> Void
    /// Opens this title's Versions: failover only tries the same release from
    /// other sources, so another version is the way on.
    let chooseVersion: (() -> Void)?
    /// The way on takes focus when the failure shows, so a remote is on it.
    @FocusState private var primary: Bool

    @ViewBuilder private var actions: some View {
        if let chooseVersion {
            Button("Choose another version", action: chooseVersion).buttonStyle(PillButtonStyle(prominent: true)).fixedSize()
                .focused($primary)
            Button("Back", action: back).buttonStyle(PillButtonStyle()).fixedSize()
        } else {
            Button("Back", action: back).buttonStyle(PillButtonStyle(prominent: true))
                .focused($primary)
        }
    }

    var body: some View {
        if model.playback?.phase == "failed" {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                Text("This could not play")
                    .display(Theme.type.title1)
                ForEach(Array((model.playback?.attempts ?? []).enumerated()), id: \.offset) { _, attempt in
                    Label("\(attempt.via): \(attempt.error)", glyph: "circle-x")
                        .font(.text(Theme.type.body))
                        .foregroundStyle(Theme.Palette.muted)
                }
                // Side by side when they fit, else stacked with the way on first.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: Theme.Space.s) { actions }
                    VStack(alignment: .leading, spacing: 10) { actions }
                }
                .padding(.top, 8)
            }
            .foregroundStyle(Theme.Palette.text)
            .padding(Theme.Space.l)
            .frame(maxWidth: 640, alignment: .leading)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
            .padding(.horizontal, Theme.Space.m)
            .onAppear { primary = true }
        } else if model.firstFrameMs == nil || model.waiting {
            VStack(spacing: 14) {
                ProgressView().controlSize(.large).tint(.white)
                if model.firstFrameMs == nil {
                    Text(model.notice ?? "Getting the stream ready")
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(.white.opacity(0.8))
                }
            }
            .allowsHitTesting(false)
        }
    }
}
