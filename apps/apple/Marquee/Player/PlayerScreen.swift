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
    /// An OSD menu is open: the controls stay up.
    @State private var menuOpen = false
    let core: CoreClient

    init(route: PlayRoute, versions: VersionsModel?, core: CoreClient) {
        _model = State(initialValue: PlayerModel(route: route, versions: versions, core: core))
        self.core = core
    }

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            VideoSurface(player: model.player, onFirstFrame: model.firstFrame)
                .ignoresSafeArea()
                .accessibilityHidden(true)
            // Taps on the picture toggle the controls. A gesture on the whole
            // screen instead would sit over the menus (Audio, Subtitles, ...)
            // and keep them from opening.
            Color.clear
                .contentShape(Rectangle())
                .ignoresSafeArea()
                .onTapGesture { touched(toggle: true) }
                .accessibilityHidden(true)
            if let text = model.subtitleText {
                GeometryReader { geo in
                    // The bottom of the picture as fitted on screen, kept clear of the controls.
                    let picture = fittedHeight(geo.size)
                    let bottom = min((geo.size.height + picture) / 2 - 16, geo.size.height - (controls ? 190 : 40))
                    SubtitleLine(text: text)
                        .frame(maxWidth: .infinity)
                        .position(x: geo.size.width / 2, y: bottom - 30)
                }
                .ignoresSafeArea()
                .allowsHitTesting(false)
                .animation(Theme.Motion.panel, value: controls)
            }
            StatusOverlay(model: model, back: close, chooseVersion: model.versions == nil ? nil : { showVersions = true })
            if controlsShown {
                PlayerControls(model: model, versions: model.versions, back: close) { open in
                    menuOpen = open
                    lastTouch = .now
                }
                    .transition(.opacity)
            }
            if let segment = model.segment, model.playback?.phase != "failed", !model.showEndCard {
                // Netflix places it bottom right on the controls' top line, and it
                // stays there whether or not the controls show.
                SkipButton(segment: segment, showSource: controls) { model.skip(segment) }
                    .id(segment)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
                    .padding(.trailing, Theme.Space.m)
                    .padding(.bottom, 212)
                    .transition(.opacity)
            }
            if model.showNext {
                // Docked 16 pt clear of the scrubber when the controls show.
                UpNextCard(model: model)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
                    .padding(.horizontal, Theme.Space.m)
                    .padding(.bottom, controls ? 212 : Theme.Space.l)
                    .transition(.move(edge: .trailing).combined(with: .opacity))
                    .animation(Theme.Motion.panel, value: controls)
            }
            if model.showEndCard {
                EndCard(model: model, back: close) { showNextVersions = true }
                    .transition(.opacity)
            }
        }
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
        .onAppear { immersion.active = true }
        .onDisappear {
            immersion.active = false
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
        withAnimation(Theme.Motion.panel) { controls = toggle ? !controls : true }
    }

    /// Hides the controls after three quiet seconds of playback.
    private func autoHide() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(500))
            if controls, model.playing, !menuOpen, ContinuousClock.now - lastTouch > .seconds(3) {
                withAnimation(Theme.Motion.panel) { controls = false }
            }
        }
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

/// Subtitle text on a dark box so it reads on any picture.
private struct SubtitleLine: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.text(Theme.type.headline + 3, weight: .semibold, relativeTo: .title3))
            .foregroundStyle(.white)
            .multilineTextAlignment(.center)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 6).fill(.black.opacity(0.62)))
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

    @ViewBuilder private var actions: some View {
        if let chooseVersion {
            Button("Choose another version", action: chooseVersion).buttonStyle(PillButtonStyle(prominent: true)).fixedSize()
            Button("Back", action: back).buttonStyle(PillButtonStyle()).fixedSize()
        } else {
            Button("Back", action: back).buttonStyle(PillButtonStyle(prominent: true))
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
