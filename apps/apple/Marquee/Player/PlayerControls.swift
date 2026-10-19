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

    var body: some View {
        VStack(spacing: 0) {
            top
            Spacer()
            bottom
        }
        .onChange(of: focus) { activity() }
        #if os(tvOS)
        .defaultFocus($focus, .play)
        .onAppear { focus = .play }
        #endif
        .padding(.horizontal, metrics.gutter)
        .padding(.vertical, metrics.wide ? Theme.Space.m : Theme.Space.s)
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
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(.white.opacity(0.75))
                        .accessibilityLabel(spokenEpisode(season: season, episode: episode))
                }
                TraktMark(trakt: model.playback?.trakt)
            }
            Spacer()
            if model.duration > 0, model.playback?.phase != "failed" {
                Text("Ends at \(endsAt.formatted(date: .omitted, time: .shortened))")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(.white.opacity(0.8))
            }
            #if os(iOS)
            // The system's own glyphs: AirPlay's picker and Picture in Picture's.
            AirPlayButton()
                .frame(width: 40, height: 40)
                .glass(Circle())
                .accessibilityLabel("AirPlay")
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
            #endif
        }
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
            .font(.text(Theme.type.callout, relativeTo: .callout))
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
            HStack(spacing: 12) {
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
                Text("-" + timecode(max(0, model.duration - (scrub ?? model.position)))).monospacedDigit()
            }
            .font(.text(Theme.type.callout, relativeTo: .callout))
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

    /// One press on the TV bar: 10 s, more while the same direction comes again within a second.
    private func seek(_ direction: Int) {
        let now = ContinuousClock.now
        let continuing = lastSeek.map { $0.direction == direction && now - $0.at < SeekRamp.window } ?? false
        seekRun = SeekRamp.run(after: seekRun, continuing: continuing)
        lastSeek = (direction, now)
        model.skip(Double(direction) * SeekRamp.step(seekRun))
        activity()
    }

    private var transport: some View {
        // Play/Pause is the white circle; only focus draws a ring.
        HStack(spacing: metrics.wide ? 28 : 20) {
            Button { model.skip(-10) } label: { SkipGlyph(name: "rotate-ccw", wide: metrics.wide) }
                .focused($focus, equals: .rewind)
                .accessibilityLabel("Back 10 seconds")
            Button(action: model.togglePlay) {
                Glyph(model.playing ? "pause-fill" : "play-fill", size: 26)
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
    /// fade at the end while more is out of sight.
    private var actions: some View {
        ScrollView(.horizontal) {
            actionRow
                .fixedSize(horizontal: true, vertical: false)
                // Room for a focus ring and the lift, which the scroll view would clip.
                .padding(.vertical, metrics.wide ? 14 : 4)
                .padding(.horizontal, metrics.wide ? 14 : 2)
                .frame(minWidth: actionsWidth, alignment: metrics.wide ? .trailing : .center)
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

    private var actionRow: some View {
        HStack(spacing: metrics.wide ? 26 : 16) {
            menu("Audio", "volume-2", audioValue) {
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
            menu("Subtitles", "captions", subtitleValue) {
                Button { model.selectSubtitle(nil); hold(false) } label: {
                    Choice(text: "Off", selected: model.subtitle == nil && model.textTrack == nil)
                }
                // A submenu per language, the profile's on top (plans/m13-subtitle-languages.md 2).
                let groups = SubtitleMenu.groups(model.playback?.subtitles ?? [], preferred: model.subtitleLanguages)
                ForEach(groups.top, id: \.language) { languageMenu($0) }
                if !groups.other.isEmpty {
                    Menu("Other languages") {
                        ForEach(groups.other, id: \.language) { languageMenu($0) }
                    }
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
                menu("Versions", "layers", currentQuality(snapshot)) {
                    ForEach([snapshot.recommended].compactMap { $0 } + snapshot.rows.prefix(12)) { row in
                        Button { model.switchVersion(row.key); hold(false) } label: {
                            Choice(text: "\(VersionWords.line(row)), \(row.via)", selected: row.key == playingKey)
                        }
                    }
                }
                .focused($focus, equals: .versions)
            }
            menu("Speed", "gauge", speedText(model.rate)) {
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
                Button { model.selectSubtitle(track); hold(false) } label: {
                    Choice(text: SubtitleMenu.title(track), detail: SubtitleMenu.detail(track), selected: model.subtitle == track)
                }
            }
        } label: {
            Choice(text: group.heading, selected: model.subtitle.map(group.tracks.contains) ?? false)
        }
    }

    /// An OSD menu that keeps the controls up while it is open, so they never
    /// hide (and close it) under the viewer's finger. Each pick ends the hold;
    /// iOS never calls the content's `onDisappear`, so a tap outside ends it
    /// at the next touch or at `PlayerLayout.menuHoldLimit`.
    private func menu<Items: View>(_ title: String, _ symbol: String, _ value: String, @ViewBuilder items: () -> Items) -> some View {
        Menu {
            Group { items() }
                .onAppear { hold(true) }
                .onDisappear { hold(false) }
        } label: {
            action(title, symbol, value)
        }
    }

    private func action(_ title: String, _ symbol: String, _ value: String) -> some View {
        VStack(spacing: 3) {
            Glyph(symbol, size: 18)
            Text(title).font(.text(Theme.type.caption + 1, weight: .semibold, relativeTo: .caption))
            // Capped: a long track name ("English Audio Description") must not widen the row without end.
            Text(value).font(.text(Theme.type.caption, relativeTo: .caption)).foregroundStyle(.white.opacity(0.75))
                .lineLimit(1)
                .frame(maxWidth: metrics.wide ? 220 : 120)
        }
        .foregroundStyle(.white)
        .accessibilityElement(children: .combine)
    }

    /// The track playing, by name; "Default" when the file names none.
    private var audioValue: String {
        model.audioTrack.flatMap { model.audioTracks.indices.contains($0) ? model.audioTracks[$0] : nil } ?? "Default"
    }

    private var subtitleValue: String {
        let found = (model.playback?.subtitles.count ?? 0) + model.textTracks.count
        if let track = model.subtitle { return SubtitleMenu.heading(track) }
        if let index = model.textTrack, model.textTracks.indices.contains(index) { return model.textTracks[index] }
        if model.playback?.subtitlesPending == true && found == 0 { return "Looking" }
        return found == 0 ? "None found" : "Off, \(found) found"
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
                Text("10").font(.text(9, weight: .bold)).monospacedDigit().offset(y: 1)
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
