import SwiftUI

/// The OSD: title top left, end time top right, the scrubber, transport in
/// the middle and the track and version actions on the right, each a word
/// with its current value.
struct PlayerControls: View {
    let model: PlayerModel
    let versions: VersionsModel?
    let back: () -> Void
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
            VStack {
                LinearGradient(colors: [.black.opacity(0.7), .clear], startPoint: .top, endPoint: .bottom).frame(height: 160)
                Spacer()
                LinearGradient(colors: [.clear, .black.opacity(0.8)], startPoint: .top, endPoint: .bottom).frame(height: 240)
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
                Image(systemName: "chevron.left")
                    .font(.system(size: 18, weight: .semibold))
                    .frame(width: 40, height: 40)
                    .background(Circle().fill(.black.opacity(0.35)))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Back")
            VStack(alignment: .leading, spacing: 2) {
                Text(target.title)
                    .font(.display(metrics.wide ? Theme.type.title1 : Theme.type.title2))
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
            Button { model.skip(-10) } label: {
                Image(systemName: "gobackward.10").font(.system(size: 24))
            }
            .accessibilityLabel("Back 10 seconds")
            Button(action: model.togglePlay) {
                Image(systemName: model.playing ? "pause.fill" : "play.fill")
                    .font(.system(size: 28, weight: .semibold))
                    .foregroundStyle(Theme.Palette.onAccent)
                    .frame(width: 68, height: 68)
                    .background(Circle().fill(Color(white: 0.96)))
                    .overlay(Circle().strokeBorder(Theme.Palette.accent, lineWidth: 3).padding(-5))
            }
            .accessibilityLabel(model.playing ? "Pause" : "Play")
            Button { model.skip(10) } label: {
                Image(systemName: "goforward.10").font(.system(size: 24))
            }
            .accessibilityLabel("Forward 10 seconds")
        }
        .buttonStyle(.plain)
    }

    private var actions: some View {
        HStack(spacing: metrics.wide ? 26 : 16) {
            Menu {
                Button { model.selectSubtitle(nil) } label: {
                    Choice(text: "Off", selected: model.subtitle == nil)
                }
                ForEach(model.playback?.subtitles ?? []) { track in
                    Button { model.selectSubtitle(track) } label: {
                        Choice(text: subtitleName(track), selected: model.subtitle == track)
                    }
                }
            } label: {
                action("Subtitles", "captions.bubble", subtitleValue)
            }
            if let versions, let snapshot = versions.snapshot {
                Menu {
                    ForEach([snapshot.recommended].compactMap { $0 } + snapshot.rows.prefix(12)) { row in
                        Button { model.switchVersion(row.key) } label: {
                            Choice(text: "\(VersionWords.line(row)), \(row.via)", selected: row.key == playingKey)
                        }
                    }
                } label: {
                    action("Versions", "square.stack.3d.up", currentQuality(snapshot))
                }
            }
            Menu {
                ForEach([0.5, 0.75, 1, 1.25, 1.5, 2] as [Float], id: \.self) { speed in
                    Button { model.rate = speed } label: {
                        Choice(text: speedText(speed), selected: model.rate == speed)
                    }
                }
            } label: {
                action("Speed", "speedometer", speedText(model.rate))
            }
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .fixedSize()
    }

    private func action(_ title: String, _ symbol: String, _ value: String) -> some View {
        VStack(spacing: 3) {
            Image(systemName: symbol).font(.system(size: 18))
            Text(title).font(.text(Theme.type.caption + 1, weight: .semibold, relativeTo: .caption))
            Text(value).font(.text(Theme.type.caption, relativeTo: .caption)).foregroundStyle(.white.opacity(0.75))
        }
        .foregroundStyle(.white)
        .accessibilityElement(children: .combine)
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
        return row.quality == "Unknown" ? "Quality unknown" : row.quality
    }

    private func speedText(_ speed: Float) -> String {
        speed == speed.rounded() ? "\(Int(speed))x" : "\(speed)x"
    }

    private func timecode(_ seconds: Double) -> String {
        clock(ms: Int64(max(0, seconds.isFinite ? seconds : 0) * 1000))
    }
}

/// A menu entry that shows a checkmark when it is the current choice.
private struct Choice: View {
    let text: String
    let selected: Bool

    var body: some View {
        if selected { Label(text, systemImage: "checkmark") } else { Text(text) }
    }
}
