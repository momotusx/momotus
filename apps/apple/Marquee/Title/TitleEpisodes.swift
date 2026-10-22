import SwiftUI

// iOS and macOS only: tvOS keeps its own title page (D9).
#if !os(tvOS)

/// What an episode card or panel row can do, besides playing.
struct EpisodeActionsSet {
    let play: (Episode) -> Void
    let chooseVersion: (Episode) -> Void
    let setWatched: (String, Bool) -> Void
    /// Opens the panel (Mac, iPad) or the Episodes page (iPhone) on the season, from the season menu or a card.
    let allEpisodes: (PanelOpener) -> Void
    /// Where keyboard focus goes back when the panel closes.
    let focus: FocusState<PanelOpener?>.Binding
    /// The episode Resume plays from its resume point, whose menu also starts it over.
    var resumable: String?
    var startOver: () -> Void = {}
}

/// The v3 episode card (plans/design-v3-phase3-apple.md 2.1, 2.2): the still with its words on a
/// legibility ramp, the progress line on its foot. A click plays it; "..." (shown on hover or keyboard
/// focus on the Mac, always on the iPhone), the context menu and the accessibility actions carry
/// Choose version, the watched mark and All episodes.
struct EpisodeCard: View {
    let episode: Episode
    let minutes: Int?
    let actions: EpisodeActionsSet
    @Environment(\.metrics) private var metrics
    @State private var hovered = false
    @FocusState private var menuFocused: Bool

    private var size: CGSize { metrics.wide ? CGSize(width: 256, height: 197) : CGSize(width: 239, height: 226) }
    private var radius: CGFloat { metrics.wide ? Theme.TileDesktop.radius : Theme.Tile.cardRadius }
    private var rampHeight: CGFloat { metrics.wide ? 141 : 170 }
    private var unaired: Bool { airsLater(episode.released) }

    var body: some View {
        ZStack(alignment: metrics.wide ? .topTrailing : .bottomTrailing) {
            Button { actions.play(episode) } label: { card }
                .buttonStyle(CardButtonStyle(radius: radius, scale: Theme.Focus.scaleTile))
                .disabled(unaired)
                .accessibilityElement(children: .combine)
                .accessibilityLabel("Episode \(episode.episode), \(name)")
                .accessibilityValue(cardCaption(episode, minutes: minutes))
                .accessibilityHint(unaired ? "" : "Plays this episode")
                .accessibilityActions { menuItems }
                .contextMenu { menuItems }
                .focused(actions.focus, equals: .card(episode.videoId))
            Menu { menuItems } label: {
                Glyph("ellipsis", size: 20)
                    .foregroundStyle(.white)
                    // The Mac's shows over the still's top, past the ramp (the glass check's card spots).
                    .background { if metrics.wide { Circle().fill(.black.opacity(BackdropStops.cardMoreBacking)).frame(width: 30, height: 30) } }
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .menuStyle(.button)
            .buttonStyle(.plain)
            .menuIndicator(.hidden)
            .fixedSize()
            .focused($menuFocused)
            .focused(actions.focus, equals: .more(episode.videoId))
            .opacity(metrics.wide && !hovered && !menuFocused ? 0 : 1)
            .accessibilityHidden(true)
            .padding(2)
        }
        .frame(width: size.width, height: size.height)
        .onHover { hovered = $0 }
        .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
    }

    private var name: String { episode.title.isEmpty ? "Episode \(episode.episode)" : episode.title }

    private var card: some View {
        ArtImage(url: episode.thumbnail, maxPixels: 640, fallback: nil)
            .frame(width: size.width, height: size.height)
            .overlay(alignment: .bottom) {
                BackdropStops.cardRampGradient
                    .frame(height: rampHeight)
                    .allowsHitTesting(false)
            }
            .overlay(alignment: .bottomLeading) { words }
            .overlay(alignment: .bottom) {
                if let progress = episode.progress, !episode.watched, progress > 0 {
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            Rectangle().fill(ProgressLine.track)
                            Rectangle().fill(ProgressLine.fill).frame(width: geo.size.width * min(max(progress, 0), 1))
                        }
                    }
                    .frame(height: 4)
                }
            }
            .cardArt(radius: radius)
    }

    private var words: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("EPISODE \(episode.episode)")
                .textFont(11, weight: metrics.wide ? .medium : .semibold, relativeTo: .caption2)
                .foregroundStyle(Theme.Palette.muted)
            Text(name)
                .textFont(15, weight: .semibold, relativeTo: .subheadline)
                .foregroundStyle(.white)
                .lineLimit(1)
            if let overview = episode.overview, !overview.isEmpty {
                Text(overview)
                    .textFont(13, weight: metrics.wide ? .medium : .regular, relativeTo: .footnote)
                    .foregroundStyle(Theme.Palette.muted)
                    .lineLimit(metrics.wide ? 2 : 3)
                    .padding(.trailing, metrics.wide ? 0 : 40)
            }
            Text(cardCaption(episode, minutes: minutes))
                .textFont(12, weight: .semibold, relativeTo: .caption)
                .foregroundStyle(Theme.Palette.muted)
                .monospacedDigit()
                .padding(.top, 2)
        }
        .multilineTextAlignment(.leading)
        .padding(12)
    }

    @ViewBuilder private var menuItems: some View {
        if episode.videoId == actions.resumable {
            Button("Start Over", glyph: "rotate-ccw") { actions.startOver() }
        }
        if !unaired {
            Button("Choose Version", glyph: "layers") { actions.chooseVersion(episode) }
            Button(episode.watched ? "Mark as Not Watched" : "Mark as Watched", glyph: episode.watched ? "eye-off" : "eye") {
                actions.setWatched(episode.videoId, !episode.watched)
            }
        }
        Button("All Episodes", glyph: "list") { actions.allEpisodes(.card(episode.videoId, unaired: unaired)) }
    }
}

/// The season shelf: "Season 6" with its menu (the seasons, then All episodes), then the cards.
struct SeasonShelf: View {
    let seasons: [Int]
    let selected: Int?
    let episodes: [Episode]
    let minutes: Int?
    let select: (Int) -> Void
    let actions: EpisodeActionsSet
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(alignment: .leading, spacing: 13) {
            Menu {
                ForEach(ordered, id: \.self) { season in
                    Button { select(season) } label: {
                        if season == selected { Label(seasonName(season), glyph: "check") } else { Text(seasonName(season)) }
                    }
                }
                Divider()
                Button("All Episodes", glyph: "list") { actions.allEpisodes(.season) }
            } label: {
                HStack(spacing: 4) {
                    Text(seasonName(selected ?? ordered.first ?? 1))
                        .display(metrics.wide ? 17 : 20)
                        .foregroundStyle(Theme.Palette.text)
                    // The desktop frame's key-blue chevron; the iPhone's pull-down mark.
                    Glyph(metrics.wide ? "chevron-down" : "chevrons-up-down", size: metrics.wide ? 14 : 15)
                        .foregroundStyle(metrics.wide ? Theme.Palette.key : Theme.Palette.text)
                }
                .contentShape(Rectangle())
            }
            .menuStyle(.button)
            .buttonStyle(.plain)
            .menuIndicator(.hidden)
            .fixedSize()
            .focused(actions.focus, equals: .season)
            .accessibilityLabel("Season, \(seasonName(selected ?? 1))")
            .accessibilityHint("Chooses a season, or shows all episodes")
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: metrics.wide ? Theme.TileDesktop.gap : Theme.Tile.gap) {
                    ForEach(episodes, id: \.videoId) { episode in
                        EpisodeCard(episode: episode, minutes: minutes, actions: actions)
                    }
                }
                .padding(.vertical, 8)
            }
            .scrollIndicators(.hidden)
            .scrollClipDisabled()
            .padding(.vertical, -8)
        }
    }

    /// Regular seasons in order, specials last.
    private var ordered: [Int] { seasons.filter { $0 > 0 } + seasons.filter { $0 == 0 } }
}

func seasonName(_ season: Int) -> String { season == 0 ? "Specials" : "Season \(season)" }

/// The cast as grey monogram circles with their names (no roles: the core sends names only, C39).
struct CastShelf: View {
    let cast: [String]
    @Environment(\.metrics) private var metrics

    var body: some View {
        let side: CGFloat = metrics.wide ? 118 : 99
        VStack(alignment: .leading, spacing: 13) {
            Text("Cast")
                .display(metrics.wide ? 17 : 20)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: 20) {
                    ForEach(cast, id: \.self) { name in
                        VStack(spacing: 8) {
                            Text(initials(name))
                                .display(metrics.wide ? 32 : 34)
                                .foregroundStyle(Theme.Palette.text)
                                .frame(width: side, height: side)
                                .background(Circle().fill(Theme.Palette.raised))
                                .accessibilityHidden(true)
                            Text(name)
                                .textFont(12, relativeTo: .caption)
                                .foregroundStyle(Theme.Palette.text)
                                .multilineTextAlignment(.center)
                                .lineLimit(2)
                                .frame(width: side)
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            }
            .scrollIndicators(.hidden)
        }
    }

    private func initials(_ name: String) -> String {
        name.split(separator: " ").prefix(2).compactMap(\.first).map(String.init).joined()
    }
}

/// The Mac's #2B2B2B band: About (the genres line and four lines of synopsis) beside Information.
struct InfoBand: View {
    let synopsis: String?
    let genres: [String]
    let rows: [(key: String, value: String)]
    @Environment(\.metrics) private var metrics

    var body: some View {
        FlowRow(spacing: 40) {
            if let synopsis {
                VStack(alignment: .leading, spacing: 0) {
                    Text("About").textFont(17, weight: .bold, relativeTo: .headline).foregroundStyle(Theme.Palette.text)
                    if !genres.isEmpty {
                        Text(genres.prefix(3).joined(separator: " \u{B7} ").uppercased())
                            .textFont(11, weight: .semibold, relativeTo: .caption2)
                            .foregroundStyle(Theme.Palette.dim)
                            .padding(.top, 2)
                    }
                    Text(synopsis)
                        .textFont(13, relativeTo: .footnote)
                        .foregroundStyle(Theme.Palette.muted)
                        .lineLimit(4)
                        .padding(.top, 8)
                }
                .padding(EdgeInsets(top: 14, leading: 16, bottom: 16, trailing: 16))
                .frame(width: 440, alignment: .leading)
                .background(.white.opacity(0.1), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            VStack(alignment: .leading, spacing: 12) {
                Text("Information").textFont(17, weight: .bold, relativeTo: .headline).foregroundStyle(Theme.Palette.text)
                LazyVGrid(columns: [GridItem(.fixed(200), spacing: 40, alignment: .topLeading), GridItem(.fixed(200), alignment: .topLeading)], alignment: .leading, spacing: 14) {
                    ForEach(rows, id: \.key) { row in
                        VStack(alignment: .leading, spacing: 0) {
                            Text(row.key).textFont(12, relativeTo: .caption).foregroundStyle(Theme.Palette.text)
                            Text(row.value).textFont(12, relativeTo: .caption).foregroundStyle(Theme.Palette.dim)
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
            }
        }
        .padding(.vertical, 24)
        .padding(.horizontal, metrics.gutter)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(red: 0x2B / 255, green: 0x2B / 255, blue: 0x2B / 255))
    }
}

/// The iPhone's Information: Genres and Directors as linked chips on the page, then the rows.
struct InfoList: View {
    let genres: [String]
    let directors: [String]
    let rows: [(key: String, value: String)]
    let open: (TitleLink) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Information")
                .textFont(22, weight: .bold, relativeTo: .title2)
                .foregroundStyle(Theme.Palette.text)
                .accessibilityAddTraits(.isHeader)
            chipRow("Genres", genres, TitleLink.genre)
            chipRow("Directors", directors, TitleLink.director)
            ForEach(rows.filter { $0.key != "Genres" }, id: \.key) { row in
                VStack(alignment: .leading, spacing: 2) {
                    Text(row.key).textFont(12, relativeTo: .caption).foregroundStyle(Theme.Palette.text)
                    Text(row.value).textFont(12, relativeTo: .caption).foregroundStyle(Theme.Palette.dim)
                }
                .frame(minHeight: 49, alignment: .topLeading)
                .accessibilityElement(children: .combine)
            }
        }
    }

    /// Chips on the page (not over art): 32 tall in a 44 target, 15/500, on the inner surface fill.
    @ViewBuilder private func chipRow(_ label: String, _ names: [String], _ link: @escaping (String) -> TitleLink) -> some View {
        if !names.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Text(label).textFont(12, relativeTo: .caption).foregroundStyle(Theme.Palette.text)
                FlowRow(spacing: 8) {
                    ForEach(names, id: \.self) { name in
                        Button { open(link(name)) } label: {
                            Text(name)
                                .textFont(15, weight: .medium, relativeTo: .subheadline)
                                .foregroundStyle(Theme.Palette.text)
                                .padding(.horizontal, 12)
                                .frame(height: 32)
                                .background(Theme.Palette.raised, in: Capsule())
                                .padding(.vertical, 6)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(FocusPlainStyle(shape: Capsule(), scale: 1))
                    }
                }
            }
        }
    }
}
#endif
