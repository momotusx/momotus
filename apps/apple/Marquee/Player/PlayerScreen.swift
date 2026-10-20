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
    /// The safe area's foot (the home indicator), which the drawn subtitles keep clear of.
    @State private var safeBottom: CGFloat = 0
    /// The drawn subtitle's box at full size, for the phone's rule.
    @State private var subtitleBox: CGFloat = 0
    #if os(iOS)
    @State private var pip = PictureInPicture()
    @Environment(FloatingPlayer.self) private var floating
    #endif
    let core: CoreClient

    init(route: PlayRoute, versions: VersionsModel?, core: CoreClient) {
        _model = State(initialValue: PlayerModel(route: route, versions: versions, core: core))
        self.core = core
    }

    #if os(iOS)
    /// The film brought back from Picture in Picture, with its own player and window.
    init(held: FloatingPlayer.Held, core: CoreClient) {
        _model = State(initialValue: held.model)
        _pip = State(initialValue: held.pip)
        self.core = core
    }
    #endif

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
                #if os(macOS)
                // A double click fills the screen, as in every Mac player.
                .onTapGesture(count: 2) { NSApp.keyWindow?.toggleFullScreen(nil) }
                #endif
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
                    let place = subtitlePlace(geo.size)
                    // Its foot stays put; a larger caption style or a second line grows upward.
                    SubtitleLine(text: text, size: model.subtitleSize)
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { subtitleBox = $0 }
                        .scaleEffect(place.scale, anchor: .bottom)
                        .frame(width: geo.size.width, height: max(place.foot, 0), alignment: .bottom)
                }
                .ignoresSafeArea()
                .allowsHitTesting(false)
                .animation(Theme.Motion.panel, value: controls)
            }
            StatusOverlay(
                model: model, back: close, chooseVersion: model.versions == nil ? nil : { showVersions = true },
                overControls: PlayerLayout.statusOverControls(controlsShown: controlsShown, windowHeight: windowHeight, controlsHeight: controlsHeight)
            )
            if controlsShown {
                PlayerControls(model: model, versions: model.versions, back: close, hold: { open in
                    menuOpenedAt = open ? .now : nil
                    lastTouch = .now
                }, activity: { lastTouch = .now }, pip: pipControl, windowHeight: windowHeight, measured: { controlsHeight = $0 })
                    .transition(.opacity)
            } else if model.playback?.phase != "failed", let trakt = model.playback?.trakt, TraktRules.playbackMark(trakt)?.warn == true {
                // A Trakt warning stays in sight with the controls hidden: it often comes after they hid.
                TraktMark(trakt: trakt)
                    .padding(.horizontal, Theme.Space.s)
                    .padding(.vertical, Theme.Space.xs)
                    .background(Capsule().fill(.black.opacity(0.55)))
                    .frame(maxWidth: 520, alignment: .leading)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .padding(Theme.Space.m)
                    .allowsHitTesting(false)
                    .transition(.opacity)
            }
            if let status = model.subtitleStatus, model.playback?.phase != "failed" {
                // Why the picked subtitles did not load, for a few seconds, clear of Skip at the other corner.
                Text(status)
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(.white)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, Theme.Space.s)
                    .padding(.vertical, Theme.Space.xs)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.m, style: .continuous).fill(.black.opacity(0.62)))
                    .frame(maxWidth: 520, alignment: .leading)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
                    .padding(.leading, Theme.Space.m)
                    .padding(.bottom, PlayerLayout.overlayBottom(controlsShown: controlsShown, controlsHeight: controlsHeight))
                    .allowsHitTesting(false)
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
        .onGeometryChange(for: CGFloat.self) { $0.safeAreaInsets.bottom } action: { safeBottom = $0 }
        .animation(Theme.Motion.panel, value: model.showNext)
        .animation(Theme.Motion.panel, value: model.showEndCard)
        .animation(Theme.Motion.panel, value: model.segment)
        .animation(Theme.Motion.panel, value: model.subtitleStatus)
        // Said once, as it shows.
        .onChange(of: model.subtitleStatus) { _, now in if let now { announce(now) } }
        // A Trakt warning is said once, when it arrives.
        .onChange(of: model.playback?.trakt) { _, now in
            if let mark = TraktRules.playbackMark(now), mark.warn { announce(mark.label) }
        }
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
        .task(id: model.taskId) { await model.drive() }
        // Back from the background on a new port: the source opens again where it was.
        .onChange(of: core.serverMoves) { model.serverMoved() }
        // The size reaches the file's own tracks as soon as it changes.
        .onChange(of: model.subtitleSize) { model.textSize() }
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
            float()
            #endif
        }
        .onDisappear {
            immersion.active = false
            #if os(iOS)
            PlayerOrientation.leave()
            #endif
            // A film floating in Picture in Picture plays on with its page gone.
            if !model.floating { model.close() }
        }
        .preferredColorScheme(.dark)
        .navigationBarBackButtonHidden()
        #if os(iOS)
        .toolbar(.hidden, for: .navigationBar, .tabBar)
        .statusBarHidden(!controls)
        .persistentSystemOverlays(.hidden)
        // A hardware keyboard on iPad: the Mac's player keys, listed when Command is held.
        .background {
            Group {
                Button("Play or Pause") { model.togglePlay() }.keyboardShortcut(.space, modifiers: [])
                Button("Back 10 Seconds") { model.skip(-10) }.keyboardShortcut(.leftArrow, modifiers: [])
                Button("Forward 10 Seconds") { model.skip(10) }.keyboardShortcut(.rightArrow, modifiers: [])
                Button("Close Player") { close() }.keyboardShortcut(.escape, modifiers: [])
            }
            .opacity(0)
            .accessibilityHidden(true)
        }
        #elseif os(macOS)
        .toolbarVisibility(.hidden, for: .windowToolbar)
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.space) { model.togglePlay(); return .handled }
        .onKeyPress(.leftArrow) { model.skip(-10); return .handled }
        .onKeyPress(.rightArrow) { model.skip(10); return .handled }
        .onKeyPress(.escape) { close(); return .handled }
        .focusedSceneValue(\.playerKeys, PlayerKeys(model: model, close: close))
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

    #if os(iOS)
    /// Picture in Picture leaves the page for the places with the film playing
    /// on; bringing it back shows the player again over them; the window's X
    /// ends the film as Back does. A film started here closes a floating one.
    private func float() {
        let (model, pip, floating, sync, dismiss) = (self.model, self.pip, self.floating, self.sync, self.dismiss)
        floating.end(except: model)
        pip.on(start: {
            floating.float(.init(model: model, pip: pip))
            dismiss()
        }, restore: floating.restore, stop: {
            if !floating.stopped(model) {
                model.end()
                sync.schedule()
            }
        })
    }
    #endif

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

    /// Where the drawn subtitle's box ends and how large it draws: on the
    /// iPhone in the black band under the picture when it holds it, else clear
    /// of the scrubber and the transport (`PhoneFrame.subtitle`); elsewhere at
    /// the picture's foot over the controls.
    private func subtitlePlace(_ size: CGSize) -> (foot: CGFloat, scale: CGFloat) {
        #if os(iOS)
        if UIDevice.current.userInterfaceIdiom == .phone {
            return PhoneFrame.subtitle(
                screenHeight: size.height, pictureHeight: fittedHeight(size), safeBottom: safeBottom,
                controlsShown: controlsShown, scrubberTop: size.height - safeBottom - controlsHeight, boxHeight: subtitleBox)
        }
        #endif
        let foot = PlayerLayout.subtitleFoot(
            screenHeight: size.height, pictureHeight: fittedHeight(size), safeBottom: safeBottom,
            controlsShown: controlsShown, controlsHeight: controlsHeight)
        return (foot - 10, 1)
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
                #if os(macOS)
                // The pointer goes with the controls and comes back with them on the next move;
                // never while another app is in front.
                if NSApp.isActive { NSCursor.setHiddenUntilMouseMoves(true) }
                #endif
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
            // A white pill: the ring shows only with focus, as on every pill.
            Button(action: skip) { Label(segment.label, glyph: "skip-forward") }
                .buttonStyle(PillButtonStyle(prominent: true))
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
    let size: SubtitleSize

    var body: some View {
        let style = CaptionStyle.current
        let points = (Theme.type.headline + 3) * style.scale * CaptionStyle.factor(size)
        Text(text)
            .font(style.face.map { .custom($0, size: points, relativeTo: .title3) } ?? .text(points, weight: .semibold, relativeTo: .title3))
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
    /// Set when the shown controls reach the middle: the waiting status sits
    /// this far up, just over them (PlayerLayout.statusOverControls).
    var overControls: CGFloat?
    /// The way on takes focus when the failure shows, so a remote is on it;
    /// after a Try again, Try again keeps it.
    @FocusState private var focus: FailureAction?

    /// The card's actions, the first the way on with focus. A view that could
    /// not open has no offer, and its Try again opens it again.
    @ViewBuilder private func actions(cannotOpen: Bool) -> some View {
        let order = actionOrder(cannotOpen: cannotOpen)
        ForEach(order, id: \.self) { action in
            button(action, cannotOpen: cannotOpen)
                .buttonStyle(PillButtonStyle(prominent: action == order.first))
                .fixedSize()
                .focused($focus, equals: action)
        }
    }

    private func button(_ action: FailureAction, cannotOpen: Bool) -> Button<Text> {
        let label = PlayerLayout.failureLabel(action, offer: model.playback?.offer?.label)
        return switch action {
        case .offer: Button(label) { if let key = model.playback?.offer?.key { model.switchVersion(key) } }
        case .retry: Button(label) { if cannotOpen { model.reopen() } else { model.retry() } }
        case .choose: Button(label) { chooseVersion?() }
        case .back: Button(label, action: back)
        }
    }

    /// A headline, one line per reason, and the actions.
    private func card(_ headline: String, lines: [String], cannotOpen: Bool) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(headline)
                .display(Theme.type.title1)
            // A Try again that failed again at once: the card stays and says it tried.
            if let note = PlayerLayout.failureNote(tryingAgain: model.tryingAgain) {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small).tint(Theme.Palette.text)
                    Text(note).font(.text(Theme.type.body))
                }
            }
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                Label(line, glyph: "circle-x")
                    .font(.text(Theme.type.body))
                    .foregroundStyle(Theme.Palette.muted)
            }
            // Side by side when they fit, else stacked with the way on first.
            ViewThatFits(in: .horizontal) {
                HStack(spacing: Theme.Space.s) { actions(cannotOpen: cannotOpen) }
                VStack(alignment: .leading, spacing: 10) { actions(cannotOpen: cannotOpen) }
            }
            .padding(.top, 8)
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.l)
        .frame(maxWidth: 640, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
        .padding(.horizontal, Theme.Space.m)
        .onAppear { focus = PlayerLayout.failureFocus(actionOrder(cannotOpen: cannotOpen)) }
    }

    private func actionOrder(cannotOpen: Bool) -> [FailureAction] {
        cannotOpen
            ? PlayerLayout.cannotOpenActions(canChoose: chooseVersion != nil)
            : PlayerLayout.failureActions(offer: model.playback?.offer != nil, canChoose: chooseVersion != nil)
    }

    private var waitingWords: some View {
        Text(PlayerLayout.waitingWords(model.playback, notice: model.notice, tryingAgain: model.tryingAgain))
            .font(.text(Theme.type.callout, relativeTo: .callout))
            .foregroundStyle(.white.opacity(0.8))
    }

    var body: some View {
        let status = PlayerLayout.status(
            phase: model.playback?.phase, cannotOpen: model.openError != nil,
            firstFrameShown: model.firstFrameMs != nil, waiting: model.waiting)
        switch status {
        case .failed:
            card(PlayerLayout.failureHeadline(offer: model.playback?.offer != nil),
                 lines: FailedSources.group(model.playback?.attempts ?? []).map { "\($0.via): \($0.words)" },
                 cannotOpen: false)
        case .cannotOpen:
            card(PlayerLayout.cannotOpenHeadline, lines: [model.openError ?? ""], cannotOpen: true)
        case .waiting(let words):
            if let overControls {
                HStack(spacing: 10) {
                    ProgressView().tint(.white)
                    if words { waitingWords.lineLimit(1) }
                }
                .padding(.horizontal, Theme.Space.s)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                .padding(.bottom, overControls)
                .allowsHitTesting(false)
            } else {
                VStack(spacing: 14) {
                    ProgressView().controlSize(.large).tint(.white)
                    if words { waitingWords }
                }
                .allowsHitTesting(false)
            }
        case .none:
            EmptyView()
        }
    }
}
