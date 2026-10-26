import AVFoundation
import AetherEngine
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
    /// This title's Versions over the paused player: Choose another version, or the failure card's way on.
    @State private var showVersions = false
    /// Where the card's Settings goes when no version could start: sources or accounts.
    @State private var settingsPlace: SettingsPlace?
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
    /// Skip's and Up Next's measured heights, which the phone's subtitle keeps over.
    @State private var skipHeight: CGFloat = 0
    @State private var upNextHeight: CGFloat = 0
    #if os(iOS)
    @State private var pip = PictureInPicture()
    @Environment(FloatingPlayer.self) private var floating
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// The level swipes (plans/player-gestures.md): the picture layer's size, the safe area's top, what the
    /// indicator shows, the system volume and brightness they set.
    @State private var swipeArea: CGSize = .zero
    @State private var safeTop: CGFloat = 0
    @State private var levels = SwipeLevel()
    @State private var volume = SystemVolume()
    @State private var keeper = BrightnessKeeper()
    /// VoiceOver or Switch Control, kept from their notifications so the body follows when either starts.
    @State private var assistive = false
    /// The double tap's answer: the side, and a count so a second one restarts it.
    @State private var seekForward: Bool?
    @State private var seekTaps = 0
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
            // The player under Versions is inert: nothing in it can be reached, read or pressed.
            ZStack {
                Color.black.ignoresSafeArea()
                Group {
                    // The second engine draws into its own view (plans/apple-second-engine.md 2.4).
                    if model.backend == .aether, let engine = model.aether?.engine {
                        AetherPlayerSurface(engine: engine)
                    } else {
                        VideoSurface(player: model.player, onFirstFrame: model.firstFrame, onLayer: attachLayer)
                    }
                }
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
                    .onTapGesture { touched(toggle: true) }
                    #else
                    // A double tap on either half steps 10 s, as Android does; the single tap waits for it.
                    .onGeometryChange(for: CGSize.self) { $0.size } action: { swipeArea = $0 }
                    .gesture(
                        SpatialTapGesture(count: 2).onEnded { seekTap($0.location) }
                            .exclusively(before: TapGesture().onEnded { touched(toggle: true) })
                    )
                    .gesture(levelSwipe)
                    #endif
                    // With the controls hidden, VoiceOver finds one element that brings them back.
                    .accessibilityElement()
                    .accessibilityLabel("Show controls")
                    .accessibilityAddTraits(.isButton)
                    .accessibilityAction { touched(toggle: false) }
                    .accessibilityHidden(controls)
                #endif
                #if os(iOS)
                if levels.shown {
                    LevelIndicator(levels: levels)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: levels.side == .brightness ? .leading : .trailing)
                        .padding(.horizontal, 40)
                        .transition(.opacity)
                }
                if let forward = seekForward {
                    SeekHint(forward: forward)
                        .id(seekTaps)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: forward ? .trailing : .leading)
                        .padding(.horizontal, 64)
                }
                // In the window for the volume swipe, and so the player answers the buttons; never under VoiceOver or
                // Switch Control, whose viewers keep the system's own volume HUD.
                if !assistive {
                    VolumeControl(volume: volume)
                        .frame(width: 1, height: 1)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
                #endif
                StatusOverlay(
                    model: model, back: close, chooseVersion: model.versions == nil ? nil : { openVersions() },
                settings: { settingsPlace = $0 },
                    overControls: phoneControls
                        ? PlayerLayout.statusOverTransport(windowHeight: windowHeight, safeBottom: safeBottom)
                        : PlayerLayout.statusOverControls(controlsShown: controlsShown, windowHeight: windowHeight, controlsHeight: controlsHeight),
                    spinnerInPlay: phoneControls
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
                // The file's bitmap subtitles (PGS, DVD, DVB) on the second engine, where the disc placed them.
                if model.playback?.phase != "failed" {
                    let images = model.subtitleImages
                    if !images.isEmpty {
                        GeometryReader { geo in
                            ForEach(images.indices, id: \.self) { index in
                                let rect = PlayerLayout.imageCueRect(position: images[index].position, video: model.videoSize, view: geo.size)
                                Image(decorative: images[index].image, scale: 1)
                                    .resizable()
                                    .frame(width: rect.width, height: rect.height)
                                    .position(x: rect.midX, y: rect.midY)
                            }
                        }
                        .ignoresSafeArea()
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                    }
                }
                // Over the controls and their scrim, so a line never reads dimmed with
                // them up (the iPhone walk, round 3); the rules keep it off their buttons.
                if let text = model.subtitleText, model.playback?.phase != "failed" {
                    GeometryReader { geo in
                        let place = subtitlePlace(geo.size)
                        // Its foot stays put; a larger caption style or a second line grows upward.
                        SubtitleLine(text: text, size: model.subtitleSize)
                            // The player's own file subtitles were never spoken line by line, so the file's are not; an
                            // addon track's lines stay readable, as they were.
                            .accessibilityHidden(model.subtitle == nil)
                            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { subtitleBox = $0 }
                            .scaleEffect(place.scale, anchor: .bottom)
                            .frame(width: geo.size.width, height: max(place.foot, 0), alignment: .bottom)
                    }
                    .ignoresSafeArea()
                    .allowsHitTesting(false)
                    .animation(Theme.Motion.panel, value: controls)
                }
                if let status = model.statusLine, model.playback?.phase != "failed" {
                    // The status line (a subtitle trouble, a move or pause for heat) for a few seconds, clear of Skip at the other corner.
                    Text(status)
                        .textFont(Theme.type.callout, relativeTo: .callout)
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
                if skipShown, let segment = model.segment {
                    // Netflix places it bottom right on the controls' top line, and it
                    // stays there whether or not the controls show.
                    SkipButton(segment: segment) { model.skip(segment) }
                        .id(segment)
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { skipHeight = $0 }
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
                        .padding(.trailing, Theme.Space.m)
                        .padding(.bottom, PlayerLayout.overlayBottom(controlsShown: controlsShown, controlsHeight: controlsHeight))
                        .transition(.opacity)
                }
                if upNextShown {
                    // Just over the measured controls while they show; on a short window it waits for them to hide.
                    UpNextCard(model: model, short: windowHeight > 0 && windowHeight < PlayerLayout.upNextRoom)
                        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { upNextHeight = $0 }
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
            .disabled(showVersions)
            .accessibilityHidden(showVersions)
            if showVersions, let versions = model.versions {
                // Over the player, which waits paused under it: a pick replaces the play, Back returns to it.
                VersionsScreen(model: versions, heading: model.route.target.title, core: core, choose: { key in
                    model.switchVersion(key)
                    closeVersions()
                }, back: { closeVersions() })
                // Modal: VoiceOver, Tab and the TV's focus stay on the page while it covers the player.
                .accessibilityAddTraits(.isModal)
                .transition(.opacity)
            }
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.height + $0.safeAreaInsets.top + $0.safeAreaInsets.bottom } action: { windowHeight = $0 }
        .onGeometryChange(for: CGFloat.self) { $0.safeAreaInsets.bottom } action: { safeBottom = $0 }
        #if os(iOS)
        .onGeometryChange(for: CGFloat.self) { $0.safeAreaInsets.top } action: { safeTop = $0 }
        .animation(reduceMotion ? nil : .easeOut(duration: levels.shown ? 0.18 : 0.26), value: levels.shown)
        .sensoryFeedback(.impact(weight: .light), trigger: levels.ends)
        .task(id: seekTaps) {
            guard seekForward != nil else { return }
            do { try await Task.sleep(for: .milliseconds(700)) } catch { return }
            seekForward = nil
        }
        // The system brightness goes back when the app does, and a return that did not take is tried again when it
        // comes back; Control Center (inactive) changes nothing.
        .onChange(of: scenePhase) { _, phase in
            if phase == .background || (phase == .active && keeper.pending) { keeper.restore() }
        }
        // The second engine's picture goes to Picture in Picture once it shows (plans/iphone-mkv.md).
        // Each engine session's first picture, failovers included; a source that leaves the engine takes its button.
        .onChange(of: model.enginePictures) { attachEngine() }
        .onChange(of: model.backend) { old, _ in if old == .aether { pip.detach(keeping: model.player) } }
        // The volume swipe turned off under the finger: its indicator goes at once.
        .onChange(of: volume.off) { _, off in if off, levels.side == .volume { levels.hideNow() } }
        .onReceive(NotificationCenter.default.publisher(for: UIAccessibility.voiceOverStatusDidChangeNotification)) { _ in
            assistive = assistiveTechRunning
        }
        .onReceive(NotificationCenter.default.publisher(for: UIAccessibility.switchControlStatusDidChangeNotification)) { _ in
            assistive = assistiveTechRunning
        }
        #endif
        .animation(Theme.Motion.panel, value: model.showNext)
        .animation(Theme.Motion.panel, value: model.showEndCard)
        .animation(Theme.Motion.panel, value: model.segment)
        .animation(Theme.Motion.panel, value: model.statusLine)
        // Said once, as it shows.
        .onChange(of: model.statusLine) { _, now in if let now { announce(now) } }
        // A Trakt warning is said once, when it arrives.
        .onChange(of: model.playback?.trakt) { _, now in
            if let mark = TraktRules.playbackMark(now), mark.warn { announce(mark.label) }
        }
        // Back from the card's Settings: the versions are asked again, so a source added there plays.
        .onChange(of: settingsPlace) { old, new in
            if old != nil, new == nil, model.noPick { model.retryPick() }
        }
        .navigationDestination(item: $settingsPlace) { place in
            switch place {
            case .addons: AddonsPage(core: core, sourcesOnly: true)
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
        .accessibilityAction(.escape) { escape() }
        .onAppear {
            immersion.active = true
            #if os(iOS)
            #if DEBUG
            if DebugHooks.playUpright, model.lastAcross == nil { model.lastAcross = false }
            #endif
            PlayerOrientation.enter(across: PlayerLayout.opensAcross(lastAcross: model.lastAcross))
            float()
            assistive = assistiveTechRunning
            let levels = levels
            volume.watch { value in
                // The buttons' answer on the right; under VoiceOver the system's HUD answers instead.
                if !(UIAccessibility.isVoiceOverRunning || UIAccessibility.isSwitchControlRunning) { levels.show(.volume, at: Double(value)) }
            }
            #if DEBUG
            WalkLevels.begin(volume)
            #endif
            #endif
        }
        .onDisappear {
            immersion.active = false
            #if os(iOS)
            if let across = PlayerOrientation.leave() { model.lastAcross = across }
            keeper.restore()
            #if DEBUG
            // close() put it back while the volume view was in the window; this is the fallback.
            WalkLevels.end(volume)
            #endif
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
            if !showVersions {
                Group {
                    Button("Play or Pause") { model.togglePlay() }.keyboardShortcut(.space, modifiers: [])
                    Button("Back 10 Seconds") { model.skip(-10) }.keyboardShortcut(.leftArrow, modifiers: [])
                    Button("Forward 10 Seconds") { model.skip(10) }.keyboardShortcut(.rightArrow, modifiers: [])
                    Button("Close Player") { close() }.keyboardShortcut(.escape, modifiers: [])
                }
                .opacity(0)
                .accessibilityHidden(true)
            }
        }
        #elseif os(macOS)
        .toolbarVisibility(.hidden, for: .windowToolbar)
        .focusable()
        .focusEffectDisabled()
        // Under Versions the keys are the page's: Escape goes back to the player. With a track picker open the keys
        // are its own: these pass them on.
        .onKeyPress(.space) { playerKey { model.togglePlay() } }
        .onKeyPress(.leftArrow) { playerKey { model.skip(-10) } }
        .onKeyPress(.rightArrow) { playerKey { model.skip(10) } }
        .onKeyPress(.escape) {
            guard !model.pickerOpen else { return .ignored }
            escape()
            return .handled
        }
        // A new value as a picker opens or closes, so the menu's bare keys follow it.
        .focusedSceneValue(\.playerKeys, PlayerKeys(model: model, close: close, pickerOpen: model.pickerOpen))
        #elseif os(tvOS)
        // Back one step at a time, as Leanback and the Android TV app do: an
        // open menu closes itself first, then the controls hide, then the player leaves.
        .onExitCommand {
            if showVersions { closeVersions() } else if controlsShown { withAnimation(Theme.Motion.panel) { controls = false } } else { close() }
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
    /// The picture takes level swipes now (`SwipeRules.active`).
    private var swipesOn: Bool {
        SwipeRules.active(
            phone: UIDevice.current.userInterfaceIdiom == .phone, across: swipeArea.width > swipeArea.height,
            status: overlayStatus, endCard: model.showEndCard, pickerOpen: model.pickerOpen, versions: showVersions,
            floating: model.floating, assistive: assistive)
    }

    /// What the overlay shows over the picture, as `StatusOverlay` decides it.
    private var overlayStatus: PlayerStatus {
        PlayerLayout.status(
            phase: model.playback?.phase, cannotOpen: model.openError != nil,
            firstFrameShown: model.firstFrameMs != nil, waiting: model.waiting, noPick: model.noPick)
    }

    private var levelSwipe: LevelSwipe {
        let (levels, volume, keeper) = (levels, volume, keeper)
        return LevelSwipe(
            enabled: swipesOn, size: swipeArea, bands: SwipeRules.bands(safeTop: safeTop, safeBottom: safeBottom),
            onStart: { side in
                let start: Double? =
                    switch side {
                    case .brightness: keeper.start()
                    case .volume: volume.off ? nil : Double(volume.level)
                    }
                #if DEBUG
                if side == .volume, let start { WalkLevels.found(Float(start)) }
                #endif
                if let start { levels.begin(side, at: start) }
                return start
            },
            onLevel: { side, level in
                switch side {
                case .brightness:
                    keeper.apply(level)
                case .volume:
                    volume.set(Float(level))
                    // A set that did not take turned the swipe off: no level the device does not have.
                    if volume.off {
                        levels.hideNow()
                        return
                    }
                }
                levels.move(to: level)
                lastTouch = .now
            },
            onEnd: {
                levels.end()
                keeper.ended()
                volume.swipeEnded()
            })
    }

    /// A double tap steps 10 s back on the left half, forward on the right, while the picture shows.
    private func seekTap(_ at: CGPoint) {
        let seeks = SwipeRules.active(
            phone: false, across: true, status: overlayStatus, endCard: model.showEndCard, pickerOpen: model.pickerOpen,
            versions: showVersions, floating: model.floating, assistive: false)
        guard seeks, swipeArea.width > 0 else { return }
        let forward = at.x >= swipeArea.width / 2
        model.skip(forward ? 10 : -10)
        seekForward = forward
        seekTaps += 1
    }

    /// Picture in Picture leaves the page for the places with the film playing
    /// on; bringing it back shows the player again over them; the window's X
    /// ends the film as Back does. A film started here closes a floating one.
    private func float() {
        let (model, pip, floating, sync, dismiss) = (self.model, self.pip, self.floating, self.sync, self.dismiss)
        floating.end(except: model)
        // The window's layer reports first frames while the page is away.
        pip.onFrame = { model.firstFrame() }
        model.windowWatchesFrames = { pip.watchesFrames }
        pip.on(start: {
            model.pictureInPicture(active: true)
            floating.float(.init(model: model, pip: pip))
            dismiss()
        }, restore: floating.restore, stop: {
            model.pictureInPicture(active: false)
            if !floating.stopped(model) {
                model.end()
                sync.schedule()
            }
        })
    }
    #endif

    #if os(iOS)
    /// The second engine's surface for Picture in Picture, once it shows a picture: the native path's AVPlayerLayer.
    /// AVPlayer's own layer attaches itself from `VideoSurface`.
    // ponytail: the software path's sample buffer window comes back when a platform with Picture in Picture admits
    // software video (`EngineChoice.allowsSoftwareVideo`); the iPhone refuses that path in v1.
    private func attachEngine() {
        guard model.backend == .aether, let layer = model.aether?.engine.nativePlayerLayer else { return }
        pip.attach(layer)
    }
    #endif

    private var pipControl: PipControl? {
        #if os(iOS)
        PipControl(possible: pip.possible, start: pip.start)
        #else
        nil
        #endif
    }

    /// The iPhone's controls are up: the waiting spinner turns in their Play circle.
    private var phoneControls: Bool {
        #if os(iOS)
        controlsShown && UIDevice.current.userInterfaceIdiom == .phone
        #else
        false
        #endif
    }

    private var skipShown: Bool {
        PlayerLayout.skipShows(
            segment: model.segment != nil, failed: model.playback?.phase == "failed", endCard: model.showEndCard,
            pickerOpen: model.pickerOpen)
    }

    private var upNextShown: Bool {
        PlayerLayout.showsUpNext(due: model.showNext, controlsShown: controlsShown, menuOpen: menuOpen, windowHeight: windowHeight)
    }

    /// The taller of Skip and Up Next while one shows, both on the foot's right.
    private var pillHeight: CGFloat? {
        [skipShown ? skipHeight : nil, upNextShown ? upNextHeight : nil].compactMap { $0 }.max()
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
                controlsShown: controlsShown, scrubberTop: size.height - safeBottom - controlsHeight, boxHeight: subtitleBox,
                pillTop: pillHeight.map { size.height - safeBottom - PlayerLayout.overlayBottom(controlsShown: controlsShown, controlsHeight: controlsHeight) - $0 })
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
        #if os(iOS) && DEBUG
        // While the volume view is still in the window.
        WalkLevels.end(volume)
        #endif
        model.close()
        sync.schedule()
        dismiss()
    }

    #if os(macOS)
    /// The player's own keys, not while Versions or a track picker is over it.
    private func playerKey(_ action: () -> Void) -> KeyPress.Result {
        guard !showVersions, !model.pickerOpen else { return .ignored }
        action()
        return .handled
    }
    #endif

    private func escape() {
        if showVersions { closeVersions() } else { close() }
    }

    /// Versions over the paused player; the attempt keeps resolving under it.
    private func openVersions() {
        model.cover()
        withAnimation(Theme.Motion.panel) { showVersions = true }
    }

    private func closeVersions() {
        model.uncover()
        withAnimation(Theme.Motion.panel) { showVersions = false }
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
            #if DEBUG
            // A walk's picker shot keeps the controls (and the picker in them) up.
            if DebugHooks.openPicker != nil { continue }
            #endif
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

/// "Skip intro" and the like: it says only what it skips, never where its times came from. It takes focus when
/// it appears, so a remote can press it without opening the controls.
private struct SkipButton: View {
    let segment: Segment
    let skip: () -> Void
    @FocusState private var focused: Bool

    var body: some View {
        // A white pill: the ring shows only with focus, as on every pill.
        Button(action: skip) { Label(segment.label, glyph: "skip-forward") }
            .buttonStyle(PillButtonStyle(prominent: true))
            .focused($focused)
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

/// Where the card's Settings goes.
private enum SettingsPlace: Hashable { case addons }

/// Starting, buffering, a failed source and a failed playback, in words.
private struct StatusOverlay: View {
    let model: PlayerModel
    let back: () -> Void
    /// Opens this title's Versions over the player: failover only tries the
    /// same release from other sources, so another version is the way on.
    let chooseVersion: (() -> Void)?
    /// Settings, Addons or Accounts: the shared table's Settings when no version could start.
    let settings: (SettingsPlace) -> Void
    /// Set when the shown controls reach the middle: the waiting status sits
    /// this far up, just over them (PlayerLayout.statusOverControls).
    var overControls: CGFloat?
    /// The iPhone's controls are up: their Play circle shows the spinner, so only the words show here.
    var spinnerInPlay = false
    /// The way on takes focus when the failure shows, so a remote is on it;
    /// after a Try again, Try again keeps it.
    @FocusState private var focus: FailureAction?

    /// The card's actions, the first the way on with focus. A view that could
    /// not open has no offer, and its Try again opens it again.
    @ViewBuilder private func actions(_ order: [FailureAction], cannotOpen: Bool) -> some View {
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
        case .playFromDevice: Button(label) { if let key = kept { model.switchVersion(key) } }
        case .retry: Button(label) { if cannotOpen { model.reopen() } else if model.noPick { model.retryPick() } else { model.retry() } }
        case .choose, .versions: Button(label) { chooseVersion?() }
        case .settingsAddons: Button(label) { settings(.addons) }
        case .back: Button(label, action: back)
        }
    }

    /// The source kept on this device, which plays with no source in reach.
    private var kept: String? { PlayerLayout.kept(model.versions?.snapshot) }

    /// A headline, one line per reason, and the actions.
    private func card(_ headline: String, lines: [String], order: [FailureAction], cannotOpen: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text(headline)
                .display(Theme.type.title1)
            // A Try again that failed again at once: the card stays and says it tried.
            if let note = PlayerLayout.failureNote(tryingAgain: model.tryingAgain) {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small).tint(Theme.Palette.text)
                    Text(note).textFont(Theme.type.body)
                }
            }
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                Label(line, glyph: "circle-x")
                    .textFont(Theme.type.body)
                    .foregroundStyle(Theme.Palette.muted)
            }
            // Side by side when they fit, else stacked with the way on first.
            ViewThatFits(in: .horizontal) {
                HStack(spacing: Theme.Space.s) { actions(order, cannotOpen: cannotOpen) }
                VStack(alignment: .leading, spacing: 10) { actions(order, cannotOpen: cannotOpen) }
            }
            .padding(.top, 8)
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.l)
        .frame(maxWidth: 640, alignment: .leading)
        // A flat surface with no edge, as every card (design/DECISION.md, Surfaces).
        .background(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous).fill(Theme.Palette.surface))
        .padding(.horizontal, Theme.Space.m)
        .onAppear { focus = PlayerLayout.failureFocus(order) }
    }

    private var waitingWords: some View {
        Text(PlayerLayout.waitingWords(
            model.playback, notice: model.notice, tryingAgain: model.tryingAgain, findingPick: model.route.waitsForPick))
            .textFont(Theme.type.callout, relativeTo: .callout)
            .foregroundStyle(.white.opacity(0.8))
    }

    /// The core's quiet "Choose another version" under the stage line: its place is held from the
    /// start, so nothing moves when it shows; hidden, it is neither seen, focused nor read.
    private func another(_ status: PlayerStatus) -> some View {
        let shown = PlayerLayout.offersAnother(model.playback, status: status) && chooseVersion != nil
        return Button(StreamChoice.words.player.chooseAnother) { chooseVersion?() }
            .buttonStyle(PillButtonStyle())
            .fixedSize()
            .opacity(shown ? 1 : 0)
            .disabled(!shown)
            .allowsHitTesting(shown)
            .accessibilityHidden(!shown)
            .onChange(of: shown) { _, now in
                if now { AccessibilityNotification.Announcement(StreamChoice.words.player.chooseAnotherSaid).post() }
            }
    }

    var body: some View {
        let status = PlayerLayout.status(
            phase: model.playback?.phase, cannotOpen: model.openError != nil,
            firstFrameShown: model.firstFrameMs != nil, waiting: model.waiting, noPick: model.noPick)
        let canChoose = chooseVersion != nil
        switch status {
        case .failed:
            let playback = model.playback
            card(PlayerLayout.failureHeadline(empty: playback?.empty, offer: playback?.offer != nil),
                 lines: PlayerLayout.failureLines(playback?.attempts ?? []),
                 order: PlayerLayout.failureActions(empty: playback?.empty, offer: playback?.offer != nil, kept: kept != nil, canChoose: canChoose))
        case .noPick:
            let empty = PlayerLayout.noPickState(model.versions?.snapshot?.empty, failed: model.versions?.failure != nil)
            card(PlayerLayout.noPickHeadline(empty), lines: [], order: PlayerLayout.noPickActions(empty, canChoose: canChoose))
        case .cannotOpen:
            card(PlayerLayout.cannotOpenHeadline, lines: [model.openError ?? ""],
                 order: PlayerLayout.cannotOpenActions(canChoose: canChoose), cannotOpen: true)
        case .waiting(let words):
            if let overControls {
                VStack(spacing: 10) {
                    HStack(spacing: 10) {
                        if !spinnerInPlay { ProgressView().tint(.white) }
                        if words { waitingWords.lineLimit(1) }
                    }
                    .allowsHitTesting(false)
                    another(status)
                }
                .padding(.horizontal, Theme.Space.s)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
                .padding(.bottom, overControls)
            } else {
                VStack(spacing: 14) {
                    VStack(spacing: 14) {
                        ProgressView().controlSize(.large).tint(.white)
                        if words { waitingWords }
                    }
                    .allowsHitTesting(false)
                    another(status)
                }
            }
        case .none:
            EmptyView()
        }
    }
}
