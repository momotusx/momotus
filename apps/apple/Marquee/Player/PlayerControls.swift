import SwiftUI

/// The OSD: title top left, end time top right, the scrubber, transport in
/// the middle and the track and version actions on the right, each a word
/// with its current value.
struct PlayerControls: View {
    let model: PlayerModel
    let versions: VersionsModel?
    let back: () -> Void
    /// True while a menu is open: the controls must stay up.
    var hold: (Bool) -> Void = { _ in }
    @Environment(\.metrics) private var metrics
    @State private var scrub: Double?

    var body: some View {
        VStack(spacing: 0) {
            top
            Spacer()
            bottom
        }
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
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Back")
            VStack(alignment: .leading, spacing: 2) {
                Text(target.title)
                    .display(metrics.wide ? Theme.type.title1 : Theme.type.title2)
                if let season = target.season, let episode = target.episode {
                    Text("S\(season):E\(episode)")
                        .font(.text(Theme.type.callout, relativeTo: .callout))
                        .foregroundStyle(.white.opacity(0.75))
                }
            }
            Spacer()
            if model.duration > 0 {
                Text("Ends at \(endsAt.formatted(date: .omitted, time: .shortened))")
                    .font(.text(Theme.type.callout, relativeTo: .callout))
                    .foregroundStyle(.white.opacity(0.8))
            }
        }
    }

    private var endsAt: Date {
        .now.addingTimeInterval(max(0, model.duration - model.position) / Double(max(model.rate, 0.25)))
    }

    private var bottom: some View {
        VStack(spacing: 14) {
            HStack(spacing: 12) {
                Text(timecode(scrub ?? model.position)).monospacedDigit()
                #if os(tvOS)
                ProgressLine(value: model.duration > 0 ? model.position / model.duration : 0)
                #else
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
                #endif
                Text("-" + timecode(max(0, model.duration - (scrub ?? model.position)))).monospacedDigit()
            }
            .font(.text(Theme.type.callout, relativeTo: .callout))
            if metrics.wide {
                HStack(alignment: .center) {
                    Spacer().frame(maxWidth: .infinity)
                    transport
                    HStack {
                        Spacer()
                        actions
                    }
                    .frame(maxWidth: .infinity)
                }
            } else {
                transport
                actions.padding(.top, 6)
            }
        }
    }

    private var transport: some View {
        HStack(spacing: metrics.wide ? 28 : 20) {
            Button { model.skip(-10) } label: { SkipGlyph(name: "rotate-ccw", wide: metrics.wide) }
            .accessibilityLabel("Back 10 seconds")
            Button(action: model.togglePlay) {
                Glyph(model.playing ? "pause-fill" : "play-fill", size: 26)
                    .foregroundStyle(Theme.Palette.onPrimary)
                    .frame(width: 68, height: 68)
                    .background(Circle().fill(Theme.Palette.primary))
                    .overlay(Circle().strokeBorder(Theme.Palette.accent, lineWidth: 3).padding(-5))
            }
            .accessibilityLabel(model.playing ? "Pause" : "Play")
            Button { model.skip(10) } label: { SkipGlyph(name: "rotate-cw", wide: metrics.wide) }
            .accessibilityLabel("Forward 10 seconds")
        }
        .buttonStyle(.plain)
    }

    /// Audio, Subtitles, Versions, Speed: each a word with its current value.
    private var actions: some View {
        HStack(spacing: metrics.wide ? 26 : 16) {
            menu("Audio", "volume-2", audioValue) {
                if model.audioTracks.isEmpty {
                    Choice(text: "As the file has it", selected: true)
                }
                ForEach(Array(model.audioTracks.enumerated()), id: \.offset) { index, name in
                    Button { model.selectAudio(index) } label: {
                        Choice(text: name, selected: model.audioTrack == index)
                    }
                }
            }
            menu("Subtitles", "captions", subtitleValue) {
                Button { model.selectSubtitle(nil) } label: {
                    Choice(text: "Off", selected: model.subtitle == nil)
                }
                ForEach(model.playback?.subtitles ?? []) { track in
                    Button { model.selectSubtitle(track) } label: {
                        Choice(text: subtitleName(track), selected: model.subtitle == track)
                    }
                }
            }
            if let versions, let snapshot = versions.snapshot {
                menu("Versions", "layers", currentQuality(snapshot)) {
                    ForEach([snapshot.recommended].compactMap { $0 } + snapshot.rows.prefix(12)) { row in
                        Button { model.switchVersion(row.key) } label: {
                            Choice(text: "\(VersionWords.line(row)), \(row.via)", selected: row.key == playingKey)
                        }
                    }
                }
            }
            menu("Speed", "gauge", speedText(model.rate)) {
                ForEach([0.5, 0.75, 1, 1.25, 1.5, 2] as [Float], id: \.self) { speed in
                    Button { model.rate = speed } label: {
                        Choice(text: speedText(speed), selected: model.rate == speed)
                    }
                }
            }
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .fixedSize()
    }

    /// An OSD menu that keeps the controls up while it is open, so they never
    /// hide (and close it) under the viewer's finger.
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
            Text(value).font(.text(Theme.type.caption, relativeTo: .caption)).foregroundStyle(.white.opacity(0.75))
        }
        .foregroundStyle(.white)
        .accessibilityElement(children: .combine)
    }

    /// The track playing, by name; "Default" when the file names none.
    private var audioValue: String {
        model.audioTrack.flatMap { model.audioTracks.indices.contains($0) ? model.audioTracks[$0] : nil } ?? "Default"
    }

    private var subtitleValue: String {
        let found = model.playback?.subtitles.count ?? 0
        if let track = model.subtitle { return subtitleName(track) }
        if model.playback?.subtitlesPending == true && found == 0 { return "Looking" }
        return found == 0 ? "None found" : "Off, \(found) found"
    }

    private func subtitleName(_ track: SubtitleTrack) -> String {
        let language = Locale.current.localizedString(forLanguageCode: track.lang) ?? track.lang
        return [track.label ?? language, track.hearingImpaired ? "SDH" : nil, track.forced ? "Forced" : nil]
            .compactMap { $0 }.joined(separator: ", ")
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
    let selected: Bool

    var body: some View {
        if selected { Label(text, image: "check") } else { Text(text) }
    }
}
