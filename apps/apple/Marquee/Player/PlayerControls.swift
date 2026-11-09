import SwiftUI
#if os(iOS)
import AVKit
#endif

/// Picture in Picture for the OSD: whether the system allows it now, and how to start it.
struct PipControl {
    let possible: Bool
    let start: () -> Void
}

/// The OSD (plans/player-bar.md, the owner's "Toolbar strip"): the title top left and the end time top right; at
/// the foot one slim bar, the scrubber's row and then Audio, Speed and Versions on the left, Back 10, Play and
/// Forward 10 in the middle, and Subtitles, the quick shift, Episodes, Next and Full screen on the right. Subtitles
/// and Audio open their strips over the bar's ends.
struct PlayerControls: View {
    let model: PlayerModel
    let versions: VersionsModel?
    let back: () -> Void
    /// True while a menu is open: the controls must stay up.
    var hold: (Bool) -> Void = { _ in }
    /// The viewer did something (focus moved, a seek on the bar): the controls stay up a while longer.
    var activity: () -> Void = {}
    /// iOS: Picture in Picture, when the system says it is possible.
    var pip: PipControl?
    /// The real window's height.
    var windowHeight: CGFloat = 0
    /// The bottom controls' height with their foot padding (and an open strip's), for Skip, Up Next and the subtitles.
    var measured: (CGFloat) -> Void = { _ in }
    /// The top bar's foot from the window's top, for a second subtitle line hung from the top (Split).
    var measuredTop: (CGFloat) -> Void = { _ in }
    /// This title's Versions over the player: the audio strip's Choose a version.
    var chooseVersion: (() -> Void)?
    /// A series: its title page on the current season and episode (section 5, Apple Episodes).
    var episodes: (() -> Void)?
    /// The subtitle picker over the picture (a sheet on the iPhone): More languages; on tvOS the subtitle and audio pickers.
    @State private var picker: TrackPickerPanel.Kind?
    /// The real window's width (with `windowHeight`, its turn), for the first pass before the bar is measured.
    var windowWidth: CGFloat = 0
    /// The bar's height, and an open strip's height.
    @State private var barHeight: CGFloat = 120
    @State private var stripHeight: CGFloat = 0
    /// TV seeking: how many quick presses in a row, and the last one's direction and time.
    @State private var seekRun = 0
    @State private var lastSeek: (direction: Int, at: ContinuousClock.Instant)?
    @Environment(\.metrics) private var metrics
    @State private var scrub: Double?
    /// Which control has focus: TV enters on Play/Pause, and every move keeps the controls up.
    @FocusState private var focus: Control?
    /// The bar's text sizes, following Dynamic Type as the text does.
    @ScaledMetric(relativeTo: .callout) private var wideText = Theme.type.callout
    /// The shift's words on the iPhone, scaled as QuickShift draws them (relative to callout).
    @ScaledMetric(relativeTo: .callout) private var phoneWord: CGFloat = 14
    @ScaledMetric(relativeTo: .caption) private var phoneTime = Theme.type.caption
    #if os(macOS)
    @State private var fullScreen = false
    /// The window the player is in, whose full screen the button shows and toggles.
    @State private var window: NSWindow?
    #endif

    enum Control: Hashable { case back, rewind, play, forward, scrub, audio, subtitles, versions, speed, episodes, next, fullScreen }

    /// The iPhone lays the OSD out as the system player does (`PhoneFrame`).
    private static var phone: Bool {
        #if os(iOS)
        UIDevice.current.userInterfaceIdiom == .phone
        #else
        false
        #endif
    }

    /// The phone's safe area, read from the layout.
    @State private var safe = EdgeInsets()

    /// The phone's turn, from the window the player measured, so a show never starts on the upright rows.
    private var landscape: Bool { windowWidth > windowHeight }

    /// The width the plans are made for: the bar's last measured width (kept on the model while the controls come
    /// and go), else the window's less the bar's insets.
    private var barWidth: CGFloat {
        BarFit.width(measured: model.barWidth.bar, measuredIn: model.barWidth.window, window: windowWidth,
                     inset: Self.phone ? 2 * PhoneFrame.edge : 2 * metrics.gutter)
    }

    private func measuredBar(_ size: CGSize) {
        model.barWidth = (size.width, windowWidth)
        barHeight = size.height
    }

    private var bar: StreamChoiceWords.Player.Bar { StreamChoice.words.player.bar }
    private var names: StreamChoiceWords.Player.Bar.Names { bar.names }

    var body: some View {
        Group {
            if Self.phone {
                phoneLayout
            } else {
                VStack(spacing: 0) {
                    top
                    Spacer()
                    bottom
                }
                .padding(.horizontal, metrics.gutter)
                .padding(.vertical, metrics.wide ? Theme.Space.m : Theme.Space.s)
            }
        }
        .onChange(of: focus) { activity() }
        #if os(tvOS)
        .defaultFocus($focus, .play)
        .onAppear { focus = .play }
        #endif
        .background {
            // Full-bleed scrims, never boxes, so every OSD word holds 4.5:1 on any picture. The bottom one reaches
            // over an open strip too (whose rows also sit on strong glass).
            VStack {
                LinearGradient(colors: [.black.opacity(0.6), .clear], startPoint: .top, endPoint: .bottom)
                    .frame(height: metrics.wide ? 200 : 120)
                Spacer()
                LinearGradient(colors: [.clear, .black.opacity(0.72)], startPoint: .top, endPoint: .bottom)
                    .frame(height: max(metrics.wide ? 320 : 300, barHeight + stripRoom + 160))
            }
            .ignoresSafeArea()
            .allowsHitTesting(false)
        }
        .foregroundStyle(.white)
        // Mac, iPad and TV: the full picker is a panel over the picture, just over the controls.
        .overlay(alignment: metrics.wide ? .bottomTrailing : .bottom) {
            if let picker, !Self.phone {
                TrackPickerPanel(kind: picker, model: model, close: closePicker)
                    #if !os(tvOS)
                    .frame(width: 400)
                    .frame(maxHeight: TrackPicker.panelHeight(window: windowHeight, below: barHeight + footPadding + Theme.Space.s))
                    .fixedSize(horizontal: false, vertical: true)
                    .glass(RoundedRectangle(cornerRadius: Theme.Radius.l, style: .continuous), strong: true)
                    #endif
                    .padding(.trailing, metrics.wide ? metrics.gutter : 0)
                    .padding(.bottom, barHeight + footPadding + Theme.Space.s)
                    .transition(.opacity)
            }
        }
        #if os(iOS)
        // The iPhone: a full-height sheet over the landscape player.
        .sheet(item: Binding(get: { Self.phone ? picker : nil }, set: { if $0 == nil { closePicker() } })) { kind in
            TrackPickerPanel(kind: kind, model: model, close: closePicker)
                .presentationDetents([.large])
                .presentationBackground(Theme.Palette.raised)
        }
        #endif
        .onChange(of: picker) { _, open in
            hold(open != nil)
            model.pickerOpen = open != nil
        }
        // The controls' foot block, an open strip included, for Skip, Up Next and the drawn subtitles.
        .onChange(of: barHeight + stripRoom, initial: true) { _, height in
            measured(height + (Self.phone ? PhoneFrame.bottomGap : footPadding))
        }
        // A strip that closes gives the keyboard back to the button that opened it.
        .onChange(of: model.strip) { old, now in
            if now == nil, let old, KeyboardNavigation.on { focus = old == .audio ? .audio : .subtitles }
        }
        // A picker goes with the controls (a click on the picture hides both): the keys go back to the player.
        .onDisappear { model.pickerOpen = false }
        .onDisappear { model.strip = nil }
        #if DEBUG
        .task {
            // A walk's shot: the picker once the core's subtitles are in (or the file's audio).
            while !Task.isCancelled, picker == nil, let kind = DebugHooks.openPicker {
                if kind == "audio" ? !model.audioTracks.isEmpty : (model.playback?.subtitles.count ?? 0) > 20 {
                    openPicker(kind == "audio" ? .audio : .subtitles)
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
        #endif
    }

    private func openPicker(_ kind: TrackPickerPanel.Kind) {
        withAnimation(Theme.Motion.panel) { picker = picker == kind ? nil : kind }
    }

    private func closePicker() {
        withAnimation(Theme.Motion.panel) { picker = nil }
    }

    /// Subtitles or Audio: its strip opens, or closes when it is open; only one is open at a time. tvOS keeps its
    /// full pickers.
    private func toggle(_ kind: BarStrip) {
        #if os(tvOS)
        openPicker(kind == .audio ? .audio : .subtitles)
        #else
        activity()
        withAnimation(Theme.Motion.panel) { model.strip = model.strip == kind ? nil : kind }
        #endif
    }

    private func closeStrip() {
        withAnimation(Theme.Motion.panel) { model.strip = nil }
    }

    /// More languages: today's full picker, in place of the strip.
    private func moreLanguages() {
        model.strip = nil
        openPicker(.subtitles)
    }

    private var stripRoom: CGFloat { model.strip == nil ? 0 : stripHeight + stripGap }
    private var stripGap: CGFloat { Self.phone ? 8 : 12 }

    private var target: PlayTargetInfo { model.route.target }

    /// The version playing: a failover's key once the core names it, else the one asked for.
    private var playingKey: String { model.playback?.key ?? model.route.key }

    private var top: some View {
        HStack(alignment: .top, spacing: Theme.Space.s) {
            Button(action: back) {
                Glyph("chevron-left", size: 22)
                    .frame(width: 40, height: 40)
                    .glass(Circle())
                    // A 44 pt target around the 40 pt circle.
                    .frame(width: Theme.minTarget, height: Theme.minTarget)
            }
            .buttonStyle(.round)
            .focused($focus, equals: .back)
            .accessibilityLabel("Back")
            VStack(alignment: .leading, spacing: 2) {
                Text(target.title)
                    .display(metrics.wide ? Theme.type.title1 : Theme.type.title2)
                if let season = target.season, let episode = target.episode {
                    Text("S\(season):E\(episode)")
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(.white.opacity(0.75))
                        .accessibilityLabel(spokenEpisode(season: season, episode: episode))
                }
            }
            Spacer()
            // Right: Picture in Picture, AirPlay when the stream allows it, then the end time.
            #if os(iOS)
            // The system's own glyphs: AirPlay's picker and Picture in Picture's.
            // AirPlay only for a stream a receiver can reach: for the core's loopback
            // one it would carry the sound alone (Control Center mirroring stays).
            if let pip, pip.possible {
                Button(action: pip.start) {
                    Image(uiImage: AVPictureInPictureController.pictureInPictureButtonStartImage)
                        .renderingMode(.template)
                        .foregroundStyle(.white)
                        .frame(width: 40, height: 40)
                        .glass(Circle())
                        .frame(width: Theme.minTarget, height: Theme.minTarget)
                }
                .buttonStyle(.round)
                .accessibilityLabel("Picture in Picture")
            }
            if model.airPlayable {
                AirPlayButton()
                    .frame(width: Theme.minTarget, height: Theme.minTarget)
                    // The picker view takes touches only inside its own bounds: the frame is the target, the circle 40 pt.
                    .background { Color.clear.frame(width: 40, height: 40).glass(Circle()) }
                    .accessibilityLabel("AirPlay")
            }
            #endif
            if model.duration > 0, model.playback?.phase != "failed" {
                Text("Ends at \(endsAt.formatted(date: .omitted, time: .shortened))")
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(.white.opacity(0.8))
                    .frame(minHeight: 40)
            }
        }
        .onGeometryChange(for: CGFloat.self) { $0.frame(in: .global).maxY } action: { measuredTop($0) }
    }

    /// The film is starting or buffering (the status overlay's waiting).
    private var waitingNow: Bool {
        let status = PlayerLayout.status(
            phase: model.playback?.phase, cannotOpen: model.openError != nil,
            firstFrameShown: model.firstFrameMs != nil, waiting: model.waiting)
        if case .waiting = status { return true }
        return false
    }

    private var endsAt: Date {
        .now.addingTimeInterval(max(0, model.duration - model.position) / Double(max(model.rate, 0.25)))
    }

    /// Mac, iPad and TV: the scrubber's row, then the control row (D16).
    private var bottom: some View {
        VStack(spacing: 8) {
            #if os(tvOS)
            // The whole row takes focus, so nothing lies left or right of it:
            // Left and Right seek 10 s a press instead of moving focus.
            HStack(spacing: 12) {
                Text(timecode(model.position)).monospacedDigit()
                ProgressLine(value: model.duration > 0 ? model.position / model.duration : 0)
                Text("-" + timecode(max(0, model.duration - model.position))).monospacedDigit()
            }
            .textFont(Theme.type.callout, relativeTo: .callout)
            .padding(.vertical, 10)
            .padding(.horizontal, 14)
            .contentShape(Capsule())
            .focusable()
            .focused($focus, equals: .scrub)
            .modifier(FocusRing(shape: Capsule(), active: focus == .scrub))
            // Left and Right seek, faster while pressed again quickly; Up and
            // Down leave the bar for Back and Play by name, not by geometry.
            .onMoveCommand { direction in
                switch direction {
                case .left: seek(-1)
                case .right: seek(1)
                case .up: focus = .back
                case .down: focus = .play
                @unknown default: break
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Position")
            .accessibilityValue("\(timecode(model.position)) of \(timecode(model.duration))")
            .accessibilityAdjustableAction { direction in
                switch direction {
                case .increment: model.skip(10)
                case .decrement: model.skip(-10)
                @unknown default: break
                }
            }
            // tvOS keeps its row as it was (the owner, 2026-10-21: tvOS only has to keep compiling): the transport at
            // its TV size in the middle, the four actions as words with their values on the right.
            HStack(alignment: .center) {
                Spacer().frame(maxWidth: .infinity)
                transport(gap: 28, skip: 56, play: 68)
                tvActions.frame(maxWidth: .infinity)
            }
            #else
            scrubRow(spacing: 12)
                .textFont(Theme.type.callout, relativeTo: .callout)
            controlRows(BarFit.desktop(width: barWidth, left: leftPills, right: rightPills, type: wideType))
            #endif
        }
        .onGeometryChange(for: CGSize.self) { $0.size } action: { measuredBar($0) }
        .overlay(alignment: model.strip == .audio ? .bottomLeading : .bottomTrailing) { strip(chevrons: false) }
    }

    /// The control row as the plan lays it: beside the transport, or a second row under it (D16 c).
    @ViewBuilder private func controlRows(_ plan: BarFit.Desktop) -> some View {
        let left = leftGroup(words: plan.leftWords)
        let right = rightGroup(words: plan.rightWords, chevrons: plan.chevrons)
        switch plan.under {
        case .none:
            HStack(spacing: BarFit.columnGap) {
                left.frame(maxWidth: .infinity, alignment: .leading)
                transport
                right.frame(maxWidth: .infinity, alignment: .trailing)
            }
        case .right:
            VStack(spacing: 8) {
                HStack(spacing: BarFit.columnGap) {
                    left.frame(maxWidth: .infinity, alignment: .leading)
                    transport
                    Color.clear.frame(maxWidth: .infinity, maxHeight: 1)
                }
                right
            }
        case .both:
            VStack(spacing: 8) {
                transport
                // Narrower than even the icons (a compact iPad window): the row scrolls, its targets whole.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: BarFit.columnGap) { left; Spacer(minLength: 0); right }
                    ScrollView(.horizontal) { HStack(spacing: BarFit.columnGap) { left; right }.padding(.vertical, 4) }
                        .scrollIndicators(.hidden)
                }
            }
        }
    }

    /// An open strip over the bar's right end (Subtitles) or left end (Audio), `stripGap` over it.
    @ViewBuilder private func strip(chevrons: Bool) -> some View {
        #if !os(tvOS)
        Group {
            switch model.strip {
            case .subtitles:
                SubtitleStrip(model: model, phone: Self.phone, upright: Self.phone && !landscape, chevrons: chevrons,
                              moreLanguages: moreLanguages, close: closeStrip, holding: hold)
            case .audio:
                AudioStrip(model: model, chooseVersion: chooseVersion, close: closeStrip)
            case nil:
                EmptyView()
            }
        }
        .frame(maxWidth: max(barWidth, 0), alignment: model.strip == .audio ? .leading : .trailing)
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { stripHeight = $0 }
        .padding(.bottom, barHeight + stripGap)
        .transition(.opacity)
        #endif
    }

    private var footPadding: CGFloat { metrics.wide ? Theme.Space.m : Theme.Space.s }

    /// The iPhone, as the system player: the top bar under the safe area's top, the transport in the screen's middle
    /// (stepping aside while a strip is open, so the strip has the room), the bar just over the safe area's foot.
    private var phoneLayout: some View {
        ZStack {
            PhoneOSD { top } bottom: { phoneBottom }
                .padding(.leading, PhoneFrame.side(safe: safe.leading))
                .padding(.trailing, PhoneFrame.side(safe: safe.trailing))
            if model.strip == nil {
                transport(gap: PhoneFrame.transportGap(landscape: landscape), skip: 50, play: 68)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .ignoresSafeArea()
            }
        }
        .onGeometryChange(for: EdgeInsets.self) { $0.safeAreaInsets } action: { safe = $0 }
    }

    private var phoneType: BarFit.TextSizes { BarFit.TextSizes(word: phoneWord, value: phoneTime) }
    private var wideType: BarFit.TextSizes { BarFit.TextSizes(word: wideText, value: wideText) }

    /// Across: one row (D17), or the scrubber's row over the controls' when the scrubber would be too short.
    /// Upright: three rows (D15). Every pill an icon at 44 pt.
    @ViewBuilder private var phoneBottom: some View {
        Group {
            if !landscape {
                let chevrons = BarFit.uprightChevrons(width: barWidth, type: phoneType)
                VStack(spacing: 6) {
                    phoneScrubber
                    HStack(spacing: BarFit.phoneGap) {
                        leftGroup(words: false)
                        Spacer(minLength: BarFit.phoneGap)
                        episodesAndNext(words: false)
                    }
                    HStack(spacing: BarFit.phoneGap) {
                        Spacer(minLength: 0)
                        subtitlesPill(words: false)
                        shift(chevrons: chevrons)
                    }
                }
            } else {
                let plan = BarFit.phone(
                    width: barWidth, left: 3, right: target.season == nil ? 1 : 3,
                    times: BarFit.phoneTimes(type: phoneType) + 2 * PhoneFrame.barInset + 16, type: phoneType)
                if plan.twoRows {
                    VStack(spacing: 6) {
                        phoneScrubber
                        HStack(spacing: BarFit.phoneGap) {
                            leftGroup(words: false)
                            Spacer(minLength: BarFit.phoneRowGap)
                            rightGroup(words: false, chevrons: plan.chevrons)
                        }
                    }
                } else {
                    HStack(spacing: BarFit.phoneRowGap) {
                        leftGroup(words: false)
                        phoneScrubber.frame(maxWidth: .infinity)
                        rightGroup(words: false, chevrons: plan.chevrons)
                    }
                }
            }
        }
        .onGeometryChange(for: CGSize.self) { $0.size } action: { measuredBar($0) }
        .overlay(alignment: model.strip == .audio ? .bottomLeading : .bottomTrailing) {
            strip(chevrons: landscape ? true : BarFit.uprightChevrons(width: barWidth, type: phoneType))
        }
    }

    /// The scrubber in its glass capsule, the times inside it.
    @ViewBuilder private var phoneScrubber: some View {
        #if !os(tvOS)
        scrubRow(spacing: 8)
            .textFont(Theme.type.caption, relativeTo: .caption)
            .padding(.horizontal, PhoneFrame.barInset)
            .frame(height: PhoneFrame.barHeight)
            .glass(Capsule())
        #endif
    }

    #if !os(tvOS)
    /// Elapsed, the slider, remaining.
    private func scrubRow(spacing: CGFloat) -> some View {
        HStack(spacing: spacing) {
            Text(timecode(scrub ?? model.position)).monospacedDigit()
            Slider(
                value: Binding(get: { scrub ?? model.position }, set: { scrub = $0 }),
                in: 0...max(model.duration, 1)
            ) { editing in
                // A drag on the bar holds the controls up, and their hide timer starts again from its end, as on
                // Android: before, the 3 s from the tap that showed them ran out under the finger (bench PGW10).
                hold(editing)
                if !editing, let value = scrub {
                    model.seek(to: value)
                    scrub = nil
                }
            }
            .tint(.white)
            .accessibilityLabel("Position")
            // As on TV: "12:04 of 45:10", not raw seconds.
            .accessibilityValue("\(timecode(scrub ?? model.position)) of \(timecode(model.duration))")
            Text("-" + timecode(max(0, model.duration - (scrub ?? model.position)))).monospacedDigit()
        }
    }
    #endif

    /// One press on the TV bar: 10 s, more while the same direction comes again within a second.
    private func seek(_ direction: Int) {
        let now = ContinuousClock.now
        let continuing = lastSeek.map { $0.direction == direction && now - $0.at < SeekRamp.window } ?? false
        seekRun = SeekRamp.run(after: seekRun, continuing: continuing)
        lastSeek = (direction, now)
        model.skip(Double(direction) * SeekRamp.step(seekRun))
        activity()
    }

    private var transport: some View { transport(gap: BarFit.transportGap, skip: BarFit.skip, play: BarFit.play) }

    private func transport(gap: CGFloat, skip: CGFloat, play: CGFloat) -> some View {
        // Play/Pause is the white circle; only focus draws a ring.
        HStack(spacing: gap) {
            Button { model.skip(-10) } label: { SkipGlyph(name: "rotate-ccw", size: skip) }
                .focused($focus, equals: .rewind)
                .accessibilityLabel("Back 10 seconds")
            Button(action: model.togglePlay) {
                Group {
                    // The iPhone: the waiting spinner turns in the circle, as the system player's does.
                    if Self.phone, waitingNow {
                        ProgressView().tint(Theme.Palette.onPrimary)
                    } else {
                        Glyph(model.playing ? "pause-fill" : "play-fill", size: play * 0.38)
                    }
                }
                .foregroundStyle(Theme.Palette.onPrimary)
                .frame(width: play, height: play)
                .background(Circle().fill(Theme.Palette.primary))
            }
            .focused($focus, equals: .play)
            .accessibilityLabel(model.playing ? "Pause" : "Play")
            Button { model.skip(10) } label: { SkipGlyph(name: "rotate-cw", size: skip) }
                .focused($focus, equals: .forward)
                .accessibilityLabel("Forward 10 seconds")
        }
        .buttonStyle(.round)
    }

    // MARK: the groups

    /// The pills as the fit rule measures them: every one the bar may show, Versions and Next included before they
    /// are known, and Subtitles' value with its subtitle off too, so the layout never jumps (D16).
    private var leftPills: [BarFit.Pill] {
        [BarFit.Pill(word: names.audio, value: audioValue), BarFit.Pill(word: names.speed, value: speedText(model.rate)),
         BarFit.Pill(word: names.versions, value: versions?.snapshot.map(currentQuality) ?? "Picked")]
    }

    private var rightPills: [BarFit.Pill] {
        let off = PlayerLayout.subtitleValue(
            picked: nil, found: (model.playback?.subtitles.count ?? 0) + model.textTracks.count,
            pending: model.playback?.subtitlesPending == true, ready: model.playback?.phase == "ready").text
        let listed = TrackPicker.languages(model.playback?.subtitles ?? [], file: model.fileTextTracks).map(\.name)
        let reserve = BarFit.subtitleReserve(
            off: off, languages: listed, fileTracks: model.textTracks, second: model.secondSubtitle.map(SubtitleMenu.heading))
        var pills = [BarFit.Pill(word: names.subtitles, value: subtitleValue.text, reserve: reserve, mark: true)]
        if target.season != nil { pills += [BarFit.Pill(word: names.episodes), BarFit.Pill(word: bar.next)] }
        #if os(macOS)
        pills.append(BarFit.Pill(word: nil))
        #endif
        return pills
    }

    /// Audio, Speed, Versions.
    private func leftGroup(words: Bool) -> some View {
        HStack(spacing: Self.phone ? BarFit.phoneGap : BarFit.gap) {
            Button { toggle(.audio) } label: { action(names.audio, "audio-lines", audioValue, words: words) }
                .focused($focus, equals: .audio)
                .accessibilityAddTraits(model.strip == .audio ? .isSelected : [])
            menu(names.speed, "gauge", speedText(model.rate), words: words) {
                ForEach([0.5, 0.75, 1, 1.25, 1.5, 2] as [Float], id: \.self) { speed in
                    Button { model.rate = speed; hold(false) } label: {
                        Choice(text: speedText(speed), selected: model.rate == speed)
                    }
                }
            }
            .focused($focus, equals: .speed)
            if let versions, let snapshot = versions.snapshot {
                menu(names.versions, "layers", currentQuality(snapshot), words: words) {
                    ForEach([snapshot.recommended].compactMap { $0 } + snapshot.rows.prefix(12)) { row in
                        Button { model.switchVersion(row.key); hold(false) } label: {
                            Choice(text: VersionWords.rowSpoken(row), selected: row.key == playingKey)
                        }
                    }
                }
                .focused($focus, equals: .versions)
            }
        }
        .menuStyle(.button)
        .buttonStyle(FocusPlainStyle(shape: Capsule()))
        .fixedSize()
    }

    /// Subtitles, the shift, Episodes, Next, Full screen.
    private func rightGroup(words: Bool, chevrons: Bool) -> some View {
        HStack(spacing: Self.phone ? BarFit.phoneGap : BarFit.gap) {
            subtitlesPill(words: words)
            shift(chevrons: chevrons)
            episodesAndNext(words: words)
            #if os(macOS)
            Button { (window ?? NSApp.keyWindow)?.toggleFullScreen(nil) } label: {
                action(fullScreen ? names.exitFullScreen : names.fullScreen, fullScreen ? "minimize" : "maximize", nil, words: false)
            }
            .focused($focus, equals: .fullScreen)
            .buttonStyle(FocusPlainStyle(shape: Capsule()))
            // The player's own window only: another window (Settings) going full screen changes nothing here.
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didEnterFullScreenNotification)) { note in
                if let window, note.object as? NSWindow === window { fullScreen = true }
            }
            .onReceive(NotificationCenter.default.publisher(for: NSWindow.didExitFullScreenNotification)) { note in
                if let window, note.object as? NSWindow === window { fullScreen = false }
            }
            .onAppear {
                window = NSApp.keyWindow
                fullScreen = window?.styleMask.contains(.fullScreen) == true
            }
            #endif
        }
        .fixedSize()
    }

    private func subtitlesPill(words: Bool) -> some View {
        Button { toggle(.subtitles) } label: {
            action(names.subtitles, "captions", subtitleValue.text, words: words, searching: subtitleValue.searching)
        }
        .focused($focus, equals: .subtitles)
        .buttonStyle(FocusPlainStyle(shape: Capsule()))
        .accessibilityAddTraits(model.strip == .subtitles ? .isSelected : [])
    }

    /// The quick shift beside Subtitles, only while the subtitle on can be timed (D4).
    @ViewBuilder private func shift(chevrons: Bool) -> some View {
        #if !os(tvOS)
        if BarRules.shiftShown(timingKey: model.timingKey), let key = model.timingKey {
            QuickShift(value: model.subtitleDelay, track: key, chevrons: chevrons, textSize: Self.phone ? 14 : Theme.type.callout, move: { by, spoken in
                model.shiftSubtitles(by: by, spoken: spoken)
                activity()
            }, holding: hold)
        }
        #endif
    }

    /// Episodes for a series, Next while the core names a next episode (D14).
    @ViewBuilder private func episodesAndNext(words: Bool) -> some View {
        let shows = BarRules.shows(season: target.season, next: model.next)
        if shows.episodes, let episodes {
            Button(action: episodes) { action(names.episodes, "list-video", nil, words: words) }
                .focused($focus, equals: .episodes)
                .buttonStyle(FocusPlainStyle(shape: Capsule()))
        }
        if shows.next, let next = model.next {
            Button { Task { await model.playNext() } } label: { action(bar.next, "skip-forward", nil, words: words) }
                .focused($focus, equals: .next)
                .buttonStyle(FocusPlainStyle(shape: Capsule()))
                .accessibilityLabel(BarRules.nextName(next))
        }
    }

    #if os(tvOS)
    /// tvOS: Audio, Subtitles, Versions and Speed as before the bar, each its word over its value; Audio and
    /// Subtitles open the full pickers.
    private var tvActions: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 26) {
                Button { openPicker(.audio) } label: { tvAction(names.audio, "audio-lines", audioValue) }
                    .focused($focus, equals: .audio)
                Button { openPicker(.subtitles) } label: { tvAction(names.subtitles, "captions", subtitleValue.text) }
                    .focused($focus, equals: .subtitles)
                if let versions, let snapshot = versions.snapshot {
                    Menu {
                        ForEach([snapshot.recommended].compactMap { $0 } + snapshot.rows.prefix(12)) { row in
                            Button { model.switchVersion(row.key) } label: {
                                Choice(text: VersionWords.rowSpoken(row), selected: row.key == playingKey)
                            }
                        }
                    } label: { tvAction(names.versions, "layers", currentQuality(snapshot)) }
                    .focused($focus, equals: .versions)
                }
                Menu {
                    ForEach([0.5, 0.75, 1, 1.25, 1.5, 2] as [Float], id: \.self) { speed in
                        Button { model.rate = speed } label: { Choice(text: speedText(speed), selected: model.rate == speed) }
                    }
                } label: { tvAction(names.speed, "gauge", speedText(model.rate)) }
                .focused($focus, equals: .speed)
            }
            .menuStyle(.button)
            .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)))
            .padding(14)
        }
        .scrollIndicators(.hidden)
    }

    private func tvAction(_ title: String, _ symbol: String, _ value: String) -> some View {
        VStack(spacing: 3) {
            Glyph(symbol, size: 18)
            Text(title).textFont(Theme.type.caption + 1, weight: .semibold, relativeTo: .caption)
            Text(value).textFont(Theme.type.caption, relativeTo: .caption).lineLimit(1)
                .foregroundStyle(.white.opacity(0.75))
                .frame(maxWidth: 220)
        }
        .foregroundStyle(.white)
        .accessibilityElement(children: .combine)
    }
    #endif

    /// An OSD menu that keeps the controls up while it is open, so they never
    /// hide (and close it) under the viewer's finger. Each pick ends the hold;
    /// iOS never calls the content's `onDisappear`, so a tap outside ends it
    /// at the next touch or at `PlayerLayout.menuHoldLimit`.
    private func menu<Items: View>(_ title: String, _ symbol: String, _ value: String, words: Bool, @ViewBuilder items: () -> Items) -> some View {
        Menu {
            Group { items() }
                .onAppear { hold(true) }
                .onDisappear { hold(false) }
        } label: {
            action(title, symbol, value, words: words)
        }
    }

    /// A bar pill: the icon, then (`words`) the word and the value, capped so a long track name truncates. Without
    /// words it is the icon alone, the word and value said to VoiceOver. Its size is `BarFit.pillWidth`'s.
    private func action(_ title: String, _ symbol: String, _ value: String?, words: Bool, searching: Bool = false) -> some View {
        HStack(spacing: BarFit.inner) {
            Glyph(symbol, size: BarFit.icon)
            if words {
                Text(title).textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout).fixedSize()
                if let value {
                    Text(value).textFont(Theme.type.callout, relativeTo: .callout)
                        .lineLimit(1)
                        .foregroundStyle(.white.opacity(0.75))
                        .frame(maxWidth: BarFit.valueCap)
                }
            }
            // More may come (the core is still asking subtitle addons): seen on an icon alone too.
            if searching {
                Glyph("loader-circle", size: BarFit.markSize).foregroundStyle(.white.opacity(0.75))
            }
        }
        .padding(.leading, words ? BarFit.lead : 0)
        .padding(.trailing, words ? BarFit.trail : 0)
        .frame(minWidth: Theme.minTarget, minHeight: Theme.minTarget)
        .background(Capsule().fill(.white.opacity(0.16)))
        .contentShape(Capsule())
        .foregroundStyle(.white)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(title)
        // More may come: the core is still asking subtitle addons.
        .accessibilityValue([value, searching ? "still searching" : nil].compactMap { $0 }.joined(separator: ", "))
    }

    /// The track playing, by name; "Default" when the file names none.
    private var audioValue: String {
        model.audioTrack.flatMap { model.audioTracks.indices.contains($0) ? model.audioTracks[$0] : nil } ?? "Default"
    }

    /// The language on, "English, Auto" when the core turned it on by itself.
    private var subtitleValue: (text: String, searching: Bool) {
        let file = model.textTrack.flatMap { model.textTracks.indices.contains($0) ? model.textTracks[$0] : nil }
        return PlayerLayout.subtitleValue(
            picked: TrackPicker.buttonValue(
                core: model.subtitle.map(SubtitleMenu.heading), file: file, auto: model.subtitleIsAuto,
                autoLanguage: model.playback?.subtitles.first { $0.url == model.playback?.autoSubtitle }.map(SubtitleMenu.heading),
                second: model.secondOn ? model.secondSubtitle.map(SubtitleMenu.heading) : nil),
            found: (model.playback?.subtitles.count ?? 0) + model.textTracks.count,
            pending: model.playback?.subtitlesPending == true,
            ready: model.playback?.phase == "ready"
        )
    }

    private func currentQuality(_ snapshot: VersionsSnapshot) -> String {
        let rows = [snapshot.recommended].compactMap { $0 } + snapshot.rows
        guard let row = rows.first(where: { $0.key == playingKey }) else { return "Picked" }
        return VersionWords.cell(row.quality)
    }

    private func speedText(_ speed: Float) -> String {
        speed == speed.rounded() ? "\(Int(speed))x" : "\(speed)x"
    }

    private func timecode(_ seconds: Double) -> String {
        clock(ms: Int64(max(0, seconds.isFinite ? seconds : 0) * 1000))
    }
}

/// The skip arrow with its interval inside, as the audit asks ("10"), in a
/// round glass button.
private struct SkipGlyph: View {
    let name: String
    let size: CGFloat

    var body: some View {
        Glyph(name, size: size * 0.46)
            .overlay {
                Text("10").textFont(9, weight: .bold).monospacedDigit().offset(y: 1)
            }
            .frame(width: size, height: size)
            .glass(Circle())
    }
}

/// A menu entry that shows a checkmark when it is the current choice.
private struct Choice: View {
    let text: String
    let selected: Bool

    var body: some View {
        if selected {
            Label { Text(text) } icon: { Image("check") }
        } else {
            Text(text)
        }
    }
}

#if !os(tvOS)
/// The subtitle Timing row, as Stremio's stepper (`PlayerLayout`, the web's delay-stepper.tsx): minus,
/// the value, plus, and Reset while it is not 0; a click on the value resets too. A press is 0.1 s;
/// held, it repeats and speeds up. On the Mac the value takes the focus (no Full Keyboard Access
/// needed) and Left and Right held move it; minus and plus take Space and Return held with Full
/// Keyboard Access. VoiceOver adjusts the value a step at a time. A hold ends on its release and
/// whenever it is cut off: a cancelled touch, focus leaving, the app going inactive, another track.
struct SubtitleTimingStepper: View {
    let value: Double
    /// The track the value is for: a new one ends a hold under way.
    let track: String
    /// The Mac: the value takes the keyboard focus as it shows. False inside the picker, whose rows keep the
    /// arrows; there it takes its keys only once focused on purpose (a click, Tab).
    var takesFocus = true
    /// A step by the signed amount; `spoken` false for VoiceOver's own adjust, which it reads itself.
    let move: (Double, _ spoken: Bool) -> Void
    let reset: () -> Void
    @State private var hold: TimingHold?
    @State private var pressing = 0
    /// One per button: a finger lifted from one never ends the other's hold.
    @GestureState private var touchingMinus = false
    @GestureState private var touchingPlus = false
    @FocusState private var focus: Part?
    /// The part a key hold started on: focus leaving it ends the hold (a click moves no focus).
    @State private var keyPart: Part?
    @Environment(\.scenePhase) private var scenePhase

    enum Part: Hashable { case minus, value, plus }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Timing").textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
                Spacer(minLength: 16)
                // By the label, so minus and plus stay put while it comes and goes.
                if value != 0 {
                    Button("Reset", action: reset)
                        .buttonStyle(.plain)
                        .textFont(Theme.type.callout, relativeTo: .callout)
                        .foregroundStyle(Theme.Palette.key)
                }
            }
            HStack(spacing: 12) {
                step(-1, glyph: "minus", label: "Minus 0.1 seconds", part: .minus)
                valueView
                step(1, glyph: "plus", label: "Plus 0.1 seconds", part: .plus)
            }
        }
        .foregroundStyle(Theme.Palette.text)
        .frame(minWidth: 240)
        .onAppear {
            hold = TimingHold { move($0, true) }
            #if os(macOS)
            // The keyboard's way in; a touch screen draws no focus.
            if takesFocus { focus = .value }
            #endif
        }
        .onDisappear { stop(single: false) }
        .onChange(of: track) { stop(single: false) }
        .onChange(of: scenePhase) { _, phase in if phase != .active { stop(single: false) } }
        .onChange(of: focus) { old, _ in if let keyPart, keyPart == old { stop(single: false) } }
        // A touch the system took back (a banner, Control Center) resets the gesture with no end.
        .onChange(of: touchingMinus) { _, now in if !now { touchEnded(-1) } }
        .onChange(of: touchingPlus) { _, now in if !now { touchEnded(1) } }
    }

    /// The value: a click resets; focused, Left and Right held move it by the stepper's rules, and
    /// Space or Return resets. A key with Command, Option, Control or Shift is not the stepper's.
    private var valueView: some View {
        Text(PlayerLayout.delayText(value))
            .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
            .monospacedDigit()
            .frame(minWidth: 88, minHeight: 36)
            .background(.white.opacity(focus == .value ? 0.12 : 0), in: Capsule())
            .overlay { if focus == .value { Capsule().strokeBorder(Theme.Palette.key, lineWidth: Theme.Focus.ring) } }
            .contentShape(Capsule())
            .onTapGesture(perform: reset)
            .focusable(interactions: .edit)
            .focusEffectDisabled()
            .focused($focus, equals: .value)
            .onKeyPress(keys: [.leftArrow, .rightArrow, .space, .return], phases: [.down, .repeat, .up]) { press in
                guard PlayerLayout.stepperTakes(down: press.phase == .down, modifiers: press.modifiers) else { return .ignored }
                if press.key == .space || press.key == .return {
                    if press.phase == .down { reset() }
                    return .handled
                }
                key(press, direction: press.key == .leftArrow ? -1 : 1, part: .value)
                return .handled
            }
            .accessibilityElement()
            .accessibilityLabel("Subtitle timing")
            .accessibilityValue(PlayerLayout.delayWords(value))
            .accessibilityHint(value == 0 ? "" : "Resets to 0.0 seconds")
            .accessibilityAddTraits(.isButton)
            .accessibilityAction(.default, reset)
            .accessibilityAdjustableAction { direction in
                move(direction == .increment ? PlayerLayout.subtitleStep : -PlayerLayout.subtitleStep, false)
            }
    }

    private func start(_ direction: Int) {
        pressing = direction
        hold?.start(direction)
    }

    /// The gesture's state reset: a hold its own end did not stop was a touch the system took back.
    private func touchEnded(_ direction: Int) {
        Task { @MainActor in if pressing == direction { stop(single: false) } }
    }

    private func stop(single: Bool) {
        (pressing, keyPart) = (0, nil)
        hold?.stop(single: single)
    }

    /// A held key: down starts the hold, the system's own repeats are ignored, up ends it.
    private func key(_ press: KeyPress, direction: Int, part: Part) {
        switch press.phase {
        case .down:
            start(direction)
            keyPart = part
        case .up: if pressing == direction { stop(single: true) }
        default: break
        }
    }

    /// Minus or plus: a press and hold by the stepper's rules, not the system's repeat; with Full
    /// Keyboard Access, Space and Return held; an accessibility activation is one step.
    private func step(_ direction: Int, glyph: String, label: String, part: Part) -> some View {
        Glyph(glyph, size: 18)
            .frame(width: 44, height: 44)
            .background(.white.opacity(pressing == direction ? 0.24 : 0.12), in: Circle())
            .contentShape(Circle())
            .modifier(FocusRing(shape: Circle(), active: focus == part))
            .gesture(
                DragGesture(minimumDistance: 0)
                    .updating(direction < 0 ? $touchingMinus : $touchingPlus) { _, state, _ in state = true }
                    .onChanged { _ in if pressing != direction { start(direction) } }
                    // Only the finger that started this hold ends it (another on the other button stops nothing).
                    .onEnded { _ in if pressing == direction { stop(single: true) } }
            )
            .focusable(interactions: .activate)
            .focusEffectDisabled()
            .focused($focus, equals: part)
            .onKeyPress(keys: [.space, .return], phases: [.down, .repeat, .up]) { press in
                guard PlayerLayout.stepperTakes(down: press.phase == .down, modifiers: press.modifiers) else { return .ignored }
                key(press, direction: direction, part: part)
                return .handled
            }
            .accessibilityElement()
            .accessibilityLabel(label)
            .accessibilityAddTraits(.isButton)
            .accessibilityAction { move(Double(direction) * PlayerLayout.subtitleStep, true) }
    }
}
#endif
