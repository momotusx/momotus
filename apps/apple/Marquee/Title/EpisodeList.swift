import SwiftUI

/// Episodes as glass cards: a still with the number and a Watched chip, the
/// title, "57 min · Feb 18, 2022" (or the time left) and two lines of
/// synopsis. Selecting an episode plays it; choosing a version and the
/// watched mark sit beside it, and in its long press or right click menu.
/// An episode still to air is shown but cannot be selected.
#if os(tvOS)
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
    /// The failure's way forward: an account, a source or a version.
    let proceed: (EpisodeFailure) -> Void
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
                    let unaired = airsLater(episode.released)
                    HStack(spacing: metrics.wide ? 14 : 8) {
                        EpisodeRow(
                            episode: episode,
                            current: episode.videoId == current,
                            minutes: minutes(runtime),
                            status: status(episode)
                        ) {
                            play(episode)
                        }
                        // "Airs Oct 21": nothing to play yet.
                        .disabled(unaired)
                        .contextMenu {
                            Button("Choose a version", glyph: "layers") { chooseVersion(episode) }
                            Button(episode.watched ? "Mark as not watched" : "Mark as watched",
                                   glyph: episode.watched ? "eye-off" : "check") {
                                setWatched(episode.videoId, !episode.watched)
                            }
                        }
                        // The same two actions beside the row, as on the web.
                        if !unaired {
                            EpisodeActions(watched: episode.watched, chooseVersion: { chooseVersion(episode) }) {
                                setWatched(episode.videoId, !episode.watched)
                            }
                        }
                    }
                    // A reason is never the end: the way forward sits under it.
                    if let failure, failure.videoId == episode.videoId {
                        Button { proceed(failure) } label: { Label(failure.way.words, glyph: failure.way.glyph) }
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

#endif

#if os(tvOS)
/// "SEASON (1) [2] (3) Specials": pills with a control edge; the selected
/// season is a lighter fill with a bright edge (the ring is only ever focus).
private struct SeasonPicker: View {
    let seasons: [Int]
    let selected: Int?
    let select: (Int) -> Void

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                Text(caps: "Season")
                    .textFont(Theme.type.caption, weight: .bold, relativeTo: .caption)
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
            .textFont(Theme.type.callout, weight: selected ? .bold : .semibold, relativeTo: .callout)
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
    @KeyboardFocused private var focused
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

/// Choose a version and the watched mark: icons over their words on a wide
/// screen, icons alone on a phone, where the row leaves little width (the
/// words are still spoken).
private struct EpisodeActions: View {
    let watched: Bool
    let chooseVersion: () -> Void
    let toggleWatched: () -> Void
    @Environment(\.metrics) private var metrics

    var body: some View {
        if metrics.wide {
            HStack(spacing: Theme.Space.s) {
                Button(action: chooseVersion) { Label("Choose a version", glyph: "layers") }
                    .buttonStyle(RoundButtonStyle())
                    .frame(maxWidth: 104)
                watchedToggle { Label(watched ? "Watched" : "Mark as watched", glyph: "check") }
                    .buttonStyle(RoundButtonStyle(on: watched))
                    .frame(maxWidth: 104)
            }
        } else {
            VStack(spacing: 8) {
                Button(action: chooseVersion) { circle("layers", on: false) }
                    .buttonStyle(.round)
                    .accessibilityLabel("Choose a version")
                watchedToggle { circle("check", on: watched) }
                    .buttonStyle(.round)
            }
        }
    }

    /// A state, spoken as one: "Watched, toggle, on".
    private func watchedToggle<Words: View>(@ViewBuilder label: () -> Words) -> some View {
        Button(action: toggleWatched, label: label)
            .accessibilityLabel("Watched")
            .accessibilityAddTraits(.isToggle)
            .accessibilityValue(watched ? "On" : "Off")
    }

    private func circle(_ glyph: String, on: Bool) -> some View {
        Glyph(glyph)
            .foregroundStyle(.white)
            .frame(width: 44, height: 44)
            .background(SecondaryFill(shape: Circle(), on: on))
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
                        size: metrics.cardName + 2, lines: 2)
                    if !meta.isEmpty {
                        Text(meta)
                            .textFont(metrics.cardInfo, weight: .medium, relativeTo: .caption)
                            .foregroundStyle(current && episode.progress != nil ? Theme.Palette.text : Theme.Palette.dim)
                            .monospacedDigit()
                    }
                    if let overview = episode.overview, !overview.isEmpty {
                        Text(overview)
                            .textFont(metrics.cardInfo + 1, relativeTo: .callout)
                            .foregroundStyle(Theme.Palette.muted)
                            .lineLimit(2)
                            .multilineTextAlignment(.leading)
                    }
                    if let status {
                        Label(status, glyph: status == "Finding a version" ? "clock" : "circle-alert", size: metrics.cardInfo + 2)
                            .textFont(metrics.cardInfo + 1, relativeTo: .callout)
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
        .accessibilityHint(airsLater(episode.released) ? "" : "Plays this episode")
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
                    .textFont(chip + 2, weight: .bold, relativeTo: .caption)
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
                        .textFont(chip, weight: .bold, relativeTo: .caption2)
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
                            Rectangle().fill(ProgressLine.track)
                            Rectangle().fill(ProgressLine.fill).frame(width: geo.size.width * min(max(progress, 0), 1))
                        }
                    }
                    .frame(height: 4)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: metrics.wide ? 10 : 8, style: .continuous))
            .accessibilityHidden(true)
    }

    /// "57 min · Feb 18, 2022", "36 min left · Feb 18, 2022" or "Airs Oct 21".
    private var meta: String { episodeCaption(episode, minutes: minutes) }
}

private struct ChipLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 3) {
            configuration.icon
            configuration.title
        }
    }
}
#endif
