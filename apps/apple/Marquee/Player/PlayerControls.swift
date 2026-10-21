import SwiftUI
#if os(iOS)
import AVKit
#endif

/// Picture in Picture for the OSD: whether the system allows it now, and how to start it.
struct PipControl {
    let possible: Bool
    let start: () -> Void
}

/// The OSD: title top left, end time top right, the scrubber, transport in
/// the middle and the track and version actions on the right, each a word
/// with its current value.
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
    /// The real window's height: below `PlayerLayout.shortHeight` the transport joins the actions.
    var windowHeight: CGFloat = 0
    /// The bottom controls' height with their foot padding, for Skip and Up Next.
    var measured: (CGFloat) -> Void = { _ in }
    @State private var actionsWidth: CGFloat = 0
    @State private var moreAfter = false
    /// TV seeking: how many quick presses in a row, and the last one's direction and time.
    @State private var seekRun = 0
    @State private var lastSeek: (direction: Int, at: ContinuousClock.Instant)?
    @Environment(\.metrics) private var metrics
    @State private var scrub: Double?
    /// Which control has focus: TV enters on Play/Pause, and every move keeps the controls up.
    @FocusState private var focus: Control?

    enum Control: Hashable { case back, rewind, play, forward, scrub, audio, subtitles, versions, speed }

    /// The iPhone lays the OSD out as the system player does (`PhoneFrame`).
    private static var phone: Bool {
        #if os(iOS)
        UIDevice.current.userInterfaceIdiom == .phone
        #else
        false
        #endif
    }

    /// The phone's safe area and turn, read from the layout.
    @State private var safe = EdgeInsets()
    @State private var landscape = false

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
            // Full-bleed scrims, never boxes, so every OSD word holds 4.5:1 on
            // any picture. The phone's bottom one reaches above the scrubber.
            VStack {
                LinearGradient(colors: [.black.opacity(0.6), .clear], startPoint: .top, endPoint: .bottom)
                    .frame(height: metrics.wide ? 200 : 120)
                Spacer()
                LinearGradient(colors: [.clear, .black.opacity(0.72)], startPoint: .top, endPoint: .bottom)
                    .frame(height: metrics.wide ? 320 : 300)
            }
            .ignoresSafeArea()
            .allowsHitTesting(false)
        }
        .foregroundStyle(.white)
    }

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
                    .frame(width: 44, height: 44)
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
                TraktMark(trakt: model.playback?.trakt)
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
                }
                .buttonStyle(.round)
                .accessibilityLabel("Picture in Picture")
            }
            if model.airPlayable {
                AirPlayButton()
                    .frame(width: 40, height: 40)
                    .glass(Circle())
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

    private var bottom: some View {
        VStack(spacing: 14) {
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
            #else
            scrubRow(spacing: 12)
                .textFont(Theme.type.callout, relativeTo: .callout)
            #endif
            if metrics.wide {
                HStack(alignment: .center) {
                    Spacer().frame(maxWidth: .infinity)
                    transport
                    actions.frame(maxWidth: .infinity)
                }
            } else if PlayerLayout.joinsTransport(windowHeight: windowHeight) {
                // A short window (a small phone in landscape): one row, so the
                // controls stay clear of the title and the status line.
                HStack(spacing: 16) {
                    transport
                    actions
                }
            } else {
                transport
                actions.padding(.top, 6)
            }
        }
        // Skip and Up Next sit just over this, the controls' own height and their foot padding.
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { measured($0 + footPadding) }
    }

    private var footPadding: CGFloat { metrics.wide ? Theme.Space.m : Theme.Space.s }

    /// The iPhone, as the system player: the top bar under the safe area's
    /// top, the transport in the screen's middle, the scrubber and the actions
    /// just over the safe area's foot, the bars `PhoneFrame.edge` from the
    /// screen, upright and across alike.
    private var phoneLayout: some View {
        ZStack {
            PhoneOSD { top } bottom: { phoneBottom }
                .padding(.leading, PhoneFrame.side(safe: safe.leading))
                .padding(.trailing, PhoneFrame.side(safe: safe.trailing))
            transport(gap: PhoneFrame.transportGap(landscape: landscape))
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .ignoresSafeArea()
        }
        .onGeometryChange(for: EdgeInsets.self) { $0.safeAreaInsets } action: { safe = $0 }
        .onGeometryChange(for: Bool.self) { $0.size.width > $0.size.height } action: { landscape = $0 }
    }

    /// The scrubber in its bar, the times inside it, then the actions in one
    /// compact row under it.
    private var phoneBottom: some View {
        VStack(spacing: 6) {
            #if !os(tvOS)
            scrubRow(spacing: 8)
                .textFont(Theme.type.caption, relativeTo: .caption)
                .padding(.horizontal, PhoneFrame.barInset)
                .frame(height: PhoneFrame.barHeight)
                .glass(Capsule())
            #endif
            actions
        }
        // Skip, Up Next and the drawn subtitles sit just over this.
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { measured($0 + PhoneFrame.bottomGap) }
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

    private var transport: some View { transport(gap: metrics.wide ? 28 : 20) }

    private func transport(gap: CGFloat) -> some View {
        // Play/Pause is the white circle; only focus draws a ring.
        HStack(spacing: gap) {
            Button { model.skip(-10) } label: { SkipGlyph(name: "rotate-ccw", wide: metrics.wide) }
                .focused($focus, equals: .rewind)
                .accessibilityLabel("Back 10 seconds")
            Button(action: model.togglePlay) {
                Group {
                    // The iPhone: the waiting spinner turns in the circle, as the system player's does.
                    if Self.phone, waitingNow {
                        ProgressView().tint(Theme.Palette.onPrimary)
                    } else {
                        Glyph(model.playing ? "pause-fill" : "play-fill", size: 26)
                    }
                }
                .foregroundStyle(Theme.Palette.onPrimary)
                .frame(width: 68, height: 68)
                .background(Circle().fill(Theme.Palette.primary))
            }
            .focused($focus, equals: .play)
            .accessibilityLabel(model.playing ? "Pause" : "Play")
            Button { model.skip(10) } label: { SkipGlyph(name: "rotate-cw", wide: metrics.wide) }
                .focused($focus, equals: .forward)
                .accessibilityLabel("Forward 10 seconds")
        }
        .buttonStyle(.round)
    }

    /// Audio, Subtitles, Versions, Speed: each a word with its current value.
    /// The actions keep their spread when they fit; past the width (a small
    /// phone, a large text size, long track names) the row scrolls, with a
    /// fade at the end while more is out of sight. On the iPhone a word is
    /// never cut: when the row is short the values go first (Default, 720p,
    /// 1x), then it scrolls (the iPhone walk, round 3: "Vers...", "S...").
    @ViewBuilder private var actions: some View {
        if Self.phone {
            ViewThatFits(in: .horizontal) {
                padded(actionRow(values: true)).fixedSize()
                padded(actionRow(values: false)).fixedSize()
                scrolling(actionRow(values: false))
            }
        } else {
            scrolling(actionRow(values: true))
        }
    }

    /// Room for a focus ring and the lift, which a scroll view would clip.
    private func padded(_ row: some View) -> some View {
        row
            .padding(.vertical, metrics.wide ? 14 : 4)
            .padding(.horizontal, metrics.wide ? 14 : 2)
    }

    private func scrolling(_ row: some View) -> some View {
        ScrollView(.horizontal) {
            padded(row.fixedSize(horizontal: true, vertical: false))
                .frame(minWidth: actionsWidth, alignment: metrics.wide && !Self.phone ? .trailing : .center)
        }
        .scrollIndicators(.hidden)
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { actionsWidth = $0 }
        .onScrollGeometryChange(for: Bool.self) { $0.contentOffset.x + $0.containerSize.width < $0.contentSize.width - 1 } action: { moreAfter = $1 }
        .mask {
            HStack(spacing: 0) {
                Color.black
                LinearGradient(colors: [.black, .clear], startPoint: .leading, endPoint: .trailing)
                    .frame(width: moreAfter ? 32 : 0)
            }
        }
    }

    private func actionRow(values: Bool) -> some View {
        HStack(spacing: metrics.wide ? 26 : 16) {
            menu("Audio", "volume-2", audioValue, values: values) {
                if model.audioTracks.isEmpty {
                    Choice(text: "As the file has it", selected: true)
                }
                ForEach(Array(model.audioTracks.enumerated()), id: \.offset) { index, name in
                    Button { model.selectAudio(index); hold(false) } label: {
                        Choice(text: name, selected: model.audioTrack == index)
                    }
                }
            }
            .focused($focus, equals: .audio)
            menu("Subtitles", "captions", subtitleValue.text, searching: subtitleValue.searching, values: values) {
                Button { model.chooseSubtitle(nil); hold(false) } label: {
                    Choice(text: "Off", selected: model.subtitle == nil && model.textTrack == nil)
                }
                // A submenu per language, the profile's on top (plans/m13-subtitle-languages.md 2).
                let groups = SubtitleMenu.groups(model.playback?.subtitles ?? [], preferred: model.subtitleLanguages)
                ForEach(groups.top, id: \.language) { languageMenu($0) }
                if !groups.other.isEmpty {
                    Menu("Other languages") {
                        ForEach(groups.other, id: \.language) { languageMenu($0) }
                            .onAppear { hold(true) }
                    }
                }
                // An addon track's timing, half a second a press, for this play;
                // the menu stays open for the next press, and each press holds the
                // controls up again: finding the timing takes listening between presses.
                if model.subtitle != nil {
                    Section("Timing: \(PlayerLayout.delayWords(model.subtitleDelay))") {
                        Button { model.subtitleDelay -= PlayerLayout.subtitleStep; hold(true) } label: {
                            Label("Earlier by 0.5 s", glyph: "chevron-left")
                        }
                        .keepsMenuOpen()
                        Button { model.subtitleDelay += PlayerLayout.subtitleStep; hold(true) } label: {
                            Label("Later by 0.5 s", glyph: "chevron-right")
                        }
                        .keepsMenuOpen()
                        if model.subtitleDelay != 0 {
                            Button { model.subtitleDelay = 0; hold(false) } label: { Label("Back on time", glyph: "rotate-ccw") }
                        }
                    }
                }
                // A file's own track is drawn by AVFoundation, which takes the size but no timing.
                if model.subtitle == nil, model.textTrack != nil {
                    Section("Timing") { Text("Timing works for addon subtitles only") }
                }
                if !model.textTracks.isEmpty {
                    Section("In the file") {
                        ForEach(Array(model.textTracks.enumerated()), id: \.offset) { index, name in
                            Button { model.selectText(index); hold(false) } label: {
                                Choice(text: name, selected: model.textTrack == index)
                            }
                        }
                    }
                }
                // Why a source's files did not load, in the core's order, for as long as the core says so.
                if let notes = model.playback?.notes, !notes.isEmpty {
                    Divider()
                    ForEach(notes, id: \.self) { Text($0.words) }
                }
            }
            .focused($focus, equals: .subtitles)
            if let versions, let snapshot = versions.snapshot {
                menu("Versions", "layers", currentQuality(snapshot), values: values) {
                    ForEach([snapshot.recommended].compactMap { $0 } + snapshot.rows.prefix(12)) { row in
                        Button { model.switchVersion(row.key); hold(false) } label: {
                            Choice(text: "\(VersionWords.line(row)), \(row.via)", selected: row.key == playingKey)
                        }
                    }
                }
                .focused($focus, equals: .versions)
            }
            menu("Speed", "gauge", speedText(model.rate), values: values) {
                ForEach([0.5, 0.75, 1, 1.25, 1.5, 2] as [Float], id: \.self) { speed in
                    Button { model.rate = speed; hold(false) } label: {
                        Choice(text: speedText(speed), selected: model.rate == speed)
                    }
                }
            }
            .focused($focus, equals: .speed)
        }
        .menuStyle(.button)
        .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)))
    }

    /// One language's tracks, each by its name with its source, Forced and SDH beneath.
    private func languageMenu(_ group: SubtitleMenu.Group) -> some View {
        Menu {
            ForEach(group.tracks) { track in
                Button { model.chooseSubtitle(track); hold(false) } label: {
                    Choice(text: SubtitleMenu.title(track), detail: SubtitleMenu.detail(track), selected: model.subtitle == track)
                }
            }
            // A submenu opened is the viewer still choosing: the hold starts again.
            .onAppear { hold(true) }
        } label: {
            Choice(text: group.heading, selected: model.subtitle.map(group.tracks.contains) ?? false)
        }
    }

    /// An OSD menu that keeps the controls up while it is open, so they never
    /// hide (and close it) under the viewer's finger. Each pick ends the hold;
    /// iOS never calls the content's `onDisappear`, so a tap outside ends it
    /// at the next touch or at `PlayerLayout.menuHoldLimit`.
    private func menu<Items: View>(_ title: String, _ symbol: String, _ value: String, searching: Bool = false, values: Bool = true, @ViewBuilder items: () -> Items) -> some View {
        Menu {
            Group { items() }
                .onAppear { hold(true) }
                .onDisappear { hold(false) }
        } label: {
            action(title, symbol, value, searching: searching, shown: values)
        }
    }

    /// `shown` false: the value is left out of sight and said with the word.
    private func action(_ title: String, _ symbol: String, _ value: String, searching: Bool = false, shown: Bool = true) -> some View {
        let current = HStack(spacing: 4) {
            // Capped: a long track name ("English Audio Description") must not widen the row without end.
            Text(value).textFont(Theme.type.caption, relativeTo: .caption)
                .lineLimit(1)
            // More may come: the core is still asking subtitle addons.
            if searching {
                Glyph("loader-circle", size: Theme.type.caption, relativeTo: .caption)
                    .accessibilityLabel("still searching")
            }
        }
        .foregroundStyle(.white.opacity(0.75))
        .frame(maxWidth: metrics.wide ? 220 : 120)
        return Group {
            if Self.phone {
                // One compact line under the scrubber: icon, word, value.
                HStack(spacing: 6) {
                    Glyph(symbol, size: 16)
                    Text(title).textFont(Theme.type.caption + 1, weight: .semibold, relativeTo: .caption)
                        .fixedSize()
                    if shown { current }
                }
                .padding(.vertical, 6)
                .accessibilityValue(shown ? "" : value)
            } else {
                VStack(spacing: 3) {
                    Glyph(symbol, size: 18)
                    Text(title).textFont(Theme.type.caption + 1, weight: .semibold, relativeTo: .caption)
                    current
                }
            }
        }
        .foregroundStyle(.white)
        .accessibilityElement(children: .combine)
    }

    /// The track playing, by name; "Default" when the file names none.
    private var audioValue: String {
        model.audioTrack.flatMap { model.audioTracks.indices.contains($0) ? model.audioTracks[$0] : nil } ?? "Default"
    }

    private var subtitleValue: (text: String, searching: Bool) {
        let file = model.textTrack.flatMap { model.textTracks.indices.contains($0) ? model.textTracks[$0] : nil }
        return PlayerLayout.subtitleValue(
            picked: model.subtitle.map(SubtitleMenu.heading) ?? file,
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
    let wide: Bool

    var body: some View {
        Glyph(name, size: 26)
            .overlay {
                Text("10").textFont(9, weight: .bold).monospacedDigit().offset(y: 1)
            }
            .frame(width: wide ? 56 : 50, height: wide ? 56 : 50)
            .glass(Circle())
    }
}

/// A menu entry that shows a checkmark when it is the current choice.
private struct Choice: View {
    let text: String
    /// A menu shows it as the item's second line.
    var detail: String?
    let selected: Bool

    var body: some View {
        if selected {
            Label {
                Text(text)
                if let detail { Text(detail) }
            } icon: {
                Image("check")
            }
        } else {
            Text(text)
            if let detail { Text(detail) }
        }
    }
}

private extension View {
    /// A menu item that leaves its menu open for the next press, where the
    /// platform can (a Mac menu closes on every pick).
    @ViewBuilder func keepsMenuOpen() -> some View {
        #if os(macOS)
        self
        #else
        menuActionDismissBehavior(.disabled)
        #endif
    }
}

