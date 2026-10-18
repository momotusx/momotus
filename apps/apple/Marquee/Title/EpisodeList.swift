import SwiftUI

/// Episodes as glass cards: a still with the number and a Watched chip, the
/// title, "57 min · Feb 18, 2022" (or the time left) and two lines of
/// synopsis. Selecting an episode plays it; choosing a version and the
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
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text("Episodes")
                .display(metrics.wide ? Theme.type.title1 : Theme.type.title2)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            SeasonPicker(seasons: seasons, selected: selected, select: select)
            LazyVStack(spacing: metrics.wide ? 14 : 10) {
                ForEach(episodes, id: \.videoId) { episode in
                    EpisodeRow(
                        episode: episode,
                        current: episode.videoId == current,
                        minutes: minutes(runtime),
                        status: status(episode)
                    ) {
                        play(episode)
                    }
                    .contextMenu {
                        Button("Choose a version", glyph: "layers") { chooseVersion(episode) }
                        Button(episode.watched ? "Mark as unwatched" : "Mark as watched",
                               glyph: episode.watched ? "eye-off" : "check") {
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
                                Button(action: addSource) { Label("Add a source", glyph: "plus") }
                            } else {
                                Button { chooseVersion(episode) } label: { Label("Choose a version", glyph: "layers") }
                            }
                        }
                        .buttonStyle(PillButtonStyle(prominent: true))
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
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

/// "SEASON (1) [2] (3) Specials": pills with a control edge; the selected
/// season is a lighter fill with a bright edge (the ring is only ever focus).
private struct SeasonPicker: View {
    let seasons: [Int]
    let selected: Int?
    let select: (Int) -> Void

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                Text("SEASON")
                    .font(.text(Theme.type.caption, weight: .bold, relativeTo: .caption))
                    .tracking(Theme.type.caption * 0.12)
                    .foregroundStyle(Theme.Palette.muted)
                    .padding(.trailing, 6)
                ForEach(ordered, id: \.self) { season in
                    Button(season == 0 ? "Specials" : "\(season)") { select(season) }
                        .buttonStyle(SeasonButtonStyle(selected: season == selected, round: season != 0))
                        .accessibilityLabel(season == 0 ? "Specials" : "Season \(season)")
                        .accessibilityAddTraits(season == selected ? .isSelected : [])
                }
            }
            .padding(.vertical, 8)
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
            .font(.text(Theme.type.callout, weight: selected ? .bold : .semibold, relativeTo: .callout))
            .foregroundStyle(selected ? Theme.Palette.text : Theme.Palette.muted)
            .padding(.horizontal, round ? 0 : 14)
            .frame(width: round ? Self.size : nil, height: Self.size)
            .background { Capsule().fill(.white.opacity(selected ? 0.16 : 0)) }
            .overlay { Capsule().strokeBorder(selected ? .white.opacity(0.55) : Theme.Palette.control, lineWidth: edgeWidth) }
            .contentShape(Capsule())
            // A 44 pt target around the 36 pt pill.
            .padding(.vertical, 4)
            .contentShape(Rectangle())
            .modifier(SeasonFocus(pressed: configuration.isPressed))
    }

    #if os(tvOS)
    private static let size: CGFloat = 54
    #else
    private static let size: CGFloat = 36
    #endif
}

private struct SeasonFocus: ViewModifier {
    let pressed: Bool
    @Environment(\.isFocused) private var focused
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content
            .pressVeil(Capsule(), pressed: pressed)
            .modifier(FocusRing(shape: Capsule(), active: focused))
            .scaleEffect(reduceMotion ? 1 : pressed ? 0.95 : focused ? Theme.Focus.scaleCard : 1)
            .animation(Theme.Motion.focus, value: pressed)
            .animation(Theme.Motion.focus, value: focused)
    }
}

private struct EpisodeRow: View {
    let episode: Episode
    /// The play target: its card holds the time left.
    let current: Bool
    let minutes: Int?
    /// "Finding a version", or why it could not start.
    let status: String?
    let play: () -> Void
    @Environment(\.metrics) private var metrics

    var body: some View {
        Button(action: play) {
            HStack(alignment: .top, spacing: metrics.wide ? 18 : 12) {
                still
                VStack(alignment: .leading, spacing: metrics.wide ? 6 : 4) {
                    // Two lines on a phone, where the still leaves the title little width.
                    CardTitle(
                        text: episode.title.isEmpty ? "Episode \(episode.episode)" : episode.title,
                        size: metrics.cardName + 2, onGlass: true, lines: 2)
                    if !meta.isEmpty {
                        Text(meta)
                            .font(.text(metrics.cardInfo, weight: .medium, relativeTo: .caption))
                            .foregroundStyle(current && episode.progress != nil ? Theme.Palette.text : Theme.Palette.dim)
                            .monospacedDigit()
                    }
                    if let overview = episode.overview, !overview.isEmpty {
                        Text(overview)
                            .font(.text(metrics.cardInfo + 1, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.muted)
                            .lineLimit(2)
                            .multilineTextAlignment(.leading)
                    }
                    if let status {
                        Label(status, glyph: status == "Finding a version" ? "clock" : "circle-alert", size: metrics.cardInfo + 2)
                            .font(.text(metrics.cardInfo + 1, relativeTo: .callout))
                            .foregroundStyle(Theme.Palette.text)
                            .multilineTextAlignment(.leading)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .padding(.top, 2)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(metrics.wide ? 12 : 10)
            .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        }
        .buttonStyle(CardButtonStyle(radius: Theme.Radius.card, scale: Theme.Focus.scaleTile))
        .accessibilityElement(children: .combine)
        .accessibilityValue(episode.watched ? "Watched" : "")
        .accessibilityHint("Plays this episode")
    }

    /// The still with the number at its bottom left, a Watched chip at its
    /// top right and progress on its foot.
    private var still: some View {
        let width: CGFloat = metrics.wide ? (metrics.poster.width >= Theme.Poster.tv.width ? 224 : 168) : 128
        let chip = metrics.cardInfo - 1
        return ArtImage(url: episode.thumbnail, maxPixels: 640)
            .frame(width: width, height: width * 9 / 16)
            .overlay(alignment: .bottomLeading) {
                Text("\(episode.episode)")
                    .font(.text(chip + 2, weight: .heavy, relativeTo: .caption))
                    .monospacedDigit()
                    .foregroundStyle(Theme.Palette.text)
                    .padding(.horizontal, chip * 0.5)
                    .frame(minWidth: chip * 2, minHeight: chip * 2)
                    .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Theme.Palette.bg.opacity(0.72)))
                    .padding(6)
            }
            .overlay(alignment: .topTrailing) {
                if episode.watched {
                    Label("Watched", glyph: "check", size: chip)
                        .labelStyle(ChipLabelStyle())
                        .font(.text(chip, weight: .bold, relativeTo: .caption2))
                        .foregroundStyle(Theme.Palette.text)
                        .padding(.horizontal, chip * 0.55)
                        .frame(height: chip * 1.8)
                        .background(Capsule().fill(Theme.Palette.bg.opacity(0.72)))
                        .overlay(Capsule().strokeBorder(Theme.Palette.edge, lineWidth: 1))
                        .padding(5)
                }
            }
            .overlay(alignment: .bottom) {
                if let progress = episode.progress, !episode.watched {
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            Rectangle().fill(.white.opacity(0.25))
                            Rectangle().fill(Theme.Palette.accent).frame(width: geo.size.width * min(max(progress, 0), 1))
                        }
                    }
                    .frame(height: metrics.wide ? 4 : 3)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: metrics.wide ? 10 : 8, style: .continuous))
            .accessibilityHidden(true)
    }

    /// "57 min · Feb 18, 2022", "36 min left · Feb 18, 2022" or "Airs Oct 21".
    private var meta: String {
        let released = releaseDate(episode.released)
        if let released, released > .now {
            return "Airs \(released.formatted(Self.day.month(.abbreviated).day()))"
        }
        var parts: [String] = []
        if let left = episode.leftMs, left > 0, !episode.watched {
            parts.append(minutesLeft(ms: left))
        } else if let minutes {
            if let progress = episode.progress, !episode.watched {
                parts.append("\(max(1, Int((Double(minutes) * (1 - progress)).rounded()))) min left")
            } else {
                parts.append("\(minutes) min")
            }
        }
        if let released { parts.append(released.formatted(Self.day.month(.abbreviated).day().year())) }
        return parts.joined(separator: " \u{B7} ")
    }

    /// Release dates are days, kept in UTC so they never slip a day.
    private static let day = Date.FormatStyle(timeZone: TimeZone(identifier: "UTC") ?? .current)
}

private struct ChipLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 3) {
            configuration.icon
            configuration.title
        }
    }
}
