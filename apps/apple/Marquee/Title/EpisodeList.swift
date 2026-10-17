import SwiftUI

/// Episodes as a table of contents (Folio): number, title, a dotted leader,
/// runtime or time left; the current episode shows its synopsis and progress.
/// Selecting an episode plays it; choosing a version, the synopsis and the
/// watched mark are the secondary actions (long press, right click, or a
/// long press of select on tvOS).
struct EpisodeList: View {
    let seasons: [Int]
    let selected: Int?
    let episodes: [Episode]
    let current: String?
    let runtime: String?
    /// The episode whose version is being found.
    let starting: String?
    let failure: EpisodeFailure?
    let select: (Int) -> Void
    let play: (Episode) -> Void
    let chooseVersion: (Episode) -> Void
    let addSource: () -> Void
    let setWatched: (String, Bool) -> Void

    @State private var opened: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text("Episodes")
                .font(.display(Theme.type.title1))
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            SeasonPicker(seasons: seasons, selected: selected, select: select)
            Rectangle().fill(Theme.Palette.line).frame(height: 1)
            LazyVStack(spacing: 0) {
                ForEach(episodes, id: \.videoId) { episode in
                    let expanded = episode.videoId == current || opened.contains(episode.videoId)
                    EpisodeRow(
                        episode: episode,
                        expanded: expanded,
                        minutes: minutes(runtime),
                        status: status(episode)
                    ) {
                        play(episode)
                    }
                    .contextMenu {
                        Button("Choose a version", systemImage: "square.stack.3d.up") { chooseVersion(episode) }
                        if episode.overview?.isEmpty == false, episode.videoId != current {
                            Button(expanded ? "Hide synopsis" : "Show synopsis", systemImage: "text.alignleft") {
                                opened.formSymmetricDifference([episode.videoId])
                            }
                        }
                        Button(episode.watched ? "Mark as unwatched" : "Mark as watched",
                               systemImage: episode.watched ? "eye.slash" : "checkmark") {
                            setWatched(episode.videoId, !episode.watched)
                        }
                    }
                    .accessibilityAction(named: "Choose a version") { chooseVersion(episode) }
                    .accessibilityAction(named: episode.watched ? "Mark as unwatched" : "Mark as watched") {
                        setWatched(episode.videoId, !episode.watched)
                    }
                    // A reason is never the end: the way forward sits under it.
                    if let failure, failure.videoId == episode.videoId {
                        Group {
                            if failure.noVersion {
                                Button(action: addSource) { Label("Add a source", systemImage: "plus") }
                            } else {
                                Button { chooseVersion(episode) } label: { Label("Choose a version", systemImage: "square.stack.3d.up") }
                            }
                        }
                        .buttonStyle(PillButtonStyle(prominent: true))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.leading, 56)
                        .padding(.bottom, 12)
                    }
                    Rectangle().fill(Theme.Palette.line.opacity(0.6)).frame(height: 1)
                }
            }
        }
    }

    private func status(_ episode: Episode) -> String? {
        if starting == episode.videoId { return "Finding a version" }
        return failure?.videoId == episode.videoId ? failure?.text : nil
    }
}

/// Why an episode from the list could not start, and whether any version exists at all.
struct EpisodeFailure: Equatable {
    let videoId: String
    let text: String
    /// Every addon answered with nothing playable: the way forward is a source.
    let noVersion: Bool
}

/// "SEASON 1 2 [3] 4 Specials": the selected season sits on a raised fill.
private struct SeasonPicker: View {
    let seasons: [Int]
    let selected: Int?
    let select: (Int) -> Void

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 6) {
                Text("SEASON")
                    .font(.text(Theme.type.caption, weight: .semibold, relativeTo: .caption))
                    .tracking(1.6)
                    .foregroundStyle(Theme.Palette.muted)
                    .padding(.trailing, 8)
                ForEach(ordered, id: \.self) { season in
                    Button(season == 0 ? "Specials" : "\(season)") { select(season) }
                        .buttonStyle(SeasonButtonStyle(selected: season == selected, round: season != 0))
                        .accessibilityLabel(season == 0 ? "Specials" : "Season \(season)")
                        .accessibilityAddTraits(season == selected ? .isSelected : [])
                }
            }
            .padding(.vertical, 6)
        }
        .scrollIndicators(.hidden)
        .scrollClipDisabled()
    }

    /// Regular seasons in order, specials last.
    private var ordered: [Int] { seasons.filter { $0 > 0 } + seasons.filter { $0 == 0 } }
}

private struct SeasonButtonStyle: ButtonStyle {
    let selected: Bool
    /// Numbers sit in a circle; "Specials" in a capsule.
    let round: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.text(Theme.type.headline, weight: selected ? .semibold : .medium, relativeTo: .headline))
            .foregroundStyle(selected ? Theme.Palette.text : Theme.Palette.muted)
            .padding(.horizontal, round ? 0 : 12)
            .frame(width: round ? 38 : nil, height: 38)
            // Selection is a fill; the ring is only ever focus.
            .background {
                if selected { Capsule().fill(Theme.Palette.raised) }
            }
            .contentShape(Capsule())
            .modifier(SeasonFocus(pressed: configuration.isPressed))
    }
}

private struct SeasonFocus: ViewModifier {
    let pressed: Bool
    @Environment(\.isFocused) private var focused

    func body(content: Content) -> some View {
        content
            .modifier(FocusRing(shape: Capsule(), active: focused))
            .scaleEffect(pressed ? 0.95 : 1)
            .animation(Theme.Motion.focus, value: pressed)
    }
}

private struct EpisodeRow: View {
    let episode: Episode
    let expanded: Bool
    let minutes: Int?
    /// "Finding a version", or why it could not start.
    let status: String?
    let play: () -> Void

    var body: some View {
        Button(action: play) {
            HStack(alignment: .firstTextBaseline, spacing: 18) {
                Text("\(episode.episode)")
                    .font(.display(Theme.type.title2))
                    .foregroundStyle(Theme.Palette.muted)
                    .frame(minWidth: 30, alignment: .leading)
                    .monospacedDigit()
                VStack(alignment: .leading, spacing: 8) {
                    HStack(alignment: .firstTextBaseline, spacing: 10) {
                        Text(episode.title.isEmpty ? "Episode \(episode.episode)" : episode.title)
                            .font(.text(Theme.type.body, weight: .medium))
                            .foregroundStyle(Theme.Palette.text)
                            .lineLimit(1)
                            .layoutPriority(1)
                        DottedLeader()
                        trailing
                    }
                    if expanded, let overview = episode.overview, !overview.isEmpty {
                        Text(overview)
                            .font(.text(Theme.type.callout, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.muted)
                            .multilineTextAlignment(.leading)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    if let progress = episode.progress, !episode.watched {
                        ProgressLine(value: progress)
                    }
                    if let status {
                        Label(status, systemImage: status == "Finding a version" ? "clock" : "exclamationmark.circle")
                            .font(.text(Theme.type.callout, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.muted)
                            .multilineTextAlignment(.leading)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            .padding(.vertical, 12)
            .padding(.horizontal, 8)
            .contentShape(Rectangle())
        }
        .buttonStyle(RowButtonStyle())
        .accessibilityElement(children: .combine)
        .accessibilityValue(episode.watched ? "Watched" : "")
        .accessibilityHint("Plays this episode")
    }

    @ViewBuilder private var trailing: some View {
        HStack(spacing: 6) {
            if episode.watched {
                Image(systemName: "checkmark").accessibilityHidden(true)
            }
            Text(trailingText)
        }
        .font(.text(Theme.type.callout, relativeTo: .callout))
        .foregroundStyle(episode.progress != nil && !episode.watched ? Theme.Palette.text : Theme.Palette.muted)
        .monospacedDigit()
        .fixedSize()
    }

    private var trailingText: String {
        if let released = releaseDate(episode.released), released > .now {
            return "Airs \(released.formatted(.dateTime.month(.abbreviated).day()))"
        }
        guard let minutes else { return "" }
        if let progress = episode.progress, !episode.watched {
            return "\(max(1, Int((Double(minutes) * (1 - progress)).rounded()))) min left"
        }
        return "\(minutes) min"
    }
}

/// The dotted rule between an entry and its page number.
private struct DottedLeader: View {
    var body: some View {
        Line()
            .stroke(Theme.Palette.muted.opacity(0.55), style: StrokeStyle(lineWidth: 1.5, lineCap: .round, dash: [0.1, 6]))
            .frame(height: 1.5)
            .frame(minWidth: 12)
            .alignmentGuide(.firstTextBaseline) { $0[.bottom] }
            .accessibilityHidden(true)
    }

    private struct Line: Shape {
        func path(in rect: CGRect) -> Path {
            Path { $0.move(to: CGPoint(x: rect.minX, y: rect.midY)); $0.addLine(to: CGPoint(x: rect.maxX, y: rect.midY)) }
        }
    }
}

/// Rows highlight on hover, focus and press instead of scaling.
private struct RowButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        RowBody(configuration: configuration)
    }

    private struct RowBody: View {
        let configuration: ButtonStyleConfiguration
        @Environment(\.isFocused) private var focused
        @State private var hovered = false

        var body: some View {
            configuration.label
                .background(
                    RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous)
                        .fill(Theme.Palette.surface.opacity(configuration.isPressed ? 1 : hovered || focused ? 0.8 : 0)))
                .modifier(FocusRing(shape: RoundedRectangle(cornerRadius: Theme.Radius.s, style: .continuous), active: focused))
                .animation(Theme.Motion.focus, value: hovered)
                #if !os(tvOS)
                .onHover { hovered = $0 }
                #endif
        }
    }
}
